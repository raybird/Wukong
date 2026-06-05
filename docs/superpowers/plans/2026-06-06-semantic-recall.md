# 語意向量召回（本機 embedding）Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 為 `wukong-memory` 的召回加入本機語意向量（cosine），作為選用增強層；未啟用或無模型時行為等同 v1 BM25。

**Architecture:** 在 `wukong-memory` crate 原地擴充。新增 `embed` 模組（Embedder trait + MockEmbedder + 純向量運算）；`memories` 表加 `embedding`/`embedding_model` 欄位；召回新增向量候選源，並把 `δ·語意` 納入既有 `combined_score`。真實模型 `FastembedBackend` 以 cargo feature `embed` 隔離（預設關），測試一律用 `MockEmbedder`，不需下載模型。

**Tech Stack:** Rust、sqlx(sqlite)、tokio、fastembed(feature-gated)。

**對應 spec：** `docs/superpowers/specs/2026-06-06-semantic-recall-design.md`

**慣例：** 所有 cargo 指令前綴 `. "$HOME/.cargo/env" &&`；串接測試與 commit 時用 `set -o pipefail`。Commit 訊息只寫功能描述，不得含任何 AI 署名。

---

## File Structure

- `crates/wukong-memory/src/embed/mod.rs` — **新**：`Embedder` trait、`MockEmbedder`、`cosine_similarity`、`embedding_to_blob`/`blob_to_embedding`、`FastembedBackend`(feature `embed`)。
- `crates/wukong-memory/src/scoring.rs` — **改**：`Weights` 加 `semantic`；`combined_score` 加 `semantic_norm` 參數。
- `crates/wukong-memory/src/store/mod.rs` — **改**：schema 遷移加欄位；`Candidate.vector_sim`；`update_embedding`/`embedded_candidates`/`rows_missing_embedding`。
- `crates/wukong-memory/src/recall/mod.rs` — **改**：`rank` 融入語意；`apply_vector_sims`、`build_vector_candidates`；`sources_for_mode` 改 3-tuple。
- `crates/wukong-memory/src/error.rs` — **改**：`MemoryError::Embed`。
- `crates/wukong-memory/src/lib.rs` — **改**：`Memory.embedder`、`with_embedder`、remember 寫向量、recall 走向量源、`backfill_embeddings`、re-export。
- `crates/wukong-memory/Cargo.toml` — **改**：feature `embed` + 選用 `fastembed` 相依。
- `crates/wukong-cli/src/main.rs`、`crates/wukong-cli/Cargo.toml` — **改**：WUKONG_EMBED 環境變數接線（feature-gated）。

---

### Task 1: 向量運算與序列化（純函式）

**Files:**
- Create: `crates/wukong-memory/src/embed/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`（加 `pub mod embed;`）

- [ ] **Step 1: 建立 embed 模組並掛上**

在 `crates/wukong-memory/src/lib.rs` 的模組宣告區（現有 `pub mod store;` 附近）加一行：

```rust
pub mod embed;
```

- [ ] **Step 2: 寫向量純函式 + 失敗測試**

建立 `crates/wukong-memory/src/embed/mod.rs`：

```rust
//! Embedding layer: trait, mock backend, and pure vector math.

/// Cosine similarity in [-1, 1]. Returns 0.0 for empty, length-mismatched,
/// or zero-magnitude inputs.
pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f64 {
    if a.is_empty() || a.len() != b.len() {
        return 0.0;
    }
    let mut dot = 0.0f64;
    let mut na = 0.0f64;
    let mut nb = 0.0f64;
    for i in 0..a.len() {
        let (x, y) = (a[i] as f64, b[i] as f64);
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Serialize an embedding to little-endian f32 bytes.
pub fn embedding_to_blob(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

/// Deserialize little-endian f32 bytes back to an embedding.
pub fn blob_to_embedding(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine_identical_is_one() {
        let a = vec![1.0, 2.0, 3.0];
        assert!((cosine_similarity(&a, &a) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_orthogonal_is_zero() {
        let a = vec![1.0, 0.0];
        let b = vec![0.0, 1.0];
        assert!(cosine_similarity(&a, &b).abs() < 1e-9);
    }

    #[test]
    fn cosine_opposite_is_minus_one() {
        let a = vec![1.0, 0.0];
        let b = vec![-1.0, 0.0];
        assert!((cosine_similarity(&a, &b) + 1.0).abs() < 1e-9);
    }

    #[test]
    fn cosine_zero_or_mismatch_is_zero() {
        assert_eq!(cosine_similarity(&[0.0, 0.0], &[1.0, 1.0]), 0.0);
        assert_eq!(cosine_similarity(&[1.0], &[1.0, 2.0]), 0.0);
        assert_eq!(cosine_similarity(&[], &[]), 0.0);
    }

    #[test]
    fn blob_roundtrip_preserves_values() {
        let v = vec![1.5f32, -2.25, 0.0, 3.125];
        let restored = blob_to_embedding(&embedding_to_blob(&v));
        assert_eq!(v, restored);
    }
}
```

- [ ] **Step 3: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory embed::`
Expected: 5 個 embed tests PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/embed/mod.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add vector math and embedding blob serialization"
```

---

### Task 2: Embedder trait + MockEmbedder

**Files:**
- Modify: `crates/wukong-memory/src/embed/mod.rs`

- [ ] **Step 1: 加 trait 與 MockEmbedder（先寫測試）**

