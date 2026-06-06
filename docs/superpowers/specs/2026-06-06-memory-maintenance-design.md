# 記憶維護(Consolidation / Prune / Markdown / Snapshot)設計

**日期:** 2026-06-06
**狀態:** 已核可(roadmap 項目 D)
**前置:** v0.2.0 語意召回(`Embedder` trait)、v0.3.0 REPL/串流。

## 目標

讓 `wukong-memory` 在記憶量變大後仍保持精簡、可觀測、可人工檢視:

1. **Consolidation** — 把零碎 `Event`/`Note` 聚合成有意義的 `Summary`。
2. **Prune** — 安全刪除已被摘要或低價值的記憶。
3. **Markdown 雙持久化** — 每次 remember 同步寫一份人類可讀、git 友善的鏡像。
4. **可觀測性快照** — 比現有 `stats()` 更豐富的健康指標。

## 設計原則

- **DB 為唯一真相來源**;markdown 是單向衍生視圖,絕不從 md 回讀。
- **trait 注入**,沿用既有 `Embedder` 模式:memory 層定義介面,cli/gateway 層注入 opencode 實作。**底層 agent 只以 opencode 為準。**
- **顯式、可控、可預覽**:刪資料只走手動 CLI 子命令,並提供 `--dry-run`。
- **優雅退場**:markdown 未設路徑即不寫;寫檔失敗只 warn 不阻斷 remember。

## 架構總覽

`wukong` 二進位新增一組 `memory` 子命令承載維護操作;核心邏輯放 `wukong-memory`,`wukong-memoryd` 共用統計層。

```
wukong [PROMPT...]                       # 對話 / REPL(維持不變)
wukong memory snapshot [--scope X]       # 健康快照(印表格)
wukong memory consolidate [--scope X] [--dry-run]
wukong memory prune       [--scope X] [--dry-run]
wukong memory export      [--dir D]      # 依 DB 全量重建 markdown
```

clap 結構:頂層 `Cli` 保留既有 `prompt: Vec<String>` 與旗標,新增 `#[command(subcommand)] command: Option<MemoryCmd>`。以 `args_conflicts_with_subcommands = true` 與 `subcommand_negates_reqs = true` 讓「裸 prompt / REPL」與子命令共存無歧義:有子命令走維護路徑,無子命令走原本對話路徑。

## 1. Consolidation

### memory 層介面

```rust
/// 把一組記憶文字濃縮成一段摘要。
pub trait Summarizer: Send + Sync {
    fn summarize(&self, texts: &[String]) -> Result<String>;
}

/// 機械式預設:依序串接,無 LLM。
pub struct ConcatSummarizer;
impl Summarizer for ConcatSummarizer { /* texts.join("\n") 加標頭 */ }
```

```rust
pub struct ConsolidatePolicy {
    /// 每批最多幾筆 event 併成一筆 summary。
    pub batch_size: usize,   // 預設 20
}

pub struct ConsolidatePlan {
    pub batches: Vec<Vec<i64>>,   // 每批的來源記憶 id
}

impl Memory {
    /// 規劃但不執行(供 --dry-run)。
    pub async fn plan_consolidation(&self, scope: &str, policy: &ConsolidatePolicy)
        -> Result<ConsolidatePlan>;

    /// 執行:對每批呼叫 summarizer、插入 Summary、把來源列標記 consolidated_into。
    pub async fn consolidate(&self, scope: &str, policy: &ConsolidatePolicy,
        summarizer: &dyn Summarizer) -> Result<Vec<i64>>;  // 回傳新建 summary ids
}
```

### 聚類規則

- 範圍限定單一 `scope`。
- 候選 = 該 scope 內 `kind IN (event, note)` 且 `consolidated_into IS NULL` 的列,依 `created_at` 升序。
- 切批:優先同 `session_id` 一批;`session_id` 為 NULL 者依 `created_at` 升序每 `batch_size` 筆一批。
- 每批產生一筆 `kind = Summary` 的新記憶(`scope` 同來源、`importance` 取批內最大值、`session_id = NULL`),來源列 `consolidated_into = <新 summary id>`。
- 若啟用 embedding,新 Summary 比照 remember 流程寫入向量,融入既有召回。

### cli/gateway 層注入

```rust
/// 以 opencode 後端做真摘要。
pub struct OpencodeSummarizer<'a, B: AiBackend> { backend: &'a B, runtime: Handle }
```

`memory consolidate` 子命令建構 `OpencodeSummarizer`,以非串流 `run` 送 prompt:「請把以下記憶濃縮成一段精簡摘要,保留關鍵決策與事實:\n\n{texts}」。`--dry-run` 只呼叫 `plan_consolidation` 並印出每批筆數與來源摘要,不注入 summarizer、不變更資料。

