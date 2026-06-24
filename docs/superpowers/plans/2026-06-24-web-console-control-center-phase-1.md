# Web Console Control Center Phase 1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Turn `wukong-web` into a tabbed Control Center shell with read-only Memory and Skills panels plus first-class global model settings.

**Architecture:** Keep the current zero-build Web Component architecture. Add small Rust API modules for memory and skills, extend the existing settings API for `agent.default_model`, and route top-level panels via URL hash in `app.js`.

**Tech Stack:** Rust workspace crates (`wukong-web`, `wukong-memory`, `wukong-skills`, `wukong-settings`), Axum, SQLite via existing memory store, plain ES modules, custom elements, existing CSS.

---

## Scope

This plan implements Phase 1 from `docs/superpowers/specs/2026-06-24-web-console-control-center-design.md`.

Included:

- Top-level `Chat`, `Memory`, `Skills`, `Schedules`, `System`, `Settings` navigation.
- Read-only Memory observability: snapshot summary and paginated records.
- Read-only Skills catalog.
- Settings global model GET/PUT using existing `wukong_settings::Settings.agent.default_model`.
- Chat indicator for current global model and whether skill preferences are active.

Deferred to later plans:

- Planner preference injection.
- Writable skill preferences.
- Memory consolidate/prune/export operations.
- Recall sandbox and scoring explanations.
- Per-scope model overrides.

## File Structure

- Modify: `crates/wukong-memory/src/model.rs`
  Add `MemoryRecord` and `MemoryRecordsPage` response types.
- Modify: `crates/wukong-memory/src/store/mod.rs`
  Add a focused `list_records(scope, kind, limit)` query.
- Modify: `crates/wukong-memory/src/lib.rs`
  Re-export new types and expose `Memory::records`.
- Create: `crates/wukong-web/src/memory_api.rs`
  Own JSON response shaping for memory summary and records.
- Create: `crates/wukong-web/src/skills_api.rs`
  Own JSON response shaping for roles and Superpowers catalog.
- Modify: `crates/wukong-web/src/lib.rs`
  Register modules, static assets, routes, settings model API, and tests.
- Modify: `crates/wukong-web/Cargo.toml`
  Add `wukong-skills` dependency.
- Modify: `crates/wukong-web/static/index.html`
  Add top-level Control Center navigation links.
- Modify: `crates/wukong-web/static/app.js`
  Replace settings-subnav-only routing with top-level tab routing.
- Create: `crates/wukong-web/static/components/wukong-memory.js`
  Memory summary and record browser component.
- Create: `crates/wukong-web/static/components/wukong-skills.js`
  Skills catalog component.
- Modify: `crates/wukong-web/static/components/wukong-settings.js`
  Add global model form while preserving Telegram settings.
- Modify: `crates/wukong-web/static/components/wukong-chat.js`
  Show model/preference status in toolbar.
- Modify: `crates/wukong-web/static/styles.css`
  Add tab, panel, cards, stat grid, and mobile styles.

---

### Task 1: Add Memory Record Listing To `wukong-memory`

**Files:**

- Modify: `crates/wukong-memory/src/model.rs`
- Modify: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Add the failing store test**

Add this test to `crates/wukong-memory/src/store/mod.rs` inside the existing `#[cfg(test)] mod tests` block. Use the existing `test_store()` helper.

```rust
#[tokio::test]
async fn list_records_filters_scope_kind_and_limit() {
    let store = test_store().await;
    let now = 1_700_000_000;

    store
        .insert_memory(None, "global", MemoryKind::Note, "global note", 0.7, now)
        .await
        .unwrap();
    store
        .insert_memory(None, "project:Wukong", MemoryKind::Decision, "keep this", 1.0, now + 1)
        .await
        .unwrap();
    store
        .insert_memory(None, "project:Wukong", MemoryKind::Note, "skip kind", 0.4, now + 2)
        .await
        .unwrap();

    let rows = store
        .list_records(Some("project:Wukong"), Some(MemoryKind::Decision), 10)
        .await
        .unwrap();

    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].scope, "project:Wukong");
    assert_eq!(rows[0].kind, MemoryKind::Decision);
    assert_eq!(rows[0].text, "keep this");
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `cargo test -p wukong-memory store::tests::list_records_filters_scope_kind_and_limit`

Expected: compile failure because `MemoryRecord` and `Store::list_records` do not exist.

- [ ] **Step 3: Add response types**

In `crates/wukong-memory/src/model.rs`, add these structs after `Snapshot`.

```rust
/// A single memory row for Web observability views.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecord {
    pub id: i64,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    pub importance: f64,
    pub created_at: i64,
    pub last_recalled_at: Option<i64>,
    pub recall_count: i64,
    pub has_embedding: bool,
    pub consolidated_into: Option<i64>,
}

