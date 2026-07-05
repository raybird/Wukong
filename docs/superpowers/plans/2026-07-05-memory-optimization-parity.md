# Memory Optimization Parity Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Improve Wukong memory recall quality, write correctness, migration safety, HTTP validation, telemetry, and confidence semantics according to `docs/superpowers/specs/2026-07-05-memory-optimization-parity-design.md`.

**Architecture:** Keep behavior centered in `wukong-memory`; transports only validate and forward typed inputs. Add small helpers and store methods rather than introducing a migration framework or telemetry service. Preserve current ranking score while adding decay-free relevance for response confidence and telemetry.

**Tech Stack:** Rust, Tokio, SQLx SQLite/FTS5, Axum, Serde, existing Cargo workspace tests.

---

## File Structure

- Modify `crates/wukong-memory/src/recall/mod.rs`: CJK query classification, fallback source helpers, per-hit relevance in ranking.
- Modify `crates/wukong-memory/src/model.rs`: strict external input fields, optional `dedupe_key`, recall telemetry structs, optional `RecallExplanation.relevance`.
- Modify `crates/wukong-memory/src/store/mod.rs`: schema additions, deduped insert, CJK fallback candidate query, telemetry insert/read helpers, migration backfill tests.
- Modify `crates/wukong-memory/src/lib.rs`: remember validation/dedupe flow, CJK fallback orchestration, confidence from relevance, telemetry writes.
- Modify `crates/wukong-memory/tests/integration.rs`: end-to-end memory tests for Chinese recall, dedupe, old DB upgrade, telemetry.
- Modify `crates/wukong-memoryd/src/lib.rs`: map Axum JSON rejections to `400`, validate blank recall query and empty items through core behavior.
- Modify `crates/wukong-memoryd/tests/http.rs`: HTTP contract tests for malformed and semantically invalid payloads.
- Modify `crates/wukong-runtime/src/turn.rs`: pass stable dedupe keys for generated runtime turn memories.
- Modify `crates/wukong-gateway/src/pipeline.rs`: pass stable dedupe keys for generated gateway turn memories.

## Task 1: CJK-Aware Recall Gate And Fallback

**Files:**
- Modify: `crates/wukong-memory/src/recall/mod.rs`
- Modify: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`
- Test: `crates/wukong-memory/tests/integration.rs`

- [ ] **Step 1: Write failing CJK recall tests**

Add these tests to `crates/wukong-memory/tests/integration.rs`:

```rust
#[tokio::test]
async fn short_cjk_query_is_not_treated_as_trivial() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("連線池設定使用 SQLite WAL")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "連線池設定".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(
        res.data.iter().any(|hit| hit.text.contains("連線池設定")),
        "expected CJK query to recall the Chinese memory, got: {:?}",
        res.data.iter().map(|hit| &hit.text).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn low_information_cjk_query_is_skipped() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("可以部署的資料庫設定")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "可以".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(res.data.is_empty());
    assert_eq!(res.confidence, 0.0);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p wukong-memory cjk`

Expected: at least `short_cjk_query_is_not_treated_as_trivial` fails before implementation because the current FTS/tokenization path does not guarantee this Chinese recall case.

- [ ] **Step 3: Add CJK classification helpers**

In `crates/wukong-memory/src/recall/mod.rs`, replace `is_trivial()` with helper-based logic:

```rust
const LOW_INFORMATION_CJK: &[&str] = &["好", "嗯", "可以", "謝謝", "谢谢", "了解", "收到"];

pub fn contains_cjk(query: &str) -> bool {
    query.chars().any(|ch| {
        matches!(
            ch as u32,
            0x3400..=0x4DBF
                | 0x4E00..=0x9FFF
                | 0xF900..=0xFAFF
                | 0x3040..=0x30FF
                | 0xAC00..=0xD7AF
        )
    })
}

pub fn is_low_information_cjk(query: &str) -> bool {
    let normalized = query.trim();
    LOW_INFORMATION_CJK.contains(&normalized)
}

pub fn cjk_weighted_len(query: &str) -> usize {
    query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| if contains_cjk(&ch.to_string()) { 2 } else { 1 })
        .sum()
}

