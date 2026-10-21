# 審查報告
- 範圍：完整 PR（#20）
- Reviewed BASE SHA：53f8243a7af6497f3a0e4148298bd7fa643b1dcf
- Reviewed HEAD SHA：fa85833f3ab2bcd8a4c88c41a68d7d8b37fa3671
- Reviewed patch-id：340f36e8c559072167ddafcae851317d5ddb0030（2026-10-21 以 `git diff 53f8243 fa85833 | git patch-id --stable` 重算，一致）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-21
- Review artifact：docs/issues/issue-0013/review-fa85833.md

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
1. **`run_turn` 與壓縮呼叫的續租串接沒有單元層守門（變異實驗 M2、M3 存活）。**
   - 輸入／狀態：拿掉 `run_turn` 外層的 `hold_session_lease`（M2），或拿掉壓縮呼叫外層的包裝（M3）。
   - 預期：測試紅燈。實際：`wukong-runtime`、`wukong-scheduler` 的 lib 測試全綠。
   - M2：PR 已揭露這項限制，由重現腳本以真實行程補證據；本次 e2e 也重跑確認（見「自行執行的驗證」）。但 PR 寫的理由「lease 時間只能從環境變數讀，在測試中修改是全行程共用的狀態」不成立：獨立的 integration test binary 本來就是獨立行程，`crates/wukong-runtime/tests/planner_probe.rs` 已有先例。reviewer 寫了約 40 行的 `tests/` 探針：設 `WUKONG_SESSION_LEASE_SECS=2`，讓最後一棒睡 3.5 秒，第 3 秒由另一個 owner 取 lease。HEAD 綠燈（`stolen.is_none()`，回合輸出「完成」），M2 紅燈（「回合執行中 session lease 被拿走」）。
   - M3：壓縮期間續租是 PR 與 AGENTS.md 明寫的行為，卻沒有任何測試或重現腳本覆蓋（repro13 沒有觸發壓縮）。`prepare_final_session_with_policy` 本身就接受 `SessionPolicy`，不必動環境變數。reviewer 在 `turn.rs` 測試模組放了一支探針：`compact_every_turns: 1`、`lease_secs: 2`，壓縮呼叫睡 3.5 秒，第 3 秒由別的 owner 取 lease。HEAD 綠燈；M3 紅燈，因為 lease 被拿走，`mark_agent_session_compacted` 也會回 stale lease。
   - 建議：把這兩支測試補進 PR。依 verification.md，這屬於證據持久力，行為本身經探針確認正確，所以不阻擋判定。
2. **SCN-001「至少每 30 秒更新」的上限沒有測試（M8 存活）。** 拿掉 `heartbeat_every` 的 `.min(30_000)` 後測試仍全綠。現有測試用 lease 3 秒，間隔是 1 秒；`kill.sh` 用 lease 2 秒，也碰不到上限。daemon 預設 `--lease-secs 300`，少了上限就會變成每 100 秒更新一次，違反 SCN-001。程式碼本身正確（讀碼確認）。建議補一支手寫期望值的單元測試，例如 `heartbeat_every(300) == 30s`、`heartbeat_every(3) == 1s`。
3. **同一批 claim、還在排隊的 job 仍可能重複執行，而文件寫得像已經全面防止。**（`需決策`）
   - 輸入／狀態：daemon 預設 `--limit 10`。`claim_due_jobs` 一次取得多個 job，再依序執行；只有正在執行的 job 會續租，排隊中的 job lease 照原時間到期。
   - reviewer 實測（`rev13/batch-dup.sh`，`--lease-secs 2`，每棒 6 秒）：第 4 秒手動 trigger 第二個 job，trigger 成功執行（run #2）。第 6 秒 daemon 又用手上的舊 `Job` 把它完整執行一次（run #3，接續 trigger 的 session），最後印出 `lease was taken by another worker before completion`，期間每個 tick 都印 `scheduler_lease_renew_lost`。
   - 這是既有行為，不是本 PR 造成的，也不違反 SCN-004（SCN-004 講的是正在執行的回合）。但 `docs/cli-reference.md` 寫「執行時間超過 `--lease-secs` 也不會被另一個 daemon 或 `trigger` 重複執行」，CHANGELOG 標題寫「執行中不會被重複執行」，讀者容易誤以為已經全面防止。
   - 待決問題：本 PR 要修，還是只揭露？
   - 可選做法：
     - (a) 本 PR 補小修：`run_claimed_job` 在 `start_run` 前先 `renew_job_lease`，回 `false` 就跳過，約 5 行，另補一支測試。
     - (b) 只在 README 待確認事項與 cli-reference 揭露，另開 issue。
   - reviewer 建議：(b) 至少要做，(a) 成本低，可以一起做。
