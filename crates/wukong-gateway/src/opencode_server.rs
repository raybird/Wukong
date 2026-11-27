mod event_map;
#[cfg(test)]
mod fake_v2;
mod sse;

use crate::backend::{
    agent_timeout, AgentRequest, AgentResponse, AiBackend, SessionListing, SessionSummary,
};
use crate::error::GatewayError;
use crate::stream::StreamEvent;
use crate::upstream_error::RetryWatch;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use event_map::{event_type_of, ServerEventAction, ServerEventMapper};
use reqwest::{StatusCode, Url};
use serde::Serialize;
use serde_json::Value;
use sse::SseParser;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const INLINE_ATTACHMENT_MAX_BYTES: u64 = 10 * 1024 * 1024;
/// V2 的 basic auth 使用者名稱固定是 opencode，`OPENCODE_SERVER_USERNAME` 已不支援，
/// 用別的名稱會被拒（2026-11-24 對 2.3.2 實測）。
const SERVER_USERNAME: &str = "opencode";
/// 列表每頁要求的筆數。V2 不帶 limit 只回最新 50 筆（地雷 W-6），所以一定要照 cursor 走完。
const SESSION_PAGE_SIZE: usize = 1000;
/// 走到這麼多頁還有下一頁就停下來，回報清單被截斷，不讓壞掉的 cursor 卡住呼叫端。
const SESSION_LIST_MAX_PAGES: usize = 100;
/// 等 agent 定義重新載入時，兩次查詢 `/api/agent` 的間隔。
const AGENT_POLL_INTERVAL: std::time::Duration = std::time::Duration::from_millis(100);
/// 查背景工作失敗時最多查幾次、每次間隔多久（PR #47 第三方審查 P2-1）。
const JOBS_LOOKUP_ATTEMPTS: usize = 3;
const JOBS_LOOKUP_RETRY_DELAY: std::time::Duration = std::time::Duration::from_millis(500);
/// 每次查背景工作最多等多久（review-364cebf NICE 1）。不沿用整個回合的 client 逾時：V2 卡住時，
/// 重試會把延遲放大成好幾倍，而且等待期間看不到回合的 deadline。
const JOBS_LOOKUP_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

pub struct OpencodeServerBackend {
    pub base_url: String,
    pub workspace: Option<PathBuf>,
    file_mode: FileTransportMode,
    server_workspace: Option<PathBuf>,
    client: reqwest::Client,
    stream_client: reqwest::Client,
    password: Option<String>,
    session_attachments: Mutex<HashMap<String, Vec<crate::backend::AgentAttachment>>>,
    /// 重試持續多久就中止回合；只有測試會改。
    retry_give_up_after: std::time::Duration,
    /// 見 [`JOBS_LOOKUP_TIMEOUT`]；只有測試會改。
    jobs_lookup_timeout: std::time::Duration,
    /// 串流中看到的 form：form ID →（session ID, 欄位）。該 session 的事件串流結束時清掉。
    forms: Forms,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum FileTransportMode {
    Shared,
    Inline,
    Disabled,
    Invalid(String),
}

#[derive(Debug, Serialize)]
struct CreateSessionBody {
    title: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    permissions: Vec<PermissionRule>,
}

/// 見 [`OpencodeServerBackend::interrupt_on_drop`]。回合本身的 session（第一個），加上串流中看到的
/// subagent 子 session（#45：subagent 在背景執行，中斷回合時要一併中斷）。
#[derive(Default)]
struct InterruptOnDrop {
    server: Option<ServerHandle>,
    sessions: Vec<String>,
}

impl InterruptOnDrop {
    fn add(&mut self, session_id: &str) {
        self.sessions.push(session_id.to_string());
    }

    fn disarm(&mut self) {
        self.server = None;
        self.sessions.clear();
    }

    /// 回合失敗時立即送出。Best-effort：server 端的執行仍會繼續並佔住 session，後續回合
    /// 要排在它後面；失敗也不影響原本要回傳的錯誤。
    async fn send_now(&mut self) {
        if let Some(server) = self.server.take() {
            server
                .interrupt_turn(std::mem::take(&mut self.sessions))
                .await;
        }
    }
}

impl Drop for InterruptOnDrop {
    fn drop(&mut self) {
        let Some(server) = self.server.take() else {
            return;
        };
        let sessions = std::mem::take(&mut self.sessions);
        if let Ok(runtime) = tokio::runtime::Handle::try_current() {
            runtime.spawn(async move {
                for result in server.interrupt_turn(sessions).await {
                    match result {
                        Ok(status) => eprintln!("turn_cancelled_interrupt status={status}"),
                        Err(err) => eprintln!("turn_cancelled_interrupt_failed: {err}"),
                    }
                }
            });
        }
    }
}

/// 中斷回合要用的連線資訊；取消發生在 backend 借用之外（future 被丟掉），所以另外帶一份。
struct ServerHandle {
    base_url: String,
    client: reqwest::Client,
    password: Option<String>,
    jobs_lookup_timeout: std::time::Duration,
}

impl ServerHandle {
    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        authorize(request, self.password.as_deref())
    }

    /// 依序中斷 `sessions`（第一個是回合本身的 session），再從背景工作清單補上串流中沒看到的
    /// subagent 子 session。V2 中斷父 session 不會取消它的背景 subagent：`Session.interrupt` 只停
    /// 那個 session 的執行，`Job.cancelSession` 只在刪除 session 時呼叫（PR #47 第三方審查 P2-2，
    /// 查 2.3.2 的原始碼）。先停 primary，它才不會在查清單之後又派出新的工作。
    async fn interrupt_turn(&self, mut sessions: Vec<String>) -> Vec<Result<u16, reqwest::Error>> {
        let mut results = Vec::new();
        for session in &sessions {
            results.push(self.interrupt(session).await);
        }
        let Some(root) = sessions.first().cloned() else {
            return results;
        };
        let jobs = match self.background_jobs(&root).await {
            Ok(jobs) => Some(jobs),
            Err(err) => {
                eprintln!("turn_cancelled_jobs_lookup_failed session_id={root}: {err}");
                None
            }
        };
        for child in jobs
            .iter()
            .flat_map(subagent_jobs)
            .filter_map(|job| job.get("childSessionID").and_then(Value::as_str))
        {
            if !sessions.iter().any(|s| s == child) {
                results.push(self.interrupt(child).await);
                sessions.push(child.to_string());
            }
        }
        results
    }

    /// 查一次這個 session 與它的子 session 的背景工作清單（`GET /api/experimental/job`），最多等
    /// `jobs_lookup_timeout`。非 2xx 與不是 JSON 的回應都算失敗。
    async fn background_jobs(&self, session_id: &str) -> Result<Value, GatewayError> {
        let url = format!(
            "{}/api/experimental/job?sessionID={session_id}&descendants=true",
            self.base_url
        );
        let request = self
            .authorize(self.client.get(url))
            .timeout(self.jobs_lookup_timeout);
        request_json("background_jobs", request).await
    }

    async fn interrupt(&self, session_id: &str) -> Result<u16, reqwest::Error> {
        let url = format!("{}/api/session/{}/interrupt", self.base_url, session_id);
        let response = self.authorize(self.client.post(url)).send().await?;
        Ok(response.status().as_u16())
    }
}

/// 送出已授權的請求並讀回 JSON。非 2xx 與不是 JSON 的回應都算失敗；送出與讀取的錯誤經
/// [`http_error`] 展開原因。
async fn request_json(
    phase: &'static str,
    request: reqwest::RequestBuilder,
) -> Result<Value, GatewayError> {
    let response = request.send().await.map_err(|err| http_error(phase, err))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|err| http_error(phase, err))?;
    if !status.is_success() {
        return Err(GatewayError::AgentFailed {
            code: Some(status.as_u16() as i32),
            stderr: format!("opencode server {phase} returned {status}: {text}"),
        });
    }
    serde_json::from_str(&text).map_err(|err| GatewayError::AgentFailed {
        code: None,
        stderr: format!("opencode server {phase} returned invalid JSON: {err}; body: {text}"),
    })
}

fn authorize(request: reqwest::RequestBuilder, password: Option<&str>) -> reqwest::RequestBuilder {
    match password {
        Some(password) => request.basic_auth(SERVER_USERNAME, Some(password)),
        None => request,
    }
}

/// 背景工作清單（`GET /api/experimental/job` 的回應）裡的 subagent 工作。清單也列出 subagent 開的
/// 背景 shell、圖片與 code mode 工作（`Job.Kind`），在用戶端過濾，不依賴 server 端的 `kind` 參數。
fn subagent_jobs(response: &Value) -> impl Iterator<Item = &Value> {
    response
        .get("data")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter(|job| job.get("kind").and_then(Value::as_str) == Some("subagent"))
}

/// V2 的權限規則（openapi 的 `Permission.Rule`）。
#[derive(Debug, Serialize, PartialEq, Eq)]
struct PermissionRule {
    action: String,
    resource: &'static str,
    effect: &'static str,
}

#[derive(Debug, Serialize)]
struct PromptBody {
    text: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    files: Vec<FileAttachment>,
}

/// `file://` 或 `data:` URI，兩種 V2 2.3.2 都讀得到內容（2026-11-24 實測）。
#[derive(Debug, Serialize, PartialEq, Eq)]
struct FileAttachment {
    uri: String,
    name: String,
}

#[derive(Debug, Serialize)]
struct ModelBody {
    model: ModelRef,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
struct ModelRef {
    id: String,
    #[serde(rename = "providerID")]
    provider_id: String,
}

#[derive(Debug, Serialize)]
struct AgentBody<'a> {
    agent: &'a str,
}

#[derive(Debug, Serialize)]
struct PermissionReplyBody<'a> {
    decision: &'a str,
}

type Forms = Mutex<HashMap<String, (String, Vec<FormField>)>>;

/// 事件串流結束（含提早回傳錯誤）時，清掉這個 session 記下的 form。
struct ForgetForms<'a> {
    forms: &'a Forms,
    session_id: &'a str,
}

impl Drop for ForgetForms<'_> {
    fn drop(&mut self) {
        self.forms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .retain(|_, (session_id, _)| session_id != self.session_id);
    }
}

/// 一則 V2 form 的欄位，回覆時要用：答案以欄位的 `key` 為鍵，選項送 `value`（入口看到的
/// 是 `label`），多選送陣列。
#[derive(Debug, Clone, PartialEq, Eq)]
struct FormField {
    key: String,
    multiselect: bool,
    /// (label, value)
    options: Vec<(String, String)>,
}

impl OpencodeServerBackend {
    pub(crate) fn for_local_process(
        base_url: String,
        workspace: Option<PathBuf>,
        password: String,
    ) -> Self {
        let mut backend = Self::from_env(base_url, workspace.clone());
        backend.file_mode = FileTransportMode::Shared;
        backend.server_workspace = workspace;
        backend.password = Some(password);
        backend
    }

    pub fn from_env(base_url: String, workspace: Option<PathBuf>) -> Self {
        let file_mode = FileTransportMode::from_env();
        let server_workspace = std::env::var("WUKONG_AGENT_SERVER_WORKSPACE")
            .ok()
            .filter(|value| !value.trim().is_empty())
            .map(PathBuf::from)
            .or_else(|| workspace.clone());
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            workspace,
            file_mode,
            server_workspace,
            client: reqwest::Client::builder()
                .timeout(agent_timeout())
                .build()
                .expect("reqwest client builder should not fail"),
            // reqwest 的 client timeout 涵蓋整個 body 讀取；`/event` SSE 要陪整
            // 回合跑，掛總 timeout 會在回合中途被切斷，所以串流專用 client 只設
            // 連線逾時，回合時限交給 consume_event_stream 的 deadline。
            stream_client: reqwest::Client::builder()
                .connect_timeout(std::time::Duration::from_secs(10))
                .build()
                .expect("reqwest client builder should not fail"),
            password: std::env::var("WUKONG_AGENT_SERVER_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty()),
            session_attachments: Mutex::new(HashMap::new()),
            retry_give_up_after: crate::upstream_error::RETRY_GIVE_UP_AFTER,
            jobs_lookup_timeout: JOBS_LOOKUP_TIMEOUT,
            forms: Mutex::new(HashMap::new()),
        }
    }

    /// V2 的 prompt 沒有每則訊息的工具開關，`tool_overrides` 只能在建立 session 時轉成
    /// 權限規則，而且會一直留在這個 session 上；帶覆寫的呼叫（規劃、路由）都用新的暫時
    /// session，所以只在這裡套用，不套到既有 session。
    async fn create_session(&self, req: &AgentRequest) -> Result<String, GatewayError> {
        let url = format!("{}/api/session", self.base_url);
        let permissions = req
            .tool_overrides
            .iter()
            .map(|(tool, enabled)| PermissionRule {
                action: tool.clone(),
                resource: "*",
                effect: if *enabled { "allow" } else { "deny" },
            })
            .collect();
        let value = self
            .send_json(
                "create_session",
                self.client.post(url).json(&CreateSessionBody {
                    title: "Wukong".to_string(),
                    permissions,
                }),
            )
            .await?;
        extract_session_id(&value).ok_or_else(|| GatewayError::AgentFailed {
            code: None,
            stderr: format!("opencode server did not return a session id: {value}"),
        })
    }

    async fn cleanup_session_if_owned(&self, should_cleanup: bool, session_id: &str) {
        if should_cleanup {
            if let Err(error) = self.delete_session(session_id).await {
                eprintln!("session_cleanup_failed session_id={session_id}: {error}");
            }
        }
    }

    /// V2 對 `/api` 以外的路徑都回網頁（200），只看狀態碼會把任何東西判成健康；
    /// `/api/info` 回的是 JSON，`send_json` 解析不了就是失敗。
    pub(crate) async fn health_check(&self) -> Result<(), GatewayError> {
        let url = format!("{}/api/info", self.base_url);
        match self.send_json("health_check", self.client.get(url)).await {
            Ok(_) => Ok(()),
            Err(GatewayError::AgentFailed {
                code: Some(401),
                stderr,
            }) => Err(GatewayError::AgentFailed {
                code: Some(401),
                stderr: format!(
                    "opencode server 認證被拒：WUKONG_AGENT_SERVER_PASSWORD 與 server 的 OPENCODE_SERVER_PASSWORD 不同或沒有設定（{stderr}）"
                ),
            }),
            Err(error) => Err(error),
        }
    }

