# 記憶維護(Consolidation / Prune / Markdown / Snapshot)實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 為 `wukong-memory` 加上記憶聚合(consolidation)、安全修剪(prune)、markdown 雙持久化與健康快照(snapshot),全部由 `wukong memory <op>` 子命令手動觸發。

**Architecture:** 核心邏輯放 `wukong-memory`(純 lib,沿用 `Embedder` 的 trait 注入模式新增 `Summarizer`);`wukong-cli` 二進位新增 `memory` 子命令群並注入 opencode 版 `Summarizer`;`wukong-memoryd` 共用 snapshot 統計層。DB 永遠是真相來源,markdown 為單向衍生鏡像。

**Tech Stack:** Rust 2021、sqlx 0.8(SQLite + FTS5)、clap 4(subcommand)、tokio、axum、serde。

**慣例提醒:** cargo 不在 PATH,所有 cargo 指令前綴 `. "$HOME/.cargo/env" &&`;測試+commit 串接時 `set -o pipefail`;sqlite 測試用 `NamedTempFile` + `std::mem::forget`。**git commit 訊息只寫功能描述,絕不含任何 AI 署名。**

---

## 檔案結構

- `crates/wukong-memory/src/store/mod.rs`(改):schema 加 `consolidated_into` 欄與 AFTER DELETE 觸發器;新增 consolidation / prune / snapshot 查詢。
- `crates/wukong-memory/src/model.rs`(改):新增 `KindCount`、`AgeBuckets`、`EmbeddingCoverage`、`Snapshot`。
- `crates/wukong-memory/src/consolidate.rs`(新):`Summarizer` trait、`ConcatSummarizer`、`MockSummarizer`、`ConsolidatePolicy`、`ConsolidatePlan`、`plan_batches` 純函式。
- `crates/wukong-memory/src/prune.rs`(新):`PrunePolicy`。
- `crates/wukong-memory/src/markdown.rs`(新):`render_markdown_entry`、`scope_to_filename` 純函式、`MarkdownSink`。
- `crates/wukong-memory/src/lib.rs`(改):`Memory` 加 `md_sink` 欄與 `with_markdown`;新增 `plan_consolidation`、`consolidate`、`plan_prune`、`prune`、`snapshot`、`export`。
- `crates/wukong-memory/src/error.rs`(改):新增 `Io` 變體。
- `crates/wukong-memoryd/src/lib.rs`(改):新增 `GET /v1/snapshot`。
- `crates/wukong-gateway/src/summarize.rs`(新):`OpencodeSummarizer`。
- `crates/wukong-gateway/src/cli.rs`(改):新增 `Command` / `MemoryOp` 子命令。
- `crates/wukong-cli/src/main.rs`(改):分派 memory 子命令、依 `WUKONG_MD_DIR` 注入 markdown。
- READMEs(改)。

---

## Task 1: schema 加 `consolidated_into` 欄與 FTS 刪除同步

**Files:**
- Modify: `crates/wukong-memory/src/store/mod.rs`

- [ ] **Step 1: 寫失敗測試**

在 `crates/wukong-memory/src/store/mod.rs` 的 `mod tests` 內新增:

```rust
    #[tokio::test]
    async fn migrate_adds_consolidated_into_column() {
        let store = test_store().await;
        let cols = sqlx::query("PRAGMA table_info(memories)")
            .fetch_all(&store.pool)
            .await
            .unwrap();
        let names: Vec<String> = cols.iter().map(|r| r.get::<String, _>("name")).collect();
        assert!(names.iter().any(|n| n == "consolidated_into"));
    }

    #[tokio::test]
    async fn delete_keeps_fts_in_sync() {
        let store = test_store().await;
        let id = store
            .insert_memory(None, "global", MemoryKind::Note, "deletable widget", 1.0, 100)
            .await
            .unwrap();
        assert_eq!(store.keyword_candidates("\"widget\"", 10).await.unwrap().len(), 1);
        sqlx::query("DELETE FROM memories WHERE id = ?1")
            .bind(id)
            .execute(&store.pool)
            .await
            .unwrap();
        // FTS index must no longer match the deleted row.
        assert_eq!(store.keyword_candidates("\"widget\"", 10).await.unwrap().len(), 0);
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory migrate_adds_consolidated_into_column delete_keeps_fts_in_sync`
Expected: FAIL(`consolidated_into` 欄不存在;刪除後 FTS 仍匹配)。

- [ ] **Step 3: 加 AFTER DELETE 觸發器到 SCHEMA**

在 `SCHEMA` 常數內、`memories_ai` 觸發器之後加上(外容 FTS5 的標準刪除同步寫法):

```rust
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.id, old.text);
END;
```

- [ ] **Step 4: migrate 加 `consolidated_into` 欄**

在 `migrate` 函式內、加完 `embedding_model` 那段之後加上:

```rust
    if !names.iter().any(|n| n == "consolidated_into") {
        sqlx::query("ALTER TABLE memories ADD COLUMN consolidated_into INTEGER")
            .execute(pool)
            .await?;
    }
```

- [ ] **Step 5: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory`
Expected: 全綠(新增 2 測試通過)。

- [ ] **Step 6: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/store/mod.rs
git commit -m "feat(memory): add consolidated_into column and FTS delete sync"
```

---

## Task 2: `Summarizer` trait 與預設/Mock 實作

**Files:**
- Create: `crates/wukong-memory/src/consolidate.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: 建模組並掛上 lib**

在 `crates/wukong-memory/src/lib.rs` 的模組宣告區(現有 `pub mod store;` 附近)加:

```rust
pub mod consolidate;
```

並在 `pub use` 區加(暫先匯出 trait 與型別,後續 Task 會用到):

```rust
pub use consolidate::{ConcatSummarizer, ConsolidatePlan, ConsolidatePolicy, MockSummarizer, Summarizer};
```

- [ ] **Step 2: 寫失敗測試**

建立 `crates/wukong-memory/src/consolidate.rs`,先放型別骨架與測試:

```rust
//! Consolidation: fold scattered event/note memories into Summary memories.
//! The memory layer stays LLM-agnostic via the `Summarizer` trait, mirroring
//! the `Embedder` pattern. A real LLM-backed summarizer is injected from the
//! cli/gateway layer; `ConcatSummarizer` is the dependency-free default.

use crate::error::Result;

/// Condenses a group of memory texts into a single summary string.
/// Object-safe (sync) so callers can hold `&dyn Summarizer`.
pub trait Summarizer: Send + Sync {
    fn summarize(&self, texts: &[String]) -> Result<String>;
}

/// Mechanical default: joins texts in order under a header. No LLM.
pub struct ConcatSummarizer;

impl Summarizer for ConcatSummarizer {
    fn summarize(&self, texts: &[String]) -> Result<String> {
        Ok(format!("[摘要] {}", texts.join(" / ")))
    }
}

/// Deterministic summarizer for tests. NOT semantic.
pub struct MockSummarizer;

