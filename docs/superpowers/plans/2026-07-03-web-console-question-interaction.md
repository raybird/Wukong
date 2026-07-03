# Web Console Question Interaction Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Web Console support for OpenCode `question` requests in direct chat and Telegram live scopes, while making chat scrolling reliable after render without disrupting history browsing.

**Architecture:** Reuse the existing gateway `QuestionRequest` model and opencode server question APIs. Direct `/chat` SSE emits a `question` event, Telegram live sync records a compatible `question` live event, and `wukong-chat.js` renders a shared inline question card that replies or rejects through new Web APIs. Scrolling is centralized through post-render helpers that distinguish initial/live bottom-following from history prepends.

**Tech Stack:** Rust, Axum, tokio SSE, `wukong_gateway::backend::AgentBackend`, `wukong_gateway::stream::QuestionRequest`, vanilla custom elements, CSS.

---

## File Structure

- Modify `crates/wukong-web/src/lib.rs`
  - Add JSON DTOs for question requests/replies.
  - Add `SseMsg::Question` and serialization.
  - Add `POST /api/questions/:request_id/reply` and `POST /api/questions/:request_id/reject` routes.
  - Add backend helper functions that call `AgentBackend::Server.reply_question/reject_question` or return a clear unsupported error.
  - Add tests for direct SSE question events and reply/reject routes.
- Modify `crates/wukong-telegram/src/dispatch.rs`
  - Record a `question` live event when Telegram receives `StreamEvent::QuestionRequest`, so Web Console Telegram scopes can render the same card while connected.
  - Add or update tests verifying live question event recording.
- Modify `crates/wukong-web/static/components/wukong-chat.js`
  - Add direct SSE `question` listener.
  - Add live `kind: question` handling.
  - Add shared question card renderer and reply/reject methods.
  - Replace scattered bottom-scroll calls with smart render/layout-aware helpers.
- Modify `crates/wukong-web/static/styles.css`
  - Add compact styles for `.question-card`, options, footer, status, and errors using existing dark/gold/orange design tokens.
- No new production files are required.

---

### Task 1: Web SSE Question Event

**Files:**
- Modify: `crates/wukong-web/src/lib.rs:121-151`
- Modify: `crates/wukong-web/src/lib.rs:448-504`
- Test: `crates/wukong-web/src/lib.rs` tests module

- [ ] **Step 1: Run impact analysis before editing symbols**

Run:

```bash
# Use GitNexus before modifying existing functions/types.
# Target: chat, SseMsg, and affected test helpers in crates/wukong-web/src/lib.rs.
```

Expected: report risk and direct callers before changing `chat` or `SseMsg`.

- [ ] **Step 2: Write the failing test for direct SSE question events**

Add this test near existing `/chat` SSE tests in `crates/wukong-web/src/lib.rs`:

```rust
struct QuestionState;

impl AiBackend for QuestionState {
    async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        Ok(AgentResponse {
            text: "done".to_string(),
            session_id: Some("ses_1".to_string()),
        })
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        on_event(wukong_gateway::StreamEvent::QuestionRequest(
            wukong_gateway::stream::QuestionRequest {
                request_id: "que_1".to_string(),
                session_id: "ses_1".to_string(),
                questions: vec![wukong_gateway::stream::QuestionInfo {
                    question: "選一個".to_string(),
                    header: "偏好".to_string(),
                    options: vec![wukong_gateway::stream::QuestionOption {
                        label: "A".to_string(),
                        description: "第一個".to_string(),
                    }],
                    multiple: false,
                    custom: true,
                }],
            },
        ));
        self.run(req).await
    }
}

async fn question_state() -> AppState<QuestionState> {
    let (memory, db_url) = test_memory().await;
    AppState {
        memory: Arc::new(memory),
        backend: Arc::new(QuestionState),
        scope: "global".to_string(),
        db_url,
        token: None,
        settings_path: tempfile::tempdir().unwrap().path().join("settings.json"),
    }
}

#[tokio::test]
async fn chat_streams_question_event() {
    let app = build_router(question_state().await);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/chat?q=hi")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("event: question"), "missing question event:\n{body}");
    assert!(body.contains(r#""request_id":"que_1""#), "missing request id:\n{body}");
    assert!(body.contains(r#""session_id":"ses_1""#), "missing session id:\n{body}");
    assert!(body.contains("選一個"), "missing question text:\n{body}");
}
```

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
cargo test -p wukong-web chat_streams_question_event
```

Expected: FAIL because `/chat` currently ignores `StreamEvent::QuestionRequest` and does not emit `event: question`.

- [ ] **Step 4: Add serializable question DTOs and SSE variant**

In `crates/wukong-web/src/lib.rs`, add structs near `SseMsg`:

```rust
#[derive(serde::Serialize)]
struct WebQuestionOption {
    label: String,
    description: String,
}