在 `crates/wukong-memory/src/embed/mod.rs` 頂端 `use` 區加：

```rust
use crate::error::Result;
```

在純函式之後、`#[cfg(test)]` 之前插入：

```rust
/// Turns text into a fixed-dimension embedding. Implementors must be cheap to
/// share across threads (used from a background task).
pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        texts.iter().map(|t| self.embed(t)).collect()
    }
    fn dim(&self) -> usize;
    fn model_id(&self) -> &str;
}

/// Deterministic, dependency-free embedder for tests. Same text always maps to
/// the same unit vector; different text usually maps elsewhere. NOT semantic —
/// it only exercises the plumbing.
pub struct MockEmbedder {
    dim: usize,
}

impl MockEmbedder {
    pub fn new(dim: usize) -> Self {
        Self { dim }
    }
}

impl Embedder for MockEmbedder {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut v = vec![0.0f32; self.dim];
        for (i, b) in text.bytes().enumerate() {
            v[(b as usize + i) % self.dim] += 1.0;
        }
        let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        if norm > 0.0 {
            for x in &mut v {
                *x /= norm;
            }
        }
        Ok(v)
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        "mock"
    }
}
```

在 `mod tests` 內新增：

```rust
    #[test]
    fn mock_is_deterministic() {
        let e = MockEmbedder::new(8);
        assert_eq!(e.embed("hello").unwrap(), e.embed("hello").unwrap());
    }

    #[test]
    fn mock_differs_for_different_text() {
        let e = MockEmbedder::new(8);
        assert_ne!(e.embed("hello").unwrap(), e.embed("world!!").unwrap());
    }

    #[test]
    fn mock_dim_and_batch() {
        let e = MockEmbedder::new(8);
        assert_eq!(e.embed("x").unwrap().len(), 8);
        assert_eq!(e.dim(), 8);
        let batch = e.embed_batch(&["a".into(), "b".into()]).unwrap();
        assert_eq!(batch.len(), 2);
        assert_eq!(batch[0], e.embed("a").unwrap());
    }
```

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory embed::`
Expected: 8 個 embed tests PASS（5 舊 + 3 新）。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-memory/src/embed/mod.rs
git commit -m "feat(memory): add Embedder trait and MockEmbedder"
```

---

### Task 3: combined_score 加入語意項

**Files:**
- Modify: `crates/wukong-memory/src/scoring.rs`

- [ ] **Step 1: 改 Weights、combined_score，並同步既有測試呼叫**

把 `crates/wukong-memory/src/scoring.rs` 的 `Weights` 與 `Default` 改為四欄：

```rust
/// Relative weights for the combined recall score. The four weights are
/// expected to sum to ~1.0 so the base score stays within [0, 1].
#[derive(Debug, Clone, Copy)]
pub struct Weights {
    pub lexical: f64,
    pub semantic: f64,
    pub decay: f64,
    pub importance: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            lexical: 0.4,
            semantic: 0.2,
            decay: 0.25,
            importance: 0.15,
        }
    }
}
```

把 `combined_score` 改成多收一個 `semantic_norm`：

```rust
/// Combined recall score. `lexical_norm`, `semantic_norm`, and `importance` are
/// expected to be in [0, 1]. Frequently recalled memories get a small
/// logarithmic bonus.
pub fn combined_score(
    lexical_norm: f64,
    semantic_norm: f64,
    age_seconds: i64,
    importance: f64,
    recall_count: i64,
    w: &Weights,
) -> f64 {
    let decay = time_decay(age_seconds, HALF_LIFE_DAYS);
    let base = w.lexical * lexical_norm
        + w.semantic * semantic_norm
        + w.decay * decay
        + w.importance * importance;
    base + 0.02 * (1.0 + recall_count.max(0) as f64).ln()
}
```

更新 `mod tests` 中**所有** `combined_score(...)` 呼叫，在 `lexical_norm` 之後插入一個 `semantic_norm` 引數（全部填 `0.0`，維持原意），並新增一個語意項測試：

```rust
    #[test]
    fn newer_memory_outranks_older_when_all_else_equal() {
        let w = Weights::default();
        let newer = combined_score(0.5, 0.0, 0, 1.0, 0, &w);
        let older = combined_score(0.5, 0.0, 200 * 86_400, 1.0, 0, &w);
        assert!(newer > older);
    }

    #[test]
    fn higher_lexical_match_outranks_lower() {
        let w = Weights::default();
        let strong = combined_score(1.0, 0.0, 0, 1.0, 0, &w);
        let weak = combined_score(0.1, 0.0, 0, 1.0, 0, &w);
        assert!(strong > weak);
    }

    #[test]
    fn recall_count_provides_small_bonus() {
        let w = Weights::default();
        let hot = combined_score(0.5, 0.0, 0, 1.0, 10, &w);
        let cold = combined_score(0.5, 0.0, 0, 1.0, 0, &w);
        assert!(hot > cold);
    }

    #[test]
    fn higher_semantic_match_outranks_lower() {
        let w = Weights::default();
        let strong = combined_score(0.0, 1.0, 0, 1.0, 0, &w);
        let weak = combined_score(0.0, 0.1, 0, 1.0, 0, &w);
        assert!(strong > weak);
    }
```

