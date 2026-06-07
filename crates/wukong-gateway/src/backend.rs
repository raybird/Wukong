use crate::error::GatewayError;
use crate::stream::{parse_event, parse_session_id, StreamEvent};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

/// A request to the AI backend.
#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub prompt: String,
    /// Some(id) → continue this opencode session via `-s <id>`; None → fresh.
    pub session_id: Option<String>,
    /// Pass `--thinking` to surface reasoning blocks.
    pub thinking: bool,
}

/// The backend's textual response.
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub text: String,
    /// opencode session id captured from the JSON stream (None on the plain path).
    pub session_id: Option<String>,
}

/// Pluggable AI backend. v1 ships `AgentCliBackend`.
#[allow(async_fn_in_trait)]
pub trait AiBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError>;

    /// Run, invoking `on_event` as events arrive, returning the full response.
    /// Default: call `run`, then emit the whole text as a single Text event.
    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        let resp = self.run(req).await?;
        on_event(StreamEvent::Text(resp.text.clone()));
        Ok(resp)
    }
}

/// Build the argv handed to the agent subprocess:
/// `command + [-s <id>]? + [--thinking]? + [prompt]`.
pub fn assemble_argv(
    command: &[String],
    session_id: Option<&str>,
    thinking: bool,
    prompt: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = command.to_vec();
    if let Some(id) = session_id {
        argv.push("-s".to_string());
        argv.push(id.to_string());
    }
    if thinking {
        argv.push("--thinking".to_string());
    }
    argv.push(prompt.to_string());
    argv
}

/// Drives a configurable agent CLI as a subprocess (run-and-capture, no shell).
pub struct AgentCliBackend {
    pub command: Vec<String>,
}

impl AiBackend for AgentCliBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        let argv = assemble_argv(&self.command, req.session_id.as_deref(), req.thinking, &req.prompt);
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .output()
            .await?;
        if !output.status.success() {
            return Err(GatewayError::AgentFailed {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(AgentResponse {
            text: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            session_id: None,
        })
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        // Build argv then insert `--format json` before the prompt (last arg).
        let mut argv = assemble_argv(&self.command, req.session_id.as_deref(), req.thinking, &req.prompt);
        let prompt = argv.pop().expect("argv always ends with the prompt");
        argv.push("--format".to_string());
        argv.push("json".to_string());
        argv.push(prompt);

        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Drain stderr concurrently so a large stderr can't deadlock us while
        // we read stdout line-by-line.
        let stderr = child.stderr.take().expect("stderr piped");
        let stderr_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = String::new();
            let mut rdr = stderr;
            let _ = rdr.read_to_string(&mut buf).await;
            buf
        });

        let stdout = child.stdout.take().expect("stdout piped");
        let mut lines = BufReader::new(stdout).lines();
        let mut full = String::new();
        let mut session_id: Option<String> = None;
        while let Some(line) = lines.next_line().await? {
            if let Some(id) = parse_session_id(&line) {
                session_id = Some(id);
            }
            if let Some(ev) = parse_event(&line) {
                if let StreamEvent::Text(t) = &ev {
                    if !full.is_empty() {
                        full.push('\n');
                    }
                    full.push_str(t);
                }
                on_event(ev);
            }
        }

        let status = child.wait().await?;
        let stderr_buf = stderr_task.await.unwrap_or_default();
        if !status.success() {
            return Err(GatewayError::AgentFailed {
                code: status.code(),
                stderr: stderr_buf.trim().to_string(),
            });
        }
        Ok(AgentResponse {
            text: full.trim().to_string(),
            session_id,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_argv_plain() {
        let argv = assemble_argv(&["opencode".to_string(), "run".to_string()], None, false, "hi");
        assert_eq!(argv, vec!["opencode", "run", "hi"]);
    }

    #[test]
    fn assemble_argv_with_session_and_thinking() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            Some("ses_x"),
            true,
            "hi",
        );
        assert_eq!(argv, vec!["opencode", "run", "-s", "ses_x", "--thinking", "hi"]);
    }

    #[tokio::test]
    async fn agent_cli_backend_captures_stdout() {
        // `echo <prompt>` prints the prompt back; verifies capture + trim.
        let backend = AgentCliBackend { command: vec!["echo".to_string()] };
        let resp = backend
            .run(AgentRequest {
                prompt: "hello wukong".to_string(),
                session_id: None,
                thinking: false,
            })
            .await
            .unwrap();
        assert_eq!(resp.text, "hello wukong");
    }

    #[tokio::test]
    async fn agent_cli_backend_reports_failure() {
        // `false` exits non-zero with no output.
        let backend = AgentCliBackend { command: vec!["false".to_string()] };
        let err = backend
            .run(AgentRequest {
                prompt: "x".to_string(),
                session_id: None,
                thinking: false,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, GatewayError::AgentFailed { .. }));
    }

    #[tokio::test]
    async fn run_streaming_default_emits_single_text() {
        // Test the DEFAULT run_streaming impl via a minimal mock backend.
        struct Plain;
        impl AiBackend for Plain {
            async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
                Ok(AgentResponse { text: "whole answer".to_string(), session_id: None })
            }
        }
        let mut events = Vec::new();
        let resp = Plain
            .run_streaming(
                AgentRequest { prompt: "x".into(), session_id: None, thinking: false },
                &mut |e| events.push(e),
            )
            .await
            .unwrap();
        assert_eq!(resp.text, "whole answer");
        assert_eq!(events, vec![StreamEvent::Text("whole answer".to_string())]);
    }

    #[tokio::test]
    async fn agent_cli_run_streaming_parses_ndjson() {
        // A fake "agent": `printf "%s\n" <lines...>` prints each extra arg on its
        // own line. argv tail (--format json <prompt>) becomes extra %s lines
        // (non-JSON) that parse_event ignores.
        let backend = AgentCliBackend {
            command: vec![
                "printf".to_string(),
                "%s\\n".to_string(),
                r#"{"type":"step_start","sessionID":"ses_T"}"#.to_string(),
                r#"{"type":"tool_use","part":{"type":"tool","tool":"read"}}"#.to_string(),
                r#"{"type":"text","part":{"type":"text","text":"hello"}}"#.to_string(),
                r#"{"type":"step_finish"}"#.to_string(),
            ],
        };
        let mut events = Vec::new();
        let resp = backend
            .run_streaming(
                AgentRequest { prompt: "ignored".into(), session_id: None, thinking: false },
                &mut |e| events.push(e),
            )
            .await
            .unwrap();
        assert_eq!(resp.text, "hello");
        assert_eq!(resp.session_id, Some("ses_T".to_string()));
        assert_eq!(
            events,
            vec![
                StreamEvent::StepStart,
                StreamEvent::ToolUse("read".to_string()),
                StreamEvent::Text("hello".to_string()),
                StreamEvent::StepFinish,
            ]
        );
    }
}
