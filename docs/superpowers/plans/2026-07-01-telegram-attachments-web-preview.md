# Telegram Attachments Web Preview Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Telegram-originated file attachments, store them under the Wukong workspace, show small previews/download links in Web Console, and pass local paths to opencode when supported.

**Architecture:** Use Wukong chat identity (`scope`, `thread_id`, `message_id`) as the attachment relationship, not opencode session ids. Store files on disk under `.wukong/uploads`, store metadata in SQLite via `wukong-chat-history`, and pass resolved local paths through `AgentRequest.attachments` to the gateway. Web Console reads attachment metadata from chat history APIs and serves token-protected downloads/previews.

**Tech Stack:** Rust, Tokio, Axum, SQLx/SQLite, Reqwest, Telegram Bot API, opencode CLI `--file`, vanilla JS Web Components.

---

## Scope And File Structure

This plan implements the first release only:

- Telegram document and photo ingestion.
- Workspace-backed attachment storage.
- Chat-history attachment metadata.
- Web Console attachment cards, image preview, and download.
- CLI backend support via opencode `--file`.
- Server backend explicit unsupported error for attachment turns.

This plan does not implement Web-originated uploads, inline PDF rendering, OCR, persistent thumbnails, cleanup UI, or shared public URLs.

Files to create:

- `crates/wukong-chat-history/src/attachments.rs`: attachment metadata structs, filename/scope sanitization, upload-root path helpers.

Files to modify:

- `crates/wukong-chat-history/src/lib.rs`: re-export attachment types, create `chat_attachments`, insert/list/get attachment metadata.
- `crates/wukong-gateway/src/backend.rs`: add `AgentAttachment`, add `AgentRequest.attachments`, append `--file` args in CLI argv.
- `crates/wukong-gateway/src/opencode_server.rs`: reject attachment turns with a clear unsupported error until server file-part support is implemented.
- `crates/wukong-runtime/src/turn.rs`: add attachment-aware turn entry point while preserving existing text-only wrappers.
- `crates/wukong-cli/src/lib.rs`: expose attachment-aware wrappers for Web/Telegram callers.
- `crates/wukong-tg-client/src/parse.rs`: parse Telegram `document`, `photo`, and `caption` into `TgAttachment`.
- `crates/wukong-tg-client/src/client.rs`: add Telegram `getFile` metadata and file download methods; update mock client.
- `crates/wukong-telegram/src/dispatch.rs`: download Telegram files, store metadata, pass attachments to `run_turn`.
- `crates/wukong-web/src/lib.rs`: include attachments in message responses and add download/preview endpoints.
- `crates/wukong-web/static/components/wukong-chat.js`: render attachment cards and image previews.
- `crates/wukong-web/static/styles.css`: style attachment cards and thumbnails.

Before editing any function, method, struct, or class, run GitNexus impact analysis for that symbol as required by `AGENTS.md`. If risk is HIGH or CRITICAL, stop and report before editing.

---

### Task 1: Chat History Attachment Storage

**Files:**

- Create: `crates/wukong-chat-history/src/attachments.rs`
- Modify: `crates/wukong-chat-history/src/lib.rs`
- Test: `crates/wukong-chat-history/src/lib.rs`

- [ ] **Step 1: Run impact analysis before editing `ChatHistoryStore`**

Run:

```bash
gitnexus_impact --target ChatHistoryStore --direction upstream --repo Wukong
```

Expected: report risk, direct callers, and affected processes. If the tool reports HIGH or CRITICAL risk, stop and report before editing.

- [ ] **Step 2: Add failing tests for attachment metadata and sanitization**

In `crates/wukong-chat-history/src/lib.rs`, add these tests inside `mod tests`:

```rust
#[tokio::test]
async fn attachments_round_trip_for_message() {
    let store = store().await;
    let thread = store.default_thread("user:tg-42").await.unwrap();
    let message_id = store
        .insert_message(&thread, "user", "請看附件", None, "complete", 100)
        .await
        .unwrap();

    let new = NewChatAttachment {
        message_id,
        scope: "user:tg-42".to_string(),
        source: "telegram".to_string(),
        original_name: "report.pdf".to_string(),
        stored_name: "report.pdf".to_string(),
        relative_path: "user_tg-42/1/report.pdf".to_string(),
        mime_type: Some("application/pdf".to_string()),
        size_bytes: 1234,
        sha256: Some("abc123".to_string()),
        telegram_file_id: Some("file_1".to_string()),
        created_at: 101,
    };

    let id = store.insert_attachment(&new).await.unwrap();
    let attachments = store.attachments_for_messages(&[message_id]).await.unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].id, id);
    assert_eq!(attachments[0].message_id, message_id);
    assert_eq!(attachments[0].original_name, "report.pdf");
    assert_eq!(attachments[0].mime_type.as_deref(), Some("application/pdf"));

    let loaded = store.attachment(id).await.unwrap().unwrap();
    assert_eq!(loaded.relative_path, "user_tg-42/1/report.pdf");
}

#[test]
fn attachment_path_parts_are_sanitized() {
    assert_eq!(sanitize_scope("user:tg-42"), "user_tg-42");
    assert_eq!(sanitize_filename("../report final.pdf"), "report final.pdf");
    assert_eq!(sanitize_filename(""), "attachment");
    assert_eq!(sanitize_filename(".."), "attachment");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test -p wukong-chat-history attachments_round_trip_for_message attachment_path_parts_are_sanitized
```

Expected: FAIL because `NewChatAttachment`, `insert_attachment`, `attachments_for_messages`, `attachment`, `sanitize_scope`, and `sanitize_filename` do not exist yet.

- [ ] **Step 4: Create `attachments.rs` with metadata and path helpers**

Create `crates/wukong-chat-history/src/attachments.rs`:

