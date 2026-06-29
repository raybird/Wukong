# Turn Events History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Persist full turn reasoning and tool activity, then show it as collapsible history in Web console for both Web and Telegram conversations while keeping Telegram's live status bubble ephemeral.

**Architecture:** Add a dedicated `turn_events` persistence model in `wukong-chat-history`, expose it through Web APIs, and accumulate stream events in both Web and Telegram turn flows before linking them to the final assistant/error message. Web UI lazy-loads events per assistant message, merges reasoning chunks into one block, and lists tool/status events as a timeline.

**Tech Stack:** Rust, Tokio, Axum SSE, SQLx SQLite, vanilla Web Components, Telegram bot client abstraction.

---

## File Map

- Modify `crates/wukong-chat-history/src/lib.rs`: add `event_count`, `TurnEvent`, `insert_event`, `list_events`, schema setup, row mapping, and tests.
- Modify `crates/wukong-web/src/lib.rs`: add `SseMsg::ToolUse`, route `/api/chat/messages/:id/events`, event accumulation/persistence, message `event_count` API behavior, and tests.
- Modify `crates/wukong-web/static/components/wukong-chat.js`: add lazy event expander and live tool event display.
- Modify `crates/wukong-telegram/src/dispatch.rs`: add tool-use progress updates, event accumulation, event persistence for Telegram scopes, and tests.
- Optional modify `crates/wukong-web/static/styles.css` only if existing styles do not cover the new event expander clearly.

## Task 1: Chat History Turn Events Model

**Files:**

- Modify: `crates/wukong-chat-history/src/lib.rs`

- [ ] **Step 1: Run impact analysis before editing symbols**

Run GitNexus impact checks before touching `ChatMessage`, `ChatHistoryStore::open`, `insert_step`, `latest_messages`, `messages_before`, and `messages_for_date`.

Expected risk: likely MEDIUM because Web and Telegram history consumers read message projections.

- [ ] **Step 2: Write failing tests for event round trip and event_count**

Add these tests inside `#[cfg(test)] mod tests` in `crates/wukong-chat-history/src/lib.rs`:

```rust
#[tokio::test]
async fn turn_events_round_trip_in_stream_order() {
    let store = store().await;
    let thread = store.default_thread("global").await.unwrap();
    let mid = store
        .insert_message(&thread, "assistant", "final", Some("<p>final</p>"), "complete", 100)
        .await
        .unwrap();

    store
        .insert_event(mid, 1, "tool_use", Some("read"), "使用工具 read", 101)
        .await
        .unwrap();
    store
        .insert_event(mid, 0, "reasoning", None, "先想一下", 100)
        .await
        .unwrap();

    let events = store.list_events(mid).await.unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].seq, 0);
    assert_eq!(events[0].kind, "reasoning");
    assert_eq!(events[0].label, None);
    assert_eq!(events[0].content, "先想一下");
    assert_eq!(events[1].seq, 1);
    assert_eq!(events[1].kind, "tool_use");
    assert_eq!(events[1].label.as_deref(), Some("read"));
}

#[tokio::test]
async fn latest_messages_include_event_count() {
    let store = store().await;
    let thread = store.default_thread("global").await.unwrap();
    let mid = store
        .insert_message(&thread, "assistant", "final", None, "complete", 100)
        .await
        .unwrap();
    store
        .insert_event(mid, 0, "reasoning", None, "想", 100)
        .await
        .unwrap();

    let messages = store.latest_messages(&thread, 10).await.unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0].event_count, 1);
}
```

- [ ] **Step 3: Run tests and verify failure**

Run: `cargo test -p wukong-chat-history turn_events_round_trip_in_stream_order latest_messages_include_event_count`

Expected: FAIL because `insert_event`, `list_events`, `TurnEvent`, and `event_count` do not exist.

- [ ] **Step 4: Add data types and schema**

