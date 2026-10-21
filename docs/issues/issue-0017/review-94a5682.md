# 審查報告
- 範圍：完整 PR（#21）
- Reviewed BASE SHA：a758f7e3b665756ec1934a149fd4cc2e303c756a
- Reviewed HEAD SHA：94a5682d1608853e98d086d2c2479b3b3161f88a
- Reviewed patch-id：d190c757bd8c1203d2adb71e3eae4ff051f3a8e6
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-21
- Review artifact：docs/issues/issue-0017/review-94a5682.md

範圍核對：`gh pr view 21` 的 headRefOid 是 `94a5682d…`，base 是 `main`；`git diff a758f7e 94a5682 | git patch-id --stable` 得到 `d190c757…`，與指定值相同。本機工作區乾淨。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX

1. **「執行中先清會讓回合失敗」這個理由與程式行為不符，卻寫進了常青文件。**
   `AGENTS.md`「排程能力注入」寫「因為回合結束時 `finish_agent_session_turn` 要求 session 狀態還在，執行中不能先清」。PR 說明與 README「已確認證據」也有同樣的說法。實際上 `run_turn` 在 finish 之前會先呼叫 `acquire_agent_session_lease`（`crates/wukong-runtime/src/turn.rs:384`），而它會用 `INSERT … ON CONFLICT DO NOTHING` 重建被刪掉的 `agent_session_state` 列，再取得 lease（`crates/wukong-memory/src/store/mod.rs:577-600`）。所以 finish 會成功。
   - 實測（變異 M10）：`delete_job` 在被持有時也立刻 `delete_scope`，但仍回 `DeletedWhileHeld`。結果 `deleting_a_running_job_cleans_up_after_the_turn`（SCN-003）是**綠的**：回合成功、輸出「提醒完成」、最後 scope 是 `(0, None)`。只有 SCN-005 那句「被持有時不先清」的斷言紅燈。
   - 這和 implementation-plan 步驟 3 寫的「被持有時也立刻清：SCN-003 與 SCN-005 都紅燈」不一致。原變異的做法可能不同（例如連回傳值也一起改了），請記下變異的確切內容。
   - 延後清理在設計上仍然成立：它是核准的 Q4 行為，回合結束時也會再清一次，最終狀態是乾淨的。所以這一項只需要修正理由。比較準確的說法是：執行中先清，回合後續仍會寫回記憶與 session 指向，所以要等回合結束後再清。
   - 修正位置：`AGENTS.md`、PR 說明，以及 README「已確認證據」或 Timeline 的更正註記。照專案「驗證紀律」，錯誤的敘述留在常青文件，會誤導之後的維護者。
2. **兩個與資料安全或驗收相關的路徑，沒有測試守住（變異存活）。**
   - **M11**：把 `clear_scope_if_deleted` 改成「job 還在也照樣清」。這等於刪掉一個仍存在、而且可能正在被別的 worker 執行的排程的全部記憶，但既有測試全綠。我在臨時 worktree 加了一支探針測試：job 被停用造成 `NotHeld`，以及停用後再啟用造成 `LeaseLost`，兩種情況都斷言 scope 保留。這支測試在 HEAD 是綠的，在 M11 下紅燈（`left: (0, None)`）。所以**目前的程式碼正確**，缺的是回歸保護。建議把類似的斷言加進既有的 SCN-008、停用排隊等測試。
   - **M9**：CLI trigger 收到 `Deleted` 時改回錯誤，`wukong-cli` 測試照樣全綠。SCN-004 的最後一句「手動 trigger 執行的，就印出回合結果，exit code 依結果決定」在 PR 的 Proof 和 implementation-plan 裡都沒有證據，步驟 4 的真實行程只跑了 daemon。我補跑了真實行程，結果見下方「自行執行的驗證」：本分支 exit 0、印出「提醒完成」、scope 歸零；main 是 exit 1，並回報「排程 lease 已被其他 worker 接手」。這一句驗收現在有證據了，但沒有自動化回歸。建議把本報告的 e2e 寫進 PR 的 Proof，或把 trigger 的結果判斷抽成純函式來測。

### NICE TO HAVE
- **M13**：Web 刪除執行中的排程（`DeletedWhileHeld`）時回 200，但沒有測試。改成 404 也不會紅燈。另外，回應沒有 body，Web 使用者看不出清理會延後；CLI 會說明這一點。
- `ClaimedJobOutcome::Deleted` 的文件寫「scope 已清掉」，daemon 也印「cleared its scope」。但 `clear_scope_if_deleted` 清理失敗時只記一行 `scheduler_scope_cleanup_failed`，這兩句話就不成立。另外，`run_claimed_job` 的 doc comment 仍然只提到 `LeaseLost`。
- 排程在執行中被刪除後，heartbeat 每一輪都會印 `scheduler_lease_renew_lost`（`executor.rs:321`），看起來像 lease 被搶，其實是排程被刪了。這只是 log 雜訊。
- 有設定 `WUKONG_MD_DIR` 時，markdown 鏡像 `agent_schedule-<id>.md` 不會刪；`memory export` 也不會移除已經不存在的 scope 檔。這和既有的 prune 一致，因為鏡像是單向衍生的資料，但 README 的不處理清單沒有寫到這一點。`recall_telemetry` 的 scope 欄只存查詢雜湊與統計，不屬於記憶，不刪是合理的。
- `NotHeld` 路徑新增的 `clear_scope_if_deleted(...).await?`：`get_job` 發生資料庫錯誤時，`run_claimed_job` 會從原本的 `NotHeld` 變成 `Err`，daemon 的 `run_scan` 因此用 `?` 中止同一批剩下的 job。只在資料庫出錯時發生，影響有限。

