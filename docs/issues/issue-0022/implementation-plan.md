# Issue 22 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)；現況見 README「已確認證據」。

## 設計
- **列出排程 scope（`wukong-memory`）**：新增 `Memory::scopes_with_prefix(prefix)`，回傳 `memories`、`sessions`、`agent_sessions`、`agent_session_state` 四張表中，以 `prefix` 開頭的不重複 scope。
  - 用 `substr(scope, 1, length(?1)) = ?1` 比對：區分大小寫，也沒有萬用字元。
  - 空字串回 `InvalidScope`。
- **清孤兒 scope（`wukong-scheduler` executor）**：新增 `clear_orphan_turn_scopes(store, memory) -> Result<u64>`。
  - 對每個 `agent:schedule-` 開頭的 scope，後綴必須能整段解析成 UUID。
  - 解析成功、而且 `get_job` 查不到時，才呼叫 `delete_scope`。
  - 回傳清掉的 scope 數。`get_job` 出錯就整個回錯誤，不刪任何東西（不確定就不刪）。
- **開始執行時推進（store）**：新增 `advance_next_run(job_id, worker_id, cron, now)`：
  - `UPDATE scheduler_jobs SET next_run_at = next_after(cron, now) WHERE id = ? AND locked_by = worker_id`；
  - 只在自己持有 lease 時才推進。
- **`run_claimed_job`**：在 `start_run` 之後呼叫 `advance_next_run`。回合正常完成時，`complete_claimed_job` 照原本的方式再推進一次，不變。
- **daemon**：
  - `run_scan` 開頭呼叫 `interrupt_stale_runs(now)`，數量大於 0 時印出來。
  - 啟動時，在 `recover_interrupted_runs` 旁呼叫 `clear_orphan_turn_scopes`；和它一樣，`--once` 不執行。
- **不做**：定期清孤兒 scope；回合總時間上限；逾時後接續。

## 實作步驟

所有步驟都要等 SCN-001～005 核准（TBD-AC）後才開始。

1. ✅ **首要驗證：只清排程不存在的 scope**（README 首要驗證；SCN-005 的資料層）
   - 產出：
     - `scopes_with_prefix` 測試：資料包含大小寫不同、含 `%`／`_` 的 scope，期望值手寫；
     - `clear_orphan_turn_scopes` 測試：SCN-005 那五種 scope，外加 `AGENT:SCHEDULE-<uuid>`，每個都有記憶與 session 指向，逐 scope 比對前後。
   - 相依：TBD-AC。
   - 完成判準：
     - 先用 stub 取得斷言失敗的紅燈，實作後轉綠；
     - 用變異確認：不檢查 UUID、改成不分大小寫比對、`get_job` 出錯時當成不存在，都會讓測試紅燈。
   - 結果（2026-10-21）：
     - 測試：
       - memory：`scopes_with_prefix_matches_case_sensitively_without_wildcards`。四張表各放一個只出現在該表的 scope，另有出現在多張表的、大小寫不同的、`_`／`%` 被當成萬用字元時才會符合的 scope，期望值手寫。
       - executor：`clearing_orphan_scopes_keeps_live_and_non_uuid_scopes`、`clearing_orphan_scopes_deletes_nothing_when_jobs_cannot_be_read`。後者另開連線刪掉 `scheduler_jobs` 表，讓查詢出錯。
     - 調整：`AGENT:SCHEDULE-<uuid>` 從 executor 測試拿掉，因為 `remember` 會以 `InvalidScope` 拒絕大寫的 scope 種類，正式路徑寫不進去；大小寫改由 memory 測試直接寫資料表來驗。
     - 紅燈（stub）：
       - memory 回空清單：`left: []`；
       - executor 清掉 0 個（預期 1）；
       - 查詢出錯時回 `Ok`（預期錯誤）。
     - 實作：
       - `scopes_with_prefix` 用四張表的 `UNION` 加上 `substr(scope, 1, length(?1)) = ?1`；
       - `clear_orphan_turn_scopes` 先把所有候選的排程都查完，查詢全部成功才開始刪。
     - 綠燈：memory 82 支、scheduler 45 支全過。
     - 變異（還原後皆確認）：
       - M1 不檢查 UUID：清孤兒測試紅燈；
       - M2 `get_job` 出錯當成不存在：查詢出錯的測試紅燈；
       - M3 改成 `LIKE ?1 || '%'`：memory 測試紅燈；
       - M4 少查 `agent_session_state`：memory 測試紅燈。