（注意：`decay_is_one_at_zero_age`、`decay_is_half_at_half_life`、`negative_age_clamped` 三個測試只呼叫 `time_decay`，不需改動。）

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory scoring::`
Expected: 7 個 scoring tests PASS（含新 `higher_semantic_match_outranks_lower`）。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-memory/src/scoring.rs
git commit -m "feat(memory): add semantic term to combined recall score"
```

---

### Task 4: Candidate.vector_sim 與 rank 融入語意

**Files:**
- Modify: `crates/wukong-memory/src/store/mod.rs`（`Candidate` 結構、`row_to_candidate`）
- Modify: `crates/wukong-memory/src/recall/mod.rs`（`rank`、`sources_for_mode`、`apply_vector_sims`、`build_vector_candidates`、測試 helper）

- [ ] **Step 1: Candidate 加 vector_sim 欄位**

在 `crates/wukong-memory/src/store/mod.rs` 的 `Candidate` 結構末欄（`bm25` 之後）加：

```rust
    /// FTS5 bm25 rank (lower = better match); None for non-keyword sources.
    pub bm25: Option<f64>,
    /// Cosine similarity to the query (higher = better); None for non-vector sources.
    pub vector_sim: Option<f64>,
}
```

在同檔 `row_to_candidate` 末欄補上（keyword/recent 源無向量）：

```rust
        bm25: r.get::<Option<f64>, _>("bm25"),
        vector_sim: None,
    }
}
```

- [ ] **Step 2: 改 recall/mod.rs 的 rank、sources_for_mode，新增兩個 helper（含測試）**

在 `crates/wukong-memory/src/recall/mod.rs` 頂端 `use` 區加：

```rust
use crate::embed::cosine_similarity;
```

把 `rank` 函式整段替換為（新增 vector_sim 的 min-max 與語意參數）：

```rust
/// Normalize bm25 and vector_sim across candidates, compute combined scores,
/// sort descending, and take top_k. bm25: lower = better. vector_sim: higher =
/// better. Either signal absent on a candidate contributes 0 for that term.
pub fn rank(
    candidates: Vec<Candidate>,
    now: i64,
    top_k: usize,
    weights: &Weights,
) -> Vec<Scored> {
    // bm25: more negative = better match.
    let bm25_vals: Vec<f64> = candidates.iter().filter_map(|c| c.bm25).collect();
    let (bmin, bmax) = match (
        bm25_vals.iter().cloned().fold(f64::INFINITY, f64::min),
        bm25_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    ) {
        (mn, mx) if mn.is_finite() && mx.is_finite() => (mn, mx),
        _ => (0.0, 0.0),
    };

    // vector_sim: higher = better match.
    let vec_vals: Vec<f64> = candidates.iter().filter_map(|c| c.vector_sim).collect();
    let (vmin, vmax) = match (
        vec_vals.iter().cloned().fold(f64::INFINITY, f64::min),
        vec_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    ) {
        (mn, mx) if mn.is_finite() && mx.is_finite() => (mn, mx),
        _ => (0.0, 0.0),
    };

    let mut scored: Vec<Scored> = candidates
        .into_iter()
        .map(|c| {
            // lexical: invert bm25 (lower better) then min-max to [0,1].
            let lexical_norm = match c.bm25 {
                None => 0.0,
                Some(_) if (bmax - bmin).abs() < 1e-9 => 1.0,
                Some(b) => (bmax - b) / (bmax - bmin),
            };
            // semantic: min-max vector_sim (higher better) to [0,1].
            let semantic_norm = match c.vector_sim {
                None => 0.0,
                Some(_) if (vmax - vmin).abs() < 1e-9 => 1.0,
                Some(s) => (s - vmin) / (vmax - vmin),
            };
            let age = (now - c.created_at).max(0);
            let score = combined_score(
                lexical_norm,
                semantic_norm,
                age,
                c.importance,
                c.recall_count,
                weights,
            );
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
```

把 `sources_for_mode` 改為 3-tuple：

```rust
/// Decide which candidate sources to combine for the given mode.
/// Returns (use_keyword, use_recent, use_vector).
pub fn sources_for_mode(mode: RecallMode) -> (bool, bool, bool) {
    match mode {
        RecallMode::Keyword => (true, false, false),
        RecallMode::Tree => (false, true, false),
        RecallMode::Hybrid => (true, true, true),
    }
}
```

在 `sources_for_mode` 之後新增兩個 helper：

```rust
/// Build vector candidates from embedded rows: compute cosine to the query,
/// set vector_sim, sort by similarity (best first), and keep `limit`.
pub fn build_vector_candidates(
    query_vec: &[f32],
    embedded: Vec<(Candidate, Vec<f32>)>,
    limit: usize,
) -> Vec<Candidate> {
    let mut cands: Vec<Candidate> = embedded
        .into_iter()
        .map(|(mut c, v)| {
            c.vector_sim = Some(cosine_similarity(query_vec, &v));
            c
        })
        .collect();
    cands.sort_by(|a, b| {
        b.vector_sim
            .partial_cmp(&a.vector_sim)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    cands.truncate(limit);
    cands
}

/// Fold vector candidates into a base set: for ids already present, copy their
/// vector_sim onto the existing candidate (preserving bm25); append vector-only ids.
pub fn apply_vector_sims(mut base: Vec<Candidate>, vector: Vec<Candidate>) -> Vec<Candidate> {
    for v in vector {
        if let Some(existing) = base.iter_mut().find(|c| c.id == v.id) {
            existing.vector_sim = v.vector_sim;
        } else {
            base.push(v);
        }
    }
    base
}
```

