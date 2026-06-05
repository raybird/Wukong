# wukong-memory v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `wukong-memory` v1 persistent memory core — lexical recall (keyword/tree/hybrid), scope isolation, time-decay scoring — exposed as a Rust library crate plus an axum HTTP API.

**Architecture:** A cargo workspace with two crates. `wukong-memory` (lib) holds all logic: domain types, SQLite store (sqlx + FTS5), scoring, and recall. `wukong-memoryd` (lib+bin) is a thin axum HTTP layer over the library. SQLite is the single source of truth; FTS5 powers keyword search; recall ranks candidates by a combined lexical + time-decay + importance score.

**Tech Stack:** Rust, tokio, axum 0.7, sqlx 0.8 (sqlite, bundled), FTS5, serde, thiserror. Dev: tower, tempfile.

---

## File Structure

```
wukong/
├── Cargo.toml                          # workspace manifest
├── .gitignore                          # (exists)
├── crates/
│   ├── wukong-memory/
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs                  # module wiring + public Memory API
│   │       ├── error.rs                # MemoryError + Result alias
│   │       ├── scope.rs                # Scope enum: parse / Display / ancestry
│   │       ├── model.rs                # DTOs: RememberInput, RecallQuery, RecallHit, WukongResult, Stats, MemoryKind, RecallMode
│   │       ├── scoring.rs              # time_decay + combined_score + Weights
│   │       ├── store/
│   │       │   └── mod.rs              # Store: open, schema, insert, candidates, stats, touch_recalled
│   │       └── recall/
│   │           └── mod.rs              # adaptive gate + keyword/tree/hybrid ranking
│   └── wukong-memoryd/
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs                  # Config + build_router + handlers + AppError
│           └── main.rs                 # binary entrypoint
```

Each unit has one responsibility: `scope`/`model` are pure data, `scoring` is pure math, `store` owns all SQL, `recall` orchestrates ranking, `lib.rs` (Memory) is the public facade, and `wukong-memoryd` is transport only.

---

## Task 1: Workspace and crate scaffolding

**Files:**
- Create: `Cargo.toml`
- Create: `crates/wukong-memory/Cargo.toml`
- Create: `crates/wukong-memory/src/lib.rs`
- Create: `crates/wukong-memoryd/Cargo.toml`
- Create: `crates/wukong-memoryd/src/lib.rs`
- Create: `crates/wukong-memoryd/src/main.rs`

- [ ] **Step 1: Create the workspace manifest**

Create `Cargo.toml`:

```toml
[workspace]
resolver = "2"
members = ["crates/wukong-memory", "crates/wukong-memoryd"]

[workspace.package]
edition = "2021"
version = "0.1.0"

[workspace.dependencies]
tokio = { version = "1", features = ["full"] }
axum = "0.7"
sqlx = { version = "0.8", default-features = false, features = ["runtime-tokio", "sqlite"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "1"
tower = "0.5"
tempfile = "3"
```

- [ ] **Step 2: Create the library crate manifest**

Create `crates/wukong-memory/Cargo.toml`:

```toml
[package]
name = "wukong-memory"
edition.workspace = true
version.workspace = true

[dependencies]
tokio = { workspace = true }
sqlx = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Create a minimal lib.rs**

Create `crates/wukong-memory/src/lib.rs`:

```rust
//! wukong-memory: persistent memory core for the Wukong assistant.

#[cfg(test)]
mod smoke_tests {
    #[test]
    fn workspace_builds() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 4: Create the server crate manifest**

Create `crates/wukong-memoryd/Cargo.toml`:

```toml
[package]
name = "wukong-memoryd"
edition.workspace = true
version.workspace = true

[lib]
name = "wukong_memoryd"
path = "src/lib.rs"

[[bin]]
name = "wukong-memoryd"
path = "src/main.rs"

[dependencies]
wukong-memory = { path = "../wukong-memory" }
tokio = { workspace = true }
axum = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }

[dev-dependencies]
tower = { workspace = true }
tempfile = { workspace = true }
```

- [ ] **Step 5: Create placeholder lib.rs and main.rs for the server**

Create `crates/wukong-memoryd/src/lib.rs`:

```rust
//! wukong-memoryd: axum HTTP transport over wukong-memory.
```

Create `crates/wukong-memoryd/src/main.rs`:

```rust
fn main() {
    println!("wukong-memoryd placeholder");
}
```

- [ ] **Step 6: Verify the workspace builds and the smoke test passes**

Run: `cargo test`
Expected: compiles; `smoke_tests::workspace_builds` passes.

- [ ] **Step 7: Commit**

```bash
git add Cargo.toml crates/
git commit -m "chore: scaffold wukong cargo workspace"
```

---

## Task 2: Error type

**Files:**
- Create: `crates/wukong-memory/src/error.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Write the error module with a unit test**

Create `crates/wukong-memory/src/error.rs`:

```rust
use thiserror::Error;

/// All errors produced by the memory core.
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found")]
    NotFound,
    #[error("invalid scope: {0}")]
    InvalidScope(String),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
}

