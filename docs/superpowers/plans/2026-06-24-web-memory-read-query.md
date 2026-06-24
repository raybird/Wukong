# Web Memory Read Query Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a read-only recall query card to the Web Memory panel and a protected recall-preview API that shows what Wukong memory would recall for a query.

**Architecture:** Extend the existing `wukong-web` memory API module with request/response validation helpers, then add a Web handler that calls `Memory::recall` with `RecallMode::Hybrid`. Update the zero-build `wukong-memory` Web Component to POST recall queries and render hit cards without adding any memory mutation operations.

**Tech Stack:** Rust, axum, serde, `wukong-memory`, plain ES modules/Web Components, existing Web token protection, Cargo tests, `node --check`.

---

## File Structure

- Modify `crates/wukong-web/src/memory_api.rs`: request/response structs and validation helpers for recall preview.
- Modify `crates/wukong-web/src/lib.rs`: new `POST /api/memory/recall-preview` route, handler, backend tests.
- Modify `crates/wukong-web/static/components/wukong-memory.js`: recall query UI and result rendering.
- Do not modify memory maintenance, prune, consolidate, export, or scheduler code.

## Task 1: Add Recall Preview API Types And Validation

**Files:**
- Modify: `crates/wukong-web/src/memory_api.rs`

- [ ] **Step 1: Impact analysis before editing**

Run:

```text
gitnexus_impact({ target: "parse_kind", direction: "upstream", file_path: "crates/wukong-web/src/memory_api.rs", kind: "Function", repo: "Wukong" })
```

Expected: LOW or MEDIUM. If HIGH/CRITICAL, report the blast radius before editing.

- [ ] **Step 2: Write failing tests for validation helpers**

Append this test module to `crates/wukong-web/src/memory_api.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_recall_query_rejects_blank_query() {
        let req = RecallPreviewRequest {
            query: "  ".to_string(),
            scope: None,
            top_k: None,
            mode: None,
        };

        let err = normalize_recall_request(req, "project:Wukong").unwrap_err();

        assert!(err.contains("query is required"));
    }

    #[test]
    fn normalize_recall_query_defaults_scope_top_k_and_mode() {
        let req = RecallPreviewRequest {
            query: " schedule memory ".to_string(),
            scope: None,
            top_k: None,
            mode: None,
        };

        let normalized = normalize_recall_request(req, "project:Wukong").unwrap();

        assert_eq!(normalized.query, "schedule memory");
        assert_eq!(normalized.scope, "project:Wukong");
        assert_eq!(normalized.top_k, 8);
        assert_eq!(normalized.mode, "hybrid");
    }

    #[test]
    fn normalize_recall_query_clamps_top_k() {
        let req = RecallPreviewRequest {
            query: "memory".to_string(),
            scope: Some("global".to_string()),
            top_k: Some(100),
            mode: Some("hybrid".to_string()),
        };

        let normalized = normalize_recall_request(req, "project:Wukong").unwrap();

        assert_eq!(normalized.top_k, 20);
    }

    #[test]
    fn normalize_recall_query_rejects_unknown_mode() {
        let req = RecallPreviewRequest {
            query: "memory".to_string(),
            scope: None,
            top_k: None,
            mode: Some("keyword".to_string()),
        };

        let err = normalize_recall_request(req, "project:Wukong").unwrap_err();

        assert!(err.contains("unsupported recall mode"));
    }
}
```

- [ ] **Step 3: Run tests to verify RED**

Run:

```bash
cargo test -p wukong-web memory_api::tests -- --nocapture
```

Expected: FAIL because `RecallPreviewRequest` and `normalize_recall_request` do not exist.

- [ ] **Step 4: Add minimal implementation**

Add these imports and types to `crates/wukong-web/src/memory_api.rs`:

```rust
use serde::{Deserialize, Serialize};
use wukong_memory::{MemoryKind, MemoryRecordsPage, RecallHit, Snapshot};
```

Replace the existing `use serde::Deserialize;` and `use wukong_memory::{MemoryKind, MemoryRecordsPage, Snapshot};` imports with the combined imports above.

