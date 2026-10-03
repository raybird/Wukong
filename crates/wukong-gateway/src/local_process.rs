use crate::backend::{
    agent_timeout, question_reject_route, question_reply_route, AgentRequest, AgentResponse,
    AiBackend, QuestionReplyRoute,
};
use crate::opencode_server::OpencodeServerBackend;
use crate::{GatewayError, StreamEvent};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::{Child, Command};

type Pending = Mutex<HashMap<(String, String), Arc<OpencodeServerBackend>>>;

/// 2026-10-03：只在執行期間持有本機 server；閒置不保留程序或 client。
pub struct LocalProcessBackend {
    binary: String,
    workspace: Option<PathBuf>,
    pending: Pending,
}

enum Operation {
    Run(AgentRequest, bool, bool),
    Compact(String, Option<String>),
    Delete(String),
}

impl LocalProcessBackend {
    pub fn new(binary: String, workspace: Option<PathBuf>) -> Self {
        Self {
            binary,
            workspace,
            pending: Mutex::new(HashMap::new()),
        }
    }

    pub async fn reply(
        &self,
        session: &str,
        request: &str,
        answers: Option<Vec<Vec<String>>>,
    ) -> Result<(), GatewayError> {
        let backend = self
            .pending
            .lock()
            .unwrap()
            .get(&(session.into(), request.into()))
            .cloned()
            .ok_or_else(|| failed("找不到此回合的待回覆問題，可能已結束。"))?;
        let route = match &answers {
            Some(answers) => question_reply_route(request, answers)?,
            None => question_reject_route(request),
        };
        match route {
            QuestionReplyRoute::Permission { id, reply } => {
                backend.reply_permission(id, reply).await
            }
            QuestionReplyRoute::Question => backend.reply_local_question(request, answers).await,
        }
    }

    async fn execute(
        &self,
        operation: Operation,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        let deadline = tokio::time::Instant::now() + agent_timeout();
        let mut command = Command::new(&self.binary);
        command
            .args(["serve", "--hostname", "127.0.0.1", "--port", "0"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .kill_on_drop(true);
        if let Some(workspace) = &self.workspace {
            command.current_dir(workspace).env("PWD", workspace);
        }
        command
            .env_remove("OPENCODE_SERVER_PASSWORD")
            .env_remove("OPENCODE_SERVER_USERNAME");
        for (source, target) in [
            ("WUKONG_AGENT_SERVER_PASSWORD", "OPENCODE_SERVER_PASSWORD"),
            ("WUKONG_AGENT_SERVER_USERNAME", "OPENCODE_SERVER_USERNAME"),
        ] {
            if let Ok(value) = std::env::var(source) {
                command.env(target, value);
            }
        }
        #[cfg(unix)]
        command.process_group(0);
        let child = command.spawn()?;
        let mut process = Process {
            pid: child.id(),
            child,
        };
        let operation = async {
            let mut lines = BufReader::new(process.child.stdout.take().unwrap()).lines();
            let url = loop {
                let line = lines
                    .next_line()
                    .await?
                    .ok_or_else(|| failed("本機 OpenCode 未回報啟動位址便結束。"))?;
                if let Some(url) = line.strip_prefix("opencode server listening on ") {
                    let parsed = reqwest::Url::parse(url.trim())
                        .map_err(|_| failed("本機 OpenCode 啟動位址無效。"))?;
                    if parsed.scheme() != "http"
                        || parsed.host_str() != Some("127.0.0.1")
                        || parsed.port().is_none()
                    {
                        return Err(failed("本機 OpenCode 必須監聽 loopback 埠。"));
                    }
                    break parsed.to_string();
                }
            };
            // 保持讀取 stdout，避免之後的程序輸出填滿 pipe。
            let drain =
                tokio::spawn(async move { while let Ok(Some(_)) = lines.next_line().await {} });
            let _drain = Drain(drain);
            let backend = Arc::new(OpencodeServerBackend::for_local_process(
                url,
                self.workspace.clone(),
            ));
            backend.health_check().await?;
            let (req, ephemeral, interactive) = match operation {
                Operation::Run(req, ephemeral, interactive) => (req, ephemeral, interactive),
                Operation::Compact(id, model) => {
                    return backend.compact_session(&id, model.as_deref()).await
                }
                Operation::Delete(id) => {
                    backend.delete_session(&id).await?;
                    return Ok(AgentResponse {
                        text: String::new(),
                        session_id: None,
                    });
                }
            };
            let _pending = PendingTurn {
                pending: &self.pending,
                backend: backend.clone(),
            };
            let (questions, mut pending_questions) = tokio::sync::mpsc::unbounded_channel();
            let mut sink = |event: StreamEvent| {
                if let StreamEvent::QuestionRequest(question) = &event {
                    self.pending.lock().unwrap().insert(
                        (question.session_id.clone(), question.request_id.clone()),
                        backend.clone(),
                    );
                    if !interactive {
                        let _ = questions.send(question.clone());
                    }
                }
                on_event(event);
            };
            let run = backend.run_streaming(req, &mut sink);
            tokio::pin!(run);
            let mut response = loop {
                tokio::select! {
                    result = &mut run => break result?,
                    Some(question) = pending_questions.recv(), if !interactive => {
                        self.reply(&question.session_id, &question.request_id, None).await?;
                    }
                }
            };
            if ephemeral {
                if let Some(id) = &response.session_id {
                    if let Err(error) = backend.delete_session(id).await {
                        eprintln!("helper_session_delete_failed session_id={id}: {error}");
                    }
                }
                response.session_id = None;
            }
            Ok(response)
        };
        let result = match tokio::time::timeout_at(deadline, operation).await {
            Ok(result) => result,
            Err(_) => Err(failed("本機 OpenCode 回合逾時，已中止程序。")),
        };
        process.stop().await;
        result
    }
}

impl AiBackend for LocalProcessBackend {
    async fn run(&self, mut req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        req.tool_overrides.insert("question".into(), false);
        self.execute(Operation::Run(req, false, false), &mut |_| {})
            .await
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        self.execute(Operation::Run(req, false, true), on_event)
            .await
    }

    async fn compact_session(
        &self,
        session_id: &str,
        model: Option<&str>,
    ) -> Result<AgentResponse, GatewayError> {
        self.execute(
            Operation::Compact(session_id.into(), model.map(str::to_owned)),
            &mut |_| {},
        )
        .await
    }

    async fn delete_session(&self, session_id: &str) -> Result<(), GatewayError> {
        self.execute(Operation::Delete(session_id.into()), &mut |_| {})
            .await
            .map(|_| ())
    }

    async fn run_ephemeral(&self, mut req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        req.tool_overrides.insert("question".into(), false);
        self.execute(Operation::Run(req, true, false), &mut |_| {})
            .await
    }

    async fn run_streaming_ephemeral(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        self.execute(Operation::Run(req, true, true), on_event)
            .await
    }
}

struct PendingTurn<'a> {
    pending: &'a Pending,
    backend: Arc<OpencodeServerBackend>,
}

impl Drop for PendingTurn<'_> {
    fn drop(&mut self) {
        self.pending
            .lock()
            .unwrap()
            .retain(|_, backend| !Arc::ptr_eq(backend, &self.backend));
    }
}

