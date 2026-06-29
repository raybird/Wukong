# Web Console Telegram Live Sync Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the web console default to the most recently active Telegram conversation, scroll like a messaging app, and live-sync Telegram turns including thinking/tool progress.

**Architecture:** Add a small scoped live-event log to `wukong-chat-history` so the separate Telegram and web binaries can communicate through the shared SQLite database. `wukong-telegram` appends live events while it handles Telegram messages, `wukong-web` exposes an SSE endpoint that tails those events for one scope, and `<wukong-chat>` renders them with the existing live turn UI while using history reloads as fallback.

**Tech Stack:** Rust workspace, Axum SSE, SQLx SQLite, Tokio, plain custom-element JavaScript, existing Rust test suites.

---

## File Structure

- Modify: `crates/wukong-chat-history/src/lib.rs`
  - Add `ChatLiveEvent` model.
  - Create `chat_live_events` table and index in `ChatHistoryStore::open`.
  - Add `insert_live_event`, `live_events_after`, and `prune_live_events_before` methods.
  - Add tests for event ordering, cursor behavior, scope filtering, and pruning.
- Modify: `crates/wukong-web/src/lib.rs`
  - Add stream query type with `scope` and optional `after` cursor.
  - Add `/api/chat/stream` SSE handler that polls `live_events_after` and emits matching events.
  - Add route and tests for token auth, scope filtering, and cursor behavior.
- Modify: `crates/wukong-telegram/src/dispatch.rs`
  - Append live events when Telegram messages are accepted, progress arrives, and final answer/error is produced.
  - Preserve existing chat history and Telegram Bot API behavior.
  - Add tests that inspect `chat_live_events` after Telegram handling.
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
  - Prefer the newest Telegram scope from the existing sorted `/api/chat/scopes` response.
  - Close/reopen `EventSource` on scope changes.
  - Render Telegram live events with current progress/thinking/tool/step/answer UI.
  - Keep initial/scope-switch scroll pinned to bottom and live-scroll only when already near bottom.

## Task 1: Add Chat Live Events To Chat History

**Files:**
- Modify: `crates/wukong-chat-history/src/lib.rs:1-377`
- Test: `crates/wukong-chat-history/src/lib.rs:437-658`

- [ ] **Step 1: Run the existing focused tests before editing**

Run: `cargo test -p wukong-chat-history`

Expected: PASS.

- [ ] **Step 2: Add the failing tests for live event storage**

Append these tests inside the existing `#[cfg(test)] mod tests` in `crates/wukong-chat-history/src/lib.rs`:

```rust
    #[tokio::test]
    async fn live_events_round_trip_by_scope_after_cursor() {
        let store = store().await;
        let first = store
            .insert_live_event("user:tg-12", "user", None, "hello", Some(10), 100)
            .await
            .unwrap();
        let second = store
            .insert_live_event("user:tg-12", "reasoning", None, "想一下", None, 101)
            .await
            .unwrap();
        store
            .insert_live_event("user:tg-99", "user", None, "other", Some(99), 102)
            .await
            .unwrap();

        let events = store.live_events_after("user:tg-12", first, 10).await.unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, second);
        assert_eq!(events[0].scope, "user:tg-12");
        assert_eq!(events[0].kind, "reasoning");
        assert_eq!(events[0].content, "想一下");
        assert_eq!(events[0].message_id, None);
    }

    #[tokio::test]
    async fn live_events_prune_by_created_at() {
        let store = store().await;
        store
            .insert_live_event("user:tg-12", "user", None, "old", None, 100)
            .await
            .unwrap();
        let kept = store
            .insert_live_event("user:tg-12", "answer", None, "new", Some(2), 200)
            .await
            .unwrap();

        let deleted = store.prune_live_events_before(150).await.unwrap();
        let events = store.live_events_after("user:tg-12", 0, 10).await.unwrap();

        assert_eq!(deleted, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, kept);
        assert_eq!(events[0].message_id, Some(2));
    }
```

- [ ] **Step 3: Run the tests to verify failure**

Run: `cargo test -p wukong-chat-history live_events -- --nocapture`

Expected: FAIL because `insert_live_event`, `live_events_after`, `prune_live_events_before`, and `ChatLiveEvent` do not exist.

- [ ] **Step 4: Add the live event model**

Make `ChatHistoryStore` cloneable because Telegram dispatch will need a lightweight cloned handle for a live-event writer task:

```rust
#[derive(Clone)]
pub struct ChatHistoryStore {
    pool: SqlitePool,
}
```

Then add this struct after `TurnEvent`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatLiveEvent {
    pub id: i64,
    pub scope: String,
    pub kind: String,
    pub label: Option<String>,
    pub content: String,
    pub message_id: Option<i64>,
    pub created_at: i64,
}
```

- [ ] **Step 5: Create the table and index in `ChatHistoryStore::open`**

Insert this after the `turn_events_message_id_idx` creation:

```rust
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_live_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scope TEXT NOT NULL,
                kind TEXT NOT NULL,
                label TEXT,
                content TEXT NOT NULL,
                message_id INTEGER,
                created_at INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS chat_live_events_scope_id_idx
             ON chat_live_events(scope, id)",
        )
        .execute(&pool)
        .await?;
```

- [ ] **Step 6: Add store methods**

Add these methods inside `impl ChatHistoryStore`, after `list_events` and before `latest_messages`:

```rust
    pub async fn insert_live_event(
        &self,
        scope: &str,
        kind: &str,
        label: Option<&str>,
        content: &str,
        message_id: Option<i64>,
        created_at: i64,
    ) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO chat_live_events (scope, kind, label, content, message_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
        )
        .bind(scope)
        .bind(kind)
        .bind(label)
        .bind(content)
        .bind(message_id)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("id"))
    }

    pub async fn live_events_after(
        &self,
        scope: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<ChatLiveEvent>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, scope, kind, label, content, message_id, created_at
             FROM chat_live_events
             WHERE scope = ?1 AND id > ?2
             ORDER BY id ASC
             LIMIT ?3",
        )
        .bind(scope)
        .bind(after)
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_live_event).collect())
    }

    pub async fn prune_live_events_before(&self, created_before: i64) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM chat_live_events WHERE created_at < ?1")
            .bind(created_before)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }
```

- [ ] **Step 7: Add the row mapper**

Add this helper after `row_to_event`:

```rust
fn row_to_live_event(row: sqlx::sqlite::SqliteRow) -> ChatLiveEvent {
    ChatLiveEvent {
        id: row.get("id"),
        scope: row.get("scope"),
        kind: row.get("kind"),
        label: row.get("label"),
        content: row.get("content"),
        message_id: row.get("message_id"),
        created_at: row.get("created_at"),
    }
}
```

- [ ] **Step 8: Run the focused tests**

Run: `cargo test -p wukong-chat-history live_events -- --nocapture`

Expected: PASS.

- [ ] **Step 9: Run the full crate tests**

Run: `cargo test -p wukong-chat-history`

Expected: PASS.

- [ ] **Step 10: Commit Task 1**

Run:

```bash
git add crates/wukong-chat-history/src/lib.rs
git commit -m "feat: add chat live event storage"
```

## Task 2: Add Web SSE Tail Endpoint

**Files:**
- Modify: `crates/wukong-web/src/lib.rs:121-167,536-658,1116-1201,1203-2369`
- Test: `crates/wukong-web/src/lib.rs:1203-2369`

- [ ] **Step 1: Run impact analysis before editing web route symbols**

Run GitNexus impact for `get_chat_messages`, `get_chat_scopes`, and `build_router` before modifying adjacent route code:

```text
gitnexus_impact({target: "get_chat_messages", direction: "upstream", repo: "Wukong"})
gitnexus_impact({target: "get_chat_scopes", direction: "upstream", repo: "Wukong"})
gitnexus_impact({target: "build_router", direction: "upstream", repo: "Wukong"})
```

Expected: report direct callers/affected processes. If risk is HIGH or CRITICAL, pause and warn the user before editing.

- [ ] **Step 2: Add failing tests for `/api/chat/stream` auth and filtering**

Add these tests near the existing chat API tests in `crates/wukong-web/src/lib.rs`:

```rust
    #[tokio::test]
    async fn chat_stream_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/stream?scope=user%3Atg-12")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn chat_stream_replays_events_after_cursor_for_scope() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let first = store
            .insert_live_event("user:tg-12", "user", None, "old", Some(1), 100)
            .await
            .unwrap();
        store
            .insert_live_event("user:tg-99", "user", None, "wrong", Some(2), 101)
            .await
            .unwrap();
        store
            .insert_live_event("user:tg-12", "tool", Some("read"), "read", None, 102)
            .await
            .unwrap();

        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/chat/stream?scope=user%3Atg-12&after={first}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("event: tool"), "body: {body}");
        assert!(body.contains("read"), "body: {body}");
        assert!(!body.contains("old"), "body should respect cursor: {body}");
        assert!(!body.contains("wrong"), "body should filter scope: {body}");
    }