    async fn build_files(&self, req: &AgentRequest) -> Result<Vec<FileAttachment>, GatewayError> {
        let mut files = Vec::new();
        for attachment in &req.attachments {
            let mime = supported_attachment_mime(attachment)?;
            let filename = safe_display_filename(&attachment.original_name, &attachment.path);
            let path = self.validated_attachment_path(&attachment.path)?;
            let uri = match &self.file_mode {
                FileTransportMode::Shared => self.shared_file_url(&path)?,
                FileTransportMode::Inline => {
                    let metadata = tokio::fs::metadata(&path).await.map_err(|err| {
                        attachment_error(format!("無法讀取附件 {} 的資訊：{err}", path.display()))
                    })?;
                    if metadata.len() > INLINE_ATTACHMENT_MAX_BYTES {
                        return Err(attachment_error(format!(
                            "遠端附件模式限制單檔 10 MiB；{} 為 {} bytes",
                            filename,
                            metadata.len()
                        )));
                    }
                    let bytes = tokio::fs::read(&path).await.map_err(|err| {
                        attachment_error(format!("無法讀取附件 {}：{err}", path.display()))
                    })?;
                    format!("data:{mime};base64,{}", BASE64_STANDARD.encode(bytes))
                }
                FileTransportMode::Disabled => {
                    return Err(attachment_error(
                        "opencode server 附件功能已由 WUKONG_AGENT_SERVER_FILE_MODE=disabled 停用",
                    ));
                }
                FileTransportMode::Invalid(value) => {
                    return Err(attachment_error(format!(
                        "無效的 WUKONG_AGENT_SERVER_FILE_MODE={value:?}；可用值為 shared、inline、disabled"
                    )));
                }
            };
            files.push(FileAttachment {
                uri,
                name: filename,
            });
        }
        Ok(files)
    }

    fn validated_attachment_path(&self, attachment: &Path) -> Result<PathBuf, GatewayError> {
        let path = attachment.canonicalize().map_err(|err| {
            attachment_error(format!("無法解析附件路徑 {}：{err}", attachment.display()))
        })?;
        let workspace = self.workspace.as_ref().ok_or_else(|| {
            attachment_error("server backend 傳送附件時必須設定 WUKONG_WORKSPACE")
        })?;
        let workspace = workspace.canonicalize().map_err(|err| {
            attachment_error(format!(
                "無法解析 WUKONG_WORKSPACE {}：{err}",
                workspace.display()
            ))
        })?;
        if !path.starts_with(&workspace) {
            return Err(attachment_error(format!(
                "拒絕傳送工作區外的附件：{}",
                path.display()
            )));
        }
        Ok(path)
    }

    fn shared_file_url(&self, attachment: &Path) -> Result<String, GatewayError> {
        let local_workspace = self
            .workspace
            .as_ref()
            .ok_or_else(|| attachment_error("shared 附件模式需要設定 WUKONG_WORKSPACE"))?;
        let local_workspace = local_workspace.canonicalize().map_err(|err| {
            attachment_error(format!(
                "無法解析 WUKONG_WORKSPACE {}：{err}",
                local_workspace.display()
            ))
        })?;
        let relative = attachment.strip_prefix(&local_workspace).map_err(|_| {
            attachment_error(format!("附件不在共享工作區內：{}", attachment.display()))
        })?;
        let server_workspace = self.server_workspace.as_ref().ok_or_else(|| {
            attachment_error(
                "shared 附件模式需要設定 WUKONG_AGENT_SERVER_WORKSPACE 或 WUKONG_WORKSPACE",
            )
        })?;
        if !server_workspace.is_absolute() {
            return Err(attachment_error(format!(
                "OpenCode server 工作區必須是絕對路徑：{}",
                server_workspace.display()
            )));
        }
        let server_path = server_workspace.join(relative);
        Url::from_file_path(&server_path)
            .map(|url| url.to_string())
            .map_err(|_| {
                attachment_error(format!(
                    "無法將附件轉成 file URL：{}",
                    server_path.display()
                ))
            })
    }

    fn remember_attachments(&self, session_id: &str, req: &AgentRequest) {
        if req.attachments.is_empty() {
            return;
        }
        self.session_attachments
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(session_id.to_string(), req.attachments.clone());
    }

    fn rehydrate_request(&self, session_id: &str, req: &AgentRequest) -> AgentRequest {
        if !req.attachments.is_empty() {
            return req.clone();
        }
        let remembered = self
            .session_attachments
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(session_id)
            .cloned()
            .unwrap_or_default();
        let mut retry = req.clone();
        retry.attachments = remembered;
        retry
    }

    /// 送出一則 prompt。V2 的 agent 與模型是 session 的狀態，要在 prompt 前分別設定；
    /// prompt 本身只收文字與附件，回 200 就排進佇列，結果由事件串流取得。session 不存在
    /// 時三個請求都回 404，呼叫端據此重建 session。
    async fn send_prompt(&self, session_id: &str, req: &AgentRequest) -> Result<(), GatewayError> {
        self.ensure_instructions(session_id, &req.session_instructions)
            .await?;
        let session = format!("{}/api/session/{session_id}", self.base_url);
        if let Some(agent) = req.agent.as_deref() {
            self.send_empty(
                "select_agent",
                self.client
                    .post(format!("{session}/agent"))
                    .json(&AgentBody { agent }),
            )
            .await?;
        }
        if let Some(model) = req.model.as_deref().and_then(parse_model_ref) {
            self.send_empty(
                "select_model",
                self.client
                    .post(format!("{session}/model"))
                    .json(&ModelBody { model }),
            )
            .await?;
        }
        let body = PromptBody {
            text: req.prompt.clone(),
            files: self.build_files(req).await?,
        };
        self.send_empty(
            "prompt",
            self.client.post(format!("{session}/prompt")).json(&body),
        )
        .await
    }

    /// 2026-11-26（#45 SCN-007）：確認 session 指令層有 `wanted` 的每一筆，缺少或值不同才寫入。
    /// 寫入後不再改的 entry 不會進對話紀錄；改值時 V2 會在對話紀錄插一則通知
    /// （docs/issues/issue-0045/probe-instructions.md），所以值相同時不重寫。每回合都確認，
    /// 升級前就在的 session 也會在下一個回合補上。
    async fn ensure_instructions(
        &self,
        session_id: &str,
        wanted: &BTreeMap<String, String>,
    ) -> Result<(), GatewayError> {
        if wanted.is_empty() {
            return Ok(());
        }
        let base = format!(
            "{}/api/experimental/session/{session_id}/instructions/entries",
            self.base_url
        );
        let listed = self
            .send_json("instruction_entries", self.client.get(&base))
            .await?;
        let current = |key: &str| {
            listed
                .get("data")
                .and_then(Value::as_array)
                .and_then(|entries| entries.iter().find(|e| e["key"] == key))
                .and_then(|entry| entry["value"].as_str().map(str::to_string))
        };
        for (key, value) in wanted {
            if current(key).as_deref() == Some(value.as_str()) {
                continue;
            }
            self.send_empty(
                "instruction_entry",
                self.client
                    .put(format!("{base}/{key}"))
                    .json(&serde_json::json!({ "value": value })),
            )
            .await?;
        }
        Ok(())
    }

    /// 回答一則提問（V2 的 form）。`request_id` 為 form ID；`answers` 依題目順序，每題為
    /// 選到的選項標籤或自訂文字。
    pub async fn reply_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        let fields = self
            .forms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .get(request_id)
            .map(|(_, fields)| fields.clone());
        let body = serde_json::json!({ "answer": form_answer(fields.as_deref(), answers) });
        let url = form_url(&self.base_url, session_id, request_id);
        self.send_empty(
            "question_reply",
            self.client.post(format!("{url}/reply")).json(&body),
        )
        .await
    }

    /// 取消一則提問：V2 沒有 reject，刪除 form 後提問工具以「使用者略過」失敗，執行結束。
    pub async fn reject_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError> {
        let url = form_url(&self.base_url, session_id, request_id);
        self.send_empty("question_reject", self.client.delete(url))
            .await
    }

    pub async fn reply_permission(
        &self,
        session_id: &str,
        request_id: &str,
        reply: &str,
    ) -> Result<(), GatewayError> {
        if !matches!(reply, "once" | "always" | "reject") {
            return Err(GatewayError::AgentFailed {
                code: None,
                stderr: format!("無效的 OpenCode permission reply：{reply}"),
            });
        }
        let url = permission_reply_url(&self.base_url, session_id, request_id);
        self.send_empty(
            "permission_reply",
            self.client
                .post(url)
                .json(&PermissionReplyBody { decision: reply }),
        )
        .await
    }

    /// 記下本 session 的 form 欄位，回覆時才知道 key、選項的 value 與是否多選。
    fn remember_form(&self, session_id: &str, value: &Value) {
        let Some(form) = value.get("data").and_then(|data| data.get("form")) else {
            return;
        };
        if form.get("sessionID").and_then(Value::as_str) != Some(session_id) {
            return;
        }
        let Some(id) = form.get("id").and_then(Value::as_str) else {
            return;
        };
        let fields = form
            .get("fields")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|field| {
                Some(FormField {
                    key: field.get("key")?.as_str()?.to_string(),
                    multiselect: field.get("type").and_then(Value::as_str) == Some("multiselect"),
                    options: field
                        .get("options")
                        .and_then(Value::as_array)
                        .into_iter()
                        .flatten()
                        .filter_map(|option| {
                            let value = option.get("value")?.as_str()?.to_string();
                            let label = option
                                .get("label")
                                .and_then(Value::as_str)
                                .map(str::to_string)
                                .unwrap_or_else(|| value.clone());
                            Some((label, value))
                        })
                        .collect(),
                })
            })
            .collect();
        self.forms
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .insert(id.to_string(), (session_id.to_string(), fields));
    }

    /// 回合的 future 在完成前被丟掉（入口取消、排程中止）時，V2 的執行仍會繼續佔住 session，
    /// 下一則 prompt 也會被併進這次執行；所以 drop 時在背景送 interrupt。
    fn interrupt_on_drop(&self, session_id: &str) -> InterruptOnDrop {
        InterruptOnDrop {
            server: Some(self.server_handle()),
            sessions: vec![session_id.to_string()],
        }
    }

    fn server_handle(&self) -> ServerHandle {
        ServerHandle {
            base_url: self.base_url.clone(),
            client: self.client.clone(),
            password: self.password.clone(),
            jobs_lookup_timeout: self.jobs_lookup_timeout,
        }
    }

    /// 2026-11-26（#45）：這個 session 與它的子 session 還有沒有 subagent 工作（執行中，或回報還沒被
    /// 讀取）。subagent 一律在背景執行：primary 派工後先結束一次執行，收到回報再執行一次，
    /// 清單裡沒有 subagent 了回合才真的結束（docs/issues/issue-0045/probe-subagent.md）。
    ///
    /// 只看 `kind` 為 subagent 的工作（2026-11-27，review-65e960a）：清單也列出 subagent 開的背景
    /// shell，結束後沒被讀取的也留著；`npm run dev` 這類不會自己結束的程式會讓回合等到逾時。
    ///
    /// 每次查詢最多等 [`JOBS_LOOKUP_TIMEOUT`]。失敗時隔 [`JOBS_LOOKUP_RETRY_DELAY`] 重試，最多查 [`JOBS_LOOKUP_ATTEMPTS`] 次（PR #47
    /// 第三方審查 P2-1）。都失敗才當作沒有，回合以目前的回覆收尾：這個查詢是事件之外的補漏，
    /// 不因為它查不到就丟掉 primary 已經寫好的回覆。
    async fn has_background_jobs(&self, session_id: &str) -> bool {
        let server = self.server_handle();
        for attempt in 1..=JOBS_LOOKUP_ATTEMPTS {
            match server.background_jobs(session_id).await {
                Ok(value) => return subagent_jobs(&value).next().is_some(),
                Err(err) => {
                    eprintln!(
                        "background_jobs_lookup_failed session_id={session_id} attempt={attempt}/{JOBS_LOOKUP_ATTEMPTS}: {err}"
                    );
                    if attempt < JOBS_LOOKUP_ATTEMPTS {
                        tokio::time::sleep(JOBS_LOOKUP_RETRY_DELAY).await;
                    }
                }
            }
        }
        false
    }

    async fn open_event_stream(&self) -> Result<reqwest::Response, GatewayError> {
        let url = format!("{}/api/event", self.base_url);
        let response = self
            .authorize(self.stream_client.get(url))
            .send()
            .await
            .map_err(|err| http_error("event_stream open", err))?;
        let status = response.status();
        if !status.is_success() {
            let text = response
                .text()
                .await
                .map_err(|err| http_error("event_stream error response body", err))?;
            return Err(GatewayError::AgentFailed {
                code: Some(status.as_u16() as i32),
                stderr: format!("opencode server event_stream returned {status}: {text}"),
            });
        }
        Ok(response)
    }

    async fn send_json(
        &self,
        phase: &'static str,
        request: reqwest::RequestBuilder,
    ) -> Result<Value, GatewayError> {
        request_json(phase, self.authorize(request)).await
    }

    async fn send_empty(
        &self,
        phase: &'static str,
        request: reqwest::RequestBuilder,
    ) -> Result<(), GatewayError> {
        let request = self.authorize(request);
        let response = request.send().await.map_err(|err| http_error(phase, err))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|err| http_error(phase, err))?;
        if !status.is_success() {
            return Err(GatewayError::AgentFailed {
                code: Some(status.as_u16() as i32),
                stderr: format!("opencode server {phase} returned {status}: {text}"),
            });
        }
        Ok(())
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        authorize(request, self.password.as_deref())
    }

    /// 等 server 上這次執行（回合或壓縮，兩者都是排進 session 佇列的項目）結束。future 被丟掉時
    /// 在背景送 interrupt，失敗時直接送，不讓 V2 繼續執行、佔住 session。
    async fn consume_or_interrupt(
        &self,
        response: reqwest::Response,
        session_id: &str,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<String, GatewayError> {
        let mut interrupt = self.interrupt_on_drop(session_id);
        let consumed = self
            .consume_event_stream(response, session_id, on_event, &mut interrupt)
            .await;
        if consumed.is_err() {
            interrupt.send_now().await;
        }
        interrupt.disarm();
        consumed
    }

    /// 逾時或斷線時用來說明「卡在哪裡」。單看「before session became idle」
    /// 分不出是模型還在跑、工具還在跑，還是有一則沒人回覆的權限詢問——本次
    /// 事故就是後者被誤讀成前者（見 docs/2026-08-06-docker-runtime-handover.md）。
    async fn consume_event_stream(
        &self,
        mut response: reqwest::Response,
        session_id: &str,
        on_event: &mut dyn FnMut(StreamEvent),
        interrupt: &mut InterruptOnDrop,
    ) -> Result<String, GatewayError> {
        let mut parser = SseParser::default();
        // 本回合（含子 session 再派出去的）subagent 的 session。
        let mut children: Vec<String> = Vec::new();
        // 回合是否還有後續（#45）：還在執行的子 session，以及 primary 的 inbox 有沒有還沒送進
        // 執行的項目（fixer 的回報）。真實 V2 有時在 primary 第一次執行結束時查背景工作拿到空清單，
        // fixer 卻還在跑（2026-11-26 兩次），所以先看事件，事件看不出來才查清單。
        let mut running_children: Vec<String> = Vec::new();
        let mut inbox_pending = false;
        // 以 bytes 緩衝、只在整行（不會切在多位元組字元中間）時才解碼，
        // 避免 chunk 邊界落在 UTF-8 字元中間時產生 U+FFFD 亂碼。
        let mut buffer: Vec<u8> = Vec::new();
        let mut mapper =
            ServerEventMapper::new(session_id, RetryWatch::new(self.retry_give_up_after));
        let mut progress = StreamProgress::default();
        let deadline = tokio::time::sleep(agent_timeout());
        tokio::pin!(deadline);
        let _forget_forms = ForgetForms {
            forms: &self.forms,
            session_id,
        };

        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk
                    .map_err(|err| http_error("event_stream failed while reading chunk", err))?,
                _ = &mut deadline => {
                    return Err(GatewayError::AgentFailed {
                        code: None,
                        stderr: format!(
                            "opencode server stream timed out before session became idle ({})",
                            progress.describe()
                        ),
                    });
                }
                // V2 對上游錯誤會一直重試、不結束執行；呼叫端收到錯誤後會送 interrupt。
                () = mapper.retry.expired() => return Err(mapper.retry.give_up()),
            };
            let Some(chunk) = chunk else {
                return Err(GatewayError::AgentFailed {
                    code: None,
                    stderr: format!(
                        "opencode server event stream ended before session became idle ({})",
                        progress.describe()
                    ),
                });
            };
            buffer.extend_from_slice(&chunk);
            while let Some(newline) = buffer.iter().position(|byte| *byte == b'\n') {
                let line_bytes: Vec<u8> = buffer.drain(..=newline).collect();
                let mut line = String::from_utf8_lossy(&line_bytes).into_owned();
                if line.ends_with('\n') {
                    line.pop();
                }
                if let Some(data) = parser.feed_line(&line) {
                    let value: Value = match serde_json::from_str(&data) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    let event_type = event_type_of(&value).to_string();
                    if event_type == "form.created" {
                        self.remember_form(session_id, &value);
                    }
                    if event_type == "session.created" {
                        let data = value.get("data");
                        let parent = data.and_then(|d| d.get("parentID")).and_then(Value::as_str);
                        let child = data
                            .and_then(|d| d.get("sessionID"))
                            .and_then(Value::as_str);
                        if let (Some(parent), Some(child)) = (parent, child) {
                            if parent == session_id || children.iter().any(|c| c == parent) {
                                interrupt.add(child);
                                children.push(child.to_string());
                            }
                        }
                    }
                    let event_session = value
                        .get("data")
                        .and_then(|d| d.get("sessionID"))
                        .and_then(Value::as_str);
                    match (event_type.as_str(), event_session) {
                        ("session.execution.started", Some(s))
                            if children.iter().any(|c| c == s) =>
                        {
                            if !running_children.iter().any(|c| c == s) {
                                running_children.push(s.to_string());
                            }
                        }
                        (
                            "session.execution.succeeded"
                            | "session.execution.failed"
                            | "session.execution.interrupted",
                            Some(s),
                        ) => running_children.retain(|c| c != s),
                        ("session.inbox.enqueued", Some(s)) if s == session_id => {
                            inbox_pending = true;
                        }
                        ("session.inbox.delivered", Some(s)) if s == session_id => {
                            inbox_pending = false;
                        }
                        _ => {}
                    }
                    match mapper.map(&value) {
                        ServerEventAction::Emit(event) => {
                            progress.record(&event_type, &event);
                            on_event(event);
                        }
                        ServerEventAction::Idle => {
                            if !running_children.is_empty()
                                || inbox_pending
                                || self.has_background_jobs(session_id).await
                            {
                                continue;
                            }
                            return Ok(mapper.final_text());
                        }
                        ServerEventAction::Failed(error) => {
                            return Err(error.to_gateway_error());
                        }
                        ServerEventAction::Ignore => {}
                    }
                }
            }
        }
    }
}