4. **「規劃中」實際涵蓋召回、planner，以及 `prepare_final_session`（取得 session lease 與壓縮）。**（`需決策`）
   - 壓縮最多可以跑 1200 秒，期間進度一直顯示「規劃中」，會讓人誤判是 planner 慢，和 TBD-4「讓原因看得到」的目的不符。
   - reviewer 重跑 `kill.sh` 時也看到失敗被標成 `規劃中失敗，已執行 0 秒：…session lease unavailable…`，但真正失敗的地方是取得 session lease，不是規劃。
   - 核准的 SCN-001 只定義了「規劃中／第 k 棒」兩種階段，所以現況符合規格。
   - 待決問題：要不要多一個「準備 session／壓縮中」的階段？
   - 可選做法：(a) `run_turn` 多一個回呼，或在壓縮前後送事件；(b) 在文件註明「規劃中」包含 session 準備與壓縮。
   - reviewer 建議：本 PR 至少做 (b)。

### NIT
- `execute_with_heartbeat` 結束後的 `roles.try_recv()` 清空迴圈沒有測試覆蓋（M6 存活），在現行 `run_turn` 流程中也觸發不到：`run_turn` 失敗時一定會 await `release_agent_session_lease`（sqlx 寫入會讓出執行權），select 迴圈會先收到 role。reviewer 另讓 backend 在 delay 0 時完全不 await，M6 仍存活。這段是無害的防禦程式，但實作計畫寫「避免最後一棒剛開始就失敗時標錯階段」，容易被當成已經驗證過。
- `turn_longer_than_job_lease_keeps_the_job_claimed` 的 `next_run_at > 1` 斷言恆真，因為 `add_job` 時就已經算好 `next_run_at`。「排定下一次執行時間」實際由 `ClaimedJobOutcome::Completed`（`complete_claimed_job` 回 true）擔保，可以改成斷言 `next_run_at` 在執行後改變，或刪掉這行。
- 時間戳以秒為單位：`WUKONG_SESSION_LEASE_SECS=1` 時，第一次續租（約 333 毫秒後）可能落在 `lease_until <= now`，`renew_agent_session_lease` 回 false，lease 實際上續不了。job lease 設 1 秒也有短暫空窗。預設值（900／300）不受影響。
- 輪替 session 的路徑上，`finish_agent_session_turn` 已釋放 lease，之後還在 `work` 裡 await `delete_session`。若刪除耗時超過 lease/3，會多印一行 `session_lease_renew_lost`。SQL 有 `lease_owner = ? AND lease_until > now` 條件，不會重新取得 lease，只是多一行 log。
- `kill.sh` 的「終止後 1 秒 trigger」結果取決於時序。實作者那次是「排程正在執行中」；reviewer 重跑時 job lease（t0+8）已經到期，但 session lease（t0+10）還沒，trigger 取得 job 後以 `規劃中失敗…session lease unavailable` 記為 failure（run #2）。兩者都符合 SCN-005，但這也顯示：行程中止後，job lease（每 30 秒續租，最多 300 秒到期）會比 session lease（每 300 秒續租，最多 900 秒到期）早到期，中間這段時間被觸發的 run 會被記為失敗。這是既有的不對稱，建議記進 TBD-1。
- README「涉及檔案」沒有列出 `wukong-runtime/src/session.rs`、`lib.rs`、`Cargo.toml`。