### 需決策（選擇性，reviewer 建議維持現狀）
- 如果 SHOULD FIX 1 屬實（M10 已證實），有另一種做法：「被持有時也立刻清，回合結束後再清一次」。好處是 TBD-1（執行中刪除後行程當掉）的殘留可以從「所有舊記憶加上 session 指向」縮小到「只剩這個回合新寫的部分」。代價是 SCN-003 核准的「回合結束後才清」與 rm 的說明訊息都要改。
  - 待決問題：要不要為了縮小 TBD-1 的殘留而修訂 SCN-003？
  - 建議：維持核准行為，只修正理由。

## 已查核維度
- **驗收與證據**：
  - SCN-001～006 都對應到測試，在 HEAD 全綠：10 支新測試都在 crate 測試輸出中列為 ok。
  - 紅燈紀錄（步驟 1～3）寫明了失敗的斷言，以及這些失敗與目標行為的關係，不是編譯失敗。
  - SCN-003／004 的 daemon 路徑，我重跑 `repro17/run.sh` 後與實作者記錄的數字一致。
  - SCN-004 的手動 trigger 子句，實作者沒有附證據，由本次 reviewer 的 e2e 補上（見 SHOULD FIX 2）。
  - SCN-006 是 characterization，在 stub 下本來就是綠的，計畫已經說明。
- **相關失敗面**（逐項列出輸入或狀態、預期、覆蓋與結論）：
  1. **可不可能刪到任意 scope**：
     - `delete_job` 只在 `remove_job` 真的刪到一列時，才清 `agent:schedule-<id>`。對不存在的 id 送 `DELETE /api/schedules/<任意字串>`，`remove_job` 回 `None`，結果是 404，不會碰 memory。SCN-006 有測到不存在的 id。
     - job id 只由 `add_job` 用 `Uuid::new_v4` 產生，`INSERT INTO scheduler_jobs` 只有這一處。
     - SQL 用參數綁定加上 `scope = ?1` 等號比對，特殊字元與前綴都不會擴大範圍：`-x` 對照組，加上實作者記錄的 LIKE 變異。
     - 空 id 會組出 `agent:schedule-`，不是空字串，但這需要一個 id 為空的 job 存在，實際上不可能。空 scope 防護有測試（M7 紅燈）。
     - Web 端點沿用原本的 token 授權。
     - 結論：安全。
  2. **表格覆蓋**：
     - `memories` 的 FTS 由 AFTER DELETE trigger 同步，測試直接數 FTS 列數。
     - embedding 是 `memories` 的欄位，刪列時一起刪掉。
     - 少刪 `sessions`、`agent_sessions` 或 `agent_session_state` 任何一張（M4、M5、M6），測試都會紅燈。
     - `recall_telemetry` 不刪，合理；聊天紀錄屬於擁有者（TBD-5）；markdown 鏡像見 NICE。
     - `delete_scope` 是單一 transaction。`remove_job` 與 `delete_scope` 是兩個 pool、兩次提交，無法一起原子化，已揭露為 TBD-2，錯誤會照實回傳。
  3. **held 判斷**：
     - 判斷式寫反（M1）或忽略到期（M3），測試都會紅燈。
     - 拿掉 `COALESCE`（M2）是等價變異：`claim` 一定同時設定 `locked_by` 與 `locked_until`，`complete` 和 `set_enabled` 也一定同時清掉兩者。不算缺口。
  4. **lease 到期但回合還在跑**：預設 lease 300 秒，每 30 秒續租，續租要停擺超過 270 秒才會發生。這時刪除會判成沒被持有，立刻清掉 scope。依 M10 的實測，回合仍會成功；回合結束時 `complete_claimed_job` 回 false，`get_job` 回 None，於是再清一次，最終狀態是乾淨的，結果是 `Deleted`。唯一的差別是 rm 不會印「執行中」的說明。結論：機率低，後果無害。
  5. **排隊中而且 lease 已到期**：刪除時立刻清；輪到時 `renew` 回 false，得到 `NotHeld`，再清一次也沒有作用。無害。
  6. **刪除後、事後清理前，同一個 id 又被建立**：UUID v4，可以忽略。
  7. **job 還在時的 NotHeld 與 LeaseLost**（被別人取得、停用、重新啟用）：不清。程式碼正確，reviewer 探針測試證實，但既有測試沒有守住（SHOULD FIX 2，M11）。
  8. **session lease 續租**：延後清理時，session 狀態在回合期間一直在，`hold_session_lease` 不受影響。立刻清的情況（失敗面 4）只會印 `session_lease_renew_lost`，結束時由 acquire 重建。
  9. **daemon 對 `Deleted` 的處理**：照常記成 succeeded 或 failed，並送出通知。改成不送（M8），測試紅燈。
