# Issue 9 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)。

## 現況（2026-10-06 讀碼與 RunWuKong 資料確認）

- **寫入**：`turn.rs` 以 `captured_session` 或 `stored` 為 `turn_key`，key 為 `runtime:{turn_key}:user|assistant`；`store::insert_memory` 遇到相同 key 直接回傳既有 id，不寫入。原 commit `e9a9e85` 的目的是讓同一次回合的寫入冪等，但 session 長期沿用，結果第二回合起全部被當成重複。
- **召回**：`run_turn` 開頭以 `RecallMode::Hybrid`、`top_k = 5` 召回一次，結果注入每一棒。Hybrid 合併關鍵字 50 筆、同 scope 最新 50 筆與向量前 20 筆，依 `lexical 0.4 / semantic 0.2 / decay 0.25 / importance 0.15` 排序；沒有相關度門檻，最新的記憶靠時間與重要度分數就能進前 5。
- **注入**：`gateway::prompt::compose_prompt` 逐筆輸出全文，沒有長度上限。
- **session**：只有最後一棒帶 session；輔助棒每次都是新的、無狀態。session 預設每 20 回合壓縮一次（`WUKONG_SESSION_COMPACT_EVERY_TURNS`）。
- **合併**：schedulerd 每 900 秒檢查，未合併的 `event/note` 達 40 筆的 scope 會把全部候選每 20 筆交給 `OpencodeSummarizer`（`run_ephemeral`）摘要，寫入 `summary` 後標記並刪除原始資料。`memory_auto_maintenance` 以 `?` 傳遞錯誤，第一個失敗的 scope 會中止整輪；`Memory::consolidate` 不檢查摘要是否為空白。
- **其他 Hybrid 使用者**：Telegram、REPL、Web 的記憶查詢功能與 memoryd 也用 `RecallMode::Hybrid`，所以不能改變 Hybrid 本身的語意。

## 設計

### 寫入 key（SCN-001）

`run_turn` 開始時產生本回合專屬的識別碼，key 改為 `runtime:{scope}:{turn_id}:user|assistant`。同一次 `run_turn` 只寫一次；新的回合（即使輸入相同）一定寫入。識別碼的產生方式由實作時沿用 workspace 既有依賴決定，不新增 crate。

### 召回（SCN-002～004）

```diff
 run_turn
-  recall(Hybrid, top_k=5)            → 注入每一棒
+  relevant = recall(新模式：關鍵字＋向量，不含最近來源；向量需達相似度門檻)
+  last_turn = 本 scope 最近一個回合的 User／Assistant（不含本回合）
+  最後一棒注入 relevant
+  輔助棒注入 relevant ＋ last_turn（去重）
```

- 新增一個 `RecallMode` 變體，只給 `run_turn` 使用；既有 `Hybrid` 行為不變。
- 相關度判斷採結構訊號：命中來自 `keyword`、`cjk_fallback`，或向量相似度達門檻（TBD-2）。只靠 `recent` 入選的不算。
- 最近一個回合用既有的 `recent_candidates` 取得，限本 scope、最多 2 筆。

### 注入長度（SCN-005）

`compose_prompt` 對每筆記憶截斷到固定字元數（初始值 800 字元，以 `char` 計，避免切斷 UTF-8），超過時加上截斷標示。只影響注入內容，不改資料庫。

### 合併（SCN-006～008）

- `memory_auto_maintenance`：每個 scope 的 consolidate／prune 錯誤改為記錄 `memory_consolidate_failed scope=… error=…` 並繼續下一個 scope；報表加上失敗數。
- SCN-007：`Memory::consolidate` 收到空白摘要時，該批不寫摘要、不標記來源，回報錯誤，交給 scope 隔離處理。
- SCN-008：以 schedulerd 的維護路徑搭配按需 OpenCode 真實程序執行一次合併。模型輸出沿用 issue 7 的可控制方式（`docs/issues/issue-0007/probe-managed.py`），不呼叫外部 LLM。

## 實作步驟

