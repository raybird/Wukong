# 審查報告
- 範圍：增量 diff（7e749d4..fe5f0dd，完整 PR 審查見 review-94a5682.md）
- Reviewed BASE SHA：a758f7e3b665756ec1934a149fd4cc2e303c756a
- Reviewed HEAD SHA：fe5f0dd16a1bdeff4b87f8111ee8b941f0c25c04
- Reviewed patch-id：8de97649c7e9a759f07233cdbdc779271d9dae4e
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-21
- Review artifact：docs/issues/issue-0017/review-fe5f0dd.md

範圍核對：
- `gh pr view 21`：headRefOid 是 `fe5f0dd16a1b…`，base 是 `main`，狀態 OPEN。本機 HEAD 相同，工作區乾淨。
- `git diff a758f7e fe5f0dd | git patch-id --stable` 得到 `8de97649…`，與指定值相同。
- `94a5682..7e749d4` 只新增了 `review-94a5682.md`（A，100644，直接後繼），符合 review-evidence.md 的「單純報告提交」例外。
- 增量 `7e749d4..fe5f0dd` 有兩個非 merge 提交：0483755 動了程式、測試與 AGENTS.md，fe5f0dd 動了 issue 文件。改動共 5 個檔案：`AGENTS.md`、`crates/wukong-cli/src/main.rs`、`crates/wukong-scheduler/src/executor.rs`、`docs/issues/issue-0017/README.md`、`implementation-plan.md`。Gherkin（SCN-001～006）沒有變動，核准 commit `22652cf` 仍然對得上。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。

### NICE TO HAVE
- README「已確認證據」寫「先寫入本回合的記憶，再 acquire，最後 finish（`turn.rs:383-401`）」，但 remember 在 `turn.rs:362-381`，引用範圍沒有涵蓋第一步。建議改成 `362-401`。
- SCN-003 的核准文字「不會因為 session 狀態被清而失敗」仍然暗示提早清可能讓回合失敗。它描述的行為沒有錯，而且改核准文字需要重新核准，所以不建議動。
- `trigger_report` 的新測試只涵蓋 `Deleted` 成功／失敗與 `LeaseLost`，沒有涵蓋 `Completed` 與 `NotHeld`。這兩條是原本就有的行為，不屬於本 issue，不構成缺口。
- D-1 的決定記在 README Timeline、implementation-plan 步驟 6，以及 PR 說明的 TBD-1。README `## 待確認事項` 的 TBD-1 那一列沒有提到 D-1，可以補一句交叉引用。
- review-94a5682 列的 NICE TO HAVE 都照實記在步驟 6 的「不處理」清單裡，沒有變化。

## 已查核維度
- **驗收與證據**：
  - **SHOULD FIX 1（理由更正）已處理，新說法經程式碼與變異證實。**
    - 讀碼結果：`run_turn` 結束時的順序是：
      1. `memory.remember(...)`（`turn.rs:362-381`），寫入本回合的 User 與 Assistant event；
      2. `acquire_agent_session_lease`（`turn.rs:383-391`）；
      3. `finish_agent_session_turn`（`turn.rs:393-401`）。
    - 第 2 步以 `INSERT OR IGNORE … SELECT FROM agent_sessions`，加上 `INSERT … ON CONFLICT(scope) DO NOTHING`（`store/mod.rs:571-584`），重建被刪掉的 `agent_session_state` 列，再取得 lease。
    - 第 3 步在同一個 transaction 裡，以 `INSERT … ON CONFLICT DO UPDATE` 寫回 `agent_sessions`（`store/mod.rs:657-666`）。
    - 所以「提早清不會讓回合失敗，但會被寫回去」是正確的。被引用的行號在 BASE `a758f7e` 與 HEAD 都成立：runtime 沒有變動，store 的新增在 753 行之後。
    - 變異 M10（被持有時也立刻清，事後照常再清，回傳值不變）：`deleting_a_running_job_cleans_up_after_the_turn`（SCN-003）是綠的，只有 SCN-005「被持有時不先清」紅燈。這證實提早清不會讓回合失敗。
    - 變異 M10'（被持有時立刻清，並拿掉事後清理）：SCN-003 紅燈，`left: (2, Some("ses_slow"))`。回合本身仍然以 `Deleted` 成功，紅燈出現在最後的 scope 斷言。這直接證實「提早清會被寫回去」：本回合的 2 筆 event 加上 session 指向。
    - 更正位置都已改好：
      - `AGENTS.md:283`；
      - `executor.rs` 的 `delete_job` doc comment；
      - README「已確認證據」，並附上更正註記；
      - implementation-plan 步驟 3：補了 SCN-003 原本紅燈的真正原因；
      - implementation-plan 步驟 4：「第 2 秒時 scope 還沒清」；
      - implementation-plan 步驟 6；
      - `gh pr view 21` 的說明：「延後的原因是…提早清不會讓回合失敗。這是依審查 SHOULD FIX 1 更正的」。
    - 舊說法沒有殘留。全 repo（排除 target、.git）搜尋「會讓回合失敗」「要求 session 狀態還在」「執行中不能先清」「session 狀態還在」，命中的只有：
      - 前一份審查報告本身；
      - README 與 plan 的更正註記，它們引用舊說法是為了標明已更正；
      - `turn.rs`、`command.rs` 原本的錯誤字串。
    - `docs/cli-reference.md:101`、CHANGELOG、CLI 的 rm 訊息只描述行為，沒有寫理由，與程式一致。
  - **SHOULD FIX 2（守門測試）已處理，兩支測試在對應變異下都紅燈。**
    - SCN-008 `queued_job_whose_lease_was_taken_is_skipped`：先 `seed_scope` 自己的 scope，被 worker-b 取得後跑 `run_claimed_job`，得到 `NotHeld`，再斷言 scope 仍是 `(1, Some("ses-…"))`。變異結果：
      - M11a（照樣清，但回傳值依 `get_job` 正確給出）：紅，`left: (0, None)`。
      - M11b（直接拿掉 `get_job` 判斷，一律清並回 true）：紅，`left: (0, None)`。

      這守住了「排程還在時不清」。M11 改的是 `clear_scope_if_deleted` 本身，`NotHeld` 與回合結束的 `LeaseLost` 兩個呼叫點共用它，所以函式層級的變異能被抓到。只改 `LeaseLost` 呼叫點的變異（例如在呼叫端無條件 `delete_scope`）不在本題範圍，沒有另外測。
    - CLI `trigger_report`：逐分支比對抽出前後的行為。
      - `Completed`／`Deleted`：先 `println!(message)`；成功回 `Ok`，失敗回 `Err(message)`。
      - `NotHeld`：印「排程正在執行中」，回 `Ok`。
      - `LeaseLost`：不印，回 `Err("排程 lease 已被其他 worker 接手")`。

      新的 `trigger_job` 先印 stdout、再依 error 回 `Err`，順序與內容都和原本相同，判定是純重構。變異結果：
      - M9（`Deleted` 當成 `LeaseLost`）：`trigger_reports_a_turn_deleted_while_running` 紅，`left: (None, Some("排程 lease 已被其他 worker 接手"))`。
      - M9b（`Deleted` 失敗時不回錯誤）：同一支測試紅。
  - 前一版的 SCN-001～006 Proof 不受影響：增量沒有改正式路徑的行為，只動 doc comment 與 CLI 的純重構。PR 表格已補上 SCN-004 trigger 子句與 SCN-005「排程還在不清」的守門測試。
