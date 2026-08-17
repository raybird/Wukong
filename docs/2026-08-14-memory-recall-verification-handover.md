# 記憶召回缺陷與驗證紀律 Handover

日期：2026-08-14

關聯文件：

- `CHANGELOG.md`（`[0.21.5]` 的 CJK 修正、`[0.21.0]`–`[0.21.4]`）
- `crates/wukong-memory/tests/cjk_recall.rs`（本文第 1 節的回歸測試）
- `docker-compose.memoria.yml`、`scripts/test-memoria-runtime.sh`

## 摘要

一次記憶層效能優化，意外連出三件事：**中文查詢從來沒有走到關鍵字索引**（已修）、
**排程產出會淹沒使用者的記憶 scope**（未決定，現在改成本為零）、以及一組在同一天
重複命中四次的驗證盲區（已寫進 `CLAUDE.md`）。

有一個前提貫穿全文，值得先講：**優化前沒有先確認主要使用情境走的是哪條路徑。**
當天花了整天優化 bm25 驅動的召回（補索引、scope 下推、分數正規化），而使用者是
中文使用者——那條路徑對他的查詢幾乎從來沒有被走到過。

---

## 1. 中文查詢從來沒有走到關鍵字索引（已修）

### 症狀

`memories_fts` 用 FTS5 預設的 unicode61 tokenizer，它把**一整串連續 CJK 當成單一
token**。所以只有在查詢字串與文件裡那串中文一字不差相同時才會命中。

實測（修正前，同一語料）：

| 查詢 | 命中 | 來源 | bm25 排序 |
|---|---|---|---|
| `幫我看一下排程設定` | 1 | `cjk_fallback` | 無 |
| `排程 設定` | **0** | — | 無 |
| `排程` | 2 | `cjk_fallback` | 無 |
| `記憶庫現在多大` | 1 | `keyword` | 有 |
| `記憶庫 多大` | **0** | — | 無 |

`記憶庫現在多大` 是唯一走到 FTS 的，因為文件裡那串連續 CJK 剛好**完全等於**查詢字串
——也就是使用者一字不差重複整句時才有效。

### 為什麼難以察覺

其餘全部由 `cjk_fallback`（`LIKE '%查詢%'` 子字串比對）頂著，而它**不會空手**——它回
得出東西，只是：

1. `排程 設定` 這種最自然的多關鍵字寫法會**完全落空**，因為 fallback 找的是含空格的
   字面字串。
2. fallback 的候選 `bm25: None` → `lexical_norm = 0` → **相關性排序失效**，排的是
   時間衰減而不是相關度。
3. `confidence` 由 `relevance` 算出，所以**中文召回一律回報 0.000**：

```
排程設定              hits=1  confidence=0.000
scheduler settings   hits=1  confidence=1.000
```

第 3 點連帶讓診斷數字失真：記憶健康快照的 `avg_top_relevance` 對純中文使用者恆讀
0.000，與「從來沒找到東西」在數字上**無法區分**。

### 修法

在**寫入與查詢兩側**把連續 CJK 展開成重疊 2-gram（`排程設定` → `排程 程設 設定`），
讓 tokenizer 有邊界可切。

**只做單側無效。** FTS5 比對的是 token 等值，文件那顆大 token 對不上任何短查詢 token。
新增 `memories.search_text` 欄位存放展開後的形式、FTS 索引建在它上面——保留為真實欄位
是為了讓 external-content 表與 delete trigger 讀到同一個值，一致性才不會壞。

修正後 `排程 設定` 5 筆、來源 `keyword`、confidence 1.000，與英文對等。

既有資料庫自動遷移（回填 `search_text` + 重建 FTS + 換 trigger），5,000 列實測
**232 ms**。

### 這個修法不能外推

同期 Memoria 有同類缺陷但**只改查詢端就夠**，因為它的 `recall_fts` 用
`tokenize='trigram'`——索引的是每個 3 字元視窗，短查詢 token 以子字串命中，文件側
不用動。

**「兩側對稱」是 token 等值架構的要求，不是通則。** 移植前先確認索引端的 tokenizer。

---

## 2. 排程產出會寫進使用者的記憶 scope（未決定）