/// A bounded page of memory records.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecordsPage {
    pub records: Vec<MemoryRecord>,
    pub has_more: bool,
}
```

- [ ] **Step 4: Re-export the new types**

In `crates/wukong-memory/src/lib.rs`, update the `pub use model::{ ... }` list so it includes the new types.

```rust
pub use model::{
    AgeBuckets, EmbeddingCoverage, Evidence, KindCount, MemoryItem, MemoryKind, MemoryRecord,
    MemoryRecordsPage, RecallHit, RecallMode, RecallQuery, RememberInput, ScopeCount, Snapshot,
    Stats, WukongResult,
};
```

- [ ] **Step 5: Implement `Store::list_records`**

In `crates/wukong-memory/src/store/mod.rs`, add this method before `all_for_export`.

```rust
    /// Recent memory rows for observability. Ordered newest-first.
    pub async fn list_records(
        &self,
        scope: Option<&str>,
        kind: Option<MemoryKind>,
        limit: i64,
    ) -> Result<Vec<MemoryRecord>> {
        let limit = limit.clamp(1, 101);
        let rows = match (scope, kind) {
            (Some(scope), Some(kind)) => {
                sqlx::query(
                    "SELECT id, scope, kind, text, importance, created_at, last_recalled_at,
                            recall_count, embedding, consolidated_into
                     FROM memories
                     WHERE scope = ?1 AND kind = ?2
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?3",
                )
                .bind(scope)
                .bind(kind.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(scope), None) => {
                sqlx::query(
                    "SELECT id, scope, kind, text, importance, created_at, last_recalled_at,
                            recall_count, embedding, consolidated_into
                     FROM memories
                     WHERE scope = ?1
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?2",
                )
                .bind(scope)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(kind)) => {
                sqlx::query(
                    "SELECT id, scope, kind, text, importance, created_at, last_recalled_at,
                            recall_count, embedding, consolidated_into
                     FROM memories
                     WHERE kind = ?1
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?2",
                )
                .bind(kind.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT id, scope, kind, text, importance, created_at, last_recalled_at,
                            recall_count, embedding, consolidated_into
                     FROM memories
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?1",
                )
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(rows.into_iter().map(row_to_memory_record).collect())
    }
```

Add this helper near `row_to_candidate`.

```rust
fn row_to_memory_record(r: sqlx::sqlite::SqliteRow) -> MemoryRecord {
    MemoryRecord {
        id: r.get::<i64, _>("id"),
        scope: r.get::<String, _>("scope"),
        kind: MemoryKind::from_db_str(&r.get::<String, _>("kind")),
        text: r.get::<String, _>("text"),
        importance: r.get::<f64, _>("importance"),
        created_at: r.get::<i64, _>("created_at"),
        last_recalled_at: r.get::<Option<i64>, _>("last_recalled_at"),
        recall_count: r.get::<i64, _>("recall_count"),
        has_embedding: r.get::<Option<Vec<u8>>, _>("embedding").is_some(),
        consolidated_into: r.get::<Option<i64>, _>("consolidated_into"),
    }
}
```

Ensure the file imports `MemoryRecord` from `crate::model`.

- [ ] **Step 6: Add the public facade method**

In `crates/wukong-memory/src/lib.rs`, add this method near `snapshot` and `stats`.

```rust
    /// Recent memory rows for Web observability.
    pub async fn records(
        &self,
        scope: Option<&str>,
        kind: Option<MemoryKind>,
        limit: i64,
    ) -> Result<MemoryRecordsPage> {
        let requested = limit.clamp(1, 100);
        let mut records = self.store.list_records(scope, kind, requested + 1).await?;
        let has_more = records.len() as i64 > requested;
        if has_more {
            records.pop();
        }
        Ok(MemoryRecordsPage { records, has_more })
    }
```

- [ ] **Step 7: Run focused memory tests**

Run: `cargo test -p wukong-memory list_records_filters_scope_kind_and_limit`

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```bash
git add crates/wukong-memory/src/model.rs crates/wukong-memory/src/store/mod.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): expose records for web observability"
```

---

### Task 2: Add Memory And Skills Web APIs

**Files:**

- Create: `crates/wukong-web/src/memory_api.rs`
- Create: `crates/wukong-web/src/skills_api.rs`
- Modify: `crates/wukong-web/src/lib.rs`
- Modify: `crates/wukong-web/Cargo.toml`

- [ ] **Step 1: Add failing API tests**

In `crates/wukong-web/src/lib.rs` tests module, add these tests.

```rust
#[tokio::test]
async fn memory_summary_returns_snapshot() {
    let state = state(None, &[]).await;
    state
        .memory
        .remember(wukong_memory::RememberInput {
            scope: "project:Wukong".to_string(),
            session_id: None,
            items: vec![wukong_memory::MemoryItem {
                kind: wukong_memory::MemoryKind::Decision,
                text: "Use Web Console as control center".to_string(),
                importance: Some(1.0),
            }],
        })
        .await
        .unwrap();
    let app = build_router(state);

    let resp = app
        .oneshot(Request::builder().uri("/api/memory/summary?scope=project:Wukong").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("project:Wukong"));
    assert!(body.contains("consolidation_candidates"));
}

#[tokio::test]
async fn memory_records_returns_recent_rows() {
    let state = state(None, &[]).await;
    state
        .memory
        .remember(wukong_memory::RememberInput {
            scope: "project:Wukong".to_string(),
            session_id: None,
            items: vec![wukong_memory::MemoryItem {
                kind: wukong_memory::MemoryKind::Note,
                text: "Memory panel can read records".to_string(),
                importance: Some(0.8),
            }],
        })
        .await
        .unwrap();
    let app = build_router(state);

    let resp = app
        .oneshot(Request::builder().uri("/api/memory/records?scope=project:Wukong&limit=5").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Memory panel can read records"));
    assert!(body.contains("has_more"));
}

#[tokio::test]
async fn skills_catalog_returns_roles_and_skills() {
    let app = build_router(state(None, &[]).await);

    let resp = app
        .oneshot(Request::builder().uri("/api/skills/catalog").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("Explorer"));
    assert!(body.contains("systematic-debugging"));
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run these commands:

```bash
cargo test -p wukong-web memory_summary_returns_snapshot
cargo test -p wukong-web memory_records_returns_recent_rows
cargo test -p wukong-web skills_catalog_returns_roles_and_skills
```

Expected: compile failure or 404 because API modules/routes do not exist.

- [ ] **Step 3: Add `wukong-skills` dependency**

In `crates/wukong-web/Cargo.toml`, add this under local crate dependencies.

```toml
wukong-skills = { path = "../wukong-skills" }
```

- [ ] **Step 4: Create `memory_api.rs`**

Create `crates/wukong-web/src/memory_api.rs`.

```rust
use serde::Deserialize;
use wukong_memory::{MemoryKind, MemoryRecordsPage, Snapshot};

#[derive(Debug, Deserialize)]
pub struct MemorySummaryQuery {
    pub token: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MemoryRecordsQuery {
    pub token: Option<String>,
    pub scope: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

pub fn parse_kind(kind: Option<&str>) -> Result<Option<MemoryKind>, String> {
    match kind.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some("decision") => Ok(Some(MemoryKind::Decision)),
        Some("event") => Ok(Some(MemoryKind::Event)),
        Some("skill") => Ok(Some(MemoryKind::Skill)),
        Some("note") => Ok(Some(MemoryKind::Note)),
        Some("summary") => Ok(Some(MemoryKind::Summary)),
        Some(other) => Err(format!("unknown memory kind: {other}")),
    }
}

pub fn capped_records_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(20).clamp(1, 100)
}

pub type MemorySummaryResponse = Snapshot;
pub type MemoryRecordsResponse = MemoryRecordsPage;
```

- [ ] **Step 5: Create `skills_api.rs`**

Create `crates/wukong-web/src/skills_api.rs`.

```rust
use serde::Serialize;
use wukong_orchestrator::Role;

#[derive(Debug, Serialize)]
pub struct RoleResponse {
    pub name: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SkillResponse {
    pub name: &'static str,
    pub description: &'static str,
    pub primary_role: &'static str,
    pub collaborator_role: Option<&'static str>,
}

#[derive(Debug, Serialize)]
pub struct SkillsCatalogResponse {
    pub roles: Vec<RoleResponse>,
    pub skills: Vec<SkillResponse>,
}

pub fn role_name(role: Role) -> &'static str {
    match role {
        Role::Explorer => "Explorer",
        Role::Oracle => "Oracle",
        Role::Librarian => "Librarian",
        Role::Fixer => "Fixer",
        Role::Designer => "Designer",
    }
}

pub fn catalog_response() -> SkillsCatalogResponse {
    SkillsCatalogResponse {
        roles: vec![
            RoleResponse { name: "Explorer" },
            RoleResponse { name: "Oracle" },
            RoleResponse { name: "Librarian" },
            RoleResponse { name: "Fixer" },
            RoleResponse { name: "Designer" },
        ],
        skills: wukong_skills::catalog::all()
            .iter()
            .map(|skill| SkillResponse {
                name: skill.name,
                description: skill.description,
                primary_role: role_name(skill.primary_role),
                collaborator_role: skill.collaborator_role.map(role_name),
            })
            .collect(),
    }
}
```

- [ ] **Step 6: Wire modules and handlers in `lib.rs`**

At the top of `crates/wukong-web/src/lib.rs`, add:

```rust
pub mod memory_api;
pub mod skills_api;
```

Add handlers near `get_system`.

```rust
async fn get_memory_summary<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<memory_api::MemorySummaryQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.memory.snapshot(params.scope.as_deref()).await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_memory_records<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<memory_api::MemoryRecordsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let kind = match memory_api::parse_kind(params.kind.as_deref()) {
        Ok(kind) => kind,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    match state
        .memory
        .records(
            params.scope.as_deref(),
            kind,
            memory_api::capped_records_limit(params.limit),
        )
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_skills_catalog<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(skills_api::catalog_response()).into_response()
}
```

Register these routes in `build_router`.

```rust
        .route("/api/memory/summary", axum::routing::get(get_memory_summary::<B>))
        .route("/api/memory/records", axum::routing::get(get_memory_records::<B>))
        .route("/api/skills/catalog", axum::routing::get(get_skills_catalog::<B>))
```

- [ ] **Step 7: Run focused web API tests**

Run these commands:

```bash
cargo test -p wukong-web memory_summary_returns_snapshot
cargo test -p wukong-web memory_records_returns_recent_rows
cargo test -p wukong-web skills_catalog_returns_roles_and_skills
```

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```bash
git add crates/wukong-web/Cargo.toml crates/wukong-web/src/lib.rs crates/wukong-web/src/memory_api.rs crates/wukong-web/src/skills_api.rs
git commit -m "feat(web): add memory and skills APIs"
```

---

### Task 3: Add Global Model Settings API

**Files:**

- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Add failing tests**

In `crates/wukong-web/src/lib.rs` tests module, add:

```rust
#[tokio::test]
async fn model_settings_round_trip() {
    let app = build_router(state(None, &[]).await);

    let resp = app
        .clone()
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings/model")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"opencode/deepseek-v4-flash-free"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let resp = app
        .oneshot(Request::builder().uri("/api/settings/model").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("opencode/deepseek-v4-flash-free"));
    assert!(body.contains("persisted"));
}

#[tokio::test]
async fn model_settings_reject_empty_model() {
    let app = build_router(state(None, &[]).await);
    let resp = app
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/api/settings/model")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"model":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Run tests and verify they fail**

Run these commands:

```bash
cargo test -p wukong-web model_settings_round_trip
cargo test -p wukong-web model_settings_reject_empty_model
```

Expected: 404 or compile failure because `/api/settings/model` does not exist.

- [ ] **Step 3: Add request/response types**

In `crates/wukong-web/src/lib.rs`, add near existing settings structs.

```rust
#[derive(serde::Serialize)]
struct ModelSettingsResponse {
    model: Option<String>,
    source: String,
    editable: bool,
}

#[derive(serde::Deserialize)]
struct SaveModelSettingsRequest {
    model: String,
}
```

- [ ] **Step 4: Add model handlers**

In `crates/wukong-web/src/lib.rs`, add near `get_settings` and `post_settings`.

```rust
fn env_model_override() -> Option<String> {
    std::env::var("WUKONG_MODEL")
        .ok()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
}

async fn get_model_settings<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(model) = env_model_override() {
        return Json(ModelSettingsResponse {
            model: Some(model),
            source: "env".to_string(),
            editable: false,
        })
        .into_response();
    }
    match wukong_settings::load_settings(&state.settings_path) {
        Ok(settings) => Json(ModelSettingsResponse {
            model: settings.agent.default_model,
            source: "persisted".to_string(),
            editable: true,
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn put_model_settings<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<SaveModelSettingsRequest>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if env_model_override().is_some() {
        return (StatusCode::CONFLICT, "model is controlled by environment").into_response();
    }
    let model = req.model.trim();
    if model.is_empty() {
        return (StatusCode::BAD_REQUEST, "model must not be empty").into_response();
    }
    let mut settings = wukong_settings::load_settings(&state.settings_path).unwrap_or_default();
    settings.agent.default_model = Some(model.to_string());
    match wukong_settings::save_settings(&state.settings_path, &settings) {
        Ok(()) => Json(ModelSettingsResponse {
            model: Some(model.to_string()),
            source: "persisted".to_string(),
            editable: true,
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

- [ ] **Step 5: Register route**

In `build_router`, add:

```rust
        .route(
            "/api/settings/model",
            axum::routing::get(get_model_settings::<B>).put(put_model_settings::<B>),
        )
```

- [ ] **Step 6: Run focused tests**

Run these commands:

```bash
cargo test -p wukong-web model_settings_round_trip
cargo test -p wukong-web model_settings_reject_empty_model
```

Expected: PASS.

- [ ] **Step 7: Commit**

Run:

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat(web): add global model settings API"
```

---

### Task 4: Add Control Center Frontend Shell

**Files:**

- Modify: `crates/wukong-web/src/lib.rs`
- Modify: `crates/wukong-web/static/index.html`
- Modify: `crates/wukong-web/static/app.js`
- Modify: `crates/wukong-web/static/styles.css`

- [ ] **Step 1: Add failing static asset test**

In `crates/wukong-web/src/lib.rs`, update `serves_static_assets_with_content_types` to also check the new assets.

```rust
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-memory.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-skills.js"
        )
        .await
        .contains("javascript"));
```

- [ ] **Step 2: Run test and verify it fails**

Run: `cargo test -p wukong-web serves_static_assets_with_content_types`

Expected: failure because new assets are not served.

- [ ] **Step 3: Serve new component assets**

In `crates/wukong-web/src/lib.rs`, add constants:

```rust
const MEMORY_JS: &str = include_str!("../static/components/wukong-memory.js");
const SKILLS_JS: &str = include_str!("../static/components/wukong-skills.js");
```

Add handlers:

```rust
async fn memory_js() -> axum::response::Response {
    asset(JS, MEMORY_JS)
}
async fn skills_js() -> axum::response::Response {
    asset(JS, SKILLS_JS)
}
```

Register routes:

```rust
        .route(
            "/components/wukong-memory.js",
            axum::routing::get(memory_js),
        )
        .route(
            "/components/wukong-skills.js",
            axum::routing::get(skills_js),
        )
```

- [ ] **Step 4: Update `index.html` navigation**

Replace the `<nav>` in `crates/wukong-web/static/index.html` with:

```html
    <nav>
      <a href="#/chat" data-route="chat">對話</a>
      <a href="#/memory" data-route="memory">記憶</a>
      <a href="#/skills" data-route="skills">技能</a>
      <a href="#/schedules" data-route="schedules">排程</a>
      <a href="#/system" data-route="system">系統</a>
      <a href="#/settings" data-route="settings">設定</a>
    </nav>
```

- [ ] **Step 5: Update `app.js` imports and routing**

Replace `crates/wukong-web/static/app.js` with:

```javascript
import { WukongChat } from '/components/wukong-chat.js';
import { WukongMemory } from '/components/wukong-memory.js';
import { WukongSkills } from '/components/wukong-skills.js';
import { WukongSettings } from '/components/wukong-settings.js';
import { WukongSchedules } from '/components/wukong-schedules.js';
import { WukongSystem } from '/components/wukong-system.js';

customElements.define('wukong-chat', WukongChat);
customElements.define('wukong-memory', WukongMemory);
customElements.define('wukong-skills', WukongSkills);
customElements.define('wukong-settings', WukongSettings);
customElements.define('wukong-schedules', WukongSchedules);
customElements.define('wukong-system', WukongSystem);

const app = document.querySelector('#app');

const routes = {
  '#/chat': '<wukong-chat></wukong-chat>',
  '#/memory': '<wukong-memory></wukong-memory>',
  '#/skills': '<wukong-skills></wukong-skills>',
  '#/schedules': '<wukong-schedules></wukong-schedules>',
  '#/system': '<wukong-system></wukong-system>',
  '#/settings': '<wukong-settings></wukong-settings>',
};

function render() {
  const route = window.location.hash || '#/chat';
  app.innerHTML = routes[route] || '<section class="empty-state"><h2>找不到頁面</h2><p><a href="#/chat">回到對話</a></p></section>';
  document.querySelectorAll('header nav a').forEach((a) => {
    a.classList.toggle('active', route === '#/' + a.dataset.route);
  });
}

window.addEventListener('hashchange', render);
if (!window.location.hash) window.location.hash = '#/chat';
render();
```

- [ ] **Step 6: Add basic Control Center CSS**

Append to `crates/wukong-web/static/styles.css`:

```css
.panel {
  flex: 1;
  min-height: 0;
  overflow: auto;
  padding: 1rem;
}
.panel-header {
  display: flex;
  align-items: flex-start;
  justify-content: space-between;
  gap: 1rem;
  margin-bottom: 1rem;
}
.panel-header h2 { margin: 0; }
.panel-help { color: var(--text-secondary); margin: 0.25rem 0 0; }
.stat-grid {
  display: grid;
  grid-template-columns: repeat(auto-fit, minmax(10rem, 1fr));
  gap: 0.75rem;
  margin-bottom: 1rem;
}
.stat-card, .control-card, .record-card, .skill-card {
  background: var(--bg-secondary);
  border: 1px solid var(--border-color);
  border-radius: var(--border-radius);
  padding: 0.85rem;
}
.stat-card strong { display: block; font-size: 1.35rem; color: var(--accent-gold); }
.control-row { display: flex; gap: 0.5rem; flex-wrap: wrap; align-items: center; }
.control-row input, .control-row select, .control-row button { font: inherit; }
.record-list, .skill-grid { display: grid; gap: 0.75rem; }
.skill-grid { grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr)); }
.tag {
  display: inline-block;
  border: 1px solid var(--border-color);
  border-radius: 999px;
  padding: 0.15rem 0.45rem;
  color: var(--text-secondary);
  font-size: 0.8rem;
}
@media (max-width: 720px) {
  header { align-items: flex-start; flex-direction: column; }
  header nav { width: 100%; overflow-x: auto; padding-bottom: 0.2rem; }
  .panel-header { flex-direction: column; }
}
```

- [ ] **Step 7: Run static asset test**

Run: `cargo test -p wukong-web serves_static_assets_with_content_types`

Expected: still fails until Task 5 creates the files, or PASS if placeholder files are created in this task. If it fails due to missing files, continue to Task 5 before committing.

---

### Task 5: Add Memory And Skills Frontend Components

**Files:**

- Create: `crates/wukong-web/static/components/wukong-memory.js`
- Create: `crates/wukong-web/static/components/wukong-skills.js`

- [ ] **Step 1: Create Memory component**

Create `crates/wukong-web/static/components/wukong-memory.js`.

```javascript
import { html } from '/lib/html.js';

export class WukongMemory extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>記憶</h2>
            <p class="panel-help">先提供可觀測能力：健康快照、scope 分布與近期記憶。</p>
          </div>
          <button id="refresh-memory">重新整理</button>
        </div>
        <div id="memory-status" class="settings-status">載入中…</div>
        <div id="memory-summary" class="stat-grid"></div>
        <section class="control-card">
          <div class="control-row">
            <label>Scope <select id="memory-scope"><option value="">全部</option></select></label>
            <label>Kind
              <select id="memory-kind">
                <option value="">全部</option>
                <option value="decision">Decision</option>
                <option value="event">Event</option>
                <option value="skill">Skill</option>
                <option value="note">Note</option>
                <option value="summary">Summary</option>
              </select>
            </label>
          </div>
        </section>
        <div id="memory-records" class="record-list"></div>
      </section>
    `.toString();
    this.status = this.querySelector('#memory-status');
    this.summary = this.querySelector('#memory-summary');
    this.records = this.querySelector('#memory-records');
    this.scopeSelect = this.querySelector('#memory-scope');
    this.kindSelect = this.querySelector('#memory-kind');
    this.querySelector('#refresh-memory').addEventListener('click', () => this.load());
    this.scopeSelect.addEventListener('change', () => this.loadRecords());
    this.kindSelect.addEventListener('change', () => this.loadRecords());
    this.load();
  }

  tokenParam(prefix = '?') {
    return window.WUKONG_TOKEN ? prefix + 'token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/memory/summary' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = '無法讀取記憶摘要：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.status.textContent = '已載入記憶摘要';
    this.renderSummary(data);
    this.renderScopes(data.by_scope || []);
    await this.loadRecords();
  }

  renderSummary(data) {
    this.summary.innerHTML = html`
      <article class="stat-card"><span>總記憶</span><strong>${data.total}</strong></article>
      <article class="stat-card"><span>Scopes</span><strong>${(data.by_scope || []).length}</strong></article>
      <article class="stat-card"><span>Consolidate 候選</span><strong>${data.consolidation_candidates}</strong></article>
      <article class="stat-card"><span>Prune 候選</span><strong>${data.prune_candidates}</strong></article>
      <article class="stat-card"><span>Embedding</span><strong>${data.embedding.embedded}/${data.embedding.total}</strong></article>
    `.toString();
  }

  renderScopes(scopes) {
    const current = this.scopeSelect.value;
    this.scopeSelect.innerHTML = '<option value="">全部</option>' + scopes.map((s) =>
      '<option value="' + encodeURIComponent(s.scope) + '">' + s.scope + ' (' + s.count + ')</option>'
    ).join('');
    this.scopeSelect.value = current;
  }

  async loadRecords() {
    const params = new URLSearchParams();
    if (window.WUKONG_TOKEN) params.set('token', window.WUKONG_TOKEN);
    if (this.scopeSelect.value) params.set('scope', decodeURIComponent(this.scopeSelect.value));
    if (this.kindSelect.value) params.set('kind', this.kindSelect.value);
    params.set('limit', '20');
    const resp = await fetch('/api/memory/records?' + params.toString());
    if (!resp.ok) {
      this.records.textContent = '無法讀取記憶列表：HTTP ' + resp.status;
      return;
    }
    const page = await resp.json();
    this.records.innerHTML = (page.records || []).map((record) => html`
      <article class="record-card">
        <div><span class="tag">${record.scope}</span> <span class="tag">${record.kind}</span></div>
        <p>${record.text}</p>
        <small>importance ${record.importance} · recalled ${record.recall_count} · ${new Date(record.created_at * 1000).toLocaleString('zh-TW')}</small>
      </article>
    `.toString()).join('') || '<p class="empty-state">沒有記憶。</p>';
  }
}
```

- [ ] **Step 2: Create Skills component**

Create `crates/wukong-web/static/components/wukong-skills.js`.

```javascript
import { html } from '/lib/html.js';

export class WukongSkills extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>技能</h2>
            <p class="panel-help">Phase 1 先顯示角色與 Superpowers catalog；偏好儲存與 planner 注入留到 Phase 2。</p>
          </div>
        </div>
        <div id="skills-status" class="settings-status">載入中…</div>
        <section class="control-card"><h3>角色</h3><div id="roles" class="control-row"></div></section>
        <section><h3>Superpowers</h3><div id="skills" class="skill-grid"></div></section>
      </section>
    `.toString();
    this.status = this.querySelector('#skills-status');
    this.roles = this.querySelector('#roles');
    this.skills = this.querySelector('#skills');
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/skills/catalog' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = '無法讀取技能目錄：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.status.textContent = '已載入技能目錄';
    this.roles.innerHTML = (data.roles || []).map((role) => '<span class="tag">' + role.name + '</span>').join('');
    this.skills.innerHTML = (data.skills || []).map((skill) => html`
      <article class="skill-card">
        <h3>${skill.name}</h3>
        <p>${skill.description}</p>
        <p><span class="tag">主責 ${skill.primary_role}</span> ${skill.collaborator_role ? '<span class="tag">協作 ' + skill.collaborator_role + '</span>' : ''}</p>
      </article>
    `.toString()).join('');
  }
}
```

- [ ] **Step 3: Run static asset test**

Run: `cargo test -p wukong-web serves_static_assets_with_content_types`

Expected: PASS.

- [ ] **Step 4: Commit Tasks 4 and 5 together**

Run:

```bash
git add crates/wukong-web/src/lib.rs crates/wukong-web/static/index.html crates/wukong-web/static/app.js crates/wukong-web/static/styles.css crates/wukong-web/static/components/wukong-memory.js crates/wukong-web/static/components/wukong-skills.js
git commit -m "feat(web): add control center shell"
```

---

### Task 6: Add Global Model UI And Chat Indicators

**Files:**

- Modify: `crates/wukong-web/static/components/wukong-settings.js`
- Modify: `crates/wukong-web/static/components/wukong-chat.js`

- [ ] **Step 1: Update Settings component**

Replace `crates/wukong-web/static/components/wukong-settings.js` with:

```javascript
import { html } from '/lib/html.js';

export class WukongSettings extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="panel">
        <div class="panel-header">
          <div>
            <h2>設定</h2>
            <p class="panel-help">可持久化設定。System 分頁保留給只讀診斷。</p>
          </div>
        </div>
        <section class="settings-card">
          <h3>全域模型</h3>
          <p class="settings-help">第一版只支援全域預設模型，後續 Web / Telegram / Scheduler / CLI turns 都會使用。</p>
          <form id="model-form" class="settings-form">
            <label>Default model<input id="model-input" type="text" placeholder="opencode/deepseek-v4-flash-free" /></label>
            <button type="submit">儲存模型</button>
          </form>
          <p id="model-status" class="settings-status">載入中…</p>
        </section>
        <section class="settings-card">
          <h3>Telegram 設定</h3>
          <p class="settings-help">輸入 Bot token 與允許的 chat/user ID。儲存後 Telegram 服務會自動開始等待訊息。</p>
          <form id="settings-form" class="settings-form">
            <label>Bot token<input id="tg-token" type="password" autocomplete="off" placeholder="123456:ABC..." /></label>
            <label>Allowed IDs<textarea id="tg-allowed" rows="3" placeholder="例如：123456789 或多個 ID 以空白分隔"></textarea></label>
            <button type="submit">儲存 Telegram</button>
          </form>
          <p id="settings-status" class="settings-status">載入中…</p>
        </section>
      </section>
    `.toString();
    this.status = this.querySelector('#settings-status');
    this.modelStatus = this.querySelector('#model-status');
    this.modelInput = this.querySelector('#model-input');
    this.tokenInput = this.querySelector('#tg-token');
    this.allowedInput = this.querySelector('#tg-allowed');
    this.querySelector('#settings-form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.saveTelegram();
    });
    this.querySelector('#model-form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.saveModel();
    });
    this.loadTelegram();
    this.loadModel();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async loadModel() {
    const resp = await fetch('/api/settings/model' + this.tokenParam());
    if (!resp.ok) {
      this.modelStatus.textContent = '無法讀取模型設定：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.modelInput.value = data.model || '';
    this.modelInput.disabled = !data.editable;
    this.querySelector('#model-form button').disabled = !data.editable;
    this.modelStatus.textContent = data.model
      ? '目前模型：' + data.model + '（來源：' + data.source + '）'
      : '尚未設定全域模型，將使用底層 agent 預設。';
  }

  async saveModel() {
    const model = this.modelInput.value.trim();
    if (!model) {
      this.modelStatus.textContent = '模型不可為空。';
      return;
    }
    const resp = await fetch('/api/settings/model' + this.tokenParam(), {
      method: 'PUT',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ model }),
    });
    if (!resp.ok) {
      this.modelStatus.textContent = '儲存模型失敗：HTTP ' + resp.status;
      return;
    }
    await this.loadModel();
  }

  async loadTelegram() {
    const resp = await fetch('/api/settings' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = '無法讀取 Telegram 設定：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.allowedInput.value = data.telegram.allowed || '';
    this.status.textContent = data.telegram.configured
      ? '已設定 token：' + data.telegram.token
      : '尚未設定 Telegram token';
  }

  async saveTelegram() {
    const body = {
      telegram: {
        token: this.tokenInput.value.trim(),
        allowed: this.allowedInput.value.trim(),
      },
    };
    const resp = await fetch('/api/settings' + this.tokenParam(), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!resp.ok) {
      this.status.textContent = '儲存失敗：HTTP ' + resp.status;
      return;
    }
    this.tokenInput.value = '';
    this.status.textContent = '已儲存。Telegram 服務會自動套用設定。';
    await this.loadTelegram();
  }
}
```

- [ ] **Step 2: Add Chat model indicator**

In `crates/wukong-web/static/components/wukong-chat.js`, find the toolbar markup in `connectedCallback`. Add this status element near the scope/source controls.

```html
<span id="chat-model-status" class="tag">模型：載入中</span>
<span id="chat-skill-status" class="tag">技能偏好：Phase 2</span>
```

In `connectedCallback`, after existing element assignments, add:

```javascript
    this.modelStatus = this.querySelector('#chat-model-status');
    this.loadModelStatus();
```

Add this method to the class.

```javascript
  async loadModelStatus() {
    if (!this.modelStatus) return;
    const token = window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
    const resp = await fetch('/api/settings/model' + token);
    if (!resp.ok) {
      this.modelStatus.textContent = '模型：未知';
      return;
    }
    const data = await resp.json();
    this.modelStatus.textContent = data.model ? '模型：' + data.model : '模型：agent 預設';
  }
```

- [ ] **Step 3: Run a web package test**

Run these commands:

```bash
cargo test -p wukong-web model_settings_round_trip
cargo test -p wukong-web serves_static_assets_with_content_types
```

Expected: PASS.

- [ ] **Step 4: Commit**

Run:

```bash
git add crates/wukong-web/static/components/wukong-settings.js crates/wukong-web/static/components/wukong-chat.js
git commit -m "feat(web): surface global model setting"
```

---

### Task 7: Final Verification

**Files:**

- No planned source changes.

- [ ] **Step 1: Run focused package tests**

Run:

```bash
cargo test -p wukong-memory
cargo test -p wukong-web
```

Expected: both pass.

- [ ] **Step 2: Run clippy for touched packages**

Run:

```bash
cargo clippy -p wukong-memory -p wukong-web --all-targets -- -D warnings
```

Expected: no warnings.

- [ ] **Step 3: Run full workspace test if time allows**

Run: `cargo test`

Expected: pass. If it is too slow or blocked by environment/provider setup, record the exact failure and keep focused package tests as required evidence.

- [ ] **Step 4: Inspect final diff**

Run:

```bash
git status --short
git diff --stat HEAD
```

Expected: no uncommitted source changes if each task committed successfully.

- [ ] **Step 5: Run GitNexus change detection before final handoff**

Run GitNexus `detect_changes` with scope `all`.

Expected: changed symbols map to memory record listing and web control center APIs/UI only. Risk should be low or medium; investigate any high/critical result before declaring completion.
