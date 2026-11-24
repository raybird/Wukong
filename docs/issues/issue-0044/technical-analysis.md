# 技術分析

2026-11-24 建立。記錄澄清時做過取捨的三個決定，以及它們對設計的約束。現況見 [requirement-analysis.md](./requirement-analysis.md)。

## 方案選型與取捨

### 1. 只支援 V2，不保留 V1 adapter（Q1）

| 方案 | 好處 | 代價 |
|---|---|---|
| 只支援 V2（採用） | 一套 adapter、一套測試；程式碼與 #45 之後要依賴的 V2 功能一致 | 回復只能靠前一版映像加 opencode.db 備份 |
| V1、V2 並存可切換 | 出問題時改設定就能切回 | 兩套 adapter 都要維護與測試；遷移後切回 V1 仍看不到 V2 期間的對話，切換的實際價值有限 |

約束：PATH 上是 V1 時要明確報錯（SCN-011），因為不再有 V1 的程式路徑可以退回。

### 2. 固定 @opencode/cli 2.3.2（Q2）

| 方案 | 好處 | 代價 |
|---|---|---|
| 固定 2.3.2（採用） | 映像裡的版本就是 #30 實測過的版本；V2 小版本的變動不會不知不覺進來 | 之後升級 V2 小版本要另外處理 |
| CI 解析 latest | 不用手動升級 | V2 剛推出，Wukong 依賴的路徑與事件可能在小版本改變；#30 已遇過釘選檔與映像實際版本不符（地雷 W-1 同類） |

約束：`resolve-opencode-version.sh` 在沒有明確要求時不能把釘選改成 npm latest；SCN-012 要在 npm 上有更新版的情況下驗證。

### 3. installer 自動備份 opencode-state 並支援 rollback（Q3）

| 方案 | 好處 | 代價 |
|---|---|---|
| 自動備份並支援 rollback（採用） | 換版失敗或要退回時不必手動操作 docker volume；沿用既有的 `--rollback` 介面 | 多改 install.sh 與它的測試；備份要停服務才一致，升級多一段停機 |
| 標為不可逆、拒絕 rollback | 改動少 | 回復全靠人工操作 volume，容易出錯 |

約束：

- `data-compatibility.json` 宣告 `affectedState` 含 opencode-state、`backupRequired: true`；因為有資料備份，`irreversibleMigration` 維持 false，`rollbackSafeTo` 指向前一版。
- 備份前停止所有會開 opencode.db 的服務（web、telegram、schedulerd、opencode-server），避免複製到寫一半的 WAL；#30 的 snapshot.py 用 SQLite Backup API 是另一個做法，但服務仍在寫入時無法保證與升級時點一致。
- 備份路徑寫進 `.wukong-release` 的 metadata；啟用失敗（SCN-014）與 `--rollback`（SCN-015）都從這裡還原，還原前同樣先停服務。
- 接受備份之後的對話遺失，installer 輸出與 docs 都要寫明。

## 架構影響評估

- 只動 `wukong-gateway` 對外的 OpenCode 介面，`AiBackend` trait 對 runtime 的契約不變；`wukong-runtime`、`wukong-orchestrator`、`wukong-skills` 不動（#45 的範圍）。
- Telegram、Web 呼叫 gateway 的提問與權限回覆 API 時，如果 V2 的 form 需要不同的參數，呼叫端跟著改，但使用者看到的操作不變。
- 依賴方向維持 `wukong-cli → { memory, gateway, orchestrator }`、`orchestrator → gateway → memory`。

## 實作細節與待釐清項目

- 本機按需模式：每次啟動 `opencode serve` 前產生隨機密碼，經 `OPENCODE_SERVER_PASSWORD` 給子程序、同一組帶進 basic auth；使用者名稱固定 opencode，`WUKONG_AGENT_SERVER_USERNAME` 在 V2 不再有作用，docs 要說明。
- CLI backend：opencode 路徑加 `--standalone`，避免連到背景 service；`--format json` 的事件形狀與 V1 不同，要在 Phase 1 實測後再改 `stream.rs` 的解析。
- 自訂 `WUKONG_AGENT_CMD` 帶 V1 才有的旗標時的行為：TBD-1。
- server 模式的密碼來源：TBD-2，Phase 3 實作前決定。