```rust
use serde::Serialize;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatAttachment {
    pub id: i64,
    pub message_id: i64,
    pub scope: String,
    pub source: String,
    pub original_name: String,
    pub stored_name: String,
    pub relative_path: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub telegram_file_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChatAttachment {
    pub message_id: i64,
    pub scope: String,
    pub source: String,
    pub original_name: String,
    pub stored_name: String,
    pub relative_path: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub telegram_file_id: Option<String>,
    pub created_at: i64,
}

pub fn sanitize_scope(scope: &str) -> String {
    let out: String = scope
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if out.trim_matches('_').is_empty() { "scope".to_string() } else { out }
}

pub fn sanitize_filename(name: &str) -> String {
    let file_name = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .trim();
    let cleaned: String = file_name
        .chars()
        .map(|c| if c == '/' || c == '\\' || c.is_control() { '_' } else { c })
        .collect();
    let cleaned = cleaned.trim_matches('.').trim();
    if cleaned.is_empty() { "attachment".to_string() } else { cleaned.to_string() }
}

pub fn relative_attachment_path(scope: &str, message_id: i64, filename: &str) -> String {
    format!("{}/{}/{}", sanitize_scope(scope), message_id, sanitize_filename(filename))
}

pub fn resolve_under_upload_root(root: &Path, relative_path: &str) -> Option<PathBuf> {
    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return None;
    }
    if rel.components().any(|c| matches!(c, Component::ParentDir | Component::RootDir | Component::Prefix(_))) {
        return None;
    }
    Some(root.join(rel))
}
```

- [ ] **Step 5: Wire attachment table and store methods**

In `crates/wukong-chat-history/src/lib.rs`, add at the top:

```rust
mod attachments;

pub use attachments::{
    relative_attachment_path, resolve_under_upload_root, sanitize_filename, sanitize_scope,
    ChatAttachment, NewChatAttachment,
};
```

Inside `ChatHistoryStore::open`, after `chat_messages` indexes and before `turn_steps`, add:

```rust
sqlx::query(
    "CREATE TABLE IF NOT EXISTS chat_attachments (
        id INTEGER PRIMARY KEY AUTOINCREMENT,
        message_id INTEGER NOT NULL,
        scope TEXT NOT NULL,
        source TEXT NOT NULL,
        original_name TEXT NOT NULL,
        stored_name TEXT NOT NULL,
        relative_path TEXT NOT NULL,
        mime_type TEXT,
        size_bytes INTEGER NOT NULL,
        sha256 TEXT,
        telegram_file_id TEXT,
        created_at INTEGER NOT NULL,
        FOREIGN KEY(message_id) REFERENCES chat_messages(id) ON DELETE CASCADE
    )",
)
.execute(&pool)
.await?;
sqlx::query(
    "CREATE INDEX IF NOT EXISTS chat_attachments_message_id_idx
     ON chat_attachments(message_id)",
)
.execute(&pool)
.await?;
sqlx::query(
    "CREATE INDEX IF NOT EXISTS chat_attachments_scope_id_idx
     ON chat_attachments(scope, id)",
)
.execute(&pool)
.await?;
```

Add these methods to `impl ChatHistoryStore`:

```rust
pub async fn insert_attachment(&self, a: &NewChatAttachment) -> Result<i64, sqlx::Error> {
    let row = sqlx::query(
        "INSERT INTO chat_attachments
         (message_id, scope, source, original_name, stored_name, relative_path, mime_type, size_bytes, sha256, telegram_file_id, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
         RETURNING id",
    )
    .bind(a.message_id)
    .bind(&a.scope)
    .bind(&a.source)
    .bind(&a.original_name)
    .bind(&a.stored_name)
    .bind(&a.relative_path)
    .bind(&a.mime_type)
    .bind(a.size_bytes)
    .bind(&a.sha256)
    .bind(&a.telegram_file_id)
    .bind(a.created_at)
    .fetch_one(&self.pool)
    .await?;
    Ok(row.get("id"))
}

pub async fn attachments_for_messages(&self, message_ids: &[i64]) -> Result<Vec<ChatAttachment>, sqlx::Error> {
    if message_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = std::iter::repeat_n("?", message_ids.len()).collect::<Vec<_>>().join(",");
    let sql = format!(
        "SELECT id, message_id, scope, source, original_name, stored_name, relative_path, mime_type, size_bytes, sha256, telegram_file_id, created_at
         FROM chat_attachments WHERE message_id IN ({placeholders}) ORDER BY id ASC"
    );
    let mut query = sqlx::query(&sql);
    for id in message_ids {
        query = query.bind(id);
    }
    let rows = query.fetch_all(&self.pool).await?;
    Ok(rows.into_iter().map(row_to_attachment).collect())
}

pub async fn attachment(&self, id: i64) -> Result<Option<ChatAttachment>, sqlx::Error> {
    let row = sqlx::query(
        "SELECT id, message_id, scope, source, original_name, stored_name, relative_path, mime_type, size_bytes, sha256, telegram_file_id, created_at
         FROM chat_attachments WHERE id = ?1",
    )
    .bind(id)
    .fetch_optional(&self.pool)
    .await?;
    Ok(row.map(row_to_attachment))
}
```

Add this helper near the existing `row_to_*` functions:

```rust
fn row_to_attachment(row: sqlx::sqlite::SqliteRow) -> ChatAttachment {
    ChatAttachment {
        id: row.get("id"),
        message_id: row.get("message_id"),
        scope: row.get("scope"),
        source: row.get("source"),
        original_name: row.get("original_name"),
        stored_name: row.get("stored_name"),
        relative_path: row.get("relative_path"),
        mime_type: row.get("mime_type"),
        size_bytes: row.get("size_bytes"),
        sha256: row.get("sha256"),
        telegram_file_id: row.get("telegram_file_id"),
        created_at: row.get("created_at"),
    }
}
```

