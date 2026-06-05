# wukong-memoryd

> `wukong-memory` 的 HTTP 服務（薄殼，axum）

讓跨語言或外部工具能存取記憶核心。所有業務邏輯在 `wukong-memory`，本 crate 只做 HTTP 轉接。

## 啟動

```bash
WUKONG_MEMORY_PORT=3917 cargo run -p wukong-memoryd
curl -s http://127.0.0.1:3917/v1/health   # {"status":"ok"}
```

設定：`WUKONG_MEMORY_DB`（預設 `$HOME/.wukong/memory.db`）、`WUKONG_MEMORY_PORT`（預設 `3917`）。

## Endpoints

| Method | Path | 說明 |
| :--- | :--- | :--- |
| GET | `/v1/health` | 健康檢查 |
| GET | `/v1/stats` | 統計（總數、各 scope 分布） |
| POST | `/v1/remember` | 寫入（body = RememberInput） |
| POST | `/v1/recall` | 召回（body = RecallQuery） |

範例：

```bash
curl -X POST http://127.0.0.1:3917/v1/remember -H 'content-type: application/json' \
  -d '{"scope":"global","items":[{"kind":"note","text":"hello"}]}'

curl -X POST http://127.0.0.1:3917/v1/recall -H 'content-type: application/json' \
  -d '{"query":"hello"}'
```

回應信封：`{ data, evidence[], confidence, latency_ms }`。錯誤映射為 400/404/500。
