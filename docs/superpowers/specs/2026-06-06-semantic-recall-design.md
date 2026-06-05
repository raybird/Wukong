# 語意向量召回（本機 embedding）設計

> v2 項目 A。在 `wukong-memory` crate 內原地擴充，為記憶召回加入本機語意向量，作為**選用增強層**：未啟用或無模型時，行為完全等同 v1 的 BM25 召回。

**狀態：** 設計已與用戶拍板（2026-06-06）。下一步進 writing-plans。

---

## 目標

讓記憶召回從「字面對得上（BM25）」進化成「意思對得上（語意向量）」，且：

- **自包含**：不新增 crate、不新增進程，維持單一 `wukong` 二進位、離線可用。
- **選用增強**：向量是 optional 疊加層，BM25 仍是真相與基本召回；任何一環不可用即靜默退回 v1。
- **不走 sidecar**：刻意不採用 Memoria 的外掛 `mcp-memory-libsql` 模式，但沿用其「向量=增強層、SQLite=真相」哲學。

## 範圍

在現有 `wukong-memory` crate 內擴充：

```
wukong-memory
├── embed/mod.rs   ← 新：Embedder trait + FastembedBackend + MockEmbedder
├── store/mod.rs   ← 改：embedding/embedding_model 欄位、寫入/讀取向量、補齊查詢
├── recall/mod.rs  ← 改：向量候選源 + δ·語意 納入 rank()
├── scoring.rs     ← 改：combined_score 多一項 δ
├── model.rs       ← 改：Candidate 加 vector_sim
├── error.rs       ← 改：MemoryError 加 Embed 變體
└── lib.rs         ← 改：open 啟動背景補齊、recall 走向量路徑
```

非範圍：sqlite-vec、向量索引（ANN）、跨進程/跨機共享、重排 API。

## 元件設計

### 1. Embedding 層（`embed/mod.rs`）

```rust
pub trait Embedder: Send + Sync {
    fn embed(&self, text: &str) -> Result<Vec<f32>>;
    fn embed_batch(&self, texts: &[String]) -> Result<Vec<Vec<f32>>>;
    fn dim(&self) -> usize;
    fn model_id(&self) -> &str;
}
```

- **`FastembedBackend`**（feature `embed`）：包 `fastembed`，預設模型 `all-MiniLM-L6-v2`（384 維）。首次取得需連線下載至 cache，之後離線；可用 env 指定本機模型路徑。
- **`MockEmbedder`**（恆可用，測試用）：把字串以確定性雜湊投影成固定維度向量，讓整條管線在無真模型下也能 TDD。
- **Cargo feature `embed`（預設關）**：隔離 fastembed/ort 重相依，預設 build 維持輕量；開啟才有真模型。

### 2. 儲存（`store/mod.rs`）

- `memories` 表新增兩欄（nullable）：
  - `embedding BLOB` — `Vec<f32>` 以 little-endian bytes 序列化。
  - `embedding_model TEXT` — 產生此向量的 model_id（換模型時可辨識重算）。
- schema 維持 idempotent；用 `PRAGMA table_info(memories)` 偵測欄位是否存在，不存在才 `ALTER TABLE ... ADD COLUMN`（相容既有 DB）。
- 寫入：`insert_memory` 在 feature 開且 embedder 可用時，順帶寫入向量；否則欄位留 NULL。
- 讀取：召回時取出候選 row 的 `embedding` BLOB 反序列化。
- 補齊查詢：`SELECT id, text FROM memories WHERE embedding IS NULL`（分批）。

### 3. 召回整合（`recall/mod.rs` + `scoring.rs` + `model.rs`）

