# wukong-memory v1 設計

> 子專案 1／4 ──「鬥戰勝佛・本我」：持久記憶核心
> 日期：2026-06-05
> 狀態：已核可，待轉實作計畫

## 背景與定位

「孫悟空」是一個全知全能的個人 AI 助手，取自三個既有專案的概念、以 Rust 從零重建：

| 子專案 | 神話對應 | 概念來源 | 職責 |
|--------|---------|---------|------|
| **1. `wukong-memory`** | 鬥戰勝佛／本我 | Memoria | 持久記憶：remember / recall、scope 隔離、時間衰減 |
| 2. `wukong-gateway` | 齊天大聖／肉身 | TeleNexus | Telegram + Web + CLI 進入點、驅動 AI、排程、可觀測性 |
| 3. `wukong-orchestrator` | 七十二變／分身 | tao-of-coding | 多角色調度引擎、技能路由 |
| 4. `wukong`（金箍棒） | 修成正果 | 三者融合 | 孫悟空人格層、單一控制點 |

建造順序：記憶 → 閘道 → 編排 → 人格圓滿（本我 → 肉身 → 分身 → 成佛）。每柱各自走「設計→計畫→實作」一輪，建立在前一柱之上、可獨立測試交付。

本文件只涵蓋 **子專案 1：`wukong-memory` v1**。

## 目標與非目標

### v1 目標
- 詞彙式（lexical）召回：keyword / tree / hybrid，純 SQLite、零外部模型、完全離線
- `remember` / `recall` + scope 隔離（global / project / agent / user）
- 時間衰減評分（90 天半衰期）
- 對外開放：核心函式庫 crate + HTTP API

### v1 非目標（延後）
- 語意向量檢索（架構不為此預留特殊設計，但 recall 以 `mode` 列舉切換，未來可加 `Vector`/`Semantic` 變體）
- markdown/wiki 編譯、governance lint、consolidation/prune、recall telemetry、sources 匯入、MCP/libSQL
- CLI 二進位（v1 只做 lib + HTTP；CLI 留待後續）

## 技術棧

Rust + tokio + axum + sqlx(sqlite, bundled) + FTS5。整個 `wukong` workspace 一開始就走 tokio 非同步，與後續三柱棧一致，避免日後橋接同步/非同步。

## 架構：Workspace 與 crate 佈局

```
wukong/
├── Cargo.toml                      # workspace
├── crates/
│   ├── wukong-memory/              # v1 主角：lib crate（核心邏輯，可被其他柱直接引用）
│   │   ├── src/
│   │   │   ├── lib.rs              # 公開 API：Memory::open(), remember(), recall(), stats()
│   │   │   ├── model.rs            # 領域型別：Scope, MemoryRecord, RememberInput, RecallQuery, RecallHit, WukongResult<T>
│   │   │   ├── scope.rs            # scope 解析與階層
│   │   │   ├── store/              # sqlx sqlite：連線池、schema、migrations
│   │   │   ├── recall/             # keyword / tree / hybrid
│   │   │   ├── scoring.rs          # 時間衰減 + 綜合排序
│   │   │   └── error.rs            # thiserror 錯誤列舉
│   │   ├── migrations/             # sqlx 遷移（含 FTS5 虛擬表）
│   │   └── tests/                  # 整合測試
│   └── wukong-memoryd/             # HTTP 伺服器 bin crate（薄殼，依賴 wukong-memory）
│       └── src/main.rs             # axum router + 設定載入
```

**核心原則**：所有業務邏輯在 `wukong-memory` lib，`wukong-memoryd` 只做 HTTP 轉接。未來 Rust gateway 直接 `use wukong_memory`，不必透過 HTTP。

## 資料模型與儲存

### Scope（對齊 Memoria 字串格式）

```rust
enum Scope { Global, Project(String), Agent(String), User(String) }
// 序列化：「global」「project:Wukong」「agent:main」「user:ray」
```

