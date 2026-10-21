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

1. ✅ **首要驗證：只刪到目標 scope**（README 首要驗證；SCN-001 的資料層）
   - 產出：`wukong-memory` 的 `delete_scope` 測試，期望值手寫。
     - 資料：目標 scope、擁有者 scope、另一個排程的 scope，以及名稱以目標 scope 開頭的 scope（`agent:schedule-<id>-x`）。每個 scope 都有 event、note、summary 和 session 指向。
     - 斷言：目標 scope 的列、FTS 命中與 session 指向都消失；其他三個 scope 逐列不變；空字串 scope 回錯誤。
   - 相依：TBD-AC。完成判準：先用 stub 取得斷言失敗的紅燈，實作後轉綠。
   - 結果（2026-10-21）：
     - 測試：`crates/wukong-memory/src/store/mod.rs` 的 `delete_scope_removes_only_that_scope` 與 `delete_scope_rejects_an_empty_scope`。
       - 每個 scope 寫入 event、note、summary 各一筆，外加 `sessions`、`agent_sessions`、`agent_session_state` 各一列。
       - 逐 scope 比對五個數字：記憶、FTS 命中、`sessions`、`agent_sessions`、`agent_session_state`。另外直接數 FTS 表的列數，確認 FTS 的列真的刪掉了，而不只是 JOIN 不到。
     - 紅燈（stub 什麼都不刪）：一支在 `delete_scope` 回傳值的斷言失敗，回 0、預期 3；另一支是空 scope 沒有回錯誤。前置斷言（每個 scope `[3, 3, 1, 1, 1]`、FTS 共 12 列）都通過，所以失敗確實來自刪除本身。
     - 實作：在一個 transaction 裡刪 `memories`、`sessions`、`agent_sessions`、`agent_session_state`，都用 `scope = ?1` 比對；空字串回 `InvalidScope`。`remember` 會為每一回合在 `sessions` 寫一列並記下 scope，所以這張表也一起清。
     - 綠燈：memory 81 支測試全過。
     - 變異：把記憶的刪除改成前綴比對（`LIKE ?1 || '%'`），測試紅燈：刪了 6 筆，連 `agent:schedule-job1-x` 也被刪掉。
2. ✅ **紅燈**（SCN-001～006）
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
   - 產出（2026-10-21）：
     - store：`remove_job_reports_whether_the_job_was_held`。測四種情況：被持有、lease 已到期、沒被取得、不存在。
     - executor：
       - `deleting_an_idle_turn_job_clears_only_its_scope`（SCN-001）：同時有 `-x` 那個 scope 與擁有者 scope。
       - `deleting_a_running_job_cleans_up_after_the_turn`（SCN-003）：`SlowBackend` 每次呼叫睡 1 秒，第 0.3 秒刪除。
       - `deleting_a_queued_job_skips_it_and_clears_its_scope`（SCN-005）：另外確認被持有時不會先清。
       - `deleting_a_maintenance_job_or_a_missing_id`（SCN-006）。
     - CLI：`schedule_rm_reports_the_delete_outcome`。
     - Web：`deleting_a_schedule_clears_its_scope`（SCN-002）。
     - daemon：`deleted_job_output_is_still_delivered`（SCN-004）。`run_scan` 用的是具體的 Telegram client 與 backend，無法直接測，所以把「哪些結果要送通知」抽成 `deliverable_output`。真的有沒有送出，由步驟 4 的真實行程驗證。
   - 紅燈：stub 的結構和最終版本相同，但不清理，也不分辨刪除當下有沒有被持有。
     - store：被持有的 job 回 `Some(false)`。
     - SCN-001：scope 還剩 `(1, Some(ses-…))`。
     - SCN-003、SCN-005：刪除結果是 `Deleted`，預期 `DeletedWhileHeld`。
     - CLI：被持有時的訊息只有「已刪除排程」。
     - Web：scope 還剩 `(1, Some("ses-1"))`。
     - daemon：`Deleted` 回 `None`，不送通知。
     - SCN-006 在 stub 下就是綠的，作為 characterization。
