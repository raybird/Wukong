# Issue 5 - 容器內 opencode session 保留期設定一致性

## 概述

[GitHub issue #5](https://github.com/raybird/Wukong/issues/5) 指出兩份 Compose 只將 `WUKONG_OPENCODE_SESSION_RETENTION_DAYS` 傳給 schedulerd。Web／Telegram 容器執行 `wukong opencode prune` 時仍使用預設 30 天，可能忽略 `.env` 的停用或自訂天數。

2026-10-03 以 PyYAML 解析兩份 Compose 的服務環境設定，確認 Web／Telegram 均缺少此變數，schedulerd 均為 `${WUKONG_OPENCODE_SESSION_RETENTION_DAYS-30}`。既有 `scripts/test-docker-runtime.sh` 只檢查整份 Compose 是否含有此字串，無法發現個別服務遺漏。

2026-10-03 使用者核准補齊環境設定與回歸檢查。PR #4 已合併，交付基準為 `198e09235244df7d7b36b208a0393e3af8f6c350`（main）。

## 涉及檔案

- `docker-compose.yml`：補齊 Web／Telegram 的環境設定。
- `docker-compose.release.yml`：同步 release 部署設定。
- `scripts/test-docker-runtime.sh`：逐服務解析 YAML，檢查環境變數值。
- 本 README：驗收、任務與證據。

## 驗收條件

1. **AC-1**：兩份 Compose 的 Web、Telegram、schedulerd 都傳入 `${WUKONG_OPENCODE_SESSION_RETENTION_DAYS-30}`；未設定時使用 30，自訂天數與 `0` 停用值一致傳入，空字串仍保留既有停用語意。檢查方式：逐服務 YAML 檢查及 Compose 展開設定，覆蓋未設定、`0`、`7`、空字串。
   - 失敗路徑：任一服務缺少變數、寫死值或使用 `:-30`，均判定失敗。
2. **AC-2**：`scripts/test-docker-runtime.sh` 對上述六個服務設定逐一檢查；移除其中任何一行或修改為不一致值，測試會失敗。檢查方式：先取得實際配置遺漏的紅燈，再取得最小修正後綠燈，並對每個服務執行獨立暫存副本的負向驗證。
   - 失敗路徑：整份 Compose 存在 schedulerd 設定但 Web／Telegram 遺漏時，測試不得通過。

- **核准日期**：2026-10-03。
- **核准來源**：2026-10-03 使用者在建議方案後回覆「我已經在 github 合併好了，可以核准 issue5」，核准補齊 Web／Telegram 環境設定與回歸檢查。
- **核准 commit**：待提交。

## 風險與首要驗證

- **最大風險**：手動 prune 忽略停用設定而刪除 session 歷史。
- **風險等級與理由**：High；問題涉及資料刪除，需確認設定確實傳入正確服務。
- **首要驗證**：解析真實 Compose 的服務環境設定；核准後以回歸檢查取得遺漏設定的紅燈，再檢查 Compose 展開值。
- **選擇理由**：問題在環境變數傳遞，配置產物檢查可直接驗證，不需實際執行刪除。
- **完成證據**：六個服務的變數及展開值一致，逐服務負向驗證確實失敗；保留既有預設及空字串語意。

## 實作與驗證步驟

1. 📝 **AC-1、AC-2：重現服務設定遺漏** — 產出逐服務回歸檢查；相依：規格核准提交。完成判準：測試因 Web／Telegram 缺少設定失敗。
2. 📝 **AC-1：補齊設定** — 產出最小 Compose 修改；相依：步驟 1。完成判準：測試通過且四組 Compose 展開值一致。
3. 📝 **AC-2：驗證與交付** — 產出逐服務負向驗證、範圍精煉及獨立審查證據；相依：步驟 2。完成判準：最終檢查通過、PR 範圍只包含本 issue，獨立 review 持久化。

## Timeline

| 日期 | 異動 | 負責人 |
|---|---|---|
| 2026-10-03 | 依 `/dev-cycle issue:5` 建立待核准規格，確認配置遺漏與 PR #4 尚未合併 | Codex |
| 2026-10-03 | 使用者核准補齊設定與回歸檢查；確認 PR #4 已合併，採 main 為基準 | 使用者／Codex |

---
**建立日期**: 2026-10-03  
**分級**: Small — 環境設定微調與局部配置檢查，無跨服務程式邏輯變更  
**風險**: High  
**狀態**: 待實作