pub fn is_trivial(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return true;
    }
    if contains_cjk(trimmed) {
        return is_low_information_cjk(trimmed) || cjk_weighted_len(trimmed) < 4;
    }
    if trimmed.chars().count() < 3 {
        return true;
    }
    let tokens = tokenize(trimmed);
    tokens.is_empty() || tokens.iter().all(|t| STOPWORDS.contains(&t.as_str()))
}
```

Add unit assertions near `trivial_queries_detected()`:

```rust
assert!(!is_trivial("連線池設定"));
assert!(is_trivial("可以"));
assert!(contains_cjk("連線池設定"));
```

- [ ] **Step 4: Add CJK fallback candidate query**

In `crates/wukong-memory/src/store/mod.rs`, add a method near `keyword_candidates()`:

```rust
pub async fn cjk_fallback_candidates(&self, query: &str, limit: i64) -> Result<Vec<Candidate>> {
    let pattern = format!("%{}%", query.trim());
    let rows = sqlx::query(
        "SELECT id, scope, kind, text, created_at, recall_count, importance,
                NULL AS bm25
         FROM memories
         WHERE text LIKE ?1
         ORDER BY created_at DESC
         LIMIT ?2",
    )
    .bind(pattern)
    .bind(limit)
    .fetch_all(&self.pool)
    .await?;

    Ok(rows
        .into_iter()
        .map(|r| {
            let mut c = row_to_candidate(r);
            c.source_signals.push("cjk_fallback".to_string());
            c
        })
        .collect())
}
```

- [ ] **Step 5: Wire fallback into recall**

In `crates/wukong-memory/src/lib.rs`, import `contains_cjk` from `recall` and change keyword fetch to:

```rust
let keyword = if use_keyword {
    match fts_match_string(&query.query) {
        Some(expr) => {
            let hits = self.store.keyword_candidates(&expr, limit).await?;
            if hits.is_empty() && contains_cjk(&query.query) {
                self.store.cjk_fallback_candidates(&query.query, limit).await?
            } else {
                hits
            }
        }
        None => Vec::new(),
    }
} else {
    Vec::new()
};
```

- [ ] **Step 6: Run focused tests**

Run: `cargo test -p wukong-memory cjk`

Expected: all named tests pass.

- [ ] **Step 7: Commit Task 1**

Run:

```bash
git add crates/wukong-memory/src/recall/mod.rs crates/wukong-memory/src/store/mod.rs crates/wukong-memory/src/lib.rs crates/wukong-memory/tests/integration.rs
git commit -m "fix(memory): support short CJK recall queries"
```

## Task 2: Deduped Writes And Migration Backfill Coverage

**Files:**
- Modify: `crates/wukong-memory/src/model.rs`
- Modify: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`
- Modify: `crates/wukong-runtime/src/turn.rs`
- Modify: `crates/wukong-gateway/src/pipeline.rs`
- Test: `crates/wukong-memory/tests/integration.rs`

- [ ] **Step 1: Write failing dedupe test**

Add to `crates/wukong-memory/tests/integration.rs`:

```rust
#[tokio::test]
async fn duplicate_dedupe_key_returns_existing_id() {
    let mem = open_memory().await;
    let input = RememberInput {
        scope: "global".to_string(),
        session_id: Some("turn-1".to_string()),
        items: vec![MemoryItem {
            kind: MemoryKind::Event,
            text: "User: deploy it".to_string(),
            importance: None,
            dedupe_key: Some("runtime:turn-1:user".to_string()),
        }],
    };

    let first = mem.remember(input.clone()).await.unwrap();
    let second = mem.remember(input).await.unwrap();

    assert_eq!(first.data, second.data);
    let stats = mem.stats().await.unwrap();
    assert_eq!(stats.total, 1);
}
```

- [ ] **Step 2: Write failing populated old DB migration test**

Add to `crates/wukong-memory/tests/integration.rs`:

```rust
#[tokio::test]
async fn populated_old_database_upgrades_and_remains_recallable() {
    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);

    {
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::raw_sql(
            r#"
            CREATE TABLE sessions (
                id TEXT PRIMARY KEY,
                scope TEXT NOT NULL,
                project TEXT,
                created_at INTEGER NOT NULL,
                summary TEXT
            );
            CREATE TABLE memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT,
                scope TEXT NOT NULL,
                kind TEXT NOT NULL,
                text TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                last_recalled_at INTEGER,
                recall_count INTEGER NOT NULL DEFAULT 0,
                importance REAL NOT NULL DEFAULT 1.0
            );
            CREATE VIRTUAL TABLE memories_fts USING fts5(text, content='memories', content_rowid='id');
            CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
            END;
            INSERT INTO memories (scope, kind, text, created_at, importance)
            VALUES ('global', 'note', 'legacy sqlite migration memory', 100, 1.0);
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    let mem = Memory::open(&url).await.unwrap();
    let hits = mem
        .recall(RecallQuery {
            query: "sqlite migration".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(hits.data.iter().any(|hit| hit.text.contains("legacy")));
    drop(mem);
    let reopened = Memory::open(&url).await.unwrap();
    assert_eq!(reopened.stats().await.unwrap().total, 1);
}
```