- [ ] **Step 6: Run tests and format**

Run:

```bash
cargo fmt
cargo test -p wukong-chat-history attachments_round_trip_for_message attachment_path_parts_are_sanitized
cargo test -p wukong-chat-history
```

Expected: all PASS.

- [ ] **Step 7: Commit Task 1**

Run:

```bash
git add crates/wukong-chat-history/src/lib.rs crates/wukong-chat-history/src/attachments.rs
git commit -m "feat: store chat attachments"
```

---

### Task 2: Gateway Attachment Requests

**Files:**

- Modify: `crates/wukong-gateway/src/backend.rs`
- Modify: `crates/wukong-gateway/src/opencode_server.rs`
- Test: existing tests in those files

- [ ] **Step 1: Run impact analysis before editing gateway symbols**

Run:

```bash
gitnexus_impact --target AgentRequest --direction upstream --repo Wukong
gitnexus_impact --target assemble_argv --direction upstream --repo Wukong
gitnexus_impact --target OpencodeServerBackend --direction upstream --repo Wukong
```

Expected: report blast radius. Stop if any risk is HIGH or CRITICAL.

- [ ] **Step 2: Add failing gateway tests**

In `crates/wukong-gateway/src/backend.rs`, add a test near the existing `assemble_argv_*` tests:

```rust
#[test]
fn assemble_argv_adds_files_before_prompt() {
    let files = vec![
        AgentAttachment {
            path: std::path::PathBuf::from("/tmp/report.pdf"),
            original_name: "report.pdf".to_string(),
            mime_type: Some("application/pdf".to_string()),
        },
        AgentAttachment {
            path: std::path::PathBuf::from("/tmp/photo.jpg"),
            original_name: "photo.jpg".to_string(),
            mime_type: Some("image/jpeg".to_string()),
        },
    ];
    let argv = assemble_argv(
        &["opencode".to_string(), "run".to_string()],
        None,
        false,
        None,
        &files,
        "describe",
    );
    assert_eq!(
        argv,
        vec![
            "opencode",
            "run",
            "--file",
            "/tmp/report.pdf",
            "--file",
            "/tmp/photo.jpg",
            "describe"
        ]
    );
}
```

In `crates/wukong-gateway/src/opencode_server.rs`, add a test near server backend tests:

```rust
#[tokio::test]
async fn server_backend_rejects_attachments_explicitly() {
    let backend = OpencodeServerBackend::from_env("http://127.0.0.1:1".to_string(), None);
    let err = backend
        .run_streaming(
            AgentRequest {
                prompt: "describe".to_string(),
                session_id: None,
                thinking: false,
                model: None,
                attachments: vec![AgentAttachment {
                    path: std::path::PathBuf::from("/tmp/report.pdf"),
                    original_name: "report.pdf".to_string(),
                    mime_type: Some("application/pdf".to_string()),
                }],
            },
            &mut |_| {},
        )
        .await
        .unwrap_err();
    assert!(err.to_string().contains("不支援附件輸入"), "{err}");
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test -p wukong-gateway assemble_argv_adds_files_before_prompt server_backend_rejects_attachments_explicitly
```

Expected: FAIL because `AgentAttachment`, `AgentRequest.attachments`, and the new `assemble_argv` signature do not exist.

- [ ] **Step 4: Add `AgentAttachment` and extend `AgentRequest`**

In `crates/wukong-gateway/src/backend.rs`, above `AgentRequest`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentAttachment {
    pub path: PathBuf,
    pub original_name: String,
    pub mime_type: Option<String>,
}
```

Add to `AgentRequest`:

```rust
pub attachments: Vec<AgentAttachment>,
```

Update every `AgentRequest { ... }` construction in the workspace to include:

```rust
attachments: Vec::new(),
```

- [ ] **Step 5: Update argv assembly**

Change `assemble_argv` signature in `crates/wukong-gateway/src/backend.rs`:

```rust
pub fn assemble_argv(
    command: &[String],
    session_id: Option<&str>,
    thinking: bool,
    model: Option<&str>,
    attachments: &[AgentAttachment],
    prompt: &str,
) -> Vec<String>
```

Before `argv.push(prompt.to_string());`, insert:

```rust
for attachment in attachments {
    argv.push("--file".to_string());
    argv.push(attachment.path.to_string_lossy().to_string());
}
```

Update all call sites to pass `&req.attachments` or `&[]`.

- [ ] **Step 6: Reject attachment turns in server backend**

In `crates/wukong-gateway/src/opencode_server.rs`, import `AgentAttachment` where `AgentRequest` is imported.

At the start of both `impl AiBackend for OpencodeServerBackend::run` and `run_streaming`, before health/session calls, add:

```rust
if !req.attachments.is_empty() {
    return Err(attachments_unsupported());
}
```

Add helper:

```rust
fn attachments_unsupported() -> GatewayError {
    GatewayError::AgentFailed {
        code: None,
        stderr: "目前的 opencode server backend 不支援附件輸入；請改用 CLI backend 或等待 server file parts 支援。".to_string(),
    }
}
```

- [ ] **Step 7: Run gateway tests**

Run:

```bash
cargo fmt
cargo test -p wukong-gateway assemble_argv_adds_files_before_prompt server_backend_rejects_attachments_explicitly
cargo test -p wukong-gateway
```

Expected: all PASS.

- [ ] **Step 8: Commit Task 2**

Run:

```bash
git add crates/wukong-gateway/src/backend.rs crates/wukong-gateway/src/opencode_server.rs
git commit -m "feat: pass attachments to gateway backends"
```

---

### Task 3: Attachment-Aware Turn Entrypoints

**Files:**

- Modify: `crates/wukong-runtime/src/turn.rs`
- Modify: `crates/wukong-cli/src/lib.rs`
- Test: existing runtime and CLI tests

- [ ] **Step 1: Run impact analysis**

Run:

```bash
gitnexus_impact --target run_turn --direction upstream --repo Wukong
gitnexus_impact --target run_turn_traced --direction upstream --repo Wukong
```

Expected: report blast radius. Stop if HIGH or CRITICAL.

- [ ] **Step 2: Add failing attachment-aware runtime test**

In `crates/wukong-runtime/src/turn.rs`, add a test that uses the existing mock backend pattern and asserts the request includes one attachment:

```rust
#[tokio::test]
async fn run_turn_with_attachments_forwards_paths_to_backend() {
    let mem = memory().await;
    let backend = RecordingBackend::default();
    let cfg = config();
    let attachments = vec![AgentAttachment {
        path: std::path::PathBuf::from("/tmp/report.pdf"),
        original_name: "report.pdf".to_string(),
        mime_type: Some("application/pdf".to_string()),
    }];

    let _ = run_turn_with_attachments(
        &mem,
        &backend,
        &cfg,
        "請分析",
        attachments.clone(),
        &mut |_| {},
    )
    .await
    .unwrap();

    let requests = backend.requests.lock().unwrap();
    assert_eq!(requests.last().unwrap().attachments, attachments);
}
```

If the existing test mock has a different name, adapt only the mock name and keep this assertion shape.

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
cargo test -p wukong-runtime run_turn_with_attachments_forwards_paths_to_backend
```

