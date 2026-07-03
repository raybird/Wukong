# Telegram Question Interaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add first-class Telegram UI support for OpenCode `question` requests so agents resume after users answer inline-keyboard prompts.

**Architecture:** Gateway parses OpenCode `question.asked` into a typed stream event and exposes reply/reject API helpers. Telegram transport parses callback queries and sends inline keyboards. Telegram dispatch owns one in-memory pending question per chat, collects answers question-by-question, then calls OpenCode reply or reject.

**Tech Stack:** Rust 2021, Tokio, reqwest, serde/serde_json, Telegram Bot API inline keyboards, OpenCode server HTTP API.

**Execution Note:** Do not commit during implementation unless the user explicitly asks for commits. Use `git diff` checkpoints instead.

---

## File Structure

- Modify `crates/wukong-gateway/src/stream.rs`
  - Owns cross-backend stream event types.
  - Add `QuestionRequest`, `QuestionInfo`, `QuestionOption`, and `StreamEvent::QuestionRequest`.
- Modify `crates/wukong-gateway/src/opencode_server.rs`
  - Parse `question.asked` events.
  - Suppress `question` tool part updates as generic `ToolUse`.
  - Add reply/reject HTTP helpers on `OpencodeServerBackend`.
- Modify `crates/wukong-tg-client/src/parse.rs`
  - Add parsed update enum with message and callback query variants.
  - Keep existing `parse_updates()` compatibility or update callers to use `parse_update_events()`.
- Modify `crates/wukong-tg-client/src/client.rs`
  - Add inline keyboard data structures.
  - Add send/edit message methods with inline keyboard.
  - Add `answer_callback_query()`.
- Modify `crates/wukong-telegram/src/main.rs`
  - Process parsed update events instead of only messages.
  - Own pending question map across polling iterations.
  - Clean question timeouts.
- Modify `crates/wukong-telegram/src/dispatch.rs`
  - Add pending question state and rendering helpers.
  - Handle `StreamEvent::QuestionRequest` during agent turn.
  - Handle callback query events and custom text answers.

---

## Task 1: Gateway Question Types And Event Parsing

**Files:**
- Modify: `crates/wukong-gateway/src/stream.rs`
- Modify: `crates/wukong-gateway/src/opencode_server.rs`

- [ ] **Step 1: Run current gateway tests before editing**

Run:

```bash
cargo test -p wukong-gateway
```

Expected: current tests pass before changes. If they do not, record the failure and do not mix unrelated fixes into this task.

- [ ] **Step 2: Add failing tests for OpenCode question events**

In `crates/wukong-gateway/src/opencode_server.rs`, replace the existing test `maps_question_tool_use_with_prompt_and_options` with two tests:

```rust
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
```

Also add this import to the test module if needed:

```rust
use crate::stream::{QuestionInfo, QuestionOption, QuestionRequest};
```

- [ ] **Step 3: Run the failing tests**

Run:

```bash
cargo test -p wukong-gateway maps_question_asked_to_question_request ignores_question_tool_part_update_as_progress
```

Expected: fail because `QuestionRequest` types and event variant do not exist yet.

- [ ] **Step 4: Add question data types and stream variant**

In `crates/wukong-gateway/src/stream.rs`, update the top of the file to define these types before `StreamEvent`:

```rust
/// One option in an OpenCode question prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

/// One question in an OpenCode question request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionInfo {
    pub question: String,
    pub header: String,
    pub options: Vec<QuestionOption>,
    pub multiple: bool,
    pub custom: bool,
}

/// A pending OpenCode question request that must be answered or rejected.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuestionRequest {
    pub request_id: String,
    pub session_id: String,
    pub questions: Vec<QuestionInfo>,
}
```

Then add this variant to `StreamEvent`:

```rust
/// A pending question request from OpenCode's question tool.
QuestionRequest(QuestionRequest),
```

- [ ] **Step 5: Implement question event parsing**

In `crates/wukong-gateway/src/opencode_server.rs`, change the import:

```rust
use crate::stream::{QuestionInfo, QuestionOption, QuestionRequest, StreamEvent};
```

Add helper functions near `format_question_tool_use()` or replace `format_question_tool_use()` if it becomes unused:

```rust
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
    let custom = value
        .get("custom")
        .and_then(Value::as_bool)
        .unwrap_or(true);
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
```

In `map_server_event()`, after `properties` is computed and before the non-`message.part.updated` early return, add:

```rust
if event_type == "question.asked" {
    return match parse_question_request(properties) {
        Some(request) if request.session_id == session_id => {
            ServerEventAction::Emit(StreamEvent::QuestionRequest(request))
        }
        _ => ServerEventAction::Ignore,
    };
}
```