impl Summarizer for MockSummarizer {
    fn summarize(&self, texts: &[String]) -> Result<String> {
        Ok(format!("SUMMARY({})", texts.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_summarizer_joins_texts() {
        let s = ConcatSummarizer;
        let out = s.summarize(&["a".to_string(), "b".to_string()]).unwrap();
        assert!(out.contains("a / b"));
    }

    #[test]
    fn mock_summarizer_reports_count() {
        let s = MockSummarizer;
        assert_eq!(s.summarize(&["x".to_string(), "y".to_string()]).unwrap(), "SUMMARY(2)");
    }
}
```

注意:Step 1 的 `pub use` 引用了 `ConsolidatePlan`、`ConsolidatePolicy`,它們在 Task 4 才定義。為讓本任務可獨立編譯,先把 `pub use` 改為只匯出本任務已存在的項目:

```rust
pub use consolidate::{ConcatSummarizer, MockSummarizer, Summarizer};
```

(Task 4 會把 `ConsolidatePlan`、`ConsolidatePolicy` 加回此 `pub use`。)

- [ ] **Step 3: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory consolidate`
Expected: PASS(2 測試)。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/consolidate.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add Summarizer trait with concat and mock impls"
```

---

## Task 3: store 層 consolidation 查詢

**Files:**
- Modify: `crates/wukong-memory/src/store/mod.rs`

- [ ] **Step 1: 寫失敗測試**

在 `mod tests` 內新增:

```rust
    #[tokio::test]
    async fn consolidation_candidates_excludes_consolidated_and_nonfoldable() {
        let store = test_store().await;
        let e1 = store.insert_memory(Some("s1"), "project:X", MemoryKind::Event, "e1", 1.0, 100).await.unwrap();
        let _e2 = store.insert_memory(None, "project:X", MemoryKind::Note, "n1", 2.0, 110).await.unwrap();
        // Decision is never foldable.
        let _d = store.insert_memory(None, "project:X", MemoryKind::Decision, "d1", 1.0, 120).await.unwrap();
        // Different scope must not appear.
        let _o = store.insert_memory(None, "global", MemoryKind::Event, "other", 1.0, 130).await.unwrap();
        // Mark e1 as already consolidated.
        store.mark_consolidated(&[e1], 999).await.unwrap();

        let rows = store.consolidation_candidates("project:X").await.unwrap();
        // Only the unconsolidated Note (n1) remains.
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "n1");
        assert_eq!(rows[0].importance, 2.0);
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory consolidation_candidates_excludes`
Expected: FAIL(`mark_consolidated`、`consolidation_candidates` 未定義)。

- [ ] **Step 3: 加型別與方法**

在 `crates/wukong-memory/src/store/mod.rs`,於 `Candidate` 定義之後加:

```rust
/// A row eligible for consolidation (event/note, not yet consolidated).
#[derive(Debug, Clone)]
pub struct ConsolidationRow {
    pub id: i64,
    pub session_id: Option<String>,
    pub text: String,
    pub importance: f64,
}
```

在 `impl Store` 內(任一方法之後)加:

```rust
    /// Foldable rows in one scope: event/note kinds not yet consolidated,
    /// oldest first.
    pub async fn consolidation_candidates(&self, scope: &str) -> Result<Vec<ConsolidationRow>> {
        let rows = sqlx::query(
            "SELECT id, session_id, text, importance
             FROM memories
             WHERE scope = ?1
               AND kind IN ('event','note')
               AND consolidated_into IS NULL
             ORDER BY created_at ASC, id ASC",
        )
        .bind(scope)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ConsolidationRow {
                id: r.get::<i64, _>("id"),
                session_id: r.get::<Option<String>, _>("session_id"),
                text: r.get::<String, _>("text"),
                importance: r.get::<f64, _>("importance"),
            })
            .collect())
    }

    /// Mark the given rows as folded into `summary_id`.
    pub async fn mark_consolidated(&self, ids: &[i64], summary_id: i64) -> Result<()> {
        for id in ids {
            sqlx::query("UPDATE memories SET consolidated_into = ?2 WHERE id = ?1")
                .bind(id)
                .bind(summary_id)
                .execute(&self.pool)
                .await?;
        }
        Ok(())
    }
```

- [ ] **Step 4: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory consolidation_candidates_excludes`
Expected: PASS。

- [ ] **Step 5: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/store/mod.rs
git commit -m "feat(memory): add store consolidation queries"
```

---

## Task 4: 批次規劃 + `Memory::plan_consolidation` / `consolidate`

**Files:**
- Modify: `crates/wukong-memory/src/consolidate.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: 寫 `plan_batches` 失敗測試**

在 `crates/wukong-memory/src/consolidate.rs` 的 `mod tests` 內加:

```rust
    use crate::store::ConsolidationRow;

    fn row(id: i64, session: Option<&str>) -> ConsolidationRow {
        ConsolidationRow { id, session_id: session.map(|s| s.to_string()), text: format!("t{id}"), importance: 1.0 }
    }

    #[test]
    fn plan_batches_groups_session_then_chunks_null() {
        let rows = vec![
            row(1, Some("a")),
            row(2, Some("a")),
            row(3, None),
            row(4, None),
            row(5, None),
        ];
        let batches = plan_batches(rows, 2);
        // Session "a" batched together first, then null rows chunked by 2.
        let ids: Vec<Vec<i64>> = batches
            .iter()
            .map(|b| b.iter().map(|r| r.id).collect())
            .collect();
        assert_eq!(ids, vec![vec![1, 2], vec![3, 4], vec![5]]);
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory plan_batches_groups`
Expected: FAIL(`plan_batches`、`ConsolidatePolicy` 等未定義)。

- [ ] **Step 3: 加 policy / plan 型別與 `plan_batches`**

在 `crates/wukong-memory/src/consolidate.rs` 的 `use` 之後、trait 之前加:

```rust
use crate::store::ConsolidationRow;

/// Tuning for a consolidation pass.
#[derive(Debug, Clone)]
pub struct ConsolidatePolicy {
    /// Max source rows folded into one summary.
    pub batch_size: usize,
}

impl Default for ConsolidatePolicy {
    fn default() -> Self {
        Self { batch_size: 20 }
    }
}

/// A dry-run plan: the source ids that would form each summary.
#[derive(Debug, Clone)]
pub struct ConsolidatePlan {
    pub batches: Vec<Vec<i64>>,
}

/// Group foldable rows into batches: rows sharing a session_id stay together
/// (first-seen order), null-session rows are chunked by `batch_size`. Every
/// group is itself capped at `batch_size`.
pub fn plan_batches(rows: Vec<ConsolidationRow>, batch_size: usize) -> Vec<Vec<ConsolidationRow>> {
    let bs = batch_size.max(1);
    let mut sessions: Vec<(String, Vec<ConsolidationRow>)> = Vec::new();
    let mut null_rows: Vec<ConsolidationRow> = Vec::new();
    for r in rows {
        match &r.session_id {
            Some(sid) => {
                if let Some(slot) = sessions.iter_mut().find(|(k, _)| k == sid) {
                    slot.1.push(r);
                } else {
                    sessions.push((sid.clone(), vec![r]));
                }
            }
            None => null_rows.push(r),
        }
    }
    let mut out: Vec<Vec<ConsolidationRow>> = Vec::new();
    for (_, group) in sessions {
        for chunk in group.chunks(bs) {
            out.push(chunk.to_vec());
        }
    }
    for chunk in null_rows.chunks(bs) {
        out.push(chunk.to_vec());
    }
    out
}
```

注意:`ConsolidationRow` 需可 `Clone`(Task 3 已 derive Clone,符合)。

- [ ] **Step 4: 把新型別加回 lib `pub use`**

在 `crates/wukong-memory/src/lib.rs` 把 Task 2 的 `pub use consolidate::{...}` 改為:

```rust
pub use consolidate::{
    plan_batches, ConcatSummarizer, ConsolidatePlan, ConsolidatePolicy, MockSummarizer, Summarizer,
};
```

- [ ] **Step 5: 跑測試確認 `plan_batches` 通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory plan_batches_groups`
Expected: PASS。

- [ ] **Step 6: 寫 `Memory::consolidate` 失敗測試**

在 `crates/wukong-memory/src/lib.rs` 的 `mod tests`(若無則於檔尾新增 `#[cfg(test)] mod tests { ... }`)加。先確認檔案是否已有測試模組;若無,新增:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::consolidate::MockSummarizer;
    use crate::model::{MemoryItem, RememberInput};
    use tempfile::NamedTempFile;

