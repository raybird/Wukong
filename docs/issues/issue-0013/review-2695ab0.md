# 審查報告
- 範圍：增量 diff（8aa7e9b..2695ab0，完整 PR 複審見 review-8aa7e9b.md）
- Reviewed BASE SHA：53f8243a7af6497f3a0e4148298bd7fa643b1dcf
- Reviewed HEAD SHA：2695ab01b94637b4ad80daf276f32eec8d910f7e
- Reviewed patch-id：700ada81edc5396a1a61b365115df640644275d7（2026-10-21 以 `git diff 53f8243 2695ab0 | git patch-id --stable` 算出）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-21
- Review artifact：docs/issues/issue-0013/review-2695ab0.md

範圍核對：
- `git merge-base main 2695ab0` 是 `53f8243`。
- `git log 8aa7e9b..2695ab0` 有兩個提交：
  - `75f1742`：`--name-status` 只有 `A docs/issues/issue-0013/review-8aa7e9b.md`，父提交是 `8aa7e9b`。
  - `2695ab0`：改了 `AGENTS.md`、`executor.rs`、`schedulerd/src/main.rs`、`implementation-plan.md` 四個檔。
- `2695ab0` 動到程式碼，不符合 review-evidence.md「只新增報告」的例外，所以需要本次增量審查。
- `gh pr view 20` 的 headRefOid 仍然是 `8aa7e9b`，`2695ab0` 還沒 push。push 後要確認 headRefOid 等於 `2695ab0`，本報告才對應遠端 PR。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
1. **`run_claimed_job` 函式內的註解還是舊說法，而且推論錯誤。**
   - 位置：`crates/wukong-scheduler/src/executor.rs:151-152`。
   - 現在的註解：
     ```rust
     // 2026-10-21（issue-0013）：同一批取得的 job 排隊時只有正在跑的那個會續租。輪到這個
     // job 時先續租一次，lease 已經被別人拿走，代表它在等待期間已經被執行過。
     ```
   - 問題：
     - 本次增量要修的就是 review-8aa7e9b NIT 1 指出的措辭，這段註解就在 `NotHeld` 的判斷上方，卻沒有跟著改。
     - 「代表已經被執行過」在停用、刪除的情況下不成立：停用和刪除都沒有執行。如果是重新啟用，`next_run_at` 也只是被推到下一次 cron，同樣沒有執行。
     - 只改了 enum 上的註解，函式內這段又寫了另一種說法，兩處互相矛盾。
   - 實作計畫步驟 7 把改動列成「`NotHeld` 的註解、daemon 的警告訊息、`AGENTS.md`」，沒有提到這段，看起來 NIT 1 已經全部處理完，但其實還沒。
   - 建議：改成類似「lease 已經不在自己手上（被別人取得，或被停用、重新啟用、刪除），就不執行」。
   - 只是註解，不影響行為，依 review-evidence.md 不阻擋判定。

### NIT
1. `docs/cli-reference.md:110` 寫「等待期間已被別人取得執行的就跳過」。這句話本身沒錯，但沒有提到停用、重新啟用、刪除也會跳過，和 `AGENTS.md` 的新說法不一樣完整。要不要補，由實作者決定。
2. `run_claimed_job` 的 doc comment（`executor.rs:136-143`）只提到 `LeaseLost`，沒有提到 `NotHeld`。這是 `8aa7e9b` 新增 `NotHeld` 時就有的狀況，不是本次造成的。
3. 新警告用「re-enabled」、「重新啟用」：能被 claim 的 job 一定是啟用中，所以這裡實際指的是「對啟用中的 job 再執行一次 `schedule enable`」或「先停用再啟用」。兩種都會清掉 lease（`set_enabled` 不管前後狀態都會把 `locked_by` 設成 NULL），用詞可以接受，記錄供參考。

## 已查核維度
- 驗收與證據：
  - **措辭正確性：列出所有會讓開始前續租回 false 的情況。**
    - `renew_job_lease` 的 SQL 是 `UPDATE scheduler_jobs SET locked_until = ?3 WHERE id = ?1 AND locked_by = ?2`（`store.rs:285`）。只有兩種情況會回 false：那一列不存在，或 `locked_by` 不是自己。SQL 錯誤會以 `Err` 往上傳，不會變成 `NotHeld`。
    - 全 repo 搜尋 `scheduler_jobs` 的寫入，都在 `store.rs` 裡；其他 crate 的 `UPDATE` 和 `ON CONFLICT` 都和這張表無關。逐一檢查會改到 `locked_by` 或刪除列的路徑：
      - `claim_due_jobs`、`claim_job`（`store.rs:194`、`225`）：被另一個 daemon 或 CLI `schedule trigger` 取得。這就是「taken by another worker」。
      - `complete_claimed_job`（`store.rs:342`，條件是 `locked_by = ?4`）：只有持有 lease 的 worker 能清掉它，所以一定是先被別人取得，也屬於「taken by another worker」。
      - `set_enabled`（`store.rs:156`）：不管 `enabled` 傳 true 或 false，都把 `locked_by` 和 `locked_until` 設成 NULL，涵蓋停用和重新啟用。呼叫端有 CLI `schedule enable`／`disable`（`wukong-cli/src/main.rs:268`、`280`）和 Web 的啟用開關（`wukong-web/src/lib.rs:466`）。
      - `remove_job`（`store.rs:137`，`DELETE`）：涵蓋刪除。呼叫端有 CLI（`main.rs:257`）和 Web（`lib.rs:490`）。
      - `complete_job`（`store.rs:310`，不檢查 worker）：只有 `store.rs:604` 的測試在用，正式路徑不會走到。
    - 結論：新措辭列出的「被別人取得、停用、重新啟用、刪除」涵蓋了正式路徑上所有會走到 `NotHeld` 的情況，也沒有寫進不會發生的情況。
    - 「lease 過期但沒有人接手」不會走到 `NotHeld`：`renew_job_lease` 不看 `locked_until`，所以續租會成功，job 照常執行。review-8aa7e9b 的探針 P1 已經證實。新措辭沒有把這種情況寫進去，是正確的。
    - review-8aa7e9b 的探針 P3～P5 已經證實停用、重新啟用、刪除三種情況會回 `NotHeld`。本次沒有改到任何控制流程（見下一點），那份證據仍然適用。
  - **AGENTS.md 與實作計畫步驟 7**：
    - `AGENTS.md:287` 的新句子「lease 已不在自己手上（排隊期間被別人取得，或被停用、重新啟用、刪除）就回 `NotHeld`、不執行」和上面的盤點一致。
    - 步驟 7 引用的新警告文字，和 `schedulerd/src/main.rs:227` 一字不差。
    - 步驟 7 說「其他 NIT 不處理」的三項（計時餘裕、`schedule enable` 清掉執行中 job 的 lease、壓縮測試的敘述），分別對應 review-8aa7e9b 的 NIT 2、3、4，對照正確。
    - 不一致的地方：步驟 7 列出的改動清單漏了函式內的註解，那段也沒有改（見 SHOULD FIX 1）。
