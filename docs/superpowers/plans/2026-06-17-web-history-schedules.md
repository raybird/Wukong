# Web History And Schedule Settings Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add routed web settings, persisted web chat history with 10-message lazy loading and date jump, plus schedule list/enable/disable/delete management.

**Architecture:** Keep the plain vanilla web app and Axum server. Add focused server-side modules for chat history storage and schedule/system API shaping, then update the static app shell with a tiny hash router and route-specific custom elements.

**Tech Stack:** Rust 2021, Axum 0.7, SQLx SQLite, Tokio, existing `wukong-memory`, `wukong-scheduler`, vanilla JS custom elements, static CSS.

---

## Pre-Implementation Requirements

- Before editing any existing Rust function, method, or class-like symbol, run `gitnexus_impact` for that symbol as required by `AGENTS.md`.
- Before committing each task, run `gitnexus_detect_changes({scope: "staged", repo: "Wukong"})`.
- Use `apply_patch` for manual edits.
- Commit messages must not contain AI attribution.

## File Map

- Modify `crates/wukong-web/Cargo.toml`: add `sqlx`, `chrono`, and `wukong-scheduler` dependencies.
- Modify `crates/wukong-web/src/main.rs`: pass `db_url` into `AppState`.
- Modify `crates/wukong-web/src/lib.rs`: wire new static assets, app state field, handlers, and routes.
- Create `crates/wukong-web/src/chat_history.rs`: SQLite schema and query/write API for web chat messages.
- Create `crates/wukong-web/src/schedule_api.rs`: response shaping and schedule store wrappers for web handlers.
- Create `crates/wukong-web/src/system_api.rs`: system summary response shaping.
- Modify `crates/wukong-web/static/index.html`: convert to app shell with a single outlet.
- Modify `crates/wukong-web/static/app.js`: define new custom elements and implement tiny hash router.
- Modify `crates/wukong-web/static/components/wukong-chat.js`: add initial history load, upward pagination, date jump, and stored-message rendering.
- Modify `crates/wukong-web/static/components/wukong-settings.js`: keep Telegram form but make it route-specific.
- Create `crates/wukong-web/static/components/wukong-schedules.js`: schedule list and actions.
- Create `crates/wukong-web/static/components/wukong-system.js`: system summary page.
- Modify `crates/wukong-web/static/styles.css`: app shell, routed pages, chat history controls, schedule/system cards.

## Task 1: Add Chat History Store

**Files:**
- Modify: `crates/wukong-web/Cargo.toml`
- Modify: `crates/wukong-web/src/lib.rs`
- Create: `crates/wukong-web/src/chat_history.rs`
- Test: `crates/wukong-web/src/chat_history.rs`

- [ ] **Step 1: Run impact analysis before editing exports in `lib.rs`**

Run GitNexus impact for `AppState` and `build_router` before touching `crates/wukong-web/src/lib.rs`:

```text
gitnexus_impact({target: "AppState", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
gitnexus_impact({target: "build_router", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
```

Expected: low or medium risk. If HIGH or CRITICAL, stop and report before editing.

- [ ] **Step 2: Add failing store tests**

Create `crates/wukong-web/src/chat_history.rs` with tests first:

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

pub struct ChatHistoryStore {
    pool: SqlitePool,
}

impl ChatHistoryStore {
    pub async fn open(_db_url: &str) -> Result<Self, sqlx::Error> {
        unimplemented!("implemented in Step 4")
    }

    pub async fn default_thread(&self, _scope: &str) -> Result<String, sqlx::Error> {
        unimplemented!("implemented in Step 4")
    }

    pub async fn insert_message(
        &self,
        _thread_id: &str,
        _role: &str,
        _content: &str,
        _content_html: Option<&str>,
        _status: &str,
        _created_at: i64,
    ) -> Result<i64, sqlx::Error> {
        unimplemented!("implemented in Step 4")
    }

    pub async fn latest_messages(&self, _thread_id: &str, _limit: i64) -> Result<Vec<ChatMessage>, sqlx::Error> {
        unimplemented!("implemented in Step 4")
    }

    pub async fn messages_before(&self, _thread_id: &str, _before: i64, _limit: i64) -> Result<Vec<ChatMessage>, sqlx::Error> {
        unimplemented!("implemented in Step 4")
    }

    pub async fn messages_for_date(
        &self,
        _thread_id: &str,
        _start: i64,
        _end: i64,
        _limit: i64,
    ) -> Result<Vec<ChatMessage>, sqlx::Error> {
        unimplemented!("implemented in Step 4")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    async fn store() -> ChatHistoryStore {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        ChatHistoryStore::open(&url).await.unwrap()
    }

    #[tokio::test]
    async fn default_thread_is_stable_per_scope() {
        let store = store().await;
        let a = store.default_thread("global").await.unwrap();
        let b = store.default_thread("global").await.unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "scope:global");
    }