階層由具體到一般：`project:X` / `agent:X` / `user:X` → `global`。

### SQLite 資料表

- `sessions`：`id, scope, project, created_at, summary`
- `memories`（召回單位）：`id, session_id(fk, nullable), scope, kind, text, created_at, last_recalled_at, recall_count, importance`
  - `kind`：`decision | event | skill | note | summary`
- `memories_fts`：FTS5 虛擬表，鏡射 `memories.text`，供 keyword 檢索（BM25）

啟用 **WAL** 模式；schema 由 `migrations/` 在啟動時自動套用（向後相容升級）。

### remember 寫入

```rust
RememberInput { scope, session_id: Option<String>, items: Vec<MemoryItem> }
MemoryItem    { kind, text, metadata: Option<...> }
```

每個 item 落成一筆 memory，並同步寫入 FTS。`importance` 若 metadata 未指定，預設為 `1.0`（基準值）。

## 召回流程與評分

`RecallQuery { query, top_k = 5, scope: Option<Scope>, mode = Hybrid }`

| mode | 作法 |
|------|------|
| **Keyword** | FTS5 `MATCH` + BM25 排名 |
| **Tree** | 沿 scope 階層（精確 scope → 父層 → global）取回近期＋高重要度記憶，不依賴全文 |
| **Hybrid**（預設） | 同時跑 keyword + tree，合併去重，依綜合分重排 |

### 綜合分（權重可設定）

```
score = α · 正規化BM25  +  β · 時間衰減  +  γ · importance
時間衰減 = 0.5 ^ (age_days / 90)      # 90 天半衰期
```

命中後更新該筆 `last_recalled_at`、`recall_count`；`recall_count` 給微幅加成，常被取用的記憶浮上來。

### Adaptive gate

過短／全停用詞的瑣碎查詢直接回空集合，省成本（對齊 Memoria）。

## HTTP API（`wukong-memoryd`，axum）

| Method | Path | 說明 |
|--------|------|------|
| GET | `/v1/health` | 健康檢查 |
| GET | `/v1/stats` | 統計（記憶數、各 scope 分布） |
| POST | `/v1/remember` | 寫入（body = RememberInput） |
| POST | `/v1/recall` | 召回（body = RecallQuery） |

### 回傳信封（對齊 Memoria `MemoriaResult<T>`）

```rust
WukongResult<T> { data: T, evidence: Vec<Evidence>, confidence: f32, latency_ms: u64 }
```

`evidence` = 召回命中的出處（memory id / scope / score）。

### 設定（env 為主）

- `WUKONG_MEMORY_DB`（預設 `~/.wukong/memory.db`）
- `WUKONG_MEMORY_PORT`（預設 `3917`）

## 錯誤處理

- lib 用 `thiserror` 定義 `MemoryError { Db, NotFound, InvalidScope, InvalidQuery, Serialize }`
- HTTP 層用 axum `IntoResponse` 把錯誤映射成 JSON ＋ 狀態碼（400 / 404 / 500）
- lib 一律回 `Result`，是否 fail-open 交由呼叫端決定

## 測試（TDD，先寫測試）

- **單元**：時間衰減數學、scope 解析與階層、hybrid 排序正確性（用 sqlite `:memory:`）
- **整合**：temp db → remember 後 recall，驗證排序、scope 隔離、adaptive gate 跳過瑣碎查詢
- **HTTP**：用 `tower::ServiceExt::oneshot` 對四個 endpoint 做請求/回應測試

## 驗收標準

1. `cargo test` 全綠（含單元 + 整合 + HTTP）
2. `remember` 後可由 `recall` 取回，且 scope 隔離正確（其他 scope 取不到）
3. hybrid 排序符合綜合分公式（較新／較重要／較常命中者排前）
4. adaptive gate 對瑣碎查詢回空集合
5. HTTP 四個 endpoint 行為與信封格式符合本設計
6. 無新增 lint 錯誤（`cargo clippy`）