In `crates/wukong-chat-history/src/lib.rs`, update `ChatMessage`, add `TurnEvent`, and extend `open`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    pub id: i64,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub content_html: Option<String>,
    pub status: String,
    pub created_at: i64,
    #[serde(default)]
    pub step_count: i64,
    #[serde(default)]
    pub event_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnEvent {
    pub id: i64,
    pub message_id: i64,
    pub seq: i64,
    pub kind: String,
    pub label: Option<String>,
    pub content: String,
    pub created_at: i64,
}
```

Add after `turn_steps` setup in `ChatHistoryStore::open`:

```rust
sqlx::query(
    "CREATE TABLE IF NOT EXISTS turn_events (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        message_id INTEGER NOT NULL,
        seq INTEGER NOT NULL,
        kind TEXT NOT NULL,
        label TEXT,
        content TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        FOREIGN KEY(message_id) REFERENCES chat_messages(id) ON DELETE CASCADE
    )",
)
.execute(&pool)
.await?;
sqlx::query(
    "CREATE INDEX IF NOT EXISTS turn_events_message_id_idx
     ON turn_events(message_id)",
)
.execute(&pool)
.await?;
```

- [ ] **Step 5: Add store methods and row mapper**

Add methods in `impl ChatHistoryStore`:

```rust
pub async fn insert_event(
    &self,
    message_id: i64,
    seq: i64,
    kind: &str,
    label: Option<&str>,
    content: &str,
    created_at: i64,
) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO turn_events (message_id, seq, kind, label, content, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)
         RETURNING id",
    )
    .bind(message_id)
    .bind(seq)
    .bind(kind)
    .bind(label)
    .bind(content)
    .bind(created_at)
    .fetch_one(&self.pool)
    .await?;
    Ok(row.get("id"))
}