- **相關失敗面**：
  1. `trigger_report` 重構會不會改變 exit code 或輸出：逐分支比對相同，見上。M9、M9b 紅燈。
  2. 新的 SCN-008 斷言會不會是假綠：`seed_scope` 之後 scope 是 `(1, Some)`，M11a 與 M11b 都把它清成 `(0, None)` 而紅燈。斷言比對的是精確值，不是「非空」。
  3. 新理由是否成立：M10 與 M10' 分別證實「不會失敗」與「會被寫回」。
  4. 增量不新增任何會刪除資料的路徑，`delete_scope` 與 `remove_job` 都沒有改動。不需要重做前一份報告列出的 9 個失敗面。
- **需求、架構、安全、品質**：
  - 正式程式碼的改動只有兩處：`delete_job` 的註解，以及 CLI 抽出私有純函式 `trigger_report`（`(Option<String>, Option<String>)`），都沒有新的 public API。
  - 依賴方向不變。
  - 抽函式是為了讓 SCN-004 子句可以測，規模合理，與前一版的 `deliverable_output` 做法一致。
  - AGENTS.md 的敘述精簡、正確，也加上了「提早清不會讓回合失敗」，避免讀者重新誤解。
- **豁免、待確認與限制**：
  - D-1：使用者 2026-10-21 選「維持現狀」，被持有時只刪 job，記錄在 README Timeline、plan 步驟 6 與 PR 說明。依前一份報告，這是 reviewer 建議的選項，殘餘風險仍是 TBD-1，已揭露。
  - 沒有 gate 豁免。
  - 我沒有重跑完整 workspace 測試與真實行程 e2e：增量不影響這兩者的被測行為，前一份報告與實作者在 0483755 的全量紀錄（passed=666）可以沿用。我自行跑了受影響的兩個 crate，CI 也跑過 HEAD 的全量檢查。
- **自行執行的驗證**（2026-10-21，HEAD `fe5f0dd`）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test -p wukong-scheduler -p wukong-cli --locked`：exit 0，passed=78、failed=0、ignored=2，用的是 repo 的 `target/`。`trigger_reports_a_turn_deleted_while_running` 與 `queued_job_whose_lease_was_taken_is_skipped` 都是 ok。
  - 變異實驗：
    - 在 `~/.cache/wukong-review-wt` 開 detached worktree，設 `CARGO_TARGET_DIR=~/.cache/wukong-review-target`，做完已移除兩者。
    - 紅燈：M11a、M11b、M9、M9b、M10'。
    - 依預期：M10 下 SCN-003 綠，只有 SCN-005 紅。
  - `gh pr checks 21`：`fmt · clippy · test` pass。對應的 run 39556078542 是 `headSha=fe5f0dd…`、`conclusion=success`、`event=pull_request`。
  - `gh pr view 21 --json headRefOid`：`fe5f0dd16a1bdeff4b87f8111ee8b941f0c25c04`。
  - 紀錄放在 scratchpad 的 `revfe5/`（`fmt.log`、`clippy.log`、`test.log`、`mut-summary.txt`、`mut-*.log`、`mutate.py`、`run_mut.sh`）。

## 流程判定
PASS

增量完成了前一份報告的 SHOULD FIX 1 與 2：理由已在所有指定位置更正，新說法經讀碼與 M10／M10' 證實；兩支守門測試在 M11 與 M9 下都紅燈。`trigger_report` 是不改行為的重構。依 review-evidence.md，本報告涵蓋 `a758f7e..fe5f0dd` 的完整 PR：審查範圍是前一份完整審查（`94a5682`），加上報告提交 `7e749d4`，再加上本次增量。沒有 MUST FIX，判定 PASS，綁定 HEAD `fe5f0dd`、patch-id `8de97649…`。之後若有本報告以外的新提交，要重新審查。