- `Candidate` 新增 `vector_sim: Option<f64>`。
- 新增**向量候選源**：對查詢 `embed(query)`，讀取**該 scope（含 global 階層）內所有 `embedding` 非 NULL 的 row**，逐筆算 **cosine 相似度**（純 Rust 暴力比對），取相似度最高的 N 筆作為候選。個人規模（數千~數萬筆）幾毫秒內完成；此源獨立於 keyword／tree 源，三者再 `merge_candidates`。
- `combined_score` 由四項組成：
  ```
  score = α·詞彙(BM25) + β·時間衰減 + γ·重要度 + δ·語意
  ```
  - `bm25` 與 `vector_sim` 各自在候選集內 **min-max 歸一到 [0,1]** 再加權。
  - 建議預設權重：`α=0.4, β=0.25, γ=0.15, δ=0.2`（沿用 v1 比例縮放）。
  - 缺 `vector_sim` 的 row（補齊前的舊資料）語意分視為 0，仍靠 BM25 出線。
- `RecallMode::Hybrid`（預設）現含四源；`Keyword`/`Tree` 維持原樣。
- **Adaptive gate 不變**：瑣碎查詢（過短／全停用詞）仍直接略過召回，連 `embed(query)` 都省。

### 4. 舊資料背景補齊（`lib.rs`）

- `Memory::open` 完成後，於 feature `embed` 開且模型可用時，啟動一個**背景 tokio task**：
  - 迴圈讀 `embedding IS NULL` 的 row，分批（如每批 32 筆）`embed_batch` 後 `UPDATE`，批間 `tokio::task::yield_now`。
  - 補齊前那些 row 的 `vector_sim` 缺席（=0，仍可 BM25 召回）；補完即全面語意。
  - 失敗只記 log（`tracing`／eprintln），絕不影響主流程。

### 5. 設定與退場

- 環境變數：
  - `WUKONG_EMBED=1` — 啟用語意層（需 feature `embed` 一併編入）。
  - `WUKONG_EMBED_MODEL` — 指定模型名。
  - `WUKONG_EMBED_PATH` — 指定本機模型路徑（離線部署）。
- 退場：feature 沒編 / env 沒開 / 模型載入失敗 → 一律靜默退回 v1 BM25（`δ=0`），絕不讓助手不可用。

## 資料流（一次 recall）

```
recall(query):
  is_trivial? ──yes──► 略過（同 v1，連 embed 都不做）
       │ no
       ▼
  [keyword 源] FTS5 → bm25 候選        ┐
  [tree 源]    scope 近期候選           ├─ merge_candidates
  [vector 源]  embed(query)→cosine 候選 ┘   （embed 不可用時此源為空）
       ▼
  filter_by_scope
       ▼
  rank: 各源 min-max 歸一 → α·bm25 + β·decay + γ·importance + δ·vector
       ▼
  truncate top_k → hits[]
```

## 錯誤處理

- `MemoryError` 新增 `Embed(String)` 變體。
- embedding 任何失敗（模型載入、推論）**不向上炸毀** remember/recall：寫入時略過向量欄位、召回時向量源為空，主流程續行。

## 測試策略

- **單元**：
  - cosine 相似度正確性（含零向量、正交、相同向量邊界）。
  - 向量 BLOB 序列化／反序列化往返。
  - 四源 `rank` 加權與 min-max 歸一（含某源缺席 = 該分量 0）。
- **整合**（用 `MockEmbedder`，不需真模型）：
  - 端到端 remember→recall 語意命中：存「記憶體釋放後存取」，查語意相近但字面不同者能召回。
  - 背景補齊把既有 NULL row 的 embedding 補上。
  - feature `embed` 關閉時：編譯通過、行為與 v1 等同（向量欄位恆 NULL、δ 分量為 0）。
- **回歸**：v1 既有 29 個記憶測試全數續綠。

## 對使用者的影響

- 預設 build（不開 feature）：零變化、零新相依。
- 開 `embed` feature + `WUKONG_EMBED=1`：多一個模型檔（~80MB，首次下載後快取），每次 remember/recall 多幾毫秒算向量，CPU 即可，無需 GPU。記憶召回從字面升級為語意。
