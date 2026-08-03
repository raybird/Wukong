use crate::error::GatewayError;
use crate::stream::{parse_event, parse_session_id, StreamEvent};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, BufReader};
use tokio::process::Command;

const DSML_TOOL_CALLS_START: &str = "<｜｜DSML｜｜tool_calls>";
const DSML_TOOL_CALLS_END: &str = "</｜｜DSML｜｜tool_calls>";
const DEFAULT_AGENT_TIMEOUT_SECS: u64 = 20 * 60;

/// A request to the AI backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAttachment {
    pub path: PathBuf,
    pub original_name: String,
    pub mime_type: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub prompt: String,
    /// Some(id) → continue this opencode session via `-s <id>`; None → fresh.
    pub session_id: Option<String>,
    /// Pass `--thinking` to surface reasoning blocks.
    pub thinking: bool,
    /// Optional opencode model override for this request.
    pub model: Option<String>,
    /// Optional opencode agent override, e.g. `plan` for internal routing.
    pub agent: Option<String>,
    /// Per-tool opencode server overrides. `false` disables a tool for this request.
    pub tool_overrides: BTreeMap<String, bool>,
    pub attachments: Vec<AgentAttachment>,
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

    async fn delete_session(&self, _session_id: &str) -> Result<(), GatewayError> {
        Ok(())
    }

    async fn compact_session(
        &self,
        session_id: &str,
        model: Option<&str>,
    ) -> Result<AgentResponse, GatewayError> {
        self.run_streaming(
            AgentRequest {
                prompt: "/compact".to_string(),
                session_id: Some(session_id.to_string()),
                thinking: false,
                model: model.map(str::to_string),
                agent: None,
                tool_overrides: BTreeMap::new(),
                attachments: Vec::new(),
            },
            &mut |_| {},
        )
        .await
    }

    async fn run_ephemeral(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        let response = self.run(req).await?;
        if let Some(session_id) = response.session_id.as_deref() {
            match self.delete_session(session_id).await {
                Ok(()) => eprintln!("helper_session_deleted session_id={session_id}"),
                Err(error) => {
                    eprintln!("helper_session_delete_failed session_id={session_id}: {error}")
                }
            }
        }
        Ok(response)
    }

    async fn run_streaming_ephemeral(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        let response = self.run_streaming(req, on_event).await?;
        if let Some(session_id) = response.session_id.as_deref() {
            match self.delete_session(session_id).await {
                Ok(()) => eprintln!("helper_session_deleted session_id={session_id}"),
                Err(error) => {
                    eprintln!("helper_session_delete_failed session_id={session_id}: {error}")
                }
            }
        }
        Ok(response)
    }
}

/// Build the argv handed to the agent subprocess:
/// `command + [-s <id>]? + [--thinking]? + [--model <m>]? + [--agent <a>]? + [--file …]* + prompt`.
pub fn assemble_argv(
    command: &[String],
    session_id: Option<&str>,
    thinking: bool,
    model: Option<&str>,
    agent: Option<&str>,
    attachments: &[AgentAttachment],
    prompt: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = strip_model_args(command);
    if let Some(id) = session_id {
        argv.push("-s".to_string());
        argv.push(id.to_string());
    }
    if thinking {
        argv.push("--thinking".to_string());
    }
    if let Some(model) = model.map(str::trim).filter(|m| !m.is_empty()) {
        argv.push("--model".to_string());
        argv.push(model.to_string());
    }
    // Route planning/aux steps to a cheaper, tool-less opencode agent when the
    // orchestrator asks for one (e.g. `plan`). Without this the CLI backend
    // silently ran every step with the default agent.
    if let Some(agent) = agent.map(str::trim).filter(|a| !a.is_empty()) {
        argv.push("--agent".to_string());
        argv.push(agent.to_string());
    }
    for attachment in attachments {
        argv.push("--file".to_string());
        argv.push(attachment.path.to_string_lossy().to_string());
    }
    argv.push(prompt.to_string());
    argv
}

fn strip_model_args(command: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = command.iter();
    while let Some(arg) = iter.next() {
        if arg == "--model" || arg == "-m" {
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--model=") {
            continue;
        }
        out.push(arg.clone());
    }
    out
}

pub fn opencode_binary(command: &[String]) -> &str {
    command.first().map(String::as_str).unwrap_or("opencode")
}