```

- [ ] **Step 3: Run the tests to verify failure**

Run: `cargo test -p wukong-web chat_stream -- --nocapture`

Expected: FAIL because `/api/chat/stream` is not registered.

- [ ] **Step 4: Add stream query and event conversion**

Add this query type after `ChatMessagesQuery`:

```rust
#[derive(serde::Deserialize)]
struct ChatStreamQuery {
    token: Option<String>,
    scope: Option<String>,
    after: Option<i64>,
}
```

Add this helper after `impl SseMsg`:

```rust
fn live_event_to_sse(event: wukong_chat_history::ChatLiveEvent) -> Event {
    let mut payload = serde_json::json!({
        "id": event.id,
        "scope": event.scope,
        "kind": event.kind,
        "content": event.content,
        "message_id": event.message_id,
        "created_at": event.created_at,
    });
    if let Some(label) = event.label {
        payload["label"] = serde_json::Value::String(label);
    }
    let name = payload["kind"].as_str().unwrap_or("message").to_string();
    Event::default().event(name).data(payload.to_string())
}
```

- [ ] **Step 5: Add the SSE handler**

Add this handler after `get_chat_scopes`:

```rust
async fn stream_chat_events<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<ChatStreamQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let scope = match params.scope.map(|s| s.trim().to_string()).filter(|s| !s.is_empty()) {
        Some(scope) => scope,
        None => return (StatusCode::BAD_REQUEST, "missing scope").into_response(),
    };
    let db_url = state.db_url.clone();
    let mut cursor = params.after.unwrap_or(0).max(0);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

    tokio::spawn(async move {
        let store = match ChatHistoryStore::open(&db_url).await {
            Ok(store) => store,
            Err(e) => {
                let _ = tx.send(Event::default().event("error").data(e.to_string()));
                return;
            }
        };
        let mut idle_ticks = 0;
        loop {
            match store.live_events_after(&scope, cursor, 50).await {
                Ok(events) => {
                    if events.is_empty() {
                        idle_ticks += 1;
                    } else {
                        idle_ticks = 0;
                    }
                    for event in events {
                        cursor = event.id;
                        if tx.send(live_event_to_sse(event)).is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Event::default().event("error").data(e.to_string()));
                    return;
                }
            }

            if idle_ticks >= 2 && cfg!(test) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });

    let stream = UnboundedReceiverStream::new(rx).map(Ok::<Event, Infallible>);
    Sse::new(stream).into_response()
}
```

- [ ] **Step 6: Register the route**

Add this route immediately after `/api/chat/scopes` in `build_router`:

```rust
        .route(
            "/api/chat/stream",
            axum::routing::get(stream_chat_events::<B>),
        )
```

- [ ] **Step 7: Run focused web tests**

Run: `cargo test -p wukong-web chat_stream -- --nocapture`

Expected: PASS.

- [ ] **Step 8: Run relevant existing web chat tests**

Run: `cargo test -p wukong-web chat_ -- --nocapture`

Expected: PASS.

- [ ] **Step 9: Commit Task 2**

Run:

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat: stream chat live events to web console"
```

## Task 3: Publish Telegram Live Events

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs:43-361,469-675`
- Test: `crates/wukong-telegram/src/dispatch.rs:469-675`

- [ ] **Step 1: Run impact analysis before editing Telegram dispatch symbols**

Run GitNexus impact for `handle_message` and `record_chat_with_events`:

```text
gitnexus_impact({target: "handle_message", direction: "upstream", repo: "Wukong"})
gitnexus_impact({target: "record_chat_with_events", direction: "upstream", repo: "Wukong"})
```

Expected: report direct callers/affected processes. If risk is HIGH or CRITICAL, pause and warn the user before editing.

- [ ] **Step 2: Add failing Telegram live event tests**

Add this test after `turn_records_telegram_events_in_chat_history`:

```rust
    #[tokio::test]
    async fn turn_records_telegram_live_events_for_web_stream() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = ToolBackend;
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        let events = history
            .live_events_after(&scope_for_chat(12), 0, 20)
            .await
            .unwrap();
        assert!(events.iter().any(|e| e.kind == "user" && e.content == "hi"));
        assert!(events.iter().any(|e| e.kind == "role"));
        assert!(events.iter().any(|e| e.kind == "tool" && e.label.as_deref() == Some("read")));
        assert!(events.iter().any(|e| e.kind == "answer" && e.content == "<p>done</p>"));
    }