impl AiBackend for OpencodeServerBackend {
    fn supports_session_instructions(&self) -> bool {
        true
    }

    /// 輪詢 `GET /api/agent`，直到 `agent` 的 `system` 與 `system` 相同（頭尾空白不算）。
    /// 實測清單比 prompt 更晚更新（Task 4.2 的 probe-agent-reload.sh），清單有了，prompt
    /// 一定也用得上。請求失敗時當作還沒載入，繼續等到逾時。
    async fn wait_for_agent_system(
        &self,
        agent: &str,
        system: &str,
        timeout: std::time::Duration,
    ) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        let url = format!("{}/api/agent", self.base_url);
        loop {
            if let Ok(listed) = self.send_json("list_agents", self.client.get(&url)).await {
                let loaded = listed
                    .get("data")
                    .and_then(Value::as_array)
                    .and_then(|agents| agents.iter().find(|a| a["id"] == agent))
                    .and_then(|a| a["system"].as_str())
                    .is_some_and(|loaded| loaded.trim() == system.trim());
                if loaded {
                    return true;
                }
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(AGENT_POLL_INTERVAL).await;
        }
    }

    /// V2 的 prompt 一律非同步，沒有等到回覆才回應的端點；非串流的回合也走事件串流。
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        self.run_streaming(req, &mut |_| {}).await
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        self.health_check().await?;

        let mut cleanup_on_error = req.session_id.is_none();
        let mut session_id = match req.session_id.clone() {
            Some(id) => id,
            None => self.create_session(&req).await?,
        };

        let response = match self.open_event_stream().await {
            Ok(response) => response,
            Err(error) => {
                self.cleanup_session_if_owned(cleanup_on_error, &session_id)
                    .await;
                return Err(error);
            }
        };
        match self.send_prompt(&session_id, &req).await {
            Ok(()) => self.remember_attachments(&session_id, &req),
            Err(GatewayError::AgentFailed {
                code: Some(code), ..
            }) if code == StatusCode::NOT_FOUND.as_u16() as i32 => {
                let retry = self.rehydrate_request(&session_id, &req);
                self.cleanup_session_if_owned(cleanup_on_error, &session_id)
                    .await;
                session_id = self.create_session(&retry).await?;
                cleanup_on_error = true;
                if let Err(error) = self.send_prompt(&session_id, &retry).await {
                    self.cleanup_session_if_owned(true, &session_id).await;
                    return Err(error);
                }
                self.remember_attachments(&session_id, &retry);
            }
            Err(err) => {
                self.cleanup_session_if_owned(cleanup_on_error, &session_id)
                    .await;
                return Err(err);
            }
        }

        match self
            .consume_or_interrupt(response, &session_id, on_event)
            .await
        {
            Ok(text) => Ok(AgentResponse {
                text,
                session_id: Some(session_id),
            }),
            Err(err) => {
                self.cleanup_session_if_owned(cleanup_on_error, &session_id)
                    .await;
                Err(err)
            }
        }
    }

    /// V2 的壓縮是排進 session 佇列的一個項目，用 session 目前的模型執行：閒置時立即
    /// 開始，事件為 execution.started → compaction.started／ended → execution.succeeded
    /// （2026-11-24 實測），所以與回合一樣等事件結束。`model` 不再需要。
    async fn compact_session(
        &self,
        session_id: &str,
        _model: Option<&str>,
    ) -> Result<AgentResponse, GatewayError> {
        self.health_check().await?;
        let response = self.open_event_stream().await?;
        let url = format!("{}/api/session/{session_id}/compact", self.base_url);
        self.send_json(
            "compact",
            self.client.post(url).json(&serde_json::json!({})),
        )
        .await?;
        self.consume_or_interrupt(response, session_id, &mut |_| {})
            .await?;
        eprintln!("session_compacted session_id={session_id}");
        Ok(AgentResponse {
            text: String::new(),
            session_id: Some(session_id.to_string()),
        })
    }

    async fn delete_session(&self, session_id: &str) -> Result<(), GatewayError> {
        let url = format!("{}/api/session/{session_id}", self.base_url);
        let result = self
            .send_empty("delete_session", self.client.delete(url))
            .await;
        match result {
            Ok(()) => {}
            Err(GatewayError::AgentFailed {
                code: Some(code), ..
            }) if code == StatusCode::NOT_FOUND.as_u16() as i32 => {}
            Err(error) => return Err(error),
        }
        self.session_attachments
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .remove(session_id);
        eprintln!("session_deleted session_id={session_id}");
        Ok(())
    }

    async fn session_created_ms(&self, session_id: &str) -> Result<i64, GatewayError> {
        let url = format!("{}/api/session/{session_id}", self.base_url);
        let value = self
            .send_json("session_created_ms", self.client.get(url))
            .await?;
        value
            .pointer("/data/time/created")
            .and_then(Value::as_i64)
            .ok_or_else(|| GatewayError::AgentFailed {
                code: None,
                stderr: format!(
                    "opencode server session_created_ms: session {session_id} has no integer time.created"
                ),
            })
    }

    async fn list_sessions(&self) -> Result<SessionListing, GatewayError> {
        // opencode 沒有「早於某時間」的過濾，所以整批取回再由呼叫端挑；列表由新到舊，
        // 只取一頁的話被截掉的正是最舊、最該清的那些。子 session 也要：呼叫端靠它們
        // 判斷哪個根 session 底下還有人接著。V2 在最後一頁之後仍給 `cursor.next`，
        // 再取一次才是空頁（2026-11-24 實測），兩種結尾都要認。
        let url = format!("{}/api/session", self.base_url);
        let mut sessions = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..SESSION_LIST_MAX_PAGES {
            let mut query = vec![("limit", SESSION_PAGE_SIZE.to_string())];
            if let Some(cursor) = cursor.take() {
                query.push(("cursor", cursor));
            }
            let value = self
                .send_json("list_sessions", self.client.get(&url).query(&query))
                .await?;
            let (page, next) = parse_session_page(&value)?;
            let last = page.is_empty() || next.is_none();
            sessions.extend(page);
            if last {
                return Ok(SessionListing {
                    sessions,
                    truncated: false,
                });
            }
            cursor = next;
        }
        Ok(SessionListing {
            sessions,
            truncated: true,
        })
    }

    async fn run_ephemeral(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        self.run_streaming_ephemeral(req, &mut |_| {}).await
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
        Ok(AgentResponse {
            session_id: None,
            ..response
        })
    }
}

impl FileTransportMode {
    fn from_env() -> Self {
        match std::env::var("WUKONG_AGENT_SERVER_FILE_MODE") {
            Ok(value) => match value.trim().to_ascii_lowercase().as_str() {
                "" | "shared" => Self::Shared,
                "inline" => Self::Inline,
                "disabled" => Self::Disabled,
                _ => Self::Invalid(value),
            },
            Err(_) => Self::Shared,
        }
    }
}

fn attachment_error(message: impl Into<String>) -> GatewayError {
    GatewayError::AgentFailed {
        code: None,
        stderr: message.into(),
    }
}

fn safe_display_filename(original_name: &str, path: &Path) -> String {
    Path::new(original_name)
        .file_name()
        .filter(|name| !name.is_empty())
        .or_else(|| path.file_name())
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "attachment".to_string())
}

fn supported_attachment_mime(
    attachment: &crate::backend::AgentAttachment,
) -> Result<String, GatewayError> {
    let declared = attachment
        .mime_type
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("");
    match declared.to_ascii_lowercase().as_str() {
        "image/png" | "image/jpeg" | "image/gif" | "image/webp" | "application/pdf" => {
            return Ok(declared.to_ascii_lowercase());
        }
        value
            if value.starts_with("text/")
                || matches!(
                    value,
                    "application/json"
                        | "application/ld+json"
                        | "application/toml"
                        | "application/yaml"
                        | "application/x-yaml"
                        | "application/xml"
                ) =>
        {
            return Ok("text/plain".to_string());
        }
        _ => {}
    }

    let extension = attachment
        .path
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let inferred = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "pdf" => "application/pdf",
        "txt" | "md" | "markdown" | "csv" | "tsv" | "json" | "jsonl" | "toml" | "yaml" | "yml"
        | "xml" | "html" | "css" | "js" | "jsx" | "ts" | "tsx" | "rs" | "py" | "rb" | "go"
        | "java" | "kt" | "kts" | "c" | "h" | "cc" | "cpp" | "hpp" | "sh" | "bash" | "zsh"
        | "fish" | "sql" | "r" => "text/plain",
        _ => {
            return Err(attachment_error(format!(
                "不支援的附件格式：{}（MIME: {}）",
                attachment.original_name,
                if declared.is_empty() {
                    "unknown"
                } else {
                    declared
                }
            )));
        }
    };
    Ok(inferred.to_string())
}

fn extract_session_id(value: &Value) -> Option<String> {
    value
        .pointer("/data/id")
        .and_then(Value::as_str)
        .map(str::to_string)
}

/// 一頁的 session 與下一頁的 cursor。任何一筆看不懂就整份失敗：這份清單決定哪些
/// session 會被刪，寧可這一輪不清，也不要拿一份殘缺的清單去比對。
fn parse_session_page(
    value: &Value,
) -> Result<(Vec<SessionSummary>, Option<String>), GatewayError> {
    let malformed = |detail: String| GatewayError::AgentFailed {
        code: None,
        stderr: format!("opencode server list_sessions returned an unexpected payload: {detail}"),
    };
    let items = value.get("data").and_then(Value::as_array).ok_or_else(|| {
        let shown: String = value.to_string().chars().take(200).collect();
        malformed(format!("no data array: {shown}"))
    })?;
    let sessions = items
        .iter()
        .map(|item| {
            let id = item.get("id").and_then(Value::as_str);
            let updated_ms = item
                .get("time")
                .and_then(|time| time.get("updated"))
                .and_then(Value::as_i64);
            match (id, updated_ms) {
                (Some(id), Some(updated_ms)) => Ok(SessionSummary {
                    id: id.to_string(),
                    updated_ms,
                    parent_id: item
                        .get("parentID")
                        .and_then(Value::as_str)
                        .map(str::to_string),
                }),
                // 只說得出是哪一筆就好：整筆 session 帶著標題，不該進日誌。
                (Some(id), None) => Err(malformed(format!("session {id} has no time.updated"))),
                (None, _) => Err(malformed("a session has no id".to_string())),
            }
        })
        .collect::<Result<Vec<_>, _>>()?;
    let next = value
        .pointer("/cursor/next")
        .and_then(Value::as_str)
        .map(str::to_string);
    Ok((sessions, next))
}

