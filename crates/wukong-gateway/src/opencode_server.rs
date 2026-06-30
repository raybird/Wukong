use crate::backend::{agent_timeout, AgentRequest, AgentResponse, AiBackend};
use crate::error::GatewayError;
use crate::stream::StreamEvent;
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
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
            .send_json(self.client.post(url).json(&CreateSessionBody {
                title: "Wukong".to_string(),
            }))
            .await?;
        extract_session_id(&value).ok_or_else(|| GatewayError::AgentFailed {
            code: None,
            stderr: format!("opencode server did not return a session id: {value}"),
        })
    }

    async fn health_check(&self) -> Result<(), GatewayError> {
        let url = format!("{}/global/health", self.base_url);
        self.send_json(self.client.get(url)).await.map(|_| ())
    }

    async fn send_message(
        &self,
        session_id: &str,
        req: &AgentRequest,
    ) -> Result<Value, GatewayError> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let body = MessageBody {
            model: req.model.as_deref().and_then(parse_model_override),
            parts: vec![MessagePart {
                kind: "text",
                text: req.prompt.clone(),
            }],
        };
        self.send_json(self.client.post(url).json(&body)).await
    }

    async fn send_message_async(
        &self,
        session_id: &str,
        req: &AgentRequest,
    ) -> Result<(), GatewayError> {
        let url = format!("{}/session/{}/prompt_async", self.base_url, session_id);
        let body = MessageBody {
            model: req.model.as_deref().and_then(parse_model_override),
            parts: vec![MessagePart {
                kind: "text",
                text: req.prompt.clone(),
            }],
        };
        self.send_empty(self.client.post(url).json(&body)).await
    }

    async fn list_messages(&self, session_id: &str) -> Result<Value, GatewayError> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        self.send_json(self.client.get(url)).await
    }

    async fn send_json(&self, request: reqwest::RequestBuilder) -> Result<Value, GatewayError> {
        let request = self.authorize(request);
        let response = request.send().await.map_err(http_error)?;
        let status = response.status();
        let text = response.text().await.map_err(http_error)?;
        if !status.is_success() {
            return Err(GatewayError::AgentFailed {
                code: Some(status.as_u16() as i32),
                stderr: format!("opencode server returned {status}: {text}"),
            });
        }
        serde_json::from_str(&text).map_err(|err| GatewayError::AgentFailed {
            code: None,
            stderr: format!("opencode server returned invalid JSON: {err}; body: {text}"),
        })
    }

    async fn send_empty(&self, request: reqwest::RequestBuilder) -> Result<(), GatewayError> {
        let request = self.authorize(request);
        let response = request.send().await.map_err(http_error)?;
        let status = response.status();
        let text = response.text().await.map_err(http_error)?;
        if !status.is_success() {
            return Err(GatewayError::AgentFailed {
                code: Some(status.as_u16() as i32),
                stderr: format!("opencode server returned {status}: {text}"),
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
}

impl AiBackend for OpencodeServerBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
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
        let resp = self.run(req).await?;
        if !resp.text.is_empty() {
            on_event(StreamEvent::Text(resp.text.clone()));
        }
        Ok(resp)
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

fn http_error(err: reqwest::Error) -> GatewayError {
    GatewayError::AgentFailed {
        code: None,
        stderr: format!("opencode server request failed: {err}"),
    }
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
    if event_type != "message.part.updated" {
        return ServerEventAction::Ignore;
    }

    let part = match properties.get("part") {
        Some(part) => part,
        None => return ServerEventAction::Ignore,
    };
    if part.get("sessionID").and_then(Value::as_str) != Some(session_id) {
        return ServerEventAction::Ignore;
    }

    match part
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or_default()
    {
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
            ServerEventAction::Emit(StreamEvent::ToolUse(name.to_string()))
        }
        "step-start" => ServerEventAction::Emit(StreamEvent::StepStart),
        "step-finish" => ServerEventAction::Emit(StreamEvent::StepFinish),
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
    use serde_json::json;

    #[test]
    fn sse_parser_collects_single_data_event() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line("data: {\"hello\":true}"), None);
        assert_eq!(
            parser.feed_line(""),
            Some("{\"hello\":true}".to_string())
        );
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