1. ⏳ **SCN-006：合併的 scope 失敗隔離** — 產出：`maintenance.rs` 的隔離與報表、以兩個 scope 一成一敗的測試。相依：無。完成判準：測試先因第二個 scope 未合併而失敗，修改後通過，且失敗 scope 的原始記憶數不變。
2. ⏳ **SCN-007：空白摘要不刪原始記憶** — 產出：`Memory::consolidate` 的空白檢查與測試。相依：步驟 1。完成判準：空白摘要的測試先紅後綠，來源記憶未被標記。
3. ⏳ **SCN-008：按需模式 schedulerd 真實合併** — 產出：真實程序的驗證腳本或 ignored 整合測試，以及執行紀錄。相依：步驟 1。完成判準：紀錄中有非空摘要、被刪除的來源筆數，以及結束後沒有 OpenCode 程序；若揭露其他失敗，回報後再決定處理方式。
4. ⏳ **SCN-005：注入長度上限** — 產出：`compose_prompt` 截斷與測試。相依：無。完成判準：超長記憶的測試先紅後綠，斷言用字面期望值，資料庫內容不變。
5. ⏳ **SCN-002、SCN-003：最後一棒只注入相關記憶** — 產出：新的召回模式、相關度門檻、`run_turn` 最後一棒改用它，以及測試。相依：步驟 4。完成判準：以假 backend 擷取最後一棒的 prompt，「只有不相關的最新記憶」時沒有 `[相關記憶]`，「有相關舊記憶」時含該筆；兩者先紅後綠。斷言檢查命中來源（`source_signals`），不只檢查筆數。
6. ⏳ **SCN-004：輔助棒取得最近一個回合** — 產出：輔助棒的注入組合與測試。相依：步驟 5。完成判準：多棒回合中輔助棒 prompt 含上一回合、不含更早的不相關回合，先紅後綠。
7. ⏳ **SCN-001：每個回合都寫入記憶** — 產出：回合識別碼與 key 修改、連續兩回合的測試。相依：步驟 1、3、5、6（先讓召回與合併準備好，再啟動寫入）。完成判準：同 session 連續兩回合（含相同輸入）後有四筆，測試先紅後綠。
8. ⏳ **收尾** — 產出：AGENTS.md「一回合資料流」更新、CHANGELOG `[Unreleased]`、全量 `cargo test`、`cargo clippy --all-targets -- -D warnings`、`gitnexus_detect_changes`。相依：步驟 1～7。完成判準：命令全綠，文件描述與實作一致。

## 測試策略

| 驗收 | 層級 | 位置 |
|------|------|------|
| SCN-001 | runtime 整合（假 backend＋暫存 SQLite） | `wukong-runtime` 的 `turn.rs` 測試 |
| SCN-002～004 | runtime 整合，擷取各棒 prompt | 同上 |
| SCN-005 | 單元 | `wukong-gateway/src/prompt.rs` |
| SCN-006、SCN-007 | runtime／memory 整合（假 summarizer） | `maintenance.rs`、`wukong-memory` 測試 |
| SCN-008 | 真實程序（按需 OpenCode、可控制模型輸出） | `wukong-schedulerd` ignored 測試或 issue 目錄的探針，紀錄保存在本計畫 |

SCN-002～006 的外迴圈與內迴圈都在同一個整合層級，可依 verification.md 合併紅燈並記錄理由。

## 使用方式對照

| 情境 | 變更前 | 變更後 |
|------|--------|--------|
| 長期聊天 scope 的第 2 回合起 | 不寫入記憶 | 每回合寫入 |
| 一般追問（「那剛剛那個呢」） | 最後一棒收到最近 5 筆全文 | 最後一棒靠 session；輔助棒收到上一回合 |
| 20 回合前討論過的細節 | 幾乎召回不到（只有第一回合在庫） | 關鍵字或語意相符時召回 |
| 單一 scope 摘要失敗 | 本輪其他 scope 全部跳過 | 只跳過該 scope |

## 檢查清單

- [ ] 每個 production 改動前執行 `gitnexus_impact` 並回報影響範圍
- [ ] 紅燈原因與目標行為相關，期望值不由實作重算
- [ ] 不改 `RecallMode::Hybrid` 的既有行為與 schema
- [ ] 提交前 `gitnexus_detect_changes()`