```

- [ ] **Step 3: Run the test to verify failure**

Run: `cargo test -p wukong-telegram telegram_live_events -- --nocapture`

Expected: FAIL because Telegram dispatch does not insert live events yet.

- [ ] **Step 4: Add a helper to append live events best-effort**

Add this type and helper after `record_chat_with_events`:

```rust
#[derive(Debug)]
struct LiveEventWrite {
    kind: String,
    label: Option<String>,
    content: String,
    message_id: Option<i64>,
    created_at: i64,
}

fn queue_live_event(
    tx: &Option<tokio::sync::mpsc::UnboundedSender<LiveEventWrite>>,
    kind: &str,
    label: Option<&str>,
    content: &str,
    message_id: Option<i64>,
) {
    let Some(tx) = tx else {
        return;
    };
    let _ = tx.send(LiveEventWrite {
        kind: kind.to_string(),
        label: label.map(str::to_string),
        content: content.to_string(),
        message_id,
        created_at: now_unix(),
    });
}
```

Also add this async helper for events that are emitted outside the live writer channel setup:

```rust
async fn record_live_event(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    kind: &str,
    label: Option<&str>,
    content: &str,
    message_id: Option<i64>,
) {
    let Some(history) = history else {
        return;
    };
    if let Err(e) = history
        .insert_live_event(scope, kind, label, content, message_id, now_unix())
        .await
    {
        eprintln!("warning: telegram live event insert failed: {e}");
    }
}
```

- [ ] **Step 5: Add a live writer channel inside turn handling**

In the `MessageAction::Turn(input)` branch, immediately after `let mut cfg = base_cfg.clone();` and the existing settings/config setup, create a channel-backed live writer after `cfg.scope` is known:

```rust
            let (live_tx, live_writer) = if let Some(history) = history {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<LiveEventWrite>();
                let history = history.clone();
                let scope = cfg.scope.clone();
                let writer = tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        let _ = history
                            .insert_live_event(
                                &scope,
                                &event.kind,
                                event.label.as_deref(),
                                &event.content,
                                event.message_id,
                                event.created_at,
                            )
                            .await;
                    }
                });
                (Some(tx), Some(writer))
            } else {
                (None, None)
            };
```

This writer makes role/reasoning/tool events visible to the web SSE tail while the Telegram turn is still running.

- [ ] **Step 6: Make `record_chat` return the inserted message id**

Change `record_chat` from returning `()` to returning `Option<i64>`:

```rust
async fn record_chat(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    role: &str,
    content: &str,
    content_html: Option<&str>,
    status: &str,
) -> Option<i64> {
    let Some(history) = history else {
        return None;
    };
    match history.default_thread(scope).await {
        Ok(thread) => match history
            .insert_message(&thread, role, content, content_html, status, now_unix())
            .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                eprintln!("warning: telegram chat history insert failed: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("warning: telegram chat history thread failed: {e}");
            None
        }
    }
}
```

- [ ] **Step 7: Publish live events in command handling**

In the `MessageAction::Command` branch, replace the user record call with:

```rust
            let user_message_id = record_chat(history, &cfg.scope, "user", &msg.text, None, "complete").await;
            record_live_event(history, &cfg.scope, "user", None, &msg.text, user_message_id).await;
```

After each assistant command reply is recorded, publish an `answer` event. For the successful command path, use:

```rust
                    let reply_message_id = record_chat(history, &cfg.scope, "assistant", &reply, None, "complete").await;
                    let reply_html = wukong_render::to_web_html(&reply);
                    record_live_event(history, &cfg.scope, "answer", None, &reply_html, reply_message_id).await;
```

For the unsupported command path, use the same two lines with that branch's `reply` variable.

- [ ] **Step 8: Publish live events in turn handling**

In the `MessageAction::Turn(input)` branch, replace the user record call with:

```rust
            let user_message_id = record_chat(history, &cfg.scope, "user", &input, None, "complete").await;
            queue_live_event(&live_tx, "user", None, &input, user_message_id);