更新 `mod tests` 內的 `cand` helper（補 `vector_sim: None`），並把現有的 `sources_for_mode` 不需測試但 `rank` 測試保留；新增三個測試：

```rust
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
            vector_sim: None,
        }
    }

    #[test]
    fn higher_vector_sim_outranks_when_equal_age() {
        let mut a = cand(1, "global", 0, None);
        let mut b = cand(2, "global", 0, None);
        a.vector_sim = Some(0.1);
        b.vector_sim = Some(0.9);
        let ranked = rank(vec![a, b], 0, 2, &Weights::default());
        assert_eq!(ranked[0].id, 2); // stronger semantic match wins
    }

    #[test]
    fn build_vector_candidates_sorts_and_truncates() {
        let q = vec![1.0f32, 0.0];
        let embedded = vec![
            (cand(1, "global", 0, None), vec![0.0f32, 1.0]), // cosine 0
            (cand(2, "global", 0, None), vec![1.0f32, 0.0]), // cosine 1
        ];
        let out = build_vector_candidates(&q, embedded, 1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 2);
        assert!(out[0].vector_sim.unwrap() > 0.99);
    }

    #[test]
    fn apply_vector_sims_merges_signals_and_appends() {
        let base = vec![cand(1, "global", 0, Some(-2.0))]; // keyword hit
        let mut v1 = cand(1, "global", 0, None);
        v1.vector_sim = Some(0.8);
        let mut v2 = cand(2, "global", 0, None);
        v2.vector_sim = Some(0.5);
        let merged = apply_vector_sims(base, vec![v1, v2]);
        assert_eq!(merged.len(), 2);
        let one = merged.iter().find(|c| c.id == 1).unwrap();
        assert!(one.bm25.is_some() && one.vector_sim == Some(0.8)); // both signals
    }
```

- [ ] **Step 3: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory recall::`
Expected: recall tests PASS（含 3 個新測試）。`rank_orders_by_score_and_truncates` 仍綠（`cand` 預設 `vector_sim: None`，語意項為 0，不影響 bm25 排序）。

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/store/mod.rs crates/wukong-memory/src/recall/mod.rs
git commit -m "feat(memory): blend semantic similarity into recall ranking"
```

---

### Task 5: MemoryError::Embed

**Files:**
- Modify: `crates/wukong-memory/src/error.rs`

- [ ] **Step 1: 加變體與測試**

在 `crates/wukong-memory/src/error.rs` 的 `MemoryError` enum 內 `Serialize` 之後加：

```rust
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("embedding error: {0}")]
    Embed(String),
}
```

在 `mod tests` 內新增：

```rust
    #[test]
    fn embed_error_message_includes_detail() {
        let err = MemoryError::Embed("model load failed".to_string());
        assert_eq!(err.to_string(), "embedding error: model load failed");
    }
```

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory error::`
Expected: 2 個 error tests PASS。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-memory/src/error.rs
git commit -m "feat(memory): add Embed error variant"
```

---

### Task 6: schema 遷移加入 embedding 欄位

**Files:**
- Modify: `crates/wukong-memory/src/store/mod.rs`（`open`、新增 `migrate`、測試）

- [ ] **Step 1: 新增 migrate 並在 open 呼叫（先寫測試）**

在 `crates/wukong-memory/src/store/mod.rs` 的 `Store::open` 內，把 schema 套用後加一行遷移呼叫：

```rust
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        migrate(&pool).await?;
        Ok(Store { pool })
```

在 `impl Store { ... }` 區塊**之後**（與 `row_to_candidate` 同層）新增自由函式：

```rust
/// Idempotently add v2 embedding columns to an existing `memories` table.
/// Safe to run on every open: checks PRAGMA table_info before ALTER.
async fn migrate(pool: &SqlitePool) -> Result<()> {
    let cols = sqlx::query("PRAGMA table_info(memories)")
        .fetch_all(pool)
        .await?;
    let names: Vec<String> = cols.iter().map(|r| r.get::<String, _>("name")).collect();
    if !names.iter().any(|n| n == "embedding") {
        sqlx::query("ALTER TABLE memories ADD COLUMN embedding BLOB")
            .execute(pool)
            .await?;
    }
    if !names.iter().any(|n| n == "embedding_model") {
        sqlx::query("ALTER TABLE memories ADD COLUMN embedding_model TEXT")
            .execute(pool)
            .await?;
    }
    Ok(())
}
```

在 `mod tests` 內新增：

```rust
    #[tokio::test]
    async fn migrate_adds_embedding_columns() {
        let store = test_store().await;
        let cols = sqlx::query("PRAGMA table_info(memories)")
            .fetch_all(&store.pool)
            .await
            .unwrap();
        let names: Vec<String> = cols.iter().map(|r| r.get::<String, _>("name")).collect();
        assert!(names.iter().any(|n| n == "embedding"));
        assert!(names.iter().any(|n| n == "embedding_model"));
    }

    #[tokio::test]
    async fn migrate_is_idempotent() {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        // Open twice: second open re-runs migrate over already-migrated table.
        let _first = Store::open(&url).await.unwrap();
        let _second = Store::open(&url).await.unwrap();
    }
```

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory store::`
Expected: store tests PASS（含 2 新）。既有 fts/recent/stats/touch 測試仍綠。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-memory/src/store/mod.rs
git commit -m "feat(memory): migrate schema with embedding columns"
```