/// Drives a configurable agent CLI as a subprocess (run-and-capture, no shell).
pub struct AgentCliBackend {
    pub command: Vec<String>,
    /// Working directory for the agent subprocess. Defaults to the process CWD
    /// when None; set to `~/.wukong/workspace` to isolate agent side-effects.
    pub workspace: Option<PathBuf>,
}

pub enum AgentBackend {
    Cli(AgentCliBackend),
    Server(crate::opencode_server::OpencodeServerBackend),
}

pub fn build_backend_from_env(command: Vec<String>, workspace: Option<PathBuf>) -> AgentBackend {
    match std::env::var("WUKONG_AGENT_SERVER_URL") {
        Ok(url) if !url.trim().is_empty() => AgentBackend::Server(
            crate::opencode_server::OpencodeServerBackend::from_env(url, workspace),
        ),
        _ => AgentBackend::Cli(AgentCliBackend { command, workspace }),
    }
}

impl AgentBackend {
    pub async fn check_ready(&self) -> Result<(), GatewayError> {
        match self {
            AgentBackend::Cli(_) => Ok(()),
            AgentBackend::Server(backend) => backend.health_check().await,
        }
    }
}

impl AiBackend for AgentBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        match self {
            AgentBackend::Cli(backend) => backend.run(req).await,
            AgentBackend::Server(backend) => backend.run(req).await,
        }
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        match self {
            AgentBackend::Cli(backend) => backend.run_streaming(req, on_event).await,
            AgentBackend::Server(backend) => backend.run_streaming(req, on_event).await,
        }
    }

    async fn delete_session(&self, session_id: &str) -> Result<(), GatewayError> {
        match self {
            AgentBackend::Cli(backend) => backend.delete_session(session_id).await,
            AgentBackend::Server(backend) => backend.delete_session(session_id).await,
        }
    }

    async fn compact_session(
        &self,
        session_id: &str,
        model: Option<&str>,
    ) -> Result<AgentResponse, GatewayError> {
        match self {
            AgentBackend::Cli(backend) => backend.compact_session(session_id, model).await,
            AgentBackend::Server(backend) => backend.compact_session(session_id, model).await,
        }
    }

    async fn run_ephemeral(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        match self {
            AgentBackend::Cli(backend) => backend.run_ephemeral(req).await,
            AgentBackend::Server(backend) => backend.run_ephemeral(req).await,
        }
    }

    async fn run_streaming_ephemeral(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        match self {
            AgentBackend::Cli(backend) => backend.run_streaming_ephemeral(req, on_event).await,
            AgentBackend::Server(backend) => backend.run_streaming_ephemeral(req, on_event).await,
        }
    }
}

pub struct OpencodeUtility {
    pub binary: String,
    pub workspace: Option<PathBuf>,
}

impl OpencodeUtility {
    pub fn from_agent_command(command: &[String], workspace: Option<PathBuf>) -> Self {
        Self {
            binary: opencode_binary(command).to_string(),
            workspace,
        }
    }