Expected: FAIL because `run_turn_with_attachments` does not exist.

- [ ] **Step 4: Add attachment-aware wrappers while preserving existing API**

In `crates/wukong-runtime/src/turn.rs`, keep `run_turn` and `run_turn_traced` signatures unchanged. Add new functions:

```rust
pub async fn run_turn_with_attachments<B: AiBackend>(
    mem: &Memory,
    backend: &B,
    cfg: &GatewayConfig,
    input: &str,
    attachments: Vec<AgentAttachment>,
    on_event: &mut dyn FnMut(StreamEvent),
) -> Result<String, RuntimeError> {
    run_turn_inner(mem, backend, cfg, input, attachments, on_event, None, None).await
}

pub async fn run_turn_traced_with_attachments<B: AiBackend>(
    mem: &Memory,
    backend: &B,
    cfg: &GatewayConfig,
    input: &str,
    attachments: Vec<AgentAttachment>,
    on_event: &mut dyn FnMut(StreamEvent),
    on_role: &mut dyn FnMut(Role),
    on_step: &mut dyn FnMut(ObservedStep),
) -> Result<String, RuntimeError> {
    run_turn_inner(
        mem,
        backend,
        cfg,
        input,
        attachments,
        on_event,
        Some(on_role),
        Some(on_step),
    )
    .await
}
```

Refactor the existing shared implementation so every `AgentRequest` includes:

```rust
attachments: attachments.clone(),
```

The existing text-only wrappers should call the new implementation with `Vec::new()`.

- [ ] **Step 5: Re-export new wrappers from CLI crate**

In `crates/wukong-cli/src/lib.rs`, export the new functions next to existing `run_turn` exports:

```rust
pub use wukong_runtime::turn::{run_turn_with_attachments, run_turn_traced_with_attachments};
```

- [ ] **Step 6: Run tests**

Run:

```bash
cargo fmt
cargo test -p wukong-runtime run_turn_with_attachments_forwards_paths_to_backend
cargo test -p wukong-runtime -p wukong-cli
```

Expected: all PASS.

- [ ] **Step 7: Commit Task 3**

Run:

```bash
git add crates/wukong-runtime/src/turn.rs crates/wukong-cli/src/lib.rs
git commit -m "feat: add attachment-aware turns"
```

---

### Task 4: Telegram Parsing And Download Client

**Files:**

- Modify: `crates/wukong-tg-client/src/parse.rs`
- Modify: `crates/wukong-tg-client/src/client.rs`
- Test: tests in those files

- [ ] **Step 1: Run impact analysis**

Run:

```bash
gitnexus_impact --target TgMessage --direction upstream --repo Wukong
gitnexus_impact --target parse_updates --direction upstream --repo Wukong
gitnexus_impact --target TgClient --direction upstream --repo Wukong
```

Expected: report blast radius. Stop if HIGH or CRITICAL.

- [ ] **Step 2: Add failing parser tests**

In `crates/wukong-tg-client/src/parse.rs`, add:

```rust
#[test]
fn parse_updates_extracts_document_with_caption() {
    let json = serde_json::json!({
        "result": [{
            "update_id": 10,
            "message": {
                "chat": {"id": 12},
                "caption": "請分析",
                "document": {
                    "file_id": "doc_file",
                    "file_unique_id": "doc_unique",
                    "file_name": "report.pdf",
                    "mime_type": "application/pdf",
                    "file_size": 1234
                }
            }
        }]
    });
    let msgs = parse_updates(&json);
    assert_eq!(msgs.len(), 1);
    assert_eq!(msgs[0].text, "請分析");
    assert_eq!(msgs[0].attachments.len(), 1);
    assert_eq!(msgs[0].attachments[0].file_id, "doc_file");
    assert_eq!(msgs[0].attachments[0].original_name, "report.pdf");
}

#[test]
fn parse_updates_extracts_largest_photo_with_fallback_prompt() {
    let json = serde_json::json!({
        "result": [{
            "update_id": 11,
            "message": {
                "chat": {"id": 12},
                "photo": [
                    {"file_id": "small", "file_unique_id": "u1", "file_size": 10, "width": 100, "height": 100},
                    {"file_id": "large", "file_unique_id": "u2", "file_size": 20, "width": 1200, "height": 900}
                ]
            }
        }]
    });
    let msgs = parse_updates(&json);
    assert_eq!(msgs.len(), 1);
    assert!(msgs[0].text.contains("使用者上傳了 photo.jpg"), "{}", msgs[0].text);
    assert_eq!(msgs[0].attachments[0].file_id, "large");
    assert_eq!(msgs[0].attachments[0].mime_type.as_deref(), Some("image/jpeg"));
}
```