    async fn open_mem() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    async fn remember_event(mem: &Memory, scope: &str, text: &str) {
        mem.remember(RememberInput {
            scope: scope.to_string(),
            session_id: None,
            items: vec![MemoryItem { kind: MemoryKind::Event, text: text.to_string(), importance: None }],
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn consolidate_creates_summary_and_marks_sources() {
        let mem = open_mem().await;
        remember_event(&mem, "project:X", "did A").await;
        remember_event(&mem, "project:X", "did B").await;

        let plan = mem
            .plan_consolidation("project:X", &ConsolidatePolicy { batch_size: 20 })
            .await
            .unwrap();
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(plan.batches[0].len(), 2);

        let summary_ids = mem
            .consolidate("project:X", &ConsolidatePolicy { batch_size: 20 }, &MockSummarizer)
            .await
            .unwrap();
        assert_eq!(summary_ids.len(), 1);

        // Sources are now consolidated => no longer candidates.
        let after = mem
            .plan_consolidation("project:X", &ConsolidatePolicy { batch_size: 20 })
            .await
            .unwrap();
        assert!(after.batches.is_empty());

        // The summary text came from the summarizer.
        let recent = mem.store.recent_candidates(10).await.unwrap();
        assert!(recent.iter().any(|c| c.kind == MemoryKind::Summary && c.text == "SUMMARY(2)"));
    }
}
```

- [ ] **Step 7: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory consolidate_creates_summary`
Expected: FAIL(`plan_consolidation`、`consolidate` 未定義)。

- [ ] **Step 8: 實作 `Memory::plan_consolidation` / `consolidate`**

在 `crates/wukong-memory/src/lib.rs` 的 `impl Memory` 內(`stats` 之後)加。先在檔頭 `use` 區補:

```rust
use consolidate::{plan_batches, ConsolidatePlan, ConsolidatePolicy, Summarizer};
```

(若與 `pub use` 重複導致警告,改用完整路徑 `crate::consolidate::...` 於方法內。)方法:

```rust
    /// Plan (without executing) which source ids would fold into each summary.
    pub async fn plan_consolidation(
        &self,
        scope: &str,
        policy: &ConsolidatePolicy,
    ) -> Result<ConsolidatePlan> {
        let rows = self.store.consolidation_candidates(scope).await?;
        let batches = plan_batches(rows, policy.batch_size)
            .into_iter()
            .map(|b| b.iter().map(|r| r.id).collect())
            .collect();
        Ok(ConsolidatePlan { batches })
    }

    /// Execute consolidation: for each batch, summarize the texts, insert a
    /// Summary memory, and mark the sources as folded into it. Returns the new
    /// summary ids. Each new summary is embedded (if an embedder is attached)
    /// and mirrored to markdown (if a sink is attached).
    pub async fn consolidate(
        &self,
        scope: &str,
        policy: &ConsolidatePolicy,
        summarizer: &dyn Summarizer,
    ) -> Result<Vec<i64>> {
        let rows = self.store.consolidation_candidates(scope).await?;
        let batches = plan_batches(rows, policy.batch_size);
        let now = now_unix();
        let mut summary_ids = Vec::with_capacity(batches.len());
        for batch in batches {
            if batch.is_empty() {
                continue;
            }
            let texts: Vec<String> = batch.iter().map(|r| r.text.clone()).collect();
            let importance = batch.iter().map(|r| r.importance).fold(0.0_f64, f64::max);
            let summary_text = summarizer.summarize(&texts)?;
            let summary_id = self
                .store
                .insert_memory(None, scope, MemoryKind::Summary, &summary_text, importance, now)
                .await?;
            let src_ids: Vec<i64> = batch.iter().map(|r| r.id).collect();
            self.store.mark_consolidated(&src_ids, summary_id).await?;
            if let Some(emb) = &self.embedder {
                if let Ok(v) = emb.embed(&summary_text) {
                    let _ = self
                        .store
                        .update_embedding(summary_id, &embedding_to_blob(&v), emb.model_id())
                        .await;
                }
            }
            if let Some(sink) = &self.md_sink {
                if let Err(e) = sink.append(scope, now, MemoryKind::Summary, &summary_text) {
                    eprintln!("wukong-memory: markdown append failed: {e}");
                }
            }
            summary_ids.push(summary_id);
        }
        Ok(summary_ids)
    }
```

注意:此處引用了 `self.md_sink`(Task 9/10 才加欄位)。為讓本任務可獨立編譯,本步驟先**省略** `if let Some(sink) = &self.md_sink { ... }` 整段;Task 10 會把這段補回。其餘照寫。

- [ ] **Step 9: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory`
Expected: 全綠。

- [ ] **Step 10: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/consolidate.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): consolidate events into summaries via Summarizer"
```

---

## Task 5: Prune

**Files:**
- Create: `crates/wukong-memory/src/prune.rs`
- Modify: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: 建 `PrunePolicy` 並掛上 lib**

建立 `crates/wukong-memory/src/prune.rs`:

```rust
//! Prune policy: which memories are safe to delete.

/// Thresholds for the low-value fallback path. The "already consolidated" path
/// needs no thresholds.
#[derive(Debug, Clone)]
pub struct PrunePolicy {
    /// Rows older than this many seconds may be pruned (fallback path).
    pub max_age_secs: i64,
    /// Rows with importance strictly below this may be pruned (fallback path).
    pub importance_floor: f64,
}

impl Default for PrunePolicy {
    fn default() -> Self {
        Self { max_age_secs: 30 * 86_400, importance_floor: 0.5 }
    }
}
```

在 `crates/wukong-memory/src/lib.rs` 模組區加 `pub mod prune;`,`pub use` 區加 `pub use prune::PrunePolicy;`。

- [ ] **Step 2: 寫 store prune 失敗測試**

在 `crates/wukong-memory/src/store/mod.rs` 的 `mod tests` 內加:

```rust
    #[tokio::test]
    async fn prune_candidates_matches_consolidated_or_low_value() {
        let store = test_store().await;
        let now = 1_000_000_000i64;
        let old = now - 40 * 86_400; // 40 days old
        // (a) consolidated event => prunable via main path.
        let a = store.insert_memory(None, "project:X", MemoryKind::Event, "a", 1.0, old).await.unwrap();
        store.mark_consolidated(&[a], 999).await.unwrap();
        // (b) old, never recalled, low importance note => prunable via fallback.
        let b = store.insert_memory(None, "project:X", MemoryKind::Note, "b", 0.2, old).await.unwrap();
        // (c) old but high importance => NOT prunable.
        let _c = store.insert_memory(None, "project:X", MemoryKind::Note, "c", 0.9, old).await.unwrap();
        // (d) decision is never prunable, even if old/low.
        let _d = store.insert_memory(None, "project:X", MemoryKind::Decision, "d", 0.1, old).await.unwrap();
        // (e) recent low-importance note => NOT prunable (too new).
        let _e = store.insert_memory(None, "project:X", MemoryKind::Note, "e", 0.1, now).await.unwrap();

        let ids = store.prune_candidates(Some("project:X"), 30 * 86_400, 0.5, now).await.unwrap();
        let mut ids = ids;
        ids.sort();
        assert_eq!(ids, vec![a, b]);
    }

    #[tokio::test]
    async fn delete_memories_removes_rows_and_fts() {
        let store = test_store().await;
        let id = store.insert_memory(None, "global", MemoryKind::Note, "gizmo", 1.0, 100).await.unwrap();
        let n = store.delete_memories(&[id]).await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(store.recent_candidates(10).await.unwrap().len(), 0);
        assert_eq!(store.keyword_candidates("\"gizmo\"", 10).await.unwrap().len(), 0);
    }
```

- [ ] **Step 3: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory prune_candidates_matches delete_memories_removes`
Expected: FAIL(`prune_candidates`、`delete_memories` 未定義)。

- [ ] **Step 4: 實作 store 方法**

在 `impl Store` 內加:

```rust
    /// Ids eligible for pruning: consolidated rows, OR old + never-recalled +
    /// low-importance event/note rows. Never decision/skill/summary.
    pub async fn prune_candidates(
        &self,
        scope: Option<&str>,
        max_age_secs: i64,
        importance_floor: f64,
        now: i64,
    ) -> Result<Vec<i64>> {
        let cutoff = now - max_age_secs;
        let mut sql = String::from(
            "SELECT id FROM memories
             WHERE (
                 consolidated_into IS NOT NULL
                 OR (kind IN ('event','note') AND created_at < ?1 AND recall_count = 0 AND importance < ?2)
             )",
        );
        if scope.is_some() {
            sql.push_str(" AND scope = ?3");
        }
        let mut q = sqlx::query(&sql).bind(cutoff).bind(importance_floor);
        if let Some(s) = scope {
            q = q.bind(s);
        }
        let rows = q.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(|r| r.get::<i64, _>("id")).collect())
    }

    /// Delete the given memory rows. The AFTER DELETE trigger keeps FTS in sync.
    /// Returns the number of rows removed.
    pub async fn delete_memories(&self, ids: &[i64]) -> Result<u64> {
        let mut affected = 0u64;
        for id in ids {
            let r = sqlx::query("DELETE FROM memories WHERE id = ?1")
                .bind(id)
                .execute(&self.pool)
                .await?;
            affected += r.rows_affected();
        }
        Ok(affected)
    }
```

- [ ] **Step 5: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory prune_candidates_matches delete_memories_removes`
Expected: PASS。

- [ ] **Step 6: 寫 `Memory::plan_prune` / `prune` 失敗測試**

在 `crates/wukong-memory/src/lib.rs` 的 `mod tests` 內加:

```rust
    #[tokio::test]
    async fn prune_deletes_only_low_value() {
        let mem = open_mem().await;
        // Two events, then consolidate so they become prunable.
        remember_event(&mem, "project:X", "did A").await;
        remember_event(&mem, "project:X", "did B").await;
        mem.consolidate("project:X", &ConsolidatePolicy::default(), &MockSummarizer)
            .await
            .unwrap();

        // dry-run plan lists the two consolidated source events.
        let plan = mem.plan_prune(Some("project:X"), &PrunePolicy::default()).await.unwrap();
        assert_eq!(plan.len(), 2);

        let deleted = mem.prune(Some("project:X"), &PrunePolicy::default()).await.unwrap();
        assert_eq!(deleted, 2);

        // The summary survives (never prunable).
        let recent = mem.store.recent_candidates(10).await.unwrap();
        assert!(recent.iter().all(|c| c.kind == MemoryKind::Summary));
    }
```

- [ ] **Step 7: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory prune_deletes_only_low_value`
Expected: FAIL(`plan_prune`、`prune` 未定義)。

- [ ] **Step 8: 實作 `Memory::plan_prune` / `prune`**

在 `impl Memory` 內加:

```rust
    /// Ids that `prune` would delete (dry-run).
    pub async fn plan_prune(
        &self,
        scope: Option<&str>,
        policy: &prune::PrunePolicy,
    ) -> Result<Vec<i64>> {
        self.store
            .prune_candidates(scope, policy.max_age_secs, policy.importance_floor, now_unix())
            .await
    }

    /// Delete prunable rows. Returns the number deleted.
    pub async fn prune(
        &self,
        scope: Option<&str>,
        policy: &prune::PrunePolicy,
    ) -> Result<u64> {
        let ids = self.plan_prune(scope, policy).await?;
        if ids.is_empty() {
            return Ok(0);
        }
        self.store.delete_memories(&ids).await
    }
```

- [ ] **Step 9: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory`
Expected: 全綠。

- [ ] **Step 10: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/prune.rs crates/wukong-memory/src/store/mod.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): prune consolidated and low-value memories"
```

---

## Task 6: Snapshot 資料模型

**Files:**
- Modify: `crates/wukong-memory/src/model.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: 加結構**

在 `crates/wukong-memory/src/model.rs` 檔尾(`Stats` 之後、`mod tests` 之前)加:

```rust
/// Count of memories of one kind.
#[derive(Debug, Clone, Serialize)]
pub struct KindCount {
    pub kind: MemoryKind,
    pub count: i64,
}

/// Memory counts bucketed by age relative to "now".
#[derive(Debug, Clone, Serialize)]
pub struct AgeBuckets {
    pub last_day: i64,
    pub last_week: i64,
    pub last_month: i64,
    pub older: i64,
}

/// How many memories carry an embedding.
#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingCoverage {
    pub embedded: i64,
    pub total: i64,
}

/// Rich health snapshot for observability.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub total: i64,
    pub by_scope: Vec<ScopeCount>,
    pub by_kind: Vec<KindCount>,
    pub age: AgeBuckets,
    pub embedding: EmbeddingCoverage,
    pub consolidation_candidates: i64,
    pub prune_candidates: i64,
}
```

- [ ] **Step 2: 匯出**

在 `crates/wukong-memory/src/lib.rs` 的 `pub use model::{...}` 清單加入 `AgeBuckets, EmbeddingCoverage, KindCount, Snapshot`(維持字母序整理即可)。

- [ ] **Step 3: 編譯確認**

Run: `. "$HOME/.cargo/env" && cargo build -p wukong-memory`
Expected: 成功編譯。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/model.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): add Snapshot data model"
```

---

## Task 7: `Store::snapshot` + `Memory::snapshot`

**Files:**
- Modify: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: 寫 store 失敗測試**

在 `crates/wukong-memory/src/store/mod.rs` 的 `mod tests` 內加:

```rust
    #[tokio::test]
    async fn snapshot_reports_counts() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let now = 1_000_000_000i64;
        let old = now - 40 * 86_400;
        let e1 = store.insert_memory(None, "project:X", MemoryKind::Event, "e1", 1.0, now).await.unwrap();
        let _n1 = store.insert_memory(None, "project:X", MemoryKind::Note, "n1", 0.2, old).await.unwrap();
        let _d1 = store.insert_memory(None, "project:X", MemoryKind::Decision, "d1", 1.0, now).await.unwrap();
        store.update_embedding(e1, &embedding_to_blob(&[0.1f32]), "mock").await.unwrap();

        let snap = store.snapshot(None, now, 30 * 86_400, 0.5).await.unwrap();
        assert_eq!(snap.total, 3);
        // by_kind has event/note/decision.
        assert_eq!(snap.by_kind.iter().map(|k| k.count).sum::<i64>(), 3);
        // age: e1 and d1 are "today", n1 is "older".
        assert_eq!(snap.age.last_day, 2);
        assert_eq!(snap.age.older, 1);
        // 1 of 3 embedded.
        assert_eq!(snap.embedding.embedded, 1);
        assert_eq!(snap.embedding.total, 3);
        // consolidation candidates: e1 + n1 (event/note, unconsolidated) = 2.
        assert_eq!(snap.consolidation_candidates, 2);
        // prune candidates: old low-value n1 = 1.
        assert_eq!(snap.prune_candidates, 1);
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory snapshot_reports_counts`
Expected: FAIL(`snapshot` 未定義)。

- [ ] **Step 3: 實作 `Store::snapshot`**

先在 `crates/wukong-memory/src/store/mod.rs` 檔頭 `use crate::model::{...}` 加入 `AgeBuckets, EmbeddingCoverage, KindCount, Snapshot`。在 `impl Store` 內加:

```rust
    /// Compose a full health snapshot. `scope` filters by-kind/age/coverage and
    /// candidate counts to one scope; by_scope/total are always global.
    pub async fn snapshot(
        &self,
        scope: Option<&str>,
        now: i64,
        max_age_secs: i64,
        importance_floor: f64,
    ) -> Result<Snapshot> {
        let base = self.stats().await?; // total + by_scope (global)

        // by_kind (optionally scoped)
        let mut kind_sql = String::from("SELECT kind, COUNT(*) AS c FROM memories");
        if scope.is_some() {
            kind_sql.push_str(" WHERE scope = ?1");
        }
        kind_sql.push_str(" GROUP BY kind ORDER BY c DESC");
        let mut kq = sqlx::query(&kind_sql);
        if let Some(s) = scope {
            kq = kq.bind(s);
        }
        let by_kind = kq
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|r| KindCount {
                kind: MemoryKind::from_db_str(&r.get::<String, _>("kind")),
                count: r.get::<i64, _>("c"),
            })
            .collect();

        // age buckets
        let day = now - 86_400;
        let week = now - 7 * 86_400;
        let month = now - 30 * 86_400;
        let mut age_sql = String::from(
            "SELECT
                 SUM(CASE WHEN created_at >= ?1 THEN 1 ELSE 0 END) AS d,
                 SUM(CASE WHEN created_at >= ?2 AND created_at < ?1 THEN 1 ELSE 0 END) AS w,
                 SUM(CASE WHEN created_at >= ?3 AND created_at < ?2 THEN 1 ELSE 0 END) AS m,
                 SUM(CASE WHEN created_at < ?3 THEN 1 ELSE 0 END) AS o
             FROM memories",
        );
        if scope.is_some() {
            age_sql.push_str(" WHERE scope = ?4");
        }
        let mut aq = sqlx::query(&age_sql).bind(day).bind(week).bind(month);
        if let Some(s) = scope {
            aq = aq.bind(s);
        }
        let ar = aq.fetch_one(&self.pool).await?;
        let age = AgeBuckets {
            last_day: ar.get::<Option<i64>, _>("d").unwrap_or(0),
            last_week: ar.get::<Option<i64>, _>("w").unwrap_or(0),
            last_month: ar.get::<Option<i64>, _>("m").unwrap_or(0),
            older: ar.get::<Option<i64>, _>("o").unwrap_or(0),
        };

        // embedding coverage
        let mut cov_sql = String::from(
            "SELECT COUNT(*) AS total, SUM(CASE WHEN embedding IS NOT NULL THEN 1 ELSE 0 END) AS emb
             FROM memories",
        );
        if scope.is_some() {
            cov_sql.push_str(" WHERE scope = ?1");
        }
        let mut cq = sqlx::query(&cov_sql);
        if let Some(s) = scope {
            cq = cq.bind(s);
        }
        let cr = cq.fetch_one(&self.pool).await?;
        let embedding = EmbeddingCoverage {
            embedded: cr.get::<Option<i64>, _>("emb").unwrap_or(0),
            total: cr.get::<i64, _>("total"),
        };

        // candidate counts
        let mut cons_sql = String::from(
            "SELECT COUNT(*) AS c FROM memories
             WHERE kind IN ('event','note') AND consolidated_into IS NULL",
        );
        if scope.is_some() {
            cons_sql.push_str(" AND scope = ?1");
        }
        let mut consq = sqlx::query(&cons_sql);
        if let Some(s) = scope {
            consq = consq.bind(s);
        }
        let consolidation_candidates = consq.fetch_one(&self.pool).await?.get::<i64, _>("c");

        let prune_candidates =
            self.prune_candidates(scope, max_age_secs, importance_floor, now).await?.len() as i64;

        Ok(Snapshot {
            total: base.total,
            by_scope: base.by_scope,
            by_kind,
            age,
            embedding,
            consolidation_candidates,
            prune_candidates,
        })
    }