    #[tokio::test]
    async fn latest_messages_returns_newest_window_in_ascending_order() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        for i in 0..12 {
            store.insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i).await.unwrap();
        }

        let messages = store.latest_messages(&thread, 10).await.unwrap();
        assert_eq!(messages.len(), 10);
        assert_eq!(messages.first().unwrap().content, "m2");
        assert_eq!(messages.last().unwrap().content, "m11");
    }

    #[tokio::test]
    async fn messages_before_returns_older_window() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        let mut ids = Vec::new();
        for i in 0..12 {
            let id = store.insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i).await.unwrap();
            ids.push(id);
        }

        let messages = store.messages_before(&thread, ids[10], 10).await.unwrap();
        assert_eq!(messages.len(), 10);
        assert_eq!(messages.first().unwrap().content, "m0");
        assert_eq!(messages.last().unwrap().content, "m9");
    }

    #[tokio::test]
    async fn messages_for_date_filters_by_time_range() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        store.insert_message(&thread, "user", "old", None, "complete", 9).await.unwrap();
        store.insert_message(&thread, "user", "in", None, "complete", 10).await.unwrap();
        store.insert_message(&thread, "assistant", "also in", Some("<p>also in</p>"), "complete", 19).await.unwrap();
        store.insert_message(&thread, "user", "new", None, "complete", 20).await.unwrap();

        let messages = store.messages_for_date(&thread, 10, 20, 10).await.unwrap();
        assert_eq!(messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(), vec!["in", "also in"]);
        assert_eq!(messages[1].content_html.as_deref(), Some("<p>also in</p>"));
    }
}
```

- [ ] **Step 3: Add dependencies and module declaration**

Modify `crates/wukong-web/Cargo.toml` dependencies:

```toml
sqlx = { workspace = true }
chrono = { workspace = true }
wukong-scheduler = { path = "../wukong-scheduler" }
```

Add near the top of `crates/wukong-web/src/lib.rs`:

```rust
pub mod chat_history;
```

- [ ] **Step 4: Run failing tests**

Run:

```bash
cargo test -p wukong-web chat_history -- --nocapture
```

Expected: tests compile and fail at runtime with `not implemented: implemented in Step 4`.

- [ ] **Step 5: Implement store**

Replace the `unimplemented!` methods in `crates/wukong-web/src/chat_history.rs` with:

```rust
impl ChatHistoryStore {
    pub async fn open(db_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(db_url).await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_threads (
                id TEXT PRIMARY KEY,
                scope TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                content_html TEXT,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(thread_id) REFERENCES chat_threads(id) ON DELETE CASCADE
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS chat_messages_thread_id_id_idx
             ON chat_messages(thread_id, id)",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS chat_messages_thread_id_created_at_idx
             ON chat_messages(thread_id, created_at)",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    pub async fn default_thread(&self, scope: &str) -> Result<String, sqlx::Error> {
        let id = format!("scope:{scope}");
        let now = now_unix();
        sqlx::query(
            "INSERT INTO chat_threads (id, scope, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at",
        )
        .bind(&id)
        .bind(scope)
        .bind("Default")
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn insert_message(
        &self,
        thread_id: &str,
        role: &str,
        content: &str,
        content_html: Option<&str>,
        status: &str,
        created_at: i64,
    ) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO chat_messages (thread_id, role, content, content_html, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
        )
        .bind(thread_id)
        .bind(role)
        .bind(content)
        .bind(content_html)
        .bind(status)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;
        sqlx::query("UPDATE chat_threads SET updated_at = ?2 WHERE id = ?1")
            .bind(thread_id)
            .bind(created_at)
            .execute(&self.pool)
            .await?;
        Ok(row.get("id"))
    }

    pub async fn latest_messages(&self, thread_id: &str, limit: i64) -> Result<Vec<ChatMessage>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT * FROM (
                 SELECT id, thread_id, role, content, content_html, status, created_at
                 FROM chat_messages
                 WHERE thread_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2
             ) ORDER BY id ASC",
        )
        .bind(thread_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_message).collect())
    }

    pub async fn messages_before(&self, thread_id: &str, before: i64, limit: i64) -> Result<Vec<ChatMessage>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT * FROM (
                 SELECT id, thread_id, role, content, content_html, status, created_at
                 FROM chat_messages
                 WHERE thread_id = ?1 AND id < ?2
                 ORDER BY id DESC
                 LIMIT ?3
             ) ORDER BY id ASC",
        )
        .bind(thread_id)
        .bind(before)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_message).collect())
    }

    pub async fn messages_for_date(
        &self,
        thread_id: &str,
        start: i64,
        end: i64,
        limit: i64,
    ) -> Result<Vec<ChatMessage>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, thread_id, role, content, content_html, status, created_at
             FROM chat_messages
             WHERE thread_id = ?1 AND created_at >= ?2 AND created_at < ?3
             ORDER BY created_at ASC, id ASC
             LIMIT ?4",
        )
        .bind(thread_id)
        .bind(start)
        .bind(end)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_message).collect())
    }
}

