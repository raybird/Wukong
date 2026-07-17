mod event_map;
mod sse;

use crate::backend::{agent_timeout, AgentRequest, AgentResponse, AiBackend};
use crate::error::GatewayError;
use crate::stream::StreamEvent;
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use event_map::{map_server_event, ServerEventAction};
use reqwest::{StatusCode, Url};
use serde::Serialize;
use serde_json::Value;
use sse::SseParser;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const INLINE_ATTACHMENT_MAX_BYTES: u64 = 10 * 1024 * 1024;

pub struct OpencodeServerBackend {
    pub base_url: String,
    pub workspace: Option<PathBuf>,
    file_mode: FileTransportMode,
    server_workspace: Option<PathBuf>,
    client: reqwest::Client,
    username: Option<String>,
    password: Option<String>,
    session_attachments: Mutex<HashMap<String, Vec<crate::backend::AgentAttachment>>>,
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
}

#[derive(Debug, Serialize)]
struct MessageBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<ModelOverride>,
    #[serde(skip_serializing_if = "Option::is_none")]
    agent: Option<String>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    tools: BTreeMap<String, bool>,
    parts: Vec<MessagePart>,
}

#[derive(Debug, Serialize)]
struct ModelOverride {
    #[serde(rename = "providerID")]
    provider_id: String,
    #[serde(rename = "modelID")]
    model_id: String,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "lowercase")]
enum MessagePart {
    Text {
        text: String,
    },
    File {
        mime: String,
        url: String,
        filename: String,
    },
}

#[derive(Debug, Serialize)]
struct QuestionReplyBody {
    answers: Vec<Vec<String>>,
}

#[derive(Debug, Serialize)]
struct PermissionReplyBody<'a> {
    reply: &'a str,
}