/// `provider/model` 轉成 V2 的 `Model.Ref`；寫法不對時不換模型，與 V1 相同。
fn parse_model_ref(model: &str) -> Option<ModelRef> {
    let (provider_id, id) = model.trim().split_once('/')?;
    if provider_id.is_empty() || id.is_empty() {
        return None;
    }
    Some(ModelRef {
        id: id.to_string(),
        provider_id: provider_id.to_string(),
    })
}

fn http_error(phase: &'static str, err: reqwest::Error) -> GatewayError {
    // reqwest 0.12 的 Display 只有「error decoding response body」這類頂層訊息，
    // 逾時、連線被 reset 等真正原因都藏在 source 鏈裡，須自行展開。
    let mut stderr = format!("opencode server {phase} failed: {err}");
    let mut source = std::error::Error::source(&err);
    while let Some(cause) = source {
        stderr.push_str(": ");
        stderr.push_str(&cause.to_string());
        source = cause.source();
    }
    GatewayError::AgentFailed { code: None, stderr }
}

/// 入口的答案轉成 V2 的 `Form.Answer`。選到的標籤換成選項的 value，對不到的視為自訂文字；
/// 多選欄位送陣列，其他欄位送第一個答案。沒看過這則 form 時（例如事件串流已斷）以 V2
/// 題目的預設鍵 `q0`、`q1`… 送文字。
fn form_answer(
    fields: Option<&[FormField]>,
    answers: Vec<Vec<String>>,
) -> serde_json::Map<String, Value> {
    answers
        .into_iter()
        .enumerate()
        .map(|(index, selected)| {
            let Some(field) = fields.and_then(|fields| fields.get(index)) else {
                return (
                    format!("q{index}"),
                    Value::String(selected.into_iter().next().unwrap_or_default()),
                );
            };
            let values: Vec<String> = selected
                .into_iter()
                .map(|answer| {
                    field
                        .options
                        .iter()
                        .find(|(label, _)| *label == answer)
                        .map(|(_, value)| value.clone())
                        .unwrap_or(answer)
                })
                .collect();
            let value = if field.multiselect {
                Value::from(values)
            } else {
                Value::String(values.into_iter().next().unwrap_or_default())
            };
            (field.key.clone(), value)
        })
        .collect()
}

/// 本次串流看到多少進度，只用於組錯誤訊息。
#[derive(Default)]
struct StreamProgress {
    events: usize,
    last_event: Option<(String, std::time::Instant)>,
    pending_requests: Vec<String>,
}

impl StreamProgress {
    fn record(&mut self, event_type: &str, event: &StreamEvent) {
        self.events += 1;
        self.last_event = Some((event_type.to_string(), std::time::Instant::now()));
        if let StreamEvent::QuestionRequest(request) = event {
            if !self.pending_requests.contains(&request.request_id) {
                self.pending_requests.push(request.request_id.clone());
            }
        }
    }

    /// 只陳述這條 stream 觀測得到的事實：待決的詢問可能已由別的路徑回覆，
    /// 回覆本身不會回到這條 stream 上，所以措辭是「沒看到回覆」而非「未回覆」。
    fn describe(&self) -> String {
        let mut parts = Vec::new();
        match &self.last_event {
            Some((event_type, at)) => parts.push(format!(
                "events seen: {}, last {event_type} {}s ago",
                self.events,
                at.elapsed().as_secs()
            )),
            None => parts.push("no events arrived for this session".to_string()),
        }
        if !self.pending_requests.is_empty() {
            parts.push(format!(
                "no reply observed for pending request(s): {}",
                self.pending_requests.join(", ")
            ));
        }
        parts.join("; ")
    }
}

fn form_url(base_url: &str, session_id: &str, form_id: &str) -> String {
    format!("{base_url}/api/session/{session_id}/form/{form_id}")
}

