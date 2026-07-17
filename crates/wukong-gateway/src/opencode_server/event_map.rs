use crate::stream::{
    QuestionInfo, QuestionOption, QuestionRequest, StreamEvent, PERMISSION_ALLOW_ALWAYS_LABEL,
    PERMISSION_ALLOW_ONCE_LABEL, PERMISSION_REJECT_LABEL, PERMISSION_REQUEST_PREFIX,
};
use serde_json::Value;

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

fn parse_permission_request(properties: &Value) -> Option<QuestionRequest> {
    let permission_id = properties
        .get("id")
        .or_else(|| properties.get("requestID"))?
        .as_str()?;
    let session_id = properties.get("sessionID")?.as_str()?.to_string();
    let permission = properties
        .get("permission")
        .and_then(Value::as_str)
        .unwrap_or("tool");
    let patterns = properties
        .get("patterns")
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
    let mut question = format!("OpenCode 要求執行權限：{permission}");
    if !patterns.is_empty() {
        question.push_str("\n\n範圍：\n• ");
        question.push_str(&patterns.join("\n• "));
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

#[derive(Debug, PartialEq)]
pub(super) enum ServerEventAction {
    Emit(StreamEvent),
    Idle,
    Ignore,
}

pub(super) fn map_server_event(
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
    if event_type == "permission.asked" {
        return match parse_permission_request(properties) {
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
    use serde_json::json;

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
    fn maps_permission_asked_to_interactive_question() {
        let value = json!({
            "payload": {
                "type": "permission.asked",
                "properties": {
                    "id": "per_1",
                    "sessionID": "ses_1",
                    "permission": "bash",
                    "patterns": ["python clean_report.py", "python clean_report.py *"]
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::QuestionRequest(QuestionRequest {
                request_id: "permission-per_1".to_string(),
                session_id: "ses_1".to_string(),
                questions: vec![QuestionInfo {
                    question: "OpenCode 要求執行權限：bash\n\n範圍：\n• python clean_report.py\n• python clean_report.py *".to_string(),
                    header: "🔐 權限確認".to_string(),
                    multiple: false,
                    custom: false,
                    options: vec![
                        QuestionOption {
                            label: "允許一次".to_string(),
                            description: "只允許這次請求".to_string(),
                        },
                        QuestionOption {
                            label: "本次工作階段總是允許".to_string(),
                            description: "在目前 OpenCode session 記住相同規則".to_string(),
                        },
                        QuestionOption {
                            label: "拒絕".to_string(),
                            description: "拒絕執行".to_string(),
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
}
