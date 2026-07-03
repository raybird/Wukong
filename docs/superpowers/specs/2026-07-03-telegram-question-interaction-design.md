# Telegram Question Interaction Design

Date: 2026-07-03

## Summary

Wukong's OpenCode server backend currently treats OpenCode `question` tool activity as a normal `ToolUse` progress event. This shows `question` in Telegram, Web, and CLI, but it does not answer the underlying OpenCode question request. OpenCode waits for a `reply` or `reject`, so the session can remain blocked until Wukong's agent timeout fires.

This design adds first-class question interaction support for Telegram. Telegram will present OpenCode question requests with inline keyboard controls, collect answers question-by-question, and send the final answer payload back to OpenCode so the agent can continue.

## Goals

- Parse OpenCode `question.asked` events from the server event stream.
- Expose question requests as first-class Wukong stream events instead of generic tool-use text.
- Let Telegram users answer single-choice, multi-choice, and custom-text questions.
- Reply to OpenCode through its question API once all answers are collected.
- Reject questions when the user cancels or the Telegram interaction times out.
- Prevent custom-answer messages from being misrouted as new agent turns.

## Non-Goals

- Implement Web or CLI question UI in this change.
- Support multiple active question requests in the same Telegram chat.
- Add Telegram WebApp-based forms.
- Persist pending question state across process restarts.

## OpenCode Behavior

OpenCode's `question` tool publishes a `question.asked` event and then waits on an internal deferred value. It resumes only after a client calls one of these APIs:

- `POST /api/session/:sessionID/question/:requestID/reply`
- `POST /api/session/:sessionID/question/:requestID/reject`

The reply payload shape is:

```json
{
  "answers": [["First answer"], ["Option A", "Option B"]]
}
```

Each entry in `answers` corresponds to the question at the same index. Single-choice answers contain one string. Multi-choice answers can contain multiple strings. Custom text is represented as a string answer.

## Gateway Design

### Question Types

Add gateway data structures for OpenCode question requests:

- `QuestionRequest`
- `QuestionInfo`
- `QuestionOption`

The request contains:

- `request_id`
- `session_id`
- `questions`

Each question contains:

- `question`
- `header`
- `options`
- `multiple`
- `custom`

### Stream Event

Add a new stream event variant:

```rust
StreamEvent::QuestionRequest(QuestionRequest)
```

`map_server_event()` should parse OpenCode `question.asked` events and emit this variant when the event belongs to the active session.

The existing `tool == "question"` part update should no longer be emitted as `StreamEvent::ToolUse`. The question request event is the canonical interaction signal. This avoids misleading Telegram output like `使用工具 question` while the session is actually waiting for a reply.

### OpenCode Question API Methods

Add server backend methods:

- `reply_question(session_id, request_id, answers)`
- `reject_question(session_id, request_id)`

They should use the existing authenticated reqwest client and error handling style. Failures should return `GatewayError::AgentFailed` with the OpenCode response body when available.

## Telegram Client Design

Extend `wukong-tg-client` to support Telegram callback queries and inline keyboards.

### Parsing

Add a parsed update enum or equivalent structure that can represent:

- normal text/attachment messages
- callback query events

Callback query fields needed by dispatch:

- `update_id`
- `callback_query_id`
- `chat_id`
- `message_id`
- `data`

`highest_update_id()` should continue to advance across all update types.

### Bot API Methods

Add `TgClient` methods:

- `send_message_with_inline_keyboard(chat_id, text, keyboard)`
- `edit_message_text_with_inline_keyboard(chat_id, message_id, text, keyboard)`
- `answer_callback_query(callback_query_id, text)`

The inline keyboard payload should use Telegram Bot API `reply_markup.inline_keyboard`.

## Telegram Interaction Design

Telegram maintains one active pending question per chat.

### Pending State

The pending state contains:

- `chat_id`
- `session_id`
- `request_id`
- `questions`
- `current_question_index`
- `answers`
- `waiting_custom_question_index`
- `deadline`
- `message_id`

The deadline is 10 minutes after the pending question is created.

### Callback Data

Use compact callback data to stay within Telegram's callback data limit:

- `q:<request_id>:pick:<question_index>:<option_index>`
- `q:<request_id>:toggle:<question_index>:<option_index>`
- `q:<request_id>:custom:<question_index>`
- `q:<request_id>:next`
- `q:<request_id>:cancel`

The dispatch layer should validate that callback `request_id` matches the chat's active pending question before mutating state.

### Single Choice

For a single-choice question:

- Render one button per option.
- Render a custom-answer button when `custom != false`.
- Selecting an option records `[label]` for the current question.
- If more questions remain, edit the same Telegram message to show the next question.
- If this is the last question, call `reply_question()` and mark the Telegram message as answered.

### Multi Choice

For a multi-choice question:

- Render one toggle button per option.
- Use a visible prefix such as `[x]` and `[ ]` to show selection state.
- Render a custom-answer button when `custom != false`.
- Render `下一題` or `送出` depending on whether more questions remain.
- Toggling an option edits the same message with updated selection state.
- Pressing next/submit records the current answer set and advances or replies.

### Custom Text

When the user presses the custom-answer button:

- Set `waiting_custom_question_index` for the pending state.
- Edit the Telegram question message to ask the user to send the next text message as the answer.
- The next text message from that chat is consumed as the custom answer and does not start a new agent turn.
- Empty text or attachment-only messages do not complete the answer. Telegram should ask for a text answer.

For single-choice questions, the custom text becomes the only answer. For multi-choice questions, the custom text is added to the selected answers.

### Cancel

Every question message includes a cancel button.

When canceled:

- Call `reject_question(session_id, request_id)`.
- Clear pending state.
- Edit the Telegram message to show that the question was canceled.

### Timeout

Question interactions time out after 10 minutes.

The Telegram dispatch loop should clean expired pending questions before or after processing updates. On expiry:

- Call `reject_question(session_id, request_id)`.
- Clear pending state.
- Edit the Telegram message to show that the question expired and was canceled.

## Data Flow

1. The user sends a Telegram message.
2. Wukong starts a normal agent turn.
3. OpenCode calls the `question` tool.
4. OpenCode emits `question.asked` and waits for an answer.
5. Wukong gateway emits `StreamEvent::QuestionRequest`.
6. Telegram dispatch creates pending question state and sends an inline keyboard message.
7. The user answers each question with buttons or custom text.
8. Telegram dispatch calls `reply_question()` once all answers are complete.
9. OpenCode resumes the agent turn.
10. Wukong continues waiting for `session.idle`, then sends the final assistant response.

## Error Handling

- If `reply_question()` fails, keep pending state and edit/send a Telegram message saying the answer failed to send. The user can retry or cancel.
- If `reject_question()` fails, clear the Telegram pending state but log the gateway error. The user should not remain stuck in Telegram UI.
- If a callback refers to an unknown or stale pending question, answer the callback query with `這個問題已失效`.
- If the user sends a normal message while a button-only question is pending, reply with `請先回答目前問題，或按取消。` and do not start a new agent turn.
- If a second question arrives for the same chat while one is active, reject the new question and report that only one active Telegram question is supported.

## Testing Strategy

### Gateway

- Parses `question.asked` into `StreamEvent::QuestionRequest`.
- Ignores or suppresses `tool == "question"` as ordinary `ToolUse`.
- Sends the correct reply URL and payload.
- Sends the correct reject URL.
- Surfaces OpenCode API errors with response bodies.

### Telegram Client

- Parses callback query updates.
- Builds inline keyboard payloads for send/edit message.
- Calls `answerCallbackQuery` with the expected payload.
- Advances update offsets for callback-only updates.

### Telegram Dispatch

- Single-choice question completes and replies.
- Multi-choice question toggles options and replies with all selected labels.
- Custom text is consumed as an answer and does not start a new agent turn.
- Cancel calls reject and clears pending state.
- Ten-minute timeout calls reject and clears pending state.
- Stale callbacks produce a callback answer but do not mutate state.

## Open Questions

- None. The MVP scope is fixed to Telegram inline keyboard support with single-choice, multi-choice, custom text, cancel, and 10-minute timeout.
