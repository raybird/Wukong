# Shared Chat History Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Web, Telegram, and Scheduler persist/read the same scope-based conversation history, with Web source switching.

**Architecture:** Extract the Web-only `ChatHistoryStore` into a new shared `wukong-chat-history` crate. Wire Web, Telegram, and Scheduler to use that crate against the existing `WUKONG_MEMORY_DB` SQLite database. Add scope-aware Web APIs and a plain-vanilla source selector in the chat UI.

**Tech Stack:** Rust workspace crates, SQLx SQLite, Axum, Tokio, existing vanilla Web Components.

---

## File Structure

- Create `crates/wukong-chat-history/Cargo.toml`: shared crate manifest.
- Create `crates/wukong-chat-history/src/lib.rs`: shared chat history store, message/thread/scope response types, table creation, inserts, scope listing, and reads.
- Modify `Cargo.toml`: add `crates/wukong-chat-history` to workspace members.
- Modify `crates/wukong-web/Cargo.toml`: depend on `wukong-chat-history`.
- Modify `crates/wukong-web/src/lib.rs`: remove local `chat_history` module usage, add scope-aware query parsing and `/api/chat/scopes`.
- Delete `crates/wukong-web/src/chat_history.rs` after migrating the code and tests.
- Modify `crates/wukong-telegram/Cargo.toml`: depend on `wukong-chat-history`.
- Modify `crates/wukong-telegram/src/dispatch.rs`: accept an optional history store and persist Telegram turns/commands.
- Modify `crates/wukong-telegram/src/main.rs`: open `ChatHistoryStore` and pass it to dispatch.
- Modify `crates/wukong-schedulerd/Cargo.toml`: depend on `wukong-chat-history`.
- Modify `crates/wukong-schedulerd/src/notify.rs`: add a history-aware notification helper.
- Modify `crates/wukong-schedulerd/src/main.rs`: open and pass the shared history store.
- Modify `crates/wukong-web/static/components/wukong-chat.js`: add source selector and include selected `scope` in API calls.

---

### Task 1: Extract Shared Chat History Crate

**Files:**
- Create: `crates/wukong-chat-history/Cargo.toml`
- Create: `crates/wukong-chat-history/src/lib.rs`
- Modify: `Cargo.toml`
- Modify: `crates/wukong-web/Cargo.toml`
- Modify: `crates/wukong-web/src/lib.rs`
- Delete: `crates/wukong-web/src/chat_history.rs`

- [ ] **Step 1: Add the new crate to the workspace and Web dependencies**

Modify `Cargo.toml` workspace members to include `crates/wukong-chat-history`:

```toml
members = ["crates/wukong-memory", "crates/wukong-memoryd", "crates/wukong-gateway", "crates/wukong-orchestrator", "crates/wukong-skills", "crates/wukong-runtime", "crates/wukong-scheduler", "crates/wukong-schedulerd", "crates/wukong-cli", "crates/wukong-telegram", "crates/wukong-tg-client", "crates/wukong-render", "crates/wukong-web", "crates/wukong-settings", "crates/wukong-chat-history"]
```

Create `crates/wukong-chat-history/Cargo.toml`:

```toml
[package]
name = "wukong-chat-history"
edition.workspace = true
version.workspace = true

[lib]
name = "wukong_chat_history"
path = "src/lib.rs"

[dependencies]
serde = { workspace = true }
sqlx = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

Add this dependency to `crates/wukong-web/Cargo.toml` under `[dependencies]`:

```toml
wukong-chat-history = { path = "../wukong-chat-history" }
```

- [ ] **Step 2: Move the store into the shared crate**

Copy the existing content of `crates/wukong-web/src/chat_history.rs` into `crates/wukong-chat-history/src/lib.rs`, then extend it with scope listing types and helpers:

```rust
use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    pub id: i64,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub content_html: Option<String>,
    pub status: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatScope {
    pub scope: String,
    pub label: String,
    pub message_count: i64,
    pub updated_at: i64,
}