/// Convenience result alias used across the crate.
pub type Result<T> = std::result::Result<T, MemoryError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_scope_message_includes_input() {
        let err = MemoryError::InvalidScope("bogus".to_string());
        assert_eq!(err.to_string(), "invalid scope: bogus");
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-memory/src/lib.rs` with:

```rust
//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;

pub use error::{MemoryError, Result};
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cargo test -p wukong-memory error::`
Expected: `invalid_scope_message_includes_input` passes.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/error.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add MemoryError type"
```

---

## Task 3: Scope type

**Files:**
- Create: `crates/wukong-memory/src/scope.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Write failing tests for scope parsing, display, and ancestry**

Create `crates/wukong-memory/src/scope.rs`:

```rust
use crate::error::{MemoryError, Result};
use std::fmt;

/// Memory isolation boundary. Serialized on the wire as a plain string:
/// `global`, `project:Name`, `agent:Name`, `user:Name`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Scope {
    Global,
    Project(String),
    Agent(String),
    User(String),
}

impl Scope {
    /// Parse a scope string. Returns `InvalidScope` for unknown prefixes or
    /// empty names.
    pub fn parse(s: &str) -> Result<Scope> {
        let s = s.trim();
        if s == "global" {
            return Ok(Scope::Global);
        }
        let (prefix, name) = s
            .split_once(':')
            .ok_or_else(|| MemoryError::InvalidScope(s.to_string()))?;
        if name.is_empty() {
            return Err(MemoryError::InvalidScope(s.to_string()));
        }
        match prefix {
            "project" => Ok(Scope::Project(name.to_string())),
            "agent" => Ok(Scope::Agent(name.to_string())),
            "user" => Ok(Scope::User(name.to_string())),
            _ => Err(MemoryError::InvalidScope(s.to_string())),
        }
    }

    /// The scope itself plus its parent scopes, most-specific first.
    /// Every non-global scope falls back to `global`.
    pub fn ancestry(&self) -> Vec<Scope> {
        match self {
            Scope::Global => vec![Scope::Global],
            other => vec![other.clone(), Scope::Global],
        }
    }
}

impl fmt::Display for Scope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Scope::Global => write!(f, "global"),
            Scope::Project(n) => write!(f, "project:{n}"),
            Scope::Agent(n) => write!(f, "agent:{n}"),
            Scope::User(n) => write!(f, "user:{n}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_global() {
        assert_eq!(Scope::parse("global").unwrap(), Scope::Global);
    }

    #[test]
    fn parses_prefixed_scopes() {
        assert_eq!(
            Scope::parse("project:Wukong").unwrap(),
            Scope::Project("Wukong".to_string())
        );
        assert_eq!(
            Scope::parse("agent:main").unwrap(),
            Scope::Agent("main".to_string())
        );
    }

    #[test]
    fn rejects_unknown_prefix_and_empty_name() {
        assert!(Scope::parse("team:x").is_err());
        assert!(Scope::parse("project:").is_err());
        assert!(Scope::parse("nonsense").is_err());
    }

    #[test]
    fn display_roundtrips() {
        let s = Scope::Project("Wukong".to_string());
        assert_eq!(Scope::parse(&s.to_string()).unwrap(), s);
    }

    #[test]
    fn ancestry_appends_global() {
        let a = Scope::Agent("main".to_string()).ancestry();
        assert_eq!(a, vec![Scope::Agent("main".to_string()), Scope::Global]);
        assert_eq!(Scope::Global.ancestry(), vec![Scope::Global]);
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-memory/src/lib.rs` with:

```rust
//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;
pub mod scope;

pub use error::{MemoryError, Result};
pub use scope::Scope;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p wukong-memory scope::`
Expected: all 5 scope tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/scope.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add Scope type with parsing and ancestry"
```

---

## Task 4: Domain model (DTOs)

**Files:**
- Create: `crates/wukong-memory/src/model.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Write the model module with serde tests**

Create `crates/wukong-memory/src/model.rs`:

```rust
use serde::{Deserialize, Serialize};

/// Category of a stored memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Decision,
    Event,
    Skill,
    Note,
    Summary,
}

impl MemoryKind {
    /// Stable lowercase string used for DB storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryKind::Decision => "decision",
            MemoryKind::Event => "event",
            MemoryKind::Skill => "skill",
            MemoryKind::Note => "note",
            MemoryKind::Summary => "summary",
        }
    }

    /// Parse from a DB string; unknown values fall back to `Note`.
    pub fn from_db_str(s: &str) -> MemoryKind {
        match s {
            "decision" => MemoryKind::Decision,
            "event" => MemoryKind::Event,
            "skill" => MemoryKind::Skill,
            "summary" => MemoryKind::Summary,
            _ => MemoryKind::Note,
        }
    }
}

/// A single memory to persist.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryItem {
    pub kind: MemoryKind,
    pub text: String,
    /// Defaults to 1.0 when omitted (see remember()).
    #[serde(default)]
    pub importance: Option<f64>,
}

/// Input to `remember`.
#[derive(Debug, Clone, Deserialize)]
pub struct RememberInput {
    /// Scope string, e.g. "project:Wukong".
    pub scope: String,
    pub session_id: Option<String>,
    pub items: Vec<MemoryItem>,
}

/// Recall strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecallMode {
    Keyword,
    Tree,
    #[default]
    Hybrid,
}

fn default_top_k() -> usize {
    5
}

/// Input to `recall`.
#[derive(Debug, Clone, Deserialize)]
pub struct RecallQuery {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// Optional scope filter; when set, only this scope + ancestors match.
    pub scope: Option<String>,
    #[serde(default)]
    pub mode: RecallMode,
}

/// A ranked recall result.
#[derive(Debug, Clone, Serialize)]
pub struct RecallHit {
    pub id: i64,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    pub score: f64,
}