```

In the `run_turn` `StreamEvent::Reasoning(t)` branch, after `let _ = tx_ev.send(Progress::Reasoning(t));`, add:

```rust
                            queue_live_event(&live_tx, "reasoning", None, events_buf.last().map(|e| e.3.as_str()).unwrap_or(""), None);
```

In the `run_turn` `StreamEvent::ToolUse(name)` branch, after `let _ = tx_ev.send(Progress::ToolUse(name));`, add:

```rust
                            queue_live_event(&live_tx, "tool", events_buf.last().and_then(|e| e.2.as_deref()), events_buf.last().map(|e| e.3.as_str()).unwrap_or(""), None);
```

Change the role callback to publish live `role` events immediately:

```rust
                &mut |r| {
                    let role_name = r.name().to_string();
                    queue_live_event(&live_tx, "role", None, &role_name, None);
                    let _ = tx.send(Progress::Role(r));
                },
```

After `run_turn` returns and after final answer/error live event is queued, close and await the writer:

```rust
            drop(live_tx);
            if let Some(writer) = live_writer {
                let _ = writer.await;
            }
```

- [ ] **Step 9: Publish final answer/error live events with message ids**

In the `Ok(out)` branch, replace `record_chat_with_events(...).await;` with a version that captures the inserted assistant id. If changing `record_chat_with_events` to return `Option<i64>`, use this signature:

```rust
async fn record_chat_with_events(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    role: &str,
    content: &str,
    content_html: Option<&str>,
    status: &str,
    events: &[(i64, String, Option<String>, String, i64)],
) -> Option<i64>
```

Return `Some(message_id)` on successful insert and `None` otherwise. Then in the `Ok(out)` branch:

```rust
                    let assistant_message_id = record_chat_with_events(
                        history,
                        &cfg.scope,
                        "assistant",
                        &out.text,
                        Some(&html),
                        "complete",
                        &events_buf,
                    )
                    .await;
                    queue_live_event(&live_tx, "answer", None, &html, assistant_message_id);
```

In the `Err(e)` branch, after recording the error assistant message, publish:

```rust
                    queue_live_event(&live_tx, "error", None, &err, assistant_message_id);
```

- [ ] **Step 10: Run focused Telegram tests**

Run: `cargo test -p wukong-telegram telegram_live_events -- --nocapture`

Expected: PASS.

- [ ] **Step 11: Run all Telegram tests**

Run: `cargo test -p wukong-telegram`

Expected: PASS.

- [ ] **Step 12: Commit Task 3**

Run:

```bash
git add crates/wukong-telegram/src/dispatch.rs
git commit -m "feat: publish telegram chat live events"
```

## Task 4: Wire Web Console Live Sync UI

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js:5-474`

- [ ] **Step 1: Run impact analysis before editing `WukongChat`**

Run GitNexus impact:

```text
gitnexus_impact({target: "WukongChat", direction: "upstream", repo: "Wukong"})
```

Expected: report direct callers/affected processes. If risk is HIGH or CRITICAL, pause and warn the user before editing.

- [ ] **Step 2: Add state fields in `connectedCallback`**

After `this.selectedScope = '';`, add:

```js
    this.liveStream = null;
    this.liveCursor = 0;
    this.renderedMessageIds = new Set();
    this.liveProgress = null;
    this.liveThinking = null;
```

- [ ] **Step 3: Track rendered message ids in `renderMessages`**

At the top of the `else` branch for `mode !== 'prepend'`, before clearing `innerHTML`, add:

```js
      this.renderedMessageIds.clear();
```

Inside the message loop, after `const bubbleNode = this.messageNode(message);`, add:

```js
      this.renderedMessageIds.add(String(message.id));
```

- [ ] **Step 4: Prefer the most recently active Telegram scope**

In `loadScopes()`, replace the current default selection block:

```js
      if (!this.selectedScope && this.scopes.length > 0) {
        const global = this.scopes.find((s) => s.scope === 'global');
        this.selectedScope = (global || this.scopes[0]).scope;
      }
```

with:

```js
      if (!this.selectedScope && this.scopes.length > 0) {
        const telegram = this.scopes.find((s) => s.scope.startsWith('user:tg-'));
        const global = this.scopes.find((s) => s.scope === 'global');
        this.selectedScope = (telegram || global || this.scopes[0]).scope;
      }
```

This works because `/api/chat/scopes` already returns scopes ordered by `updated_at DESC`.

- [ ] **Step 5: Add live stream lifecycle helpers**

Add these methods before `initialize()`:

```js
  isTelegramScope() {
    return this.selectedScope && this.selectedScope.startsWith('user:tg-');
  }

  closeLiveStream() {
    if (this.liveStream) {
      this.liveStream.close();
      this.liveStream = null;
    }
    this.liveProgress = null;
    this.liveThinking = null;
  }

  startLiveStream() {
    this.closeLiveStream();
    if (!this.isTelegramScope()) return;
    const stream = new EventSource(this.chatUrl('/api/chat/stream', { after: this.liveCursor }));
    this.liveStream = stream;
    stream.addEventListener('user', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('role', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('reasoning', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('tool', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('step', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('answer', (ev) => this.handleLiveEvent(ev));
    stream.addEventListener('error', (ev) => {
      if (ev.data) this.handleLiveEvent(ev);
    });
  }
```

- [ ] **Step 6: Start live stream after initial load and scope switches**

In `initialize()`, after `await this.loadLatest();`, add:

```js
    this.startLiveStream();
```

In the scope select `change` handler, replace the body with:

```js
      this.closeLiveStream();
      this.selectedScope = this.scopeSelect.value;
      this.liveCursor = 0;
      this.resetMessages();
      this.loadLatest().then(() => this.startLiveStream());
```

- [ ] **Step 7: Add scroll helpers**

Add these methods before `renderMessages`:

```js
  isNearBottom() {
    return this.log.scrollHeight - this.log.scrollTop - this.log.clientHeight < 120;
  }

  scrollToBottom() {
    this.log.scrollTop = this.log.scrollHeight;
  }

  maybeScrollToBottom(wasNearBottom) {
    if (wasNearBottom) this.scrollToBottom();
  }
```

Replace direct bottom scrolling after replace renders from:

```js
      this.log.scrollTop = this.log.scrollHeight;
```

to:

```js
      this.scrollToBottom();
```

- [ ] **Step 8: Add live event rendering**

Add these methods before `send()`:

```js
  appendBubble(cls, innerHTML) {
    const div = document.createElement('div');
    div.className = 'bubble ' + cls;
    div.innerHTML = innerHTML;
    this.log.appendChild(div);
    return div;
  }

  parseLiveEvent(ev) {
    try {
      const data = JSON.parse(ev.data);
      if (data.id) this.liveCursor = Math.max(this.liveCursor, Number(data.id));
      return data;
    } catch (_err) {
      return null;
    }
  }

  ensureLiveProgress() {
    if (!this.liveProgress) {
      this.liveProgress = this.appendBubble('status', '🐵 收到，思考中…');
    }
    return this.liveProgress;
  }

  ensureLiveThinking() {
    if (!this.liveThinking) {
      this.liveThinking = document.createElement('details');
      this.liveThinking.className = 'thinking';
      this.liveThinking.innerHTML = '<summary>💭 思考過程</summary><pre class="reasoning"></pre>';
      this.log.appendChild(this.liveThinking);
    }
    return this.liveThinking;
  }

  handleLiveEvent(ev) {
    const data = this.parseLiveEvent(ev);
    if (!data) return;
    if (data.message_id && this.renderedMessageIds.has(String(data.message_id))) return;
    const wasNearBottom = this.isNearBottom();

    if (data.kind === 'user') {
      const node = this.appendBubble('user', html`${data.content || ''}`.toString());
      if (data.message_id) {
        node.dataset.messageId = data.message_id;
        this.renderedMessageIds.add(String(data.message_id));
      }
    } else if (data.kind === 'role') {
      this.ensureLiveProgress().innerHTML = '🐵 悟空·' + escapeHTML(data.content || '') + ' 思考中…';
    } else if (data.kind === 'reasoning') {
      const thinking = this.ensureLiveThinking();
      thinking.querySelector('.reasoning').textContent += data.content || '';
    } else if (data.kind === 'tool') {
      const progress = this.ensureLiveProgress();
      progress.innerHTML = '🐵 使用工具 ' + escapeHTML(data.label || data.content || 'tool') + '…';
      const thinking = this.ensureLiveThinking();
      thinking.querySelector('.reasoning').textContent += '\n▸ 使用工具 ' + (data.label || data.content || 'tool') + '\n';
    } else if (data.kind === 'step') {
      const details = document.createElement('details');
      details.className = 'baton';
      details.innerHTML =
        '<summary>🔍 悟空·' + escapeHTML(data.label || 'step') + ' 的產出</summary>' +
        '<div class="baton-body">' + (data.content || '') + '</div>';
      this.log.appendChild(details);
      this.enhanceCodeBlocks(details);
    } else if (data.kind === 'answer') {
      if (this.liveProgress) this.liveProgress.remove();
      this.liveProgress = null;
      const div = this.appendBubble('assistant', unsafe(data.content_html || data.content || '').toString());
      if (data.message_id) {
        div.dataset.messageId = data.message_id;
        this.renderedMessageIds.add(String(data.message_id));
      }
      this.enhanceCodeBlocks(div);
      this.liveThinking = null;
    } else if (data.kind === 'error') {
      if (this.liveProgress) this.liveProgress.remove();
      this.liveProgress = null;
      const div = this.appendBubble('assistant', '⚠️ ' + escapeHTML(data.content || '處理失敗'));
      if (data.message_id) {
        div.dataset.messageId = data.message_id;
        this.renderedMessageIds.add(String(data.message_id));
      }
      this.liveThinking = null;
    }

    this.maybeScrollToBottom(wasNearBottom);
  }
```