pub struct ChatHistoryStore {
    pool: SqlitePool,
}
```

Keep existing methods: `open`, `default_thread`, `insert_message`, `latest_messages`, `messages_before`, and `messages_for_date`.

Add these methods:

```rust
    pub async fn list_scopes(&self, default_scope: &str) -> Result<Vec<ChatScope>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT t.scope AS scope,
                    COALESCE(MAX(t.updated_at), 0) AS updated_at,
                    COUNT(m.id) AS message_count
             FROM chat_threads t
             LEFT JOIN chat_messages m ON m.thread_id = t.id
             GROUP BY t.scope
             ORDER BY updated_at DESC, scope ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut scopes: Vec<ChatScope> = rows
            .into_iter()
            .map(|row| {
                let scope: String = row.get("scope");
                ChatScope {
                    label: scope_label(&scope),
                    scope,
                    message_count: row.get("message_count"),
                    updated_at: row.get("updated_at"),
                }
            })
            .collect();

        if !scopes.iter().any(|s| s.scope == default_scope) {
            scopes.push(ChatScope {
                scope: default_scope.to_string(),
                label: scope_label(default_scope),
                message_count: 0,
                updated_at: 0,
            });
        }

        Ok(scopes)
    }
```

Add this helper:

```rust
pub fn scope_label(scope: &str) -> String {
    if let Some(id) = scope.strip_prefix("user:tg-") {
        format!("Telegram {id}")
    } else if let Some(project) = scope.strip_prefix("project:") {
        format!("Project {project}")
    } else if scope == "global" {
        "Global".to_string()
    } else {
        scope.to_string()
    }
}
```

- [ ] **Step 3: Move and extend store tests**

Move the existing `#[cfg(test)]` tests from `crates/wukong-web/src/chat_history.rs` into `crates/wukong-chat-history/src/lib.rs`.

Add this test:

```rust
#[tokio::test]
async fn list_scopes_includes_existing_and_empty_default() {
    let store = store().await;
    let tg = store.default_thread("user:tg-915354960").await.unwrap();
    store.insert_message(&tg, "user", "hi", None, "complete", 10).await.unwrap();

    let scopes = store.list_scopes("global").await.unwrap();

    assert!(scopes.iter().any(|s| {
        s.scope == "user:tg-915354960"
            && s.label == "Telegram 915354960"
            && s.message_count == 1
            && s.updated_at == 10
    }));
    assert!(scopes.iter().any(|s| {
        s.scope == "global" && s.label == "Global" && s.message_count == 0
    }));
}

#[test]
fn labels_known_scope_prefixes() {
    assert_eq!(scope_label("user:tg-12"), "Telegram 12");
    assert_eq!(scope_label("project:Wukong"), "Project Wukong");
    assert_eq!(scope_label("global"), "Global");
    assert_eq!(scope_label("agent:fixer"), "agent:fixer");
}
```

- [ ] **Step 4: Run the new crate tests and verify they pass**

Run: `cargo test -p wukong-chat-history`

Expected: PASS, including moved pagination/date tests and the new scope listing tests.

- [ ] **Step 5: Update Web imports and remove old module**

In `crates/wukong-web/src/lib.rs`, remove:

```rust
pub mod chat_history;
use chat_history::ChatHistoryStore;
```

Add:

```rust
use wukong_chat_history::{ChatHistoryStore, ChatMessage};
```

Change response fields using `chat_history::ChatMessage` to `ChatMessage`.

Change test references from:

```rust
crate::chat_history::ChatHistoryStore::open(&app_state.db_url)
```

to:

```rust
ChatHistoryStore::open(&app_state.db_url)
```

Delete `crates/wukong-web/src/chat_history.rs`.

- [ ] **Step 6: Run Web tests and verify they pass**

Run: `cargo test -p wukong-web`