In the `"tool"` arm, after extracting `name`, suppress question tool part updates:

```rust
let name = part.get("tool").and_then(Value::as_str).unwrap_or("tool");
if name == "question" {
    return ServerEventAction::Ignore;
}
ServerEventAction::Emit(StreamEvent::ToolUse(format_tool_use_name(part, name)))
```

- [ ] **Step 6: Run gateway tests**

Run:

```bash
cargo test -p wukong-gateway
```

Expected: pass. If other crates now fail to compile because `StreamEvent` is non-exhaustive, fix match statements by adding a no-op `StreamEvent::QuestionRequest(_) => {}` outside Telegram until later tasks implement handling.

- [ ] **Step 7: Check diff instead of committing**

Run:

```bash
git diff -- crates/wukong-gateway/src/stream.rs crates/wukong-gateway/src/opencode_server.rs
```

Expected: only gateway question event changes are present.

---

## Task 2: Gateway Reply And Reject API Helpers

**Files:**
- Modify: `crates/wukong-gateway/src/opencode_server.rs`

- [ ] **Step 1: Add failing unit tests for reply and reject request construction**

In the existing `tests` module in `crates/wukong-gateway/src/opencode_server.rs`, add a lightweight request-builder helper test target by testing URL/payload helper functions first. Add production helper functions in this task named `question_reply_body()` and `question_reply_url()`.

Add tests:

```rust
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
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p wukong-gateway question_reply_body_serializes_answers question_api_urls_target_session_scoped_routes
```

Expected: fail because helpers do not exist.

- [ ] **Step 3: Implement helper types and methods**

In `crates/wukong-gateway/src/opencode_server.rs`, add this serializable body near `MessageBody`:

```rust
#[derive(Debug, Serialize)]
struct QuestionReplyBody {
    answers: Vec<Vec<String>>,
}
```

Add helper functions near `http_error()`:

```rust
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
```

Add methods inside `impl OpencodeServerBackend`:

```rust
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
    self.send_empty("question_reject", self.client.post(url)).await
}
```

- [ ] **Step 4: Run gateway tests**

Run:

```bash
cargo test -p wukong-gateway
```

Expected: pass.

- [ ] **Step 5: Check diff instead of committing**

Run:

```bash
git diff -- crates/wukong-gateway/src/opencode_server.rs
```

Expected: reply/reject helper changes only, plus tests.

---

## Task 3: Telegram Client Callback And Inline Keyboard Support

**Files:**
- Modify: `crates/wukong-tg-client/src/parse.rs`
- Modify: `crates/wukong-tg-client/src/client.rs`

- [ ] **Step 1: Add failing parse tests for callback queries**

In `crates/wukong-tg-client/src/parse.rs`, add these public types near `TgMessage`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgCallbackQuery {
    pub update_id: i64,
    pub callback_query_id: String,
    pub chat_id: i64,
    pub message_id: i64,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TgUpdateEvent {
    Message(TgMessage),
    CallbackQuery(TgCallbackQuery),
}
```

Then add this failing test:

```rust
#[test]
fn parse_update_events_extracts_callback_query() {
    let json = serde_json::json!({
        "result": [{
            "update_id": 42,
            "callback_query": {
                "id": "cb_1",
                "data": "q:que_1:pick:0:1",
                "message": {
                    "message_id": 99,
                    "chat": { "id": 7 }
                }
            }
        }]
    });

    assert_eq!(
        parse_update_events(&json),
        vec![TgUpdateEvent::CallbackQuery(TgCallbackQuery {
            update_id: 42,
            callback_query_id: "cb_1".to_string(),
            chat_id: 7,
            message_id: 99,
            data: "q:que_1:pick:0:1".to_string(),
        })]
    );
}
```

- [ ] **Step 2: Run failing parse test**

Run:

```bash
cargo test -p wukong-tg-client parse_update_events_extracts_callback_query
```

Expected: fail because `parse_update_events()` does not exist.

- [ ] **Step 3: Implement parsed update events**

Add this function to `crates/wukong-tg-client/src/parse.rs`:

```rust
pub fn parse_update_events(json: &serde_json::Value) -> Vec<TgUpdateEvent> {
    let Some(arr) = json.get("result").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|u| {
            parse_callback_query(u)
                .map(TgUpdateEvent::CallbackQuery)
                .or_else(|| parse_message_update(u).map(TgUpdateEvent::Message))
        })
        .collect()
}