- [ ] **Step 9: Keep `bubble()` behavior scoped to send-time turns**

Do not change `bubble()`. Web-submitted turns should stay pinned. Live event handlers must use `appendBubble()` so they can capture `wasNearBottom` and call `maybeScrollToBottom()` only when the user was already near the bottom.

- [ ] **Step 10: Run web crate tests**

Run: `cargo test -p wukong-web chat_ -- --nocapture`

Expected: PASS.

- [ ] **Step 11: Manual browser verification**

Run the web and Telegram services as normally documented for this repo. Verify:

1. Opening `/chat` chooses the newest `user:tg-*` scope when it exists.
2. Switching scope loads latest messages and lands at the bottom.
3. Sending a Telegram message appends the user bubble in the web console.
4. Telegram role/reasoning/tool progress appears in the web console using the existing thinking/tool UI.
5. Final Telegram answer appears without refreshing the page.

- [ ] **Step 12: Commit Task 4**

Run:

```bash
git add crates/wukong-web/static/components/wukong-chat.js
git commit -m "feat: live sync telegram chats in web console"
```

## Task 5: Full Verification And Cleanup

**Files:**
- Modify only if tests expose issues in files already touched by Tasks 1-4.

- [ ] **Step 1: Run formatting**

Run: `cargo fmt --all`

Expected: exits 0.

- [ ] **Step 2: Run focused test suites**

Run:

```bash
cargo test -p wukong-chat-history
cargo test -p wukong-web chat_ -- --nocapture
cargo test -p wukong-telegram
```

Expected: all PASS.

- [ ] **Step 3: Run broader workspace tests**

Run: `cargo test --workspace`

Expected: PASS. If unrelated pre-existing failures appear, record the failing command and exact failure without changing unrelated files.

- [ ] **Step 4: Run GitNexus change detection before final commit/PR**

Run:

```text
gitnexus_detect_changes({scope: "all", repo: "Wukong"})
```

Expected: affected symbols match chat history live events, web chat stream/UI, and Telegram dispatch.

- [ ] **Step 5: Inspect final git state**

Run:

```bash
git status --short
git diff --stat
git log --oneline -10
```

Expected: only intended files changed or committed; no unrelated `AGENTS.md` or `CLAUDE.md` changes staged.

- [ ] **Step 6: Commit any final fixes**

If formatting or verification changed files, commit only the intended files:

```bash
git add crates/wukong-chat-history/src/lib.rs crates/wukong-web/src/lib.rs crates/wukong-telegram/src/dispatch.rs crates/wukong-web/static/components/wukong-chat.js
git commit -m "test: verify telegram live sync"
```

Skip this commit if there are no remaining uncommitted intended changes.

## Self-Review Notes

- Spec coverage: Tasks cover default Telegram scope selection, scroll-to-bottom on load/switch, Telegram live SSE via shared database events, history fallback, duplicate avoidance via message ids, auth, scope filtering, cursor behavior, and tests.
- Plan hygiene: No incomplete file paths or unresolved implementation notes remain.
- Type consistency: `ChatLiveEvent` fields match `live_event_to_sse` JSON fields and front-end `handleLiveEvent` reads `id`, `kind`, `label`, `content`, `message_id`, and `created_at`. Store methods use `insert_live_event`, `live_events_after`, and `prune_live_events_before` consistently.