---

### Task 7: Store 向量讀寫與補齊查詢

**Files:**
- Modify: `crates/wukong-memory/src/store/mod.rs`（新增三個方法、測試）

- [ ] **Step 1: 新增 update_embedding / embedded_candidates / rows_missing_embedding（先寫測試）**

在 `crates/wukong-memory/src/store/mod.rs` 頂端 `use` 區確認有 `use crate::embed::blob_to_embedding;`（若無則加）。在 `impl Store` 內 `stats` 之前插入：

```rust
    /// Store an embedding blob and its model id for one memory.
    pub async fn update_embedding(&self, id: i64, blob: &[u8], model: &str) -> Result<()> {
        sqlx::query("UPDATE memories SET embedding = ?2, embedding_model = ?3 WHERE id = ?1")
            .bind(id)
            .bind(blob)
            .bind(model)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// All memories that have an embedding, paired with the decoded vector.
    /// bm25/vector_sim on the Candidate are None (filled later during ranking).
    pub async fn embedded_candidates(&self, limit: i64) -> Result<Vec<(Candidate, Vec<f32>)>> {
        let rows = sqlx::query(
            "SELECT id, scope, kind, text, created_at, recall_count, importance,
                    NULL AS bm25, embedding
             FROM memories
             WHERE embedding IS NOT NULL
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let blob: Vec<u8> = r.get::<Vec<u8>, _>("embedding");
                let cand = row_to_candidate(r);
                (cand, blob_to_embedding(&blob))
            })
            .collect())
    }

    /// (id, text) for memories still lacking an embedding (backfill source).
    pub async fn rows_missing_embedding(&self, limit: i64) -> Result<Vec<(i64, String)>> {
        let rows = sqlx::query(
            "SELECT id, text FROM memories WHERE embedding IS NULL ORDER BY id ASC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get::<i64, _>("id"), r.get::<String, _>("text")))
            .collect())
    }
```

> 注意：`row_to_candidate` 會讀取名為 `embedding` 以外的欄位，而 `embedded_candidates` 的查詢已 `r.get("embedding")` 取出 blob 後才把 `r` 交給 `row_to_candidate`，順序正確（先取 blob 再 move row）。

在 `mod tests` 內新增：

```rust
    #[tokio::test]
    async fn update_and_read_embedding() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let id = store
            .insert_memory(None, "global", MemoryKind::Note, "vec me", 1.0, 100)
            .await
            .unwrap();
        let v = vec![0.1f32, 0.2, 0.3];
        store
            .update_embedding(id, &embedding_to_blob(&v), "mock")
            .await
            .unwrap();
        let embedded = store.embedded_candidates(10).await.unwrap();
        assert_eq!(embedded.len(), 1);
        assert_eq!(embedded[0].0.id, id);
        assert_eq!(embedded[0].1, v);
    }

    #[tokio::test]
    async fn rows_missing_embedding_excludes_embedded() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let a = store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100)
            .await
            .unwrap();
        let _b = store
            .insert_memory(None, "global", MemoryKind::Note, "b", 1.0, 100)
            .await
            .unwrap();
        store
            .update_embedding(a, &embedding_to_blob(&[1.0f32]), "mock")
            .await
            .unwrap();
        let missing = store.rows_missing_embedding(10).await.unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].1, "b");
    }
```

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory store::`
Expected: store tests PASS（含 2 新）。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-memory/src/store/mod.rs
git commit -m "feat(memory): store embedding write, read, and backfill queries"
```

---

### Task 8: Memory embedder 接線 — 寫入向量與背景補齊

**Files:**
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Memory 加 embedder 欄位、with_embedder、remember 寫向量、backfill_embeddings、re-export**

在 `crates/wukong-memory/src/lib.rs` 的 `use` 區加：

```rust
use embed::{embedding_to_blob, Embedder};
use std::sync::Arc;
```

並在 re-export 區加：

```rust
pub use embed::{cosine_similarity, Embedder, MockEmbedder};
```

把 `Memory` 結構與 `open` 改為帶 embedder（預設 None）：

```rust
/// The public memory facade. Wraps the store, ranking weights, and an optional
/// embedder. With no embedder, recall and remember behave exactly like v1.
pub struct Memory {
    store: Store,
    weights: Weights,
    embedder: Option<Arc<dyn Embedder>>,
}

impl Memory {
    /// Open (creating if missing) the memory database. No semantic layer.
    pub async fn open(db_url: &str) -> Result<Memory> {
        Ok(Memory {
            store: Store::open(db_url).await?,
            weights: Weights::default(),
            embedder: None,
        })
    }

    /// Attach an embedder, enabling the semantic layer. Spawns a background task
    /// to backfill embeddings for any rows that lack them.
    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder.clone());
        let store = self.store.clone();
        tokio::spawn(async move {
            if let Err(e) = backfill(&store, embedder.as_ref()).await {
                eprintln!("wukong-memory: backfill failed: {e}");
            }
        });
        self
    }

    /// Embed and store vectors for every memory still missing one. Awaitable
    /// directly (used by tests and by the spawned background task).
    pub async fn backfill_embeddings(&self) -> Result<()> {
        match &self.embedder {
            Some(emb) => backfill(&self.store, emb.as_ref()).await,
            None => Ok(()),
        }
    }
```

在 `remember` 內，插入 row 取得 `id` 後、`ids.push(id)` 前，順帶寫入向量：