impl OpencodeServerBackend {
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
            username: std::env::var("WUKONG_AGENT_SERVER_USERNAME")
                .ok()
                .filter(|s| !s.is_empty()),
            password: std::env::var("WUKONG_AGENT_SERVER_PASSWORD")
                .ok()
                .filter(|s| !s.is_empty()),
            session_attachments: Mutex::new(HashMap::new()),
        }
    }

    async fn create_session(&self) -> Result<String, GatewayError> {
        let url = format!("{}/session", self.base_url);
        let value = self
            .send_json(
                "create_session",
                self.client.post(url).json(&CreateSessionBody {
                    title: "Wukong".to_string(),
                }),
            )
            .await?;
        extract_session_id(&value).ok_or_else(|| GatewayError::AgentFailed {
            code: None,
            stderr: format!("opencode server did not return a session id: {value}"),
        })
    }

    pub(crate) async fn health_check(&self) -> Result<(), GatewayError> {
        let url = format!("{}/global/health", self.base_url);
        self.send_json("health_check", self.client.get(url))
            .await
            .map(|_| ())
    }

    async fn build_message_parts(
        &self,
        req: &AgentRequest,
    ) -> Result<Vec<MessagePart>, GatewayError> {
        let mut parts = vec![MessagePart::Text {
            text: req.prompt.clone(),
        }];
        for attachment in &req.attachments {
            let mime = supported_attachment_mime(attachment)?;
            let filename = safe_display_filename(&attachment.original_name, &attachment.path);
            let path = self.validated_attachment_path(&attachment.path)?;
            let url = match &self.file_mode {
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
            parts.push(MessagePart::File {
                mime,
                url,
                filename,
            });
        }
        Ok(parts)
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

    async fn send_message(
        &self,
        session_id: &str,
        req: &AgentRequest,
    ) -> Result<Value, GatewayError> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let body = MessageBody {
            model: req.model.as_deref().and_then(parse_model_override),
            agent: req.agent.clone(),
            tools: req.tool_overrides.clone(),
            parts: self.build_message_parts(req).await?,
        };
        self.send_json("send_message", self.client.post(url).json(&body))
            .await
    }

    async fn send_message_async(
        &self,
        session_id: &str,
        req: &AgentRequest,
    ) -> Result<(), GatewayError> {
        let url = format!("{}/session/{}/prompt_async", self.base_url, session_id);
        let body = MessageBody {
            model: req.model.as_deref().and_then(parse_model_override),
            agent: req.agent.clone(),
            tools: req.tool_overrides.clone(),
            parts: self.build_message_parts(req).await?,
        };
        self.send_empty("prompt_async", self.client.post(url).json(&body))
            .await
    }

    async fn list_messages(&self, session_id: &str) -> Result<Value, GatewayError> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        self.send_json("list_messages", self.client.get(url)).await
    }

    pub async fn reply_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        let url = question_reply_url(&self.base_url, session_id, request_id);
        self.send_empty(
            "question_reply",
            self.client.post(url).json(&question_reply_body(answers)),
        )
        .await
    }

    pub async fn reject_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError> {
        let url = question_reject_url(&self.base_url, session_id, request_id);
        self.send_empty("question_reject", self.client.post(url))
            .await
    }

    pub async fn reply_permission(
        &self,
        request_id: &str,
        reply: &str,
    ) -> Result<(), GatewayError> {
        if !matches!(reply, "once" | "always" | "reject") {
            return Err(GatewayError::AgentFailed {
                code: None,
                stderr: format!("無效的 OpenCode permission reply：{reply}"),
            });
        }
        let url = permission_reply_url(&self.base_url, request_id);
        self.send_empty(
            "permission_reply",
            self.client.post(url).json(&PermissionReplyBody { reply }),
        )
        .await
    }

    async fn open_event_stream(&self) -> Result<reqwest::Response, GatewayError> {
        let url = format!("{}/event", self.base_url);
        let response = self
            .authorize(self.client.get(url))
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
        serde_json::from_str(&text).map_err(|err| GatewayError::AgentFailed {
            code: None,
            stderr: format!("opencode server {phase} returned invalid JSON: {err}; body: {text}"),
        })
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
        match self.password.as_deref() {
            Some(password) => request.basic_auth(
                self.username.as_deref().unwrap_or("opencode"),
                Some(password),
            ),
            None => request,
        }
    }

    async fn consume_event_stream(
        &self,
        mut response: reqwest::Response,
        session_id: &str,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), GatewayError> {
        let mut parser = SseParser::default();
        let mut buffer = String::new();
        let mut seen_tools = std::collections::HashSet::new();
        let deadline = tokio::time::sleep(agent_timeout());
        tokio::pin!(deadline);

        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk
                    .map_err(|err| http_error("event_stream failed while reading chunk", err))?,
                _ = &mut deadline => {
                    return Err(GatewayError::AgentFailed {
                        code: None,
                        stderr: "opencode server stream timed out before session became idle".to_string(),
                    });
                }
            };
            let Some(chunk) = chunk else {
                return Err(GatewayError::AgentFailed {
                    code: None,
                    stderr: "opencode server event stream ended before session became idle"
                        .to_string(),
                });
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(newline) = buffer.find('\n') {
                let mut line = buffer.drain(..=newline).collect::<String>();
                if line.ends_with('\n') {
                    line.pop();
                }
                if let Some(data) = parser.feed_line(&line) {
                    let value: Value = match serde_json::from_str(&data) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    match map_server_event(&value, session_id, &mut seen_tools) {
                        ServerEventAction::Emit(event) => on_event(event),
                        ServerEventAction::Idle => return Ok(()),
                        ServerEventAction::Ignore => {}
                    }
                }
            }
        }
    }
}