Add below `MemoryRecordsQuery`:

```rust
#[derive(Debug, Deserialize)]
pub struct RecallPreviewRequest {
    pub query: String,
    pub scope: Option<String>,
    pub top_k: Option<usize>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRecallRequest {
    pub query: String,
    pub scope: String,
    pub top_k: usize,
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub struct RecallPreviewResponse {
    pub query: String,
    pub scope: String,
    pub mode: String,
    pub hits: Vec<RecallHit>,
    pub evidence: Vec<String>,
    pub confidence: f64,
    pub latency_ms: u64,
}
```

Add below `capped_records_limit`:

```rust
pub fn normalize_recall_request(
    req: RecallPreviewRequest,
    default_scope: &str,
) -> Result<NormalizedRecallRequest, String> {
    let query = req.query.trim().to_string();
    if query.is_empty() {
        return Err("query is required".to_string());
    }

    let scope = req
        .scope
        .map(|scope| scope.trim().to_string())
        .filter(|scope| !scope.is_empty())
        .unwrap_or_else(|| default_scope.to_string());
    let top_k = req.top_k.unwrap_or(8).clamp(1, 20);
    let mode = req
        .mode
        .map(|mode| mode.trim().to_ascii_lowercase())
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "hybrid".to_string());
    if mode != "hybrid" {
        return Err(format!("unsupported recall mode: {mode}"));
    }

    Ok(NormalizedRecallRequest {
        query,
        scope,
        top_k,
        mode,
    })
}
```

- [ ] **Step 5: Run tests to verify GREEN**

Run:

```bash
cargo test -p wukong-web memory_api::tests -- --nocapture
```

Expected: PASS with 4 validation tests.

- [ ] **Step 6: Commit Task 1**

Run:

```bash
git status --short
git diff -- crates/wukong-web/src/memory_api.rs
git add crates/wukong-web/src/memory_api.rs
git commit -m "feat(web): add memory recall preview types"
```

