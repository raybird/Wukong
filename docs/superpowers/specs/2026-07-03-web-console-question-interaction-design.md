# Web Console Question Interaction Design

## Context

Wukong already supports OpenCode `question` interactions in Telegram. The Web Console still ignores `StreamEvent::QuestionRequest` in the direct `/chat` SSE path, so a Web-initiated turn can stall when OpenCode waits for a question answer. The Web Console chat loader also needs more reliable bottom scrolling after asynchronous rendering, while preserving scroll position when users read older history.

This design extends Web Console question handling using the same conceptual model as OpenCode TUI/Web and the recent Telegram implementation: the agent turn may wait for an answer, but the UI transport remains responsive and can send `reply` or `reject` through a separate API.

## Goals

- Web Console can display and answer OpenCode `question` requests during direct `/chat?q=` turns.
- Web Console can display the same question UI for Telegram scope live events.
- Question UI supports single choice, multiple choice, custom answer, next/submit, cancel, retry on failure.
- Scrolling uses a smart post-render strategy: auto-scroll when appropriate, preserve position when reading older messages.
- The API/event shape leaves room for future pending-question recovery after refresh, without implementing persistence now.

## Non-Goals

- Do not recover pending questions after page refresh or scope switch in this iteration.
- Do not persist question cards into chat history yet.
- Do not redesign the whole Web Console visual language.
- Do not support multiple active questions per scope at the same time.

## Backend Design

### Direct Chat SSE

`GET /chat?q=...` should stop ignoring `StreamEvent::QuestionRequest`. When received, it sends an SSE message:

```text
event: question
data: { ... }
```

The JSON payload should include:

- `request_id`
- `session_id`
- `questions`

The `questions` shape should follow the existing gateway `QuestionRequest` model: each question has `question`, `header`, `multiple`, `custom`, and `options` with labels/descriptions.

### Reply And Reject API

Add Web APIs:

- `POST /api/questions/:request_id/reply`
- `POST /api/questions/:request_id/reject`

`reply` accepts:

```json
{
  "session_id": "ses_...",
  "answers": [["A"], ["B", "C"]]
}
```

The handler calls the existing opencode server backend question API through `reply_question(session_id, request_id, answers)`.

`reject` accepts:

```json
{
  "session_id": "ses_..."
}
```

The handler calls `reject_question(session_id, request_id)`.

If the active backend cannot answer questions, return a clear error response instead of silently accepting the request.

### Telegram Live Stream

The Web live stream should be able to emit question events for Telegram scopes. When Telegram receives a `QuestionRequest`, it should record or forward a live event with `kind: "question"` and a JSON payload compatible with the direct `/chat` `question` event.

This iteration does not need durable pending-question lookup. If the browser is connected when the live event arrives, it can render the card. If the browser refreshes later, the question is not restored.

## Frontend Design

### Shared Question Card

`wukong-chat.js` should expose a shared renderer, for example `renderQuestionCard(request, source)`, used by both:

- direct `/chat` SSE `event: question`
- Telegram scope live stream `kind: question`

The card should appear inline at the bottom of the chat log and match existing bubble styling. It should not require a full UI redesign.

### Question Behavior

Single choice:

- Selecting an option stores the answer.
- If this is the last question, submit immediately.
- Otherwise, advance to the next question.

Multiple choice:

- Options toggle on/off.
- A footer button advances to the next question or submits on the last question.

Custom answer:

- A custom answer button reveals a textarea.
- For single choice, the custom text becomes the only answer.
- For multiple choice, the custom text can be included with selected options.

Cancel:

- Calls the reject API.
- On success, card becomes an inactive “已取消問題。” state.

Submit:

- Calls the reply API with `session_id`, `request_id`, and `answers`.
- On success, card becomes an inactive “已送出回答。” state.
- On failure, card stays active and shows an error so the user can retry.

Only one active question card per scope is supported. If a new question arrives for the same scope, replace or supersede the active card.

## Scroll And Loading Design

Use smart scrolling:

- Initial load, scope switch, direct message send, live answer, and live question should scroll to bottom after render/layout completes.
- Live updates should only auto-scroll if the user was already near the bottom before the update.
- Loading older messages must preserve visual position and must not scroll to bottom.
- Jumping to a date should not force bottom scrolling.

Replace one-off scroll calls with a centralized helper, for example `scrollToBottomAfterRender()`, that waits for rendering and initial layout to settle before scrolling. The helper can use animation frames and a short image/layout wait for attachments or dynamic content that changes message height after insertion.

## Error Handling

- Direct SSE question event parse failure: ignore the malformed event and keep the stream alive.
- Reply/reject network failure: leave the card active and show a retryable error.
- Backend question API failure: show the returned error message in the card.
- Stale question: show a clear inactive state such as “這個問題已失效。”
- Empty custom answer: keep focus on the textarea and show a small validation message.

## Testing Strategy

Follow TDD.

Backend tests:

- `/chat` direct SSE emits `event: question` for `StreamEvent::QuestionRequest`.
- Reply API passes answers to the question responder.
- Reject API calls the question responder.
- Unsupported question backend returns a clear failure.
- Telegram live event stream can emit a `kind: question` event.

Frontend tests using the existing Rust `CHAT_JS` checks:

- Chat component listens for direct `question` SSE events.
- Chat component handles live `kind: question` events.
- A shared question-card renderer exists.
- Reply and reject API paths are referenced.
- Scroll helper waits until after render/layout.
- `loadOlder()` preserves scroll position and does not call the bottom-scroll helper.

Manual verification:

- Trigger a direct Web Console question and answer it from the card; the agent continues to the final answer.
- Watch a Telegram scope while a Telegram question arrives; Web Console renders the same card.
- Initial load and send scroll to bottom after content appears.
- Loading older messages preserves position.
- If the user is reading older messages, live updates do not yank the view downward.

## Deferred Decisions

- Pending question recovery after refresh is intentionally deferred.
- Persisting question cards into chat history is intentionally deferred.
- Multiple simultaneous active questions per scope are intentionally deferred.