/// Provenance entry attached to every result envelope.
#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub id: i64,
    pub scope: String,
    pub score: f64,
}

/// Standard response envelope (mirrors Memoria's MemoriaResult).
#[derive(Debug, Clone, Serialize)]
pub struct WukongResult<T> {
    pub data: T,
    pub evidence: Vec<Evidence>,
    pub confidence: f64,
    pub latency_ms: u64,
}

/// Count of memories within one scope.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeCount {
    pub scope: String,
    pub count: i64,
}

/// Aggregate statistics.
#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub total: i64,
    pub by_scope: Vec<ScopeCount>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_mode_defaults_to_hybrid() {
        assert_eq!(RecallMode::default(), RecallMode::Hybrid);
    }

    #[test]
    fn recall_query_applies_defaults() {
        let q: RecallQuery = serde_json::from_str(r#"{"query":"sqlite"}"#).unwrap();
        assert_eq!(q.top_k, 5);
        assert_eq!(q.mode, RecallMode::Hybrid);
        assert!(q.scope.is_none());
    }

    #[test]
    fn memory_kind_serde_is_snake_case() {
        let json = serde_json::to_string(&MemoryKind::Decision).unwrap();
        assert_eq!(json, "\"decision\"");
        let parsed: MemoryKind = serde_json::from_str("\"skill\"").unwrap();
        assert_eq!(parsed, MemoryKind::Skill);
    }

    #[test]
    fn memory_kind_db_roundtrip() {
        for k in [
            MemoryKind::Decision,
            MemoryKind::Event,
            MemoryKind::Skill,
            MemoryKind::Note,
            MemoryKind::Summary,
        ] {
            assert_eq!(MemoryKind::from_db_str(k.as_str()), k);
        }
        assert_eq!(MemoryKind::from_db_str("garbage"), MemoryKind::Note);
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-memory/src/lib.rs` with:

```rust
//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;
pub mod model;
pub mod scope;

pub use error::{MemoryError, Result};
pub use model::{
    Evidence, MemoryItem, MemoryKind, RecallHit, RecallMode, RecallQuery, RememberInput,
    ScopeCount, Stats, WukongResult,
};
pub use scope::Scope;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p wukong-memory model::`
Expected: all 4 model tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/model.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add domain model DTOs"
```

---

## Task 5: Scoring

**Files:**
- Create: `crates/wukong-memory/src/scoring.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Write failing tests for time decay and combined score**

Create `crates/wukong-memory/src/scoring.rs`:

```rust
/// Relative weights for the combined recall score. The three weights are
/// expected to sum to ~1.0 so the base score stays within [0, 1].
#[derive(Debug, Clone, Copy)]
pub struct Weights {
    pub lexical: f64,
    pub decay: f64,
    pub importance: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            lexical: 0.5,
            decay: 0.3,
            importance: 0.2,
        }
    }
}

const HALF_LIFE_DAYS: f64 = 90.0;

/// Exponential time decay with a 90-day half-life. Returns 1.0 at age 0,
/// 0.5 at 90 days. Negative ages are clamped to 0.
pub fn time_decay(age_seconds: i64, half_life_days: f64) -> f64 {
    let age_days = age_seconds.max(0) as f64 / 86_400.0;
    0.5_f64.powf(age_days / half_life_days)
}