#[derive(serde::Serialize)]
struct WebQuestionInfo {
    question: String,
    header: String,
    options: Vec<WebQuestionOption>,
    multiple: bool,
    custom: bool,
}

#[derive(serde::Serialize)]
struct WebQuestionRequest {
    request_id: String,
    session_id: String,
    questions: Vec<WebQuestionInfo>,
}

fn web_question_request(req: wukong_gateway::stream::QuestionRequest) -> WebQuestionRequest {
    WebQuestionRequest {
        request_id: req.request_id,
        session_id: req.session_id,
        questions: req
            .questions
            .into_iter()
            .map(|q| WebQuestionInfo {
                question: q.question,
                header: q.header,
                options: q
                    .options
                    .into_iter()
                    .map(|o| WebQuestionOption {
                        label: o.label,
                        description: o.description,
                    })
                    .collect(),
                multiple: q.multiple,
                custom: q.custom,
            })
            .collect(),
    }
}
```

Add to `enum SseMsg`:

```rust
Question(WebQuestionRequest),
```

Add to `SseMsg::into_event`:

```rust
SseMsg::Question(request) => Event::default()
    .event("question")
    .data(serde_json::to_string(&request).unwrap_or_else(|_| "{}".to_string())),
```

- [ ] **Step 5: Emit question SSE from `/chat`**

In the `run_turn_traced` callback inside `chat`, replace the ignore branch:

```rust
wukong_gateway::StreamEvent::QuestionRequest(request) => {
    let _ = ev_tx.send(SseMsg::Question(web_question_request(request)));
}
wukong_gateway::StreamEvent::Text(_) => {}
```

- [ ] **Step 6: Run test to verify it passes**

Run:

```bash
cargo test -p wukong-web chat_streams_question_event
```

Expected: PASS.

- [ ] **Step 7: Commit Task 1**

Run:

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat: stream web console question events"
```

Expected: one commit containing only the Web SSE question event work.

---

### Task 2: Web Question Reply And Reject APIs

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`
- Test: `crates/wukong-web/src/lib.rs` tests module

- [ ] **Step 1: Run impact analysis before editing route handlers**

Run GitNexus API impact before changing or adding route handlers in `crates/wukong-web/src/lib.rs`.

Expected: report consumers/risk for new route-adjacent work.

- [ ] **Step 2: Write failing tests for reply and reject APIs**

Add a backend test helper that can record question replies:

```rust
#[derive(Default)]
struct RecordingQuestionBackend {
    replies: std::sync::Mutex<Vec<(String, String, Vec<Vec<String>>)>>,
    rejects: std::sync::Mutex<Vec<(String, String)>>,
}

impl AiBackend for RecordingQuestionBackend {
    async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        Ok(AgentResponse { text: "ok".to_string(), session_id: None })
    }
}
```

Because `AiBackend` does not define question reply methods, add a local `WebQuestionResponder` trait in `wukong-web`. Production `AgentBackend` will implement it by calling opencode server question APIs. Tests will inject `RecordingQuestionBackend`, which implements both `AiBackend` and `WebQuestionResponder`.

```rust
#[allow(async_fn_in_trait)]
trait WebQuestionResponder {
    async fn reply_web_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError>;