pub async fn list_events(&self, message_id: i64) -> Result<Vec<TurnEvent>, sqlx::Error> {
    let rows = sqlx::query(
        "SELECT id, message_id, seq, kind, label, content, created_at
         FROM turn_events
         WHERE message_id = ?1
         ORDER BY seq ASC, id ASC",
    )
    .bind(message_id)
    .fetch_all(&self.pool)
    .await?;
    Ok(rows.into_iter().map(row_to_event).collect())
}
```

Add mapper:

```rust
fn row_to_event(row: sqlx::sqlite::SqliteRow) -> TurnEvent {
    TurnEvent {
        id: row.get("id"),
        message_id: row.get("message_id"),
        seq: row.get("seq"),
        kind: row.get("kind"),
        label: row.get("label"),
        content: row.get("content"),
        created_at: row.get("created_at"),
    }
}
```

- [ ] **Step 6: Include event_count in message queries**

Update all message SELECT projections that compute `step_count` to also compute:

```sql
(SELECT COUNT(*) FROM turn_events te WHERE te.message_id = chat_messages.id) AS event_count
```

Update `row_to_message`:

```rust
event_count: row.try_get("event_count").unwrap_or(0),
```

- [ ] **Step 7: Run tests and commit**

Run: `cargo test -p wukong-chat-history`

Expected: PASS.

Commit:

```bash
git add crates/wukong-chat-history/src/lib.rs
git commit -m "feat: persist turn event history"
```

## Task 2: Web API Persistence and Event Endpoint

**Files:**

- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run impact analysis before editing symbols**

Run GitNexus impact checks for `chat`, `SseMsg`, `get_chat_messages`, `get_chat_steps`, and `build_router`.

Expected risk: likely MEDIUM because `/chat` and message APIs are Web console entry points.

- [ ] **Step 2: Write failing Web tests**

Add tests in `crates/wukong-web/src/lib.rs` test module near existing chat tests:

```rust
#[tokio::test]
async fn chat_streams_tool_event() {
    let state = reasoning_state("想一下").await;
    let app = build_router(state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/chat?q=hi")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let body = body_string(resp).await;
    assert!(body.contains("event: reasoning"));
    assert!(body.contains("event: tool"));
}

#[tokio::test]
async fn chat_persists_turn_events_and_serves_them() {
    let state = reasoning_state("想一下").await;
    let app = build_router(state.clone());
    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/chat?q=hi")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert!(body_string(resp).await.contains("event: answer"));

    let store = ChatHistoryStore::open(&state.db_url).await.unwrap();
    let thread = store.default_thread("global").await.unwrap();
    let messages = store.latest_messages(&thread, 10).await.unwrap();
    let assistant = messages.iter().find(|m| m.role == "assistant").unwrap();
    assert!(assistant.event_count > 0);

    let resp = app
        .oneshot(
            Request::builder()
                .uri(format!("/api/chat/messages/{}/events", assistant.id))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("reasoning"));
}
```

Update or create the test backend to emit a tool event. If `ReasoningBackend` currently emits only reasoning, change its `run_streaming` test implementation to include:

```rust
on_event(wukong_gateway::StreamEvent::ToolUse("read".to_string()));
```

- [ ] **Step 3: Run tests and verify failure**

Run: `cargo test -p wukong-web chat_streams_tool_event chat_persists_turn_events_and_serves_them`

Expected: FAIL because `tool` SSE and `/events` endpoint do not exist.

- [ ] **Step 4: Add SSE tool event**

Update `SseMsg`:

```rust
enum SseMsg {
    Role(String),
    Reasoning(String),
    ToolUse(String),
    Step {
        role: String,
        skill: Option<String>,
        html: String,
    },
    Answer(String),
    Error(String),
    Done,
}
```

Update `into_event`:

```rust
SseMsg::ToolUse(name) => Event::default().event("tool").data(name),
```

- [ ] **Step 5: Accumulate and persist turn events in `/chat`**

Inside the thread-local `/chat` async block, add:

```rust
let mut events_buf: Vec<(i64, String, Option<String>, String, i64)> = Vec::new();
let mut event_seq: i64 = 0;
```

In the stream callback passed to `run_turn_traced`, replace the reasoning-only match with:

```rust
match ev {
    wukong_gateway::StreamEvent::Reasoning(t) => {
        if !t.trim().is_empty() {
            let now = now_unix();
            events_buf.push((event_seq, "reasoning".to_string(), None, t.clone(), now));
            event_seq += 1;
            let _ = ev_tx.send(SseMsg::Reasoning(t));
        }
    }
    wukong_gateway::StreamEvent::ToolUse(name) => {
        let now = now_unix();
        events_buf.push((
            event_seq,
            "tool_use".to_string(),
            Some(name.clone()),
            format!("使用工具 {name}"),
            now,
        ));
        event_seq += 1;
        let _ = ev_tx.send(SseMsg::ToolUse(name));
    }
    wukong_gateway::StreamEvent::StepStart => {
        let now = now_unix();
        events_buf.push((event_seq, "step_start".to_string(), None, "step_start".to_string(), now));
        event_seq += 1;
    }
    wukong_gateway::StreamEvent::StepFinish => {
        let now = now_unix();
        events_buf.push((event_seq, "step_finish".to_string(), None, "step_finish".to_string(), now));
        event_seq += 1;
    }
    wukong_gateway::StreamEvent::Text(_) => {}
}
```

After inserting the assistant message in both success and error branches, insert events:

```rust
for (seq, kind, label, content, created_at) in &events_buf {
    let _ = store
        .insert_event(message_id, *seq, kind, label.as_deref(), content, *created_at)
        .await;
}
```

- [ ] **Step 6: Add events API route**

Add handler near `get_chat_steps`:

```rust
async fn get_chat_events<B>(
    State(state): State<AppState<B>>,
    Path(message_id): Path<i64>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let store = match ChatHistoryStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.list_events(message_id).await {
        Ok(events) => Json(events).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

Add route in `build_router`:

```rust
.route("/api/chat/messages/:id/events", get(get_chat_events::<B>))
```

- [ ] **Step 7: Run tests and commit**

Run: `cargo test -p wukong-web chat_streams_tool_event chat_persists_turn_events_and_serves_them`

Expected: PASS.

Commit:

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat: expose web turn event history"
```

## Task 3: Web Console Event Expander

**Files:**

- Modify: `crates/wukong-web/static/components/wukong-chat.js`
- Optional modify: `crates/wukong-web/static/styles.css`

- [ ] **Step 1: Review impact before editing UI behavior**

Use grep/read to verify existing `lazyStepsNode`, `messageNode`, `renderMessages`, and `send` behavior.

- [ ] **Step 2: Add lazy event expander**

In `wukong-chat.js`, add this method after `lazyStepsNode`:

```js
  lazyEventsNode(message) {
    const details = document.createElement('details');
    details.className = 'turn-events-group';
    details.innerHTML =
      '<summary>💭 思考與工具紀錄</summary><div class="turn-events-body"></div>';
    let loaded = false;
    details.addEventListener('toggle', async () => {
      if (!details.open || loaded) return;
      loaded = true;
      const body = details.querySelector('.turn-events-body');
      body.innerHTML = '<p class="baton-loading">載入中…</p>';
      try {
        const resp = await fetch(
          this.chatUrl('/api/chat/messages/' + encodeURIComponent(message.id) + '/events')
        );
        if (!resp.ok) throw new Error('HTTP ' + resp.status);
        const events = await resp.json();
        const reasoning = events
          .filter((event) => event.kind === 'reasoning')
          .map((event) => event.content)
          .join('');
        const tools = events.filter((event) => event.kind !== 'reasoning');

        body.innerHTML = '';
        if (reasoning.trim()) {
          const block = document.createElement('details');
          block.className = 'thinking';
          block.open = true;
          block.innerHTML =
            '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
          block.querySelector('.reasoning').textContent = reasoning;
          body.appendChild(block);
        }
        if (tools.length) {
          const list = document.createElement('ol');
          list.className = 'turn-events-timeline';
          for (const event of tools) {
            const item = document.createElement('li');
            if (event.kind === 'tool_use') {
              item.textContent = '使用工具 ' + (event.label || event.content || 'tool');
            } else {
              item.textContent = event.content || event.kind;
            }
            list.appendChild(item);
          }
          body.appendChild(list);
        }
        if (!reasoning.trim() && !tools.length) {
          body.innerHTML = '<p class="baton-loading">沒有紀錄。</p>';
        }
      } catch (err) {
        body.innerHTML = '<p class="baton-loading">載入失敗：' + escapeHTML(err.message) + '</p>';
        loaded = false;
      }
    });
    return details;
  }
```

- [ ] **Step 3: Render event expander above assistant answer**

In `renderMessages`, before `lazyStepsNode`, add:

```js
        if (message.event_count > 0) nodes.push(this.lazyEventsNode(message));
```

Keep existing `step_count` rendering after this line so stream history and helper-baton output remain separate.

- [ ] **Step 4: Display live tool events**

In `send`, add a `tool` event listener after `reasoning`:

```js
    es.addEventListener('tool', (ev) => {
      progress.innerHTML = '🐵 使用工具 ' + escapeHTML(ev.data) + '…';
      if (!thinking) {
        thinking = document.createElement('details');
        thinking.className = 'thinking';
        thinking.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
        this.log.appendChild(thinking);
      }
      thinking.querySelector('.reasoning').textContent += '\n▸ 使用工具 ' + ev.data + '\n';
      this.log.scrollTop = this.log.scrollHeight;
    });
```

- [ ] **Step 5: Add minimal styles if needed**

If `turn-events-group` looks unstyled, add to `crates/wukong-web/static/styles.css`:

```css
.turn-events-group {
  margin: 0.5rem 0;
  opacity: 0.95;
}

.turn-events-timeline {
  margin: 0.5rem 0 0;
  padding-left: 1.5rem;
}
```

- [ ] **Step 6: Run Web tests and commit**

Run: `cargo test -p wukong-web chat_messages_returns_latest_ten chat_persists_turn_events_and_serves_them`

Expected: PASS.

Commit:

```bash
git add crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/static/styles.css
git commit -m "feat: show turn event history in web console"
```

## Task 4: Telegram Tool Status and Event Persistence

**Files:**

- Modify: `crates/wukong-telegram/src/dispatch.rs`

- [ ] **Step 1: Run impact analysis before editing symbols**

Run GitNexus impact checks for `handle_message`, `record_chat`, `bubble_text`, and `Progress`.

Expected risk: MEDIUM because Telegram message handling has many tests and production entry points.

- [ ] **Step 2: Write failing Telegram tests**

Add or adjust tests in `crates/wukong-telegram/src/dispatch.rs`:

```rust
#[tokio::test]
async fn tool_use_appears_in_status_bubble() {
    struct ToolBackend;
    impl AiBackend for ToolBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse { text: "done".to_string(), session_id: None })
        }
        async fn run_streaming(
            &self,
            _req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::ToolUse("read".to_string()));
            Ok(AgentResponse { text: "done".to_string(), session_id: None })
        }
    }

    let client = RecordingTgClient::default();
    let mem = open_memory().await;
    let cfg = base_cfg();
    let msg = TgMessage { chat_id: 12, text: "hi".to_string(), message_id: 1 };
    handle_message(&client, &mem, &cfg, &ToolBackend, None, &[12], &msg).await;
    let edits = client.edits.lock().unwrap().clone();
    assert!(edits.iter().any(|(_, _, text)| text.contains("使用工具 read")), "tool edit missing: {edits:?}");
}