- 相關失敗面：
  - **行為不變**：
    - `git show -w 2695ab0` 的實質差異只有四處：enum 的 `///` 註解、`eprintln!` 的字串字面值、`AGENTS.md` 的一句話、實作計畫新增的步驟 7。
    - 不加 `-w` 時，`schedulerd` 的差異是 rustfmt 因為字串變長而改了 `match` 的排版（`let output = match …(`）。分支內容、順序、`continue` 都和原本相同。
    - `eprintln!` 字串那一行仍然多縮排 4 格，這是 rustfmt 遇到超長字串時保留原樣，`cargo fmt --check` 通過。
  - **舊字串的使用者**：
    - 用 `lease was taken by another worker while queued`、`lease no longer held`、`taken by another worker`、`NotHeld` 搜尋全 repo，排除 `target/`，範圍包含 `scripts/`、`docs/`、`crates/` 底下的測試。
    - 沒有任何測試或腳本比對 daemon 的警告字串。`scripts/` 底下也沒有任何檔案含 `skipped`。
    - 舊字串只出現在 `implementation-plan.md:124`、`review-fa85833.md`、`review-8aa7e9b.md`，三處都是當時實際執行的輸出紀錄，維持原文是正確的。
    - `LeaseLost` 的警告 `lease was taken by another worker before completion` 沒有改，語意也正確：完成時 lease 不在手上，不是被別人取得就是被清掉。這句不在本次範圍。
  - **CLI 的 `NotHeld`**：`wukong-cli/src/main.rs:365` 印「排程正在執行中」，本次沒有改。review-8aa7e9b 已判定只有極短的空窗才會走到，可以接受。
- 需求、架構、安全、品質：
  - 只改了註解、log 字串和文件，沒有新的 SQL、權限或依賴，crate 邊界不變。
  - 新的警告文字比較長，但只在 `NotHeld` 時印一行，不影響 log 的量。
- 豁免、待確認與限制：
  - 本次增量沒有新的豁免或待確認事項。TBD-1～5 沒有變動。
  - 限制：遠端 PR head 還是 `8aa7e9b`，本報告只對應本機 `2695ab0`。
- 自行執行的驗證（2026-10-21，HEAD `2695ab0`，執行前工作區乾淨）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test -p wukong-scheduler -p wukong-schedulerd --locked`：exit 0。
    - wukong-scheduler lib：38 passed。其中包含 `queued_job_whose_lease_was_taken_is_skipped`。
    - `cli_backend`：1 passed。
    - `managed_actual`：1 ignored，本來就是 ignored。
    - wukong-schedulerd main：22 passed。
    - `retention_daemon`：2 passed。
  - 直接檢查編譯產物：`cargo build -p wukong-schedulerd --locked` 後，`strings target/debug/wukong-schedulerd` 含有新字串 `skipped: lease no longer held when its turn came (taken by another worker, or the job was disabled, re-enabled or deleted while queued)`，舊字串 `lease was taken by another worker while queued` 出現 0 次。
    - unit test 的二進位檔沒有這段字串，因為 `run_scan` 只從 `main` 呼叫，在測試二進位檔裡被裁掉了。所以改用正式的 binary 檢查。
  - 沒有重跑全量 workspace 測試：本次只改了註解和字串，`-w` diff 已經證實控制流程不變，review-8aa7e9b 的全量結果（655 passed）仍然適用。
  - 收尾：repo 除了本報告沒有其他改動；`target/` 只多了 `wukong-schedulerd` 的 debug 建置產物。

## 流程判定
PASS

沒有 MUST FIX。本次增量只改了註解、log 字串和文件，`-w` diff 和編譯產物都證實控制流程不變，受影響 crate 的 fmt、clippy、測試都通過。新措辭和 store 的 SQL 盤點一致：會走到 `NotHeld` 的正式路徑只有被別人取得、停用、重新啟用、刪除四種，沒有遺漏，也沒有寫進不會發生的情況。沒有測試或腳本比對舊字串。`75f1742` 只新增報告，`2695ab0` 經本次審查，完整 PR 可以沿用 review-8aa7e9b 的 PASS 證據。SHOULD FIX 1（函式內的舊註解）只是註解，不阻擋判定，但它就是本次增量要處理的那個 NIT，建議合併前一起改掉。push 後要確認 PR 的 headRefOid 是 `2695ab0`。