    async fn reject_web_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError>;
}
```

Then tests can inject `RecordingQuestionBackend` by implementing both `AiBackend` and `WebQuestionResponder`.

Add this implementation for the test helper:

```rust
impl WebQuestionResponder for RecordingQuestionBackend {
    async fn reply_web_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        self.replies.lock().unwrap().push((
            session_id.to_string(),
            request_id.to_string(),
            answers,
        ));
        Ok(())
    }

    async fn reject_web_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError> {
        self.rejects
            .lock()
            .unwrap()
            .push((session_id.to_string(), request_id.to_string()));
        Ok(())
    }
}
```

Add tests:

```rust
#[tokio::test]
async fn question_reply_api_records_answers() {
    let backend = Arc::new(RecordingQuestionBackend::default());
    let app = build_router(AppState {
        memory: Arc::new(test_memory().await.0),
        backend: backend.clone(),
        scope: "global".to_string(),
        db_url: "sqlite::memory:".to_string(),
        token: None,
        settings_path: tempfile::tempdir().unwrap().path().join("settings.json"),
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/questions/que_1/reply")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"session_id":"ses_1","answers":[["A"]]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        backend.replies.lock().unwrap().as_slice(),
        &[("ses_1".to_string(), "que_1".to_string(), vec![vec!["A".to_string()]])]
    );
}

#[tokio::test]
async fn question_reject_api_records_reject() {
    let backend = Arc::new(RecordingQuestionBackend::default());
    let app = build_router(AppState {
        memory: Arc::new(test_memory().await.0),
        backend: backend.clone(),
        scope: "global".to_string(),
        db_url: "sqlite::memory:".to_string(),
        token: None,
        settings_path: tempfile::tempdir().unwrap().path().join("settings.json"),
    });
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/questions/que_1/reject")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"session_id":"ses_1"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert_eq!(
        backend.rejects.lock().unwrap().as_slice(),
        &[("ses_1".to_string(), "que_1".to_string())]
    );
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test -p wukong-web question_reply_api_records_answers question_reject_api_records_reject
```

Expected: FAIL because routes and responder trait do not exist.

- [ ] **Step 4: Add request DTOs**

Add near query structs:

```rust
#[derive(serde::Deserialize)]
struct QuestionReplyRequest {
    session_id: String,
    answers: Vec<Vec<String>>,
}

#[derive(serde::Deserialize)]
struct QuestionRejectRequest {
    session_id: String,
}
```

- [ ] **Step 5: Add responder trait and implementation**

Add in `crates/wukong-web/src/lib.rs`:

```rust
#[allow(async_fn_in_trait)]
trait WebQuestionResponder {
    async fn reply_web_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError>;

    async fn reject_web_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError>;
}

impl WebQuestionResponder for wukong_gateway::backend::AgentBackend {
    async fn reply_web_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        match self {
            wukong_gateway::backend::AgentBackend::Server(server) => {
                server.reply_question(session_id, request_id, answers).await
            }
            wukong_gateway::backend::AgentBackend::Cli(_) => Err(GatewayError::AgentFailed {
                code: None,
                stderr: "目前只有 opencode server backend 支援 question 回答。".to_string(),
            }),
        }
    }

    async fn reject_web_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError> {
        match self {
            wukong_gateway::backend::AgentBackend::Server(server) => {
                server.reject_question(session_id, request_id).await
            }
            wukong_gateway::backend::AgentBackend::Cli(_) => Err(GatewayError::AgentFailed {
                code: None,
                stderr: "目前只有 opencode server backend 支援 question 取消。".to_string(),
            }),
        }
    }
}
```

Update route-facing generic bounds from `B: AiBackend + Send + Sync + 'static` to `B: AiBackend + WebQuestionResponder + Send + Sync + 'static` for `build_router`, `chat`, route handlers, and other Web route functions that receive `AppState<B>`. Existing test backends must implement `WebQuestionResponder` with an unsupported response so unrelated tests keep compiling:

```rust
impl WebQuestionResponder for MockBackend {
    async fn reply_web_question(
        &self,
        _session_id: &str,
        _request_id: &str,
        _answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "question responder is not configured for this test backend".to_string(),
        })
    }

    async fn reject_web_question(
        &self,
        _session_id: &str,
        _request_id: &str,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "question responder is not configured for this test backend".to_string(),
        })
    }
}