- [ ] **Step 3: Add failing client tests**

In `crates/wukong-tg-client/src/client.rs`, extend mock tests to assert file metadata/download hooks:

```rust
#[tokio::test]
async fn mock_returns_file_metadata_and_bytes() {
    let c = MockTgClient::default().with_file("file_1", "docs/report.pdf", b"hello".to_vec());
    let info = c.get_file("file_1").await.unwrap();
    assert_eq!(info.file_path, "docs/report.pdf");
    let bytes = c.download_file(&info.file_path).await.unwrap();
    assert_eq!(bytes, b"hello");
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run:

```bash
cargo test -p wukong-tg-client parse_updates_extracts_document_with_caption parse_updates_extracts_largest_photo_with_fallback_prompt mock_returns_file_metadata_and_bytes
```

Expected: FAIL because attachment fields and file methods do not exist.

- [ ] **Step 5: Extend Telegram parse models**

In `crates/wukong-tg-client/src/parse.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TgAttachmentKind {
    Document,
    Photo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgAttachment {
    pub kind: TgAttachmentKind,
    pub file_id: String,
    pub unique_file_id: Option<String>,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
}
```

Add to `TgMessage`:

```rust
pub attachments: Vec<TgAttachment>,
```

Update text-only parsing to set `attachments: Vec::new()`.

Parse document and photo messages from `message.document`, `message.photo`, and `message.caption`. Use `caption` as text when present. Generate fallback text with:

```rust
fn fallback_prompt(name: &str) -> String {
    format!("使用者上傳了 {name}，請分析附件內容。")
}
```

- [ ] **Step 6: Extend Telegram client trait**

In `crates/wukong-tg-client/src/client.rs`, add:

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgFileInfo {
    pub file_id: String,
    pub file_unique_id: Option<String>,
    pub file_size: Option<i64>,
    pub file_path: String,
}
```

Add trait methods:

```rust
fn get_file(&self, file_id: &str) -> impl std::future::Future<Output = Result<TgFileInfo, TgError>> + Send;
fn download_file(&self, file_path: &str) -> impl std::future::Future<Output = Result<Vec<u8>, TgError>> + Send;
```

Implement real client with Telegram `getFile` and `https://api.telegram.org/file/bot<TOKEN>/<file_path>`. Store both API base and file base in `ReqwestTgClient`:

```rust
base: String,
file_base: String,
```

Implement mock support:

```rust
pub fn with_file(self, file_id: &str, file_path: &str, bytes: Vec<u8>) -> Self
```

- [ ] **Step 7: Run tests**

Run:

```bash
cargo fmt
cargo test -p wukong-tg-client parse_updates_extracts_document_with_caption parse_updates_extracts_largest_photo_with_fallback_prompt mock_returns_file_metadata_and_bytes
cargo test -p wukong-tg-client
```

Expected: all PASS.

- [ ] **Step 8: Commit Task 4**

Run:

```bash
git add crates/wukong-tg-client/src/parse.rs crates/wukong-tg-client/src/client.rs
git commit -m "feat: parse telegram attachments"
```

---

### Task 5: Telegram Attachment Ingestion

**Files:**

- Modify: `crates/wukong-telegram/src/dispatch.rs`
- Test: `crates/wukong-telegram/src/dispatch.rs`

- [ ] **Step 1: Run impact analysis**

Run:

```bash
gitnexus_impact --target handle_message --direction upstream --repo Wukong
```

Expected: report blast radius. Stop if HIGH or CRITICAL.

- [ ] **Step 2: Add failing ingestion test**

In `crates/wukong-telegram/src/dispatch.rs`, add a test that sends a `TgMessage` with one document attachment and asserts chat history stores attachment metadata and the backend receives a path:

```rust
#[tokio::test]
async fn document_message_stores_attachment_and_passes_to_backend() {
    let mem = open_memory().await;
    let backend = RecordingBackend::default();
    let client = MockTgClient::default().with_file("doc_file", "documents/report.pdf", b"pdf".to_vec());
    let history = open_history().await;
    let cfg = base_cfg();
    let msg = TgMessage {
        update_id: 1,
        chat_id: 7,
        text: "請分析".to_string(),
        attachments: vec![TgAttachment {
            kind: TgAttachmentKind::Document,
            file_id: "doc_file".to_string(),
            unique_file_id: Some("unique".to_string()),
            original_name: "report.pdf".to_string(),
            mime_type: Some("application/pdf".to_string()),
            size_bytes: Some(3),
        }],
    };

    handle_message(&client, &mem, &backend, &cfg, &[7], msg, Some(&history)).await;

    let thread = history.default_thread("user:tg-7").await.unwrap();
    let messages = history.latest_messages(&thread, 10).await.unwrap();
    let user = messages.iter().find(|m| m.role == "user").unwrap();
    let attachments = history.attachments_for_messages(&[user.id]).await.unwrap();
    assert_eq!(attachments.len(), 1);
    assert_eq!(attachments[0].original_name, "report.pdf");
    assert!(std::path::Path::new(&attachments[0].relative_path).is_relative());

    let requests = backend.requests.lock().unwrap();
    assert_eq!(requests.last().unwrap().attachments.len(), 1);
    assert!(requests.last().unwrap().attachments[0].path.ends_with("report.pdf"));
}
```

If helper names differ, add `open_history()` and use the existing mock backend pattern in this file.

- [ ] **Step 3: Run test to verify it fails**

Run:

```bash
cargo test -p wukong-telegram document_message_stores_attachment_and_passes_to_backend
```

Expected: FAIL because ingestion logic does not download/store/pass attachments.

- [ ] **Step 4: Add ingestion constants and helpers**

In `crates/wukong-telegram/src/dispatch.rs`, add:

```rust
const MAX_ATTACHMENT_BYTES: i64 = 25 * 1024 * 1024;
const MAX_ATTACHMENTS_PER_MESSAGE: usize = 5;

fn upload_root() -> std::path::PathBuf {
    std::env::var("WUKONG_WORKSPACE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
        .join(".wukong")
        .join("uploads")
}
```

Add helper:

```rust
async fn store_telegram_attachments<C: TgClient>(
    client: &C,
    history: &ChatHistoryStore,
    scope: &str,
    message_id: i64,
    attachments: &[TgAttachment],
) -> Result<Vec<AgentAttachment>, String> {
    if attachments.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err("⚠️ 附件數量超過目前支援上限。".to_string());
    }
    let root = upload_root();
    let mut out = Vec::new();
    for attachment in attachments {
        if attachment.size_bytes.unwrap_or(0) > MAX_ATTACHMENT_BYTES {
            return Err("⚠️ 檔案超過目前支援大小，請改用較小的檔案。".to_string());
        }
        let info = client.get_file(&attachment.file_id).await.map_err(|_| "⚠️ 無法下載 Telegram 檔案，請稍後再試。".to_string())?;
        let bytes = client.download_file(&info.file_path).await.map_err(|_| "⚠️ 無法下載 Telegram 檔案，請稍後再試。".to_string())?;
        if bytes.len() as i64 > MAX_ATTACHMENT_BYTES {
            return Err("⚠️ 檔案超過目前支援大小，請改用較小的檔案。".to_string());
        }
        let stored_name = wukong_chat_history::sanitize_filename(&attachment.original_name);
        let relative_path = wukong_chat_history::relative_attachment_path(scope, message_id, &stored_name);
        let path = wukong_chat_history::resolve_under_upload_root(&root, &relative_path)
            .ok_or_else(|| "⚠️ 附件路徑無效。".to_string())?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| e.to_string())?;
        }
        tokio::fs::write(&path, &bytes).await.map_err(|e| e.to_string())?;
        history
            .insert_attachment(&NewChatAttachment {
                message_id,
                scope: scope.to_string(),
                source: "telegram".to_string(),
                original_name: attachment.original_name.clone(),
                stored_name: stored_name.clone(),
                relative_path,
                mime_type: attachment.mime_type.clone(),
                size_bytes: bytes.len() as i64,
                sha256: None,
                telegram_file_id: Some(attachment.file_id.clone()),
                created_at: now_unix(),
            })
            .await
            .map_err(|e| e.to_string())?;
        out.push(AgentAttachment {
            path,
            original_name: attachment.original_name.clone(),
            mime_type: attachment.mime_type.clone(),
        });
    }
    Ok(out)
}
```

- [ ] **Step 5: Integrate helper in turn flow**

In the `MessageAction::Turn(input)` branch, after inserting the user message and before starting `run_turn`, call `store_telegram_attachments` when `history` is available.

If attachments exist but `history` is `None`, reply:

```rust
"⚠️ 目前無法保存附件，請稍後再試。"
```

Replace the call to `run_turn` with:

```rust
run_turn_with_attachments(mem, backend, &cfg, &input, agent_attachments, &mut |ev| { ... }).await
```

Keep the existing stream event handling unchanged.

- [ ] **Step 6: Run tests**

Run:

```bash
cargo fmt
cargo test -p wukong-telegram document_message_stores_attachment_and_passes_to_backend
cargo test -p wukong-telegram
```

Expected: all PASS.

- [ ] **Step 7: Commit Task 5**

Run:

```bash
git add crates/wukong-telegram/src/dispatch.rs
git commit -m "feat: ingest telegram attachments"
```

---

### Task 6: Web Attachment APIs

**Files:**

- Modify: `crates/wukong-web/src/lib.rs`
- Test: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run API impact analysis before editing route handlers**

Run:

```bash
gitnexus_api_impact --route /api/chat/messages --repo Wukong
gitnexus_impact --target get_chat_messages --direction upstream --repo Wukong
gitnexus_impact --target build_router --direction upstream --repo Wukong
```

Expected: report consumers and risk. Stop if HIGH or CRITICAL.

- [ ] **Step 2: Add failing Web API tests**

In `crates/wukong-web/src/lib.rs`, add:

```rust
#[tokio::test]
async fn chat_messages_include_attachments() {
    let app_state = state(None, &[]).await;
    let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
    let thread = store.default_thread(&app_state.scope).await.unwrap();
    let message_id = store.insert_message(&thread, "user", "請看附件", None, "complete", 100).await.unwrap();
    store.insert_attachment(&NewChatAttachment {
        message_id,
        scope: app_state.scope.clone(),
        source: "telegram".to_string(),
        original_name: "report.pdf".to_string(),
        stored_name: "report.pdf".to_string(),
        relative_path: "project_Wukong/1/report.pdf".to_string(),
        mime_type: Some("application/pdf".to_string()),
        size_bytes: 12,
        sha256: None,
        telegram_file_id: Some("file_1".to_string()),
        created_at: 100,
    }).await.unwrap();
    let app = build_router(app_state);
    let resp = app.oneshot(Request::builder().uri("/api/chat/messages").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("attachments"), "{body}");
    assert!(body.contains("report.pdf"), "{body}");
}

#[tokio::test]
async fn attachment_download_requires_token_when_set() {
    let app = build_router(state(Some("sekret"), &[]).await);
    let resp = app.oneshot(Request::builder().uri("/api/chat/attachments/1").body(Body::empty()).unwrap()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run:

```bash
cargo test -p wukong-web chat_messages_include_attachments attachment_download_requires_token_when_set
```

Expected: FAIL because response shape and route do not exist.

- [ ] **Step 4: Add response DTOs**

In `crates/wukong-web/src/lib.rs`, add:

```rust
#[derive(serde::Serialize)]
struct ChatAttachmentResponse {
    id: i64,
    original_name: String,
    mime_type: Option<String>,
    size_bytes: i64,
    download_url: String,
    preview_url: Option<String>,
}

#[derive(serde::Serialize)]
struct ChatMessageResponse {
    #[serde(flatten)]
    message: ChatMessage,
    attachments: Vec<ChatAttachmentResponse>,
}
```

Change `ChatMessagesResponse` to:

```rust
struct ChatMessagesResponse {
    messages: Vec<ChatMessageResponse>,
    has_more: bool,
}
```

Add helper:

```rust
fn attachment_response(a: ChatAttachment) -> ChatAttachmentResponse {
    let is_image = a.mime_type.as_deref().unwrap_or("").starts_with("image/");
    ChatAttachmentResponse {
        id: a.id,
        original_name: a.original_name,
        mime_type: a.mime_type,
        size_bytes: a.size_bytes,
        download_url: format!("/api/chat/attachments/{}", a.id),
        preview_url: is_image.then(|| format!("/api/chat/attachments/{}/preview", a.id)),
    }
}
```

- [ ] **Step 5: Include attachments in message list responses**

In `get_chat_messages`, after loading `messages`, collect IDs, load attachments, group by `message_id`, and return `ChatMessageResponse`.

Use this pattern:

```rust
let message_ids = messages.iter().map(|m| m.id).collect::<Vec<_>>();
let attachments = store.attachments_for_messages(&message_ids).await?;
let mut by_message: std::collections::HashMap<i64, Vec<ChatAttachmentResponse>> = std::collections::HashMap::new();
for attachment in attachments {
    by_message.entry(attachment.message_id).or_default().push(attachment_response(attachment));
}
let messages = messages
    .into_iter()
    .map(|message| ChatMessageResponse {
        attachments: by_message.remove(&message.id).unwrap_or_default(),
        message,
    })
    .collect();
```

- [ ] **Step 6: Add download and preview handlers**

Add query type:

```rust
#[derive(serde::Deserialize)]
struct AttachmentQuery {
    token: Option<String>,
    scope: Option<String>,
}
```

Add upload-root helper matching Telegram:

```rust
fn upload_root() -> std::path::PathBuf {
    std::env::var("WUKONG_WORKSPACE")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from(".")))
        .join(".wukong")
        .join("uploads")
}
```

Add handlers:

```rust
async fn get_attachment<B>(
    State(state): State<AppState<B>>,
    Path(id): Path<i64>,
    Query(params): Query<AttachmentQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    attachment_file_response(state, id, params, false).await
}