fn parse_message_update(u: &serde_json::Value) -> Option<TgMessage> {
    let update_id = u.get("update_id")?.as_i64()?;
    let msg = u.get("message")?;
    let chat_id = msg.get("chat")?.get("id")?.as_i64()?;
    let attachments = parse_attachments(msg);
    let text = msg
        .get("text")
        .or_else(|| msg.get("caption"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| attachments.first().map(|a| fallback_prompt(&a.original_name)))?;
    Some(TgMessage {
        update_id,
        chat_id,
        text,
        attachments,
    })
}

fn parse_callback_query(u: &serde_json::Value) -> Option<TgCallbackQuery> {
    let update_id = u.get("update_id")?.as_i64()?;
    let callback = u.get("callback_query")?;
    let message = callback.get("message")?;
    Some(TgCallbackQuery {
        update_id,
        callback_query_id: callback.get("id")?.as_str()?.to_string(),
        chat_id: message.get("chat")?.get("id")?.as_i64()?,
        message_id: message.get("message_id")?.as_i64()?,
        data: callback.get("data")?.as_str()?.to_string(),
    })
}
```

Then simplify `parse_updates()` to preserve compatibility:

```rust
pub fn parse_updates(json: &serde_json::Value) -> Vec<TgMessage> {
    parse_update_events(json)
        .into_iter()
        .filter_map(|event| match event {
            TgUpdateEvent::Message(message) => Some(message),
            TgUpdateEvent::CallbackQuery(_) => None,
        })
        .collect()
}
```

- [ ] **Step 4: Add inline keyboard types and client method tests**

In `crates/wukong-tg-client/src/client.rs`, add data types near `TgFileInfo`:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

pub type InlineKeyboard = Vec<Vec<InlineKeyboardButton>>;
```

In the test `FakeClient` implementation inside `client.rs`, add captured fields for inline keyboard calls and callback answers. Then add tests that call the new methods and assert payload-equivalent behavior. If the fake stores high-level values, use this expected shape:

```rust
let keyboard = vec![vec![InlineKeyboardButton {
    text: "選項 A".to_string(),
    callback_data: "q:que_1:pick:0:0".to_string(),
}]];
```

- [ ] **Step 5: Implement Telegram Bot API methods**

Extend `TgClient` with:

```rust
fn send_message_with_inline_keyboard(
    &self,
    chat_id: i64,
    text: &str,
    keyboard: InlineKeyboard,
) -> impl std::future::Future<Output = Result<i64, TgError>> + Send;

fn edit_message_text_with_inline_keyboard(
    &self,
    chat_id: i64,
    message_id: i64,
    text: &str,
    keyboard: InlineKeyboard,
) -> impl std::future::Future<Output = Result<(), TgError>> + Send;

fn answer_callback_query(
    &self,
    callback_query_id: &str,
    text: &str,
) -> impl std::future::Future<Output = Result<(), TgError>> + Send;
```

Add helper:

```rust
fn inline_keyboard_markup(keyboard: InlineKeyboard) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": keyboard
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|button| serde_json::json!({
                        "text": button.text,
                        "callback_data": button.callback_data,
                    }))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    })
}
```

Implement reqwest methods with `sendMessage`, `editMessageText`, and `answerCallbackQuery`.

- [ ] **Step 6: Run Telegram client tests**

Run:

```bash
cargo test -p wukong-tg-client
```

Expected: pass.

- [ ] **Step 7: Check diff instead of committing**

Run:

```bash
git diff -- crates/wukong-tg-client/src/parse.rs crates/wukong-tg-client/src/client.rs
```

Expected: only callback query and inline keyboard support changes.

---

## Task 4: Telegram Pending Question State And Rendering Helpers

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs`

- [ ] **Step 1: Run current Telegram tests before editing**

Run:

```bash
cargo test -p wukong-telegram
```

Expected: pass before changes.

- [ ] **Step 2: Add tests for callback data and keyboard rendering**

In `crates/wukong-telegram/src/dispatch.rs` tests module, add tests for pure helpers first:

```rust
#[test]
fn question_callback_data_is_compact_and_parseable() {
    let data = question_callback_data("que_1", QuestionAction::Pick { question: 0, option: 2 });
    assert_eq!(data, "q:que_1:pick:0:2");
    assert_eq!(
        parse_question_callback(&data),
        Some(ParsedQuestionCallback {
            request_id: "que_1".to_string(),
            action: QuestionAction::Pick { question: 0, option: 2 },
        })
    );
}

#[test]
fn render_single_choice_question_has_option_custom_and_cancel_buttons() {
    let pending = sample_pending_question(false);
    let (text, keyboard) = render_pending_question(&pending);

    assert!(text.contains("第 1 / 1 題"));
    assert!(text.contains("選一個"));
    assert_eq!(keyboard.len(), 3);
    assert_eq!(keyboard[0][0].text, "A");
    assert_eq!(keyboard[1][0].text, "自訂回答");
    assert_eq!(keyboard[2][0].text, "取消");
}

#[test]
fn render_multi_choice_question_marks_selected_options() {
    let mut pending = sample_pending_question(true);
    pending.answers[0] = vec!["A".to_string()];
    let (_text, keyboard) = render_pending_question(&pending);

    assert_eq!(keyboard[0][0].text, "[x] A");
    assert_eq!(keyboard[0][1].text, "[ ] B");
    assert_eq!(keyboard[1][0].text, "送出");
}
```

Add sample helper in tests:

```rust
fn sample_pending_question(multiple: bool) -> PendingQuestion {
    PendingQuestion {
        chat_id: 7,
        session_id: "ses_1".to_string(),
        request_id: "que_1".to_string(),
        questions: vec![wukong_gateway::stream::QuestionInfo {
            question: if multiple { "選多個" } else { "選一個" }.to_string(),
            header: "偏好".to_string(),
            multiple,
            custom: true,
            options: vec![
                wukong_gateway::stream::QuestionOption {
                    label: "A".to_string(),
                    description: "".to_string(),
                },
                wukong_gateway::stream::QuestionOption {
                    label: "B".to_string(),
                    description: "".to_string(),
                },
            ],
        }],
        current_question_index: 0,
        answers: vec![Vec::new()],
        waiting_custom_question_index: None,
        deadline: std::time::Instant::now() + std::time::Duration::from_secs(600),
        message_id: Some(10),
    }
}
```

- [ ] **Step 3: Run failing helper tests**

Run:

```bash
cargo test -p wukong-telegram question_callback_data_is_compact_and_parseable render_single_choice_question_has_option_custom_and_cancel_buttons render_multi_choice_question_marks_selected_options
```

Expected: fail because helper types/functions do not exist.

- [ ] **Step 4: Implement pending state and helpers**

In `crates/wukong-telegram/src/dispatch.rs`, add imports:

```rust
use std::collections::HashMap;
use wukong_telegram::client::{InlineKeyboard, InlineKeyboardButton};
```

If paths cannot use `wukong_telegram` from inside the crate, import from `crate::client` instead:

```rust
use crate::client::{InlineKeyboard, InlineKeyboardButton};
```

Add constants and types near `Progress`:

```rust
const QUESTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionAction {
    Pick { question: usize, option: usize },
    Toggle { question: usize, option: usize },
    Custom { question: usize },
    Next,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuestionCallback {
    pub request_id: String,
    pub action: QuestionAction,
}

#[derive(Debug, Clone)]
pub struct PendingQuestion {
    pub chat_id: i64,
    pub session_id: String,
    pub request_id: String,
    pub questions: Vec<wukong_gateway::stream::QuestionInfo>,
    pub current_question_index: usize,
    pub answers: Vec<Vec<String>>,
    pub waiting_custom_question_index: Option<usize>,
    pub deadline: std::time::Instant,
    pub message_id: Option<i64>,
}

pub type PendingQuestions = HashMap<i64, PendingQuestion>;
```

Add callback helpers:

```rust
pub fn question_callback_data(request_id: &str, action: QuestionAction) -> String {
    match action {
        QuestionAction::Pick { question, option } => {
            format!("q:{request_id}:pick:{question}:{option}")
        }
        QuestionAction::Toggle { question, option } => {
            format!("q:{request_id}:toggle:{question}:{option}")
        }
        QuestionAction::Custom { question } => format!("q:{request_id}:custom:{question}"),
        QuestionAction::Next => format!("q:{request_id}:next"),
        QuestionAction::Cancel => format!("q:{request_id}:cancel"),
    }
}

pub fn parse_question_callback(data: &str) -> Option<ParsedQuestionCallback> {
    let mut parts = data.split(':');
    if parts.next()? != "q" {
        return None;
    }
    let request_id = parts.next()?.to_string();
    let action = match parts.next()? {
        "pick" => QuestionAction::Pick {
            question: parts.next()?.parse().ok()?,
            option: parts.next()?.parse().ok()?,
        },
        "toggle" => QuestionAction::Toggle {
            question: parts.next()?.parse().ok()?,
            option: parts.next()?.parse().ok()?,
        },
        "custom" => QuestionAction::Custom {
            question: parts.next()?.parse().ok()?,
        },
        "next" => QuestionAction::Next,
        "cancel" => QuestionAction::Cancel,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(ParsedQuestionCallback { request_id, action })
}
```

Add rendering helper:

```rust
pub fn render_pending_question(pending: &PendingQuestion) -> (String, InlineKeyboard) {
    let question = &pending.questions[pending.current_question_index];
    let total = pending.questions.len();
    let current = pending.current_question_index + 1;
    let mut text = format!("❓ 第 {current} / {total} 題");
    if !question.header.trim().is_empty() {
        text.push_str("\n");
        text.push_str(&question.header);
    }
    text.push_str("\n\n");
    text.push_str(&question.question);

    let selected = pending
        .answers
        .get(pending.current_question_index)
        .cloned()
        .unwrap_or_default();
    let mut keyboard = Vec::new();

    if question.multiple {
        for (row, chunk) in question.options.chunks(2).enumerate() {
            let buttons = chunk
                .iter()
                .enumerate()
                .map(|(offset, option)| {
                    let option_index = row * 2 + offset;
                    let mark = if selected.contains(&option.label) { "[x]" } else { "[ ]" };
                    InlineKeyboardButton {
                        text: format!("{mark} {}", option.label),
                        callback_data: question_callback_data(
                            &pending.request_id,
                            QuestionAction::Toggle {
                                question: pending.current_question_index,
                                option: option_index,
                            },
                        ),
                    }
                })
                .collect::<Vec<_>>();
            keyboard.push(buttons);
        }
        keyboard.push(vec![InlineKeyboardButton {
            text: if pending.current_question_index + 1 == total {
                "送出".to_string()
            } else {
                "下一題".to_string()
            },
            callback_data: question_callback_data(&pending.request_id, QuestionAction::Next),
        }]);
    } else {
        for option in question.options.iter().enumerate() {
            keyboard.push(vec![InlineKeyboardButton {
                text: option.1.label.clone(),
                callback_data: question_callback_data(
                    &pending.request_id,
                    QuestionAction::Pick {
                        question: pending.current_question_index,
                        option: option.0,
                    },
                ),
            }]);
        }
    }

    if question.custom {
        keyboard.push(vec![InlineKeyboardButton {
            text: "自訂回答".to_string(),
            callback_data: question_callback_data(
                &pending.request_id,
                QuestionAction::Custom {
                    question: pending.current_question_index,
                },
            ),
        }]);
    }
    keyboard.push(vec![InlineKeyboardButton {
        text: "取消".to_string(),
        callback_data: question_callback_data(&pending.request_id, QuestionAction::Cancel),
    }]);

    (text, keyboard)
}
```

- [ ] **Step 5: Run helper tests**

Run:

```bash
cargo test -p wukong-telegram question_callback_data_is_compact_and_parseable render_single_choice_question_has_option_custom_and_cancel_buttons render_multi_choice_question_marks_selected_options
```

Expected: pass.

- [ ] **Step 6: Check diff instead of committing**

Run:

```bash
git diff -- crates/wukong-telegram/src/dispatch.rs
```

Expected: only pending state and pure rendering/callback helpers.

---

## Task 5: Telegram Dispatch Question Event Handling

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs`

- [ ] **Step 1: Add question request handling tests**

Add tests using the existing fake Telegram client/backend patterns in `dispatch.rs`. Create a fake backend that emits `StreamEvent::QuestionRequest` before text, and assert an inline keyboard message is sent. Use this event:

```rust
wukong_gateway::StreamEvent::QuestionRequest(wukong_gateway::stream::QuestionRequest {
    request_id: "que_1".to_string(),
    session_id: "ses_1".to_string(),
    questions: vec![wukong_gateway::stream::QuestionInfo {
        question: "選一個".to_string(),
        header: "偏好".to_string(),
        multiple: false,
        custom: true,
        options: vec![wukong_gateway::stream::QuestionOption {
            label: "A".to_string(),
            description: "".to_string(),
        }],
    }],
})
```

Expected assertions:

```rust
assert!(client.inline_messages.lock().unwrap()[0].1.contains("選一個"));
assert_eq!(pending.get(&7).unwrap().request_id, "que_1");
```

- [ ] **Step 2: Run failing test**

Run:

```bash
cargo test -p wukong-telegram question_request_sends_inline_keyboard_and_tracks_pending
```

Expected: fail because `handle_message()` does not accept pending state and ignores question events.

- [ ] **Step 3: Extend handle_message signature**

Change `handle_message()` signature from:

```rust
pub async fn handle_message<C: TgClient, B: AiBackend>(
    client: &C,
    memory: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    history: Option<&ChatHistoryStore>,
    allow: &[i64],
    msg: &TgMessage,
)
```

to:

```rust
pub async fn handle_message<C: TgClient, B: AiBackend>(
    client: &C,
    memory: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    history: Option<&ChatHistoryStore>,
    allow: &[i64],
    pending_questions: &mut PendingQuestions,
    msg: &TgMessage,
)
```

At the top of `handle_message()`, before normal command/turn handling, consume custom text or block new turns when a question is pending:

```rust
if let Some(pending) = pending_questions.get(&msg.chat_id) {
    if pending.waiting_custom_question_index.is_some() {
        // Task 6 implements custom completion. For now, leave this branch to compile.
    } else {
        let _ = client
            .send_message(msg.chat_id, "請先回答目前問題，或按取消。")
            .await;
        return;
    }
}
```

- [ ] **Step 4: Handle stream question event**

In the `run_turn_observed_with_attachments()` event callback, add an arm:

```rust
StreamEvent::QuestionRequest(request) => {
    let pending = PendingQuestion {
        chat_id: msg.chat_id,
        session_id: request.session_id.clone(),
        request_id: request.request_id.clone(),
        answers: vec![Vec::new(); request.questions.len()],
        questions: request.questions.clone(),
        current_question_index: 0,
        waiting_custom_question_index: None,
        deadline: std::time::Instant::now() + QUESTION_TIMEOUT,
        message_id: None,
    };
    let (text, keyboard) = render_pending_question(&pending);
    let sent = futures::executor::block_on(client.send_message_with_inline_keyboard(
        msg.chat_id,
        &text,
        keyboard,
    ));
    if let Ok(message_id) = sent {
        let mut pending = pending;
        pending.message_id = Some(message_id);
        pending_questions.insert(msg.chat_id, pending);
    }
}
```

If using `block_on` inside the callback is rejected by the compiler because of runtime nesting, replace this with a `Progress::QuestionRequest` channel event and send the Telegram inline keyboard from the surrounding async progress task. Keep the same final behavior: pending state must be inserted only after Telegram returns a message id.

- [ ] **Step 5: Update existing tests and callers**

Every existing `handle_message()` call in tests and `main.rs` must pass a mutable `PendingQuestions` map:

```rust
let mut pending_questions = PendingQuestions::new();
handle_message(
    &client,
    &memory,
    &base_cfg,
    &backend,
    history.as_ref(),
    &allow,
    &mut pending_questions,
    &msg,
)
.await;
```

- [ ] **Step 6: Run Telegram tests**

Run:

```bash
cargo test -p wukong-telegram
```

Expected: pass.

- [ ] **Step 7: Check diff instead of committing**

Run:

```bash
git diff -- crates/wukong-telegram/src/dispatch.rs
```

Expected: message handling now accepts pending questions and creates Telegram question UI on stream events.

---

## Task 6: Telegram Callback Actions And Reply/Reject Integration

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs`
- Modify: `crates/wukong-telegram/src/main.rs`

- [ ] **Step 1: Add tests for callback behavior**

Add tests in `dispatch.rs` for:

```rust
#[tokio::test]
async fn single_choice_callback_records_answer_and_replies() { /* creates pending, sends pick callback, asserts reply_question called with [["A"]] */ }

#[tokio::test]
async fn multi_choice_callback_toggles_and_submit_replies() { /* toggles A and B, next submits [["A", "B"]] */ }

#[tokio::test]
async fn cancel_callback_rejects_and_clears_pending() { /* cancel calls reject and removes map entry */ }

#[tokio::test]
async fn stale_callback_answers_callback_query_without_mutating_state() { /* stale request_id sends 這個問題已失效 */ }
```

Use a fake question responder abstraction instead of depending directly on `OpencodeServerBackend` in dispatch tests:

```rust
pub trait QuestionResponder {
    fn reply_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> impl std::future::Future<Output = Result<(), wukong_gateway::GatewayError>> + Send;

    fn reject_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> impl std::future::Future<Output = Result<(), wukong_gateway::GatewayError>> + Send;
}
```

- [ ] **Step 2: Run failing callback tests**

Run:

```bash
cargo test -p wukong-telegram single_choice_callback_records_answer_and_replies multi_choice_callback_toggles_and_submit_replies cancel_callback_rejects_and_clears_pending stale_callback_answers_callback_query_without_mutating_state
```

Expected: fail because callback handler and responder trait are not implemented.

- [ ] **Step 3: Implement responder trait**

Implement `QuestionResponder` for `OpencodeServerBackend` only when the backend is server-backed. If dispatch currently receives `B: AiBackend`, add a separate responder parameter to callback handling instead of agent turn handling:

```rust
pub async fn handle_callback_query<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    pending_questions: &mut PendingQuestions,
    callback: &crate::parse::TgCallbackQuery,
)
```

If `build_backend_from_env()` returns an enum that hides the server backend, add methods on `AgentBackend` to forward `reply_question()` and `reject_question()` when server backend is active and return `GatewayError::AgentFailed` for CLI backend.

- [ ] **Step 4: Implement callback handling**

Implement `handle_callback_query()` with this behavior:

```rust
pub async fn handle_callback_query<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    pending_questions: &mut PendingQuestions,
    callback: &crate::parse::TgCallbackQuery,
) {
    let Some(parsed) = parse_question_callback(&callback.data) else {
        let _ = client
            .answer_callback_query(&callback.callback_query_id, "無法處理這個操作")
            .await;
        return;
    };
    let Some(pending) = pending_questions.get_mut(&callback.chat_id) else {
        let _ = client
            .answer_callback_query(&callback.callback_query_id, "這個問題已失效")
            .await;
        return;
    };
    if pending.request_id != parsed.request_id {
        let _ = client
            .answer_callback_query(&callback.callback_query_id, "這個問題已失效")
            .await;
        return;
    }

    match parsed.action {
        QuestionAction::Pick { question, option } => {
            if question != pending.current_question_index {
                let _ = client.answer_callback_query(&callback.callback_query_id, "題目已更新").await;
                return;
            }
            if let Some(label) = pending.questions[question].options.get(option).map(|o| o.label.clone()) {
                pending.answers[question] = vec![label];
                advance_or_reply(client, responder, pending_questions, callback.chat_id).await;
            }
        }
        QuestionAction::Toggle { question, option } => {
            if question == pending.current_question_index {
                if let Some(label) = pending.questions[question].options.get(option).map(|o| o.label.clone()) {
                    toggle_answer(&mut pending.answers[question], &label);
                    edit_pending_question(client, pending).await;
                }
            }
        }
        QuestionAction::Custom { question } => {
            pending.waiting_custom_question_index = Some(question);
            if let Some(message_id) = pending.message_id {
                let _ = client
                    .edit_message_text(
                        pending.chat_id,
                        message_id,
                        "請直接傳下一則文字作為自訂回答。",
                    )
                    .await;
            }
        }
        QuestionAction::Next => {
            advance_or_reply(client, responder, pending_questions, callback.chat_id).await;
        }
        QuestionAction::Cancel => {
            reject_and_clear_question(client, responder, pending_questions, callback.chat_id, "已取消問題。").await;
        }
    }
    let _ = client.answer_callback_query(&callback.callback_query_id, "").await;
}
```

Implement the referenced helpers with exact behavior from the spec: edit current message after toggle, advance question when not last, reply and clear state when last, reject and clear on cancel.

- [ ] **Step 5: Wire main loop to callback events**

In `crates/wukong-telegram/src/main.rs`, change imports:

```rust
use wukong_telegram::dispatch::{handle_callback_query, handle_message, PendingQuestions};
use wukong_telegram::parse::{highest_update_id, parse_allowlist, parse_update_events, TgUpdateEvent};
```

Create the map before the polling loop:

```rust
let mut pending_questions = PendingQuestions::new();
```

Replace the message loop:

```rust
for event in parse_update_events(&json) {
    match event {
        TgUpdateEvent::Message(msg) => {
            handle_message(
                &client,
                &memory,
                &base_cfg,
                &backend,
                history.as_ref(),
                &allow,
                &mut pending_questions,
                &msg,
            )
            .await;
        }
        TgUpdateEvent::CallbackQuery(callback) => {
            handle_callback_query(&client, &backend, &mut pending_questions, &callback).await;
        }
    }
}
```

- [ ] **Step 6: Run Telegram tests**

Run:

```bash
cargo test -p wukong-telegram
```

Expected: pass.

- [ ] **Step 7: Check diff instead of committing**

Run:

```bash
git diff -- crates/wukong-telegram/src/dispatch.rs crates/wukong-telegram/src/main.rs
```

Expected: callback handling and main loop event routing only.

---

## Task 7: Custom Text Answers And Timeout Cleanup

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs`
- Modify: `crates/wukong-telegram/src/main.rs`

- [ ] **Step 1: Add tests for custom text and timeout**

Add tests:

```rust
#[tokio::test]
async fn custom_text_is_consumed_as_answer_without_starting_turn() { /* pending.waiting_custom_question_index=Some(0), message text becomes answer, backend not called for new turn */ }

#[tokio::test]
async fn attachment_only_custom_answer_is_rejected_with_prompt() { /* attachments non-empty and no typed text keeps pending */ }

#[tokio::test]
async fn expired_question_rejects_and_clears_pending() { /* deadline in past, cleanup calls reject */ }
```

- [ ] **Step 2: Run failing tests**

Run:

```bash
cargo test -p wukong-telegram custom_text_is_consumed_as_answer_without_starting_turn attachment_only_custom_answer_is_rejected_with_prompt expired_question_rejects_and_clears_pending
```

Expected: fail because custom text and timeout cleanup are not implemented.

- [ ] **Step 3: Implement custom text consumption**

At the top of `handle_message()`, replace the temporary `waiting_custom_question_index` branch from Task 5 with:

```rust
if pending_questions
    .get(&msg.chat_id)
    .and_then(|pending| pending.waiting_custom_question_index)
    .is_some()
{
    if msg.text.trim().is_empty() || !msg.attachments.is_empty() {
        let _ = client
            .send_message(msg.chat_id, "請傳文字答案，不要傳附件。")
            .await;
        return;
    }
    complete_custom_answer(client, responder, pending_questions, msg.chat_id, msg.text.trim().to_string()).await;
    return;
}
```

If `handle_message()` does not have access to a question responder yet, add it as a parameter. The responder is needed because custom text can complete the final question and call `reply_question()`.

Implement `complete_custom_answer()`:

```rust
async fn complete_custom_answer<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    pending_questions: &mut PendingQuestions,
    chat_id: i64,
    answer: String,
) {
    let Some(pending) = pending_questions.get_mut(&chat_id) else {
        return;
    };
    let Some(index) = pending.waiting_custom_question_index.take() else {
        return;
    };
    if pending.questions[index].multiple {
        if !pending.answers[index].contains(&answer) {
            pending.answers[index].push(answer);
        }
    } else {
        pending.answers[index] = vec![answer];
    }
    advance_or_reply(client, responder, pending_questions, chat_id).await;
}
```

- [ ] **Step 4: Implement timeout cleanup**

Add public cleanup function:

```rust
pub async fn cleanup_expired_questions<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    pending_questions: &mut PendingQuestions,
) {
    let now = std::time::Instant::now();
    let expired = pending_questions
        .iter()
        .filter_map(|(chat_id, pending)| (pending.deadline <= now).then_some(*chat_id))
        .collect::<Vec<_>>();

    for chat_id in expired {
        reject_and_clear_question(
            client,
            responder,
            pending_questions,
            chat_id,
            "問題已逾時，已取消。",
        )
        .await;
    }
}
```

In `main.rs`, call cleanup after a successful `get_updates()` and before parsing events:

```rust
cleanup_expired_questions(&client, &backend, &mut pending_questions).await;
```

- [ ] **Step 5: Run Telegram tests**

Run:

```bash
cargo test -p wukong-telegram
```

Expected: pass.

- [ ] **Step 6: Check diff instead of committing**

Run:

```bash
git diff -- crates/wukong-telegram/src/dispatch.rs crates/wukong-telegram/src/main.rs
```

Expected: custom text consumption and timeout cleanup only.

---

## Task 8: Integration Compile, Formatting, And Regression Checks

**Files:**
- Modify only if compiler or test failures identify small integration mismatches.

- [ ] **Step 1: Format code**

Run:

```bash
cargo fmt
```

Expected: completes without error.

- [ ] **Step 2: Run targeted crate tests**

Run:

```bash
cargo test -p wukong-gateway -p wukong-tg-client -p wukong-telegram
```

Expected: pass.

- [ ] **Step 3: Run full workspace tests**

Run:

```bash
cargo test --workspace
```

Expected: pass. If a crate unrelated to this feature fails due to pre-existing issues, record the failing crate/test and still verify the three targeted crates pass.

- [ ] **Step 4: Run GitNexus change detection before completion**

Run the GitNexus MCP tool:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Expected: changed symbols are limited to gateway question event/API support, Telegram client callback/keyboard support, Telegram dispatch pending question flow, and the plan/spec docs.

- [ ] **Step 5: Final diff review**

Run:

```bash
git diff --stat
git diff -- docs/superpowers/specs/2026-07-03-telegram-question-interaction-design.md docs/superpowers/plans/2026-07-03-telegram-question-interaction.md crates/wukong-gateway/src/stream.rs crates/wukong-gateway/src/opencode_server.rs crates/wukong-tg-client/src/parse.rs crates/wukong-tg-client/src/client.rs crates/wukong-telegram/src/dispatch.rs crates/wukong-telegram/src/main.rs
```

Expected: no unrelated files and no accidental formatting-only rewrites outside touched files.

---

## Self-Review Notes

- Spec coverage: Gateway parsing, reply/reject API, Telegram callback parsing, inline keyboard rendering, custom text, cancel, 10-minute timeout, and tests are covered by Tasks 1-8.
- Scope: This plan intentionally excludes Web/CLI UI and persistent pending state, matching the spec non-goals.
- Type consistency: The plan uses `QuestionRequest`, `QuestionInfo`, `QuestionOption`, `PendingQuestion`, `PendingQuestions`, `QuestionAction`, and `ParsedQuestionCallback` consistently across tasks.
- Red-flag scan: No task contains deferred-work markers or open-ended implementation instructions. Where implementation depends on existing async constraints, the plan gives a concrete fallback using a progress event instead of nested `block_on`.