impl WebQuestionResponder for ReasoningBackend {
    async fn reply_web_question(
        &self,
        _session_id: &str,
        _request_id: &str,
        _answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "question responder is not configured for this test backend".to_string(),
        })
    }

    async fn reject_web_question(
        &self,
        _session_id: &str,
        _request_id: &str,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "question responder is not configured for this test backend".to_string(),
        })
    }
}

impl WebQuestionResponder for ReasoningToolBackend {
    async fn reply_web_question(
        &self,
        _session_id: &str,
        _request_id: &str,
        _answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "question responder is not configured for this test backend".to_string(),
        })
    }

    async fn reject_web_question(
        &self,
        _session_id: &str,
        _request_id: &str,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "question responder is not configured for this test backend".to_string(),
        })
    }
}
```

- [ ] **Step 6: Add route handlers**

Add handlers:

```rust
async fn post_question_reply<B>(
    State(state): State<AppState<B>>,
    Path(request_id): Path<String>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<QuestionReplyRequest>,
) -> axum::response::Response
where
    B: AiBackend + WebQuestionResponder + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state
        .backend
        .reply_web_question(&req.session_id, &request_id, req.answers)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

async fn post_question_reject<B>(
    State(state): State<AppState<B>>,
    Path(request_id): Path<String>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<QuestionRejectRequest>,
) -> axum::response::Response
where
    B: AiBackend + WebQuestionResponder + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state
        .backend
        .reject_web_question(&req.session_id, &request_id)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}
```

- [ ] **Step 7: Register routes**

In `build_router`, add:

```rust
.route(
    "/api/questions/{request_id}/reply",
    axum::routing::post(post_question_reply::<B>),
)
.route(
    "/api/questions/{request_id}/reject",
    axum::routing::post(post_question_reject::<B>),
)
```

Use the existing Axum path syntax style already used in this repository.

- [ ] **Step 8: Run tests to verify pass**

Run:

```bash
cargo test -p wukong-web question_reply_api_records_answers question_reject_api_records_reject
```

Expected: PASS.

- [ ] **Step 9: Run broader Web tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 10: Commit Task 2**

Run:

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat: add web question reply api"
```

Expected: commit only Web API changes.

---