- **需求、架構、安全、品質**：
  - `wukong-scheduler → wukong-memory` 是既有的依賴（BASE 的 Cargo.toml 已經有），方向沒變。
  - 新的 public API 是 `Memory::delete_scope`、`delete_job`、`DeleteJobOutcome`、`SchedulerError::Memory`，範圍恰當。
  - `remove_job` 改成回傳 `Option<bool>`。呼叫端只有 CLI 與 Web，都已經改用 `delete_job`，store 的測試也一併更新。
  - `ClaimedJobOutcome` 新增 variant 後，所有 match（CLI trigger、daemon 的 `deliverable_output`、測試）都是窮舉的，其他 crate 沒有用到。
  - CLI 的 exit code：`rm` 維持 0；trigger 對 `Deleted` 依結果決定 exit code，已用 e2e 驗證成功時是 0。
  - Web：NotFound 回 404，其餘回 200，錯誤回 500。
  - 被刪除的排程照常送 Telegram，這是使用者 Q5 的決定。
  - 品質：清理邏輯只在 `delete_scope` 一處，事後清理共用 `clear_scope_if_deleted`，沒有重複，也沒有過度抽象。
  - `deliverable_output` 是為了可測而抽出來的純函式，規模合理。
- **豁免、待確認與限制**：
  - README 的 TBD-1～5 都誠實揭露，理由和追蹤方式都在，與 PR 的「限制與待確認」一致。
  - 沒有 gate 豁免。
  - SCN-004 的 daemon 部分由「抽出的純函式加上真實行程」承擔，我認為足夠：`run_scan` 用的是具體的 Telegram client，真實行程的 proxy 失敗訊息證明確實嘗試送出了。
  - 文件：`docs/cli-reference.md` 與 CHANGELOG 的描述和行為一致。`AGENTS.md` 那句理由不正確（SHOULD FIX 1）。
- **自行執行的驗證**（2026-10-21，HEAD `94a5682`，主 repo 的 `target/`；變異 worktree 用 `scratchpad/mut-target`，`/tmp` 空間足夠，沒有改到 `~/.cache`）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test --locked -p wukong-memory -p wukong-scheduler -p wukong-cli -p wukong-web -p wukong-schedulerd`：passed=283、failed=0、ignored=3。
  - `cargo test --workspace --locked --no-fail-fast`：exit 0，passed=665、failed=0、ignored=12，與實作者的紀錄相同。
  - `sh repro17/run.sh target/debug rev17/e2e-daemon`：
    - scope 記憶、session_state、agent_sessions、擁有者記憶、jobs 依序是：開始前 `1|0|1|1|1`，第 2 秒 `1|1|1|1|0`，結束後 `0|0|0|1|0`。
    - rm 印「已刪除排程；排程正在執行，回合結束後才會清掉它的記憶與 session」。
    - daemon 依序印出 `was deleted while running; cleared its scope`、`succeeded`、`telegram delivery … failed`（proxy 擋下）。
  - reviewer 自寫的 `rev17/trigger.sh`（手動 trigger、agent 在回合中刪掉自己，沿用 repro17 的 `agent.sh`）：
    - 本分支：`trigger exit=0`，stdout 是「提醒完成」，結束後 `0|0|0|1|0`。
    - main binary：`exit=1`，`error: … 排程 lease 已被其他 worker 接手`，結束後 `3|1|1|1|0`。
  - 變異（臨時 worktree，做完已移除）：
    - 紅燈：M1、M3、M4、M5、M6、M7、M8、M10（只有 SCN-005 紅）、M12。
    - 存活：M2（等價變異）、M9、M11、M13。
    - M11 另由 reviewer 的探針測試證實「HEAD 綠、變異紅」。
  - `gh pr checks 21`：`fmt · clippy · test` pass。
  - 紀錄放在 scratchpad 的 `rev17/` 目錄（`fmt.log`、`clippy.log`、`crates-test.log`、`ws-test.log`、`mut-summary.txt`、`mutlogs/`、`probe.log`、`e2e-*`）。

## 流程判定
PASS

沒有 MUST FIX。SCN-001～006 與重要失敗面都有證據：SCN-004 的手動 trigger 子句由 reviewer 的 e2e 補上，「job 還在就不清」由 reviewer 的探針測試補上。SHOULD FIX 1 是錯誤的設計理由，不影響行為的正確性。SHOULD FIX 2 屬於證據持久力，依 verification.md 不阻擋這次驗收，建議合併前或在後續 issue 處理。