Also add `sqlx = { workspace = true }` to `[dev-dependencies]` in `crates/wukong-memory/Cargo.toml` if the integration test cannot see `sqlx`.

- [ ] **Step 3: Run tests and verify failure**

Run: `cargo test -p wukong-memory duplicate_dedupe_key_returns_existing_id`

Then run: `cargo test -p wukong-memory populated_old_database_upgrades_and_remains_recallable`

Expected: dedupe test fails to compile because `dedupe_key` does not exist; migration test may fail if old FTS data is not rebuilt.

- [ ] **Step 4: Add `dedupe_key` to memory input and schema**

In `crates/wukong-memory/src/model.rs`, change `MemoryItem`:

```rust
pub struct MemoryItem {
    pub kind: MemoryKind,
    pub text: String,
    #[serde(default)]
    pub importance: Option<f64>,
    #[serde(default)]
    pub dedupe_key: Option<String>,
}
```

Update test helper `item()` to set `dedupe_key: None`.

In `crates/wukong-memory/src/store/mod.rs`, add to migration:

```rust
if !names.iter().any(|n| n == "dedupe_key") {
    sqlx::query("ALTER TABLE memories ADD COLUMN dedupe_key TEXT")
        .execute(pool)
        .await?;
}
sqlx::query(
    "CREATE UNIQUE INDEX IF NOT EXISTS memories_dedupe_key_idx
     ON memories(dedupe_key)
     WHERE dedupe_key IS NOT NULL",
)
.execute(pool)
.await?;
```

- [ ] **Step 5: Implement deduped insert**

Change `insert_memory()` signature to accept `dedupe_key: Option<&str>` and use select-after-conflict:

```rust
if let Some(key) = dedupe_key {
    let existing = sqlx::query("SELECT id FROM memories WHERE dedupe_key = ?1")
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
    if let Some(row) = existing {
        return Ok(row.get::<i64, _>("id"));
    }
}

let row = sqlx::query(
    "INSERT INTO memories (session_id, scope, kind, text, created_at, importance, dedupe_key)
     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
     RETURNING id",
)
.bind(session_id)
.bind(scope)
.bind(kind.as_str())
.bind(text)
.bind(now)
.bind(importance)
.bind(dedupe_key)
.fetch_one(&self.pool)
.await?;
```

Update all `insert_memory()` call sites to pass `item.dedupe_key.as_deref()` or `None` for generated summaries.

- [ ] **Step 6: Avoid duplicate side effects**

In `Memory::remember()`, after `insert_memory()`, only embed and markdown-mirror if the returned id was newly inserted. If the select-after-conflict pattern does not expose this, add a `Store::memory_has_embedding(id)` helper and skip markdown for deduped rows by returning `(id, inserted)` from `insert_memory()`.

Use this shape:

```rust
let (id, inserted) = self
    .store
    .insert_memory(
        input.session_id.as_deref(),
        &scope_str,
        item.kind,
        &item.text,
        importance,
        now,
        item.dedupe_key.as_deref(),
    )
    .await?;
if inserted {
    if let Some(emb) = &self.embedder {
        if let Ok(v) = emb.embed(&item.text) {
            let _ = self.store.update_embedding(id, &embedding_to_blob(&v), emb.model_id()).await;
        }
    }
    if let Some(sink) = &self.md_sink {
        let _ = sink.append(&scope_str, now, item.kind, &item.text);
    }
}
```

- [ ] **Step 7: Add stable runtime/gateway dedupe keys**

In `crates/wukong-runtime/src/turn.rs`, set keys for the final persisted turn:

```rust
let turn_key = captured_session
    .clone()
    .or_else(|| stored.clone())
    .unwrap_or_else(|| format!("scope:{}:input:{}", cfg.scope, input));
```

Then set `dedupe_key` on generated items:

```rust
dedupe_key: Some(format!("runtime:{turn_key}:user")),
dedupe_key: Some(format!("runtime:{turn_key}:assistant")),
```

In `crates/wukong-gateway/src/pipeline.rs`, use:

```rust
let turn_key = format!("gateway:{}:{}", cfg.scope, input);
```

and set `dedupe_key` on its generated user/assistant event items.

- [ ] **Step 8: Run focused tests**

Run: `cargo test -p wukong-memory duplicate_dedupe_key_returns_existing_id`

Then run: `cargo test -p wukong-memory populated_old_database_upgrades_and_remains_recallable`

Expected: both pass.

- [ ] **Step 9: Commit Task 2**

Run:

```bash
git add crates/wukong-memory crates/wukong-runtime/src/turn.rs crates/wukong-gateway/src/pipeline.rs
git commit -m "fix(memory): make generated writes idempotent"
```

## Task 3: HTTP Boundary Validation

**Files:**
- Modify: `crates/wukong-memory/src/model.rs`
- Modify: `crates/wukong-memory/src/lib.rs`
- Modify: `crates/wukong-memoryd/src/lib.rs`
- Test: `crates/wukong-memoryd/tests/http.rs`

- [ ] **Step 1: Add HTTP contract tests**

Add to `crates/wukong-memoryd/tests/http.rs`:

```rust
#[tokio::test]
async fn malformed_remember_payload_returns_400() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remember")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"scope":"global","items":"bad"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unknown_memory_kind_returns_400() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remember")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"scope":"global","items":[{"kind":"unknown","text":"x"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn empty_items_returns_400() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remember")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"scope":"global","items":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn blank_recall_query_returns_400() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/recall")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p wukong-memoryd returns_400`

Expected: at least malformed JSON extractor and blank/empty semantic validation fail before implementation.

- [ ] **Step 3: Add Axum JSON rejection handling**

In `crates/wukong-memoryd/src/lib.rs`, add `JsonRejection` and handler signatures:

```rust
use axum::extract::rejection::JsonRejection;

async fn remember(
    State(mem): State<Arc<Memory>>,
    input: std::result::Result<Json<RememberInput>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let Json(input) = input.map_err(|err| AppError(MemoryError::InvalidQuery(err.to_string())))?;
    Ok(Json(mem.remember(input).await?))
}

async fn recall(
    State(mem): State<Arc<Memory>>,
    query: std::result::Result<Json<RecallQuery>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let Json(query) = query.map_err(|err| AppError(MemoryError::InvalidQuery(err.to_string())))?;
    Ok(Json(mem.recall(query).await?))
}
```

- [ ] **Step 4: Add core semantic validation**

In `Memory::remember()` before session upsert:

```rust
if input.items.is_empty() {
    return Err(MemoryError::InvalidQuery("remember requires at least one item".to_string()));
}
if input.items.iter().any(|item| item.text.trim().is_empty()) {
    return Err(MemoryError::InvalidQuery("memory item text is required".to_string()));
}
```

In `Memory::recall()` before `is_trivial()`:

```rust
if query.query.trim().is_empty() {
    return Err(MemoryError::InvalidQuery("recall query is required".to_string()));
}
```

- [ ] **Step 5: Run focused tests**

Run: `cargo test -p wukong-memoryd returns_400`

Expected: all pass.

- [ ] **Step 6: Commit Task 3**

Run:

```bash
git add crates/wukong-memory/src/lib.rs crates/wukong-memoryd/src/lib.rs crates/wukong-memoryd/tests/http.rs
git commit -m "fix(memoryd): validate memory API boundaries"
```

## Task 4: Recall Telemetry And Confidence Relevance

**Files:**
- Modify: `crates/wukong-memory/src/model.rs`
- Modify: `crates/wukong-memory/src/recall/mod.rs`
- Modify: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`
- Test: `crates/wukong-memory/tests/integration.rs`

- [ ] **Step 1: Add failing telemetry/confidence tests**

Add to `crates/wukong-memory/tests/integration.rs`:

```rust
#[tokio::test]
async fn recall_confidence_uses_decay_free_relevance() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("sqlite migration confidence check")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "sqlite migration".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(!res.data.is_empty());
    assert_eq!(res.confidence, res.data[0].explanation.relevance);
}