async fn get_attachment_preview<B>(
    State(state): State<AppState<B>>,
    Path(id): Path<i64>,
    Query(params): Query<AttachmentQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    attachment_file_response(state, id, params, true).await
}
```

Implement `attachment_file_response` to:

- authorize token;
- load attachment by id;
- when `params.scope` is set, require `attachment.scope == params.scope`;
- resolve `relative_path` with `resolve_under_upload_root(&upload_root(), ...)`;
- reject missing files with `404`;
- for preview, require `mime_type` starts with `image/`;
- return bytes with `Content-Type` and safe `Content-Disposition` for download.

- [ ] **Step 7: Register routes**

In `build_router`, add:

```rust
.route("/api/chat/attachments/:id", axum::routing::get(get_attachment::<B>))
.route("/api/chat/attachments/:id/preview", axum::routing::get(get_attachment_preview::<B>))
```

- [ ] **Step 8: Run Web tests**

Run:

```bash
cargo fmt
cargo test -p wukong-web chat_messages_include_attachments attachment_download_requires_token_when_set
cargo test -p wukong-web
```

Expected: all PASS.

- [ ] **Step 9: Commit Task 6**

Run:

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat: expose chat attachments in web api"
```

---

### Task 7: Web Console Attachment UI

**Files:**