```rust
            ids.push(id);
            if let Some(emb) = &self.embedder {
                match emb.embed(&item.text) {
                    Ok(v) => {
                        let _ = self
                            .store
                            .update_embedding(id, &embedding_to_blob(&v), emb.model_id())
                            .await;
                    }
                    Err(e) => eprintln!("wukong-memory: embed on remember failed: {e}"),
                }
            }
```

在檔案末端（`impl Memory` 之外）新增背景補齊自由函式：

```rust
/// Embed and persist vectors for memories lacking them, in batches.
async fn backfill(store: &Store, embedder: &dyn Embedder) -> Result<()> {
    loop {
        let batch = store.rows_missing_embedding(32).await?;
        if batch.is_empty() {
            break;
        }
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let vecs = embedder.embed_batch(&texts)?;
        for ((id, _), v) in batch.iter().zip(vecs.iter()) {
            store
                .update_embedding(*id, &embedding_to_blob(v), embedder.model_id())
                .await?;
        }
        tokio::task::yield_now().await;
    }
    Ok(())
}
```

- [ ] **Step 2: 加整合測試（用 MockEmbedder）**

在 `crates/wukong-memory/tests/integration.rs` 末端新增（沿用該檔既有的開 DB 慣例；若該檔已有建立 `Memory` 的 helper 則複用，否則用下列 inline 形式）：

```rust
#[tokio::test]
async fn remember_writes_embedding_and_backfill_fills_old_rows() {
    use std::sync::Arc;
    use wukong_memory::{Memory, MemoryKind, MemoryItem, RememberInput};

    let file = tempfile::NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);

    // v1-style write: no embedder yet → row has no embedding.
    let plain = Memory::open(&url).await.unwrap();
    plain
        .remember(RememberInput {
            scope: "global".into(),
            session_id: None,
            items: vec![MemoryItem {
                kind: MemoryKind::Note,
                text: "old row without vector".into(),
                importance: None,
            }],
        })
        .await
        .unwrap();
    drop(plain);

    // Reopen with embedder, run backfill directly (deterministic).
    let mem = Memory::open(&url)
        .await
        .unwrap()
        .with_embedder(Arc::new(wukong_memory::MockEmbedder::new(16)));
    mem.backfill_embeddings().await.unwrap();

    // New write now also gets an embedding inline.
    mem.remember(RememberInput {
        scope: "global".into(),
        session_id: None,
        items: vec![MemoryItem {
            kind: MemoryKind::Note,
            text: "new row with vector".into(),
            importance: None,
        }],
    })
    .await
    .unwrap();

    // Both rows should now be embedded (none missing).
    // Access via a fresh recall that exercises the vector path without panicking.
    let hits = mem
        .recall(wukong_memory::RecallQuery {
            query: "row".into(),
            top_k: 5,
            scope: Some("global".into()),
            mode: wukong_memory::RecallMode::Hybrid,
        })
        .await
        .unwrap();
    assert!(!hits.data.is_empty());
}
```

- [ ] **Step 3: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory`
Expected: 全綠（此測試現會走到 recall 的向量分支——但 Task 9 才接 recall 向量源；目前 recall 尚未呼叫向量源，測試仍應通過因為 keyword/recent 已能回 "row"）。

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-memory/src/lib.rs crates/wukong-memory/tests/integration.rs
git commit -m "feat(memory): embed on remember and background backfill"
```

---

### Task 9: recall 串接向量候選源

**Files:**
- Modify: `crates/wukong-memory/src/lib.rs`（`recall` 內接向量源）

- [ ] **Step 1: 在 recall 注入向量源**

在 `crates/wukong-memory/src/lib.rs` 的 `use recall::{...}` 匯入清單加入兩個新 helper：

```rust
use recall::{
    apply_vector_sims, build_vector_candidates, filter_by_scope, fts_match_string, is_trivial,
    merge_candidates, rank, sources_for_mode,
};
```

把 `recall` 內取得 `sources_for_mode` 的那行改為 3-tuple，並在組出 `merged` 後、`filter_by_scope` 前插入向量源融合：

```rust
        let (use_keyword, use_recent, use_vector) = sources_for_mode(query.mode);
```

```rust
        let merged: Vec<Candidate> = match query.mode {
            RecallMode::Keyword => keyword,
            RecallMode::Tree => recent,
            RecallMode::Hybrid => merge_candidates(keyword, recent),
        };

        // Vector source: only when enabled by mode AND an embedder is attached.
        let merged = if use_vector {
            match &self.embedder {
                Some(emb) => {
                    let qvec = emb.embed(&query.query)?;
                    let embedded = self.store.embedded_candidates(limit).await?;
                    let vector_cands =
                        build_vector_candidates(&qvec, embedded, query.top_k.max(5) * 4);
                    apply_vector_sims(merged, vector_cands)
                }
                None => merged,
            }
        } else {
            merged
        };

        let filtered = filter_by_scope(merged, &scope_filter);
```

（`limit` 變數已在前面以 `fetch_limit(query.top_k)` 計得，直接複用。）

- [ ] **Step 2: 加語意排序整合測試（用 stub embedder 證明語意影響排名）**

在 `crates/wukong-memory/tests/integration.rs` 末端新增。此測試用一個受控 stub embedder：把含 "alpha" 的文字映到向量 A、其餘映到正交向量 B，藉此在「兩列都由 recent 源回來、bm25 皆無」時，證明語意相似者排前：