#[tokio::test]
async fn recall_telemetry_records_hit_and_skip_cases() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("telemetry sqlite row")],
    })
    .await
    .unwrap();

    let _hit = mem
        .recall(RecallQuery {
            query: "telemetry sqlite".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();
    let _skip = mem
        .recall(RecallQuery {
            query: "of".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    let summary = mem.recall_telemetry_summary().await.unwrap();
    assert_eq!(summary.total_queries, 2);
    assert_eq!(summary.skipped_queries, 1);
    assert!(summary.avg_top_relevance > 0.0);
}
```

- [ ] **Step 2: Run tests and verify failure**

Run: `cargo test -p wukong-memory recall_`

Expected: compile failure because `RecallExplanation.relevance` and `recall_telemetry_summary()` do not exist.

- [ ] **Step 3: Add telemetry/relevance models**

In `crates/wukong-memory/src/model.rs`, add:

```rust
pub relevance: f64,
```

to `RecallExplanation`, and add:

```rust
#[derive(Debug, Clone)]
pub struct RecallTelemetryInput {
    pub scope: Option<String>,
    pub mode: RecallMode,
    pub query_hash: String,
    pub token_count: i64,
    pub skipped: bool,
    pub skipped_reason: Option<String>,
    pub hit_count: i64,
    pub top_score: Option<f64>,
    pub top_relevance: Option<f64>,
    pub latency_ms: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RecallTelemetrySummary {
    pub total_queries: i64,
    pub skipped_queries: i64,
    pub zero_hit_queries: i64,
    pub avg_top_relevance: f64,
}
```

Export these from `crates/wukong-memory/src/lib.rs`.

- [ ] **Step 4: Compute relevance during ranking**

In `crates/wukong-memory/src/recall/mod.rs`, set:

```rust
let relevance = lexical_norm.max(semantic_norm);
```

and include `relevance` in `RecallExplanation`.

- [ ] **Step 5: Add telemetry schema and store helpers**

In `crates/wukong-memory/src/store/mod.rs`, add to `SCHEMA`:

```sql
CREATE TABLE IF NOT EXISTS recall_telemetry (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    created_at INTEGER NOT NULL,
    scope TEXT,
    mode TEXT NOT NULL,
    query_hash TEXT NOT NULL,
    token_count INTEGER NOT NULL,
    skipped INTEGER NOT NULL DEFAULT 0,
    skipped_reason TEXT,
    hit_count INTEGER NOT NULL,
    top_score REAL,
    top_relevance REAL,
    latency_ms INTEGER NOT NULL
);
```

Add methods:

```rust
pub async fn insert_recall_telemetry(&self, now: i64, input: &RecallTelemetryInput) -> Result<()> {
    sqlx::query(
        "INSERT INTO recall_telemetry
         (created_at, scope, mode, query_hash, token_count, skipped, skipped_reason,
          hit_count, top_score, top_relevance, latency_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
    )
    .bind(now)
    .bind(input.scope.as_deref())
    .bind(input.mode.as_str())
    .bind(&input.query_hash)
    .bind(input.token_count)
    .bind(if input.skipped { 1 } else { 0 })
    .bind(input.skipped_reason.as_deref())
    .bind(input.hit_count)
    .bind(input.top_score)
    .bind(input.top_relevance)
    .bind(input.latency_ms)
    .execute(&self.pool)
    .await?;
    Ok(())
}

pub async fn recall_telemetry_summary(&self) -> Result<RecallTelemetrySummary> {
    let row = sqlx::query(
        "SELECT COUNT(*) AS total,
                SUM(CASE WHEN skipped = 1 THEN 1 ELSE 0 END) AS skipped,
                SUM(CASE WHEN skipped = 0 AND hit_count = 0 THEN 1 ELSE 0 END) AS zero_hit,
                AVG(CASE WHEN skipped = 0 THEN top_relevance ELSE NULL END) AS avg_rel
         FROM recall_telemetry",
    )
    .fetch_one(&self.pool)
    .await?;
    Ok(RecallTelemetrySummary {
        total_queries: row.get::<i64, _>("total"),
        skipped_queries: row.get::<Option<i64>, _>("skipped").unwrap_or(0),
        zero_hit_queries: row.get::<Option<i64>, _>("zero_hit").unwrap_or(0),
        avg_top_relevance: row.get::<Option<f64>, _>("avg_rel").unwrap_or(0.0),
    })
}
```

- [ ] **Step 6: Add privacy-preserving query hash**

In `crates/wukong-memory/src/lib.rs`, add a standard-library hash helper:

```rust
fn query_hash(query: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    query.hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}
```

Use it only for telemetry; do not store raw query text.

- [ ] **Step 7: Wire telemetry and confidence into `Memory::recall()`**

For skipped recalls, write telemetry best-effort before returning:

```rust
let now = now_unix();
let telemetry = RecallTelemetryInput {
    scope: query.scope.clone(),
    mode: query.mode,
    query_hash: query_hash(&query.query),
    token_count: recall::tokenize(&query.query).len() as i64,
    skipped: true,
    skipped_reason: Some("trivial".to_string()),
    hit_count: 0,
    top_score: None,
    top_relevance: None,
    latency_ms: start.elapsed().as_millis() as i64,
};
let _ = self.store.insert_recall_telemetry(now, &telemetry).await;
```

For normal recalls, change confidence to:

```rust
let confidence = scored
    .first()
    .map(|s| s.explanation.relevance.clamp(0.0, 1.0))
    .unwrap_or(0.0);
```

Write telemetry after scoring with `top_score` and `top_relevance`:

```rust
let telemetry = RecallTelemetryInput {
    scope: query.scope.clone(),
    mode: query.mode,
    query_hash: query_hash(&query.query),
    token_count: recall::tokenize(&query.query).len() as i64,
    skipped: false,
    skipped_reason: None,
    hit_count: scored.len() as i64,
    top_score: scored.first().map(|s| s.score),
    top_relevance: scored.first().map(|s| s.explanation.relevance),
    latency_ms: start.elapsed().as_millis() as i64,
};
let _ = self.store.insert_recall_telemetry(now, &telemetry).await;
```

Add public method:

```rust
pub async fn recall_telemetry_summary(&self) -> Result<model::RecallTelemetrySummary> {
    self.store.recall_telemetry_summary().await
}
```

- [ ] **Step 8: Run focused tests**

Run: `cargo test -p wukong-memory recall_`

Expected: both pass.

- [ ] **Step 9: Commit Task 4**

Run:

```bash
git add crates/wukong-memory
git commit -m "feat(memory): record recall telemetry"
```

## Task 5: Final Regression And Documentation Alignment

**Files:**
- Modify if needed: `docs/memory.md`
- Modify if needed: `crates/wukong-memory/README.md`

- [ ] **Step 1: Update docs for changed behavior**

If implementation changed public behavior, update `docs/memory.md` and `crates/wukong-memory/README.md` with these facts:

```markdown
- Adaptive gate is CJK-aware: short meaningful Chinese/Japanese/Korean queries recall normally, while low-information confirmations are skipped.
- Response `confidence` represents decay-free top-hit relevance; each hit's `score` remains the final ranking score.
- Generated turn writes may carry dedupe keys so retries are idempotent.
- Recall telemetry records privacy-preserving query hashes, skipped/zero-hit routing, top score, top relevance, and latency.
```

- [ ] **Step 2: Run memory crate tests**

Run: `cargo test -p wukong-memory`

Expected: all tests pass.

- [ ] **Step 3: Run memoryd tests**

Run: `cargo test -p wukong-memoryd`

Expected: all tests pass.

- [ ] **Step 4: Run web tests**

Run: `cargo test -p wukong-web`

Expected: all tests pass.

- [ ] **Step 5: Run full workspace tests**

Run: `cargo test`

Expected: all tests pass.

- [ ] **Step 6: Run GitNexus change detection before final commit**

Run GitNexus with staged scope after staging final docs/test adjustments:

```text
gitnexus_detect_changes({ scope: "staged", repo: "Wukong" })
```

Expected: changed symbols and affected processes match memory/HTTP/runtime surfaces touched by this plan.

- [ ] **Step 7: Commit final docs/regression adjustments**

Run:

```bash
git add docs/memory.md crates/wukong-memory/README.md
git commit -m "docs(memory): document optimization parity behavior"
```

If no docs changed, skip this commit.

## Self-Review Checklist

- Spec coverage: Task 1 covers CJK gate and Chinese recall fallback; Task 2 covers dedupe and populated old DB migration; Task 3 covers HTTP validation; Task 4 covers telemetry and confidence semantics; Task 5 covers regression and docs.
- No host hook adapter work is included, matching non-goals.
- No recall ranking weight tuning is included, matching non-goals.
- The plan uses small commits after independently testable changes.
- Every code-changing task starts with failing tests and includes exact test commands.
