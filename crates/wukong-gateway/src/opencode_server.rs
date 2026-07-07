mod event_map;
mod sse;

use crate::backend::{agent_timeout, AgentRequest, AgentResponse, AiBackend};
use crate::error::GatewayError;
use crate::stream::StreamEvent;
use event_map::{map_server_event, ServerEventAction};
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use sse::SseParser;
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