- Modify: `crates/wukong-web/static/components/wukong-chat.js`
- Modify: `crates/wukong-web/static/styles.css`
- Test: `node --check`, `cargo test -p wukong-web`

- [ ] **Step 1: Add attachment rendering helper**

In `crates/wukong-web/static/components/wukong-chat.js`, add this method before `messageNode`:

```javascript
  attachmentsNode(message) {
    const attachments = message.attachments || [];
    if (!attachments.length) return null;
    const wrap = document.createElement('div');
    wrap.className = 'attachments';
    for (const attachment of attachments) {
      const card = document.createElement('a');
      card.className = 'attachment-card';
      card.href = this.chatUrl(attachment.download_url || ('/api/chat/attachments/' + encodeURIComponent(attachment.id)));
      card.target = '_blank';
      card.rel = 'noopener';
      const name = escapeHTML(attachment.original_name || 'attachment');
      const type = escapeHTML(attachment.mime_type || '檔案');
      const size = this.formatBytes(attachment.size_bytes || 0);
      if (attachment.preview_url) {
        const img = document.createElement('img');
        img.className = 'attachment-thumb';
        img.src = this.chatUrl(attachment.preview_url);
        img.alt = attachment.original_name || 'attachment preview';
        card.appendChild(img);
      }
      const meta = document.createElement('span');
      meta.className = 'attachment-meta';
      meta.innerHTML = '<strong>' + name + '</strong><small>' + type + ' · ' + escapeHTML(size) + '</small>';
      card.appendChild(meta);
      wrap.appendChild(card);
    }
    return wrap;
  }

  formatBytes(bytes) {
    if (!bytes) return '0 B';
    const units = ['B', 'KiB', 'MiB', 'GiB'];
    let value = Number(bytes);
    let unit = 0;
    while (value >= 1024 && unit < units.length - 1) {
      value /= 1024;
      unit += 1;
    }
    return (unit === 0 ? value.toFixed(0) : value.toFixed(1)) + ' ' + units[unit];
  }
```