impl AiBackend for OpencodeServerBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        self.health_check().await?;

        let mut session_id = match req.session_id.clone() {
            Some(id) => id,
            None => self.create_session().await?,
        };

        let value = match self.send_message(&session_id, &req).await {
            Ok(value) => {
                self.remember_attachments(&session_id, &req);
                value
            }
            Err(GatewayError::AgentFailed {
                code: Some(code), ..
            }) if code == StatusCode::NOT_FOUND.as_u16() as i32 => {
                let retry = self.rehydrate_request(&session_id, &req);
                session_id = self.create_session().await?;
                let value = self.send_message(&session_id, &retry).await?;
                self.remember_attachments(&session_id, &retry);
                value
            }
            Err(err) => return Err(err),
        };

        Ok(AgentResponse {
            text: extract_text(&value).trim().to_string(),
            session_id: Some(session_id),
        })
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        self.health_check().await?;

        let mut session_id = match req.session_id.clone() {
            Some(id) => id,
            None => self.create_session().await?,
        };

        let response = self.open_event_stream().await?;
        match self.send_message_async(&session_id, &req).await {
            Ok(()) => self.remember_attachments(&session_id, &req),
            Err(GatewayError::AgentFailed {
                code: Some(code), ..
            }) if code == StatusCode::NOT_FOUND.as_u16() as i32 => {
                let retry = self.rehydrate_request(&session_id, &req);
                session_id = self.create_session().await?;
                self.send_message_async(&session_id, &retry).await?;
                self.remember_attachments(&session_id, &retry);
            }
            Err(err) => return Err(err),
        }

        self.consume_event_stream(response, &session_id, on_event)
            .await?;
        let messages = self.list_messages(&session_id).await?;
        Ok(AgentResponse {
            text: extract_latest_assistant_text(&messages),
            session_id: Some(session_id),
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
    value.get("id").and_then(Value::as_str).map(str::to_string)
}

fn extract_text(value: &Value) -> String {
    let mut out = Vec::new();
    collect_text(value, &mut out);
    out.join("\n")
}

fn extract_latest_assistant_text(value: &Value) -> String {
    let Some(messages) = value.as_array() else {
        return String::new();
    };
    messages
        .iter()
        .rev()
        .find(|message| {
            message
                .get("info")
                .and_then(|info| info.get("role"))
                .and_then(Value::as_str)
                == Some("assistant")
        })
        .map(extract_text)
        .unwrap_or_default()
        .trim()
        .to_string()
}

fn collect_text(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let is_text_part = map
                .get("type")
                .and_then(Value::as_str)
                .map(|kind| kind == "text" || kind == "assistant_text")
                .unwrap_or(false);
            if is_text_part {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        out.push(text.to_string());
                    }
                }
            }
            for child in map.values() {
                collect_text(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text(item, out);
            }
        }
        _ => {}
    }
}

fn parse_model_override(model: &str) -> Option<ModelOverride> {
    let (provider_id, model_id) = model.trim().split_once('/')?;
    if provider_id.is_empty() || model_id.is_empty() {
        return None;
    }
    Some(ModelOverride {
        provider_id: provider_id.to_string(),
        model_id: model_id.to_string(),
    })
}

fn http_error(phase: &'static str, err: reqwest::Error) -> GatewayError {
    GatewayError::AgentFailed {
        code: None,
        stderr: format!("opencode server {phase} failed: {err}"),
    }
}

fn question_reply_body(answers: Vec<Vec<String>>) -> QuestionReplyBody {
    QuestionReplyBody { answers }
}

fn question_reply_url(base_url: &str, session_id: &str, request_id: &str) -> String {
    format!(
        "{}/api/session/{}/question/{}/reply",
        base_url, session_id, request_id
    )
}

fn question_reject_url(base_url: &str, session_id: &str, request_id: &str) -> String {
    format!(
        "{}/api/session/{}/question/{}/reject",
        base_url, session_id, request_id
    )
}

