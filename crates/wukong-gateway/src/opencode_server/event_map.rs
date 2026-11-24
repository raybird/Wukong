//! OpenCode V2 `/api/event` 事件到 [`StreamEvent`] 的對應。
//!
//! V2 的事件形狀為 `{"type": "...", "data": {"sessionID": ..., ...}}`，對照見
//! `docs/issues/issue-0044/probe-events.md`，測試以 `tests/fixtures/opencode-v2/` 的真實樣本重播。

use crate::stream::{
    QuestionInfo, QuestionOption, QuestionRequest, StreamEvent, PERMISSION_ALLOW_ALWAYS_LABEL,
    PERMISSION_ALLOW_ONCE_LABEL, PERMISSION_REJECT_LABEL, PERMISSION_REQUEST_PREFIX,
};
use crate::upstream_error::{parse_v2_error, RetryWatch, UpstreamError};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, PartialEq)]
pub(super) enum ServerEventAction {
    Emit(StreamEvent),
    /// 本 session 的執行成功結束。
    Idle,
    /// 執行失敗、被中止或壓縮失敗。**必須**中止這次回合並回 `Err`：失敗的回合不能以
    /// 成功收場（實測：中止後仍有 step 事件，只看「結束了」會把它當成正常完成）。
    Failed(UpstreamError),
    Ignore,
}

pub(super) fn event_type_of(value: &Value) -> &str {
    value
        .get("type")
        .and_then(Value::as_str)
        .unwrap_or("unknown")
}

/// 一次回合的事件對應。V2 的工具名稱只出現在 `session.tool.input.started`，輸入在之後的
/// `session.tool.called`；回覆文字在 `session.text.ended`，要記到回合結束才知道哪則是最後的。
pub(super) struct ServerEventMapper {
    session_id: String,
    tool_names: HashMap<String, String>,
    last_message: Option<String>,
    texts: Vec<(String, String)>,
    pub(super) retry: RetryWatch,
    /// 使用者取消了提問或拒絕了權限。V2 之後會以 `execution.interrupted`（reason
    /// `shutdown`）結束執行；V1 1.19.6 同樣情況是不帶錯誤的 `session.idle`，回合照常收尾
    /// （2026-11-24 兩版實測，見 docs/issues/issue-0044/probe-reply.md）。
    declined: bool,
}

impl ServerEventMapper {
    pub(super) fn new(session_id: &str, retry: RetryWatch) -> Self {
        Self {
            session_id: session_id.to_string(),
            tool_names: HashMap::new(),
            last_message: None,
            texts: Vec::new(),
            retry,
            declined: false,
        }
    }