- [ ] **Step 2: Render attachments under message bubbles**

In `messageNode(message)`, after content/status logic and before `return div;`, add:

```javascript
    const attachments = this.attachmentsNode(message);
    if (attachments) div.appendChild(attachments);
```

- [ ] **Step 3: Add CSS**

In `crates/wukong-web/static/styles.css`, add:

```css
.attachments {
  display: grid;
  gap: 0.5rem;
  margin-top: 0.65rem;
}

.attachment-card {
  align-items: center;
  background: rgba(255, 255, 255, 0.08);
  border: 1px solid rgba(255, 255, 255, 0.16);
  border-radius: 0.8rem;
  color: inherit;
  display: flex;
  gap: 0.65rem;
  max-width: 22rem;
  padding: 0.55rem;
  text-decoration: none;
}

.attachment-card:hover {
  background: rgba(255, 255, 255, 0.14);
}

.attachment-thumb {
  border-radius: 0.55rem;
  height: 4rem;
  object-fit: cover;
  width: 4rem;
}

.attachment-meta {
  display: grid;
  gap: 0.15rem;
  min-width: 0;
}

.attachment-meta strong {
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.attachment-meta small {
  opacity: 0.72;
}
```

- [ ] **Step 4: Run JS and Web checks**

Run:

```bash
node --check crates/wukong-web/static/components/wukong-chat.js
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 5: Commit Task 7**

Run:

```bash
git add crates/wukong-web/static/components/wukong-chat.js crates/wukong-web/static/styles.css
git commit -m "feat: render chat attachment previews"
```

---

### Task 8: Final Verification And Documentation

**Files:**

- Modify: `README.md` if user-facing usage docs need a short note.
- No code changes unless verification reveals a defect.

- [ ] **Step 1: Run full verification**

Run:

```bash
cargo fmt --check
cargo test -p wukong-chat-history -p wukong-gateway -p wukong-runtime -p wukong-cli -p wukong-tg-client -p wukong-telegram -p wukong-web
cargo test
node --check crates/wukong-web/static/components/wukong-chat.js
```

Expected: all PASS.

- [ ] **Step 2: Run GitNexus change detection before final commit or handoff**

Run:

```bash
gitnexus_detect_changes --scope all --repo Wukong
```

Expected: changed symbols match attachment work; affected processes are expected chat/telegram/web/gateway flows. If unexpected unrelated symbols appear, inspect before proceeding.

- [ ] **Step 3: Manual smoke test with CLI backend**

Use a local Telegram update or a test harness to send one PDF or image attachment. Confirm:

- Telegram receives progress and final reply.
- File exists under `.wukong/uploads/<safe_scope>/<message_id>/`.
- `chat_attachments` row exists.
- Web Console shows file card or image thumbnail.
- Download link returns the file.
- CLI backend receives opencode `--file <path>`.

- [ ] **Step 4: Manual smoke test with server backend**

Set `WUKONG_AGENT_SERVER_URL` and send an attachment message. Confirm Telegram replies with:

```text
⚠️ 目前的 agent backend 不支援附件輸入。
```

or the more specific opencode server backend unsupported message. Confirm attachments are not silently ignored.

- [ ] **Step 5: Update README only if needed**

If user-facing docs are updated, add a short note near Telegram/Web Console usage:

```markdown
Telegram 上傳的圖片與文件會保存在 Wukong workspace 的 `.wukong/uploads/`，Web Console 可在同一個 Telegram scope 的聊天紀錄中看到附件卡片、圖片預覽與下載連結。附件會在 CLI backend 下傳給 opencode `--file`；server backend 若不支援附件會明確回報錯誤。
```

Run:

```bash
git add README.md
git commit -m "docs: describe telegram attachments"
```

Skip this commit if no README change is made.

- [ ] **Step 6: Final status**

Run:

```bash
git status --short --branch
```

Expected: only pre-existing unrelated local changes remain. Do not revert `AGENTS.md` or `CLAUDE.md` unless the user explicitly asks.

---

## Plan Self-Review

Spec coverage:

- Telegram documents/photos/captions are covered by Tasks 4 and 5.
- Workspace file storage and path sanitization are covered by Tasks 1 and 5.
- SQLite attachment metadata is covered by Task 1.
- Web message response attachments, download, and image preview endpoints are covered by Task 6.
- Web Console file cards and thumbnails are covered by Task 7.
- CLI backend `--file` support and server backend unsupported behavior are covered by Task 2.
- Attachment-aware runtime entrypoints are covered by Task 3.
- Security limits and no silent dropping are covered by Tasks 2, 5, and 6.
- Full verification is covered by Task 8.

Placeholder scan:

- Every implementation task names exact files, signatures, commands, and expected outcomes. The plan contains no deferred sections or open-ended implementation instructions.

Type consistency:

- `ChatAttachment`, `NewChatAttachment`, `AgentAttachment`, `TgAttachment`, and `TgFileInfo` are defined before later tasks use them.
- Existing text-only APIs are preserved and attachment-aware APIs are additive.