`persona::scheduling_capability_hint` 產生的提示是
`schedule add-turn ... --scope "<當前 scope>"`，規則寫「除非使用者另外指定，否則一律
沿用」。`executor.rs` 在 job 觸發時 `cfg.scope = scope.clone()`。

所以使用者請 agent 排一個定期任務，它會用**使用者當下的 scope** 建立；之後每次觸發都
往那裡寫 `User: <提示詞>` + `Assistant: <報告>`。

外部參照：另一個以 Memoria 為記憶層的部署實測過同型資料——**單一一種排程任務佔語料
60%**，真人對話被近乎逐字重複的自動報告以 5:1 稀釋。

Wukong 多一層問題：到 `WUKONG_MEMORY_CONSOLIDATE_THRESHOLD` 之後這些事件會折成
`Summary`，而 **`Summary` 永遠不可 prune**（prune 只挑 `event`/`note`）。原始事件會被
清掉，但近乎相同的摘要會永久留在使用者的 scope 裡。

**現在改成本為零**：生產部署的記憶庫是空的（`memories`、`recall_telemetry` 皆為 0 列，
代表從未服務過任何一次 turn）。等資料開始累積之後就要處理遷移或與雜訊共存。

選項大致是「排程 turn 預設用獨立 scope」（召回的 ancestry 過濾就會自動把它排除在使用者
scope 之外）或「維持現狀但讓排程摘要可 prune」。這是產品判斷，尚未拍板。

---

## 3. 未來若要加記憶健康檢查：連線不可 pool

Wukong 目前**沒有**任何 SQLite 完整性檢查。加之前要知道下面這件事。

Memoria 遇到的缺陷：長生命週期的 pooled 連線，在**任何其他行程寫過該資料庫之後**，
完整性檢查就會誤報 `malformed inverted index for FTS5 table`，且：

- 不是唯讀專屬——read-write handle 一樣壞。
- **重新 `prepare` 無效**，強制讀 `schema_version` 觸發 schema 重載也無效。
- **只有關閉並重開連線會恢復。**
- 那條陳舊連線對它宣稱損壞的那張索引，一般查詢與 FTS MATCH **都跑得好好的**。

Wukong 在生產環境符合全部三個前提：`wukong-web`／`wukong-telegram`／
`wukong-schedulerd` 各自持有 sqlx `SqlitePool` 指向同一個 `/data/memory.db`，而
`memories_fts` 是 FTS5。

本 repo 實測**不重現**：以真正獨立的 OS 行程寫入、以及對真實 Memoria schema
（trigram `recall_fts`）測試，兩者的 `quick_check` 與 FTS5 `integrity-check` 都回 `ok`；
跨 pool 的讀取可見性也正確。目前證據指向讀取端 binding 的差異（sqlx/libsqlite3 對上
better-sqlite3），而且**只影響健康訊號，不影響資料讀取**——兩邊獨立得到同一結論。

**要小心的不是修法，是防退化。** 正確寫法「每次檢查開一條新連線、檢查完關掉」看起來
像一個沒被優化到的地方，很容易被後來的人 pool 起來當成效能改善——而那會把同一個 bug
搬到更難聯想的位置。那段程式的註解必須寫明**為什麼不能 pool**。

若真的報出損壞，最便宜的判別法是**去查那張它宣稱壞掉的表**：MATCH 查得動就是誤報，
不需要重啟服務。

---

## 4. 實測數字附錄

同期記憶層效能優化（20,000 筆含 embedding）：

| 項目 | 前 | 後 |
|---|---|---|
| 寫入 20,000 筆 | 26.6 s | 8.8 s |
| 帶 scope 的 hybrid recall | ~554 ms | ~96 ms |

Memoria runtime image：

| 項目 | 大小 |
|---|---|
| image 總計（優化前 → 後） | 2.15 GB → 1.26 GB |
| `onnxruntime-node` | 513 MB |
| `onnxruntime-web`（Node 下永不載入，但為硬相依） | 130 MB |
| `@huggingface/transformers`（含模型快取 130 MB） | 146 MB |
| `better-sqlite3` | 12 MB |