fn permission_reply_url(base_url: &str, request_id: &str) -> String {
    format!("{}/permission/{}/reply", base_url, request_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AgentAttachment;
    use serde_json::json;
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
        }
    }

    #[tokio::test]
    async fn server_backend_builds_shared_file_parts() {
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

        let parts = backend
            .build_message_parts(&request_with_attachment(
                path,
                "../quarter report.pdf",
                "application/pdf",
            ))
            .await
            .unwrap();

        assert_eq!(
            serde_json::to_value(parts).unwrap(),
            json!([
                { "type": "text", "text": "describe" },
                {
                    "type": "file",
                    "mime": "application/pdf",
                    "url": "file:///workspace/.wukong/uploads/user/42/quarter%20report.pdf",
                    "filename": "quarter report.pdf"
                }
            ])
        );
    }

    #[tokio::test]
    async fn server_backend_builds_inline_file_parts() {
        let workspace = tempfile::tempdir().unwrap();
        let path = workspace.path().join("data.csv");
        std::fs::write(&path, b"a,b\n1,2\n").unwrap();
        let mut backend = OpencodeServerBackend::from_env(
            "http://127.0.0.1:1".to_string(),
            Some(workspace.path().to_path_buf()),
        );
        backend.file_mode = FileTransportMode::Inline;

        let parts = backend
            .build_message_parts(&request_with_attachment(path, "data.csv", "text/csv"))
            .await
            .unwrap();

        assert_eq!(
            serde_json::to_value(parts).unwrap(),
            json!([
                { "type": "text", "text": "describe" },
                {
                    "type": "file",
                    "mime": "text/plain",
                    "url": "data:text/plain;base64,YSxiCjEsMgo=",
                    "filename": "data.csv"
                }
            ])
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
            .build_message_parts(&request_with_attachment(
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
            .consume_event_stream(response, "ses_1", &mut |_| {})
            .await
            .unwrap_err();
        let stderr = agent_failed_stderr(err);

        assert!(stderr.contains("opencode server event_stream failed while reading chunk"));
        assert!(stderr.contains("error decoding response body"));
    }

    #[test]
    fn question_reply_body_serializes_answers() {
        let body = question_reply_body(vec![
            vec!["A".to_string()],
            vec!["B".to_string(), "C".to_string()],
        ]);

        assert_eq!(
            serde_json::to_value(body).unwrap(),
            json!({ "answers": [["A"], ["B", "C"]] })
        );
    }

    #[test]
    fn question_api_urls_target_session_scoped_routes() {
        assert_eq!(
            question_reply_url("http://server", "ses_1", "que_1"),
            "http://server/api/session/ses_1/question/que_1/reply"
        );
        assert_eq!(
            question_reject_url("http://server", "ses_1", "que_1"),
            "http://server/api/session/ses_1/question/que_1/reject"
        );
    }

    #[test]
    fn permission_api_uses_current_reply_contract() {
        assert_eq!(
            permission_reply_url("http://server", "per_1"),
            "http://server/permission/per_1/reply"
        );
        let body = PermissionReplyBody { reply: "always" };
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            json!({ "reply": "always" })
        );
    }

    #[test]
    fn extracts_latest_assistant_text_from_message_list() {
        let value = json!([
            {
                "info": { "id": "msg_user", "role": "user", "sessionID": "ses_1" },
                "parts": [{ "type": "text", "text": "question" }]
            },
            {
                "info": { "id": "msg_old", "role": "assistant", "sessionID": "ses_1" },
                "parts": [{ "type": "text", "text": "old" }]
            },
            {
                "info": { "id": "msg_new", "role": "assistant", "sessionID": "ses_1" },
                "parts": [
                    { "type": "reasoning", "text": "hidden" },
                    { "type": "text", "text": "hello" },
                    { "type": "text", "text": "world" }
                ]
            }
        ]);

        assert_eq!(extract_latest_assistant_text(&value), "hello\nworld");
    }

    #[test]
    fn latest_assistant_text_is_empty_when_absent() {
        let value = json!([
            {
                "info": { "id": "msg_user", "role": "user", "sessionID": "ses_1" },
                "parts": [{ "type": "text", "text": "question" }]
            }
        ]);

        assert_eq!(extract_latest_assistant_text(&value), "");
    }

    #[test]
    fn extracts_text_from_nested_message_parts() {
        let value = json!({
            "info": { "id": "msg_1" },
            "parts": [
                { "type": "reasoning", "text": "hidden" },
                { "type": "text", "text": "hello" },
                { "type": "text", "text": "world" }
            ]
        });

        assert_eq!(extract_text(&value), "hello\nworld");
    }

    #[test]
    fn extracts_session_id_from_session_response() {
        let value = json!({ "id": "ses_123", "title": "New Session" });
        assert_eq!(extract_session_id(&value).as_deref(), Some("ses_123"));
    }

    #[test]
    fn trims_base_url_once() {
        let backend =
            OpencodeServerBackend::from_env("http://opencode-server:4096///".to_string(), None);

        assert_eq!(backend.base_url, "http://opencode-server:4096");
    }

    #[test]
    fn parses_provider_model_override_for_server_api() {
        let model = parse_model_override("opencode/deepseek-v4-flash-free").unwrap();

        assert_eq!(model.provider_id, "opencode");
        assert_eq!(model.model_id, "deepseek-v4-flash-free");
    }
}
