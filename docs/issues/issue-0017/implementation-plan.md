# Issue 17 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)；現況見 README「已確認證據」。

## 設計
- **刪除單一 scope（`wukong-memory`）**：新增 `Memory::delete_scope(scope)`，在一個 transaction 裡做三件事：
  - 刪掉 `memories` 中 `scope = ?1` 的列，由既有的 AFTER DELETE trigger 同步 FTS；
  - 刪掉 `agent_sessions` 的那一列；
  - 刪掉 `agent_session_state` 的那一列。

  只用等號比對 scope；空字串直接回錯誤，不執行刪除。回傳刪除的記憶筆數。
- **刪除 job（`wukong-scheduler` store）**：`remove_job` 改成 `DELETE … RETURNING`，同一句 SQL 回報刪除當下 job 是否正被持有（`locked_by IS NOT NULL AND locked_until > now`），避免「先檢查、再刪除」之間的競態。
- **共用的刪除函式（`wukong-scheduler` executor）**：新增 `delete_job(store, memory, id)`，回傳三種結果：
  - `NotFound`；
  - `Deleted`：當時沒有被持有，立刻呼叫 `delete_scope(scheduled_turn_scope(id))`；
  - `DeletedWhileHeld`：當時被持有，不清 scope。

  維護排程沒有寫入這個 scope，清理不會刪到任何東西，所以不分 job 類型。CLI 的 `schedule rm` 與 Web 的 `delete_schedule` 都改用這個函式。
- **事後清理（executor）**：`run_claimed_job` 在兩個時間點用 `get_job` 確認排程是否還在，不在就呼叫 `delete_scope`：
  - `NotHeld` 時：排隊中被刪除；
  - 回合結束、`complete_claimed_job` 回 false 時：執行中被刪除。

  回合結束時，如果排程已經不在了，就回傳新的 `ClaimedJobOutcome::Deleted(output)`；排程還在，才是原本的 `LeaseLost`。
- **daemon 與 CLI**：
  - daemon 遇到 `Deleted` 時，印出「job <id> was deleted while running」，然後照常送 Telegram 通知（SCN-004）。
  - CLI 的 trigger 遇到 `Deleted` 時，印出結果；成功時 exit 0，失敗時回錯誤。
  - `schedule rm` 依刪除結果印出訊息。
- **不做**：
  - 定期掃描孤兒 scope（使用者 Q1）；
  - 直接刪除 opencode session（使用者 Q3）；
  - 刪除擁有者聊天紀錄（TBD-5）。

## 實作步驟

所有步驟都要等 SCN-001～006 核准（TBD-AC）後才開始。

1. 📝 **首要驗證：只刪到目標 scope**（README 首要驗證；SCN-001 的資料層）
   - 產出：`wukong-memory` 的 `delete_scope` 測試，期望值手寫。
     - 資料：目標 scope、擁有者 scope、另一個排程的 scope，以及名稱以目標 scope 開頭的 scope（`agent:schedule-<id>-x`）。每個 scope 都有 event、note、summary 和 session 指向。
     - 斷言：目標 scope 的列、FTS 命中與 session 指向都消失；其他三個 scope 逐列不變；空字串 scope 回錯誤。
   - 相依：TBD-AC。完成判準：先用 stub 取得斷言失敗的紅燈，實作後轉綠。
2. 📝 **紅燈**（SCN-001～006）
   - scheduler store：`remove_job` 回報刪除當下是否被持有。
   - executor，用假 backend：
     - `delete_job` 的三種結果，以及立即清理（SCN-001）；
     - 執行中刪除：回合成功、回傳 `Deleted`、結束後 scope 清空，包含本回合寫入的記憶（SCN-003）；
     - 排隊中刪除：`NotHeld` 並清理（SCN-005）；
     - 維護排程與找不到的排程（SCN-006，characterization）。
   - CLI：`schedule rm` 的訊息（SCN-001／003／006）。
   - Web：`DELETE /api/schedules/<id>` 回 200，資料清空（SCN-002）。
   - daemon：`Deleted` 時照常送通知（SCN-004），用 `wukong-tg-client` 的 `mock` feature 或既有的通知測試方式。
   - 相依：步驟 1。完成判準：在現行程式碼下以斷言失敗，不是編譯失敗。
3. 📝 **最小實作**
   - 先對 `remove_job`、`run_claimed_job`、`run_scan`、`delete_schedule`、`run_schedule_op` 跑 `gitnexus_impact`，回報風險等級。
   - 相依：步驟 2。完成判準：步驟 2 轉綠，受影響 crate 的測試全綠。
4. 📝 **真實行程驗證**（SCN-003／004）
   - 在臨時目錄用假 agent 建一個排程。假 agent 在回合中途執行 `wukong schedule rm --id <自己的 id>`，再睡幾秒後結束。
   - 由 daemon 執行，記錄以下結果：
     - daemon 的輸出：被刪除的訊息、送出通知的嘗試；
     - 回合結束後 `agent:schedule-<id>` 在 `memories`、`agent_sessions`、`agent_session_state` 的筆數；
     - 擁有者 scope 的筆數有沒有變。
   - 相依：步驟 3。完成判準：回合成功、scope 三張表都是 0、擁有者 scope 不變。
5. 📝 **常青文件與交付**
   - 根目錄 `AGENTS.md` 的「排程能力注入」：補上刪除排程會清掉它的 scope，以及執行中刪除的處理方式。
   - `docs/cli-reference.md` 的 `schedule rm`。
   - `CHANGELOG.md` 的 `[Unreleased]`。
   - 全量 fmt、clippy、`cargo test --workspace --locked --no-fail-fast`，以及 `gitnexus_detect_changes`。
   - 相依：步驟 1～4。

## 驗收對照
| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | 單元（memory、scheduler、CLI） | 步驟 1、2／3 |
| SCN-002 | 單元（Web） | 步驟 2／3 |
| SCN-003 | 單元（scheduler、CLI）＋真實行程 | 步驟 2／3、4 |
| SCN-004 | 單元（daemon）＋真實行程 | 步驟 2／3、4 |
| SCN-005 | 單元（scheduler） | 步驟 2／3 |
| SCN-006 | 單元（characterization） | 步驟 2／3 |
