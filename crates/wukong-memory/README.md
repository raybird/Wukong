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
    items: vec![MemoryItem { kind: MemoryKind::Decision, text: "用 Rust 重寫".into(), importance: None }],
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
| `Hybrid`（預設） | 合併兩者，依綜合分重排 |

綜合分：`α·正規化BM25 + β·時間衰減(90天半衰期) + γ·importance`，常被召回者加成。
過短／全停用詞的查詢由 adaptive gate 直接略過。

## Scope

`Global` / `Project(name)` / `Agent(name)` / `User(name)`，序列化為 `global`、`project:X` 等。
召回具體 scope 時自動含 `global`（階層 fallback）。

## 儲存

SQLite（WAL）+ FTS5 外部內容表。schema 啟動時冪等套用。回應信封：`WukongResult<T> { data, evidence[], confidence, latency_ms }`。

詳見 [`docs/superpowers/specs/2026-06-05-wukong-memory-design.md`](../../docs/superpowers/specs/2026-06-05-wukong-memory-design.md)。