2. ✅ **紅燈**（SCN-001～004）
   - 產出：
     - executor 測試，SCN-001：開始執行之後，模擬行程結束（只取得、`start_run`，不完成），lease 到期後 `claim_due_jobs` 不再取得它，`next_run_at` 是開始時間之後的下一個時間點；
     - executor 測試，SCN-002：取得但沒開始，lease 到期後照常取得；
     - SCN-003 的 characterization；
     - daemon 測試，SCN-004：`run_scan` 在兩種 lease 狀態下的標記結果。如果 `run_scan` 依賴具體的 backend 而不好測，就抽出掃描前的標記步驟來測，真實效果由步驟 4 驗證。
   - 相依：步驟 1。
   - 完成判準：SCN-001、SCN-004 在現行程式碼下以斷言失敗；SCN-002、SCN-003 在現行程式碼下就是綠的。
   - 產出（2026-10-21）：
     - 測試要讓排程以實際時間到期（`run_claimed_job` 內部用實際時間），所以直接把 `next_run_at` 改到 5 秒前。cron 用每年一次的 `0 0 1 1 *`，「開始之後的下一個時間點」不會因為測試跨分鐘而變動。
     - 「行程中途結束」用 `tokio::time::timeout` 丟掉 `run_claimed_job` 的 future 來模擬：不會 `finish_run`，也不再續租。
     - executor：
       - `a_turn_interrupted_after_it_started_is_not_run_again`（SCN-001）；
       - `a_claimed_turn_that_never_started_is_claimed_again`（SCN-002）；
       - `a_completed_turn_is_rescheduled_from_its_finish_time`、`manually_triggering_a_job_that_is_not_due_keeps_its_next_run`（SCN-003）。
     - daemon：
       - `each_scan_interrupts_runs_whose_lease_expired`（SCN-004）：用連不上的 backend 直接呼叫 `run_scan`。它在取得排程時回錯誤，所以能驗證標記發生在取得之前，不必真的跑回合。
       - `orphan_scopes_are_cleared_only_for_daemon_mode`（SCN-005 的啟動部分）。
   - 紅燈：
     - SCN-001：lease 到期後 `claim_due_jobs` 又取得同一個排程；
     - SCN-004：`left: (Running, "")`；
     - 啟動清理（stub）清掉 0 個，預期 1。
     - SCN-002、SCN-003 在現行程式碼下就是綠的。
3. ✅ **最小實作**
   - 產出：依「設計」實作。修改前對 `run_claimed_job`、`run_scan`、`claim_due_jobs` 跑 impact 分析。
   - 相依：步驟 2。
   - 完成判準：
     - 步驟 1、2 都綠，受影響 crate 的測試全綠；
     - 用變異確認：拿掉推進、改成取得時推進、拿掉掃描時的標記，各自讓對應的測試紅燈。
   - 影響分析（2026-10-21）：
     - `run_claimed_job` 為 HIGH，直接呼叫端是 `run_scan` 與 CLI 的 `trigger_job`。只在 `start_run` 之後加一行推進，而且推進以 `locked_by = worker_id` 為條件。
     - `run_scan` 為 LOW，呼叫端只有 daemon 的主迴圈。
   - 實作：
     - store 新增 `advance_next_run`；
     - `run_claimed_job` 在 `start_run` 之後呼叫它；
     - `run_scan` 開頭呼叫 `interrupt_stale_runs`，放在取得排程之前，backend 還沒準備好時也會標記；
     - daemon 啟動時呼叫 `clear_orphan_scopes`（`--once` 不做；失敗只印警告，不阻擋 daemon 啟動）。
   - 綠燈：memory 105、scheduler 50、schedulerd 27、cli 34、web 77 支全過。clippy 通過。
   - 變異（還原後皆確認）：
     - M5 拿掉推進：SCN-001 紅燈。
     - M6 改成取得時推進（`claim_due_jobs` 順便把 `next_run_at` 設到一年後）：SCN-002、SCN-001 與兩支既有的 store 測試紅燈。
     - M8 推進過頭（多算一年）：SCN-001 與手動 trigger 測試紅燈。
     - M7 掃描時的標記：
       - 第一次的變異把 `now` 換成 0，測試沒紅。原因是當時的測試資料用的是沒有 lease 的排程，不論 `now` 是多少都會被標記，測不出差別。改成「600 秒前取得、lease 300 秒」的真實到期 lease。
       - 第二次替換時，字串在檔案裡出現兩次，沒有套用上，那次結果不算數。改用唯一的前後文後重跑：M7a（`now` 換成 0）、M7b（拿掉標記）、M7c（標記移到取得排程之後）都讓 SCN-004 紅燈。
     - M10 `--once` 也清：啟動清理測試紅燈。
     - 推進的 `locked_by` 條件沒有測試守著：`run_claimed_job` 在 `start_run` 前已經以續租確認自己持有 lease，這個條件只是額外的防線。