Expected: PASS with the same 24 Web tests passing.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/wukong-chat-history crates/wukong-web/Cargo.toml crates/wukong-web/src/lib.rs crates/wukong-web/src/chat_history.rs
git commit -m "refactor(chat): extract shared history store"
```

---

### Task 2: Add Scope-Aware Web APIs

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Write failing Web tests for scope selection and scope listing**

Add these tests to the existing `#[cfg(test)] mod tests` in `crates/wukong-web/src/lib.rs`:

```rust
#[tokio::test]
async fn chat_scopes_lists_default_and_telegram_scope() {
    let app_state = state(None, &[]).await;
    let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
    let tg_thread = store.default_thread("user:tg-915354960").await.unwrap();
    store.insert_message(&tg_thread, "user", "from tg", None, "complete", 123).await.unwrap();

    let app = build_router(app_state);
    let resp = app
        .oneshot(Request::builder().uri("/api/chat/scopes").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("user:tg-915354960"), "body: {body}");
    assert!(body.contains("Telegram 915354960"), "body: {body}");
    assert!(body.contains("global"), "body: {body}");
}

#[tokio::test]
async fn chat_messages_reads_requested_scope() {
    let app_state = state(None, &[]).await;
    let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
    let default_thread = store.default_thread(&app_state.scope).await.unwrap();
    let tg_thread = store.default_thread("user:tg-915354960").await.unwrap();
    store.insert_message(&default_thread, "user", "from web", None, "complete", 100).await.unwrap();
    store.insert_message(&tg_thread, "user", "from telegram", None, "complete", 101).await.unwrap();

    let app = build_router(app_state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/api/chat/messages?scope=user%3Atg-915354960")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("from telegram"), "body: {body}");
    assert!(!body.contains("from web"), "body: {body}");
}

#[tokio::test]
async fn chat_turn_records_into_requested_scope() {
    let app_state = state(None, &["oracle", "scoped answer"]).await;
    let db_url = app_state.db_url.clone();
    let app = build_router(app_state);
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/chat?q=hi&scope=user%3Atg-915354960")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = body_string(resp).await;

    let store = ChatHistoryStore::open(&db_url).await.unwrap();
    let tg_thread = store.default_thread("user:tg-915354960").await.unwrap();
    let messages = store.latest_messages(&tg_thread, 10).await.unwrap();
    assert!(messages.iter().any(|m| m.role == "user" && m.content == "hi"));
    assert!(messages.iter().any(|m| m.role == "assistant" && m.content == "scoped answer"));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run: `cargo test -p wukong-web chat_scopes_lists_default_and_telegram_scope chat_messages_reads_requested_scope chat_turn_records_into_requested_scope`

Expected: FAIL because `/api/chat/scopes` is not routed and `scope` is ignored.

- [ ] **Step 3: Add scope query fields and response type**

In `crates/wukong-web/src/lib.rs`, update `ChatQuery` and `ChatMessagesQuery`:

```rust
#[derive(Debug, Deserialize)]
struct ChatQuery {
    q: Option<String>,
    token: Option<String>,
    scope: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ChatMessagesQuery {
    token: Option<String>,
    before: Option<i64>,
    limit: Option<i64>,
    date: Option<String>,
    scope: Option<String>,
}
```

Add a helper near `capped_limit`:

```rust
fn selected_scope(default_scope: &str, requested: Option<String>) -> String {
    requested
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_scope.to_string())
}
```

- [ ] **Step 4: Implement `/api/chat/scopes`**

Add handler:

```rust
async fn get_chat_scopes<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<ChatMessagesQuery>,
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
    match store.list_scopes(&state.scope).await {
        Ok(scopes) => Json(scopes).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

Add route in `build_router`:

```rust
.route("/api/chat/scopes", axum::routing::get(get_chat_scopes::<B>))
```

- [ ] **Step 5: Apply selected scope in Web chat and messages**

In `chat`, replace direct use of `state.scope` with:

```rust
let scope = selected_scope(&state.scope, params.scope.clone());
```

Use that `scope` for `store.default_thread(&scope)` and move the same `scope` into the background thread's `GatewayConfig`.

In `get_chat_messages`, add:

```rust
let scope = selected_scope(&state.scope, params.scope.clone());
let thread = match store.default_thread(&scope).await {
    Ok(thread) => thread,
    Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
};
```

- [ ] **Step 6: Run Web tests and verify they pass**

Run: `cargo test -p wukong-web`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat(web): add scope-aware chat APIs"
```

---

### Task 3: Persist Telegram Turns and Commands to Shared History

**Files:**
- Modify: `crates/wukong-telegram/Cargo.toml`
- Modify: `crates/wukong-telegram/src/dispatch.rs`
- Modify: `crates/wukong-telegram/src/main.rs`

- [ ] **Step 1: Add dependency**

Add to `crates/wukong-telegram/Cargo.toml`:

```toml
wukong-chat-history = { path = "../wukong-chat-history" }
```

- [ ] **Step 2: Write failing dispatch tests**

In `crates/wukong-telegram/src/dispatch.rs` tests, update `open_memory` to return both memory and URL:

```rust
async fn open_memory_with_url() -> (Memory, String) {
    let f = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", f.path().display());
    std::mem::forget(f);
    (Memory::open(&url).await.unwrap(), url)
}

async fn open_memory() -> Memory {
    open_memory_with_url().await.0
}
```

Add this test:

```rust
#[tokio::test]
async fn turn_records_telegram_user_and_assistant_messages_in_chat_history() {
    let client = MockTgClient::default();
    let (mem, db_url) = open_memory_with_url().await;
    let history = wukong_chat_history::ChatHistoryStore::open(&db_url).await.unwrap();
    let backend = MockBackend::new(&["oracle", "telegram answer"]);
    let msg = TgMessage { update_id: 1, chat_id: 12, text: "hello from tg".to_string() };

    handle_message(&client, &mem, &base_cfg(), &backend, Some(&history), &[12], &msg).await;

    let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
    let messages = history.latest_messages(&thread, 10).await.unwrap();
    assert!(messages.iter().any(|m| m.role == "user" && m.content == "hello from tg"));
    assert!(messages.iter().any(|m| m.role == "assistant" && m.content == "telegram answer"));
}

#[tokio::test]
async fn command_records_telegram_user_and_reply_messages_in_chat_history() {
    let client = MockTgClient::default();
    let (mem, db_url) = open_memory_with_url().await;
    let history = wukong_chat_history::ChatHistoryStore::open(&db_url).await.unwrap();
    let backend = MockBackend::new(&[]);
    let msg = TgMessage { update_id: 1, chat_id: 12, text: "/new".to_string() };

    handle_message(&client, &mem, &base_cfg(), &backend, Some(&history), &[12], &msg).await;

    let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
    let messages = history.latest_messages(&thread, 10).await.unwrap();
    assert!(messages.iter().any(|m| m.role == "user" && m.content == "/new"));
    assert!(messages.iter().any(|m| m.role == "assistant" && m.content.contains("已開新")));
}
```

- [ ] **Step 3: Run tests and verify they fail**

Run: `cargo test -p wukong-telegram turn_records_telegram_user_and_assistant_messages_in_chat_history command_records_telegram_user_and_reply_messages_in_chat_history`

Expected: FAIL because `handle_message` does not accept history and does not persist chat messages.

- [ ] **Step 4: Update dispatch signature and add persistence helpers**

In `dispatch.rs`, import:

```rust
use wukong_chat_history::ChatHistoryStore;
```

Change signature:

```rust
pub async fn handle_message<C, B>(
    client: &C,
    mem: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    history: Option<&ChatHistoryStore>,
    allow: &[i64],
    msg: &TgMessage,
) where
```

Add helper near `bubble_text`:

```rust
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn record_chat(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    role: &str,
    content: &str,
    content_html: Option<&str>,
    status: &str,
) {
    let Some(history) = history else { return; };
    match history.default_thread(scope).await {
        Ok(thread) => {
            if let Err(e) = history
                .insert_message(&thread, role, content, content_html, status, now_unix())
                .await
            {
                eprintln!("warning: telegram chat history insert failed: {e}");
            }
        }
        Err(e) => eprintln!("warning: telegram chat history thread failed: {e}"),
    }
}
```

- [ ] **Step 5: Persist command and turn messages**

For command branch, after `cfg.scope = scope_for_chat(chat_id);`, add:

```rust
record_chat(history, &cfg.scope, "user", &msg.text, None, "complete").await;
```

After computing `reply`, before sending it, add:

```rust
record_chat(history, &cfg.scope, "assistant", &reply, None, "complete").await;
```

For unsupported commands, build a `reply` string first, record it, then send it.

For turn branch, after `cfg.scope = scope_for_chat(chat_id);`, add:

```rust
record_chat(history, &cfg.scope, "user", &input, None, "complete").await;
```

On success, after `Ok(out) => {`, add:

```rust
let html = wukong_render::to_web_html(&out.text);
record_chat(history, &cfg.scope, "assistant", &out.text, Some(&html), "complete").await;
```

On error, before editing status bubble, add:

```rust
let err = format!("⚠️ 處理失敗：{e}");
record_chat(history, &cfg.scope, "assistant", &err, None, "error").await;
```

Then use `err` for `edit_message_text`.

- [ ] **Step 6: Update all existing tests to pass `None` or `Some(&history)`**

Existing tests that do not inspect chat history should call:

```rust
handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
```

New history tests use `Some(&history)`.

- [ ] **Step 7: Wire production Telegram main**

In `crates/wukong-telegram/src/main.rs`, import:

```rust
use wukong_chat_history::ChatHistoryStore;
```

After opening `memory`, open history with the same DB URL:

```rust
let history = match ChatHistoryStore::open(&db_url).await {
    Ok(store) => Some(store),
    Err(e) => {
        eprintln!("warning: chat history disabled for telegram: {e}");
        None
    }
};
```

Change dispatch call:

```rust
handle_message(&client, &memory, &base_cfg, &backend, history.as_ref(), &allow, &msg).await;
```

- [ ] **Step 8: Run Telegram tests and verify they pass**

Run: `cargo test -p wukong-telegram`

Expected: PASS.

- [ ] **Step 9: Commit**

```bash
git add crates/wukong-telegram/Cargo.toml crates/wukong-telegram/src/dispatch.rs crates/wukong-telegram/src/main.rs
git commit -m "feat(telegram): persist chat history"
```

---

### Task 4: Persist Scheduler Telegram Notifications to Shared History

**Files:**
- Modify: `crates/wukong-schedulerd/Cargo.toml`
- Modify: `crates/wukong-schedulerd/src/notify.rs`
- Modify: `crates/wukong-schedulerd/src/main.rs`

- [ ] **Step 1: Add dependency**

Add to `crates/wukong-schedulerd/Cargo.toml`:

```toml
wukong-chat-history = { path = "../wukong-chat-history" }
```

- [ ] **Step 2: Write failing notification history test**

In `notify.rs` tests, add helper:

```rust
async fn history() -> (wukong_chat_history::ChatHistoryStore, String) {
    let f = tempfile::NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", f.path().display());
    std::mem::forget(f);
    (wukong_chat_history::ChatHistoryStore::open(&url).await.unwrap(), url)
}
```

Add test:

```rust
#[tokio::test]
async fn records_scheduled_telegram_result_in_chat_history() {
    let client = MockTgClient::default();
    let (history, _url) = history().await;
    let job = turn_job("user:tg-555");

    let sent = notify_turn_result_with_history(&client, Some(&history), &job, &ok("今天一切正常"))
        .await
        .unwrap();

    assert!(sent);
    let thread = history.default_thread("user:tg-555").await.unwrap();
    let messages = history.latest_messages(&thread, 10).await.unwrap();
    assert!(messages.iter().any(|m| {
        m.role == "assistant"
            && m.content.contains("⏰ 晨間報告")
            && m.content.contains("今天一切正常")
    }));
}
```

- [ ] **Step 3: Run test and verify it fails**

Run: `cargo test -p wukong-schedulerd records_scheduled_telegram_result_in_chat_history`

Expected: FAIL because `notify_turn_result_with_history` does not exist.

- [ ] **Step 4: Implement history-aware notification helper**

In `notify.rs`, import:

```rust
use wukong_chat_history::ChatHistoryStore;
```

Add helper:

```rust
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn record_history(history: Option<&ChatHistoryStore>, scope: &str, content: &str, status: &str) {
    let Some(history) = history else { return; };
    match history.default_thread(scope).await {
        Ok(thread) => {
            let html = wukong_render::to_web_html(content);
            if let Err(e) = history
                .insert_message(&thread, "assistant", content, Some(&html), status, now_unix())
                .await
            {
                eprintln!("warning: scheduler chat history insert failed: {e}");
            }
        }
        Err(e) => eprintln!("warning: scheduler chat history thread failed: {e}"),
    }
}
```

Add public wrapper:

```rust
pub async fn notify_turn_result_with_history<C: TgClient + Sync>(
    client: &C,
    history: Option<&ChatHistoryStore>,
    job: &Job,
    output: &ExecutionOutput,
) -> Result<bool, TgError> {
    let JobKind::Turn { scope, .. } = &job.kind else {
        return Ok(false);
    };
    let Some(_chat_id) = chat_id_from_scope(scope) else {
        return Ok(false);
    };

    let content = if output.success {
        if output.message.trim().is_empty() {
            format!("⏰ {}（無內容）", job.name)
        } else {
            format!("⏰ {}\n\n{}", job.name, output.message)
        }
    } else {
        format!("⏰ {} 執行失敗：{}", job.name, failure_summary(&output.message))
    };
    let status = if output.success { "complete" } else { "error" };

    let sent = notify_turn_result(client, job, output).await?;
    if sent {
        record_history(history, scope, &content, status).await;
    }
    Ok(sent)
}
```

- [ ] **Step 5: Wire daemon main to open and pass history**

In `main.rs`, import:

```rust
use wukong_chat_history::ChatHistoryStore;
```

After `let store = SchedulerStore::open...`, open history:

```rust
let history = match ChatHistoryStore::open(&cfg.db_url).await {
    Ok(store) => Some(store),
    Err(e) => {
        eprintln!("warning: chat history disabled for scheduler: {e}");
        None
    }
};
```

Add `history.as_ref()` parameter to `run_scan` signature and call sites.

Inside `run_scan`, replace:

```rust
notify::notify_turn_result(client, &job, &output).await
```

with:

```rust
notify::notify_turn_result_with_history(client, history, &job, &output).await
```

- [ ] **Step 6: Run schedulerd tests and verify they pass**

Run: `cargo test -p wukong-schedulerd`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add crates/wukong-schedulerd/Cargo.toml crates/wukong-schedulerd/src/main.rs crates/wukong-schedulerd/src/notify.rs
git commit -m "feat(scheduler): persist telegram notifications"
```

---

### Task 5: Add Web Scope Selector UI

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
- Modify: `crates/wukong-web/static/styles.css`
- Add or Modify: `scripts/test-web-chat-scope.sh`

- [ ] **Step 1: Write failing static regression script**

Create `scripts/test-web-chat-scope.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

chat="crates/wukong-web/static/components/wukong-chat.js"

if ! grep -q "/api/chat/scopes" "$chat"; then
    echo "FAIL: chat component should fetch available chat scopes" >&2
    exit 1
fi

if ! grep -q "selectedScope" "$chat"; then
    echo "FAIL: chat component should track selectedScope" >&2
    exit 1
fi

if ! grep -q "scope=" "$chat"; then
    echo "FAIL: chat API calls should include selected scope" >&2
    exit 1
fi

if ! grep -q "chat-source" "$chat"; then
    echo "FAIL: chat component should render a source selector" >&2
    exit 1
fi

echo "web chat scope checks passed"
```

Make executable: `chmod +x scripts/test-web-chat-scope.sh`

- [ ] **Step 2: Run script and verify it fails**

Run: `bash scripts/test-web-chat-scope.sh`

Expected: FAIL because scope selector is not implemented.

- [ ] **Step 3: Update chat component state and rendering**

In `wukong-chat.js`, add state in constructor or initialization area:

```js
this.scopes = [];
this.selectedScope = '';
```

Add selector markup near the top of the component template:

```js
<div class="chat-source">
  <label>來源
    <select id="chat-scope"></select>
  </label>
</div>
```

After querying DOM nodes, add:

```js
this.scopeSelect = this.querySelector('#chat-scope');
this.scopeSelect.addEventListener('change', () => {
  this.selectedScope = this.scopeSelect.value;
  this.resetMessages();
  this.loadLatest();
});
```

- [ ] **Step 4: Add scope loading and query helper**

Add methods:

```js
scopeParam() {
  return this.selectedScope ? '&scope=' + encodeURIComponent(this.selectedScope) : '';
}

async loadScopes() {
  const resp = await fetch('/api/chat/scopes' + this.tokenParam('?'));
  if (!resp.ok) return;
  this.scopes = await resp.json();
  if (!this.selectedScope && this.scopes.length > 0) {
    this.selectedScope = this.scopes[0].scope;
  }
  this.scopeSelect.innerHTML = this.scopes
    .map((s) => `<option value="${this.escapeAttr(s.scope)}">${this.escapeText(s.label)}</option>`)
    .join('');
  this.scopeSelect.value = this.selectedScope;
}

escapeText(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

escapeAttr(value) {
  return this.escapeText(value);
}
```

If the component already has a token helper that only returns `?token=...`, keep it and add a URL builder instead:

```js
chatUrl(path, params = {}) {
  const search = new URLSearchParams();
  if (window.WUKONG_TOKEN) search.set('token', window.WUKONG_TOKEN);
  if (this.selectedScope) search.set('scope', this.selectedScope);
  Object.entries(params).forEach(([k, v]) => {
    if (v !== undefined && v !== null && v !== '') search.set(k, v);
  });
  const qs = search.toString();
  return qs ? path + '?' + qs : path;
}
```

Use `chatUrl` for `/api/chat/messages`, date/before loads, and `/chat?q=...`.

- [ ] **Step 5: Ensure initial load fetches scopes before messages**

In `connectedCallback` or equivalent startup method, call:

```js
await this.loadScopes();
await this.loadLatest();
```

If the current startup method is not async, wrap with:

```js
this.initialize();
```

and add:

```js
async initialize() {
  await this.loadScopes();
  await this.loadLatest();
}
```

- [ ] **Step 6: Add minimal CSS**

In `crates/wukong-web/static/styles.css`, add:

```css
.chat-source {
  display: flex;
  justify-content: flex-end;
  margin-bottom: 0.75rem;
}

.chat-source label {
  display: flex;
  align-items: center;
  gap: 0.5rem;
  color: var(--muted);
  font-size: 0.9rem;
}

.chat-source select {
  border: 1px solid var(--border);
  border-radius: 999px;
  padding: 0.35rem 0.75rem;
  background: var(--surface);
  color: var(--text);
}
```

- [ ] **Step 7: Run static and JS syntax checks**

Run:

```bash
bash scripts/test-web-chat-scope.sh
node --check crates/wukong-web/static/components/wukong-chat.js
```

Expected: PASS.

- [ ] **Step 8: Commit**

```bash
git add crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/static/styles.css scripts/test-web-chat-scope.sh
git commit -m "feat(web): add chat source selector"
```

---

### Task 6: End-to-End Verification and Release

**Files:**
- Modify: `README.md` if needed for user-visible docs.

- [ ] **Step 1: Run focused package tests**

Run:

```bash
cargo test -p wukong-chat-history
cargo test -p wukong-web
cargo test -p wukong-telegram
cargo test -p wukong-schedulerd
bash scripts/test-web-chat-scope.sh
```

Expected: all PASS.

- [ ] **Step 2: Run full workspace test**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 3: Run GitNexus change detection before commit/release**

Run via MCP: `gitnexus_detect_changes({ scope: "all", repo: "Wukong" })`

Expected: review output; if HIGH or CRITICAL, stop and report before release.

- [ ] **Step 4: Update README if the UI behavior is not documented**

Add a short note in the Web/Docker section:

```md
Web 的對話頁可切換來源 scope；Telegram 對話會以 `Telegram <chat_id>` 顯示，並與 Telegram bot / schedulerd 共用同一份歷史紀錄。
```

- [ ] **Step 5: Commit final docs if changed**

```bash
git add README.md
git commit -m "docs: describe shared chat history"
```

Skip this commit if README did not change.

- [ ] **Step 6: Tag and publish a patch release**

Use the next patch version after the current latest tag. If current latest is `v0.16.4`, use `v0.16.5`.

```bash
git status --short
git tag -a v0.16.5 -m "🐵 v0.16.5 — 同源對話：網頁觀照 × 電報留痕"
git push origin main
git push origin v0.16.5
gh release create v0.16.5 --title "🐵 v0.16.5 — 同源對話：網頁觀照 × 電報留痕" --notes "## 新增\n- Web 對話頁可切換 conversation scope/source。\n- Telegram incoming/reply messages persist to the same chat history timeline.\n- Scheduler Telegram notifications persist to the same Telegram-scoped timeline.\n\n## 驗證\n- cargo test -p wukong-chat-history\n- cargo test -p wukong-web\n- cargo test -p wukong-telegram\n- cargo test -p wukong-schedulerd\n- cargo test\n- bash scripts/test-web-chat-scope.sh"
```

- [ ] **Step 7: Verify release workflow and Docker bundle**

Run:

```bash
gh run list --workflow release.yml --limit 3
gh run watch <run-id> --exit-status
gh release view v0.16.5 --json url,assets --jq '{url: .url, assets: [.assets[].name]}'
mkdir -p /tmp/opencode/wukong-v0.16.5
gh release download v0.16.5 --pattern wukong-docker-v0.16.5.tar.gz --dir /tmp/opencode/wukong-v0.16.5 --clobber
tar -xzf /tmp/opencode/wukong-v0.16.5/wukong-docker-v0.16.5.tar.gz -C /tmp/opencode/wukong-v0.16.5
grep '^ARG VERSION=' /tmp/opencode/wukong-v0.16.5/wukong-docker/Dockerfile
```

Expected: workflow success, release contains `wukong-docker-v0.16.5.tar.gz`, and Dockerfile prints `ARG VERSION=v0.16.5`.

---

## Self-Review

- Spec coverage: shared crate, Web scope APIs, Web UI selector, Telegram persistence, Scheduler persistence, error handling, tests, and release notes are all mapped to tasks.
- Red-flag scan: no incomplete markers or vague implementation-only steps remain; each implementation step names exact files, signatures, commands, and expected outcomes.
- Type consistency: `ChatHistoryStore`, `ChatMessage`, `ChatScope`, `list_scopes`, `selected_scope`, `notify_turn_result_with_history`, and `handle_message(..., history, ...)` are introduced before later use.
