# 記憶模型與服務

> ← 回到 [主 README](../README.md)｜相關文件:[CLI 參考](cli-reference.md)

## 記憶模型

- **儲存**：SQLite + FTS5（BM25 關鍵字檢索），啟用 WAL。 FTS5 的關鍵字匹配會將輸入的 token 以 `OR` 連接查詢。
- **召回模式**：`keyword`（FTS5）、`tree`（依 scope 階層取近期）、`hybrid`（合併重排，預設）。
- **排序**：採用混合正規化計分：
  - **Min-Max 正規化**：因 BM25（越小越好）與 Cosine 語意相似度（越大越好）量綱不同，排序前會先對所有候選人進行 Min-Max 正規化至 $[0, 1]$ 區間。
  - **權重公式**：
    $$\text{Score} = \alpha \cdot \text{Lexical} + \delta \cdot \text{Semantic} + \beta \cdot \text{Decay} + \gamma \cdot \text{Importance}$$
    （預設 $\alpha=0.4$、$\delta=0.2$、$\beta=0.25$、$\gamma=0.15$）。其中時間衰減 $\text{Decay}$ 半衰期為 90 天。
  - **對數熱點加成**：常被召回的熱點記憶會獲得對數加成：
    $$\text{Score}_{\text{final}} = \text{Score}_{\text{base}} + 0.02 \cdot \ln(1 + \text{recall\_count})$$
    同時觸發 `touch_recalled` 更新其 `last_recalled_at` 時間戳記以延緩衰減。
- **語意向量召回（選用增強層）**：cargo feature `embed` + `WUKONG_EMBED=1` 啟用本機 embedding（fastembed `all-MiniLM-L6-v2`，384 維，離線）。向量存同一 SQLite、純 Rust cosine、併入 Hybrid 綜合分；未啟用或模型載入失敗即優雅退回 BM25。既有記憶開機背景補齊。
- **Scope 階層**：`project:X` / `agent:X` / `user:X` 召回時自動含 `global`。
- **Adaptive gate**：過短／全停用詞的瑣碎查詢直接略過召回。
- **記憶維護（手動）**：`consolidation`（`Summarizer` trait 注入，預設機械串接、cli 注入 opencode 真摘要）把零碎記憶聚合成 `Summary`；`prune` 安全刪除已摘要或低價值記憶；`markdown` 雙持久化（`WUKONG_MD_DIR` 開啟、per-scope 單向鏡像）；`snapshot` 健康快照。詳見 `wukong memory <op>`（見 [CLI 參考](cli-reference.md)）。

## 記憶服務（選用）

`wukong-memory` 同時提供一個獨立的 HTTP 服務 `wukong-memoryd`，供跨語言或外部工具存取：

```bash
WUKONG_MEMORY_PORT=3917 cargo run -p wukong-memoryd
curl -s http://127.0.0.1:3917/v1/health        # {"status":"ok"}
```

| Method | Path | 說明 |
| :--- | :--- | :--- |
| GET | `/v1/health` | 健康檢查 |
| GET | `/v1/stats` | 統計（總數、各 scope 分布） |
| GET | `/v1/snapshot` | 健康快照（總數/類型/年齡/embedding 覆蓋率/維護候選數） |
| POST | `/v1/remember` | 寫入記憶 |
| POST | `/v1/recall` | 召回記憶 |

回應信封：`{ data, evidence[], confidence, latency_ms }`。