4. ✅ **真實行程驗證**（SCN-001／004／005）
   - 產出：在臨時目錄跑 main 與本分支的 binary，做法：
     - 假 agent 睡 30 秒；
     - daemon `--lease-secs 10` 開始執行後 `kill -9`，立刻重啟，並記錄這之後 30 秒內的情況；
     - 另外準備一個排程已不存在的 scope，確認重啟時被清掉。
   - 相依：步驟 3。
   - 完成判準：
     - main 會重跑同一次，舊 run 一直停在 running；
     - 本分支不重跑，舊 run 在 lease 到期後的那一輪掃描標成 interrupted，孤兒 scope 被清掉、其他 scope 不變。
   - 腳本：scratchpad 的 `repro22/run.sh`、`agent.sh`。main 的 binary 由 `c08c03d` 在 `~/.cache` 的 worktree 建置。
     - 排程用 `0 9 1 1 *`，`next_run_at` 設成 0。另外寫入一個孤兒 scope 和一筆擁有者記憶。
     - daemon 以 `setsid` 啟動（`--lease-secs 10 --tick-secs 2`）。agent 開始執行後 1 秒，用 `/bin/kill -KILL -- -<pgid>` 殺掉整個行程群組，立刻重啟，觀察 25 秒。
   - 第一次執行無效：dash 內建的 `kill` 不接受 `-- -<pgid>`，daemon 1 沒有被殺掉，持續續租。事後以 `pgrep -af` 逐一確認 PID，只停掉測試啟動的四個 daemon；容器內的 `wukong-schedulerd`（PID 5415、5651）沒有動。改用 `/bin/kill` 後重跑。
   - 結果（2026-10-21）：

     | 項目 | main `c08c03d`（`before/`） | 本分支（`after/`） |
     |---|---|---|
     | 殺掉後的下次執行時間 | 0（沒推進） | 1798794000（明年 1/1，開始執行時就推進） |
     | 重啟後 25 秒內 | 又取得同一次排程，新增 run 2；因為當掉的回合還占著 session lease，在「規劃中失敗」，記成 failure | 沒有再取得 |
     | 舊 run 1 | 一直是 running（`進行中：第 1 棒 oracle，已執行 0 秒`） | `interrupted`，訊息是 `scheduler process ended before the run completed`；daemon 2 印 `marked 1 stale scheduler run(s) as interrupted` |
     | 孤兒 scope 記憶／指向 | 1／1 | 0／0（daemon 1 啟動時就清掉） |
     | 擁有者記憶 | 1 | 1 |
     | agent 啟動次數 | 1 | 1 |

     main 這次沒有真的再跑 agent，是因為 session lease（預設 900 秒）還在。重啟發生在 900 秒之後，就會從頭重跑。
5. ✅ **常青文件與交付**
   - 產出：
     - `AGENTS.md` 的「lease 在執行中續租」條目：刪掉「不處理的部分」中已經處理的兩項，補上新行為；「排程能力注入」條目更新當掉後 scope 殘留的說明；
     - `CHANGELOG.md`；
     - `docs/issues/issue-0013/`、`issue-0017/` 的 TBD-1 標註由 #22 處理。
   - 全量 fmt、clippy、`cargo test --workspace --locked --no-fail-fast`，以及 `gitnexus_detect_changes`。
   - 相依：步驟 1～4。
   - 文件：
     - `AGENTS.md`：「lease 在執行中續租」條目以新行為取代「不處理的部分」中已處理的兩項，並補上「當掉的回合留下的 session lease 最多占用 900 秒」；「排程能力注入」條目改寫當掉後 scope 的清理方式。
     - `CHANGELOG.md` 新增一條 Changed。
     - `issue-0013`、`issue-0017` 的 TBD-1 標註由 #22 處理。
   - 全量檢查（2026-10-21）：fmt、clippy 通過；`cargo test --workspace --locked --no-fail-fast` passed=675 failed=0 ignored=12。main 是 666，本次新增 9 支測試：memory 1、executor 6、daemon 2。
   - `gitnexus_detect_changes`（risk=critical）：diff 除了 `scheduled_turn_scope` 改用常數那一行，其餘都是新增。列出的 `remember`、`interrupt_stale_runs`、`claim_ready_jobs`、`run_unattended_turn` 等，只是和新程式碼相鄰，本體沒有改。
   - code-simplify：`clear_orphan_turn_scopes` 與 #17 的 `clear_scope_if_deleted` 都是「排程不在就清」，但前者要先確認所有候選、查詢出錯就整批不刪，後者只處理單一排程、出錯只記 log，失效方向不同，不合併。no-op。