fn row_to_message(row: sqlx::sqlite::SqliteRow) -> ChatMessage {
    ChatMessage {
        id: row.get("id"),
        thread_id: row.get("thread_id"),
        role: row.get("role"),
        content: row.get("content"),
        content_html: row.get("content_html"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
```

- [ ] **Step 6: Run store tests**

Run:

```bash
cargo test -p wukong-web chat_history -- --nocapture
```

Expected: all `chat_history` tests pass.

- [ ] **Step 7: Commit Task 1**

Run:

```bash
git add crates/wukong-web/Cargo.toml crates/wukong-web/src/lib.rs crates/wukong-web/src/chat_history.rs
gitnexus_detect_changes --scope staged
git commit -m "feat(web): add chat history store"
```

If using MCP instead of CLI for GitNexus, run `gitnexus_detect_changes({scope: "staged", repo: "Wukong"})` before the commit.

## Task 2: Add Chat History API And SSE Persistence

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`
- Test: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run impact analysis**

Run:

```text
gitnexus_impact({target: "chat", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
gitnexus_impact({target: "AppState", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
```

Expected: understand affected tests/routes before editing.

- [ ] **Step 2: Extend app state and test helper**

Add `db_url` to `AppState` in `crates/wukong-web/src/lib.rs`:

```rust
pub db_url: String,
```

Update `Clone`:

```rust
db_url: self.db_url.clone(),
```

Update each test state helper to include:

```rust
db_url: url,
```

Update `crates/wukong-web/src/main.rs` state construction:

```rust
db_url,
```

- [ ] **Step 3: Add failing API tests**

Add tests in the existing `#[cfg(test)] mod tests` in `crates/wukong-web/src/lib.rs`:

```rust
#[tokio::test]
async fn chat_messages_requires_token_when_set() {
    let app = build_router(state(Some("sekret"), &[]).await);
    let resp = app
        .oneshot(Request::builder().uri("/api/chat/messages").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn chat_messages_returns_latest_ten() {
    let app_state = state(None, &[]).await;
    let store = crate::chat_history::ChatHistoryStore::open(&app_state.db_url).await.unwrap();
    let thread = store.default_thread(&app_state.scope).await.unwrap();
    for i in 0..12 {
        store.insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i).await.unwrap();
    }
    let app = build_router(app_state);
    let resp = app
        .oneshot(Request::builder().uri("/api/chat/messages").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(!body.contains("m0"), "body should omit oldest rows: {body}");
    assert!(body.contains("m2"), "body should include first returned row: {body}");
    assert!(body.contains("m11"), "body should include newest row: {body}");
}

#[tokio::test]
async fn chat_messages_before_returns_older_ten() {
    let app_state = state(None, &[]).await;
    let store = crate::chat_history::ChatHistoryStore::open(&app_state.db_url).await.unwrap();
    let thread = store.default_thread(&app_state.scope).await.unwrap();
    let mut ids = Vec::new();
    for i in 0..12 {
        ids.push(store.insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i).await.unwrap());
    }
    let app = build_router(app_state);
    let resp = app
        .oneshot(Request::builder().uri(format!("/api/chat/messages?before={}", ids[10])).body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("m0"), "body: {body}");
    assert!(body.contains("m9"), "body: {body}");
    assert!(!body.contains("m10"), "body should omit boundary row: {body}");
}

#[tokio::test]
async fn chat_turn_records_user_and_assistant_messages() {
    let app_state = state(None, &["oracle", "**ans**"]).await;
    let db_url = app_state.db_url.clone();
    let app = build_router(app_state.clone());
    let resp = app
        .oneshot(Request::builder().uri("/chat?q=hi").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let _ = body_string(resp).await;

    let store = crate::chat_history::ChatHistoryStore::open(&db_url).await.unwrap();
    let thread = store.default_thread("global").await.unwrap();
    let messages = store.latest_messages(&thread, 10).await.unwrap();
    assert!(messages.iter().any(|m| m.role == "user" && m.content == "hi"));
    assert!(messages.iter().any(|m| m.role == "assistant" && m.content == "**ans**" && m.content_html.as_deref() == Some("<p><strong>ans</strong></p>\n")));
}
```

- [ ] **Step 4: Run failing API tests**

Run:

```bash
cargo test -p wukong-web chat_messages -- --nocapture
```

Expected: compile fails or tests fail because handlers/routes are missing.

- [ ] **Step 5: Implement query types and handlers**

Add imports in `crates/wukong-web/src/lib.rs`:

```rust
use chrono::{NaiveDate, TimeZone, Utc};
use chat_history::ChatHistoryStore;
```

Add query/response types near existing query structs:

```rust
#[derive(serde::Deserialize)]
struct ChatMessagesQuery {
    token: Option<String>,
    before: Option<i64>,
    date: Option<String>,
    limit: Option<i64>,
}

#[derive(serde::Serialize)]
struct ChatMessagesResponse {
    messages: Vec<chat_history::ChatMessage>,
    has_more: bool,
}

fn capped_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(10).clamp(1, 50)
}

fn date_bounds_utc(date: &str) -> Result<(i64, i64), String> {
    let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let start = day.and_hms_opt(0, 0, 0).ok_or_else(|| "invalid date".to_string())?;
    let end = day.succ_opt().ok_or_else(|| "invalid date".to_string())?.and_hms_opt(0, 0, 0).ok_or_else(|| "invalid date".to_string())?;
    Ok((Utc.from_utc_datetime(&start).timestamp(), Utc.from_utc_datetime(&end).timestamp()))
}
```

Add handler:

```rust
async fn get_chat_messages<B>(
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

    let limit = capped_limit(params.limit);
    let store = match ChatHistoryStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let thread = match store.default_thread(&state.scope).await {
        Ok(thread) => thread,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let result = if let Some(date) = params.date.as_deref() {
        match date_bounds_utc(date) {
            Ok((start, end)) => store.messages_for_date(&thread, start, end, limit + 1).await,
            Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
        }
    } else if let Some(before) = params.before {
        store.messages_before(&thread, before, limit + 1).await
    } else {
        store.latest_messages(&thread, limit + 1).await
    };

    match result {
        Ok(mut messages) => {
            let has_more = messages.len() as i64 > limit;
            if has_more {
                messages.remove(0);
            }
            Json(ChatMessagesResponse { messages, has_more }).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

- [ ] **Step 6: Persist messages from `/chat`**

Inside `chat`, after validating non-empty `q` and before spawning the worker thread, open store and record the user message:

```rust
let store = match ChatHistoryStore::open(&state.db_url).await {
    Ok(store) => store,
    Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
};
let thread = match store.default_thread(&state.scope).await {
    Ok(thread) => thread,
    Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
};
let now = now_unix();
if let Err(e) = store.insert_message(&thread, "user", &q, None, "complete", now).await {
    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
}
let db_url = state.db_url.clone();
```

Move `db_url` and `thread` into the worker thread. When the final answer is produced, compute `html` once and store both markdown and HTML:

```rust
let html = wukong_render::to_web_html(&out.text);
if let Ok(store) = ChatHistoryStore::open(&db_url).await {
    let _ = store.insert_message(&thread, "assistant", &out.text, Some(&html), "complete", now_unix()).await;
}
let _ = tx.send(SseMsg::Answer(html));
```

When `Err(e)` happens, store an error assistant message:

```rust
let msg = e.to_string();
if let Ok(store) = ChatHistoryStore::open(&db_url).await {
    let _ = store.insert_message(&thread, "assistant", &msg, None, "error", now_unix()).await;
}
let _ = tx.send(SseMsg::Error(msg));
```

Add a local helper in `lib.rs`:

```rust
fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}
```

- [ ] **Step 7: Add route**

In `build_router`, add:

```rust
.route("/api/chat/messages", axum::routing::get(get_chat_messages::<B>))
```

- [ ] **Step 8: Run web API tests**

Run:

```bash
cargo test -p wukong-web chat_messages -- --nocapture
cargo test -p wukong-web chat_streams_role_answer_done -- --nocapture
```

Expected: all named tests pass.

- [ ] **Step 9: Commit Task 2**

Run:

```bash
git add crates/wukong-web/src/main.rs crates/wukong-web/src/lib.rs
gitnexus_detect_changes --scope staged
git commit -m "feat(web): expose chat history API"
```

## Task 3: Add Schedule And System APIs

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`
- Create: `crates/wukong-web/src/schedule_api.rs`
- Create: `crates/wukong-web/src/system_api.rs`
- Test: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run impact analysis**

Run:

```text
gitnexus_impact({target: "build_router", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
```

- [ ] **Step 2: Add module declarations**

Add to `crates/wukong-web/src/lib.rs`:

```rust
pub mod schedule_api;
pub mod system_api;
```

- [ ] **Step 3: Create response shaping modules**

Create `crates/wukong-web/src/schedule_api.rs`:

```rust
use serde::Serialize;
use wukong_scheduler::{Job, JobKind, MaintenanceTask};

#[derive(Debug, Serialize)]
pub struct ScheduleJobResponse {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub scope: Option<String>,
    pub prompt: Option<String>,
    pub task: Option<String>,
    pub cron: String,
    pub enabled: bool,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
}

pub fn job_response(job: Job) -> ScheduleJobResponse {
    let (kind, scope, prompt, task) = match job.kind {
        JobKind::Turn { scope, prompt } => ("turn".to_string(), Some(scope), Some(prompt), None),
        JobKind::Maintenance { scope, task } => ("maintenance".to_string(), scope, None, Some(task_label(task).to_string())),
    };
    ScheduleJobResponse {
        id: job.id,
        name: job.name,
        kind,
        scope,
        prompt,
        task,
        cron: job.cron,
        enabled: job.enabled,
        next_run_at: job.next_run_at,
        last_run_at: job.last_run_at,
    }
}

fn task_label(task: MaintenanceTask) -> &'static str {
    match task {
        MaintenanceTask::Snapshot => "snapshot",
        MaintenanceTask::Consolidate => "consolidate",
        MaintenanceTask::Prune => "prune",
    }
}
```

Create `crates/wukong-web/src/system_api.rs`:

```rust
use serde::Serialize;
use wukong_scheduler::Job;

#[derive(Debug, Serialize)]
pub struct SystemResponse {
    pub scope: String,
    pub token_enabled: bool,
    pub memory_db: String,
    pub schedule_total: usize,
    pub schedule_enabled: usize,
    pub next_run_at: Option<i64>,
}

pub fn system_response(scope: &str, token_enabled: bool, db_url: &str, jobs: &[Job]) -> SystemResponse {
    SystemResponse {
        scope: scope.to_string(),
        token_enabled,
        memory_db: if db_url.trim().is_empty() { "unavailable".to_string() } else { "configured".to_string() },
        schedule_total: jobs.len(),
        schedule_enabled: jobs.iter().filter(|j| j.enabled).count(),
        next_run_at: jobs.iter().filter_map(|j| j.next_run_at).min(),
    }
}
```

- [ ] **Step 4: Add failing API tests**

Add tests to `crates/wukong-web/src/lib.rs` tests:

```rust
#[tokio::test]
async fn schedules_requires_token_when_set() {
    let app = build_router(state(Some("sekret"), &[]).await);
    let resp = app
        .oneshot(Request::builder().uri("/api/schedules").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn schedules_list_enable_disable_delete() {
    let app_state = state(None, &[]).await;
    let store = wukong_scheduler::SchedulerStore::open(&app_state.db_url).await.unwrap();
    let job = store.add_job(wukong_scheduler::NewJob {
        name: "morning".to_string(),
        kind: wukong_scheduler::JobKind::Turn { scope: "global".to_string(), prompt: "hi".to_string() },
        cron: "0 9 * * *".to_string(),
    }).await.unwrap();
    let app = build_router(app_state);

    let resp = app.clone().oneshot(Request::builder().uri("/api/schedules").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("morning"), "body: {body}");
    assert!(body.contains("turn"), "body: {body}");

    let resp = app.clone().oneshot(Request::builder().method("POST").uri(format!("/api/schedules/{}/disable", job.id)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(!store.get_job(&job.id).await.unwrap().unwrap().enabled);

    let resp = app.clone().oneshot(Request::builder().method("POST").uri(format!("/api/schedules/{}/enable", job.id)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(store.get_job(&job.id).await.unwrap().unwrap().enabled);

    let resp = app.oneshot(Request::builder().method("DELETE").uri(format!("/api/schedules/{}", job.id)).body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    assert!(store.get_job(&job.id).await.unwrap().is_none());
}

#[tokio::test]
async fn system_returns_summary() {
    let app_state = state(Some("sekret"), &[]).await;
    let store = wukong_scheduler::SchedulerStore::open(&app_state.db_url).await.unwrap();
    store.add_job(wukong_scheduler::NewJob {
        name: "prune".to_string(),
        kind: wukong_scheduler::JobKind::Maintenance { scope: Some("global".to_string()), task: wukong_scheduler::MaintenanceTask::Prune },
        cron: "0 3 * * *".to_string(),
    }).await.unwrap();
    let app = build_router(app_state);
    let resp = app
        .oneshot(Request::builder().uri("/api/system?token=sekret").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains(r#""scope":"global""#), "body: {body}");
    assert!(body.contains(r#""token_enabled":true"#), "body: {body}");
    assert!(body.contains(r#""schedule_total":1"#), "body: {body}");
}
```

- [ ] **Step 5: Run failing tests**

Run:

```bash
cargo test -p wukong-web schedules -- --nocapture
cargo test -p wukong-web system_returns_summary -- --nocapture
```

Expected: fail because routes/handlers are not present.

- [ ] **Step 6: Implement handlers and routes**

Add imports in `lib.rs`:

```rust
use axum::extract::Path;
use wukong_scheduler::SchedulerStore;
```

Add handlers:

```rust
async fn list_schedules<B>(State(state): State<AppState<B>>, Query(params): Query<SettingsQuery>) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let store = match SchedulerStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.list_jobs().await {
        Ok(jobs) => Json(jobs.into_iter().map(schedule_api::job_response).collect::<Vec<_>>()).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn set_schedule_enabled<B>(
    State(state): State<AppState<B>>,
    Path((id, action)): Path<(String, String)>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let enabled = match action.as_str() {
        "enable" => true,
        "disable" => false,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let store = match SchedulerStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.set_enabled(&id, enabled).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_schedule<B>(
    State(state): State<AppState<B>>,
    Path(id): Path<String>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let store = match SchedulerStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.remove_job(&id).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_system<B>(State(state): State<AppState<B>>, Query(params): Query<SettingsQuery>) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let store = match SchedulerStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.list_jobs().await {
        Ok(jobs) => Json(system_api::system_response(&state.scope, state.token.is_some(), &state.db_url, &jobs)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

Add routes in `build_router`:

```rust
.route("/api/schedules", axum::routing::get(list_schedules::<B>))
.route("/api/schedules/:id/:action", axum::routing::post(set_schedule_enabled::<B>))
.route("/api/schedules/:id", axum::routing::delete(delete_schedule::<B>))
.route("/api/system", axum::routing::get(get_system::<B>))
```

- [ ] **Step 7: Run API tests**

Run:

```bash
cargo test -p wukong-web schedules -- --nocapture
cargo test -p wukong-web system_returns_summary -- --nocapture
```

Expected: all named tests pass.

- [ ] **Step 8: Commit Task 3**

Run:

```bash
git add crates/wukong-web/src/lib.rs crates/wukong-web/src/schedule_api.rs crates/wukong-web/src/system_api.rs
gitnexus_detect_changes --scope staged
git commit -m "feat(web): add schedule and system APIs"
```

## Task 4: Add Hash Router And Static Assets

**Files:**
- Modify: `crates/wukong-web/static/index.html`
- Modify: `crates/wukong-web/static/app.js`
- Modify: `crates/wukong-web/src/lib.rs`
- Test: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run impact analysis**

Run:

```text
gitnexus_impact({target: "index", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
gitnexus_impact({target: "build_router", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
```

- [ ] **Step 2: Update shell markup**

Replace `crates/wukong-web/static/index.html` body with:

```html
<body>
  <header>
    <h1>🐵 悟空</h1>
    <nav>
      <a href="#/chat" data-route="chat">對話</a>
      <a href="#/settings/telegram" data-route="settings">設定</a>
    </nav>
  </header>
  <main id="app" class="app-shell"></main>
</body>
```

- [ ] **Step 3: Add router**

Replace `crates/wukong-web/static/app.js` with:

```js
import { WukongChat } from '/components/wukong-chat.js';
import { WukongSettings } from '/components/wukong-settings.js';
import { WukongSchedules } from '/components/wukong-schedules.js';
import { WukongSystem } from '/components/wukong-system.js';
import { html } from '/lib/html.js';

customElements.define('wukong-chat', WukongChat);
customElements.define('wukong-settings', WukongSettings);
customElements.define('wukong-schedules', WukongSchedules);
customElements.define('wukong-system', WukongSystem);

const app = document.querySelector('#app');

function settingsShell(active, tag) {
  return html`
    <section class="settings-layout">
      <nav class="settings-tabs">
        <a class="${active === 'telegram' ? 'active' : ''}" href="#/settings/telegram">Telegram</a>
        <a class="${active === 'system' ? 'active' : ''}" href="#/settings/system">系統</a>
        <a class="${active === 'schedules' ? 'active' : ''}" href="#/settings/schedules">排程</a>
      </nav>
      <div class="settings-outlet">${tag}</div>
    </section>
  `.toString();
}

function render() {
  const route = window.location.hash || '#/chat';
  if (route === '#/settings') {
    window.location.hash = '#/settings/telegram';
    return;
  }
  if (route === '#/chat') {
    app.innerHTML = '<wukong-chat></wukong-chat>';
  } else if (route === '#/settings/telegram') {
    app.innerHTML = settingsShell('telegram', '<wukong-settings></wukong-settings>');
  } else if (route === '#/settings/system') {
    app.innerHTML = settingsShell('system', '<wukong-system></wukong-system>');
  } else if (route === '#/settings/schedules') {
    app.innerHTML = settingsShell('schedules', '<wukong-schedules></wukong-schedules>');
  } else {
    app.innerHTML = '<section class="empty-state"><h2>找不到頁面</h2><p><a href="#/chat">回到對話</a></p></section>';
  }
  document.querySelectorAll('header nav a').forEach((a) => {
    const key = a.dataset.route;
    a.classList.toggle('active', route.includes(key));
  });
}

window.addEventListener('hashchange', render);
if (!window.location.hash) window.location.hash = '#/chat';
render();
```

- [ ] **Step 4: Add minimal route components so static imports resolve**

Create `crates/wukong-web/static/components/wukong-schedules.js`:

```js
import { html } from '/lib/html.js';

export class WukongSchedules extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`<section class="settings-card"><h2>排程</h2><p class="settings-help">載入中…</p></section>`.toString();
  }
}
```

Create `crates/wukong-web/static/components/wukong-system.js`:

```js
import { html } from '/lib/html.js';

export class WukongSystem extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`<section class="settings-card"><h2>系統</h2><p class="settings-help">載入中…</p></section>`.toString();
  }
}
```

- [ ] **Step 5: Serve new assets**

In `lib.rs`, add constants and handlers:

```rust
const SCHEDULES_JS: &str = include_str!("../static/components/wukong-schedules.js");
const SYSTEM_JS: &str = include_str!("../static/components/wukong-system.js");

async fn schedules_js() -> axum::response::Response { asset(JS, SCHEDULES_JS) }
async fn system_js() -> axum::response::Response { asset(JS, SYSTEM_JS) }
```

Add routes:

```rust
.route("/components/wukong-schedules.js", axum::routing::get(schedules_js))
.route("/components/wukong-system.js", axum::routing::get(system_js))
```

- [ ] **Step 6: Update shell tests**

Modify `index_serves_the_shell` assertion:

```rust
assert!(body.contains(r#"id="app""#));
assert!(body.contains(r##"href="#/chat""##));
```

Modify `settings_route_serves_the_shell` assertion:

```rust
assert!(body.contains(r#"id="app""#));
```

Extend `serves_static_assets_with_content_types`:

```rust
assert!(content_type(build_router(state(None, &[]).await), "/components/wukong-schedules.js")
    .await
    .contains("javascript"));
assert!(content_type(build_router(state(None, &[]).await), "/components/wukong-system.js")
    .await
    .contains("javascript"));
```

- [ ] **Step 7: Run static tests**

Run:

```bash
cargo test -p wukong-web serves_static_assets_with_content_types -- --nocapture
cargo test -p wukong-web index_serves_the_shell -- --nocapture
cargo test -p wukong-web settings_route_serves_the_shell -- --nocapture
```

Expected: tests pass.

- [ ] **Step 8: Commit Task 4**

Run:

```bash
git add crates/wukong-web/static/index.html crates/wukong-web/static/app.js crates/wukong-web/static/components/wukong-schedules.js crates/wukong-web/static/components/wukong-system.js crates/wukong-web/src/lib.rs
gitnexus_detect_changes --scope staged
git commit -m "feat(web): add routed app shell"
```

## Task 5: Implement Chat History Frontend

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
- Modify: `crates/wukong-web/static/styles.css`

- [ ] **Step 1: Run impact analysis**

Run:

```text
gitnexus_impact({target: "WukongChat", direction: "upstream", file_path: "crates/wukong-web/static/components/wukong-chat.js", repo: "Wukong"})
```

- [ ] **Step 2: Replace chat component markup and add state**

In `connectedCallback`, use:

```js
this.innerHTML = html`
  <div class="chat-toolbar">
    <label>跳到日期 <input id="jump-date" type="date" /></label>
    <button id="jump-button" type="button">前往</button>
  </div>
  <div class="log" id="log"></div>
  <form id="form" class="composer">
    <input id="q" type="text" autocomplete="off" placeholder="問悟空…" />
    <button type="submit">送出</button>
  </form>
`.toString();
this.loadingOlder = false;
this.hasMore = false;
this.oldestId = null;
```

Bind date and scroll listeners:

```js
this.querySelector('#jump-button').addEventListener('click', () => this.jumpToDate());
this.log.addEventListener('scroll', () => {
  if (this.log.scrollTop < 80) this.loadOlder();
});
this.loadLatest();
```

- [ ] **Step 3: Add API helpers and rendering**

Add methods to `WukongChat`:

```js
tokenParam(prefix = '?') {
  return window.WUKONG_TOKEN ? prefix + 'token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
}

async fetchMessages(params = '') {
  const token = this.tokenParam(params ? '&' : '?');
  const resp = await fetch('/api/chat/messages' + (params ? '?' + params : '') + token);
  if (!resp.ok) throw new Error('HTTP ' + resp.status);
  return resp.json();
}

messageNode(message) {
  const div = document.createElement('div');
  div.className = 'bubble ' + (message.role === 'user' ? 'user' : 'assistant');
  div.dataset.messageId = message.id;
  if (message.role === 'assistant' && message.content_html) {
    div.innerHTML = message.content_html;
  } else {
    div.textContent = message.content;
  }
  if (message.status === 'error') div.classList.add('error');
  return div;
}

renderMessages(messages, mode) {
  const nodes = [];
  let lastDate = null;
  for (const message of messages) {
    const date = new Date(message.created_at * 1000).toLocaleDateString('zh-TW', {
      year: 'numeric', month: 'long', day: 'numeric'
    });
    if (date !== lastDate) {
      const sep = document.createElement('div');
      sep.className = 'date-separator';
      sep.textContent = date;
      nodes.push(sep);
      lastDate = date;
    }
    nodes.push(this.messageNode(message));
  }
  if (mode === 'prepend') {
    const previousHeight = this.log.scrollHeight;
    for (const node of nodes.reverse()) this.log.prepend(node);
    this.log.scrollTop = this.log.scrollHeight - previousHeight;
  } else {
    this.log.innerHTML = '';
    for (const node of nodes) this.log.appendChild(node);
    this.log.scrollTop = this.log.scrollHeight;
  }
  this.oldestId = this.log.querySelector('[data-message-id]')?.dataset.messageId || null;
}
```

- [ ] **Step 4: Add load methods**

```js
async loadLatest() {
  try {
    const data = await this.fetchMessages('limit=10');
    if (!data.messages.length) {
      this.log.innerHTML = '<p class="empty-state">還沒有對話，問悟空第一個問題。</p>';
      return;
    }
    this.hasMore = data.has_more;
    this.renderMessages(data.messages, 'replace');
  } catch (err) {
    this.log.innerHTML = '<p class="empty-state">無法讀取對話歷史：' + escapeHTML(err.message) + '</p>';
  }
}

async loadOlder() {
  if (this.loadingOlder || !this.hasMore || !this.oldestId) return;
  this.loadingOlder = true;
  try {
    const data = await this.fetchMessages('before=' + encodeURIComponent(this.oldestId) + '&limit=10');
    this.hasMore = data.has_more;
    this.renderMessages(data.messages, 'prepend');
  } catch (err) {
    const note = document.createElement('p');
    note.className = 'load-error';
    note.textContent = '載入較舊訊息失敗，請重試。';
    this.log.prepend(note);
  } finally {
    this.loadingOlder = false;
  }
}

async jumpToDate() {
  const date = this.querySelector('#jump-date').value;
  if (!date) return;
  try {
    const data = await this.fetchMessages('date=' + encodeURIComponent(date) + '&limit=10');
    this.hasMore = data.has_more;
    if (!data.messages.length) {
      this.log.innerHTML = '<p class="empty-state">這天沒有對話。</p>';
      return;
    }
    this.renderMessages(data.messages, 'replace');
  } catch (err) {
    this.log.innerHTML = '<p class="empty-state">無法跳到指定日期：' + escapeHTML(err.message) + '</p>';
  }
}
```

- [ ] **Step 5: Avoid duplicate final assistant bubble after send**

Keep current `send()` SSE behavior for instant feedback. After `done`, do not call `loadLatest()` automatically; stored history is for reload/navigation. Ensure the user bubble still appends immediately with `this.bubble('user', ...)`.

- [ ] **Step 6: Add CSS**

Append to `styles.css`:

```css
.app-shell { flex: 1; min-height: 0; display: flex; flex-direction: column; }
.chat-toolbar { display: flex; align-items: center; gap: 0.5rem; padding: 0.5rem 0.75rem; border-bottom: 1px solid #8884; }
.chat-toolbar label { display: flex; align-items: center; gap: 0.4rem; font-size: 0.9rem; opacity: 0.85; }
.chat-toolbar input, .chat-toolbar button { font: inherit; }
.date-separator { align-self: center; font-size: 0.8rem; opacity: 0.7; padding: 0.2rem 0.6rem; border-radius: 999px; background: #8882; }
.empty-state, .load-error { align-self: center; opacity: 0.75; margin: 1rem; }
.bubble.error { border: 1px solid #d33; }
.settings-layout { flex: 1; min-height: 0; display: flex; flex-direction: column; }
.settings-tabs { padding: 0.75rem 1rem; border-bottom: 1px solid #8884; }
.settings-tabs a.active, header nav a.active { opacity: 1; text-decoration: underline; }
.settings-outlet { flex: 1; overflow: auto; }
```

- [ ] **Step 7: Manual browser verification**

Run:

```bash
cargo run -p wukong-web
```

Open `http://127.0.0.1:8787/#/chat` and verify latest load, send, reload, upward scroll, and date jump.

- [ ] **Step 8: Commit Task 5**

Run:

```bash
git add crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/static/styles.css
gitnexus_detect_changes --scope staged
git commit -m "feat(web): load chat history in UI"
```

## Task 6: Implement Schedule And System Frontend

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-schedules.js`
- Modify: `crates/wukong-web/static/components/wukong-system.js`
- Modify: `crates/wukong-web/static/styles.css`

- [ ] **Step 1: Run impact analysis**

Run:

```text
gitnexus_impact({target: "WukongSchedules", direction: "upstream", file_path: "crates/wukong-web/static/components/wukong-schedules.js", repo: "Wukong"})
gitnexus_impact({target: "WukongSystem", direction: "upstream", file_path: "crates/wukong-web/static/components/wukong-system.js", repo: "Wukong"})
```

- [ ] **Step 2: Implement schedules component**

Replace `wukong-schedules.js` with:

```js
import { html, escapeHTML } from '/lib/html.js';

export class WukongSchedules extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`<section class="settings-card"><h2>排程</h2><div id="schedules-list">載入中…</div></section>`.toString();
    this.list = this.querySelector('#schedules-list');
    this.load();
  }

  tokenParam(prefix = '?') {
    return window.WUKONG_TOKEN ? prefix + 'token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/schedules' + this.tokenParam());
    if (!resp.ok) {
      this.list.textContent = resp.status === 401 ? '沒有權限讀取資料。' : '無法讀取排程：HTTP ' + resp.status;
      return;
    }
    const jobs = await resp.json();
    if (!jobs.length) {
      this.list.innerHTML = '<p class="empty-state">目前沒有排程，可先用 CLI 建立排程。</p>';
      return;
    }
    this.list.innerHTML = jobs.map((job) => this.card(job)).join('');
    this.list.querySelectorAll('[data-action]').forEach((button) => {
      button.addEventListener('click', () => this.act(button.dataset.id, button.dataset.action));
    });
  }

  card(job) {
    const next = job.next_run_at ? new Date(job.next_run_at * 1000).toLocaleString('zh-TW') : '未排定';
    const last = job.last_run_at ? new Date(job.last_run_at * 1000).toLocaleString('zh-TW') : '尚未執行';
    const detail = job.kind === 'turn' ? job.prompt : job.task;
    const toggle = job.enabled ? 'disable' : 'enable';
    const toggleLabel = job.enabled ? '停用' : '啟用';
    return `
      <article class="schedule-card">
        <h3>${escapeHTML(job.name)}</h3>
        <p>類型：${escapeHTML(job.kind)} / ${escapeHTML(job.scope || 'global')}</p>
        <p>內容：${escapeHTML(detail || '')}</p>
        <p>Cron：<code>${escapeHTML(job.cron)}</code></p>
        <p>狀態：${job.enabled ? '啟用' : '停用'}</p>
        <p>下次：${escapeHTML(next)}</p>
        <p>上次：${escapeHTML(last)}</p>
        <div class="schedule-actions">
          <button data-id="${escapeHTML(job.id)}" data-action="${toggle}">${toggleLabel}</button>
          <button data-id="${escapeHTML(job.id)}" data-action="delete">刪除</button>
        </div>
      </article>
    `;
  }

  async act(id, action) {
    if (action === 'delete' && !confirm('確定要刪除這個排程？')) return;
    const method = action === 'delete' ? 'DELETE' : 'POST';
    const path = action === 'delete' ? '/api/schedules/' + encodeURIComponent(id) : '/api/schedules/' + encodeURIComponent(id) + '/' + action;
    const resp = await fetch(path + this.tokenParam(), { method });
    if (!resp.ok) {
      alert(resp.status === 404 ? '找不到排程。' : '操作失敗：HTTP ' + resp.status);
      return;
    }
    await this.load();
  }
}
```

- [ ] **Step 3: Implement system component**

Replace `wukong-system.js` with:

```js
import { html } from '/lib/html.js';

export class WukongSystem extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`<section class="settings-card"><h2>系統</h2><div id="system-summary">載入中…</div></section>`.toString();
    this.summary = this.querySelector('#system-summary');
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/system' + this.tokenParam());
    if (!resp.ok) {
      this.summary.textContent = resp.status === 401 ? '沒有權限讀取資料。' : '無法讀取系統資訊：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    const next = data.next_run_at ? new Date(data.next_run_at * 1000).toLocaleString('zh-TW') : '未排定';
    this.summary.innerHTML = html`
      <dl class="system-list">
        <dt>Scope</dt><dd>${data.scope}</dd>
        <dt>Web token</dt><dd>${data.token_enabled ? '已啟用' : '未啟用'}</dd>
        <dt>Memory DB</dt><dd>${data.memory_db}</dd>
        <dt>排程總數</dt><dd>${data.schedule_total}</dd>
        <dt>啟用排程</dt><dd>${data.schedule_enabled}</dd>
        <dt>最近下次執行</dt><dd>${next}</dd>
      </dl>
    `.toString();
  }
}
```

- [ ] **Step 4: Add CSS**

Append:

```css
.schedule-card { border: 1px solid #8884; border-radius: 0.75rem; padding: 0.75rem; margin: 0.75rem 0; }
.schedule-card h3 { margin: 0 0 0.5rem; }
.schedule-card p { margin: 0.25rem 0; }
.schedule-actions { display: flex; gap: 0.5rem; margin-top: 0.75rem; }
.system-list { display: grid; grid-template-columns: max-content 1fr; gap: 0.5rem 1rem; }
.system-list dt { font-weight: 700; }
.system-list dd { margin: 0; }
```

- [ ] **Step 5: Manual browser verification**

Run:

```bash
cargo run -p wukong-web
```

Open `#/settings/system` and `#/settings/schedules`. Verify system summary renders, empty schedule state renders, and schedule controls work when jobs exist.

- [ ] **Step 6: Commit Task 6**

Run:

```bash
git add crates/wukong-web/static/components/wukong-schedules.js crates/wukong-web/static/components/wukong-system.js crates/wukong-web/static/styles.css
gitnexus_detect_changes --scope staged
git commit -m "feat(web): add schedule and system settings UI"
```

## Task 7: Final Verification

**Files:**
- Verify all modified files.

- [ ] **Step 1: Run full web crate tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: all `wukong-web` tests pass.

- [ ] **Step 2: Run scheduler tests for regression safety**

Run:

```bash
cargo test -p wukong-scheduler
```

Expected: all `wukong-scheduler` tests pass.

- [ ] **Step 3: Run final change detection**

Run:

```text
gitnexus_detect_changes({scope: "all", repo: "Wukong"})
```

Expected: changed symbols match web history, schedule API, and route UI changes. Investigate any unrelated processes.

- [ ] **Step 4: Manual route checklist**

Run:

```bash
cargo run -p wukong-web
```

Verify:

- `http://127.0.0.1:8787/#/chat` loads.
- Latest history appears after reload.
- Upward scroll requests older messages.
- Date jump shows selected date or empty-date state.
- `#/settings/telegram` preserves existing settings behavior.
- `#/settings/system` renders summary.
- `#/settings/schedules` renders list or empty state.
- Schedule enable, disable, and delete work.

- [ ] **Step 5: Commit any remaining verification fixes**

If final verification required code fixes, stage only those files and run:

```bash
gitnexus_detect_changes --scope staged
git commit -m "fix(web): polish history and schedule settings"
```

If no fixes were needed, do not create an empty commit.