/// Combined recall score. `lexical_norm` and `importance` are expected to be
/// in [0, 1]. Frequently recalled memories get a small logarithmic bonus.
pub fn combined_score(
    lexical_norm: f64,
    age_seconds: i64,
    importance: f64,
    recall_count: i64,
    w: &Weights,
) -> f64 {
    let decay = time_decay(age_seconds, HALF_LIFE_DAYS);
    let base = w.lexical * lexical_norm + w.decay * decay + w.importance * importance;
    base + 0.02 * (1.0 + recall_count.max(0) as f64).ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_is_one_at_zero_age() {
        assert!((time_decay(0, HALF_LIFE_DAYS) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn decay_is_half_at_half_life() {
        let ninety_days = 90 * 86_400;
        assert!((time_decay(ninety_days, HALF_LIFE_DAYS) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn negative_age_clamped() {
        assert!((time_decay(-100, HALF_LIFE_DAYS) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn newer_memory_outranks_older_when_all_else_equal() {
        let w = Weights::default();
        let newer = combined_score(0.5, 0, 1.0, 0, &w);
        let older = combined_score(0.5, 200 * 86_400, 1.0, 0, &w);
        assert!(newer > older);
    }

    #[test]
    fn higher_lexical_match_outranks_lower() {
        let w = Weights::default();
        let strong = combined_score(1.0, 0, 1.0, 0, &w);
        let weak = combined_score(0.1, 0, 1.0, 0, &w);
        assert!(strong > weak);
    }

    #[test]
    fn recall_count_provides_small_bonus() {
        let w = Weights::default();
        let hot = combined_score(0.5, 0, 1.0, 10, &w);
        let cold = combined_score(0.5, 0, 1.0, 0, &w);
        assert!(hot > cold);
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-memory/src/lib.rs` with:

```rust
//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;
pub mod model;
pub mod scope;
pub mod scoring;

pub use error::{MemoryError, Result};
pub use model::{
    Evidence, MemoryItem, MemoryKind, RecallHit, RecallMode, RecallQuery, RememberInput,
    ScopeCount, Stats, WukongResult,
};
pub use scope::Scope;
pub use scoring::Weights;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p wukong-memory scoring::`
Expected: all 6 scoring tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/scoring.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add time-decay and combined scoring"
```

---

## Task 6: Store (SQLite + FTS5)

**Files:**
- Create: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Write the store module with schema, insert, candidates, and stats**

Create `crates/wukong-memory/src/store/mod.rs`:

```rust
use crate::error::Result;
use crate::model::{MemoryKind, ScopeCount, Stats};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

/// Idempotent schema. Applied on every open. External-content FTS5 table is
/// kept in sync by an AFTER INSERT trigger (v1 only inserts).
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id         TEXT PRIMARY KEY,
    scope      TEXT NOT NULL,
    project    TEXT,
    created_at INTEGER NOT NULL,
    summary    TEXT
);
CREATE TABLE IF NOT EXISTS memories (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id       TEXT,
    scope            TEXT NOT NULL,
    kind             TEXT NOT NULL,
    text             TEXT NOT NULL,
    created_at       INTEGER NOT NULL,
    last_recalled_at INTEGER,
    recall_count     INTEGER NOT NULL DEFAULT 0,
    importance       REAL NOT NULL DEFAULT 1.0
);
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    text,
    content='memories',
    content_rowid='id'
);
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
END;
"#;

/// A raw row pulled during recall, before scoring.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: i64,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    pub created_at: i64,
    pub recall_count: i64,
    pub importance: f64,
    /// FTS5 bm25 rank (lower = better match); None for non-keyword sources.
    pub bm25: Option<f64>,
}

/// Owns the SQLite connection pool and all SQL.
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Open (creating if missing) a SQLite database at `db_url`
    /// (e.g. "sqlite://data/memory.db" or "sqlite::memory:") and apply schema.
    pub async fn open(db_url: &str) -> Result<Store> {
        let opts = SqliteConnectOptions::from_str(db_url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal);
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        Ok(Store { pool })
    }

    /// Insert a session row if it does not already exist.
    pub async fn upsert_session(&self, id: &str, scope: &str, now: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id, scope, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(id)
        .bind(scope)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Insert one memory and return its row id.
    pub async fn insert_memory(
        &self,
        session_id: Option<&str>,
        scope: &str,
        kind: MemoryKind,
        text: &str,
        importance: f64,
        now: i64,
    ) -> Result<i64> {
        let row = sqlx::query(
            "INSERT INTO memories (session_id, scope, kind, text, created_at, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
        )
        .bind(session_id)
        .bind(scope)
        .bind(kind.as_str())
        .bind(text)
        .bind(now)
        .bind(importance)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("id"))
    }

    /// Keyword candidates ranked by FTS5 bm25 (best first).
    pub async fn keyword_candidates(
        &self,
        match_expr: &str,
        limit: i64,
    ) -> Result<Vec<Candidate>> {
        let rows = sqlx::query(
            "SELECT m.id, m.scope, m.kind, m.text, m.created_at, m.recall_count, m.importance,
                    bm25(memories_fts) AS bm25
             FROM memories_fts
             JOIN memories m ON m.id = memories_fts.rowid
             WHERE memories_fts MATCH ?1
             ORDER BY bm25 ASC
             LIMIT ?2",
        )
        .bind(match_expr)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_candidate).collect())
    }

    /// Most recent memories (tree/recency source). bm25 is None.
    pub async fn recent_candidates(&self, limit: i64) -> Result<Vec<Candidate>> {
        let rows = sqlx::query(
            "SELECT id, scope, kind, text, created_at, recall_count, importance,
                    NULL AS bm25
             FROM memories
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_candidate).collect())
    }

    /// Bump recall_count and last_recalled_at for the given ids.
    pub async fn touch_recalled(&self, ids: &[i64], now: i64) -> Result<()> {
        for id in ids {
            sqlx::query(
                "UPDATE memories
                 SET recall_count = recall_count + 1, last_recalled_at = ?2
                 WHERE id = ?1",
            )
            .bind(id)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }
        Ok(())
    }

    /// Total memory count and per-scope breakdown.
    pub async fn stats(&self) -> Result<Stats> {
        let total: i64 = sqlx::query("SELECT COUNT(*) AS c FROM memories")
            .fetch_one(&self.pool)
            .await?
            .get::<i64, _>("c");
        let rows = sqlx::query(
            "SELECT scope, COUNT(*) AS c FROM memories GROUP BY scope ORDER BY c DESC",
        )
        .fetch_all(&self.pool)
        .await?;
        let by_scope = rows
            .into_iter()
            .map(|r| ScopeCount {
                scope: r.get::<String, _>("scope"),
                count: r.get::<i64, _>("c"),
            })
            .collect();
        Ok(Stats { total, by_scope })
    }
}

fn row_to_candidate(r: sqlx::sqlite::SqliteRow) -> Candidate {
    Candidate {
        id: r.get::<i64, _>("id"),
        scope: r.get::<String, _>("scope"),
        kind: MemoryKind::from_db_str(&r.get::<String, _>("kind")),
        text: r.get::<String, _>("text"),
        created_at: r.get::<i64, _>("created_at"),
        recall_count: r.get::<i64, _>("recall_count"),
        importance: r.get::<f64, _>("importance"),
        bm25: r.get::<Option<f64>, _>("bm25"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    async fn test_store() -> Store {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        // Leak the temp file handle so it lives for the whole test process.
        std::mem::forget(file);
        Store::open(&url).await.unwrap()
    }

    #[tokio::test]
    async fn fts5_is_available() {
        // Fails loudly if the bundled sqlite lacks FTS5.
        let store = test_store().await;
        store
            .insert_memory(None, "global", MemoryKind::Note, "hello world", 1.0, 100)
            .await
            .unwrap();
        let hits = store.keyword_candidates("\"hello\"", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].bm25.is_some());
    }

    #[tokio::test]
    async fn insert_and_recent() {
        let store = test_store().await;
        store
            .insert_memory(None, "global", MemoryKind::Note, "first", 1.0, 100)
            .await
            .unwrap();
        store
            .insert_memory(None, "global", MemoryKind::Note, "second", 1.0, 200)
            .await
            .unwrap();
        let recent = store.recent_candidates(10).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].text, "second"); // newest first
        assert!(recent[0].bm25.is_none());
    }

    #[tokio::test]
    async fn stats_counts_by_scope() {
        let store = test_store().await;
        store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100)
            .await
            .unwrap();
        store
            .insert_memory(None, "project:X", MemoryKind::Note, "b", 1.0, 100)
            .await
            .unwrap();
        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.by_scope.len(), 2);
    }

    #[tokio::test]
    async fn touch_recalled_bumps_count() {
        let store = test_store().await;
        let id = store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100)
            .await
            .unwrap();
        store.touch_recalled(&[id], 500).await.unwrap();
        let recent = store.recent_candidates(1).await.unwrap();
        assert_eq!(recent[0].recall_count, 1);
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-memory/src/lib.rs` with:

```rust
//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;
pub mod model;
pub mod scope;
pub mod scoring;
pub mod store;

pub use error::{MemoryError, Result};
pub use model::{
    Evidence, MemoryItem, MemoryKind, RecallHit, RecallMode, RecallQuery, RememberInput,
    ScopeCount, Stats, WukongResult,
};
pub use scope::Scope;
pub use scoring::Weights;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p wukong-memory store::`
Expected: `fts5_is_available`, `insert_and_recent`, `stats_counts_by_scope`, `touch_recalled_bumps_count` all pass.

> The `sqlite` feature of sqlx 0.8 bundles `libsqlite3-sys`, which ships with FTS5 enabled — so `fts5_is_available` should pass out of the box. If it ever fails with "no such module: fts5", the build is linking a system sqlite without FTS5; force the bundled build by ensuring no `SQLITE3_LIB_DIR`/`SQLITE3_STATIC` env vars point at a system library. Do not work around it by dropping FTS5 — BM25 keyword ranking depends on it.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/store/mod.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add SQLite store with FTS5"
```

---

## Task 7: Recall (adaptive gate + keyword/tree/hybrid)

**Files:**
- Create: `crates/wukong-memory/src/recall/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Write the recall module with unit tests for the pure helpers**

Create `crates/wukong-memory/src/recall/mod.rs`:

```rust
use crate::model::RecallMode;
use crate::scope::Scope;
use crate::scoring::{combined_score, Weights};
use crate::store::Candidate;

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "of", "to", "and", "it", "in", "on", "for",
];

/// Trivial queries (too short or only stopwords) skip recall entirely.
pub fn is_trivial(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.chars().count() < 3 {
        return true;
    }
    let tokens = tokenize(trimmed);
    tokens.is_empty() || tokens.iter().all(|t| STOPWORDS.contains(&t.as_str()))
}

/// Lowercase alphanumeric tokens.
pub fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Build an FTS5 MATCH expression: each token quoted, OR-joined.
/// Returns None when there are no usable tokens.
pub fn fts_match_string(query: &str) -> Option<String> {
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Merge keyword + recency candidates by id, preferring the keyword row's bm25.
pub fn merge_candidates(keyword: Vec<Candidate>, recent: Vec<Candidate>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = keyword;
    for c in recent {
        if !out.iter().any(|k| k.id == c.id) {
            out.push(c);
        }
    }
    out
}

/// Keep only candidates whose scope is within the filter's ancestry.
pub fn filter_by_scope(candidates: Vec<Candidate>, filter: &Option<Scope>) -> Vec<Candidate> {
    match filter {
        None => candidates,
        Some(scope) => {
            let allowed: Vec<String> =
                scope.ancestry().iter().map(|s| s.to_string()).collect();
            candidates
                .into_iter()
                .filter(|c| allowed.contains(&c.scope))
                .collect()
        }
    }
}

/// A scored hit (id, scope, kind, text, score), produced by `rank`.
#[derive(Debug, Clone)]
pub struct Scored {
    pub id: i64,
    pub scope: String,
    pub kind: crate::model::MemoryKind,
    pub text: String,
    pub score: f64,
}

/// Normalize bm25 across candidates (lower bm25 = better => higher norm),
/// compute combined scores, sort descending, and take top_k.
pub fn rank(
    candidates: Vec<Candidate>,
    now: i64,
    top_k: usize,
    weights: &Weights,
) -> Vec<Scored> {
    // Collect bm25 values (more negative = better match).
    let bm25_vals: Vec<f64> = candidates.iter().filter_map(|c| c.bm25).collect();
    let (min, max) = match (
        bm25_vals.iter().cloned().fold(f64::INFINITY, f64::min),
        bm25_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    ) {
        (mn, mx) if mn.is_finite() && mx.is_finite() => (mn, mx),
        _ => (0.0, 0.0),
    };

    let mut scored: Vec<Scored> = candidates
        .into_iter()
        .map(|c| {
            // relevance: invert bm25 (lower is better) then min-max to [0,1].
            let lexical_norm = match c.bm25 {
                None => 0.0,
                Some(_) if (max - min).abs() < 1e-9 => 1.0,
                Some(b) => (max - b) / (max - min),
            };
            let age = (now - c.created_at).max(0);
            let score =
                combined_score(lexical_norm, age, c.importance, c.recall_count, weights);
            Scored {
                id: c.id,
                scope: c.scope,
                kind: c.kind,
                text: c.text,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    scored
}

/// Decide which candidate sources to combine for the given mode.
pub fn sources_for_mode(mode: RecallMode) -> (bool, bool) {
    // returns (use_keyword, use_recent)
    match mode {
        RecallMode::Keyword => (true, false),
        RecallMode::Tree => (false, true),
        RecallMode::Hybrid => (true, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemoryKind;

    fn cand(id: i64, scope: &str, created_at: i64, bm25: Option<f64>) -> Candidate {
        Candidate {
            id,
            scope: scope.to_string(),
            kind: MemoryKind::Note,
            text: format!("memory {id}"),
            created_at,
            recall_count: 0,
            importance: 1.0,
            bm25,
        }
    }

    #[test]
    fn trivial_queries_detected() {
        assert!(is_trivial("of"));
        assert!(is_trivial("a the"));
        assert!(is_trivial("  "));
        assert!(!is_trivial("sqlite migration"));
    }

    #[test]
    fn fts_match_quotes_and_or_joins() {
        assert_eq!(
            fts_match_string("SQLite, migration!").unwrap(),
            "\"sqlite\" OR \"migration\""
        );
        assert!(fts_match_string("  ").is_none());
    }

    #[test]
    fn merge_prefers_keyword_rows() {
        let kw = vec![cand(1, "global", 100, Some(-2.0))];
        let recent = vec![cand(1, "global", 100, None), cand(2, "global", 100, None)];
        let merged = merge_candidates(kw, recent);
        assert_eq!(merged.len(), 2);
        let one = merged.iter().find(|c| c.id == 1).unwrap();
        assert!(one.bm25.is_some()); // kept keyword row
    }

    #[test]
    fn scope_filter_includes_ancestry() {
        let cands = vec![
            cand(1, "agent:main", 100, None),
            cand(2, "global", 100, None),
            cand(3, "project:X", 100, None),
        ];
        let filtered =
            filter_by_scope(cands, &Some(Scope::Agent("main".to_string())));
        let ids: Vec<i64> = filtered.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2]); // agent:main + global, not project:X
    }

    #[test]
    fn rank_orders_by_score_and_truncates() {
        let cands = vec![
            cand(1, "global", 0, Some(-1.0)),   // best bm25
            cand(2, "global", 0, Some(-5.0)),   // worse bm25
        ];
        let ranked = rank(cands, 0, 1, &Weights::default());
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].id, 1); // higher lexical_norm wins
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-memory/src/lib.rs` with:

```rust
//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;
pub mod model;
pub mod recall;
pub mod scope;
pub mod scoring;
pub mod store;

pub use error::{MemoryError, Result};
pub use model::{
    Evidence, MemoryItem, MemoryKind, RecallHit, RecallMode, RecallQuery, RememberInput,
    ScopeCount, Stats, WukongResult,
};
pub use scope::Scope;
pub use scoring::Weights;
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p wukong-memory recall::`
Expected: all 5 recall tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/recall/mod.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add recall ranking and adaptive gate"
```

---

## Task 8: Public Memory API

**Files:**
- Modify: `crates/wukong-memory/src/lib.rs`
- Create: `crates/wukong-memory/tests/integration.rs`

- [ ] **Step 1: Add the Memory facade to lib.rs**

Replace the contents of `crates/wukong-memory/src/lib.rs` with:

```rust
//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod error;
pub mod model;
pub mod recall;
pub mod scope;
pub mod scoring;
pub mod store;

pub use error::{MemoryError, Result};
pub use model::{
    Evidence, MemoryItem, MemoryKind, RecallHit, RecallMode, RecallQuery, RememberInput,
    ScopeCount, Stats, WukongResult,
};
pub use scope::Scope;
pub use scoring::Weights;

use recall::{
    filter_by_scope, fts_match_string, is_trivial, merge_candidates, rank, sources_for_mode,
};
use store::{Candidate, Store};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Internal fetch fan-out before ranking.
fn fetch_limit(top_k: usize) -> i64 {
    (top_k.max(5) * 10).max(50) as i64
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The public memory facade. Wraps the store and ranking weights.
pub struct Memory {
    store: Store,
    weights: Weights,
}

impl Memory {
    /// Open (creating if missing) the memory database.
    pub async fn open(db_url: &str) -> Result<Memory> {
        Ok(Memory {
            store: Store::open(db_url).await?,
            weights: Weights::default(),
        })
    }

    /// Persist a batch of memories. Returns the new row ids.
    pub async fn remember(&self, input: RememberInput) -> Result<WukongResult<Vec<i64>>> {
        let start = Instant::now();
        let scope = Scope::parse(&input.scope)?;
        let scope_str = scope.to_string();
        let now = now_unix();

        if let Some(session_id) = &input.session_id {
            self.store.upsert_session(session_id, &scope_str, now).await?;
        }

        let mut ids = Vec::with_capacity(input.items.len());
        for item in &input.items {
            let importance = item.importance.unwrap_or(1.0);
            let id = self
                .store
                .insert_memory(
                    input.session_id.as_deref(),
                    &scope_str,
                    item.kind,
                    &item.text,
                    importance,
                    now,
                )
                .await?;
            ids.push(id);
        }

        Ok(WukongResult {
            data: ids,
            evidence: Vec::new(),
            confidence: 1.0,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Recall memories relevant to a query.
    pub async fn recall(&self, query: RecallQuery) -> Result<WukongResult<Vec<RecallHit>>> {
        let start = Instant::now();

        // Adaptive gate: skip trivial queries.
        if is_trivial(&query.query) {
            return Ok(WukongResult {
                data: Vec::new(),
                evidence: Vec::new(),
                confidence: 0.0,
                latency_ms: start.elapsed().as_millis() as u64,
            });
        }

        let scope_filter = match &query.scope {
            Some(s) => Some(Scope::parse(s)?),
            None => None,
        };
        let (use_keyword, use_recent) = sources_for_mode(query.mode);
        let limit = fetch_limit(query.top_k);
        let now = now_unix();

        let keyword = if use_keyword {
            match fts_match_string(&query.query) {
                Some(expr) => self.store.keyword_candidates(&expr, limit).await?,
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let recent = if use_recent {
            self.store.recent_candidates(limit).await?
        } else {
            Vec::new()
        };

        let merged: Vec<Candidate> = match query.mode {
            RecallMode::Keyword => keyword,
            RecallMode::Tree => recent,
            RecallMode::Hybrid => merge_candidates(keyword, recent),
        };
        let filtered = filter_by_scope(merged, &scope_filter);
        let scored = rank(filtered, now, query.top_k, &self.weights);

        let ids: Vec<i64> = scored.iter().map(|s| s.id).collect();
        if !ids.is_empty() {
            self.store.touch_recalled(&ids, now).await?;
        }

        let evidence: Vec<Evidence> = scored
            .iter()
            .map(|s| Evidence {
                id: s.id,
                scope: s.scope.clone(),
                score: s.score,
            })
            .collect();
        let confidence = scored.first().map(|s| s.score.clamp(0.0, 1.0)).unwrap_or(0.0);
        let hits: Vec<RecallHit> = scored
            .into_iter()
            .map(|s| RecallHit {
                id: s.id,
                scope: s.scope,
                kind: s.kind,
                text: s.text,
                score: s.score,
            })
            .collect();

        Ok(WukongResult {
            data: hits,
            evidence,
            confidence,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Aggregate statistics.
    pub async fn stats(&self) -> Result<Stats> {
        self.store.stats().await
    }
}
```

- [ ] **Step 2: Write the integration test**

Create `crates/wukong-memory/tests/integration.rs`:

```rust
use tempfile::NamedTempFile;
use wukong_memory::{Memory, MemoryItem, MemoryKind, RecallMode, RecallQuery, RememberInput};

async fn open_memory() -> Memory {
    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);
    Memory::open(&url).await.unwrap()
}

fn item(text: &str) -> MemoryItem {
    MemoryItem {
        kind: MemoryKind::Note,
        text: text.to_string(),
        importance: None,
    }
}

#[tokio::test]
async fn remember_then_recall_finds_match() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("we migrated the database to SQLite")],
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

    assert_eq!(res.data.len(), 1);
    assert!(res.data[0].text.contains("SQLite"));
    assert_eq!(res.evidence.len(), 1);
    assert!(res.confidence > 0.0);
}

#[tokio::test]
async fn scope_isolation_excludes_other_scopes() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "project:Alpha".to_string(),
        session_id: None,
        items: vec![item("alpha secret token")],
    })
    .await
    .unwrap();
    mem.remember(RememberInput {
        scope: "project:Beta".to_string(),
        session_id: None,
        items: vec![item("beta secret token")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "secret token".to_string(),
            top_k: 5,
            scope: Some("project:Alpha".to_string()),
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(res.data.iter().all(|h| h.scope == "project:Alpha"));
    assert!(res.data.iter().any(|h| h.text.contains("alpha")));
}

#[tokio::test]
async fn trivial_query_returns_empty() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("something memorable")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "of".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(res.data.is_empty());
    assert_eq!(res.confidence, 0.0);
}

#[tokio::test]
async fn invalid_scope_is_rejected() {
    let mem = open_memory().await;
    let err = mem
        .remember(RememberInput {
            scope: "bogus".to_string(),
            session_id: None,
            items: vec![item("x")],
        })
        .await;
    assert!(err.is_err());
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test -p wukong-memory`
Expected: all unit tests plus the 4 integration tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/lib.rs crates/wukong-memory/tests/integration.rs
git commit -m "feat(memory): add public Memory API with integration tests"
```

---

## Task 9: HTTP server (wukong-memoryd)

**Files:**
- Modify: `crates/wukong-memoryd/src/lib.rs`
- Modify: `crates/wukong-memoryd/src/main.rs`
- Create: `crates/wukong-memoryd/tests/http.rs`

- [ ] **Step 1: Implement config, router, handlers, and error mapping**

Replace the contents of `crates/wukong-memoryd/src/lib.rs` with:

```rust
//! wukong-memoryd: axum HTTP transport over wukong-memory.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::sync::Arc;
use wukong_memory::{Memory, MemoryError, RecallQuery, RememberInput};

/// Server configuration sourced from the environment.
pub struct Config {
    pub db_url: String,
    pub port: u16,
}

impl Config {
    /// Build config from env vars, with sensible defaults:
    /// WUKONG_MEMORY_DB (default $HOME/.wukong/memory.db) and
    /// WUKONG_MEMORY_PORT (default 3917).
    pub fn from_env() -> Config {
        let db_url = match std::env::var("WUKONG_MEMORY_DB") {
            Ok(v) => v,
            Err(_) => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let dir = format!("{home}/.wukong");
                let _ = std::fs::create_dir_all(&dir);
                format!("sqlite://{dir}/memory.db")
            }
        };
        let port = std::env::var("WUKONG_MEMORY_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3917);
        Config { db_url, port }
    }
}

/// Newtype so we can implement axum's IntoResponse for the library error
/// (orphan rule: MemoryError and IntoResponse are both foreign here).
pub struct AppError(MemoryError);

impl From<MemoryError> for AppError {
    fn from(e: MemoryError) -> Self {
        AppError(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            MemoryError::InvalidScope(_) | MemoryError::InvalidQuery(_) => {
                StatusCode::BAD_REQUEST
            }
            MemoryError::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(serde_json::json!({ "error": self.0.to_string() }));
        (status, body).into_response()
    }
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn stats(State(mem): State<Arc<Memory>>) -> Result<impl IntoResponse, AppError> {
    Ok(Json(mem.stats().await?))
}

async fn remember(
    State(mem): State<Arc<Memory>>,
    Json(input): Json<RememberInput>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(mem.remember(input).await?))
}

async fn recall(
    State(mem): State<Arc<Memory>>,
    Json(query): Json<RecallQuery>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(mem.recall(query).await?))
}

/// Build the axum router over a shared Memory instance.
pub fn build_router(mem: Arc<Memory>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/stats", get(stats))
        .route("/v1/remember", post(remember))
        .route("/v1/recall", post(recall))
        .with_state(mem)
}
```

- [ ] **Step 2: Implement the binary entrypoint**

Replace the contents of `crates/wukong-memoryd/src/main.rs` with:

```rust
use std::sync::Arc;
use wukong_memory::Memory;
use wukong_memoryd::{build_router, Config};

#[tokio::main]
async fn main() {
    let config = Config::from_env();
    let memory = Memory::open(&config.db_url)
        .await
        .expect("failed to open memory database");
    let app = build_router(Arc::new(memory));

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");
    println!("wukong-memoryd listening on {addr}");
    axum::serve(listener, app).await.expect("server error");
}
```

- [ ] **Step 3: Write HTTP tests**

Create `crates/wukong-memoryd/tests/http.rs`:

```rust
use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tempfile::NamedTempFile;
use tower::ServiceExt; // for `oneshot`
use wukong_memory::Memory;
use wukong_memoryd::build_router;

async fn test_app() -> axum::Router {
    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);
    let memory = Memory::open(&url).await.unwrap();
    build_router(Arc::new(memory))
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_returns_ok() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn remember_then_recall_over_http() {
    let app = test_app().await;

    let remember_req = Request::builder()
        .method("POST")
        .uri("/v1/remember")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"scope":"global","items":[{"kind":"note","text":"axum powers the http layer"}]}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(remember_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["data"].as_array().unwrap().len(), 1);

    let recall_req = Request::builder()
        .method("POST")
        .uri("/v1/recall")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"query":"http layer"}"#))
        .unwrap();
    let resp = app.oneshot(recall_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json["data"].as_array().unwrap().len() >= 1);
    assert!(json["latency_ms"].is_number());
}

#[tokio::test]
async fn invalid_scope_returns_400() {
    let app = test_app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/remember")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"scope":"bogus","items":[{"kind":"note","text":"x"}]}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn stats_returns_totals() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["total"], 0);
}
```

- [ ] **Step 4: Run the full test suite**

Run: `cargo test`
Expected: all wukong-memory unit + integration tests AND all 4 wukong-memoryd HTTP tests pass.

- [ ] **Step 5: Verify the server boots**

Run: `WUKONG_MEMORY_DB=sqlite://./scratch.db WUKONG_MEMORY_PORT=3917 cargo run -p wukong-memoryd &`
Then: `sleep 2 && curl -sf http://127.0.0.1:3917/v1/health`
Expected: `{"status":"ok"}`. Then stop the server (`kill %1`) and remove `scratch.db`.

- [ ] **Step 6: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/wukong-memoryd/src/lib.rs crates/wukong-memoryd/src/main.rs crates/wukong-memoryd/tests/http.rs
git commit -m "feat(memoryd): add axum HTTP server"
```

---

## Acceptance Criteria (from spec)

1. `cargo test` is green (unit + integration + HTTP). — Tasks 2-9
2. `remember` then `recall` retrieves the memory; scope isolation correct. — Task 8 (`remember_then_recall_finds_match`, `scope_isolation_excludes_other_scopes`)
3. Hybrid ordering follows the combined-score formula (newer / more important / more-recalled rank higher). — Task 5 + Task 7 (`rank_orders_by_score_and_truncates`)
4. Adaptive gate returns empty for trivial queries. — Task 7 (`trivial_queries_detected`), Task 8 (`trivial_query_returns_empty`)
5. HTTP endpoints behave and envelope matches the design. — Task 9
6. No new lint errors (`cargo clippy`). — Task 9 Step 6
```