### Task 3: Telegram Live Question Events For Web Console

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs:1051-1071`
- Test: `crates/wukong-telegram/src/dispatch.rs` tests module

- [ ] **Step 1: Run impact analysis before editing dispatch**

Run GitNexus impact on `handle_message_with_responder` or, if GitNexus cannot resolve the generic function, on `crates/wukong-telegram/src/dispatch.rs` scoped symbols near question handling.

Expected: report blast radius before editing.

- [ ] **Step 2: Write failing test for live question event recording**

Add a test near `question_request_sends_inline_keyboard_and_tracks_pending`:

```rust
#[tokio::test]
async fn question_request_records_live_question_event_for_web_stream() {
    let client = MockTgClient::default();
    let (mem, db_url) = open_memory_with_url().await;
    let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
        .await
        .unwrap();
    let backend = QuestionBackend;
    let pending = Arc::new(Mutex::new(PendingQuestions::new()));
    let msg = TgMessage {
        update_id: 1,
        chat_id: 12,
        text: "hi".to_string(),
        attachments: Vec::new(),
    };

    handle_message_with_pending(
        &client,
        &mem,
        &base_cfg(),
        &backend,
        Some(&history),
        &[12],
        pending,
        &msg,
    )
    .await;

    let events = history
        .live_events_after(&scope_for_chat(12), 0, 20)
        .await
        .unwrap();
    let question = events
        .iter()
        .find(|event| event.kind == "question")
        .expect("missing question live event");
    assert!(question.content.contains(r#""request_id":"que_1""#));
    assert!(question.content.contains("選一個"));
}
```

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
cargo test -p wukong-telegram question_request_records_live_question_event_for_web_stream
```

Expected: FAIL because no `question` live event is recorded.

- [ ] **Step 4: Add JSON payload helper in Telegram dispatch**

Add near other helper functions in `crates/wukong-telegram/src/dispatch.rs`:

```rust
fn question_request_json(request: &QuestionRequest) -> String {
    serde_json::json!({
        "request_id": request.request_id,
        "session_id": request.session_id,
        "questions": request.questions.iter().map(|q| {
            serde_json::json!({
                "question": q.question,
                "header": q.header,
                "multiple": q.multiple,
                "custom": q.custom,
                "options": q.options.iter().map(|o| {
                    serde_json::json!({
                        "label": o.label,
                        "description": o.description,
                    })
                }).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>()
    })
    .to_string()
}
```

- [ ] **Step 5: Record live event when question arrives**

In the `StreamEvent::QuestionRequest(request)` branch, before or after `tx_ev.send`, add:

```rust
queue_live_event(
    &live_tx,
    "question",
    Some(&request.request_id),
    &question_request_json(&request),
    None,
);
```

- [ ] **Step 6: Run test to verify pass**

Run:

```bash
cargo test -p wukong-telegram question_request_records_live_question_event_for_web_stream
```

Expected: PASS.

- [ ] **Step 7: Run related package tests**

Run:

```bash
cargo test -p wukong-telegram -p wukong-web
```

Expected: PASS.

- [ ] **Step 8: Commit Task 3**

Run:

```bash
git add crates/wukong-telegram/src/dispatch.rs
git commit -m "feat: stream telegram questions to web console"
```

Expected: commit only Telegram live question event work.

---

### Task 4: Frontend Question Card

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
- Modify: `crates/wukong-web/static/styles.css`
- Test: `crates/wukong-web/src/lib.rs` `CHAT_JS`/`STYLES_CSS` tests

- [ ] **Step 1: Write failing string-coverage tests**

Add tests in `crates/wukong-web/src/lib.rs` near other `CHAT_JS` tests:

```rust
#[test]
fn chat_component_handles_question_events() {
    assert!(
        CHAT_JS.contains("addEventListener('question'")
            || CHAT_JS.contains("addEventListener(\"question\""),
        "direct chat SSE should listen for question events"
    );
    assert!(
        CHAT_JS.contains("data.kind === 'question'"),
        "live stream should handle question live events"
    );
    assert!(
        CHAT_JS.contains("renderQuestionCard"),
        "question UI should use a shared renderer"
    );
    assert!(
        CHAT_JS.contains("/api/questions/")
            && CHAT_JS.contains("/reply")
            && CHAT_JS.contains("/reject"),
        "question UI should call reply and reject APIs"
    );
}

#[test]
fn chat_styles_include_question_card() {
    assert!(STYLES_CSS.contains(".question-card"));
    assert!(STYLES_CSS.contains(".question-option"));
    assert!(STYLES_CSS.contains(".question-footer"));
}
```

- [ ] **Step 2: Run tests to verify fail**

Run:

```bash
cargo test -p wukong-web chat_component_handles_question_events chat_styles_include_question_card
```

Expected: FAIL because the frontend has no question UI.

- [ ] **Step 3: Add direct and live event listeners**

In `startLiveStream()`, add:

```js
stream.addEventListener('question', (ev) => this.handleLiveEvent(ev));
```

In `send()`, after creating `EventSource`, add:

```js
es.addEventListener('question', (ev) => {
  let request = null;
  try {
    request = JSON.parse(ev.data);
  } catch (_err) {
    return;
  }
  this.renderQuestionCard(request, 'direct');
  this.scrollToBottomAfterRender();
});
```

In `handleLiveEvent(ev)`, add before answer/error branches:

```js
} else if (data.kind === 'question') {
  let request = null;
  try {
    request = typeof data.content === 'string' ? JSON.parse(data.content) : data.content;
  } catch (_err) {
    return;
  }
  this.renderQuestionCard(request, 'live');
```

- [ ] **Step 4: Add active question state**

In `connectedCallback()`, initialize:

```js
this.activeQuestionCard = null;
```

In `resetMessages()` and `closeLiveStream()`, clear it:

```js
this.activeQuestionCard = null;
```

- [ ] **Step 5: Add question API helper**

Add methods before `send()`:

```js
async questionRequest(path, body) {
  const resp = await fetch(this.chatUrl(path), {
    method: 'POST',
    headers: { 'content-type': 'application/json' },
    body: JSON.stringify(body),
  });
  if (!resp.ok) {
    const text = await resp.text();
    throw new Error(text || 'HTTP ' + resp.status);
  }
}
```

- [ ] **Step 6: Add shared question card renderer**

Add method before `send()`:

```js
renderQuestionCard(request, source) {
  if (!request || !request.request_id || !request.session_id || !Array.isArray(request.questions)) return null;
  if (this.activeQuestionCard) this.activeQuestionCard.remove();

  const state = {
    tab: 0,
    answers: request.questions.map(() => []),
    custom: request.questions.map(() => ''),
    sending: false,
  };

  const card = document.createElement('section');
  card.className = 'question-card';
  card.dataset.requestId = request.request_id;
  this.activeQuestionCard = card;

  const setStatus = (text, cls = '') => {
    const status = card.querySelector('.question-status');
    if (!status) return;
    status.textContent = text;
    status.className = 'question-status ' + cls;
  };

  const finish = (text) => {
    card.classList.add('question-card-done');
    card.innerHTML = '<div class="question-done">' + escapeHTML(text) + '</div>';
    if (this.activeQuestionCard === card) this.activeQuestionCard = null;
  };

  const submit = async () => {
    if (state.sending) return;
    state.sending = true;
    setStatus('送出中…');
    try {
      await this.questionRequest('/api/questions/' + encodeURIComponent(request.request_id) + '/reply', {
        session_id: request.session_id,
        answers: state.answers,
      });
      finish('已送出回答。');
    } catch (err) {
      state.sending = false;
      setStatus('送出失敗：' + err.message, 'error');
    }
  };

  const reject = async () => {
    if (state.sending) return;
    state.sending = true;
    setStatus('取消中…');
    try {
      await this.questionRequest('/api/questions/' + encodeURIComponent(request.request_id) + '/reject', {
        session_id: request.session_id,
      });
      finish('已取消問題。');
    } catch (err) {
      state.sending = false;
      setStatus('取消失敗：' + err.message, 'error');
    }
  };

  const render = () => {
    const question = request.questions[state.tab];
    if (!question) return;
    const isLast = state.tab >= request.questions.length - 1;
    const selected = state.answers[state.tab] || [];
    card.innerHTML = '';

    const title = document.createElement('div');
    title.className = 'question-title';
    title.textContent = '❓ 第 ' + (state.tab + 1) + ' / ' + request.questions.length + ' 題';
    card.appendChild(title);

    if (question.header) {
      const header = document.createElement('div');
      header.className = 'question-header';
      header.textContent = question.header;
      card.appendChild(header);
    }

    const text = document.createElement('div');
    text.className = 'question-text';
    text.textContent = question.question || '';
    card.appendChild(text);

    const options = document.createElement('div');
    options.className = 'question-options';
    for (const option of question.options || []) {
      const button = document.createElement('button');
      button.type = 'button';
      button.className = 'question-option';
      const picked = selected.includes(option.label);
      if (picked) button.classList.add('picked');
      button.innerHTML = '<span>' + escapeHTML(picked ? '✓ ' : '') + escapeHTML(option.label || '') + '</span>' +
        (option.description ? '<small>' + escapeHTML(option.description) + '</small>' : '');
      button.addEventListener('click', () => {
        if (question.multiple) {
          state.answers[state.tab] = picked
            ? selected.filter((item) => item !== option.label)
            : [...selected, option.label];
          render();
          return;
        }
        state.answers[state.tab] = [option.label];
        if (isLast) void submit();
        else {
          state.tab += 1;
          render();
        }
      });
      options.appendChild(button);
    }
    card.appendChild(options);

    if (question.custom) {
      const custom = document.createElement('textarea');
      custom.className = 'question-custom';
      custom.rows = 2;
      custom.placeholder = '自訂回答…';
      custom.value = state.custom[state.tab] || '';
      custom.addEventListener('input', () => {
        state.custom[state.tab] = custom.value;
      });
      card.appendChild(custom);
    }

    const status = document.createElement('div');
    status.className = 'question-status';
    card.appendChild(status);

    const footer = document.createElement('div');
    footer.className = 'question-footer';

    const cancel = document.createElement('button');
    cancel.type = 'button';
    cancel.textContent = '取消';
    cancel.addEventListener('click', () => void reject());
    footer.appendChild(cancel);

    const next = document.createElement('button');
    next.type = 'button';
    next.textContent = isLast ? '送出' : '下一題';
    next.addEventListener('click', () => {
      const custom = (state.custom[state.tab] || '').trim();
      if (custom) {
        state.answers[state.tab] = question.multiple
          ? Array.from(new Set([...(state.answers[state.tab] || []), custom]))
          : [custom];
      }
      if (isLast) void submit();
      else {
        state.tab += 1;
        render();
      }
    });
    footer.appendChild(next);
    card.appendChild(footer);
  };

  render();
  this.log.appendChild(card);
  return card;
}
```

- [ ] **Step 7: Add CSS for question card**

Append to `crates/wukong-web/static/styles.css`:

```css
.question-card {
  align-self: flex-start;
  background: rgba(234, 179, 8, 0.06);
  border: 1px solid rgba(234, 179, 8, 0.22);
  border-radius: var(--border-radius);
  display: grid;
  gap: 0.65rem;
  max-width: min(36rem, 92%);
  padding: 0.9rem;
}
.question-title { color: var(--accent-gold); font-weight: 700; }
.question-header { color: var(--text-secondary); font-size: 0.9rem; }
.question-text { line-height: 1.5; }
.question-options { display: grid; gap: 0.45rem; }
.question-option {
  background: rgba(255, 255, 255, 0.06);
  border: 1px solid var(--border-color);
  border-radius: 0.75rem;
  color: var(--text-primary);
  cursor: pointer;
  display: grid;
  gap: 0.15rem;
  padding: 0.65rem 0.75rem;
  text-align: left;
}
.question-option.picked { border-color: var(--accent-gold); background: rgba(234, 179, 8, 0.12); }
.question-option small { color: var(--text-secondary); }
.question-custom {
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: 0.75rem;
  color: var(--text-primary);
  font: inherit;
  padding: 0.65rem;
  resize: vertical;
}
.question-footer { display: flex; gap: 0.5rem; justify-content: flex-end; }
.question-footer button {
  background: var(--bg-tertiary);
  border: 1px solid var(--border-color);
  border-radius: 999px;
  color: var(--text-primary);
  cursor: pointer;
  padding: 0.45rem 0.9rem;
}
.question-footer button:last-child { background: var(--accent-sun); border-color: transparent; }
.question-status { color: var(--text-secondary); font-size: 0.85rem; min-height: 1em; }
.question-status.error { color: #fca5a5; }
.question-done { color: var(--text-secondary); }
```

- [ ] **Step 8: Run frontend string tests**

Run:

```bash
cargo test -p wukong-web chat_component_handles_question_events chat_styles_include_question_card
```

Expected: PASS.

- [ ] **Step 9: Run Web package tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 10: Commit Task 4**

Run:

```bash
git add crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/static/styles.css crates/wukong-web/src/lib.rs
git commit -m "feat: render web console question cards"
```

Expected: commit frontend question card and its coverage tests.

---

### Task 5: Smart Post-Render Scrolling

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
- Test: `crates/wukong-web/src/lib.rs` `CHAT_JS` tests

- [ ] **Step 1: Write failing scroll helper tests**

Add tests near existing scroll test:

```rust
#[test]
fn chat_component_scrolls_after_render_and_layout() {
    assert!(
        CHAT_JS.contains("scrollToBottomAfterRender"),
        "chat should centralize post-render bottom scrolling"
    );
    assert!(
        CHAT_JS.contains("requestAnimationFrame") && CHAT_JS.contains("decode()"),
        "bottom scroll should wait for animation frames and image decode/layout"
    );
}

#[test]
fn chat_component_preserves_position_when_loading_older() {
    assert!(
        CHAT_JS.contains("preserveScrollPosition"),
        "loadOlder should use a named preserve-position helper"
    );
    assert!(
        CHAT_JS.contains("this.renderMessages(data.messages, 'prepend')"),
        "older messages should still render in prepend mode"
    );
}
```

- [ ] **Step 2: Run tests to verify fail**

Run:

```bash
cargo test -p wukong-web chat_component_scrolls_after_render_and_layout chat_component_preserves_position_when_loading_older
```

Expected: FAIL because helpers are not present.

- [ ] **Step 3: Add render/layout wait helper**

Replace existing `scrollToBottomAfterLayout()` with:

```js
nextFrame() {
  return new Promise((resolve) => requestAnimationFrame(() => resolve()));
}

async waitForLayoutContent() {
  await this.nextFrame();
  await this.nextFrame();
  const images = Array.from(this.log.querySelectorAll('img'))
    .filter((img) => !img.complete)
    .slice(0, 8);
  await Promise.allSettled(images.map((img) => img.decode ? img.decode() : Promise.resolve()));
}

async scrollToBottomAfterRender() {
  await this.waitForLayoutContent();
  this.scrollToBottom();
}
```

Keep `scrollToBottom()` unchanged.

- [ ] **Step 4: Add preserve helper**

Add:

```js
preserveScrollPosition(previousHeight) {
  this.log.scrollTop = this.log.scrollHeight - previousHeight;
}
```

In `renderMessages(messages, mode)`, change prepend branch:

```js
if (mode === 'prepend') {
  const previousHeight = this.log.scrollHeight;
  for (const node of nodes.reverse()) this.log.prepend(node);
  this.preserveScrollPosition(previousHeight);
} else {
  this.log.innerHTML = '';
  for (const node of nodes) this.log.appendChild(node);
  void this.scrollToBottomAfterRender();
}
```

- [ ] **Step 5: Replace direct bottom scrolls in send/live flows**

Replace instances of:

```js
this.log.scrollTop = this.log.scrollHeight;
```

for new bottom-following content with:

```js
void this.scrollToBottomAfterRender();
```

Do not add this to `loadOlder()` or prepend logic.

For `handleLiveEvent`, keep:

```js
const wasNearBottom = this.isNearBottom();
```

and update `maybeScrollToBottom`:

```js
maybeScrollToBottom(wasNearBottom) {
  if (wasNearBottom) void this.scrollToBottomAfterRender();
}
```

- [ ] **Step 6: Run scroll tests**

Run:

```bash
cargo test -p wukong-web chat_component_scrolls_after_render_and_layout chat_component_preserves_position_when_loading_older chat_component_scrolls_to_bottom_after_layout
```

Expected: PASS. If the old `chat_component_scrolls_to_bottom_after_layout` asserts the old helper string, update it to assert `scrollToBottomAfterRender` instead.

- [ ] **Step 7: Run Web package tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 8: Commit Task 5**

Run:

```bash
git add crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/src/lib.rs
git commit -m "fix: stabilize web chat scroll after render"
```

Expected: commit only scroll behavior and tests.

---

### Task 6: Final Verification

**Files:**
- No code changes unless verification exposes a bug.

- [ ] **Step 1: Run formatter check**

Run:

```bash
cargo fmt --check
```

Expected: PASS.

- [ ] **Step 2: Run related package tests**

Run:

```bash
cargo test -p wukong-web -p wukong-telegram -p wukong-gateway -p wukong-tg-client
```

Expected: PASS.

- [ ] **Step 3: Run full test suite**

Run:

```bash
cargo test
```

Expected: PASS.

- [ ] **Step 4: Run GitNexus change detection**

Run:

```bash
# gitnexus_detect_changes({scope: "all", repo: "Wukong"})
```

Expected: affected scope matches Web Console question handling, Telegram live question event recording, and chat scroll behavior.

- [ ] **Step 5: Inspect final diff and status**

Run:

```bash
git status --short --branch
git log --oneline -8
```

Expected: branch contains the task commits; only pre-existing unrelated dirty files remain.

- [ ] **Step 6: Manual verification checklist**

Run the app and verify:

```bash
cargo run -p wukong-web
```

Expected manual outcomes:

- Direct Web Console prompt that triggers OpenCode `question` shows an inline question card.
- Selecting an option sends reply and the assistant continues to final answer.
- Cancel sends reject and card becomes inactive.
- Telegram scope live stream renders a question card when a Telegram question arrives while Web Console is connected.
- Initial load and new direct messages scroll to bottom after content appears.
- Loading older messages preserves scroll position.
- Reading older messages is not yanked to bottom by live updates.

Do not commit manual verification output unless a bug fix is needed.
