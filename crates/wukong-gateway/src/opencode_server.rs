use crate::backend::{agent_timeout, AgentRequest, AgentResponse, AiBackend};
use crate::error::GatewayError;
use crate::stream::{QuestionInfo, QuestionOption, QuestionRequest, StreamEvent};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::PathBuf;

pub struct OpencodeServerBackend {
    pub base_url: String,
    pub workspace: Option<PathBuf>,
    client: reqwest::Client,
    username: Option<String>,
    password: Option<String>,
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

#[derive(Debug, Serialize)]
struct MessagePart {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

#[derive(Debug, Serialize)]
struct QuestionReplyBody {
    answers: Vec<Vec<String>>,
}

impl OpencodeServerBackend {
    pub fn from_env(base_url: String, workspace: Option<PathBuf>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            workspace,
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

    async fn health_check(&self) -> Result<(), GatewayError> {
        let url = format!("{}/global/health", self.base_url);
        self.send_json("health_check", self.client.get(url))
            .await
            .map(|_| ())
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
            parts: vec![MessagePart {
                kind: "text",
                text: req.prompt.clone(),
            }],
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
            parts: vec![MessagePart {
                kind: "text",
                text: req.prompt.clone(),
            }],
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
        if !req.attachments.is_empty() {
            return Err(attachments_unsupported());
        }
        self.health_check().await?;

        let mut session_id = match req.session_id.clone() {
            Some(id) => id,
            None => self.create_session().await?,
        };

        let value = match self.send_message(&session_id, &req).await {
            Ok(value) => value,
            Err(GatewayError::AgentFailed {
                code: Some(code), ..
            }) if code == StatusCode::NOT_FOUND.as_u16() as i32 => {
                session_id = self.create_session().await?;
                self.send_message(&session_id, &req).await?
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
        if !req.attachments.is_empty() {
            return Err(attachments_unsupported());
        }
        self.health_check().await?;

        let mut session_id = match req.session_id.clone() {
            Some(id) => id,
            None => self.create_session().await?,
        };

        let response = self.open_event_stream().await?;
        match self.send_message_async(&session_id, &req).await {
            Ok(()) => {}
            Err(GatewayError::AgentFailed {
                code: Some(code), ..
            }) if code == StatusCode::NOT_FOUND.as_u16() as i32 => {
                session_id = self.create_session().await?;
                self.send_message_async(&session_id, &req).await?;
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

fn attachments_unsupported() -> GatewayError {
    GatewayError::AgentFailed {
        code: None,
        stderr: "目前的 opencode server backend 不支援附件輸入；請改用 CLI backend 或等待 server file parts 支援。"
            .to_string(),
    }
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

#[derive(Default)]
struct SseParser {
    data_lines: Vec<String>,
}

impl SseParser {
    fn feed_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if self.data_lines.is_empty() {
                return None;
            }
            return Some(std::mem::take(&mut self.data_lines).join("\n"));
        }
        if line.starts_with(':') {
            return None;
        }
        if let Some(data) = line.strip_prefix("data:") {
            self.data_lines
                .push(data.strip_prefix(' ').unwrap_or(data).to_string());
        }
        None
    }
}

fn format_tool_use_name(part: &Value, name: &str) -> String {
    if name == "question" {
        return format_question_tool_use(part, name);
    }

    let Some(input) = part.get("state").and_then(|state| state.get("input")) else {
        return name.to_string();
    };
    let fields: &[&str] = match name {
        "bash" => &["command"],
        "read" => &["filePath"],
        "grep" => &["pattern", "path", "include"],
        "glob" => &["pattern", "path"],
        _ => return name.to_string(),
    };

    let mut lines = vec![name.to_string()];
    for field in fields {
        if let Some(value) = input.get(field).and_then(Value::as_str) {
            if !value.is_empty() {
                lines.push(format!("  {field}: {}", truncate_tool_value(value)));
            }
        }
    }

    if lines.len() == 1 {
        name.to_string()
    } else {
        lines.join("\n")
    }
}

fn format_question_tool_use(part: &Value, name: &str) -> String {
    let Some(questions) = part
        .get("state")
        .and_then(|state| state.get("input"))
        .and_then(|input| input.get("questions"))
        .and_then(Value::as_array)
    else {
        return name.to_string();
    };

    let mut lines = vec![name.to_string()];
    for question in questions {
        let prompt = question.get("question").and_then(Value::as_str);
        let header = question.get("header").and_then(Value::as_str);
        match (header, prompt) {
            (Some(header), Some(prompt)) if !header.is_empty() && !prompt.is_empty() => {
                lines.push(format!("  {header}: {}", truncate_tool_value(prompt)));
            }
            (_, Some(prompt)) if !prompt.is_empty() => {
                lines.push(format!("  {}", truncate_tool_value(prompt)));
            }
            _ => {}
        }

        if let Some(options) = question.get("options").and_then(Value::as_array) {
            for (idx, option) in options.iter().enumerate() {
                let label = option.get("label").and_then(Value::as_str).unwrap_or("");
                if label.is_empty() {
                    continue;
                }
                let description = option
                    .get("description")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if description.is_empty() {
                    lines.push(format!("  {}. {}", idx + 1, truncate_tool_value(label)));
                } else {
                    lines.push(format!(
                        "  {}. {} - {}",
                        idx + 1,
                        truncate_tool_value(label),
                        truncate_tool_value(description)
                    ));
                }
            }
        }
    }

    if lines.len() == 1 {
        name.to_string()
    } else {
        lines.join("\n")
    }
}

fn parse_question_request(properties: &Value) -> Option<QuestionRequest> {
    let request_id = properties.get("id")?.as_str()?.to_string();
    let session_id = properties.get("sessionID")?.as_str()?.to_string();
    let questions = properties
        .get("questions")?
        .as_array()?
        .iter()
        .filter_map(parse_question_info)
        .collect::<Vec<_>>();
    if questions.is_empty() {
        return None;
    }
    Some(QuestionRequest {
        request_id,
        session_id,
        questions,
    })
}

fn parse_question_info(value: &Value) -> Option<QuestionInfo> {
    let question = value.get("question")?.as_str()?.to_string();
    let header = value
        .get("header")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let multiple = value
        .get("multiple")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let custom = value.get("custom").and_then(Value::as_bool).unwrap_or(true);
    let options = value
        .get("options")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|option| {
                    Some(QuestionOption {
                        label: option.get("label")?.as_str()?.to_string(),
                        description: option
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default();

    Some(QuestionInfo {
        question,
        header,
        options,
        multiple,
        custom,
    })
}

fn truncate_tool_value(value: &str) -> String {
    const MAX_CHARS: usize = 120;
    let mut chars = value.chars();
    let truncated: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!(
            "{}...",
            truncated.chars().take(MAX_CHARS - 3).collect::<String>()
        )
    } else {
        truncated
    }
}

#[derive(Debug, PartialEq)]
enum ServerEventAction {
    Emit(StreamEvent),
    Idle,
    Ignore,
}

fn map_server_event(
    value: &Value,
    session_id: &str,
    seen_tools: &mut std::collections::HashSet<String>,
) -> ServerEventAction {
    let payload = match value.get("payload") {
        Some(payload) => payload,
        None => value,
    };
    let event_type = payload
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let properties = payload.get("properties").unwrap_or(payload);

    if event_type == "session.idle" {
        return match event_session_id(properties).as_deref() {
            Some(id) if id == session_id => ServerEventAction::Idle,
            _ => ServerEventAction::Ignore,
        };
    }
    if event_type == "session.status" {
        let is_idle = properties
            .get("status")
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
            .map(|kind| kind == "idle")
            .unwrap_or(false);
        return match (event_session_id(properties).as_deref(), is_idle) {
            (Some(id), true) if id == session_id => ServerEventAction::Idle,
            _ => ServerEventAction::Ignore,
        };
    }
    if event_type == "question.asked" {
        return match parse_question_request(properties) {
            Some(request) if request.session_id == session_id => {
                ServerEventAction::Emit(StreamEvent::QuestionRequest(request))
            }
            _ => ServerEventAction::Ignore,
        };
    }
    if event_type != "message.part.updated" {
        return ServerEventAction::Ignore;
    }

    let part = match properties.get("part") {
        Some(part) => part,
        None => return ServerEventAction::Ignore,
    };
    let matches_session = part.get("sessionID").and_then(Value::as_str) == Some(session_id)
        || event_session_id(properties).as_deref() == Some(session_id);
    if !matches_session {
        return ServerEventAction::Ignore;
    }

    match part.get("type").and_then(Value::as_str).unwrap_or_default() {
        "reasoning" => {
            let text = properties
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| part.get("text").and_then(Value::as_str))
                .unwrap_or_default();
            if text.trim().is_empty() {
                ServerEventAction::Ignore
            } else {
                ServerEventAction::Emit(StreamEvent::Reasoning(text.to_string()))
            }
        }
        "tool" => {
            let dedupe_key = part
                .get("callID")
                .and_then(Value::as_str)
                .or_else(|| part.get("id").and_then(Value::as_str))
                .unwrap_or("tool")
                .to_string();
            if !seen_tools.insert(dedupe_key) {
                return ServerEventAction::Ignore;
            }
            let name = part.get("tool").and_then(Value::as_str).unwrap_or("tool");
            if name == "question" {
                return ServerEventAction::Ignore;
            }
            ServerEventAction::Emit(StreamEvent::ToolUse(format_tool_use_name(part, name)))
        }
        "step-start" => ServerEventAction::Emit(StreamEvent::StepStart),
        "step-finish" => ServerEventAction::Emit(StreamEvent::StepFinish),
        // `text` parts are intentionally NOT streamed here. The server backend
        // fetches the final assistant text once via `list_messages` at the end
        // of `run` (see `extract_latest_assistant_text`); emitting text deltas
        // too would double-render the answer. Only reasoning/tool/step activity
        // is streamed live. The CLI backend does stream text — this deliberate
        // difference is documented in docs/entrypoints.md.
        _ => ServerEventAction::Ignore,
    }
}

fn event_session_id(properties: &Value) -> Option<String> {
    properties
        .get("sessionID")
        .and_then(Value::as_str)
        .or_else(|| {
            properties
                .get("session")
                .and_then(|session| session.get("id"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            properties
                .get("info")
                .and_then(|info| info.get("id"))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
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

    #[tokio::test]
    async fn server_backend_rejects_attachments_explicitly() {
        let backend = OpencodeServerBackend::from_env("http://127.0.0.1:1".to_string(), None);
        let err = backend
            .run_streaming(
                AgentRequest {
                    prompt: "describe".to_string(),
                    session_id: None,
                    thinking: false,
                    model: None,
                    agent: None,
                    tool_overrides: BTreeMap::new(),
                    attachments: vec![AgentAttachment {
                        path: std::path::PathBuf::from("/tmp/report.pdf"),
                        original_name: "report.pdf".to_string(),
                        mime_type: Some("application/pdf".to_string()),
                    }],
                },
                &mut |_| {},
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("不支援附件輸入"), "{err}");
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
    fn sse_parser_collects_single_data_event() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line("data: {\"hello\":true}"), None);
        assert_eq!(parser.feed_line(""), Some("{\"hello\":true}".to_string()));
    }

    #[test]
    fn sse_parser_joins_multiline_data_and_ignores_comments() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line(": keep-alive"), None);
        assert_eq!(parser.feed_line("event: message"), None);
        assert_eq!(parser.feed_line("data: {\"a\":"), None);
        assert_eq!(parser.feed_line("data: 1}"), None);

        assert_eq!(parser.feed_line(""), Some("{\"a\":\n1}".to_string()));
    }

    #[test]
    fn sse_parser_ignores_blank_events() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line("event: ping"), None);
        assert_eq!(parser.feed_line(""), None);
    }

    #[test]
    fn maps_reasoning_delta_for_matching_session() {
        let value = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "delta": "thinking",
                    "part": {
                        "id": "part_1",
                        "sessionID": "ses_1",
                        "messageID": "msg_1",
                        "type": "reasoning",
                        "text": "thinking total"
                    }
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::Reasoning("thinking".to_string()))
        );
    }

    #[test]
    fn maps_reasoning_text_when_delta_missing() {
        let value = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": {
                        "id": "part_1",
                        "sessionID": "ses_1",
                        "messageID": "msg_1",
                        "type": "reasoning",
                        "text": "thinking total"
                    }
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::Reasoning("thinking total".to_string()))
        );
    }

    #[test]
    fn maps_part_updates_when_session_id_is_on_properties() {
        let reasoning = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "sessionID": "ses_1",
                    "delta": "thinking",
                    "part": {
                        "id": "part_1",
                        "messageID": "msg_1",
                        "type": "reasoning",
                        "text": "thinking total"
                    }
                }
            }
        });
        let tool = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "sessionID": "ses_1",
                    "part": {
                        "id": "part_tool",
                        "messageID": "msg_1",
                        "type": "tool",
                        "callID": "call_1",
                        "tool": "bash"
                    }
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&reasoning, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::Reasoning("thinking".to_string()))
        );
        assert_eq!(
            map_server_event(&tool, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::ToolUse("bash".to_string()))
        );
    }

    #[test]
    fn maps_tool_use_once_per_call_id() {
        let value = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": {
                        "id": "part_tool",
                        "sessionID": "ses_1",
                        "messageID": "msg_1",
                        "type": "tool",
                        "callID": "call_1",
                        "tool": "bash"
                    }
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::ToolUse("bash".to_string()))
        );
        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Ignore
        );
    }

    #[test]
    fn maps_question_asked_to_question_request() {
        let value = json!({
            "payload": {
                "type": "question.asked",
                "properties": {
                    "id": "que_1",
                    "sessionID": "ses_1",
                    "questions": [{
                        "question": "要怎麼處理 question 工具顯示？",
                        "header": "顯示方式",
                        "multiple": true,
                        "custom": true,
                        "options": [
                            {
                                "label": "輸出選項",
                                "description": "遇到 question 時直接列出可選項目"
                            },
                            {
                                "label": "查文件",
                                "description": "先確認是否有官方格式可轉換"
                            }
                        ]
                    }]
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::QuestionRequest(QuestionRequest {
                request_id: "que_1".to_string(),
                session_id: "ses_1".to_string(),
                questions: vec![QuestionInfo {
                    question: "要怎麼處理 question 工具顯示？".to_string(),
                    header: "顯示方式".to_string(),
                    multiple: true,
                    custom: true,
                    options: vec![
                        QuestionOption {
                            label: "輸出選項".to_string(),
                            description: "遇到 question 時直接列出可選項目".to_string(),
                        },
                        QuestionOption {
                            label: "查文件".to_string(),
                            description: "先確認是否有官方格式可轉換".to_string(),
                        },
                    ],
                }],
            }))
        );
    }

    #[test]
    fn ignores_question_tool_part_update_as_progress() {
        let value = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": {
                        "id": "part_tool",
                        "sessionID": "ses_1",
                        "messageID": "msg_1",
                        "type": "tool",
                        "callID": "call_1",
                        "tool": "question",
                        "state": {
                            "status": "running",
                            "input": {
                                "questions": [{
                                    "question": "要怎麼處理 question 工具顯示？",
                                    "header": "顯示方式",
                                    "options": []
                                }]
                            }
                        }
                    }
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Ignore
        );
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
    fn maps_whitelisted_tool_use_input_summaries() {
        fn tool_part(tool: &str, input: serde_json::Value) -> serde_json::Value {
            json!({
                "payload": {
                    "type": "message.part.updated",
                    "properties": {
                        "part": {
                            "id": format!("part_{tool}"),
                            "sessionID": "ses_1",
                            "messageID": "msg_1",
                            "type": "tool",
                            "callID": format!("call_{tool}"),
                            "tool": tool,
                            "state": {
                                "status": "running",
                                "input": input
                            }
                        }
                    }
                }
            })
        }

        let cases = [
            (
                tool_part(
                    "bash",
                    json!({
                        "command": "cargo test -p wukong-gateway",
                        "secret": "do not show"
                    }),
                ),
                "bash\n  command: cargo test -p wukong-gateway",
            ),
            (
                tool_part(
                    "read",
                    json!({
                        "filePath": "/workspace/crates/wukong-gateway/src/opencode_server.rs"
                    }),
                ),
                "read\n  filePath: /workspace/crates/wukong-gateway/src/opencode_server.rs",
            ),
            (
                tool_part(
                    "grep",
                    json!({
                        "pattern": "ToolUse",
                        "path": "/workspace/crates",
                        "include": "*.rs"
                    }),
                ),
                "grep\n  pattern: ToolUse\n  path: /workspace/crates\n  include: *.rs",
            ),
            (
                tool_part(
                    "glob",
                    json!({
                        "pattern": "**/*.rs",
                        "path": "/workspace/crates"
                    }),
                ),
                "glob\n  pattern: **/*.rs\n  path: /workspace/crates",
            ),
            (
                tool_part(
                    "webfetch",
                    json!({
                        "url": "https://example.com",
                        "format": "markdown"
                    }),
                ),
                "webfetch",
            ),
        ];

        let mut seen_tools = std::collections::HashSet::new();
        for (value, expected) in cases {
            assert_eq!(
                map_server_event(&value, "ses_1", &mut seen_tools),
                ServerEventAction::Emit(StreamEvent::ToolUse(expected.to_string()))
            );
        }

        let long_command = "a".repeat(130);
        let expected_long_command = format!("bash\n  command: {}...", "a".repeat(117));
        let value = tool_part("bash", json!({ "command": long_command }));
        let mut seen_tools = std::collections::HashSet::new();
        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::ToolUse(expected_long_command))
        );
    }

    #[test]
    fn maps_step_boundaries_and_idle() {
        let mut seen_tools = std::collections::HashSet::new();
        let step_start = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": { "id": "s1", "sessionID": "ses_1", "type": "step-start" }
                }
            }
        });
        let step_finish = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": { "id": "s2", "sessionID": "ses_1", "type": "step-finish" }
                }
            }
        });
        let idle = json!({
            "payload": {
                "type": "session.idle",
                "properties": { "sessionID": "ses_1" }
            }
        });

        assert_eq!(
            map_server_event(&step_start, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::StepStart)
        );
        assert_eq!(
            map_server_event(&step_finish, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::StepFinish)
        );
        assert_eq!(
            map_server_event(&idle, "ses_1", &mut seen_tools),
            ServerEventAction::Idle
        );
    }

    #[test]
    fn ignores_events_for_other_sessions_and_text_parts() {
        let mut seen_tools = std::collections::HashSet::new();
        let other = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "delta": "hidden",
                    "part": { "id": "p", "sessionID": "ses_2", "type": "reasoning" }
                }
            }
        });
        let text = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "delta": "answer",
                    "part": { "id": "p", "sessionID": "ses_1", "type": "text", "text": "answer" }
                }
            }
        });

        assert_eq!(
            map_server_event(&other, "ses_1", &mut seen_tools),
            ServerEventAction::Ignore
        );
        assert_eq!(
            map_server_event(&text, "ses_1", &mut seen_tools),
            ServerEventAction::Ignore
        );
    }

    #[test]
    fn maps_sse_payload_sequence_to_stream_events_until_idle() {
        let payloads = vec![
            json!({
                "payload": {
                    "type": "message.part.updated",
                    "properties": {
                        "delta": "think",
                        "part": { "id": "r1", "sessionID": "ses_1", "type": "reasoning" }
                    }
                }
            }),
            json!({
                "payload": {
                    "type": "message.part.updated",
                    "properties": {
                        "part": { "id": "t1", "sessionID": "ses_1", "type": "tool", "tool": "bash" }
                    }
                }
            }),
            json!({
                "payload": {
                    "type": "session.idle",
                    "properties": { "sessionID": "ses_1" }
                }
            }),
        ];
        let mut seen_tools = std::collections::HashSet::new();
        let mut events = Vec::new();
        let mut idle = false;

        for payload in payloads {
            match map_server_event(&payload, "ses_1", &mut seen_tools) {
                ServerEventAction::Emit(event) => events.push(event),
                ServerEventAction::Idle => {
                    idle = true;
                    break;
                }
                ServerEventAction::Ignore => {}
            }
        }

        assert!(idle);
        assert_eq!(
            events,
            vec![
                StreamEvent::Reasoning("think".to_string()),
                StreamEvent::ToolUse("bash".to_string()),
            ]
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