6. ✅ **審查修正**（`review-1d65567.md`）
   - 相依：步驟 1～5；D-1、D-2 由使用者決定（README 待確認事項）。
   - 影響分析（2026-10-21）：`run_claimed_job` 仍是 HIGH（呼叫端 `run_scan`、CLI `trigger_job`），這次只在推進前加上排程種類的條件；`recover_interrupted_runs` 為 LOW，唯一呼叫端是 daemon 的 `run`；`clear_orphan_turn_scopes` 是本 issue 新增的，唯一呼叫端是 daemon 的 `clear_orphan_scopes`。
   - 修正：
     - M-1：`clear_orphan_turn_scopes` 只把小寫、有連字號的 UUID 當成排程 id（`Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == id)`）。大寫、`{...}`、`urn:uuid:`、不含連字號都能解析成 UUID，但 `get_job` 是精確比對，一定查不到，原本會被當成孤兒刪掉。
     - S-2／D-2：`run_claimed_job` 只對 `JobKind::Turn` 呼叫 `advance_next_run`。
     - S-1／D-1：刪掉 daemon 啟動時的 `recover_interrupted_runs` 與測試 `startup_recovery_runs_only_for_daemon_mode`；`run_scan` 每輪的標記在常駐模式第一輪（啟動時立刻執行）與 `--once` 都會做。
     - S-3：把 `clearing_orphan_scopes_deletes_nothing_when_jobs_cannot_be_read`（刪掉整張表）換成 `clearing_orphan_scopes_deletes_nothing_when_a_job_cannot_be_read`：排在前面的是真正的孤兒，排在後面的排程列被改成種類對不上，讓 `get_job` 出錯；斷言兩個 scope 都沒被刪。
     - N-1：`scheduled_turn_scope` 的 doc comment 移回函式上。N-4：CHANGELOG 範圍改寫，`issue-0017` 的 TBD-4 標註由 #22 處理。N-5：AGENTS.md 補上兩個執行者同時在跑時的中途顯示。
     - 不處理：N-2（推進失敗時 run 要等 lease 到期才被標記，訊息寫成行程結束）、N-3（標記出錯時整輪不取得排程）。兩者都是資料庫出錯時才會發生，和 `start_run` 失敗時的既有模式相同。
   - 紅燈（修正前）：
     - `clearing_orphan_scopes_keeps_live_and_non_uuid_scopes` 加入存在中排程 id 的四種非正規寫法：`left: 5, right: 1`。
     - 新測試 `an_interrupted_maintenance_job_is_claimed_again`：維護排程（prune）開始後，由另一條連線拿住記憶庫的寫入鎖讓它卡住，再以 `tokio::time::timeout` 丟掉 future；斷言 run 停在 running 之後，lease 到期再取得：`left: 0, right: 1`。
     - S-3 的新測試在修正前就是綠的（守著既有行為），用變異 ME 確認。
   - 綠燈：scheduler lib 50、schedulerd 24 支全過。
   - 變異（還原後皆確認，每次都確認是哪個斷言紅的）：
     - M2 `get_job` 出錯當成不存在：新的查詢出錯測試在 `is_err()` 斷言紅燈；
     - ME 邊查邊刪：同一支測試在「前面那個孤兒仍在」的斷言紅燈；
     - MS 推進條件改成只對維護排程：維護排程測試（再取得 0 個）與 SCN-001（lease 到期後又取得）紅燈。
   - 全量檢查（2026-10-21）：fmt、clippy 通過；`cargo test --workspace --locked --no-fail-fast` passed=675 failed=0 ignored=12（新增維護排程測試、刪掉啟動檢查測試，數量不變）。

## 驗收對照
| Scenario | 步驟 | 證據 |
|---|---|---|
| SCN-001 | 2、3、4、6 | executor 測試；真實行程；維護排程不推進 |
| SCN-002 | 2、3 | executor 測試（characterization） |
| SCN-003 | 2、3 | executor 測試（characterization） |
| SCN-004 | 2、3、4、6 | daemon 測試；真實行程 |
| SCN-005 | 1、3、4、6 | memory 與 executor 測試；真實行程 |