## 2. Prune

可刪謂詞(任一主路或輔路成立即可刪,且不在永不刪集合):

- **主路**:`consolidated_into IS NOT NULL`(資訊已被 Summary 保留)。
- **輔路**:`kind IN (event, note)` 且 `created_at < now - 30 天` 且 `recall_count = 0` 且 `importance < 0.5`。
- **永不刪**:`kind IN (decision, skill, summary)`。

```rust
pub struct PrunePolicy {
    pub max_age_secs: i64,   // 預設 30*86400
    pub importance_floor: f64, // 預設 0.5
}

impl Memory {
    pub async fn plan_prune(&self, scope: Option<&str>, policy: &PrunePolicy)
        -> Result<Vec<i64>>;          // 將刪的 ids
    pub async fn prune(&self, scope: Option<&str>, policy: &PrunePolicy)
        -> Result<u64>;               // 實刪筆數
}
```

刪除同時清掉 FTS5 鏡像列(既有 trigger 或顯式 delete)。`--dry-run` 印出 `plan_prune` 結果與筆數,不動資料。

## 3. Markdown 雙持久化

- **啟用**:設 `WUKONG_MD_DIR` 環境變數(或對應 config 欄位)才啟用;未設則完全不寫。
- **佈局**:每個 scope 一個檔。檔名由 scope 字串清洗而來(`:`/`/` → `_`),例如 `project:Wukong` → `project_Wukong.md`。
- **寫入**:`remember` 落盤 DB 成功後,把該批新記憶 append 到對應 scope 檔。每筆一段:
  ```
  ## 2026-06-06T12:34:56Z · event
  User: ...
  ```
- **best-effort**:append 失敗只記 `warn`(stderr),不讓 `remember` 失敗——DB 已落盤即視為成功。
- **export**:`memory export [--dir D]` 忽略增量,依 DB 現況把所有 scope 的 md 全量重寫(供初次補導或修復漂移)。

memory 層提供純函式 `render_markdown_entry(created_at, kind, text) -> String` 與 `scope_to_filename(scope) -> String`,寫檔 I/O 由薄封裝 `MarkdownSink` 持有 `dir`,在 `remember` 後呼叫;沒有 sink 即略過。

## 4. 可觀測性快照

```rust
pub struct KindCount { pub kind: MemoryKind, pub count: i64 }
pub struct AgeBuckets { pub last_day: i64, pub last_week: i64, pub last_month: i64, pub older: i64 }
pub struct EmbeddingCoverage { pub embedded: i64, pub total: i64 } // pct 由呼叫端算

pub struct Snapshot {
    pub total: i64,
    pub by_scope: Vec<ScopeCount>,
    pub by_kind: Vec<KindCount>,
    pub age: AgeBuckets,
    pub embedding: EmbeddingCoverage,
    pub consolidation_candidates: i64, // 未被摘要的 event/note 數
    pub prune_candidates: i64,         // 符合 prune 謂詞的數
}

impl Memory { pub async fn snapshot(&self, scope: Option<&str>) -> Result<Snapshot>; }
```

- **CLI**:`memory snapshot` 印人類可讀表格。
- **memoryd**:新增 `GET /snapshot`(可選 `?scope=`)回 `Snapshot` 的 JSON。
- 核心統計查詢在 memory 層,兩個表面共用。

## Schema 變更

冪等遷移,比照既有 embedding 欄位的 PRAGMA 檢查後 `ALTER`:

```sql
ALTER TABLE memories ADD COLUMN consolidated_into INTEGER;  -- nullable, 指向 summary 的 memories.id
```

## 測試策略

- `MockSummarizer`:回傳決定性字串(如 `format!("SUMMARY({})", texts.len())`),不依賴 opencode。
- in-memory / NamedTempFile sqlite,沿用既有測試慣例。
- consolidation:plan 切批數正確;consolidate 後來源列 `consolidated_into` 被設、Summary 列出現、回傳 ids 正確。
- prune:謂詞單測(主路/輔路/永不刪各一)、`plan_prune` 與 `prune` 一致、dry-run 不變更。
- markdown:`render_markdown_entry` / `scope_to_filename` 純函式單測;`MarkdownSink` append 後檔案含該筆;無 sink 時 remember 正常;export 重建內容。
- snapshot:塞入已知分佈後各計數正確(by_kind、age buckets、coverage、候選數)。
- memoryd:`/snapshot` 回 JSON 結構正確。

## 非目標(YAGNI)

- 不做自動/排程觸發(僅手動子命令)。
- 不從 markdown 回讀或雙向同步。
- 不做跨 scope 的全域 consolidation;一次一個 scope。
- 不做 markdown 的增量去重/壓縮;export 為全量重寫。