    /// 最後一則 assistant 訊息的文字。與 V1 取「最新一則 assistant message 的 text part」
    /// 相同：最後一步沒有文字時是空字串，交給 runtime 的收尾處理，不拿前面步驟的文字頂替。
    pub(super) fn final_text(&self) -> String {
        let Some(last) = self.last_message.as_deref() else {
            return String::new();
        };
        self.texts
            .iter()
            .filter(|(message, text)| message == last && !text.trim().is_empty())
            .map(|(_, text)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n")
            .trim()
            .to_string()
    }

    pub(super) fn map(&mut self, value: &Value) -> ServerEventAction {
        let event_type = event_type_of(value);
        let Some(data) = value.get("data") else {
            return ServerEventAction::Ignore;
        };
        // form 的 session 在 `data.form` 底下，其他事件都在 `data`。
        let session = data
            .get("sessionID")
            .or_else(|| data.get("form").and_then(|form| form.get("sessionID")))
            .and_then(Value::as_str);
        // 不是本 session 的事件（含子 agent 的 session）一律忽略：別人的錯誤不能中止本回合。
        if session != Some(self.session_id.as_str()) {
            return ServerEventAction::Ignore;
        }
        if let Some(message) = data.get("assistantMessageID").and_then(Value::as_str) {
            if matches!(event_type, "session.step.started" | "session.text.ended") {
                self.last_message = Some(message.to_string());
            }
        }

        // 重試之後模型有了實際輸出才算恢復；重試前後都會有 step.started，不能拿來判斷。
        if matches!(
            event_type,
            "session.step.ended"
                | "session.reasoning.delta"
                | "session.text.delta"
                | "session.text.ended"
                | "session.tool.called"
        ) {
            self.retry.recovered();
        }

        match event_type {
            "session.retry.scheduled" => {
                self.retry
                    .retrying(data.get("error").and_then(parse_v2_error));
                ServerEventAction::Ignore
            }
            "session.execution.succeeded" => ServerEventAction::Idle,
            "session.execution.failed" | "session.compaction.failed" => {
                ServerEventAction::Failed(data.get("error").and_then(parse_v2_error).unwrap_or(
                    UpstreamError {
                        name: "UnknownError".to_string(),
                        status_code: None,
                        message: format!("{event_type} without error"),
                    },
                ))
            }
            "form.cancelled" => {
                self.declined = true;
                ServerEventAction::Ignore
            }
            "permission.replied" => {
                if data.get("reply").and_then(Value::as_str) == Some("reject") {
                    self.declined = true;
                }
                ServerEventAction::Ignore
            }
            "session.execution.interrupted" if self.declined => ServerEventAction::Idle,
            "session.execution.interrupted" => ServerEventAction::Failed(UpstreamError {
                name: "aborted".to_string(),
                status_code: None,
                message: format!(
                    "execution interrupted ({})",
                    data.get("reason")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown")
                ),
            }),
            "session.step.started" => ServerEventAction::Emit(StreamEvent::StepStart),
            "session.step.ended" => ServerEventAction::Emit(StreamEvent::StepFinish),
            "session.reasoning.delta" => match data.get("delta").and_then(Value::as_str) {
                Some(delta) if !delta.trim().is_empty() => {
                    ServerEventAction::Emit(StreamEvent::Reasoning(delta.to_string()))
                }
                _ => ServerEventAction::Ignore,
            },
            "session.text.ended" => {
                if let (Some(message), Some(text)) = (
                    data.get("assistantMessageID").and_then(Value::as_str),
                    data.get("text").and_then(Value::as_str),
                ) {
                    self.texts.push((message.to_string(), text.to_string()));
                }
                // 文字不在串流中送出：與 V1 server 路徑相同，回覆在回合結束時一次取得，
                // 一邊送一邊再整段回傳會讓入口顯示兩次（見 docs/entrypoints.md）。
                ServerEventAction::Ignore
            }
            "session.tool.input.started" => {
                if let (Some(id), Some(name)) = (
                    data.get("id").and_then(Value::as_str),
                    data.get("name").and_then(Value::as_str),
                ) {
                    self.tool_names.insert(id.to_string(), name.to_string());
                }
                ServerEventAction::Ignore
            }
            "session.tool.called" => {
                let name = data
                    .get("id")
                    .and_then(Value::as_str)
                    .and_then(|id| self.tool_names.get(id))
                    .map(String::as_str)
                    .unwrap_or("tool");
                // 提問由 form.created 顯示，工具本身不再當成進度送出。
                if name == "question" {
                    return ServerEventAction::Ignore;
                }
                ServerEventAction::Emit(StreamEvent::ToolUse(format_tool_use(
                    name,
                    data.get("input"),
                )))
            }
            "form.created" => match data.get("form").and_then(parse_form_request) {
                Some(request) => ServerEventAction::Emit(StreamEvent::QuestionRequest(request)),
                None => ServerEventAction::Ignore,
            },
            "permission.asked" => match parse_permission_request(data) {
                Some(request) => ServerEventAction::Emit(StreamEvent::QuestionRequest(request)),
                None => ServerEventAction::Ignore,
            },
            _ => ServerEventAction::Ignore,
        }
    }
}

/// 工具名稱加上少數欄位的摘要。只列白名單欄位：其他工具的輸入可能很長或帶敏感內容。
fn format_tool_use(name: &str, input: Option<&Value>) -> String {
    let fields: &[&str] = match name {
        "shell" | "bash" => &["command"],
        "read" => &["path", "filePath"],
        "grep" => &["pattern", "path", "include"],
        "glob" => &["pattern", "path"],
        _ => return name.to_string(),
    };
    let mut lines = vec![name.to_string()];
    if let Some(input) = input {
        for field in fields {
            if let Some(value) = input.get(field).and_then(Value::as_str) {
                if !value.is_empty() {
                    lines.push(format!("  {field}: {}", truncate_tool_value(value)));
                }
            }
        }
    }
    lines.join("\n")
}

/// question 工具在 V2 變成 form：每個 field 是一題，`description` 是題目、`title` 是標題。
/// `request_id` 是 form id，回覆時由它找回 field 的 key。
fn parse_form_request(form: &Value) -> Option<QuestionRequest> {
    let request_id = form.get("id")?.as_str()?.to_string();
    let session_id = form.get("sessionID")?.as_str()?.to_string();
    let questions = form
        .get("fields")?
        .as_array()?
        .iter()
        .filter_map(parse_form_field)
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

fn parse_form_field(field: &Value) -> Option<QuestionInfo> {
    let text = |key: &str| {
        field
            .get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
    };
    let header = text("title").unwrap_or("").to_string();
    let question = text("description").or_else(|| text("title"))?.to_string();
    let options = field
        .get("options")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|option| {
                    let label = option
                        .get("label")
                        .or_else(|| option.get("value"))?
                        .as_str()?;
                    Some(QuestionOption {
                        label: label.to_string(),
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
        multiple: field.get("type").and_then(Value::as_str) == Some("multiselect"),
        custom: field.get("custom").and_then(Value::as_bool).unwrap_or(true),
    })
}

fn parse_permission_request(data: &Value) -> Option<QuestionRequest> {
    let permission_id = data.get("id")?.as_str()?;
    let session_id = data.get("sessionID")?.as_str()?.to_string();
    let action = data.get("action").and_then(Value::as_str).unwrap_or("tool");
    let resources = data
        .get("resources")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .take(5)
                .map(truncate_tool_value)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let mut question = format!("OpenCode 要求執行權限：{action}");
    if !resources.is_empty() {
        question.push_str("\n\n範圍：\n• ");
        question.push_str(&resources.join("\n• "));
    }
    Some(QuestionRequest {
        request_id: format!("{PERMISSION_REQUEST_PREFIX}{permission_id}"),
        session_id,
        questions: vec![QuestionInfo {
            question,
            header: "🔐 權限確認".to_string(),
            options: vec![
                QuestionOption {
                    label: PERMISSION_ALLOW_ONCE_LABEL.to_string(),
                    description: "只允許這次請求".to_string(),
                },
                QuestionOption {
                    label: PERMISSION_ALLOW_ALWAYS_LABEL.to_string(),
                    description: "在目前 OpenCode session 記住相同規則".to_string(),
                },
                QuestionOption {
                    label: PERMISSION_REJECT_LABEL.to_string(),
                    description: "拒絕執行".to_string(),
                },
            ],
            multiple: false,
            custom: false,
        }],
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::upstream_error::UpstreamFailure;
    use serde_json::json;

    /// 重播一份真實樣本，回傳送出的事件、結尾動作與最後的回覆文字。session 取樣本裡
    /// 第一個 `session.created`。
    fn replay(fixture: &str) -> (Vec<StreamEvent>, Option<ServerEventAction>, String) {
        let path = format!(
            "{}/tests/fixtures/opencode-v2/{fixture}.ndjson",
            env!("CARGO_MANIFEST_DIR")
        );
        let events: Vec<Value> = std::fs::read_to_string(&path)
            .unwrap()
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let session = events
            .iter()
            .find(|event| event["type"] == "session.created")
            .and_then(|event| event["data"]["sessionID"].as_str())
            .unwrap()
            .to_string();
        let mut mapper = ServerEventMapper::new(&session, RetryWatch::default());
        let mut emitted = Vec::new();
        for event in &events {
            match mapper.map(event) {
                ServerEventAction::Emit(event) => emitted.push(event),
                ServerEventAction::Ignore => {}
                end => return (emitted, Some(end), mapper.final_text()),
            }
        }
        (emitted, None, mapper.final_text())
    }

    // 期望值取自樣本內容本身：一次 shell 呼叫（`echo probe`），之後一步思考加上回覆 DONE。
    #[test]
    fn replays_a_tool_call_and_the_final_answer() {
        let (events, end, text) = replay("server-text-tool");
        assert_eq!(end, Some(ServerEventAction::Idle));
        assert_eq!(text, "DONE");
        let shape: Vec<String> = events
            .iter()
            .map(|event| match event {
                StreamEvent::StepStart => "step".to_string(),
                StreamEvent::StepFinish => "/step".to_string(),
                StreamEvent::Reasoning(_) => "think".to_string(),
                StreamEvent::ToolUse(tool) => format!("tool:{tool}"),
                other => format!("{other:?}"),
            })
            .collect::<Vec<_>>();
        let mut collapsed = shape.clone();
        collapsed.dedup();
        assert_eq!(
            collapsed,
            [
                "step",
                "tool:shell\n  command: echo probe",
                "/step",
                "step",
                "think",
                "/step"
            ],
            "{shape:?}"
        );
    }

    #[test]
    fn replays_a_permission_request() {
        let (events, end, _) = replay("server-permission");
        assert_eq!(end, Some(ServerEventAction::Idle));
        let requests: Vec<&QuestionRequest> = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::QuestionRequest(request) => Some(request),
                _ => None,
            })
            .collect();
        assert_eq!(requests.len(), 1);
        let request = requests[0];
        assert_eq!(
            request.request_id,
            format!("{PERMISSION_REQUEST_PREFIX}per_20c8cc493001tAuadPx45uP440")
        );
        assert_eq!(
            request.questions[0].question,
            "OpenCode 要求執行權限：shell\n\n範圍：\n• echo probe"
        );
        assert_eq!(
            request.questions[0]
                .options
                .iter()
                .map(|option| option.label.as_str())
                .collect::<Vec<_>>(),
            [
                PERMISSION_ALLOW_ONCE_LABEL,
                PERMISSION_ALLOW_ALWAYS_LABEL,
                PERMISSION_REJECT_LABEL
            ]
        );
    }

    #[test]
    fn replays_a_question_form() {
        let (events, end, _) = replay("server-form");
        assert_eq!(end, Some(ServerEventAction::Idle));
        let requests: Vec<&QuestionRequest> = events
            .iter()
            .filter_map(|event| match event {
                StreamEvent::QuestionRequest(request) => Some(request),
                _ => None,
            })
            .collect();
        assert_eq!(
            requests,
            [&QuestionRequest {
                request_id: "frm_20c8cdc8c001tihFwyDW9Qv4EM".to_string(),
                session_id: "ses_df3732be1ffeE82SQhxZQCmaDm".to_string(),
                questions: vec![QuestionInfo {
                    question: "Choose between A and B.".to_string(),
                    header: "Choice".to_string(),
                    options: vec![
                        QuestionOption {
                            label: "A".to_string(),
                            description: "Option A".to_string(),
                        },
                        QuestionOption {
                            label: "B".to_string(),
                            description: "Option B".to_string(),
                        },
                    ],
                    multiple: false,
                    custom: true,
                }],
            }]
        );
        assert!(!events.iter().any(
            |event| matches!(event, StreamEvent::ToolUse(tool) if tool.starts_with("question"))
        ));
    }

    #[test]
    fn an_interrupted_execution_is_not_success() {
        // 樣本裡中止之後還有 step.failed：結尾要以 execution 事件為準。
        let (_, end, _) = replay("server-interrupt");
        match end {
            Some(ServerEventAction::Failed(error)) => {
                assert_eq!(
                    crate::upstream_error::classify(&error.name, error.status_code),
                    UpstreamFailure::Aborted
                );
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn a_failed_execution_carries_the_v2_error() {
        let (_, end, _) = replay("server-no-route");
        match end {
            Some(ServerEventAction::Failed(error)) => {
                assert_eq!(error.name, "provider.no-route");
                assert_eq!(error.message, "Model unavailable: missing/nope");
            }
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    #[test]
    fn events_of_other_sessions_are_ignored() {
        let mut mapper = ServerEventMapper::new("ses_mine", RetryWatch::default());
        for event in [
            json!({"type":"session.execution.failed","data":{"sessionID":"ses_child","error":{"type":"provider.transport","message":"x"}}}),
            json!({"type":"session.execution.succeeded","data":{"sessionID":"ses_child"}}),
            json!({"type":"form.created","data":{"form":{"id":"frm_1","sessionID":"ses_child","fields":[{"key":"q0","title":"T","type":"string"}]}}}),
            json!({"type":"session.step.started","data":{"sessionID":"ses_child","assistantMessageID":"msg_1"}}),
        ] {
            assert_eq!(mapper.map(&event), ServerEventAction::Ignore, "{event}");
        }
        assert_eq!(
            mapper.map(
                &json!({"type":"session.execution.succeeded","data":{"sessionID":"ses_mine"}})
            ),
            ServerEventAction::Idle
        );
    }

    #[test]
    fn final_text_is_empty_when_the_last_step_said_nothing() {
        let mut mapper = ServerEventMapper::new("ses_1", RetryWatch::default());
        for event in [
            json!({"type":"session.step.started","data":{"sessionID":"ses_1","assistantMessageID":"msg_1"}}),
            json!({"type":"session.text.ended","data":{"sessionID":"ses_1","assistantMessageID":"msg_1","ordinal":0,"text":"Let me check."}}),
            json!({"type":"session.step.started","data":{"sessionID":"ses_1","assistantMessageID":"msg_2"}}),
        ] {
            mapper.map(&event);
        }
        assert_eq!(mapper.final_text(), "");

        mapper.map(&json!({"type":"session.text.ended","data":{"sessionID":"ses_1","assistantMessageID":"msg_2","ordinal":0,"text":"first"}}));
        mapper.map(&json!({"type":"session.text.ended","data":{"sessionID":"ses_1","assistantMessageID":"msg_2","ordinal":1,"text":"second"}}));
        assert_eq!(mapper.final_text(), "first\nsecond");
    }

    #[test]
    fn long_tool_values_are_truncated() {
        let long = "x".repeat(300);
        let shown = format_tool_use("shell", Some(&json!({ "command": long })));
        assert!(shown.starts_with("shell\n  command: "));
        assert!(shown.ends_with("..."));
        assert_eq!(
            format_tool_use("webfetch", Some(&json!({"url":"u"}))),
            "webfetch"
        );
    }
}