3. ✅ **最小實作**
   - 先對 `remove_job`、`run_claimed_job`、`run_scan`、`delete_schedule`、`run_schedule_op` 跑 `gitnexus_impact`，回報風險等級。
   - 相依：步驟 2。完成判準：步驟 2 轉綠，受影響 crate 的測試全綠。
   - 影響分析（2026-10-21）：
     - `remove_job` 為 HIGH，直接呼叫端只有 CLI 的 `run_schedule_op` 與 Web 的 `delete_schedule`，兩者都改用 `delete_job`。
     - `run_claimed_job` 為 HIGH，呼叫端是 `run_scan` 與 CLI 的 `trigger_job`。只在排程已經不在的兩條路徑加上清理，排程還在時行為不變（SCN-006 與既有測試）。
     - `delete_schedule` 為 LOW。
   - 實作：
     - `remove_job` 改成 `DELETE … RETURNING locked_by IS NOT NULL AND COALESCE(locked_until, 0) > now`。
     - `delete_job`：被持有就回 `DeletedWhileHeld`，否則呼叫 `delete_scope`。
     - `clear_scope_if_deleted` 用 `get_job` 判斷排程是否還在。`run_claimed_job` 在 `NotHeld` 與 `complete_claimed_job` 回 false 時呼叫它，後者改回 `Deleted(output)`。清理失敗只記 `scheduler_scope_cleanup_failed`，結果照常回報。
     - `SchedulerError` 新增 `Memory` variant。
     - CLI 的 trigger 把 `Deleted` 和 `Completed` 一樣處理。
     - daemon 的 `deliverable_output` 對 `Deleted` 印出被刪除的訊息，然後照常記錄並送通知。
   - 綠燈：memory、scheduler、cli、web、schedulerd 全部測試通過。
   - 變異（還原後皆確認）：
     - 拿掉回合結束後的清理：SCN-003 紅燈。
     - 拿掉排隊跳過時的清理：SCN-005 紅燈。
     - 被持有時也立刻清：SCN-003 與 SCN-005 都紅燈。
       - 2026-10-21 依審查更正：SCN-003 紅燈只是因為刪除結果從 `DeletedWhileHeld` 變成了 `Deleted`，斷言對不上，並不是回合失敗。審查的變異 M10（照常回 `DeletedWhileHeld`，但也立刻清）下，SCN-003 全綠，回合照樣成功，見步驟 6。
4. ✅ **真實行程驗證**（SCN-003／004）
   - 在臨時目錄用假 agent 建一個排程。假 agent 在回合中途執行 `wukong schedule rm --id <自己的 id>`，再睡幾秒後結束。
   - 由 daemon 執行，記錄以下結果：
     - daemon 的輸出：被刪除的訊息、送出通知的嘗試；
     - 回合結束後 `agent:schedule-<id>` 在 `memories`、`agent_sessions`、`agent_session_state` 的筆數；
     - 擁有者 scope 的筆數有沒有變。
   - 相依：步驟 3。完成判準：回合成功、scope 三張表都是 0、擁有者 scope 不變。
   - 腳本：scratchpad 的 `repro17/run.sh`、`agent.sh`。
     - 事先替 scope 寫入既有的 summary 與 `agent_sessions` 指向，擁有者 scope 寫一筆 summary。
     - `WUKONG_TG_TOKEN` 設成假值，`HTTPS_PROXY` 指向沒人聽的本機 port，這樣送 Telegram 會失敗並印出 warning，證明有嘗試送出，又不會碰外部網路。
     - 假 agent 在執行棒裡呼叫 `wukong schedule rm --id <自己>`，再睡 3 秒。daemon 用 `--once --lease-secs 30` 執行。
   - 結果（2026-10-21），欄位依序是 scope 記憶、`agent_session_state`、`agent_sessions`、擁有者記憶、jobs：

     | 時間點 | main（`before/`） | 本分支（`after/`） |
     |---|---|---|
     | 開始前 | 1／0／1／1／1 | 1／0／1／1／1 |
     | 第 2 秒（已 rm） | 1／1／1／1／0 | 1／1／1／1／0 |
     | 結束後 | 3／1／1／1／0 | 0／0／0／1／0 |
     | rm 的輸出 | 已刪除排程 | 已刪除排程；排程正在執行，回合結束後才會清掉它的記憶與 session |
     | daemon | `lease was taken by another worker before completion`，沒有送通知 | `was deleted while running; cleared its scope`、`succeeded`、`telegram delivery … failed`（proxy 擋下） |

     第 2 秒時，排程已經不在了，但 scope 還沒清（SCN-003）；回合結束後才清空。兩邊結束後的 trigger 都印「找不到排程」。