```

- [ ] **Step 4: 實作 `Memory::snapshot`**

在 `crates/wukong-memory/src/lib.rs` 的 `impl Memory` 內加:

```rust
    /// Compose a health snapshot using default prune thresholds.
    pub async fn snapshot(&self, scope: Option<&str>) -> Result<model::Snapshot> {
        let p = prune::PrunePolicy::default();
        self.store
            .snapshot(scope, now_unix(), p.max_age_secs, p.importance_floor)
            .await
    }
```

- [ ] **Step 5: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory`
Expected: 全綠。

- [ ] **Step 6: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/store/mod.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): compose health snapshot query"
```

---

## Task 8: memoryd `GET /v1/snapshot`

**Files:**
- Modify: `crates/wukong-memoryd/src/lib.rs`
- Modify: `crates/wukong-memoryd/tests/http.rs`

- [ ] **Step 1: 看現有 http 測試慣例**

Read: `crates/wukong-memoryd/tests/http.rs`(沿用其建構 router / 發請求的既有風格)。

- [ ] **Step 2: 寫失敗測試**

在 `crates/wukong-memoryd/tests/http.rs` 仿照既有 `/v1/stats` 測試新增一個 `snapshot_endpoint_returns_json` 測試:對 `GET /v1/snapshot` 發請求,斷言狀態 200 且回傳 JSON 內含 `"total"` 與 `"by_kind"` 鍵。(沿用該檔既有的 helper 與 import;若既有測試用 `tower::ServiceExt::oneshot`,照用。)

```rust
#[tokio::test]
async fn snapshot_endpoint_returns_json() {
    let app = test_app().await; // 沿用檔案內既有 helper 名稱;若不同則對應替換
    let res = app
        .oneshot(
            axum::http::Request::builder()
                .uri("/v1/snapshot")
                .body(axum::body::Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(res.status(), axum::http::StatusCode::OK);
    let bytes = axum::body::to_bytes(res.into_body(), usize::MAX).await.unwrap();
    let v: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert!(v.get("total").is_some());
    assert!(v.get("by_kind").is_some());
}
```

注意:若 `tests/http.rs` 沒有可重用的 `test_app()` helper,複製既有測試開頭建構 `build_router(Arc::new(memory))` 的那幾行到本測試。

- [ ] **Step 3: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memoryd snapshot_endpoint_returns_json`
Expected: FAIL(404,路由不存在)。

- [ ] **Step 4: 加 handler 與路由**

在 `crates/wukong-memoryd/src/lib.rs` 的 `stats` handler 之後加:

```rust
async fn snapshot(State(mem): State<Arc<Memory>>) -> Result<impl IntoResponse, AppError> {
    Ok(Json(mem.snapshot(None).await?))
}
```

在 `build_router` 內 `.route("/v1/stats", get(stats))` 之後加:

```rust
        .route("/v1/snapshot", get(snapshot))
```

- [ ] **Step 5: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memoryd`
Expected: 全綠。

- [ ] **Step 6: commit**

```bash
set -o pipefail
git add crates/wukong-memoryd/src/lib.rs crates/wukong-memoryd/tests/http.rs
git commit -m "feat(memoryd): add GET /v1/snapshot endpoint"
```

---

## Task 9: Markdown 純函式與 `MarkdownSink`

**Files:**
- Create: `crates/wukong-memory/src/markdown.rs`
- Modify: `crates/wukong-memory/src/error.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: error 加 `Io` 變體**

Read: `crates/wukong-memory/src/error.rs`。在 `MemoryError` enum 內加(沿用既有 `thiserror` 風格):

```rust
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
```

- [ ] **Step 2: 寫純函式失敗測試**

建立 `crates/wukong-memory/src/markdown.rs`:

```rust
//! Markdown mirror of memory. DB is the source of truth; markdown is a
//! one-way, human-readable, git-friendly derived view. Opt-in via a directory.

use crate::error::Result;
use crate::model::MemoryKind;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Map a scope string to a safe filename, e.g. "project:Wukong" ->
/// "project_Wukong.md". Replaces ':' and '/' with '_'.
pub fn scope_to_filename(scope: &str) -> String {
    let safe: String = scope
        .chars()
        .map(|c| if c == ':' || c == '/' || c == '\\' { '_' } else { c })
        .collect();
    format!("{safe}.md")
}

/// Render one memory as a markdown block. `created_at` is unix seconds.
pub fn render_markdown_entry(created_at: i64, kind: MemoryKind, text: &str) -> String {
    format!("## {} · {}\n{}\n\n", iso8601(created_at), kind.as_str(), text)
}

/// Minimal UTC ISO-8601 formatter (no external date dep).
fn iso8601(unix_secs: i64) -> String {
    // days since epoch / time-of-day via civil-from-days algorithm.
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    let (h, mi, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Appends memory entries to per-scope markdown files under `dir`.
#[derive(Debug, Clone)]
pub struct MarkdownSink {
    dir: PathBuf,
}

impl MarkdownSink {
    /// Create a sink writing under `dir` (created on first append).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Append one entry to the scope's file. Creates dir/file as needed.
    pub fn append(&self, scope: &str, created_at: i64, kind: MemoryKind, text: &str) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(scope_to_filename(scope));
        let mut f = OpenOptions::new().create(true).append(true).open(path)?;
        f.write_all(render_markdown_entry(created_at, kind, text).as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scope_to_filename_sanitizes() {
        assert_eq!(scope_to_filename("project:Wukong"), "project_Wukong.md");
        assert_eq!(scope_to_filename("global"), "global.md");
    }

    #[test]
    fn render_entry_has_kind_and_text() {
        let s = render_markdown_entry(0, MemoryKind::Event, "hello");
        assert!(s.contains("1970-01-01T00:00:00Z"));
        assert!(s.contains("· event"));
        assert!(s.contains("hello"));
    }

    #[test]
    fn sink_appends_to_scope_file() {
        let dir = tempdir().unwrap();
        let sink = MarkdownSink::new(dir.path());
        sink.append("project:X", 100, MemoryKind::Note, "first").unwrap();
        sink.append("project:X", 200, MemoryKind::Note, "second").unwrap();
        let body = std::fs::read_to_string(dir.path().join("project_X.md")).unwrap();
        assert!(body.contains("first"));
        assert!(body.contains("second"));
        // Two entries => two headers.
        assert_eq!(body.matches("## ").count(), 2);
    }
}
```

在 `crates/wukong-memory/src/lib.rs` 模組區加 `pub mod markdown;`,`pub use` 區加 `pub use markdown::MarkdownSink;`。

- [ ] **Step 3: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory markdown`
Expected: PASS(3 測試)。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/markdown.rs crates/wukong-memory/src/error.rs crates/wukong-memory/src/lib.rs
git commit -m "feat(memory): markdown rendering and per-scope sink"
```

---

## Task 10: 把 markdown 接進 `Memory`(remember 雙寫 + export)

**Files:**
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: 寫雙寫失敗測試**

在 `crates/wukong-memory/src/lib.rs` 的 `mod tests` 內加:

```rust
    #[tokio::test]
    async fn remember_mirrors_to_markdown_when_sink_set() {
        let dir = tempfile::tempdir().unwrap();
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        let mem = Memory::open(&url).await.unwrap().with_markdown(dir.path());

        remember_event(&mem, "project:X", "mirrored note").await;

        let body = std::fs::read_to_string(dir.path().join("project_X.md")).unwrap();
        assert!(body.contains("mirrored note"));
    }

    #[tokio::test]
    async fn export_rebuilds_all_scope_files() {
        let dir = tempfile::tempdir().unwrap();
        let mem = open_mem().await; // no sink attached
        remember_event(&mem, "project:X", "x-note").await;
        remember_event(&mem, "global", "g-note").await;

        mem.export(dir.path()).await.unwrap();

        assert!(std::fs::read_to_string(dir.path().join("project_X.md")).unwrap().contains("x-note"));
        assert!(std::fs::read_to_string(dir.path().join("global.md")).unwrap().contains("g-note"));
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory remember_mirrors_to_markdown export_rebuilds`
Expected: FAIL(`with_markdown`、`export`、`md_sink` 未定義)。

- [ ] **Step 3: 加欄位與 builder**

在 `crates/wukong-memory/src/lib.rs` 的 `use` 區加 `use markdown::MarkdownSink;`。`Memory` struct 加欄位:

```rust
    md_sink: Option<MarkdownSink>,
```

在 `Memory::open` 的回傳 struct 初始化加 `md_sink: None,`。`with_embedder` 內重建 struct 時若有逐欄列出也要帶上 `md_sink`(實際 `with_embedder` 是 `mut self` 改欄位,不需動)。新增 builder(放在 `with_embedder` 之後):

```rust
    /// Attach a markdown mirror directory. Every `remember` then also appends
    /// to the per-scope markdown file (best-effort).
    pub fn with_markdown(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.md_sink = Some(MarkdownSink::new(dir));
        self
    }
```

- [ ] **Step 4: remember 雙寫**

在 `remember` 的 for-item 迴圈內、`ids.push(id);` 與 embedder 區塊之後加(DB 落盤成功後才寫 md,失敗只 warn):

```rust
            if let Some(sink) = &self.md_sink {
                if let Err(e) = sink.append(&scope_str, now, item.kind, &item.text) {
                    eprintln!("wukong-memory: markdown append failed: {e}");
                }
            }
```

- [ ] **Step 5: 把 Task 4 略過的 consolidate md 段補回**

在 `consolidate` 方法內、`mark_consolidated` 之後(embed 區塊附近)補回 Task 4 Step 8 註明略過的:

```rust
            if let Some(sink) = &self.md_sink {
                if let Err(e) = sink.append(scope, now, MemoryKind::Summary, &summary_text) {
                    eprintln!("wukong-memory: markdown append failed: {e}");
                }
            }
```

- [ ] **Step 6: 實作 `export`**

需要一個讀全部記憶的 store 方法。先在 `crates/wukong-memory/src/store/mod.rs` 的 `impl Store` 加:

```rust
    /// All memories ordered oldest-first, for full markdown export.
    /// Returns (scope, created_at, kind, text).
    pub async fn all_for_export(&self) -> Result<Vec<(String, i64, MemoryKind, String)>> {
        let rows = sqlx::query(
            "SELECT scope, created_at, kind, text FROM memories ORDER BY created_at ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<String, _>("scope"),
                    r.get::<i64, _>("created_at"),
                    MemoryKind::from_db_str(&r.get::<String, _>("kind")),
                    r.get::<String, _>("text"),
                )
            })
            .collect())
    }
```

在 `impl Memory` 加 `export`:

```rust
    /// Rebuild markdown for every scope from the DB (full overwrite). Ignores
    /// any attached live sink; writes a fresh mirror under `dir`.
    pub async fn export(&self, dir: impl Into<std::path::PathBuf>) -> Result<()> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        // Truncate existing scope files first so export is a clean rebuild.
        let rows = self.store.all_for_export().await?;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let sink = markdown::MarkdownSink::new(&dir);
        for (scope, created_at, kind, text) in rows {
            if seen.insert(scope.clone()) {
                // First time this scope appears in the rebuild: truncate its file.
                let path = dir.join(markdown::scope_to_filename(&scope));
                let _ = std::fs::write(&path, "");
            }
            sink.append(&scope, created_at, kind, &text)?;
        }
        Ok(())
    }
```

並把 `scope_to_filename` 加入 `crates/wukong-memory/src/markdown.rs` 的對外可見(已是 `pub fn`,於 lib `pub use markdown::{MarkdownSink, scope_to_filename};` 補上;或如上以 `markdown::scope_to_filename` 完整路徑引用,免改 `pub use`)。

- [ ] **Step 7: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-memory`
Expected: 全綠。

- [ ] **Step 8: commit**

```bash
set -o pipefail
git add crates/wukong-memory/src/lib.rs crates/wukong-memory/src/store/mod.rs
git commit -m "feat(memory): mirror remember to markdown and add export"
```

---

## Task 11: `OpencodeSummarizer`(gateway)

**Files:**
- Create: `crates/wukong-gateway/src/summarize.rs`
- Modify: `crates/wukong-gateway/src/lib.rs`

- [ ] **Step 1: 看 backend trait 形狀**

Read: `crates/wukong-gateway/src/backend.rs:1-60`(確認 `AgentRequest { prompt, continue_session }`、`AgentResponse { text }`、`async fn run`)。

- [ ] **Step 2: 寫測試**

建立 `crates/wukong-gateway/src/summarize.rs`:

```rust
//! Opencode-backed Summarizer: bridges the async AiBackend to the synchronous
//! `wukong_memory::Summarizer` trait. Runs on the current tokio runtime via
//! block_in_place (requires the multi-thread runtime, which #[tokio::main] uses
//! by default).

use crate::backend::{AgentRequest, AiBackend};
use wukong_memory::error::Result as MemResult;
use wukong_memory::{MemoryError, Summarizer};

/// Wraps an AiBackend so the memory layer can request summaries.
pub struct OpencodeSummarizer<'a, B: AiBackend> {
    pub backend: &'a B,
    pub handle: tokio::runtime::Handle,
}

impl<'a, B: AiBackend> OpencodeSummarizer<'a, B> {
    pub fn new(backend: &'a B) -> Self {
        Self { backend, handle: tokio::runtime::Handle::current() }
    }
}

impl<B: AiBackend> Summarizer for OpencodeSummarizer<'_, B> {
    fn summarize(&self, texts: &[String]) -> MemResult<String> {
        let prompt = format!(
            "請把以下記憶濃縮成一段精簡摘要,保留關鍵決策與事實,只輸出摘要本身:\n\n{}",
            texts.join("\n")
        );
        let fut = self.backend.run(AgentRequest { prompt, continue_session: false });
        let resp = tokio::task::block_in_place(|| self.handle.block_on(fut))
            .map_err(|e| MemoryError::Other(format!("summarizer backend failed: {e}")))?;
        Ok(resp.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AgentResponse;
    use crate::error::GatewayError;

    struct Echo;
    impl AiBackend for Echo {
        async fn run(&self, req: AgentRequest) -> std::result::Result<AgentResponse, GatewayError> {
            Ok(AgentResponse { text: format!("SUM[{}]", req.prompt.len()) })
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn summarizer_calls_backend() {
        let b = Echo;
        let s = OpencodeSummarizer::new(&b);
        let out = s.summarize(&["a".to_string(), "b".to_string()]).unwrap();
        assert!(out.starts_with("SUM["));
    }
}
```

- [ ] **Step 3: 掛模組**

在 `crates/wukong-gateway/src/lib.rs` 加 `pub mod summarize;`。

- [ ] **Step 4: 確認 `MemoryError::Other` 存在**

Read: `crates/wukong-memory/src/error.rs`。若沒有可裝任意字串的變體,新增:

```rust
    #[error("{0}")]
    Other(String),
```

並確認 `wukong_memory::error` 與 `MemoryError` 為對外可見(`error.rs` 已 `pub use`,`lib.rs` 已 `pub use error::{MemoryError, Result}`)。`wukong_memory::error::Result` 路徑需可用 —— `error` 模組是 `pub mod error;`(在 lib.rs 已是),OK。

- [ ] **Step 5: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-gateway summarizer_calls_backend`
Expected: PASS。

- [ ] **Step 6: commit**

```bash
set -o pipefail
git add crates/wukong-gateway/src/summarize.rs crates/wukong-gateway/src/lib.rs crates/wukong-memory/src/error.rs
git commit -m "feat(gateway): opencode-backed Summarizer bridge"
```

---

## Task 12: CLI `memory` 子命令

**Files:**
- Modify: `crates/wukong-gateway/src/cli.rs`
- Modify: `crates/wukong-cli/src/main.rs`

- [ ] **Step 1: 寫 cli 解析失敗測試**

在 `crates/wukong-gateway/src/cli.rs` 的 `mod tests` 內加:

```rust
    #[test]
    fn parses_memory_snapshot_subcommand() {
        let cli = Cli::try_parse_from(["wukong", "memory", "snapshot"]).unwrap();
        match cli.command {
            Some(Command::Memory { op: MemoryOp::Snapshot { scope } }) => assert!(scope.is_none()),
            _ => panic!("expected memory snapshot"),
        }
    }

    #[test]
    fn parses_memory_prune_dry_run() {
        let cli = Cli::try_parse_from(["wukong", "memory", "prune", "--dry-run"]).unwrap();
        match cli.command {
            Some(Command::Memory { op: MemoryOp::Prune { dry_run, .. } }) => assert!(dry_run),
            _ => panic!("expected memory prune"),
        }
    }

    #[test]
    fn bare_prompt_has_no_subcommand() {
        let cli = Cli::try_parse_from(["wukong", "hello", "world"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.prompt_text(), "hello world");
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-gateway parses_memory`
Expected: FAIL(`Command`、`MemoryOp` 未定義)。

- [ ] **Step 3: 加子命令到 cli.rs**

在 `crates/wukong-gateway/src/cli.rs` 頂部 `use clap::Parser;` 改為:

```rust
use clap::{Parser, Subcommand};
```

`Cli` struct 上方屬性與欄位調整 —— 在 `#[command(...)]` 加上旗標,並新增 `command` 欄:

```rust
#[derive(Parser, Debug)]
#[command(
    name = "wukong",
    about = "Wukong assistant gateway (CLI)",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
pub struct Cli {
    /// Memory maintenance subcommands. Absent => chat / REPL.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// The prompt to send to the assistant (joined with spaces). Empty => REPL.
    #[arg(num_args = 0..)]
    pub prompt: Vec<String>,

    // ... 其餘既有欄位(continue_session / scope / db / agent_cmd / no_stream)維持不變 ...
}
```

在 `impl Cli { ... }` 之後加:

```rust
/// Top-level subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Memory maintenance operations.
    Memory {
        #[command(subcommand)]
        op: MemoryOp,
    },
}

/// `wukong memory <op>`.
#[derive(Subcommand, Debug)]
pub enum MemoryOp {
    /// Print a health snapshot.
    Snapshot {
        #[arg(long)]
        scope: Option<String>,
    },
    /// Fold scattered events into summaries.
    Consolidate {
        #[arg(long)]
        scope: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Delete consolidated / low-value memories.
    Prune {
        #[arg(long)]
        scope: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Rebuild markdown mirror from the DB.
    Export {
        #[arg(long)]
        dir: Option<String>,
    },
}
```

- [ ] **Step 4: 跑 cli 測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-gateway`
Expected: 全綠(注意:既有 `no_prompt_is_allowed_for_repl` 等測試仍須通過;`Option<Command>` 不影響裸 prompt 解析)。

- [ ] **Step 5: main.rs 分派子命令**

在 `crates/wukong-cli/src/main.rs`:

(a) import 加:

```rust
use wukong_gateway::cli::{Command, MemoryOp};
use wukong_gateway::summarize::OpencodeSummarizer;
use wukong_memory::{ConsolidatePolicy, PrunePolicy};
```

(b) 在 `let backend = AgentCliBackend { ... };` 之後、`let prompt = cli.prompt_text();` 之前插入分派區塊:

```rust
    if let Some(Command::Memory { op }) = &cli.command {
        if let Err(e) = run_memory_op(&memory, &backend, &cfg, op).await {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }
```

(c) 在 `run_one` 之後新增 `run_memory_op`:

```rust
async fn run_memory_op(
    memory: &Memory,
    backend: &AgentCliBackend,
    cfg: &GatewayConfig,
    op: &MemoryOp,
) -> Result<(), wukong_cli::WukongError> {
    match op {
        MemoryOp::Snapshot { scope } => {
            let snap = memory.snapshot(scope.as_deref()).await?;
            println!("總計: {}", snap.total);
            println!("依範圍:");
            for s in &snap.by_scope {
                println!("  {} = {}", s.scope, s.count);
            }
            println!("依類型:");
            for k in &snap.by_kind {
                println!("  {} = {}", k.kind.as_str(), k.count);
            }
            println!(
                "年齡: <1d={} <7d={} <30d={} older={}",
                snap.age.last_day, snap.age.last_week, snap.age.last_month, snap.age.older
            );
            println!("embedding 覆蓋: {}/{}", snap.embedding.embedded, snap.embedding.total);
            println!("consolidation 候選: {}", snap.consolidation_candidates);
            println!("prune 候選: {}", snap.prune_candidates);
        }
        MemoryOp::Consolidate { scope, dry_run } => {
            let scope = scope.clone().unwrap_or_else(|| cfg.scope.clone());
            let policy = ConsolidatePolicy::default();
            if *dry_run {
                let plan = memory.plan_consolidation(&scope, &policy).await?;
                println!("[dry-run] 將產生 {} 筆摘要:", plan.batches.len());
                for (i, b) in plan.batches.iter().enumerate() {
                    println!("  批 {}: {} 筆來源 {:?}", i + 1, b.len(), b);
                }
            } else {
                let summarizer = OpencodeSummarizer::new(backend);
                let ids = memory.consolidate(&scope, &policy, &summarizer).await?;
                println!("已建立 {} 筆摘要: {:?}", ids.len(), ids);
            }
        }
        MemoryOp::Prune { scope, dry_run } => {
            let policy = PrunePolicy::default();
            if *dry_run {
                let ids = memory.plan_prune(scope.as_deref(), &policy).await?;
                println!("[dry-run] 將刪除 {} 筆: {:?}", ids.len(), ids);
            } else {
                let n = memory.prune(scope.as_deref(), &policy).await?;
                println!("已刪除 {n} 筆");
            }
        }
        MemoryOp::Export { dir } => {
            let dir = dir
                .clone()
                .or_else(|| std::env::var("WUKONG_MD_DIR").ok())
                .ok_or_else(|| {
                    wukong_cli::WukongError::from(wukong_memory::MemoryError::Other(
                        "未指定輸出目錄,請用 --dir 或設 WUKONG_MD_DIR".to_string(),
                    ))
                })?;
            memory.export(&dir).await?;
            println!("已匯出 markdown 至 {dir}");
        }
    }
    Ok(())
}
```

注意:`wukong_cli::WukongError` 需能從 `MemoryError` 轉換(`?` 已在既有 `run_turn` 路徑使用,故 `From<MemoryError>` 應已存在;若 `WukongError::from` 不可用則改用既有的轉換方式)。確認方式見 Step 7。

(d) 在 `main` 建 `memory` 之後加上 markdown 注入(於 embedder 注入區塊之後):

```rust
    let memory = match std::env::var("WUKONG_MD_DIR") {
        Ok(dir) if !dir.is_empty() => memory.with_markdown(dir),
        _ => memory,
    };
```

- [ ] **Step 6: 確認 `WukongError: From<MemoryError>`**

Read: `crates/wukong-cli/src/lib.rs`(找 `WukongError` 定義)。若無 `#[from] MemoryError`,在其 enum 加:

```rust
    #[error(transparent)]
    Memory(#[from] wukong_memory::MemoryError),
```

(若已有等價變體則跳過。)

- [ ] **Step 7: 編譯並跑全 workspace 測試**

Run: `. "$HOME/.cargo/env" && cargo build && cargo test`
Expected: 編譯成功、全 workspace 測試綠。

- [ ] **Step 8: 真實 opencode 煙霧測試(手動)**

```bash
. "$HOME/.cargo/env"
TMP=$(mktemp -d)
export WUKONG_MEMORY_DB="sqlite://$TMP/m.db"
export WUKONG_MD_DIR="$TMP/md"
# 塞幾筆記憶
cargo run -q -p wukong-cli -- --scope project:Demo "記住:我們選用 sqlite" >/dev/null
cargo run -q -p wukong-cli -- --scope project:Demo "記住:embedding 用 fastembed" >/dev/null
# 快照
cargo run -q -p wukong-cli -- memory snapshot --scope project:Demo
# 乾跑 consolidation
cargo run -q -p wukong-cli -- memory consolidate --scope project:Demo --dry-run
# 真做(走 opencode 摘要)
cargo run -q -p wukong-cli -- memory consolidate --scope project:Demo
# 乾跑 prune
cargo run -q -p wukong-cli -- memory prune --scope project:Demo --dry-run
ls "$TMP/md"
cat "$TMP/md/project_Demo.md"
```
Expected:snapshot 印出計數;dry-run 列批次;consolidate 後印出摘要 id;markdown 檔含記憶與摘要。觀察 opencode 是否成功回摘要(失敗則檢查 `WUKONG_AGENT_CMD`)。

- [ ] **Step 9: commit**

```bash
set -o pipefail
git add crates/wukong-gateway/src/cli.rs crates/wukong-cli/src/main.rs crates/wukong-cli/src/lib.rs
git commit -m "feat(cli): add memory maintenance subcommands"
```

---

## Task 13: clippy 與文件

**Files:**
- Modify: `README.md`
- Modify: `crates/wukong-memory/README.md`
- Modify: `crates/wukong-cli/README.md`
- Modify: `crates/wukong-memoryd/README.md`

- [ ] **Step 1: clippy 全綠(含 embed feature)**

Run: `. "$HOME/.cargo/env" && cargo clippy --all-targets -- -D warnings && cargo clippy -p wukong-memory --features embed -- -D warnings`
Expected:零警告。若有,逐一修正後再跑。

- [ ] **Step 2: 更新文件**

- `README.md`:roadmap 標記 D 完成;usage 區加 `wukong memory snapshot|consolidate|prune|export` 與 `WUKONG_MD_DIR` 說明。
- `crates/wukong-memory/README.md`:新增「記憶維護」段(Summarizer trait、prune 謂詞、markdown 鏡像、snapshot)。
- `crates/wukong-cli/README.md`:`memory` 子命令表格(含 `--dry-run`、`--scope`、`--dir`)。
- `crates/wukong-memoryd/README.md`:新增 `GET /v1/snapshot`。

- [ ] **Step 3: commit**

```bash
set -o pipefail
git add README.md crates/wukong-memory/README.md crates/wukong-cli/README.md crates/wukong-memoryd/README.md
git commit -m "docs: document memory maintenance commands and snapshot endpoint"
```

---

## 完成後

依 `superpowers:finishing-a-development-branch`:跑全測試 → 呈現 4 選項(合併/PR/保留/丟棄)。合併後比照 v0.2/v0.3 慣例詢問是否開 **v0.4.0 release**。

## 自我複查紀錄

- **Spec 覆蓋:** Consolidation(T2-4)、Prune(T5)、Markdown 雙寫+export(T9-10)、Snapshot(T6-8)、schema(T1)、CLI(T12)、Summarizer 注入(T11)、memoryd 端點(T8)。四子系統皆有對應 task。
- **型別一致:** `ConsolidationRow`(T3 定義,T4 使用)、`ConsolidatePolicy/Plan`(T4)、`PrunePolicy`(T5)、`Snapshot` 家族(T6 定義,T7/T8/T12 使用)、`MarkdownSink`(T9 定義,T10 使用)、`OpencodeSummarizer`(T11)、`Command/MemoryOp`(T12)。方法名 `plan_consolidation/consolidate/plan_prune/prune/snapshot/export/with_markdown` 全程一致。
- **跨任務前向引用已標註:** T2 `pub use` 暫不含 T4 型別;T4 補回。T4 consolidate 暫略 `md_sink` 段;T10 補回。