struct Drain(tokio::task::JoinHandle<()>);
impl Drop for Drain {
    fn drop(&mut self) {
        self.0.abort();
    }
}

struct Process {
    child: Child,
    pid: Option<u32>,
}
impl Process {
    async fn stop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid {
            unsafe {
                libc::kill(pid as i32, libc::SIGINT);
            }
        }
        #[cfg(not(unix))]
        let _ = self.child.start_kill();
        if tokio::time::timeout(Duration::from_secs(3), self.child.wait())
            .await
            .is_err()
        {
            let _ = self.child.start_kill();
            let _ = self.child.wait().await;
        }
        self.kill_group();
    }

    fn kill_group(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.pid.take() {
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.kill_group();
    }
}

fn failed(message: &str) -> GatewayError {
    GatewayError::AgentFailed {
        code: None,
        stderr: message.into(),
    }
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use std::collections::BTreeMap;
    use std::os::unix::fs::PermissionsExt;

    fn request() -> AgentRequest {
        AgentRequest {
            prompt: "probe".into(),
            session_id: None,
            thinking: false,
            model: None,
            agent: None,
            tool_overrides: BTreeMap::new(),
            attachments: vec![],
        }
    }

    fn fixture(body: &str) -> (tempfile::TempDir, LocalProcessBackend) {
        let dir = tempfile::tempdir().unwrap();
        let binary = dir.path().join("agent");
        std::fs::write(&binary, format!("#!/bin/sh\necho $$ > pid\n{body}\n")).unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        let backend = LocalProcessBackend::new(
            binary.to_string_lossy().into_owned(),
            Some(dir.path().into()),
        );
        (dir, backend)
    }

    async fn assert_stopped(dir: &std::path::Path) {
        let pid = std::fs::read_to_string(dir.join("pid")).unwrap();
        let path = PathBuf::from(format!("/proc/{}", pid.trim()));
        for _ in 0..50 {
            if !path.exists() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("child still alive: {}", pid.trim());
    }

    // SCN-006／007：future 取消時，尚未完成啟動的 child 也須回收。
    #[tokio::test]
    async fn cancelled_startup_releases_child() {
        let (dir, backend) = fixture("exec sleep 30");
        assert!(
            tokio::time::timeout(Duration::from_millis(300), backend.run(request()))
                .await
                .is_err()
        );
        assert_stopped(dir.path()).await;
    }

    #[tokio::test]
    async fn invalid_listener_releases_child() {
        let (dir, backend) =
            fixture("echo 'opencode server listening on http://0.0.0.0:1234'\nexec sleep 30");
        let error = backend.run(request()).await.unwrap_err();
        assert!(error.to_string().contains("loopback"));
        assert_stopped(dir.path()).await;
    }

    #[tokio::test]
    async fn health_failure_releases_child() {
        let (dir, backend) =
            fixture("echo 'opencode server listening on http://127.0.0.1:1'\nexec sleep 30");
        let error = backend.run(request()).await.unwrap_err();
        assert!(error.to_string().contains("health_check"));
        assert_stopped(dir.path()).await;
    }
}
