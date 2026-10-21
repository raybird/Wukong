# Issue 13 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)；現況見 README「已確認證據」。

## 設計
- **session lease 續租（`wukong-runtime`）**：`run_turn` 取得 session lease 後，把逐棒執行的 future 和一個計時器放在同一個 `select!` 裡，每隔 lease 時間的三分之一呼叫一次 `renew_agent_session_lease`。兩者在同一個 task：回合結束或行程中止，續租就停止。續租失敗只記一行 log，因為 lease 已經被別人拿走，回合結束時取得 lease 的檢查會照現行邏輯處理。壓縮期間也要續租。
- **job lease 續租與進度（`wukong-scheduler`）**：`run_claimed_job` 多收一個 lease 時間參數。回合執行時，同一個 task 每 30 秒做兩件事：
  - 續租 job lease（`locked_by` 必須是自己）。
  - 把目前階段與已執行秒數寫進 run 訊息（只更新 `status = 'running'` 的 run）。

  換階段時立刻更新一次。階段來自 `run_turn` 的 `on_role`：還沒收到任何 `on_role` 時是「規劃中」，收到第 k 次時是「第 k 棒 <角色>」。
- **失敗訊息**：`execute_job` 失敗時，在原錯誤前面加上「<階段>失敗，已執行 N 秒：」。
- **手動 trigger**：`claim_job` 拿不到 job 時，再查一次 job，區分不存在、已停用與正在執行（`locked_until > now`）。
- **不做**：重啟後立刻重跑、回合總上限、接續、成果驗證（TBD-1～3）。

## 實作步驟

所有步驟都要等 SCN-001～007 核准（TBD-AC）後才開始。

1. ✅ **首要驗證：在真實行程重現 lease 過期後的重複執行**（SCN-003、SCN-004）
   - 產出：一支重現腳本，放在 scratchpad，結果抄錄進本計畫。內容：
     - 臨時目錄、假 agent，每次呼叫睡 N 秒。
     - `WUKONG_SESSION_LEASE_SECS` 與 daemon 的 `--lease-secs` 都設成遠小於回合時間。
     - 用 `wukong-schedulerd --once` 執行排程，跑到 lease 過期後，另開行程 `wukong schedule trigger` 同一個排程。
     - 記錄兩邊的 exit code、run 紀錄、`agent_session_state`，以及假 agent 收到的 session。
   - 相依：TBD-AC。
   - 完成判準：現行版本至少重現下列其中一項，作為紅燈基線：trigger 重複執行、共用 session、原回合以 `session lease unavailable` 失敗。若都沒有重現，回 README 重新評估。
   - 結果（2026-10-21，`a7d8935` 的 debug binary；腳本在 scratchpad 的 `repro13/run.sh`、`agent.sh`）：
     - 設定：`WUKONG_SESSION_LEASE_SECS=4`、daemon `--lease-secs 2`，假 agent 每棒睡 8 秒。先用 trigger 暖身一次，讓排程 scope 存下 session `ses_2931387`。之後 daemon 開始執行，第 5 秒另開行程 trigger。
     - 第 5 秒的 lease：job 的 `locked_until` 是 t0+2，已經過期；session 的 `lease_until` 是 t0+4，也已經過期。
     - **重複執行（SCN-004 紅燈）**：trigger 取得 job 並執行一個完整回合，exit 0，run #3 為 success。daemon 印出 `lease was taken by another worker before completion`。
     - **共用 session（SCN-003 紅燈）**：假 agent 的紀錄顯示，daemon 那棒（第 0～8 秒）和 trigger 那棒（第 5～13 秒）都以 `resumed=ses_2931387` 執行，有 3 秒重疊。
     - **誤判失敗**：daemon 的 run #2 在第 8 秒失敗，原因是 `session lease unavailable for scope agent:schedule-…`。但它的回答 `done by daemon` 已經寫進記憶。
     - 結論：三項全部重現，設計前提成立。