fn permission_reply_url(base_url: &str, session_id: &str, request_id: &str) -> String {
    format!("{base_url}/api/session/{session_id}/permission/{request_id}/reply")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AgentAttachment;
    use serde_json::json;
    use std::collections::BTreeMap;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    async fn malformed_chunked_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\nnot-a-chunk\r\n")
                .await
                .unwrap();
        });
        format!("http://{addr}")
    }

    fn agent_failed_stderr(err: GatewayError) -> String {
        match err {
            GatewayError::AgentFailed { stderr, .. } => stderr,
            other => panic!("expected AgentFailed, got {other:?}"),
        }
    }

    /// SSE server 送出 headers 後保持連線開啟但不再送任何事件。
    async fn silent_sse_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            socket
                .write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n: keep-alive\n\n",
                )
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });
        format!("http://{addr}")
    }

    /// 送一則事件、緊接著送 execution.succeeded。V1 實測過錯誤之後還會收到 idle
    /// （2026-09-08），讀到結束就回 Ok 會把失敗的回合當成正常結束；V2 換了事件名稱，
    /// 失敗事件仍要先於後面的成功事件生效。
    async fn error_then_idle_sse_server(error_json: &'static str) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            socket
                .write_all(format!("data: {error_json}\n\n").as_bytes())
                .await
                .unwrap();
            socket
                .write_all(
                    b"data: {\"type\":\"session.execution.succeeded\",\"data\":{\"sessionID\":\"ses_1\"}}\n\n",
                )
                .await
                .unwrap();
            socket.flush().await.unwrap();
        });
        format!("http://{addr}")
    }

    fn upstream_failure_kind(error: GatewayError) -> crate::upstream_error::UpstreamFailure {
        match error {
            GatewayError::UpstreamFailed { kind, .. } => kind,
            other => panic!("expected UpstreamFailed, got {other:?}"),
        }
    }

    /// SSE server 把一個含中文 reasoning 的事件拆在多位元組字元中間分兩段送出，
    /// 之後送 execution.succeeded 正常收尾。
    async fn utf8_splitting_sse_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let reasoning = concat!(
                "data: {\"type\":\"session.reasoning.delta\",\"data\":{",
                "\"sessionID\":\"ses_1\",\"assistantMessageID\":\"msg_1\",\"delta\":\"悟空登場\"}}\n\n"
            );
            let idle =
                "data: {\"type\":\"session.execution.succeeded\",\"data\":{\"sessionID\":\"ses_1\"}}\n\n";
            let bytes = reasoning.as_bytes();
            let split = reasoning.find('悟').unwrap() + 1;
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n")
                .await
                .unwrap();
            socket.write_all(&bytes[..split]).await.unwrap();
            socket.flush().await.unwrap();
            tokio::time::sleep(std::time::Duration::from_millis(80)).await;
            socket.write_all(&bytes[split..]).await.unwrap();
            socket.write_all(idle.as_bytes()).await.unwrap();
            socket.flush().await.unwrap();
        });
        format!("http://{addr}")
    }

    #[tokio::test]
    async fn event_stream_outlives_client_timeout_and_reports_friendly_deadline() {
        let _guard = crate::backend::AGENT_TIMEOUT_ENV_LOCK.lock().await;
        std::env::set_var("WUKONG_AGENT_TIMEOUT_SECS", "1");
        let backend = OpencodeServerBackend::from_env(silent_sse_server().await, None);

        let response = backend.open_event_stream().await.unwrap();
        let result = backend
            .consume_event_stream(
                response,
                "ses_1",
                &mut |_| {},
                &mut InterruptOnDrop::default(),
            )
            .await;
        std::env::remove_var("WUKONG_AGENT_TIMEOUT_SECS");

        let stderr = agent_failed_stderr(result.unwrap_err());
        assert!(
            stderr.contains("stream timed out before session became idle"),
            "{stderr}"
        );
    }

    /// SSE server 送出一則 permission.asked 後就不再前進，模擬無人回覆權限詢問。
    async fn pending_permission_sse_server() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0; 1024];
            let _ = socket.read(&mut buf).await.unwrap();
            let event = concat!(
                "data: {\"type\":\"permission.asked\",\"data\":{",
                "\"id\":\"per_1\",\"sessionID\":\"ses_1\",\"action\":\"external_directory\",",
                "\"resources\":[\"/tmp/*\"]}}\n\n"
            );
            socket
                .write_all(
                    format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n{event}"
                    )
                    .as_bytes(),
                )
                .await
                .unwrap();
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        });
        format!("http://{addr}")
    }

    /// 逾時訊息必須指出卡住的原因。事故當下只看得到「before session became
    /// idle」，讀起來像模型逾時，實際上是沒人回覆的權限詢問。
    #[tokio::test]
    async fn stream_timeout_names_the_pending_permission_request() {
        let _guard = crate::backend::AGENT_TIMEOUT_ENV_LOCK.lock().await;
        std::env::set_var("WUKONG_AGENT_TIMEOUT_SECS", "1");
        let backend = OpencodeServerBackend::from_env(pending_permission_sse_server().await, None);

        let response = backend.open_event_stream().await.unwrap();
        let result = backend
            .consume_event_stream(
                response,
                "ses_1",
                &mut |_| {},
                &mut InterruptOnDrop::default(),
            )
            .await;
        std::env::remove_var("WUKONG_AGENT_TIMEOUT_SECS");

        let stderr = agent_failed_stderr(result.unwrap_err());
        assert!(stderr.contains("permission.asked"), "{stderr}");
        assert!(stderr.contains("permission-per_1"), "{stderr}");
        assert!(stderr.contains("no reply observed"), "{stderr}");
    }

    #[tokio::test]
    async fn stream_timeout_reports_when_no_events_arrived() {
        let _guard = crate::backend::AGENT_TIMEOUT_ENV_LOCK.lock().await;
        std::env::set_var("WUKONG_AGENT_TIMEOUT_SECS", "1");
        let backend = OpencodeServerBackend::from_env(silent_sse_server().await, None);

        let response = backend.open_event_stream().await.unwrap();
        let result = backend
            .consume_event_stream(
                response,
                "ses_1",
                &mut |_| {},
                &mut InterruptOnDrop::default(),
            )
            .await;
        std::env::remove_var("WUKONG_AGENT_TIMEOUT_SECS");

        let stderr = agent_failed_stderr(result.unwrap_err());
        assert!(stderr.contains("no events arrived"), "{stderr}");
        assert!(!stderr.contains("no reply observed"), "{stderr}");
    }

    /// AC-2：錯誤事件之後就算收到 idle，也不得以 Ok 收場。
    #[tokio::test]
    async fn upstream_error_event_fails_the_turn_even_when_idle_follows() {
        let url = error_then_idle_sse_server(
            r#"{"type":"session.execution.failed","data":{"sessionID":"ses_1","error":{"type":"provider.http","message":"model reached end of life","status":410}}}"#,
        )
        .await;
        let backend = OpencodeServerBackend::from_env(url, None);
        let response = backend.open_event_stream().await.unwrap();

        let result = backend
            .consume_event_stream(
                response,
                "ses_1",
                &mut |_| {},
                &mut InterruptOnDrop::default(),
            )
            .await;

        let error = result.expect_err("410 必須讓回合失敗，不能因為隨後的 idle 就當成功");
        assert_eq!(
            upstream_failure_kind(error),
            crate::upstream_error::UpstreamFailure::ModelEol
        );
    }

    /// AC-3：被中止的回合同樣不得回報成功，且分類要與下架分得開。
    #[tokio::test]
    async fn aborted_turn_does_not_report_success() {
        let url = error_then_idle_sse_server(
            r#"{"type":"session.execution.interrupted","data":{"sessionID":"ses_1","reason":"user"}}"#,
        )
        .await;
        let backend = OpencodeServerBackend::from_env(url, None);
        let response = backend.open_event_stream().await.unwrap();

        let result = backend
            .consume_event_stream(
                response,
                "ses_1",
                &mut |_| {},
                &mut InterruptOnDrop::default(),
            )
            .await;

        let kind = upstream_failure_kind(result.expect_err("中止的回合不是成功"));
        assert_eq!(kind, crate::upstream_error::UpstreamFailure::Aborted);
        assert_ne!(kind, crate::upstream_error::UpstreamFailure::ModelEol);
    }

    /// AC-2 的失敗路徑：沒有錯誤事件時，idle 仍然要正常收尾——偵測不得誤殺。
    #[tokio::test]
    async fn plain_idle_still_succeeds() {
        let url = error_then_idle_sse_server(
            r#"{"type":"session.step.started","data":{"sessionID":"ses_1","assistantMessageID":"msg_1"}}"#,
        )
        .await;
        let backend = OpencodeServerBackend::from_env(url, None);
        let response = backend.open_event_stream().await.unwrap();

        backend
            .consume_event_stream(
                response,
                "ses_1",
                &mut |_| {},
                &mut InterruptOnDrop::default(),
            )
            .await
            .expect("沒有錯誤事件時必須正常收尾");
    }

    #[tokio::test]
    async fn event_stream_preserves_utf8_split_across_chunks() {
        let backend = OpencodeServerBackend::from_env(utf8_splitting_sse_server().await, None);
        let response = backend.open_event_stream().await.unwrap();

        let mut reasoning = String::new();
        backend
            .consume_event_stream(
                response,
                "ses_1",
                &mut |event| {
                    if let StreamEvent::Reasoning(text) = event {
                        reasoning.push_str(&text);
                    }
                },
                &mut InterruptOnDrop::default(),
            )
            .await
            .unwrap();

        assert!(reasoning.contains("悟空登場"), "{reasoning:?}");
        assert!(!reasoning.contains('\u{FFFD}'), "{reasoning:?}");
    }

    fn plain_request(prompt: &str) -> AgentRequest {
        AgentRequest {
            prompt: prompt.to_string(),
            session_id: None,
            thinking: false,
            model: None,
            agent: None,
            tool_overrides: BTreeMap::new(),
            attachments: Vec::new(),
            session_instructions: Default::default(),
        }
    }

    // issue-0044 Task 2.2：以真實樣本重播一個回合。期望值取自樣本：一次 shell 呼叫後回覆 DONE。
    #[tokio::test]
    async fn run_streaming_replays_a_real_v2_turn() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));
        let mut events = Vec::new();

        let response = v2_backend(&fake)
            .run_streaming(plain_request("hi"), &mut |event| events.push(event))
            .await
            .unwrap();

        assert_eq!(response.text, "DONE");
        assert_eq!(response.session_id.as_deref(), Some("ses_fake1"));
        assert!(events.contains(&StreamEvent::ToolUse(
            "shell\n  command: echo probe".to_string()
        )));
        assert!(!events
            .iter()
            .any(|event| matches!(event, StreamEvent::Text(_))));
        assert_eq!(
            fake.requests(),
            [
                "GET /api/info",
                "POST /api/session",
                "GET /api/event",
                "POST /api/session/ses_fake1/prompt",
                // #45：執行結束時確認沒有背景工作（subagent）才收尾
                JOBS_QUERY,
            ]
        );
        assert_eq!(
            fake.body_of("POST /api/session/ses_fake1/prompt"),
            Some(json!({ "text": "hi" }))
        );
    }

    #[tokio::test]
    async fn run_waits_for_the_events_too() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));
        let response = v2_backend(&fake).run(plain_request("hi")).await.unwrap();
        assert_eq!(response.text, "DONE");
    }

    /// 真實派工樣本裡 fixer 的子 session。
    const DISPATCH_CHILD: &str = "ses_de972586effeSd8dv5TyYgP2bk";
    const JOBS_QUERY: &str = "GET /api/experimental/job?sessionID=ses_fake1&descendants=true";

    /// 派工樣本裡 primary 與 fixer 的生命週期事件位置，用來重排或拿掉事件。
    fn dispatch_event_index(
        events: &[String],
        session: &str,
        event_type: &str,
        nth: usize,
    ) -> usize {
        events
            .iter()
            .enumerate()
            .filter(|(_, e)| {
                let e: Value = serde_json::from_str(e).unwrap();
                e["type"] == event_type && e["data"]["sessionID"] == session
            })
            .nth(nth)
            .unwrap()
            .0
    }

    // #45 Task 3.1：subagent 在背景執行（docs/issues/issue-0045/probe-subagent.md）。primary 第一次
    // 執行結束時 fixer 還在跑，回覆只是「已派工」；要等 fixer 回報、primary 再執行一次才是最終回覆。
    // 2026-11-26 真實 V2 有兩次在這個時點查背景工作拿到空清單（fixer 仍在執行），所以事件裡看得到
    // 仍在執行的子 session 時就繼續等，不靠清單。
    #[tokio::test]
    async fn a_turn_waits_for_a_running_subagent_even_when_the_job_list_is_empty() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events(
            "server-dispatch-subagent",
            "ses_fake1",
        ));

        let response = v2_backend(&fake)
            .run_streaming(plain_request("派給 fixer"), &mut |_| {})
            .await
            .unwrap();

        assert!(
            response.text.starts_with("fixer 已完成，我也驗證過了"),
            "{}",
            response.text
        );
        assert!(!response.text.contains("已派給 fixer"), "{}", response.text);
        let requests = fake.requests();
        // 第一次結束時事件已說明 fixer 還在跑，不必查；只在最後一次結束時查一次
        assert_eq!(
            requests.iter().filter(|r| *r == JOBS_QUERY).count(),
            1,
            "{requests:?}"
        );
        assert!(
            !requests.iter().any(|r| r.ends_with("/interrupt")),
            "{requests:?}"
        );
    }

    // 事件串流沒有子 session 開始執行的事件時（例如串流晚接上），仍以背景工作清單判斷。
    #[tokio::test]
    async fn the_job_list_keeps_the_turn_open_when_the_events_show_no_running_subagent() {
        let fake = fake_v2::FakeV2::start().await;
        let mut events = fake_v2::fixture_events("server-dispatch-subagent", "ses_fake1");
        events.remove(dispatch_event_index(
            &events,
            DISPATCH_CHILD,
            "session.execution.started",
            0,
        ));
        fake.script_events(events);
        fake.script_jobs(vec![
            json!([{"kind": "subagent", "agent": "fixer", "status": "running"}]),
        ]);

        let response = v2_backend(&fake)
            .run_streaming(plain_request("派給 fixer"), &mut |_| {})
            .await
            .unwrap();

        assert!(
            response.text.starts_with("fixer 已完成，我也驗證過了"),
            "{}",
            response.text
        );
        assert_eq!(
            fake.requests().iter().filter(|r| *r == JOBS_QUERY).count(),
            2
        );
    }

    // fixer 的回報已放進 primary 的 inbox、還沒送進下一次執行時，primary 那次執行結束也不算回合結束。
    #[tokio::test]
    async fn a_report_waiting_in_the_inbox_keeps_the_turn_open() {
        let fake = fake_v2::FakeV2::start().await;
        let mut events = fake_v2::fixture_events("server-dispatch-subagent", "ses_fake1");
        // primary 第一次執行結束移到 fixer 結束、回報進 inbox 之後
        let first_end =
            dispatch_event_index(&events, "ses_fake1", "session.execution.succeeded", 0);
        let ended = events.remove(first_end);
        let enqueued = dispatch_event_index(&events, "ses_fake1", "session.inbox.enqueued", 1);
        events.insert(enqueued + 1, ended);
        fake.script_events(events);

        let response = v2_backend(&fake)
            .run_streaming(plain_request("派給 fixer"), &mut |_| {})
            .await
            .unwrap();

        assert!(
            response.text.starts_with("fixer 已完成，我也驗證過了"),
            "{}",
            response.text
        );
    }

    #[tokio::test]
    async fn a_turn_without_background_work_ends_at_the_first_execution() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));

        let response = v2_backend(&fake)
            .run_streaming(plain_request("hi"), &mut |_| {})
            .await
            .unwrap();

        assert_eq!(response.text, "DONE");
        assert_eq!(
            fake.requests().iter().filter(|r| *r == JOBS_QUERY).count(),
            1
        );
    }

    // #45 審查 review-65e960a SHOULD FIX 1（2026-11-27）：V2 的清單也列出 subagent 開的背景 shell，
    // 而且結束後還沒被讀取的工作也留在清單裡（Job.Kind 有 shell、subagent、image、codemode）。fixer 開了
    // 不會自己結束的程式（例如 `npm run dev`）時，回合不能因此一直等到逾時；只有 subagent 讓回合繼續。
    #[tokio::test]
    async fn background_shells_of_a_subagent_do_not_keep_the_turn_open() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));
        fake.script_jobs(vec![json!([
            {"kind": "shell", "status": "running", "command": "npm run dev"},
            {"kind": "image", "status": "completed"}
        ])]);

        let response = v2_backend(&fake)
            .run_streaming(plain_request("hi"), &mut |_| {})
            .await
            .unwrap();

        assert_eq!(response.text, "DONE");
        assert_eq!(
            fake.requests().iter().filter(|r| *r == JOBS_QUERY).count(),
            1
        );
    }

    // 第三方審查 P2-1（PR #47，2026-11-27）：查背景工作失敗時不能直接當作沒有，否則 fixer 還在跑、
    // 事件又看不出來時，回合會以「已派工」收尾。先重試，重試都失敗才照舊收尾。
    #[tokio::test]
    async fn a_failed_job_lookup_is_retried_before_the_turn_ends() {
        // 重試間隔讓這個測試跑約 1 秒；別的測試會暫時把回合逾時設成 1 秒，拿同一把鎖錯開
        let _guard = crate::backend::AGENT_TIMEOUT_ENV_LOCK.lock().await;
        let fake = fake_v2::FakeV2::start().await;
        let mut events = fake_v2::fixture_events("server-dispatch-subagent", "ses_fake1");
        events.remove(dispatch_event_index(
            &events,
            DISPATCH_CHILD,
            "session.execution.started",
            0,
        ));
        fake.script_events(events);
        fake.script_jobs(vec![
            json!("fail"),
            json!("fail"),
            json!([{"kind": "subagent", "agent": "fixer", "status": "running"}]),
        ]);

        let response = v2_backend(&fake)
            .run_streaming(plain_request("派給 fixer"), &mut |_| {})
            .await
            .unwrap();

        assert!(
            response.text.starts_with("fixer 已完成，我也驗證過了"),
            "{}",
            response.text
        );
        // 第一次結束：失敗兩次、第三次看到 fixer；最後一次結束再查一次
        assert_eq!(
            fake.requests().iter().filter(|r| *r == JOBS_QUERY).count(),
            4
        );
    }

    #[tokio::test]
    async fn a_job_lookup_that_keeps_failing_ends_the_turn_with_the_current_reply() {
        // 重試間隔讓這個測試跑約 1 秒；別的測試會暫時把回合逾時設成 1 秒，拿同一把鎖錯開
        let _guard = crate::backend::AGENT_TIMEOUT_ENV_LOCK.lock().await;
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));
        fake.script_jobs(vec![json!("fail"), json!("fail"), json!("fail")]);

        let response = v2_backend(&fake)
            .run_streaming(plain_request("hi"), &mut |_| {})
            .await
            .unwrap();

        assert_eq!(response.text, "DONE");
        assert_eq!(
            fake.requests().iter().filter(|r| *r == JOBS_QUERY).count(),
            3
        );
    }

    // review-364cebf NICE 1（2026-11-27）：V2 卡住、不斷線也不回應時，每次查詢有自己的短逾時，
    // 不沿用整個回合的 client 逾時，重試才不會把延遲放大好幾倍。
    #[tokio::test]
    async fn a_job_lookup_that_hangs_times_out_on_its_own() {
        let _guard = crate::backend::AGENT_TIMEOUT_ENV_LOCK.lock().await;
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));
        fake.script_jobs(vec![json!("hang"), json!("hang"), json!("hang")]);
        let mut backend = v2_backend(&fake);
        backend.jobs_lookup_timeout = std::time::Duration::from_millis(200);

        let response = tokio::time::timeout(
            std::time::Duration::from_secs(10),
            backend.run_streaming(plain_request("hi"), &mut |_| {}),
        )
        .await
        .expect("each lookup should give up on its own")
        .unwrap();

        assert_eq!(response.text, "DONE");
        assert_eq!(
            fake.requests().iter().filter(|r| *r == JOBS_QUERY).count(),
            3
        );
    }

    // review-794daea SHOULD FIX 1（2026-11-27）：查詢失敗的原因要留在 log 裡。reqwest 的 Display 只有
    // 「error sending request」，逾時、連線被拒都長這樣，要展開 source 鏈才分得出來（`http_error`）。
    #[tokio::test]
    async fn a_failed_job_lookup_says_why() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_jobs(vec![json!("hang")]);
        let mut backend = v2_backend(&fake);
        backend.jobs_lookup_timeout = std::time::Duration::from_millis(100);

        let err = backend
            .server_handle()
            .background_jobs("ses_fake1")
            .await
            .unwrap_err()
            .to_string();

        assert!(err.contains("background_jobs"), "{err}");
        assert!(err.contains("timed out"), "{err}");
    }

    /// 派工樣本到 fixer 的子 session 建立之後（含）的前 `extra` 則事件。
    fn dispatch_events_until_child(extra: usize) -> Vec<String> {
        let events = fake_v2::fixture_events("server-dispatch-subagent", "ses_fake1");
        let created = events
            .iter()
            .position(|e| e.contains("\"session.created\""))
            .unwrap();
        events[..=created + extra].to_vec()
    }

    #[tokio::test]
    async fn a_failed_turn_interrupts_the_subagent_sessions_too() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(dispatch_events_until_child(3));

        v2_backend(&fake)
            .run_streaming(plain_request("派給 fixer"), &mut |_| {})
            .await
            .unwrap_err();

        let requests = fake.requests();
        for session in ["ses_fake1", DISPATCH_CHILD] {
            let interrupt = format!("POST /api/session/{session}/interrupt");
            assert!(
                requests.contains(&interrupt),
                "{interrupt} not in {requests:?}"
            );
        }
    }

    #[tokio::test]
    async fn a_cancelled_turn_interrupts_the_subagent_sessions_too() {
        let fake = fake_v2::FakeV2::start().await;
        let mut events = dispatch_events_until_child(3);
        events.push("hold".to_string());
        fake.script_events(events);
        // 串流看過的 fixer 也在背景工作清單裡：只中斷一次（review-364cebf NICE 2）
        fake.script_jobs(vec![json!([
            {"kind": "subagent", "status": "running", "agent": "fixer", "childSessionID": DISPATCH_CHILD}
        ])]);
        let backend = v2_backend(&fake);

        let cancelled = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            backend.run_streaming(plain_request("派給 fixer"), &mut |_| {}),
        )
        .await;

        assert!(cancelled.is_err(), "the turn should still be running");
        let child = format!("POST /api/session/{DISPATCH_CHILD}/interrupt");
        for _ in 0..50 {
            if fake.requests().contains(&child) && fake.requests().contains(&JOBS_QUERY.to_string())
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        // 清單查完之後若還會重複中斷，給它時間送出
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        let requests = fake.requests();
        assert!(
            requests.contains(&"POST /api/session/ses_fake1/interrupt".to_string()),
            "{requests:?}"
        );
        assert!(requests.contains(&JOBS_QUERY.to_string()), "{requests:?}");
        assert_eq!(
            requests.iter().filter(|r| **r == child).count(),
            1,
            "{requests:?}"
        );
    }

    // 第三方審查 P2-2（PR #47，2026-11-27）：V2 中斷父 session 不會取消它的背景 subagent
    // （`Session.interrupt` 只停那個 session 的執行，`Job.cancelSession` 只在刪除 session 時呼叫）。
    // 串流沒送到子 session 的建立事件（建立前就取消、或事件漏掉）時，取消要從背景工作清單找出子 session。
    #[tokio::test]
    async fn a_cancelled_turn_interrupts_subagents_the_stream_never_announced() {
        let fake = fake_v2::FakeV2::start().await;
        let mut events = dispatch_events_until_child(3);
        let created = events
            .iter()
            .position(|e| e.contains("\"session.created\""))
            .unwrap();
        events.remove(created);
        events.push("hold".to_string());
        fake.script_events(events);
        fake.script_jobs(vec![json!([
            {"kind": "shell", "status": "running", "shellID": "sh_1", "command": "npm run dev"},
            {"kind": "subagent", "status": "running", "agent": "fixer", "childSessionID": DISPATCH_CHILD}
        ])]);
        let backend = v2_backend(&fake);

        let cancelled = tokio::time::timeout(
            std::time::Duration::from_millis(500),
            backend.run_streaming(plain_request("派給 fixer"), &mut |_| {}),
        )
        .await;

        assert!(cancelled.is_err(), "the turn should still be running");
        let child = format!("POST /api/session/{DISPATCH_CHILD}/interrupt");
        for _ in 0..50 {
            if fake.requests().contains(&child) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let requests = fake.requests();
        assert!(requests.contains(&child), "{requests:?}");
        // 先停 primary，不讓它再派新的工作，再查清單
        let root = requests
            .iter()
            .position(|r| r == "POST /api/session/ses_fake1/interrupt")
            .expect("primary interrupted");
        let lookup = requests.iter().position(|r| r == JOBS_QUERY).unwrap();
        assert!(root < lookup, "{requests:?}");
        assert_eq!(
            requests
                .iter()
                .filter(|r| r.ends_with("/interrupt"))
                .count(),
            2,
            "{requests:?}"
        );
    }

    #[tokio::test]
    async fn agent_and_model_are_selected_before_the_prompt() {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_1", 1, 1, None);
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_1"));
        let mut req = plain_request("plan this");
        req.session_id = Some("ses_1".to_string());
        req.agent = Some("plan".to_string());
        req.model = Some("opencode/big-pickle".to_string());

        v2_backend(&fake)
            .run_streaming(req, &mut |_| {})
            .await
            .unwrap();

        let requests = fake.requests();
        let position = |request: &str| requests.iter().position(|seen| seen == request).unwrap();
        assert!(
            position("POST /api/session/ses_1/agent") < position("POST /api/session/ses_1/prompt")
        );
        assert!(
            position("POST /api/session/ses_1/model") < position("POST /api/session/ses_1/prompt")
        );
        assert_eq!(
            fake.body_of("POST /api/session/ses_1/agent"),
            Some(json!({ "agent": "plan" }))
        );
        assert_eq!(
            fake.body_of("POST /api/session/ses_1/model"),
            Some(json!({ "model": { "id": "big-pickle", "providerID": "opencode" } }))
        );
    }

    // #45 SCN-013：新建的 session 也在送 prompt 前選取 agent 與模型；subagent 沿用 session 的模型
    // （docs/issues/issue-0045/probe-subagent.md），所以派工的子 session 也跑在這個模型上。
    #[tokio::test]
    async fn a_new_session_selects_the_agent_and_model_before_the_prompt() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));
        let mut req = plain_request("hi");
        req.agent = Some("wukong".to_string());
        req.model = Some("opencode/big-pickle".to_string());

        v2_backend(&fake)
            .run_streaming(req, &mut |_| {})
            .await
            .unwrap();

        let requests = fake.requests();
        let position = |request: &str| {
            requests
                .iter()
                .position(|seen| seen == request)
                .unwrap_or_else(|| panic!("{request} not in {requests:?}"))
        };
        let created = position("POST /api/session");
        let agent = position("POST /api/session/ses_fake1/agent");
        let model = position("POST /api/session/ses_fake1/model");
        let prompt = position("POST /api/session/ses_fake1/prompt");
        assert!(created < agent && agent < prompt && created < model && model < prompt);
        assert_eq!(
            fake.body_of("POST /api/session/ses_fake1/agent"),
            Some(json!({ "agent": "wukong" }))
        );
        assert_eq!(
            fake.body_of("POST /api/session/ses_fake1/model"),
            Some(json!({ "model": { "id": "big-pickle", "providerID": "opencode" } }))
        );
    }

    // #45 SCN-007：session 指令層的 entry。
    fn scope_request(session: Option<&str>, scope: &str) -> AgentRequest {
        let mut req = plain_request("hi");
        req.session_id = session.map(str::to_string);
        req.session_instructions =
            BTreeMap::from([("wukong.scope".to_string(), format!("scope: {scope}"))]);
        req
    }

    const ENTRIES: &str = "/api/experimental/session/ses_fake1/instructions/entries";

    // #45 SCN-008：常駐 server 改寫定義後要等它用上新內容（實測改寫後 17 毫秒送出的回合仍用舊定義）。
    fn agent_listing(system: &str) -> serde_json::Value {
        json!([
            {"id": "build", "name": "Build", "mode": "primary"},
            {"id": "wukong", "name": "wukong", "mode": "primary", "system": system},
        ])
    }

    #[tokio::test]
    async fn waits_until_the_server_lists_the_rewritten_definition() {
        let fake = fake_v2::FakeV2::start().await;
        // 清單一開始沒有自訂 agent（server 剛啟動），接著是舊內容，之後才是新內容
        fake.script_agents(vec![
            json!([{"id": "build", "name": "Build"}]),
            agent_listing("舊的內文"),
            agent_listing("新的內文\n\n第二段"),
        ]);

        let loaded = v2_backend(&fake)
            .wait_for_agent_system(
                "wukong",
                "新的內文\n\n第二段\n",
                std::time::Duration::from_secs(5),
            )
            .await;

        assert!(loaded);
        let listed = fake
            .requests()
            .iter()
            .filter(|r| *r == "GET /api/agent")
            .count();
        assert_eq!(listed, 3, "{:?}", fake.requests());
    }

    #[tokio::test]
    async fn gives_up_when_the_server_keeps_the_old_definition() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_agents(vec![agent_listing("舊的內文")]);
        let started = std::time::Instant::now();

        let loaded = v2_backend(&fake)
            .wait_for_agent_system("wukong", "新的內文", std::time::Duration::from_millis(300))
            .await;

        assert!(!loaded);
        assert!(started.elapsed() < std::time::Duration::from_secs(2));
        let listed = fake
            .requests()
            .iter()
            .filter(|r| *r == "GET /api/agent")
            .count();
        assert!(listed >= 2, "{:?}", fake.requests());
    }

    #[tokio::test]
    async fn a_new_session_gets_its_instructions_before_the_prompt() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));

        v2_backend(&fake)
            .run_streaming(scope_request(None, "user:tg-42"), &mut |_| {})
            .await
            .unwrap();

        assert_eq!(
            fake.entries_of("ses_fake1"),
            vec![("wukong.scope".to_string(), json!("scope: user:tg-42"))]
        );
        let requests = fake.requests();
        let put = format!("PUT {ENTRIES}/wukong.scope");
        let written = requests.iter().position(|r| *r == put).unwrap();
        let prompt = requests
            .iter()
            .position(|r| r == "POST /api/session/ses_fake1/prompt")
            .unwrap();
        assert!(written < prompt, "{requests:?}");
    }

    // 值沒變就不重寫：改值會在對話紀錄插一則通知（probe-instructions.md）。
    #[tokio::test]
    async fn an_unchanged_entry_is_not_written_again() {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_fake1", 1, 1, None);
        fake.set_entry("ses_fake1", "wukong.scope", json!("scope: user:tg-42"));
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));

        v2_backend(&fake)
            .run_streaming(scope_request(Some("ses_fake1"), "user:tg-42"), &mut |_| {})
            .await
            .unwrap();

        let requests = fake.requests();
        assert!(requests.contains(&format!("GET {ENTRIES}")), "{requests:?}");
        assert!(
            !requests.iter().any(|r| r.starts_with("PUT ")),
            "{requests:?}"
        );
    }

    // 升級前就在的 session（沒有 entry）或值不同的 entry，在下一個回合補寫。
    #[tokio::test]
    async fn a_missing_or_stale_entry_is_written_on_the_next_turn() {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_fake1", 1, 1, None);
        fake.set_entry("ses_fake1", "wukong.scope", json!("scope: user:old"));
        fake.set_entry("ses_fake1", "user.note", json!("kept"));
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));

        v2_backend(&fake)
            .run_streaming(scope_request(Some("ses_fake1"), "user:tg-42"), &mut |_| {})
            .await
            .unwrap();

        let mut entries = fake.entries_of("ses_fake1");
        entries.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            entries,
            vec![
                ("user.note".to_string(), json!("kept")),
                ("wukong.scope".to_string(), json!("scope: user:tg-42")),
            ]
        );
    }

    #[tokio::test]
    async fn a_request_without_instructions_does_not_touch_the_entries() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));

        v2_backend(&fake)
            .run_streaming(plain_request("hi"), &mut |_| {})
            .await
            .unwrap();

        assert!(!fake.requests().iter().any(|r| r.contains("/instructions/")));
    }

    #[tokio::test]
    async fn attachments_are_sent_as_file_uris_in_the_prompt() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("secret.txt");
        std::fs::write(&path, b"The secret word is PINEAPPLE42.\n").unwrap();
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-attach-file", "ses_fake1"));
        let mut backend = v2_backend(&fake);
        backend.workspace = Some(workspace.path().to_path_buf());
        backend.file_mode = FileTransportMode::Shared;
        backend.server_workspace = Some(PathBuf::from("/p/workspace"));

        let response = backend
            .run_streaming(
                request_with_attachment(path, "secret.txt", "text/plain"),
                &mut |_| {},
            )
            .await
            .unwrap();

        assert_eq!(response.text, "PINEAPPLE42");
        assert_eq!(
            fake.body_of("POST /api/session/ses_fake1/prompt"),
            Some(json!({
                "text": "describe",
                "files": [{ "uri": "file:///p/workspace/secret.txt", "name": "secret.txt" }]
            }))
        );
    }

    // 規劃與路由用 `question: false` 關掉提問；V2 只能在建立 session 時以權限規則套用。
    #[tokio::test]
    async fn tool_overrides_become_permissions_of_the_ephemeral_session() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));
        let mut req = plain_request("route");
        req.tool_overrides.insert("question".to_string(), false);

        let response = v2_backend(&fake).run_ephemeral(req).await.unwrap();

        assert_eq!(response.text, "DONE");
        assert_eq!(response.session_id, None);
        assert_eq!(
            fake.body_of("POST /api/session"),
            Some(json!({
                "title": "Wukong",
                "permissions": [{ "action": "question", "resource": "*", "effect": "deny" }]
            }))
        );
        assert!(
            fake.session_ids().is_empty(),
            "ephemeral session was not deleted"
        );
    }

    #[tokio::test]
    async fn a_missing_session_is_replaced_by_a_new_one() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-text-tool", "ses_fake1"));
        let mut req = plain_request("hi");
        req.session_id = Some("ses_gone".to_string());

        let response = v2_backend(&fake)
            .run_streaming(req, &mut |_| {})
            .await
            .unwrap();

        assert_eq!(response.session_id.as_deref(), Some("ses_fake1"));
        let requests = fake.requests();
        assert!(requests.contains(&"POST /api/session/ses_gone/prompt".to_string()));
        assert!(requests.contains(&"POST /api/session/ses_fake1/prompt".to_string()));
    }

    #[tokio::test]
    async fn run_streaming_interrupts_the_session_when_the_event_stream_dies() {
        let fake = fake_v2::FakeV2::start().await;
        let stderr = agent_failed_stderr(
            v2_backend(&fake)
                .run_streaming(plain_request("hi"), &mut |_| {})
                .await
                .unwrap_err(),
        );
        assert!(
            stderr.contains("event stream ended before session became idle"),
            "{stderr}"
        );
        let requests = fake.requests();
        assert!(
            requests.contains(&"POST /api/session/ses_fake1/interrupt".to_string()),
            "{requests:?}"
        );
        assert!(
            fake.session_ids().is_empty(),
            "owned session was not cleaned up"
        );
    }

    /// 真實重試樣本裡，到第 `retries` 次 `session.retry.scheduled` 為止的事件。
    fn events_until_retry(session: &str, retries: usize) -> Vec<String> {
        let mut seen = 0;
        let mut events = Vec::new();
        for event in fake_v2::fixture_events("server-retry", session) {
            let is_retry = event.contains("\"session.retry.scheduled\"");
            events.push(event);
            if is_retry {
                seen += 1;
                if seen == retries {
                    break;
                }
            }
        }
        assert_eq!(seen, retries);
        events
    }

    // 真實 I/O 下不能暫停 tokio 時間（reqwest 的逾時會被快轉觸發），整合測試改用縮短的時限；
    // 90 秒本身由 upstream_error 以暫停的時間驗證。
    const SHORT_RETRY_LIMIT: std::time::Duration = std::time::Duration::from_millis(400);

    // issue-0044 SCN-007 第三列：連不到上游時 V2 一直重試，超過時限由 Wukong 中止並送 interrupt。
    #[tokio::test]
    async fn a_retry_lasting_too_long_is_interrupted_as_unreachable() {
        let fake = fake_v2::FakeV2::start().await;
        let mut events = events_until_retry("ses_fake1", 2);
        events.push("hold".to_string());
        fake.script_events(events);
        let mut backend = v2_backend(&fake);
        backend.retry_give_up_after = SHORT_RETRY_LIMIT;
        let started = tokio::time::Instant::now();

        let error = backend
            .run_streaming(plain_request("hi"), &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("Can't reach opencode.ai"),
            "{error}"
        );
        assert_eq!(
            upstream_failure_kind(error),
            crate::upstream_error::UpstreamFailure::Unreachable
        );
        let waited = started.elapsed();
        assert!(
            waited >= SHORT_RETRY_LIMIT && waited < SHORT_RETRY_LIMIT * 5,
            "{waited:?}"
        );
        assert!(fake
            .requests()
            .contains(&"POST /api/session/ses_fake1/interrupt".to_string()));
    }

    // issue-0044 SCN-016：重試開始後、時限之前恢復，回合照常完成。
    #[tokio::test]
    async fn a_retry_that_recovers_in_time_lets_the_turn_finish() {
        let fake = fake_v2::FakeV2::start().await;
        // 重試後 300ms 恢復輸出，再 300ms 才結束：從第一次重試算起超過時限，恢復時沒有重新
        // 起算就會被中止。
        let turn = fake_v2::fixture_events("server-text-tool", "ses_fake1");
        let end = turn
            .iter()
            .position(|event| event.contains("\"session.execution.succeeded\""))
            .unwrap();
        let mut events = events_until_retry("ses_fake1", 1);
        events.push("pause:300".to_string());
        events.extend(turn[..end].iter().cloned());
        events.push("pause:300".to_string());
        events.extend(turn[end..].iter().cloned());
        fake.script_events(events);
        let mut backend = v2_backend(&fake);
        backend.retry_give_up_after = SHORT_RETRY_LIMIT;

        let response = backend
            .run_streaming(plain_request("hi"), &mut |_| {})
            .await
            .unwrap();

        assert_eq!(response.text, "DONE");
        assert!(!fake
            .requests()
            .iter()
            .any(|request| request.ends_with("/interrupt")));
    }

    // issue-0044 SCN-005：入口取消（回合的 future 被丟掉）時送 interrupt，V2 不再繼續執行。
    #[tokio::test]
    async fn dropping_the_turn_interrupts_the_session() {
        let fake = fake_v2::FakeV2::start().await;
        let mut events = fake_v2::fixture_events("server-text-tool", "ses_fake1");
        let step = events
            .iter()
            .position(|event| event.contains("\"session.step.started\""))
            .unwrap();
        events.truncate(step + 1);
        events.push("hold".to_string());
        fake.script_events(events);
        let backend = v2_backend(&fake);

        let mut sink = |_| {};
        let turn = backend.run_streaming(plain_request("hi"), &mut sink);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(500), turn)
                .await
                .is_err(),
            "the held turn should still be running"
        );

        let interrupt = "POST /api/session/ses_fake1/interrupt".to_string();
        for _ in 0..50 {
            if fake.requests().contains(&interrupt) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!(
            "no interrupt after the turn was dropped: {:?}",
            fake.requests()
        );
    }

    // issue-0044 SCN-007 第一列、review-0405603 S3：額度用完的真實樣本（2026-11-25）。V2 不重試
    // free-limit，第一步就 step.failed、execution.failed，所以不會碰到 90 秒的重試中止。
    #[tokio::test]
    async fn a_real_free_limit_fails_at_once_as_rate_limited() {
        let fake = fake_v2::FakeV2::start().await;
        let events = fake_v2::fixture_events("server-free-limit", "ses_fake1");
        assert!(!events.iter().any(|event| event.contains("session.retry")));
        fake.script_events(events);
        let started = tokio::time::Instant::now();

        let error = v2_backend(&fake)
            .run_streaming(plain_request("hi"), &mut |_| {})
            .await
            .unwrap_err();

        assert!(
            error.to_string().contains("Free usage limit reached"),
            "{error}"
        );
        assert_eq!(
            upstream_failure_kind(error),
            crate::upstream_error::UpstreamFailure::RateLimited
        );
        assert!(started.elapsed() < std::time::Duration::from_secs(5));
    }

    #[tokio::test]
    async fn an_interrupted_execution_reports_the_turn_as_aborted() {
        let fake = fake_v2::FakeV2::start().await;
        fake.script_events(fake_v2::fixture_events("server-interrupt", "ses_fake1"));

        let error = v2_backend(&fake)
            .run_streaming(plain_request("hi"), &mut |_| {})
            .await
            .unwrap_err();

        assert_eq!(
            upstream_failure_kind(error),
            crate::upstream_error::UpstreamFailure::Aborted
        );
    }

    enum Reply {
        Answer(&'static str),
        Cancel,
    }

    /// 重播真實的提問或權限樣本：事件停在詢問之後 300ms，期間經 `AgentBackend`（入口共用的
    /// 分派）回覆，之後送出樣本剩下的事件。
    async fn turn_with_reply(
        fixture: &str,
        reply: Reply,
    ) -> (
        Result<AgentResponse, GatewayError>,
        fake_v2::FakeV2,
        crate::backend::AgentBackend,
    ) {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_1", 1, 1, None);
        let events = fake_v2::fixture_events(fixture, "ses_1");
        let asked = events
            .iter()
            .position(|event| {
                let value: Value = serde_json::from_str(event).unwrap();
                value["type"] == "form.created" || value["type"] == "permission.asked"
            })
            .unwrap();
        let mut script = events[..=asked].to_vec();
        script.push("pause:300".to_string());
        script.extend(events[asked + 1..].iter().cloned());
        fake.script_events(script);
        let backend = crate::backend::AgentBackend::Server(v2_backend(&fake));
        let (questions, mut asked_rx) = tokio::sync::mpsc::unbounded_channel();
        let mut sink = |event| {
            if let StreamEvent::QuestionRequest(question) = event {
                questions.send(question).unwrap();
            }
        };
        let mut req = plain_request("ask me");
        req.session_id = Some("ses_1".to_string());
        let turn = backend.run_streaming(req, &mut sink);
        let answer = async {
            let question = asked_rx.recv().await.unwrap();
            match reply {
                Reply::Answer(text) => backend
                    .answer_question(
                        &question.session_id,
                        &question.request_id,
                        vec![vec![text.to_string()]],
                    )
                    .await
                    .unwrap(),
                Reply::Cancel => backend
                    .cancel_question(&question.session_id, &question.request_id)
                    .await
                    .unwrap(),
            }
        };
        let (result, ()) = tokio::join!(turn, answer);
        (result, fake, backend)
    }

    fn form_id(fixture: &str) -> String {
        fake_v2::fixture_events(fixture, "ses_1")
            .iter()
            .find_map(|event| {
                let value: Value = serde_json::from_str(event).ok()?;
                (value["type"] == "form.created")
                    .then(|| value["data"]["form"]["id"].as_str().unwrap().to_string())
            })
            .unwrap()
    }

    // issue-0044 SCN-006：回答、自訂回答、取消與權限回覆送到 V2 的端點，回合依樣本繼續或結束。
    #[tokio::test]
    async fn an_answer_is_sent_to_the_form_and_the_turn_goes_on() {
        let (result, fake, backend) = turn_with_reply("server-form", Reply::Answer("A")).await;
        assert_eq!(result.unwrap().text, "A");
        let crate::backend::AgentBackend::Server(server) = &backend else {
            unreachable!()
        };
        assert!(
            server.forms.lock().unwrap().is_empty(),
            "forms kept after the turn"
        );
        let reply = format!(
            "POST /api/session/ses_1/form/{}/reply",
            form_id("server-form")
        );
        assert_eq!(
            fake.body_of(&reply),
            Some(json!({ "answer": { "q0": "A" } }))
        );
    }

    #[tokio::test]
    async fn a_custom_answer_is_sent_as_text() {
        let (result, fake, _) =
            turn_with_reply("server-form-custom", Reply::Answer("CUSTOM-42")).await;
        assert_eq!(result.unwrap().text, "CUSTOM-42");
        let reply = format!(
            "POST /api/session/ses_1/form/{}/reply",
            form_id("server-form-custom")
        );
        assert_eq!(
            fake.body_of(&reply),
            Some(json!({ "answer": { "q0": "CUSTOM-42" } }))
        );
    }

    // V1 1.19.6 取消提問後是不帶錯誤的 idle（回合照常結束、沒有新回覆）；V2 改送
    // execution.interrupted，不能因此回報成「回合被中止」。
    #[tokio::test]
    async fn a_cancelled_question_ends_the_turn_like_v1() {
        let (result, fake, _) = turn_with_reply("server-form-cancel", Reply::Cancel).await;
        assert_eq!(result.unwrap().text, "");
        let form = format!(
            "DELETE /api/session/ses_1/form/{}",
            form_id("server-form-cancel")
        );
        assert!(fake.requests().contains(&form), "{:?}", fake.requests());
    }

    #[tokio::test]
    async fn an_allowed_permission_lets_the_turn_finish() {
        let (result, fake, _) = turn_with_reply(
            "server-permission",
            Reply::Answer(crate::stream::PERMISSION_ALLOW_ONCE_LABEL),
        )
        .await;
        assert_eq!(result.unwrap().text, "DONE");
        let request = fake
            .requests()
            .into_iter()
            .find(|request| request.contains("/permission/"))
            .unwrap();
        assert!(
            request.starts_with("POST /api/session/ses_1/permission/per_"),
            "{request}"
        );
        assert_eq!(fake.body_of(&request), Some(json!({ "decision": "once" })));
    }

    #[tokio::test]
    async fn a_rejected_permission_ends_the_turn_like_v1() {
        let (result, fake, _) = turn_with_reply("server-permission-reject", Reply::Cancel).await;
        assert_eq!(result.unwrap().text, "");
        let request = fake
            .requests()
            .into_iter()
            .find(|request| request.contains("/permission/"))
            .unwrap();
        assert_eq!(
            fake.body_of(&request),
            Some(json!({ "decision": "reject" }))
        );
    }

    // issue-0044 SCN-008（壓縮）：compact 排進佇列後等到執行結束；樣本為真實的壓縮過程。
    #[tokio::test]
    async fn compaction_waits_for_the_compaction_to_finish() {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_1", 1, 1, None);
        let all = fake_v2::fixture_events("server-compact", "ses_1");
        // 樣本前段是壓縮前的那一次回覆；只重播 compact 之後的事件。
        let start = all
            .iter()
            .position(|event| {
                let event: Value = serde_json::from_str(event).unwrap();
                event["type"] == "session.inbox.enqueued"
                    && event["data"]["item"]["type"] == "compaction"
            })
            .unwrap();
        fake.script_events(all[start..].to_vec());

        let compacted = v2_backend(&fake)
            .compact_session("ses_1", Some("opencode/big-pickle"))
            .await
            .unwrap();

        assert_eq!(compacted.text, "");
        assert_eq!(compacted.session_id.as_deref(), Some("ses_1"));
        assert_eq!(
            fake.body_of("POST /api/session/ses_1/compact"),
            Some(json!({}))
        );
    }

    // issue-0044 review-0405603 S1：壓縮與回合一樣在 server 上執行；失敗或被丟掉時不送 interrupt，
    // V2 會繼續重試那次壓縮，下一個回合的 prompt 要排在它後面等。
    #[tokio::test]
    async fn a_compaction_retrying_too_long_is_interrupted() {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_1", 1, 1, None);
        let mut events = events_until_retry("ses_1", 2);
        events.push("hold".to_string());
        fake.script_events(events);
        let mut backend = v2_backend(&fake);
        backend.retry_give_up_after = SHORT_RETRY_LIMIT;

        let error = backend.compact_session("ses_1", None).await.unwrap_err();

        assert_eq!(
            upstream_failure_kind(error),
            crate::upstream_error::UpstreamFailure::Unreachable
        );
        assert!(fake
            .requests()
            .contains(&"POST /api/session/ses_1/interrupt".to_string()));
    }

    #[tokio::test]
    async fn dropping_a_compaction_interrupts_the_session() {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_1", 1, 1, None);
        fake.script_events(vec!["hold".to_string()]);
        let backend = v2_backend(&fake);

        let compaction = backend.compact_session("ses_1", None);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(500), compaction)
                .await
                .is_err(),
            "the held compaction should still be running"
        );

        let interrupt = "POST /api/session/ses_1/interrupt".to_string();
        for _ in 0..50 {
            if fake.requests().contains(&interrupt) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        panic!(
            "no interrupt after the compaction was dropped: {:?}",
            fake.requests()
        );
    }

    #[tokio::test]
    async fn a_failed_compaction_is_an_error() {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_1", 1, 1, None);
        fake.script_events(vec![
            r#"{"type":"session.compaction.failed","data":{"sessionID":"ses_1","reason":"manual","error":{"type":"provider.transport","message":"offline"}}}"#.to_string(),
            r#"{"type":"session.execution.succeeded","data":{"sessionID":"ses_1"}}"#.to_string(),
        ]);
        let error = v2_backend(&fake)
            .compact_session("ses_1", None)
            .await
            .unwrap_err();
        assert!(error.to_string().contains("offline"), "{error}");
    }

    /// Answers every request with the same status and body, recording what was asked.
    async fn canned_server(
        hits: std::sync::Arc<Mutex<Vec<String>>>,
        status: &'static str,
        body: &'static str,
    ) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            loop {
                let Ok((mut socket, _)) = listener.accept().await else {
                    break;
                };
                let mut buf = Vec::new();
                let mut tmp = [0u8; 1024];
                while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                    match socket.read(&mut tmp).await {
                        Ok(0) | Err(_) => break,
                        Ok(n) => buf.extend_from_slice(&tmp[..n]),
                    }
                }
                let head = String::from_utf8_lossy(&buf);
                let mut words = head.split_whitespace();
                let method = words.next().unwrap_or_default();
                let path = words.next().unwrap_or_default();
                hits.lock().unwrap().push(format!("{method} {path}"));
                let response = format!(
                    "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = socket.write_all(response.as_bytes()).await;
            }
        });
        format!("http://{addr}")
    }

    async fn list_sessions_against(
        status: &'static str,
        body: &'static str,
    ) -> (Result<SessionListing, GatewayError>, Vec<String>) {
        let hits = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let backend =
            OpencodeServerBackend::from_env(canned_server(hits.clone(), status, body).await, None);
        let result = backend.list_sessions().await;
        let seen = hits.lock().unwrap().clone();
        (result, seen)
    }

    fn v2_backend(fake: &fake_v2::FakeV2) -> OpencodeServerBackend {
        let mut backend = OpencodeServerBackend::from_env(fake.url.clone(), None);
        backend.password = Some(fake_v2::PASSWORD.to_string());
        backend
    }

    // issue-0044 SCN-004：就緒檢查問 `/api/info`，使用者名稱固定 opencode。
    #[tokio::test]
    async fn health_check_asks_api_info() {
        let fake = fake_v2::FakeV2::start().await;
        v2_backend(&fake).health_check().await.unwrap();
        assert_eq!(fake.requests(), ["GET /api/info"]);
    }

    #[tokio::test]
    async fn health_check_reports_a_rejected_password() {
        let fake = fake_v2::FakeV2::start().await;
        let mut backend = v2_backend(&fake);
        backend.password = Some("wrong".to_string());
        let stderr = agent_failed_stderr(backend.health_check().await.unwrap_err());
        assert!(stderr.contains("認證被拒"), "{stderr}");
        assert!(stderr.contains("WUKONG_AGENT_SERVER_PASSWORD"), "{stderr}");

        backend.password = None;
        let stderr = agent_failed_stderr(backend.health_check().await.unwrap_err());
        assert!(stderr.contains("認證被拒"), "{stderr}");
    }

    #[tokio::test]
    async fn health_check_does_not_take_the_web_page_for_a_healthy_server() {
        // V2 對 `/api` 以外的路徑都回網頁 200，只看狀態碼的檢查會把它判成健康。
        let hits = std::sync::Arc::new(Mutex::new(Vec::<String>::new()));
        let backend = OpencodeServerBackend::from_env(
            canned_server(hits, "200 OK", "<!doctype html><html></html>").await,
            None,
        );
        let stderr = agent_failed_stderr(backend.health_check().await.unwrap_err());
        assert!(stderr.contains("health_check"), "{stderr}");
    }

    // 地雷 W-6、issue-0044 SCN-008：最舊的 session 在最後一頁，只取一頁就看不到它們。
    #[tokio::test]
    async fn list_sessions_follows_the_cursor_to_the_oldest_session() {
        let fake = fake_v2::FakeV2::start().await;
        for i in 0..150 {
            fake.add_session(&format!("ses_{i:03}"), i, 1000 + i, None);
        }
        fake.add_session("ses_child", 200, 2000, Some("ses_000"));

        let listing = v2_backend(&fake).list_sessions().await.unwrap();

        assert!(!listing.truncated);
        let mut ids: Vec<&str> = listing.sessions.iter().map(|s| s.id.as_str()).collect();
        ids.sort_unstable();
        let mut expected = fake.session_ids();
        expected.sort_unstable();
        assert_eq!(ids, expected);
        let oldest = listing.sessions.iter().find(|s| s.id == "ses_000").unwrap();
        assert_eq!(oldest.updated_ms, 1000);
        let child = listing
            .sessions
            .iter()
            .find(|s| s.id == "ses_child")
            .unwrap();
        assert_eq!(child.parent_id.as_deref(), Some("ses_000"));
        let pages: Vec<String> = fake
            .requests()
            .into_iter()
            .filter(|request| request.starts_with("GET /api/session?"))
            .collect();
        assert!(pages.len() > 151 / fake_v2::PAGE_CAP, "{pages:?}");
        assert!(
            pages[1..].iter().all(|page| page.contains("cursor=")),
            "{pages:?}"
        );
    }

    #[tokio::test]
    async fn list_sessions_stops_when_the_cursor_never_ends() {
        let (result, seen) = list_sessions_against(
            "200 OK",
            r#"{"data":[{"id":"ses_1","time":{"updated":1}}],"cursor":{"next":"again"}}"#,
        )
        .await;
        assert!(result.unwrap().truncated);
        assert_eq!(seen.len(), SESSION_LIST_MAX_PAGES);
    }

    #[tokio::test]
    async fn list_sessions_ends_on_a_page_without_a_next_cursor() {
        let (result, seen) = list_sessions_against(
            "200 OK",
            r#"{"data":[{"id":"ses_1","time":{"updated":1}}],"cursor":{"previous":null,"next":null}}"#,
        )
        .await;
        assert_eq!(
            seen,
            [format!("GET /api/session?limit={SESSION_PAGE_SIZE}")]
        );
        assert_eq!(
            result.unwrap(),
            SessionListing {
                sessions: vec![SessionSummary {
                    id: "ses_1".to_string(),
                    updated_ms: 1,
                    parent_id: None,
                }],
                truncated: false,
            }
        );
    }

    // issue-0028 SCN-004～006、issue-0044 SCN-008：session 年齡取自
    // `GET /api/session/{id}` 的 `data.time.created`（毫秒）；看不懂的回應是錯誤，不是 0。
    #[tokio::test]
    async fn session_created_ms_reads_time_created_from_the_session() {
        let fake = fake_v2::FakeV2::start().await;
        fake.add_session("ses_1", 1793677006691, 1793677027641, None);
        let backend = v2_backend(&fake);
        assert_eq!(
            backend.session_created_ms("ses_1").await.unwrap(),
            1793677006691
        );
        assert_eq!(fake.requests(), ["GET /api/session/ses_1"]);
        assert!(backend.session_created_ms("ses_missing").await.is_err());

        for body in [
            r#"{"data":{"id":"ses_1","time":{"updated":1}}}"#,
            r#"{"id":"ses_1","time":{"created":1}}"#,
        ] {
            let backend = OpencodeServerBackend::from_env(
                canned_server(std::sync::Arc::new(Mutex::new(Vec::new())), "200 OK", body).await,
                None,
            );
            let stderr =
                agent_failed_stderr(backend.session_created_ms("ses_1").await.unwrap_err());
            assert!(stderr.contains("time.created"), "{body}: {stderr}");
        }
    }

    // 保留期清理、輪替與 `--new` 都經由這裡刪 session；已經不在的 session 不算失敗。
    #[tokio::test]
    async fn sessions_are_created_and_deleted_through_the_api_routes() {
        let fake = fake_v2::FakeV2::start().await;
        let backend = v2_backend(&fake);
        let id = backend.create_session(&plain_request("x")).await.unwrap();
        assert_eq!(fake.session_ids(), std::slice::from_ref(&id));
        backend.delete_session(&id).await.unwrap();
        assert!(fake.session_ids().is_empty());
        backend.delete_session(&id).await.unwrap();
        assert_eq!(
            fake.requests(),
            [
                "POST /api/session".to_string(),
                format!("DELETE /api/session/{id}"),
                format!("DELETE /api/session/{id}"),
            ]
        );
    }

    #[tokio::test]
    async fn list_sessions_fails_instead_of_returning_a_partial_list() {
        // 清理靠這份清單決定刪誰；任何看不懂的回應都必須是錯誤，不能是「沒有 session」。
        for (status, body) in [
            ("200 OK", r#"{"name":"UnknownError"}"#),
            ("200 OK", r#"[{"id":"ses_1","time":{"updated":1}}]"#),
            ("200 OK", r#"{"data":[{"id":"ses_1"}],"cursor":{}}"#),
            ("200 OK", r#"{"data":[{"time":{"updated":1}}],"cursor":{}}"#),
            (
                "200 OK",
                r#"{"data":[{"id":"ses_1","time":{"updated":1}},{"id":"ses_2","time":{}}],"cursor":{}}"#,
            ),
        ] {
            let (result, _) = list_sessions_against(status, body).await;
            let stderr = agent_failed_stderr(result.unwrap_err());
            assert!(stderr.contains("list_sessions"), "{body}: {stderr}");
        }
    }

    #[test]
    fn a_malformed_listing_is_reported_without_dumping_its_contents() {
        // 這些錯誤每一輪都會進 schedulerd 的日誌。session 物件帶著標題，整份回應
        // 可能有幾 MB——錯誤訊息只需要說得出是哪一筆、哪裡不對。
        let missing_time = json!({"data":[{"id":"ses_1","title":"quarterly salary review"}]});
        let stderr = agent_failed_stderr(parse_session_page(&missing_time).unwrap_err());
        assert!(stderr.contains("ses_1"), "{stderr}");
        assert!(!stderr.contains("quarterly salary review"), "{stderr}");

        let missing_id = json!({"data":[{"title":"quarterly salary review","time":{"updated":1}}]});
        let stderr = agent_failed_stderr(parse_session_page(&missing_id).unwrap_err());
        assert!(!stderr.contains("quarterly salary review"), "{stderr}");

        // serde_json 依鍵名排序輸出，所以把大欄位放在 _tag 之後。
        let not_a_list = json!({"_tag":"UnknownError","stack":"x".repeat(5000)});
        let stderr = agent_failed_stderr(parse_session_page(&not_a_list).unwrap_err());
        assert!(stderr.contains("UnknownError"), "{stderr}");
        assert!(stderr.len() < 500, "{} bytes", stderr.len());
    }

    #[tokio::test]
    async fn list_sessions_surfaces_the_upstream_error_body() {
        // 實測：一筆 directory 不是絕對路徑的資料列就會讓整個列表回 500，而且每一輪
        // 都一樣。錯誤要帶著上游回應，才看得出是同一個原因在重複。
        let (result, _) = list_sessions_against(
            "500 Internal Server Error",
            r#"{"name":"UnknownError","data":{"ref":"err_717b5ac3"}}"#,
        )
        .await;
        let stderr = agent_failed_stderr(result.unwrap_err());
        assert!(stderr.contains("500"), "{stderr}");
        assert!(stderr.contains("err_717b5ac3"), "{stderr}");
    }

    fn request_with_attachment(path: PathBuf, name: &str, mime: &str) -> AgentRequest {
        AgentRequest {
            prompt: "describe".to_string(),
            session_id: None,
            thinking: false,
            model: None,
            agent: None,
            tool_overrides: BTreeMap::new(),
            attachments: vec![AgentAttachment {
                path,
                original_name: name.to_string(),
                mime_type: Some(mime.to_string()),
            }],
            session_instructions: BTreeMap::new(),
        }
    }

    #[tokio::test]
    async fn server_backend_builds_shared_file_uris() {
        let workspace = tempfile::tempdir().unwrap();
        let upload_dir = workspace.path().join(".wukong/uploads/user/42");
        std::fs::create_dir_all(&upload_dir).unwrap();
        let path = upload_dir.join("quarter report.pdf");
        std::fs::write(&path, b"%PDF-test").unwrap();
        let mut backend = OpencodeServerBackend::from_env(
            "http://127.0.0.1:1".to_string(),
            Some(workspace.path().to_path_buf()),
        );
        backend.file_mode = FileTransportMode::Shared;
        backend.server_workspace = Some(PathBuf::from("/workspace"));

        let files = backend
            .build_files(&request_with_attachment(
                path,
                "../quarter report.pdf",
                "application/pdf",
            ))
            .await
            .unwrap();

        assert_eq!(
            files,
            [FileAttachment {
                uri: "file:///workspace/.wukong/uploads/user/42/quarter%20report.pdf".to_string(),
                name: "quarter report.pdf".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn server_backend_builds_inline_data_uris() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("data.csv");
        std::fs::write(&path, b"a,b\n1,2\n").unwrap();
        let mut backend = OpencodeServerBackend::from_env(
            "http://127.0.0.1:1".to_string(),
            Some(workspace.path().to_path_buf()),
        );
        backend.file_mode = FileTransportMode::Inline;

        let files = backend
            .build_files(&request_with_attachment(path, "data.csv", "text/csv"))
            .await
            .unwrap();

        assert_eq!(
            files,
            [FileAttachment {
                uri: "data:text/plain;base64,YSxiCjEsMgo=".to_string(),
                name: "data.csv".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn server_backend_rejects_attachment_outside_workspace() {
        let workspace = tempfile::tempdir().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        let backend = OpencodeServerBackend::from_env(
            "http://127.0.0.1:1".to_string(),
            Some(workspace.path().to_path_buf()),
        );

        let err = backend
            .build_files(&request_with_attachment(
                outside.path().to_path_buf(),
                "outside.txt",
                "text/plain",
            ))
            .await
            .unwrap_err();

        assert!(err.to_string().contains("工作區外"), "{err}");
    }

    #[test]
    fn server_backend_rehydrates_attachments_when_session_disappears() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("report.csv");
        std::fs::write(&path, b"a,b\n1,2\n").unwrap();
        let backend = OpencodeServerBackend::from_env(
            "http://127.0.0.1:1".to_string(),
            Some(workspace.path().to_path_buf()),
        );
        let uploaded = request_with_attachment(path.clone(), "report.csv", "text/csv");
        backend.remember_attachments("ses_old", &uploaded);
        let follow_up = AgentRequest {
            prompt: "第 20 列呢？".to_string(),
            session_id: Some("ses_old".to_string()),
            thinking: false,
            model: None,
            agent: None,
            tool_overrides: BTreeMap::new(),
            attachments: Vec::new(),
            session_instructions: Default::default(),
        };

        let retry = backend.rehydrate_request("ses_old", &follow_up);

        assert_eq!(retry.prompt, "第 20 列呢？");
        assert_eq!(retry.attachments.len(), 1);
        assert_eq!(retry.attachments[0].path, path);
    }

    #[tokio::test]
    async fn health_check_decode_error_includes_phase() {
        let backend = OpencodeServerBackend::from_env(malformed_chunked_server().await, None);

        let err = backend.health_check().await.unwrap_err();
        let stderr = agent_failed_stderr(err);

        assert!(stderr.contains("opencode server health_check failed"));
        assert!(stderr.contains("error decoding response body"));
    }

    #[tokio::test]
    async fn event_stream_chunk_decode_error_includes_phase() {
        let backend = OpencodeServerBackend::from_env(malformed_chunked_server().await, None);
        let response = backend.open_event_stream().await.unwrap();

        let err = backend
            .consume_event_stream(
                response,
                "ses_1",
                &mut |_| {},
                &mut InterruptOnDrop::default(),
            )
            .await
            .unwrap_err();
        let stderr = agent_failed_stderr(err);

        assert!(stderr.contains("opencode server event_stream failed while reading chunk"));
        // reqwest 0.12 的 Display 不含原因鏈；http_error 需自行附加，否則只剩
        // 「error decoding response body」無從診斷。
        assert!(
            stderr.contains("error decoding response body: "),
            "{stderr}"
        );
    }

    fn real_form_fields(fixture: &str) -> Vec<FormField> {
        let backend = OpencodeServerBackend::from_env("http://127.0.0.1:1".to_string(), None);
        for event in fake_v2::fixture_events(fixture, "ses_1") {
            let value: Value = serde_json::from_str(&event).unwrap();
            if value["type"] == "form.created" {
                backend.remember_form("ses_1", &value);
            }
        }
        let forms = backend.forms.lock().unwrap();
        assert_eq!(forms.len(), 1);
        forms.values().next().unwrap().1.clone()
    }

    // issue-0044 SCN-006：答案依真實 form 的欄位轉成 V2 的 Form.Answer。
    #[test]
    fn answers_follow_the_fields_of_the_real_form() {
        let single = real_form_fields("server-form");
        assert_eq!(
            Value::Object(form_answer(Some(&single), vec![vec!["A".to_string()]])),
            json!({ "q0": "A" })
        );
        // 自訂回答原樣送出。
        assert_eq!(
            Value::Object(form_answer(
                Some(&single),
                vec![vec!["我自己的答案".to_string()]]
            )),
            json!({ "q0": "我自己的答案" })
        );
        let multi = real_form_fields("server-form-multi");
        assert!(multi[0].multiselect);
        assert_eq!(
            Value::Object(form_answer(
                Some(&multi),
                vec![vec!["Red".to_string(), "Blue".to_string()]]
            )),
            json!({ "q0": ["Red", "Blue"] })
        );
    }

    #[test]
    fn option_labels_are_sent_as_their_values() {
        let fields = [FormField {
            key: "color".to_string(),
            multiselect: false,
            options: vec![("紅色".to_string(), "red".to_string())],
        }];
        assert_eq!(
            Value::Object(form_answer(Some(&fields), vec![vec!["紅色".to_string()]])),
            json!({ "color": "red" })
        );
        // 沒看過的 form 以預設鍵送文字。
        assert_eq!(
            Value::Object(form_answer(None, vec![vec!["A".to_string()], vec![]])),
            json!({ "q0": "A", "q1": "" })
        );
    }

    #[test]
    fn reply_routes_follow_the_v2_contract() {
        assert_eq!(
            form_url("http://server", "ses_1", "frm_1"),
            "http://server/api/session/ses_1/form/frm_1"
        );
        assert_eq!(
            permission_reply_url("http://server", "ses_1", "per_1"),
            "http://server/api/session/ses_1/permission/per_1/reply"
        );
        assert_eq!(
            serde_json::to_value(PermissionReplyBody { decision: "always" }).unwrap(),
            json!({ "decision": "always" })
        );
    }

    #[test]
    fn extracts_session_id_from_session_response() {
        let value = json!({ "data": { "id": "ses_123", "title": "New Session" } });
        assert_eq!(extract_session_id(&value).as_deref(), Some("ses_123"));
    }

    #[test]
    fn trims_base_url_once() {
        let backend =
            OpencodeServerBackend::from_env("http://opencode-server:4096///".to_string(), None);

        assert_eq!(backend.base_url, "http://opencode-server:4096");
    }

    #[test]
    fn parses_provider_model_into_a_v2_model_ref() {
        assert_eq!(
            parse_model_ref("opencode/deepseek-v4-flash-free"),
            Some(ModelRef {
                id: "deepseek-v4-flash-free".to_string(),
                provider_id: "opencode".to_string(),
            })
        );
        assert_eq!(parse_model_ref("no-provider"), None);
        assert_eq!(parse_model_ref("/model"), None);
    }
}