```rust
#[tokio::test]
async fn semantic_similarity_boosts_ranking() {
    use std::sync::Arc;
    use wukong_memory::{
        Embedder, Memory, MemoryItem, MemoryKind, RecallMode, RecallQuery, RememberInput, Result,
    };

    struct StubEmbedder;
    impl Embedder for StubEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            // "alpha"-bearing text → [1,0]; everything else → [0,1] (orthogonal).
            if text.contains("alpha") {
                Ok(vec![1.0, 0.0])
            } else {
                Ok(vec![0.0, 1.0])
            }
        }
        fn dim(&self) -> usize {
            2
        }
        fn model_id(&self) -> &str {
            "stub"
        }
    }

    let file = tempfile::NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);

    let mem = Memory::open(&url)
        .await
        .unwrap()
        .with_embedder(Arc::new(StubEmbedder));

    // Two memories created at the same logical time; neither shares a query token.
    mem.remember(RememberInput {
        scope: "global".into(),
        session_id: None,
        items: vec![
            MemoryItem { kind: MemoryKind::Note, text: "zzz alpha zzz".into(), importance: None },
            MemoryItem { kind: MemoryKind::Note, text: "yyy beta yyy".into(), importance: None },
        ],
    })
    .await
    .unwrap();
    mem.backfill_embeddings().await.unwrap();

    // Query embeds to [1,0] (contains "alpha"); semantically matches row 1.
    // Use a query token ("query") absent from both rows so keyword source is empty;
    // both rows still arrive via the recency source, so ranking decides order.
    let hits = mem
        .recall(RecallQuery {
            query: "alpha query".into(),
            top_k: 2,
            scope: Some("global".into()),
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert_eq!(hits.data.len(), 2);
    assert!(
        hits.data[0].text.contains("alpha"),
        "semantic match should rank first, got: {:?}",
        hits.data.iter().map(|h| &h.text).collect::<Vec<_>>()
    );
}
```

- [ ] **Step 3: 跑全部測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory`
Expected: 全綠（含 `semantic_similarity_boosts_ranking`）。

- [ ] **Step 4: 確認 feature 關閉時整體編譯與行為不變**

Run: `. "$HOME/.cargo/env" && set -o pipefail && cargo build && cargo test 2>&1 | tail -20`
Expected: workspace 全綠；wukong-cli / wukong-memoryd 因 `Memory::open` 簽名未變而不受影響。

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-memory/src/lib.rs crates/wukong-memory/tests/integration.rs
git commit -m "feat(memory): wire vector candidate source into recall"
```

---

### Task 10: FastembedBackend（feature `embed`）與 CLI 接線

**Files:**
- Modify: `crates/wukong-memory/Cargo.toml`（feature + 選用相依）
- Modify: `crates/wukong-memory/src/embed/mod.rs`（FastembedBackend）
- Modify: `crates/wukong-cli/Cargo.toml`（透傳 feature）
- Modify: `crates/wukong-cli/src/main.rs`（讀 WUKONG_EMBED 接 embedder）

- [ ] **Step 1: 在 wukong-memory 加 feature 與選用相依**

編輯 `crates/wukong-memory/Cargo.toml`，在 `[dependencies]` 後加：

```toml
[dependencies.fastembed]
version = "4"
optional = true

[features]
embed = ["dep:fastembed"]
```

- [ ] **Step 2: 加 FastembedBackend（feature-gated）**

在 `crates/wukong-memory/src/embed/mod.rs` 末端（`#[cfg(test)]` 之前）加：

```rust
/// Real local embedder backed by fastembed (ONNX). Behind the `embed` feature
/// so the default build pulls no heavy dependencies. Default model:
/// all-MiniLM-L6-v2 (384 dims). First use downloads the model to cache.
#[cfg(feature = "embed")]
pub struct FastembedBackend {
    model: fastembed::TextEmbedding,
    dim: usize,
    model_id: String,
}

#[cfg(feature = "embed")]
impl FastembedBackend {
    /// Construct with the default all-MiniLM-L6-v2 model.
    pub fn new() -> Result<Self> {
        use fastembed::{EmbeddingModel, InitOptions, TextEmbedding};
        let model = TextEmbedding::try_new(
            InitOptions::new(EmbeddingModel::AllMiniLML6V2).with_show_download_progress(false),
        )
        .map_err(|e| crate::error::MemoryError::Embed(e.to_string()))?;
        Ok(Self {
            model,
            dim: 384,
            model_id: "all-MiniLM-L6-v2".to_string(),
        })
    }
}

#[cfg(feature = "embed")]
impl Embedder for FastembedBackend {
    fn embed(&self, text: &str) -> Result<Vec<f32>> {
        let mut out = self
            .model
            .embed(vec![text], None)
            .map_err(|e| crate::error::MemoryError::Embed(e.to_string()))?;
        out.pop()
            .ok_or_else(|| crate::error::MemoryError::Embed("empty embedding".to_string()))
    }

    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>> {
        self.model
            .embed(texts.to_vec(), None)
            .map_err(|e| crate::error::MemoryError::Embed(e.to_string()))
    }

    fn dim(&self) -> usize {
        self.dim
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }
}
```

並在 lib.rs re-export 區加（feature-gated）：

```rust
#[cfg(feature = "embed")]
pub use embed::FastembedBackend;
```

- [ ] **Step 3: 確認 feature 開啟也能編譯**