## 已查核維度
- 驗收與證據：
  - Proof 表 SCN-001～007 七列，與核准規格（`5a64393`）的集合相等。
  - 變異實驗（worktree + `CARGO_TARGET_DIR=…/scratchpad/mut-target`，跑 `cargo test -p wukong-runtime -p wukong-scheduler --lib`），基準 84／36 全綠：
    - M1 續租把 lease 延長 100 倍，模擬工作停了 lease 卻不會到期：`lease_expires_once_the_work_is_dropped` 紅燈。
    - M4 `renew_job_lease` 不檢查 `locked_by`：store 續租測試紅燈。
    - M5 `update_running_message` 不檢查 status：store 進度測試紅燈。
    - M7 棒次計數錯誤：`failure_message_names_the_failing_step` 紅燈。
    - M9 不續租 job lease：SCN-004 測試紅燈。
    - M10 tick 時不寫進度：SCN-001 測試紅燈。
    - M11 session 續租間隔等於 lease：SCN-003 helper 測試紅燈。
    - 存活：M2（`run_turn` 沒包續租）、M3（壓縮沒包續租）、M6（不清空 channel）、M8（沒有 30 秒上限）。見 SHOULD FIX 1、2 與 NIT。
  - 「只續租 job、不續租 session」就是 M2。「改用 `tokio::spawn` 背景 task、做完不停」在現行簽名下做不到，因為 `&Memory` 不是 `'static`，`Memory` 也沒有實作 `Clone`。這種寫法的效果由 M1 與 `kill.sh` 防護。
  - SCN-003 串接：PR 以重現腳本補證據。reviewer 另寫 `rev13/session-contend.sh`：daemon 執行排程回合時，第 5 秒另一個行程用 `wukong --scope agent:schedule-<id> "hi"` 在同一個 scope 開始回合，結果 exit 1、`session lease unavailable`。daemon 的 run 成功，session 寫回（turn_count 1）；第 5 秒的 session `lease_until` 是 t0+8，原本會在 t0+4 到期。以真實行程確認 SCN-003。
  - 現有證據對本次驗收有效；單元層守門的缺口列在 SHOULD FIX 1。
- 相關失敗面：
  - `hold_session_lease` 的 select：
    - 續租 await 期間，work 不會被 poll。這段時間只是一次 SQLite 寫入，backend 計時器由 runtime 追蹤，不會因此漏判逾時。判定：可接受。
    - `interval_at`：第一次在 now + lease/3 觸發，`MissedTickBehavior::Delay`。lease ≥ 2 秒時，即使有秒級截斷，續租也一定落在 `lease_until` 之前；lease = 1 秒見 NIT。極大的值會讓 `u64 * 1000` 溢位，但那遠超過任何合理設定。
    - 回合結束後：`finish_agent_session_turn` 釋放 lease 之後，只剩輪替清理那段還在 work 裡。續租 SQL 要求 `lease_owner = owner AND lease_until > now`，不會重新取得已釋放的 lease，只會印 log。判定：正確。
  - 各進入點的 runtime：
    - 實際檢查各個 builder：`wukong-cli`、`wukong-schedulerd`、`wukong-web` 的 main 用 `#[tokio::main]`（多執行緒，enable_all）。
    - Web 的 `chat_api.rs:234` 用 `Builder::new_current_thread().enable_all()`。
    - Telegram 用 `#[tokio::main(flavor = "current_thread")]`（enable_all）+ `LocalSet`。
    - workspace 的 tokio 是 `features = ["full"]`。
    - `select!` 不要求 `Send`，非 Send 的 `on_event`／`on_role` 仍在同一個 task。
    - 沒有發現任何 runtime 沒開 timer。全量測試涵蓋 Web 與 Telegram 呼叫 `run_turn` 的既有測試，都通過。
    - 唯一使用暫停時間（`start_paused`）的測試是 `session_retention`，沒有經過 `run_turn`。
  - executor：
    - `on_role` 的 sender 由 closure 持有，活得和函式一樣久，所以迴圈中 `roles.recv()` 不會回 None。
    - 進度只更新 `status='running'` 的 run，而且 `finish_run` 之前迴圈已經結束，同一個行程內沒有進度蓋過最終結果的競態。另一個行程只會寫自己的 run_id。
    - LeaseLost 時，續租失敗只記 log，進度照寫自己的 run，最後回傳 `LeaseLost`，與既有行為相同。
    - 維護作業顯示「維護作業」進度，失敗訊息不加前綴，與文件一致。
  - 手動 trigger：claim 與續租都用 300 秒，回合執行中會續租；`trigger_unavailable_message` 依 `get_job` 結果區分三種原因，exit code 維持 0，與原本「找不到排程」一致。同一批其他 job 的問題見 SHOULD FIX 3。
  - 失敗訊息格式：
    - Telegram 的 `notify::failure_summary` 取第一個非空行的前 300 字。前綴和原錯誤在同一行，所以通知會多出階段資訊，原錯誤仍然看得到。
    - notify 的測試用的是自造的訊息，沒有寫死訊息開頭。全量測試也沒有斷言原本的錯誤開頭。
    - `schedule runs` 會把換行換成空白，實測顯示正常。
  - 與其他 issue 的交互：
    - #12：`active_session_leases` 因為續租，對長回合變得準確，AGENTS.md 和 CHANGELOG 已刪除相應的限制，描述正確。
    - #14：續租用的 scope 是 `scheduled_turn_scope`，e2e 確認是 `agent:schedule-<id>`。
    - session_retention 看的是 session 有沒有被指向，與 lease 無關，沒有交互。
    - daemon 啟動時的 `interrupt_stale_runs` 以 `locked_until` 判斷 run 是否中斷。續租之後，仍在執行的 trigger 回合不會在 300 秒後被誤標成 interrupted，是改善。
