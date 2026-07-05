# wukong-memory

> 柱 1 ──「鬥戰勝佛・本我」：持久記憶核心

跨對話、可追溯、完全離線的本機記憶。SQLite + FTS5 詞彙式召回，零外部模型。

## 公開 API（lib）

```rust
use wukong_memory::{Memory, RememberInput, MemoryItem, MemoryKind, RecallQuery, RecallMode};

let mem = Memory::open("sqlite://./memory.db").await?;

mem.remember(RememberInput {
    scope: "project:Wukong".into(),
    session_id: None,
    items: vec![MemoryItem {
        kind: MemoryKind::Decision,
        text: "用 Rust 重寫".into(),
        importance: None,
        dedupe_key: None,
    }],
}).await?;

let hits = mem.recall(RecallQuery {
    query: "Rust 決策".into(),
    top_k: 5,
    scope: Some("project:Wukong".into()),
    mode: RecallMode::Hybrid,
}).await?;

let stats = mem.stats().await?;
```

## 召回（recall）

| mode | 作法 |
| :--- | :--- |
| `Keyword` | FTS5 `MATCH` + BM25 |
| `Tree` | 依 scope 階層取近期記憶 |
| `Hybrid`（預設） | 合併詞彙＋近期＋語意，依綜合分重排 |

綜合分：`α·正規化BM25 + δ·語意相似 + β·時間衰減(90天半衰期) + γ·importance`，常被召回者加成。
過短／全停用詞的查詢由 adaptive gate 直接略過；CJK 查詢使用字元權重，短中文關鍵詞可召回，低資訊量回覆會跳過。
回應 `confidence` 取 top hit 的 decay-free `relevance = max(lexical, semantic)`；排序仍使用完整綜合分。
`recall_telemetry_summary()` 可讀取聚合 telemetry。Telemetry 只儲存 query hash，不保存原始查詢文字。

## 語意向量召回（選用增強層）

預設純 BM25、零外部模型。開啟 cargo feature `embed` 後可加入本機 embedding：

- 模型：fastembed `all-MiniLM-L6-v2`（384 維），首次使用下載至 cache、之後離線。
- 儲存：向量以 BLOB 存在 `memories.embedding`，與 SQLite 同庫；純 Rust 暴力 cosine。
- 排序：語意分以 `δ` 權重併入 Hybrid 綜合分（`δ` 缺席時為 0）。
- 啟用：建置帶 `--features embed` 並設環境變數 `WUKONG_EMBED=1`。
- 退場：未編 feature／未設環境變數／模型載入失敗 → 一律退回 v1 BM25，助手照常可用。
- 既有記憶：開機背景批次補齊向量；補齊前仍可由 BM25 召回。

```rust
use std::sync::Arc;
use wukong_memory::{Memory, MockEmbedder};

// 測試用確定性 embedder（真實用 FastembedBackend，需 feature `embed`）
let mem = Memory::open("sqlite://./memory.db").await?
    .with_embedder(Arc::new(MockEmbedder::new(384)));
mem.backfill_embeddings().await?; // 直接補齊（with_embedder 也會背景補齊）
```

## 記憶維護（手動）

記憶量變大後保持精簡、可觀測、可人工檢視。皆由上層手動觸發（不自動排程）。

- **Consolidation**：`Summarizer` trait（比照 `Embedder` 注入模式）把零碎 `event`/`note` 聚合成 `Summary`。預設 `ConcatSummarizer`（機械串接、零依賴）；cli/gateway 層注入 `OpencodeSummarizer` 做真摘要。同 `session_id` 一批，其餘依時間每 `batch_size`（預設 20）一批；來源列標記 `consolidated_into`。`plan_consolidation` 提供 dry-run。
- **Prune**：刪除「已被摘要」(`consolidated_into IS NOT NULL`) 或「老舊 + `recall_count=0` + 低重要度」的 `event`/`note`；`Decision`/`Skill`/`Summary` 永不刪。`PrunePolicy` 預設 30 天 / importance < 0.5。`plan_prune` 提供 dry-run。刪除經 `AFTER DELETE` 觸發器同步 FTS5 索引。
- **Markdown 雙持久化**：`Memory::with_markdown(dir)` 開啟後，每次 `remember` 把記憶 append 到 per-scope markdown（`project:X` → `project_X.md`），best-effort（寫檔失敗只 warn，不阻斷 remember）。`export(dir)` 依 DB 全量重建。**DB 為唯一真相來源，markdown 單向衍生。**
- **Snapshot**：`snapshot(scope)` 回 `Snapshot { total, by_scope, by_kind, age, embedding, consolidation_candidates, prune_candidates }`。

```rust
use wukong_memory::{ConsolidatePolicy, ConcatSummarizer, PrunePolicy};

let summary_ids = mem.consolidate("project:X", &ConsolidatePolicy::default(), &ConcatSummarizer).await?;
let removed = mem.prune(Some("project:X"), &PrunePolicy::default()).await?;
let snap = mem.snapshot(Some("project:X")).await?;
```

## Scope

`Global` / `Project(name)` / `Agent(name)` / `User(name)`，序列化為 `global`、`project:X` 等。
召回具體 scope 時自動含 `global`（階層 fallback）。

## 儲存

SQLite（WAL）+ FTS5 外部內容表。schema 啟動時冪等套用。回應信封：`WukongResult<T> { data, evidence[], confidence, latency_ms }`。
`MemoryItem::dedupe_key` 可由系統產生的記憶寫入提供；相同 key 會回傳既有 row id，避免重試造成重複記憶。人工輸入可保持 `None`。

詳見 [`docs/superpowers/specs/2026-06-05-wukong-memory-design.md`](../../docs/superpowers/specs/2026-06-05-wukong-memory-design.md)
與語意層 [`docs/superpowers/specs/2026-06-06-semantic-recall-design.md`](../../docs/superpowers/specs/2026-06-06-semantic-recall-design.md)。