Run: `. "$HOME/.cargo/env" && cargo build -p wukong-memory --features embed 2>&1 | tail -20`
Expected: 編譯成功（首次會下載 fastembed 相依；若 fastembed 4.x API 名稱有出入，依該版文件調整 `InitOptions`/`EmbeddingModel` 呼叫，語義不變）。

- [ ] **Step 4: CLI 透傳 feature**

編輯 `crates/wukong-cli/Cargo.toml`，在 `[features]`（無則新增該段）加：

```toml
[features]
embed = ["wukong-memory/embed"]
```

- [ ] **Step 5: CLI 讀 WUKONG_EMBED 接 embedder**

在 `crates/wukong-cli/src/main.rs` 建立 `Memory` 之後（`Memory::open(...).await?` 那行之後），加上 feature-gated 接線：

```rust
    #[cfg(feature = "embed")]
    let memory = if std::env::var("WUKONG_EMBED").as_deref() == Ok("1") {
        match wukong_memory::FastembedBackend::new() {
            Ok(backend) => memory.with_embedder(std::sync::Arc::new(backend)),
            Err(e) => {
                eprintln!("🐵 語意層停用（模型載入失敗）：{e}");
                memory
            }
        }
    } else {
        memory
    };
```

（`memory` 變數需為 `let mut` 或以 shadowing 重綁；上式以 shadowing 重綁，故原宣告維持 `let memory = ...` 即可。確認其後使用 `memory` 之處型別不變。）

- [ ] **Step 6: 確認預設與 feature 兩種建置都綠**

Run: `. "$HOME/.cargo/env" && set -o pipefail && cargo build && cargo build -p wukong-cli --features embed 2>&1 | tail -20`
Expected: 兩者皆成功。

- [ ] **Step 7: 手動煙霧驗證（需網路下載模型，一次性）**

Run:
```bash
. "$HOME/.cargo/env" && rm -f /tmp/wk-embed.db* && \
WUKONG_EMBED=1 cargo run -p wukong-cli --features embed -- \
  --db "sqlite:///tmp/wk-embed.db" --scope global --agent-cmd "opencode run" \
  "記住：我習慣用 4 空格縮排"
```
Expected: 正常回合輸出；`sqlite3 /tmp/wk-embed.db "SELECT count(*) FROM memories WHERE embedding IS NOT NULL;"` 回 ≥ 1。清理：`rm -f /tmp/wk-embed.db*`。

> 備註：底層 agent 一律以 **opencode** 為準（用戶指示，不用 claude）。embedding 本身不需 agent，但端到端煙霧仍走真實回合。

- [ ] **Step 8: Commit**

```bash
git add crates/wukong-memory/Cargo.toml crates/wukong-memory/src/embed/mod.rs \
        crates/wukong-memory/src/lib.rs crates/wukong-cli/Cargo.toml \
        crates/wukong-cli/src/main.rs
git commit -m "feat: add fastembed backend and WUKONG_EMBED CLI wiring"
```

---

## 完成後

全部任務完成、`cargo test` 全綠後，套用 **superpowers:finishing-a-development-branch** 收尾（驗測試 → 選 merge/PR/keep/discard）。

更新文件（非阻塞，可併入收尾）：
- `crates/wukong-memory/README.md`：補語意召回段（feature `embed`、WUKONG_EMBED、MiniLM 384 維、退場行為）。
- 根 `README.md` 記憶模型段：把「語意向量召回」從 roadmap 移到已實作，標註為選用增強層。

---

## Self-Review

**1. Spec coverage：**
- Embedder trait + Fastembed + Mock + feature `embed` → Task 1/2/10 ✓
- `embedding`/`embedding_model` 欄位 + idempotent 遷移 → Task 6 ✓
- 純 Rust 暴力 cosine、scope 內取候選 → Task 1（cosine）、Task 7（embedded_candidates）、Task 9（recall 串接）✓
- Candidate.vector_sim + 四項 combined_score + min-max → Task 3/4 ✓
- Hybrid 含向量、Keyword/Tree 不變 → Task 4（sources_for_mode）、Task 9 ✓
- Adaptive gate 不變（is_trivial 早退）→ 未改動 recall 早退邏輯 ✓
- 開機背景補齊 + 失敗不影響主流程 → Task 8（with_embedder spawn + backfill，錯誤僅 eprintln）✓
- 退場（無 feature/未啟用/載入失敗 → BM25）→ Task 8（embedder None 路徑）、Task 10（new() 失敗 fallback）✓
- MemoryError::Embed → Task 5 ✓
- 測試策略（cosine/blob/rank/端到端/補齊/feature 關閉等同 v1）→ Task 1/4/8/9 + Task 9 Step 4 回歸 ✓

**2. Placeholder scan：** 無 TBD/TODO；每個改碼步驟均附完整程式碼與預期輸出。

**3. Type consistency：** `Weights` 四欄一致；`combined_score(lexical, semantic, age, importance, recall, w)` 在 Task 3 定義、Task 4 rank 呼叫一致；`Candidate.vector_sim: Option<f64>` 在 Task 4 定義、Task 7 embedded_candidates 與 Task 9 build_vector_candidates 使用一致；`Embedder`/`embedding_to_blob`/`blob_to_embedding`/`cosine_similarity` 命名跨 Task 1/2/7/8/9/10 一致；`sources_for_mode` 3-tuple 在 Task 4 定義、Task 9 解構一致。