    pub async fn run_fixed(&self, args: &[&str]) -> Result<String, GatewayError> {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args).stdin(Stdio::null());
        if let Some(ws) = &self.workspace {
            cmd.current_dir(ws);
        }
        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(GatewayError::AgentFailed {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}

impl AiBackend for AgentCliBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        let argv = assemble_argv(
            &self.command,
            req.session_id.as_deref(),
            req.thinking,
            req.model.as_deref(),
            req.agent.as_deref(),
            &req.attachments,
            &req.prompt,
        );
        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(ws) = &self.workspace {
            cmd.current_dir(ws);
        }
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().expect("stdout piped");
        let stderr = child.stderr.take().expect("stderr piped");
        let stdout_task = tokio::spawn(read_pipe(stdout));
        let stderr_task = tokio::spawn(read_pipe(stderr));

        let status = match tokio::time::timeout(agent_timeout(), child.wait()).await {
            Ok(status) => status?,
            Err(_) => {
                let _ = child.start_kill();
                let _ = child.wait().await;
                stdout_task.abort();
                stderr_task.abort();
                return Err(agent_timeout_error(""));
            }
        };
        let stdout_buf = stdout_task.await.unwrap_or_default();
        let stderr_buf = stderr_task.await.unwrap_or_default();
        if !status.success() {
            return Err(GatewayError::AgentFailed {
                code: status.code(),
                stderr: stderr_buf.trim().to_string(),
            });
        }
        Ok(AgentResponse {
            text: stdout_buf.trim().to_string(),
            session_id: None,
        })
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        // Build argv then insert `--format json` before the prompt (last arg).
        let mut argv = assemble_argv(
            &self.command,
            req.session_id.as_deref(),
            req.thinking,
            req.model.as_deref(),
            req.agent.as_deref(),
            &req.attachments,
            &req.prompt,
        );
        let prompt = argv.pop().expect("argv always ends with the prompt");
        argv.push("--format".to_string());
        argv.push("json".to_string());
        argv.push(prompt);

        let mut cmd = Command::new(&argv[0]);
        cmd.args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(ws) = &self.workspace {
            cmd.current_dir(ws);
        }
        let mut child = cmd.spawn()?;

        // Drain stderr concurrently so a large stderr can't deadlock us while
        // we read stdout line-by-line.
        let stderr = child.stderr.take().expect("stderr piped");
        let stderr_task = tokio::spawn(read_pipe(stderr));

        let stdout = child.stdout.take().expect("stdout piped");
        let mut lines = BufReader::new(stdout).lines();
        let mut full = String::new();
        let mut session_id: Option<String> = None;
        let mut text_filter = DsmlToolCallFilter::default();
        let deadline = tokio::time::sleep(agent_timeout());
        tokio::pin!(deadline);
        loop {
            let line = tokio::select! {
                line = lines.next_line() => line?,
                _ = &mut deadline => {
                    let _ = child.start_kill();
                    let _ = child.wait().await;
                    stderr_task.abort();
                    return Err(agent_timeout_error(""));
                }
            };
            let Some(line) = line else {
                break;
            };
            if let Some(id) = parse_session_id(&line) {
                session_id = Some(id);
            }
            if let Some(ev) = parse_event(&line) {
                match ev {
                    StreamEvent::Text(t) => {
                        let clean = text_filter.feed(&t);
                        if !clean.is_empty() {
                            if !full.is_empty() {
                                full.push('\n');
                            }
                            full.push_str(&clean);
                            on_event(StreamEvent::Text(clean));
                        }
                    }
                    other => on_event(other),
                }
            }
        }

        let clean = text_filter.finish();
        if !clean.is_empty() {
            if !full.is_empty() {
                full.push('\n');
            }
            full.push_str(&clean);
            on_event(StreamEvent::Text(clean));
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

async fn read_pipe(mut pipe: impl tokio::io::AsyncRead + Unpin) -> String {
    let mut buf = String::new();
    let _ = pipe.read_to_string(&mut buf).await;
    buf
}

/// 序列化所有讀寫 `WUKONG_AGENT_TIMEOUT_SECS` 的測試（backend 與 opencode_server
/// 兩個模組共用），避免 process-global env var 在平行測試間互相干擾。
#[cfg(test)]
pub(crate) static AGENT_TIMEOUT_ENV_LOCK: tokio::sync::Mutex<()> =
    tokio::sync::Mutex::const_new(());

pub(crate) fn agent_timeout() -> Duration {
    std::env::var("WUKONG_AGENT_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_AGENT_TIMEOUT_SECS))
}

fn agent_timeout_error(stderr: &str) -> GatewayError {
    let mut message =
        "處理逾時，可能是模型 rate limit 或 opencode 無回應；已中斷 agent process。".to_string();
    let stderr = stderr.trim();
    if !stderr.is_empty() {
        message.push_str(" stderr: ");
        message.push_str(stderr);
    }
    GatewayError::AgentFailed {
        code: None,
        stderr: message,
    }
}

#[derive(Default)]
struct DsmlToolCallFilter {
    pending: String,
    in_tool_calls: bool,
}

impl DsmlToolCallFilter {
    fn feed(&mut self, text: &str) -> String {
        self.pending.push_str(text);
        let mut out = String::new();

        loop {
            if self.in_tool_calls {
                if let Some(end) = self.pending.find(DSML_TOOL_CALLS_END) {
                    let drain_to = end + DSML_TOOL_CALLS_END.len();
                    self.pending.drain(..drain_to);
                    if self.pending.starts_with('\n') {
                        self.pending.drain(..1);
                    }
                    self.in_tool_calls = false;
                    continue;
                }
                self.pending.clear();
                break;
            }

            if let Some(start) = self.pending.find(DSML_TOOL_CALLS_START) {
                out.push_str(&self.pending[..start]);
                let drain_to = start + DSML_TOOL_CALLS_START.len();
                self.pending.drain(..drain_to);
                self.in_tool_calls = true;
                continue;
            }

            let keep = suffix_prefix_len(&self.pending, DSML_TOOL_CALLS_START);
            let emit_len = self.pending.len().saturating_sub(keep);
            out.push_str(&self.pending[..emit_len]);
            self.pending.drain(..emit_len);
            break;
        }

        out
    }

    fn finish(mut self) -> String {
        if self.in_tool_calls {
            String::new()
        } else {
            std::mem::take(&mut self.pending)
        }
    }
}

fn suffix_prefix_len(text: &str, prefix: &str) -> usize {
    let max = text.len().min(prefix.len().saturating_sub(1));
    for len in (1..=max).rev() {
        if text.is_char_boundary(text.len() - len)
            && prefix.is_char_boundary(len)
            && text[text.len() - len..] == prefix[..len]
        {
            return len;
        }
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn health_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"healthy\":true}",
                )
                .await
                .unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn cli_backend_is_ready_without_launching_command() {
        let backend = AgentBackend::Cli(AgentCliBackend {
            command: vec!["command-that-must-not-run".to_string()],
            workspace: None,
        });

        backend.check_ready().await.unwrap();
    }

    #[tokio::test]
    async fn server_backend_readiness_delegates_to_health_endpoint() {
        let backend = AgentBackend::Server(
            crate::opencode_server::OpencodeServerBackend::from_env(health_server().await, None),
        );

        backend.check_ready().await.unwrap();
    }

    #[tokio::test]
    async fn server_backend_readiness_reports_connection_failure() {
        let backend =
            AgentBackend::Server(crate::opencode_server::OpencodeServerBackend::from_env(
                "http://127.0.0.1:1".to_string(),
                None,
            ));

        let err = backend.check_ready().await.unwrap_err();
        assert!(err.to_string().contains("health_check"), "{err}");
    }

    #[test]
    fn backend_from_env_uses_cli_when_server_url_missing() {
        static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _lock = ENV_LOCK.lock().unwrap();
        let previous = std::env::var("WUKONG_AGENT_SERVER_URL").ok();
        std::env::remove_var("WUKONG_AGENT_SERVER_URL");

        let backend = build_backend_from_env(vec!["opencode".to_string(), "run".to_string()], None);

        match previous {
            Some(value) => std::env::set_var("WUKONG_AGENT_SERVER_URL", value),
            None => std::env::remove_var("WUKONG_AGENT_SERVER_URL"),
        }

        assert!(matches!(backend, AgentBackend::Cli(_)));
    }

    #[test]
    fn assemble_argv_plain() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            None,
            false,
            None,
            None,
            &[],
            "hi",
        );
        assert_eq!(argv, vec!["opencode", "run", "hi"]);
    }

    #[test]
    fn assemble_argv_with_session_and_thinking() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            Some("ses_x"),
            true,
            None,
            None,
            &[],
            "hi",
        );
        assert_eq!(
            argv,
            vec!["opencode", "run", "-s", "ses_x", "--thinking", "hi"]
        );
    }