## Task 2: Add Protected Recall Preview Handler

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`
- Modify: `crates/wukong-web/src/memory_api.rs` only if Task 1 types need small compile fixes

- [ ] **Step 1: Impact analysis before editing**

Run:

```text
gitnexus_impact({ target: "build_router", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", kind: "Function", repo: "Wukong" })
gitnexus_impact({ target: "get_memory_records", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", kind: "Function", repo: "Wukong" })
```

Expected: `build_router` may be HIGH/CRITICAL because it defines all routes. Report the risk before editing and proceed only because this task adds one protected route with tests.

- [ ] **Step 2: Write failing API tests**

Add these tests near existing memory tests in `crates/wukong-web/src/lib.rs`:

```rust
#[tokio::test]
async fn memory_recall_preview_returns_hits() {
    let app_state = state(None, &[]).await;
    app_state
        .memory
        .remember(wukong_memory::RememberInput {
            scope: "project:Wukong".to_string(),
            session_id: None,
            items: vec![wukong_memory::MemoryItem {
                kind: wukong_memory::MemoryKind::Note,
                text: "scheduler memory query note".to_string(),
                importance: Some(0.8),
            }],
        })
        .await
        .unwrap();
    let app = build_router(app_state);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/memory/recall-preview")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"query":"scheduler memory","scope":"project:Wukong","top_k":5,"mode":"hybrid"}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("scheduler memory query note"), "body: {body}");
    assert!(body.contains("\"mode\":\"hybrid\""), "body: {body}");
}

#[tokio::test]
async fn memory_recall_preview_rejects_blank_query() {
    let app = build_router(state(None, &[]).await);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/memory/recall-preview")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(body_string(resp).await.contains("query is required"));
}

#[tokio::test]
async fn memory_recall_preview_rejects_unknown_mode() {
    let app = build_router(state(None, &[]).await);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/memory/recall-preview")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"memory","mode":"keyword"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    assert!(body_string(resp).await.contains("unsupported recall mode"));
}

#[tokio::test]
async fn memory_recall_preview_requires_token_when_set() {
    let app = build_router(state(Some("sekret"), &[]).await);

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/memory/recall-preview")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"memory"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

- [ ] **Step 3: Run tests to verify RED**

Run:

```bash
cargo test -p wukong-web memory_recall_preview -- --nocapture
```

Expected: FAIL because `/api/memory/recall-preview` returns 404.

- [ ] **Step 4: Add handler**

Add this import near the existing memory imports in `crates/wukong-web/src/lib.rs`:

```rust
use wukong_memory::{Memory, RecallMode, RecallQuery};
```

Replace the existing `use wukong_memory::Memory;` import with the import above.

Add this handler near `get_memory_records`:

```rust
async fn post_memory_recall_preview<B>(
    State(state): State<AppState<B>>,
    Query(query): Query<SettingsQuery>,
    Json(req): Json<memory_api::RecallPreviewRequest>,
) -> Result<Json<memory_api::RecallPreviewResponse>, (StatusCode, String)>
where
    B: AiBackend + Send + Sync + 'static,
{
    if !authorized(&state.token, query.token.as_deref()) {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    }
    let normalized = memory_api::normalize_recall_request(req, &state.scope)
        .map_err(|err| (StatusCode::BAD_REQUEST, err))?;
    let result = state
        .memory
        .recall(RecallQuery {
            query: normalized.query.clone(),
            top_k: normalized.top_k,
            scope: Some(normalized.scope.clone()),
            mode: RecallMode::Hybrid,
        })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(Json(memory_api::RecallPreviewResponse {
        query: normalized.query,
        scope: normalized.scope,
        mode: normalized.mode,
        hits: result.data,
        evidence: result.evidence,
        confidence: result.confidence,
        latency_ms: result.latency_ms,
    }))
}
```

Add this route in `build_router` next to other memory routes:

```rust
.route(
    "/api/memory/recall-preview",
    axum::routing::post(post_memory_recall_preview::<B>),
)
```

- [ ] **Step 5: Run tests to verify GREEN**

Run:

```bash
cargo test -p wukong-web memory_recall_preview -- --nocapture
```

Expected: PASS with 4 tests.

- [ ] **Step 6: Run full web tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 7: GitNexus detect changes and commit Task 2**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Review changed symbols and affected processes. Then run:

```bash
git status --short
git diff -- crates/wukong-web/src/lib.rs crates/wukong-web/src/memory_api.rs
git add crates/wukong-web/src/lib.rs crates/wukong-web/src/memory_api.rs
git commit -m "feat(web): add memory recall preview API"
```

## Task 3: Add Recall Query UI To Memory Panel

**Files:**
- Modify: `crates/wukong-web/static/components/wukong-memory.js`

- [ ] **Step 1: Impact analysis before editing**

Run:

```text
gitnexus_impact({ target: "WukongMemory", direction: "upstream", file_path: "crates/wukong-web/static/components/wukong-memory.js", kind: "Class", repo: "Wukong" })
```

Expected: LOW; likely direct consumer is `crates/wukong-web/static/app.js`.

- [ ] **Step 2: Add recall UI markup**

In `connectedCallback`, insert this section after the scope/kind filter card and before `<div id="memory-records" class="record-list"></div>`:

```javascript
        <section class="control-card">
          <h3>Recall 查詢</h3>
          <p class="panel-help">只讀查詢：顯示 Wukong 會從目前 scope 想起哪些記憶，不會修改記憶資料。</p>
          <div class="control-row">
            <label>Top K <input id="recall-top-k" type="number" min="1" max="20" value="8"></label>
            <span class="tag">mode hybrid</span>
          </div>
          <textarea id="recall-query" rows="3" placeholder="輸入要查詢的記憶線索…"></textarea>
          <div class="control-row">
            <button id="run-recall" type="button">查詢記憶</button>
          </div>
          <div id="recall-status" class="settings-status"></div>
          <div id="recall-results" class="record-list"></div>
        </section>
```

Add element references below existing `this.kindSelect`:

```javascript
    this.recallQuery = this.querySelector('#recall-query');
    this.recallTopK = this.querySelector('#recall-top-k');
    this.recallStatus = this.querySelector('#recall-status');
    this.recallResults = this.querySelector('#recall-results');
```

Add event listener after the refresh listener:

```javascript
    this.querySelector('#run-recall').addEventListener('click', () => this.runRecall());
```

- [ ] **Step 3: Add recall methods**

Add these methods before the closing class brace:

```javascript
  async runRecall() {
    const query = this.recallQuery.value.trim();
    if (!query) {
      this.recallStatus.textContent = '請先輸入查詢內容。';
      this.recallResults.innerHTML = '';
      return;
    }
    const topK = Number.parseInt(this.recallTopK.value || '8', 10);
    this.recallStatus.textContent = '查詢中…';
    this.recallResults.innerHTML = '';
    const params = new URLSearchParams();
    if (window.WUKONG_TOKEN) params.set('token', window.WUKONG_TOKEN);
    const resp = await fetch('/api/memory/recall-preview' + (params.toString() ? '?' + params.toString() : ''), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify({
        query,
        scope: this.scopeSelect.value ? decodeURIComponent(this.scopeSelect.value) : undefined,
        top_k: Number.isFinite(topK) ? topK : 8,
        mode: 'hybrid',
      }),
    });
    if (!resp.ok) {
      this.recallStatus.textContent = '查詢失敗：HTTP ' + resp.status + ' ' + await resp.text();
      return;
    }
    const data = await resp.json();
    this.renderRecallResults(data);
  }

  renderRecallResults(data) {
    const hits = data.hits || [];
    this.recallStatus.textContent = '命中 ' + hits.length + ' 筆 · confidence ' + data.confidence + ' · ' + data.latency_ms + 'ms';
    this.recallResults.innerHTML = hits.map((hit) => html`
      <article class="record-card">
        <div><span class="tag">${hit.scope}</span> <span class="tag">${hit.kind}</span> <span class="tag">score ${Number(hit.score).toFixed(3)}</span></div>
        <p>${hit.text}</p>
      </article>
    `.toString()).join('') || '<p class="empty-state">沒有符合的記憶。</p>';
  }
```

- [ ] **Step 4: Check JavaScript syntax**

Run:

```bash
node --check crates/wukong-web/static/components/wukong-memory.js
```

Expected: no output and exit 0.

- [ ] **Step 5: Run web tests**

Run:

```bash
cargo test -p wukong-web
```

Expected: PASS.

- [ ] **Step 6: GitNexus detect changes and commit Task 3**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Review changed symbols and affected processes. Then run:

```bash
git status --short
git diff -- crates/wukong-web/static/components/wukong-memory.js
git add crates/wukong-web/static/components/wukong-memory.js
git commit -m "feat(web): add memory recall query UI"
```

## Task 4: Final Verification

**Files:**
- No production edits expected unless verification reveals an issue.

- [ ] **Step 1: Run formatting**

Run:

```bash
cargo fmt
```

Expected: no errors. If `cargo fmt` changes unrelated Rust files, inspect `git diff --stat`; commit formatting separately only if it is pure rustfmt output.

- [ ] **Step 2: Run targeted tests**

Run:

```bash
cargo test -p wukong-web memory_recall_preview -- --nocapture
cargo test -p wukong-web memory_api::tests -- --nocapture
```

Expected: PASS.

- [ ] **Step 3: Run full verification**

Run:

```bash
node --check crates/wukong-web/static/components/wukong-memory.js
cargo test -p wukong-web
cargo test
cargo clippy --all-targets -- -D warnings
```

Expected: all commands pass with 0 failures and no clippy warnings.

- [ ] **Step 4: Final GitNexus and git checks**

Run:

```text
gitnexus_detect_changes({ scope: "all", repo: "Wukong" })
```

Then run:

```bash
git status --short
git log --oneline -10
```

Expected: no uncommitted changes after all commits. If verification caused formatting changes, commit them with `style: format rust code` after inspecting the diff.