- 需求、架構、安全、品質：
  - runtime 原本只在 dev-dependencies 有 tokio，這次移到正式依賴。memory、gateway 本來就依賴 tokio，所以依賴圖沒有新增節點，也不違反相依方向。
  - re-export `Role` 讓 scheduler 不必直接依賴 orchestrator，恰當。
  - 不改 schema，只新增兩個 UPDATE。SQL 都用參數綁定，沒有注入面。
  - code-simplify：兩個計時迴圈分屬不同 crate、續租不同的 lease，合併反而要新增跨 crate 的抽象，同意保留。`Phase` 很小，`failed()` 對空白的特判有註解說明。`execute_job_observed` 讓公開的 `execute_job` 維持原簽名，被 integration test 使用。沒有過度設計。
- 豁免、待確認與限制：
  - README 的 TBD-1～4 狀態與理由誠實，TBD-AC 已解決並記錄核准來源。
  - PR 揭露了 runtime 層 SCN-003 只測 helper，依規則不構成缺失（理由的商榷見 SHOULD FIX 1）。
  - 沒有揭露的有三項：壓縮續租沒有證據（SHOULD FIX 1）、同一批 job 重複執行（SHOULD FIX 3）、「規劃中」的涵蓋範圍（SHOULD FIX 4）。
  - 沒有 Gate 豁免。
- 自行執行的驗證（2026-10-21，HEAD `fa85833`，工作區乾淨）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test --workspace --locked`：exit 0，passed=651、failed=0、ignored=12，與實作者紀錄一致。指定的 `-p wukong-runtime -p wukong-scheduler -p wukong-cli -p wukong-schedulerd` 都包含在內。
  - `gh pr checks 20`：`fmt · clippy · test` pass（1m39s）。
  - 重跑 `repro13/run.sh`，`cargo build -p wukong-cli -p wukong-schedulerd` 的 debug binary，輸出 `scratchpad/rev13/e2e-run`：第 5 秒 job lease 是 t0+7、session lease 是 t0+8；trigger 印「排程正在執行中」；run 只有 #1 暖身與 #2 daemon（success，`done by daemon`）；agent.log 只有 daemon 一棒。與實作者的 `after/` 一致。
  - 重跑 `repro13/kill.sh`，輸出 `rev13/e2e-kill`：
    - t+1／4／7 的進度依序是 `進行中：第 1 棒 oracle，已執行 0／4／6 秒`。
    - SIGKILL 後 lease 不再延長（job t0+8、session t0+10）。
    - 終止後 1 秒的 trigger 結果見 NIT（時序差異）；終止後 5 秒的 trigger exit 0，`done by late`。符合 SCN-005。
  - reviewer 自寫的腳本：`rev13/session-contend.sh`（SCN-003 跨行程，結果見上）、`rev13/batch-dup.sh`（SHOULD FIX 3）。
  - 變異實驗與兩支探針測試的紀錄在 `scratchpad/rev13/M*.log`、`probe_compact.rs`、`probe_turn_lease.rs`。worktree 已移除，repo 沒有任何改動。

## 流程判定
PASS

沒有 MUST FIX。SCN-001～007 都有測試或真實行程證據，主要失敗面經 reviewer 以變異實驗、探針與 e2e 查核。SHOULD FIX 1、2 是證據持久力，依 verification.md 不阻擋判定。SHOULD FIX 3、4 標為 `需決策`，由協調者依 acceptance.md 逐題確認，結論記入 README 待確認事項。