    #[test]
    fn assemble_argv_adds_model_before_prompt() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            None,
            false,
            Some("opencode/deepseek-v4-flash-free"),
            None,
            &[],
            "hi",
        );
        assert_eq!(
            argv,
            vec![
                "opencode",
                "run",
                "--model",
                "opencode/deepseek-v4-flash-free",
                "hi"
            ]
        );
    }

    #[test]
    fn assemble_argv_replaces_existing_model_flag() {
        let argv = assemble_argv(
            &[
                "opencode".to_string(),
                "run".to_string(),
                "--model".to_string(),
                "old/model".to_string(),
            ],
            Some("ses_x"),
            true,
            Some("new/model"),
            None,
            &[],
            "hi",
        );
        assert_eq!(
            argv,
            vec![
                "opencode",
                "run",
                "-s",
                "ses_x",
                "--thinking",
                "--model",
                "new/model",
                "hi"
            ]
        );
    }

    #[test]
    fn assemble_argv_adds_files_before_prompt() {
        let files = vec![
            AgentAttachment {
                path: std::path::PathBuf::from("/tmp/report.pdf"),
                original_name: "report.pdf".to_string(),
                mime_type: Some("application/pdf".to_string()),
            },
            AgentAttachment {
                path: std::path::PathBuf::from("/tmp/photo.jpg"),
                original_name: "photo.jpg".to_string(),
                mime_type: Some("image/jpeg".to_string()),
            },
        ];
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            None,
            false,
            None,
            None,
            &files,
            "describe",
        );
        assert_eq!(
            argv,
            vec![
                "opencode",
                "run",
                "--file",
                "/tmp/report.pdf",
                "--file",
                "/tmp/photo.jpg",
                "describe"
            ]
        );
    }

    #[test]
    fn assemble_argv_adds_agent_before_prompt() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            None,
            false,
            None,
            Some("plan"),
            &[],
            "hi",
        );
        assert_eq!(argv, vec!["opencode", "run", "--agent", "plan", "hi"]);
    }

    #[test]
    fn assemble_argv_omits_agent_when_none_or_blank() {
        let none = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            None,
            false,
            None,
            None,
            &[],
            "hi",
        );
        assert!(!none.contains(&"--agent".to_string()));
        let blank = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            None,
            false,
            None,
            Some("  "),
            &[],
            "hi",
        );
        assert!(!blank.contains(&"--agent".to_string()));
    }

    #[test]
    fn opencode_binary_uses_first_base_command_arg() {
        assert_eq!(
            opencode_binary(&["opencode".to_string(), "run".to_string()]),
            "opencode"
        );
        assert_eq!(
            opencode_binary(&["/usr/local/bin/opencode".to_string(), "run".to_string()]),
            "/usr/local/bin/opencode"
        );
    }

    #[tokio::test]
    async fn agent_cli_backend_captures_stdout() {
        // `echo <prompt>` prints the prompt back; verifies capture + trim.
        let backend = AgentCliBackend {
            command: vec!["echo".to_string()],
            workspace: None,
        };
        let resp = backend
            .run(AgentRequest {
                prompt: "hello wukong".to_string(),
                session_id: None,
                thinking: false,
                model: None,
                agent: None,
                tool_overrides: BTreeMap::new(),
                attachments: Vec::new(),
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
            workspace: None,
        };
        let err = backend
            .run(AgentRequest {
                prompt: "x".to_string(),
                session_id: None,
                thinking: false,
                model: None,
                agent: None,
                tool_overrides: BTreeMap::new(),
                attachments: Vec::new(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, GatewayError::AgentFailed { .. }));
    }

    #[tokio::test]
    async fn agent_cli_backend_times_out_and_reports_stuck_agent() {
        let _guard = AGENT_TIMEOUT_ENV_LOCK.lock().await;
        std::env::set_var("WUKONG_AGENT_TIMEOUT_SECS", "1");
        let backend = AgentCliBackend {
            command: vec!["sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
            workspace: None,
        };

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            backend.run(AgentRequest {
                prompt: "ignored".to_string(),
                session_id: None,
                thinking: false,
                model: None,
                agent: None,
                tool_overrides: BTreeMap::new(),
                attachments: Vec::new(),
            }),
        )
        .await;
        std::env::remove_var("WUKONG_AGENT_TIMEOUT_SECS");

        let err = result
            .expect("backend should return before the outer test timeout")
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("處理逾時"));
        assert!(msg.contains("rate limit"));
        assert!(msg.contains("opencode 無回應"));
    }

    #[tokio::test]
    async fn agent_timeout_defaults_to_twenty_minutes() {
        let _guard = AGENT_TIMEOUT_ENV_LOCK.lock().await;
        std::env::remove_var("WUKONG_AGENT_TIMEOUT_SECS");

        assert_eq!(agent_timeout(), std::time::Duration::from_secs(20 * 60));
    }

    #[tokio::test]
    async fn run_streaming_default_emits_single_text() {
        // Test the DEFAULT run_streaming impl via a minimal mock backend.
        struct Plain;
        impl AiBackend for Plain {
            async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
                Ok(AgentResponse {
                    text: "whole answer".to_string(),
                    session_id: None,
                })
            }
        }
        let mut events = Vec::new();
        let resp = Plain
            .run_streaming(
                AgentRequest {
                    prompt: "x".into(),
                    session_id: None,
                    thinking: false,
                    model: None,
                    agent: None,
                    tool_overrides: BTreeMap::new(),
                    attachments: Vec::new(),
                },
                &mut |e| events.push(e),
            )
            .await
            .unwrap();
        assert_eq!(resp.text, "whole answer");
        assert_eq!(events, vec![StreamEvent::Text("whole answer".to_string())]);
    }

    #[tokio::test]
    async fn default_session_controls_compact_and_cleanup() {
        struct RecordingBackend {
            requests: std::sync::Mutex<Vec<AgentRequest>>,
            deleted: std::sync::Mutex<Vec<String>>,
        }

        impl AiBackend for RecordingBackend {
            async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
                self.requests.lock().unwrap().push(req);
                Ok(AgentResponse {
                    text: "helper".to_string(),
                    session_id: Some("ses_helper".to_string()),
                })
            }

            async fn delete_session(&self, session_id: &str) -> Result<(), GatewayError> {
                self.deleted.lock().unwrap().push(session_id.to_string());
                Ok(())
            }
        }

        let backend = RecordingBackend {
            requests: std::sync::Mutex::new(Vec::new()),
            deleted: std::sync::Mutex::new(Vec::new()),
        };
        let response = backend
            .run_ephemeral(AgentRequest {
                prompt: "route".to_string(),
                session_id: None,
                thinking: false,
                model: None,
                agent: None,
                tool_overrides: BTreeMap::new(),
                attachments: Vec::new(),
            })
            .await
            .unwrap();
        assert_eq!(response.text, "helper");
        assert_eq!(backend.deleted.lock().unwrap().as_slice(), &["ses_helper"]);

        let response = backend
            .compact_session("ses_42", Some("opencode/deepseek-v4-flash-free"))
            .await
            .unwrap();
        assert_eq!(response.text, "helper");
        let request = backend.requests.lock().unwrap().last().cloned().unwrap();
        assert_eq!(request.prompt, "/compact");
        assert_eq!(request.session_id.as_deref(), Some("ses_42"));
        assert_eq!(
            request.model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
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
            workspace: None,
        };
        let mut events = Vec::new();
        let resp = backend
            .run_streaming(
                AgentRequest {
                    prompt: "ignored".into(),
                    session_id: None,
                    thinking: false,
                    model: None,
                    agent: None,
                    tool_overrides: BTreeMap::new(),
                    attachments: Vec::new(),
                },
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

    #[tokio::test]
    async fn agent_cli_run_streaming_removes_dsml_tool_call_blocks_from_text() {
        let backend = AgentCliBackend {
            command: vec![
                "printf".to_string(),
                "%s\n".to_string(),
                r#"{"type":"text","part":{"type":"text","text":"before\n<｜｜DSML｜｜tool_calls>\n<｜｜DSML｜｜invoke name=\"bash\">\n<｜｜DSML｜｜parameter name=\"command\" string=\"true\">pwd</｜｜DSML｜｜parameter>\n</｜｜DSML｜｜invoke>\n</｜｜DSML｜｜tool_calls>\nafter"}}"#.to_string(),
            ],
            workspace: None,
        };
        let mut events = Vec::new();

        let resp = backend
            .run_streaming(
                AgentRequest {
                    prompt: "ignored".into(),
                    session_id: None,
                    thinking: false,
                    model: None,
                    agent: None,
                    tool_overrides: BTreeMap::new(),
                    attachments: Vec::new(),
                },
                &mut |e| events.push(e),
            )
            .await
            .unwrap();

        assert_eq!(resp.text, "before\nafter");
        assert_eq!(events, vec![StreamEvent::Text("before\nafter".to_string())]);
    }

    #[tokio::test]
    async fn agent_cli_run_streaming_times_out_and_reports_stuck_agent() {
        let _guard = AGENT_TIMEOUT_ENV_LOCK.lock().await;
        std::env::set_var("WUKONG_AGENT_TIMEOUT_SECS", "1");
        let backend = AgentCliBackend {
            command: vec!["sh".to_string(), "-c".to_string(), "sleep 30".to_string()],
            workspace: None,
        };
        let mut events = Vec::new();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            backend.run_streaming(
                AgentRequest {
                    prompt: "ignored".into(),
                    session_id: None,
                    thinking: false,
                    model: None,
                    agent: None,
                    tool_overrides: BTreeMap::new(),
                    attachments: Vec::new(),
                },
                &mut |e| events.push(e),
            ),
        )
        .await;
        std::env::remove_var("WUKONG_AGENT_TIMEOUT_SECS");

        let err = result
            .expect("backend should return before the outer test timeout")
            .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("處理逾時"));
        assert!(msg.contains("rate limit"));
        assert!(msg.contains("opencode 無回應"));
        assert!(events.is_empty());
    }
}