#[tokio::test]
async fn turn_records_telegram_events_in_chat_history() {
    let history = wukong_chat_history::ChatHistoryStore::open(&db_url()).await.unwrap();
    let client = RecordingTgClient::default();
    let mem = open_memory().await;
    let cfg = base_cfg();
    let msg = TgMessage { chat_id: 12, text: "hi".to_string(), message_id: 1 };

    handle_message(&client, &mem, &cfg, &ReasoningBackend, Some(&history), &[12], &msg).await;

    let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
    let messages = history.latest_messages(&thread, 10).await.unwrap();
    let assistant = messages.iter().find(|m| m.role == "assistant").unwrap();
    assert!(assistant.event_count > 0);
    let events = history.list_events(assistant.id).await.unwrap();
    assert!(events.iter().any(|event| event.kind == "reasoning"));
}
```

Use existing test helper names where they differ; keep the assertions identical.

- [ ] **Step 3: Run tests and verify failure**

Run: `cargo test -p wukong-telegram tool_use_appears_in_status_bubble turn_records_telegram_events_in_chat_history`

Expected: FAIL because tool progress and event persistence are not implemented.

- [ ] **Step 4: Extend progress model and bubble text**

Update `Progress`:

```rust
enum Progress {
    Role(Role),
    Reasoning(String),
    ToolUse(String),
}
```

In the progress task match, add:

```rust
Progress::ToolUse(name) => {
    reasoning.push_str("\n▸ 使用工具 ");
    reasoning.push_str(&name);
    reasoning.push('\n');
    let _ = c
        .edit_message_text(chat_id, mid, &bubble_text(role.as_deref(), &reasoning))
        .await;
}
```

- [ ] **Step 5: Accumulate events in Telegram turn flow**

Before `run_turn`, add:

```rust
let mut events_buf: Vec<(i64, String, Option<String>, String, i64)> = Vec::new();
let mut event_seq: i64 = 0;
```

Replace the stream callback with:

```rust
match ev {
    StreamEvent::Reasoning(t) => {
        if !t.trim().is_empty() {
            let now = now_unix();
            events_buf.push((event_seq, "reasoning".to_string(), None, t.clone(), now));
            event_seq += 1;
            let _ = tx_ev.send(Progress::Reasoning(t));
        }
    }
    StreamEvent::ToolUse(name) => {
        let now = now_unix();
        events_buf.push((
            event_seq,
            "tool_use".to_string(),
            Some(name.clone()),
            format!("使用工具 {name}"),
            now,
        ));
        event_seq += 1;
        let _ = tx_ev.send(Progress::ToolUse(name));
    }
    StreamEvent::StepStart => {
        let now = now_unix();
        events_buf.push((event_seq, "step_start".to_string(), None, "step_start".to_string(), now));
        event_seq += 1;
    }
    StreamEvent::StepFinish => {
        let now = now_unix();
        events_buf.push((event_seq, "step_finish".to_string(), None, "step_finish".to_string(), now));
        event_seq += 1;
    }
    StreamEvent::Text(_) => {}
}
```

- [ ] **Step 6: Persist Telegram events after assistant/error message insertion**

Either extend `record_chat` to return `Option<i64>` or add a new helper:

```rust
async fn record_chat_with_events(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    role: &str,
    content: &str,
    content_html: Option<&str>,
    status: &str,
    events: &[(i64, String, Option<String>, String, i64)],
) {
    let Some(history) = history else {
        return;
    };
    match history.default_thread(scope).await {
        Ok(thread) => {
            match history
                .insert_message(&thread, role, content, content_html, status, now_unix())
                .await
            {
                Ok(message_id) => {
                    for (seq, kind, label, content, created_at) in events {
                        let _ = history
                            .insert_event(message_id, *seq, kind, label.as_deref(), content, *created_at)
                            .await;
                    }
                }
                Err(e) => eprintln!("warning: telegram chat history insert failed: {e}"),
            }
        }
        Err(e) => eprintln!("warning: telegram chat history thread failed: {e}"),
    }
}
```

Use this helper only for assistant success/error messages from turns. Keep user and command history on the existing `record_chat` path.

- [ ] **Step 7: Run tests and commit**

Run: `cargo test -p wukong-telegram`

Expected: PASS.

Commit:

```bash
git add crates/wukong-telegram/src/dispatch.rs
git commit -m "feat: persist telegram turn event history"
```

## Task 5: Integration Verification and Scope Review

**Files:**

- Read/verify: `docs/superpowers/specs/2026-06-29-turn-events-history-design.md`
- Run tests across affected crates.

- [ ] **Step 1: Run focused affected tests**

Run:

```bash
cargo test -p wukong-chat-history
cargo test -p wukong-web chat_
```

Expected: PASS.

- [ ] **Step 2: Run formatting**

Run: `cargo fmt --all --check`

Expected: PASS. If it fails, run `cargo fmt --all`, inspect changes, then rerun `cargo fmt --all --check`.

- [ ] **Step 3: Run GitNexus change detection before final commit or handoff**

Run `gitnexus_detect_changes({ scope: "all", repo: "Wukong" })`.

Expected: changed symbols match this plan: chat history store, Web chat/API, Telegram dispatch, Web chat component, optional CSS.

- [ ] **Step 4: Review git diff**

Run:

```bash
git status --short
git diff -- crates/wukong-chat-history/src/lib.rs crates/wukong-web/src/lib.rs crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/static/styles.css crates/wukong-telegram/src/dispatch.rs docs/superpowers/specs/2026-06-29-turn-events-history-design.md docs/superpowers/plans/2026-06-29-turn-events-history.md
```

Expected: only planned files changed; no unrelated reversions.

- [ ] **Step 5: Final commit if requested**

Only commit if the user explicitly requests it. Commit message must not include AI attribution.

```bash
git add crates/wukong-chat-history/src/lib.rs crates/wukong-web/src/lib.rs crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/static/styles.css crates/wukong-telegram/src/dispatch.rs docs/superpowers/specs/2026-06-29-turn-events-history-design.md docs/superpowers/plans/2026-06-29-turn-events-history.md
git commit -m "feat: add turn event history"
```

## Self-Review

- Spec coverage: DB persistence, full reasoning, tool status, Web/Telegram scope display, Web merged reasoning, ordered tool timeline, and Telegram ephemeral status are covered by Tasks 1-4.
- Placeholder scan: no implementation step relies on undefined placeholders; test helper names may need to be adapted only where existing local helpers already use different names.
- Type consistency: `event_count`, `TurnEvent`, `insert_event`, `list_events`, `SseMsg::ToolUse`, and `/api/chat/messages/:id/events` are introduced before later tasks use them.