2. ✅ **紅燈**
   - `wukong-memory`／`wukong-runtime`：用假 backend 讓一棒的執行時間超過 session lease 時間。執行中，從另一個 owner 嘗試取得同一個 scope 的 lease，預期拿不到；回合成功並寫回 session（SCN-003）。
   - `wukong-scheduler`：
     - job lease 很短、回合較長時，執行中 `claim_job` 拿不到 job，原回合完成後 `Completed`（SCN-004）。
     - 回合執行中，run 訊息出現「規劃中」與「第 k 棒 <角色>」以及已執行秒數（SCN-001）。
     - 在規劃階段與第 2 棒失敗時，訊息開頭標出階段（SCN-002）。
     - 短回合成功時，run 訊息等於最終輸出（SCN-006，characterization）。
   - `wukong-scheduler` store：區分 trigger 拿不到 job 的三種原因（SCN-007）。
   - 期望值手寫，不由被測函式產生。測試中的時間用很短的 lease 與可控制的假 backend，不睡到真實的 30 秒：進度更新間隔要能在測試中調短。
   - 相依：步驟 1。完成判準：在現行程式碼下以斷言失敗，不是編譯失敗。
   - 產出（2026-10-21）：
     - `crates/wukong-runtime/src/session.rs`：
       - `lease_stays_held_while_work_outlives_it`（SCN-003）：lease 2 秒、工作 3.5 秒，第 3 秒別的 owner 嘗試取得。
       - `lease_expires_once_the_work_is_dropped`（SCN-005）：工作被丟掉後，lease 照原本的時間到期。
     - `crates/wukong-scheduler/src/executor.rs`，用 `SlowBackend`，每次呼叫先睡一段時間再回覆：
       - `turn_longer_than_job_lease_keeps_the_job_claimed`（SCN-004）
       - `running_run_shows_phase_and_elapsed_seconds`（SCN-001）：在 0.3、1.3、2.5 秒讀 run 訊息。
       - `failure_message_names_the_planning_phase`、`failure_message_names_the_failing_step`（SCN-002）
       - `short_turn_result_and_leases_are_unchanged`（SCN-006）
     - `crates/wukong-scheduler/src/store.rs`：`renew_job_lease_extends_only_the_holders_lease`、`update_running_message_skips_finished_runs`。
     - `crates/wukong-cli/src/main.rs`：`trigger_reports_why_the_job_was_not_claimed`（SCN-007）。
   - 紅燈：先放 stub（續租不做事、進度不寫、trigger 一律回「找不到排程」）。
     - SCN-003：`stolen.is_none()` 失敗。
     - SCN-004：「回合執行中 job 被 worker-b 取得」。
     - SCN-001：讀到的訊息是 `["", "", ""]`。
     - SCN-002：訊息只有原錯誤，例如 `orchestrator error: backend error: … 本機 OpenCode 回合逾時`。
     - store 兩支測試與 SCN-007 都在 `assert` 失敗。
     - SCN-006 與 SCN-005 在 stub 下是綠的。SCN-006 作為 characterization；SCN-005 是防護測試，擋住把續租改成背景 task、而工作結束後沒有跟著停的寫法。
   - 測試間隔：進度與 job lease 的間隔是 `min(30 秒, lease/3)`，session lease 是 `lease/3`，所以測試用 2～3 秒的 lease 就能在幾秒內跑完。沒有另外加只給測試用的設定。
3. ✅ **最小實作**
   - 先跑 `gitnexus_impact`（`run_turn_traced_with_attachments`、`run_claimed_job`、`claim_job` 等），回報風險等級。
   - 相依：步驟 2。完成判準：步驟 2 轉綠；受影響 crate 的測試全綠。
   - 影響分析（2026-10-21）：
     - `run_claimed_job` 為 HIGH，直接呼叫端是 `run_scan` 與 CLI 的 `trigger_job`。
     - `prepare_final_session_with_policy` 與 `run_turn_traced_with_attachments` 為 CRITICAL，所有進入點的每一回合都會經過。
     - 因此以加法為主：回合在 lease 時間內結束時行為不變，由 SCN-006 鎖住。
   - 實作：
     - `session::hold_session_lease`：把工作和 `tokio::time::interval` 放進同一個 `select!`，每隔 lease/3 呼叫 `renew_agent_session_lease`；續租失敗只記 log。`run_turn` 用它包住逐棒執行，`prepare_final_session_with_policy` 用它包住壓縮呼叫。
       - 包住逐棒執行之後，rustfmt 把整個區塊重新縮排，所以 `turn.rs` 的 diff 很大。`git diff -w` 只剩包裝那幾行。
       - `wukong-runtime` 在 `[dependencies]` 加上 workspace 已有的 `tokio`，Cargo.lock 不變。另外 re-export `Role`，讓 scheduler 不用直接依賴 orchestrator。
     - store：
       - `renew_job_lease`：只更新 `locked_by` 是自己的 job。
       - `update_running_message`：只更新 `status = 'running'` 的 run。
     - executor：
       - `execute_with_heartbeat` 把 `on_role` 送進 channel，在同一個 `select!` 裡推進 `Phase`、續租並寫進度。
       - 結束後會再清空 channel 一次，避免最後一棒剛開始就失敗時標錯階段。
       - Turn 回合失敗時，訊息加上 `<階段>失敗，已執行 N 秒：`；角色名稱和「失敗」之間隔一個空白。
       - 維護作業不加前綴，進度顯示「維護作業」。
     - CLI：`MANUAL_TRIGGER_LEASE_SECS = 300`，claim 與續租都用它。`trigger_unavailable_message` 依 `get_job` 的結果區分三種原因。
   - 綠燈：runtime 84、scheduler 36、wukong（bin）1，全數通過。