5. ✅ **常青文件與交付**
   - 根目錄 `AGENTS.md` 的「排程能力注入」：補上刪除排程會清掉它的 scope，以及執行中刪除的處理方式。
   - `docs/cli-reference.md` 的 `schedule rm`。
   - `CHANGELOG.md` 的 `[Unreleased]`。
   - 全量 fmt、clippy、`cargo test --workspace --locked --no-fail-fast`，以及 `gitnexus_detect_changes`。
   - 相依：步驟 1～4。
   - 文件：
     - `AGENTS.md` 的「排程能力注入」補上刪除流程與執行中的處理。
     - `docs/cli-reference.md` 的 `schedule rm`。
     - `CHANGELOG.md` 新增一條 Changed，並刪掉 #14 條目裡「刪除排程後不會被清理（#17）」那句已知限制。
   - 全量檢查（2026-10-21）：fmt、clippy 通過；`cargo test --workspace --locked --no-fail-fast` passed=665 failed=0 ignored=12。main 是 655，本次新增 10 支測試。
   - `gitnexus_detect_changes`（risk=critical）：正式程式碼改動的是：
     - 新函式：`Store::delete_scope`、`Memory::delete_scope`、`delete_job`、`clear_scope_if_deleted`、`deliverable_output`；
     - 修改：`remove_job`、`run_claimed_job`、`run_schedule_op`／`trigger_job`、`delete_schedule`、`run_scan`。

     列出的 `Memory::remember`、`set_enabled`、`snapshot`、`due_job` 等只是位置相鄰；`git diff` 確認 `memory/src/lib.rs` 只有新增，`store.rs` 只改了 `remove_job` 與測試。
   - code-simplify：清理邏輯只有 `delete_scope` 一處；executor 的兩條事後清理共用 `clear_scope_if_deleted`。no-op。

6. ✅ **審查修正**（PR #21 審查 `review-94a5682.md`，PASS，2026-10-21）
   - **SHOULD FIX 1：延後清理的理由寫錯了**。原本寫「執行中先清，`finish_agent_session_turn` 會失敗」，但回合結束前的 `acquire_agent_session_lease` 會重建 session 狀態列，所以先清不會讓回合失敗。審查的變異 M10 證實了這一點。
     - 真正的理由是：回合結束時才寫入記憶、重建 session 狀態，提早清也會被寫回去。
     - 已更正 `AGENTS.md`、`delete_job` 的註解、README「已確認證據」，以及步驟 3、步驟 4 的描述。延後清理本身是使用者 Q4 核准的行為，不變。
   - **D-1**：審查建議可以改成「被持有時也先清一次，回合結束後再清一次」，讓 TBD-1 的殘留只剩當回合寫入的部分。使用者 2026-10-21 選維持現狀。
   - **SHOULD FIX 2：兩條路徑沒有守門**。
     - M11（排程還在時照樣清 scope）：在 SCN-008 的 `queued_job_whose_lease_was_taken_is_skipped` 裡，先為 scope 寫入資料，並斷言被別人取得時 scope 不變。M11 下這支測試紅燈。
     - M9（CLI trigger 收到 `Deleted` 時改回錯誤）：把 trigger 的結果判斷抽成 `trigger_report`，回傳要印的訊息與錯誤，行為不變；新增 `trigger_reports_a_turn_deleted_while_running`。M9 下這支測試紅燈。
     - 審查另外用真實行程補跑了 trigger：本分支 exit 0、印出「提醒完成」、scope 歸零；main 是 exit 1，回報 lease 被接手。紀錄在 scratchpad 的 `rev17/`。
   - 全量檢查（2026-10-21）：fmt、clippy 通過；`cargo test --workspace --locked --no-fail-fast` passed=666 failed=0 ignored=12，比步驟 5 多 1 支 trigger 測試。SCN-008 那支是在原測試裡加斷言，不算新測試。
   - `gitnexus_detect_changes`（risk=medium）：正式程式碼只動到 CLI 的 `trigger_job`（抽出 `trigger_report`）與 `delete_job` 的註解。列出的 `run_claimed_job`、`describe_job_kind` 等只是位置相鄰。
   - NICE TO HAVE 不處理，只記錄：
     - Web 刪除執行中的排程沒有回應碼測試；
     - `Deleted` 的註解寫「已清掉」，但清理失敗時只記 log；
     - heartbeat 在排程被刪除後會印 `lease_renew_lost`；
     - `WUKONG_MD_DIR` 的 markdown 鏡像不會刪。

## 驗收對照
| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | 單元（memory、scheduler、CLI） | 步驟 1、2／3 |
| SCN-002 | 單元（Web） | 步驟 2／3 |
| SCN-003 | 單元（scheduler、CLI）＋真實行程 | 步驟 2／3、4 |
| SCN-004 | 單元（daemon）＋真實行程 | 步驟 2／3、4 |
| SCN-005 | 單元（scheduler） | 步驟 2／3 |
| SCN-006 | 單元（characterization） | 步驟 2／3 |
