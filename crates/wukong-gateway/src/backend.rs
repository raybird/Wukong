use crate::error::GatewayError;
use std::process::Stdio;
use tokio::process::Command;

/// A request to the AI backend.
#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub prompt: String,
    pub continue_session: bool,
}

/// The backend's textual response.
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub text: String,
}

/// Pluggable AI backend. v1 ships `AgentCliBackend`.
#[allow(async_fn_in_trait)]
pub trait AiBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError>;
}

/// Build the argv handed to the agent subprocess:
/// `command + (continue_args if continue_session) + [prompt]`.
pub fn assemble_argv(
    command: &[String],
    continue_args: &[String],
    continue_session: bool,
    prompt: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = command.to_vec();
    if continue_session {
        argv.extend(continue_args.iter().cloned());
    }
    argv.push(prompt.to_string());
    argv
}

/// Drives a configurable agent CLI as a subprocess (run-and-capture, no shell).
pub struct AgentCliBackend {
    pub command: Vec<String>,
    pub continue_args: Vec<String>,
}

impl AiBackend for AgentCliBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        let argv = assemble_argv(
            &self.command,
            &self.continue_args,
            req.continue_session,
            &req.prompt,
        );
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_argv_without_continue() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            &["-c".to_string()],
            false,
            "hi",
        );
        assert_eq!(argv, vec!["opencode", "run", "hi"]);
    }

    #[test]
    fn assemble_argv_with_continue() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            &["-c".to_string()],
            true,
            "hi",
        );
        assert_eq!(argv, vec!["opencode", "run", "-c", "hi"]);
    }

    #[tokio::test]
    async fn agent_cli_backend_captures_stdout() {
        // `echo <prompt>` prints the prompt back; verifies capture + trim.
        let backend = AgentCliBackend {
            command: vec!["echo".to_string()],
            continue_args: vec![],
        };
        let resp = backend
            .run(AgentRequest {
                prompt: "hello wukong".to_string(),
                continue_session: false,
            })
            .await
            .unwrap();
        assert_eq!(resp.text, "hello wukong");
    }

    #[tokio::test]
    async fn agent_cli_backend_reports_failure() {
        // `false` exits non-zero with no output.
        let backend = AgentCliBackend {
            command: vec!["false".to_string()],
            continue_args: vec![],
        };
        let err = backend
            .run(AgentRequest {
                prompt: "x".to_string(),
                continue_session: false,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, GatewayError::AgentFailed { .. }));
    }
}