4. ✅ **重現腳本複驗與中止測試**（SCN-003、SCN-004、SCN-005）
   - 用步驟 1 的腳本在變更後再跑一次：trigger 印「排程正在執行中」，原回合成功，沒有共用 session。
   - 另跑一次：回合執行中用 SIGKILL 強制終止 daemon，確認 job lease 與 session lease 在 lease 時間後到期，之後可以再次 trigger（SCN-005）。
   - 相依：步驟 3。
   - 複驗結果（2026-10-21，`repro13/run.sh` 的 `after/`，設定與步驟 1 相同）：
     - 第 5 秒的 lease 是 job t0+7、session t0+8，持續往後延，沒有過期。
     - trigger 印出 `排程正在執行中`，沒有執行回合。run 紀錄只有暖身 #1 與 daemon #2，假 agent 的紀錄裡也只有 daemon 一棒，沒有共用 session。
     - daemon 的 run #2 為 success（`done by daemon`），daemon 印出 `job … succeeded`。
   - 中止測試（`repro13/kill.sh` 的 `kill/`）：daemon 執行一棒 30 秒的回合，lease 設定同上。
     - 執行中的 `wukong schedule runs`：t+1、t+4、t+7 依序是 `進行中：第 1 棒 oracle，已執行 1 秒`、`4 秒`、`7 秒`，在真實行程中確認 SCN-001。
     - 第 7 秒以 SIGKILL 終止 daemon，當時 lease 是 job t0+9、session t0+11。終止 1 秒後再查，兩者都沒有再延長。
     - 終止後 1 秒 trigger：`排程正在執行中`（lease 還沒到期）。
     - 終止後 5 秒（t0+13）trigger：exit 0，`done by late`。job 與 session 都重新取得，所以 lease 照原本的時間到期（SCN-005）。
     - 被終止的 run #1 停在 `running`，訊息是最後一次的進度。這是 TBD-1 記下的既有行為，不在本次範圍。
5. ✅ **常青文件與交付**
   - 根目錄 `AGENTS.md`「關鍵設計細節」：補上 lease 續租與 run 進度訊息；更新 #12「同時執行的回合告知」裡「回合超過 lease 時間就看不到」這項限制。
   - `docs/cli-reference.md`：多個 daemon 的描述、`schedule runs` 的進度訊息、trigger 的訊息。
   - `CHANGELOG.md` `[Unreleased]`。
   - 全量 fmt、clippy、`cargo test --workspace --locked`，以及 `gitnexus_detect_changes`。
   - 相依：步驟 1～4。
   - 文件：
     - `AGENTS.md` 新增「lease 在執行中續租」，並刪掉「同時執行的回合告知」裡「超過 lease 時間就看不到」那項限制。
     - `docs/cli-reference.md` 更新 trigger 與 runs 的說明，補上多個 daemon 時的續租。
     - `CHANGELOG.md` 新增一條 Changed，並刪掉 #12 條目中同一項限制。
   - 全量檢查（2026-10-21）：fmt、clippy 通過；`cargo test --workspace --locked` passed=651 failed=0 ignored=12。main 是 641，本次新增 10 支測試。
   - `gitnexus_detect_changes`：正式程式碼改動的是：
     - runtime：`run_turn_traced_with_attachments`、`prepare_final_session_with_policy`、新的 `hold_session_lease`。
     - scheduler：`run_claimed_job`、`execute_job`、`execute_job_inner`、`run_unattended_turn`，新的 `execute_with_heartbeat`、`Phase`、`renew_job_lease`、`update_running_message`。
     - daemon 與 CLI：`run_scan`、`run_schedule_op`。

     其他列出的符號都是相鄰的測試或函式。
   - code-simplify：
     - session 與 job 各有一個計時迴圈。兩者分屬不同 crate、續租對象不同，合併反而要新增跨 crate 的抽象，所以保留。
     - 進度與續租共用一個計時器。no-op。

## 驗收對照
| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | 單元（scheduler，假 backend） | 步驟 2／3 |
| SCN-002 | 單元（scheduler，假 backend） | 步驟 2／3 |
| SCN-003 | 單元（runtime）＋重現腳本 | 步驟 1、2／3、4 |
| SCN-004 | 單元（scheduler）＋重現腳本 | 步驟 1、2／3、4 |
| SCN-005 | 強制終止行程的腳本 | 步驟 4 |
| SCN-006 | 單元（characterization） | 步驟 2／3 |
| SCN-007 | 單元（scheduler store、CLI） | 步驟 2／3 |
