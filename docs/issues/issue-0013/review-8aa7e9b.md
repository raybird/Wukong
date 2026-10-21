# 審查報告
- 範圍：完整 PR（#20）
- Reviewed BASE SHA：53f8243a7af6497f3a0e4148298bd7fa643b1dcf
- Reviewed HEAD SHA：8aa7e9bd7f1f533d56c912ccf1f9a8f13719c7dd
- Reviewed patch-id：b4dce375ae16d6da0f8e5a0cf350611036ef211e（2026-10-21 以 `git diff 53f8243 8aa7e9b | git patch-id --stable` 重算，一致）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-21
- Review artifact：docs/issues/issue-0013/review-8aa7e9b.md

範圍核對：`gh pr view 20` 的 headRefOid 是 `8aa7e9b…`，base 是 `main`；`git merge-base main 8aa7e9b` 與 `main`、`origin/main` 都是 `53f8243`。本次重點看 `fa85833..8aa7e9b`（8fc55c9 報告、a5a0340 SCN-008 與決策、8aa7e9b 修正），判定涵蓋 `53f8243..8aa7e9b` 全部。

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。

### NIT
1. **`NotHeld` 的註解與 daemon 的 warning 只描述「被別人拿去執行」，但還有三種情況也會走到這裡。**
   - 原因：`set_enabled`（CLI 的 `schedule enable`／`disable`，以及 Web 的啟用開關）會把 `locked_by` 清成 NULL；`remove_job` 會刪掉整列。這兩個動作都會讓開始前的續租回 false。
   - reviewer 探針（見「自行執行的驗證」P3～P5）：排隊中被停用、被重新啟用、被刪除，三種都回 `NotHeld`，daemon 印出 `skipped: lease was taken by another worker while queued`。
   - 行為本身比修改前好：修改前會用手上的舊 `Job` 照樣執行一個已停用或已刪除的排程，最後才以 `LeaseLost` 結束。重新啟用時，`set_enabled` 已經把 `next_run_at` 推到下一次 cron，所以跳過這一次也符合使用者的操作。
   - 只是 log 和註解的文字會誤導。可以改成類似「lease 已不在這個 worker 手上（被別人取得、排程被停用或刪除）」。
2. **`tests/session_lease_renewal.rs` 與 `lease_stays_held_during_a_long_compaction` 的尾端時間餘裕只有 0.5 秒。**
   - 兩支測試的 intruder 都在第 3 秒取 lease，工作在第 3.5 秒結束並釋放 lease。
   - 如果 intruder 在負載很高時延遲超過 0.5 秒，會在 lease 釋放後才取得，造成假紅燈。
   - 本次在 load 約 5 的機器上全量跑一次、單獨跑多次，都是綠燈。這是測試穩定性的提醒，不影響證據的有效性。
3. **執行中的 job 被 `schedule enable` 時，lease 也會被清掉。** 這是 BASE 就有的行為，不是本 PR 造成的，也不屬於任何 SCN。
   - 結果：之後每個 tick 都印 `scheduler_lease_renew_lost`。手動 trigger 的 `claim_job` 不檢查 `next_run_at`，所以可以再執行一次。
   - 如果要處理，另開 issue。
4. 實作計畫步驟 6 寫「第一版壓縮測試 `turn_count` 是 0，是假的綠燈」。以現在的測試結構，沒有壓縮的話測試會紅燈（變異 N3），所以第一版的結構應該和現在不同。
   - 這不影響現行證據：現行測試必須真的觸發壓縮才會綠，等於自帶證明。只是那段敘述無法從 repo 重建。

## 已查核維度
- 驗收與證據：
  - **Proof 表與規格**：
    - Proof 表有 SCN-001～008 八列，和核准規格的集合相等：SCN-001～007 在 `5a64393`，SCN-008 在 `a5a0340`。
    - `git diff 5a64393 a5a0340` 中，Scenario 只新增了 SCN-008。SCN-001～007 的內容沒有被改動。
    - SCN-008 的核准來源、日期、Timeline 都寫在 README。TBD-5 寫明使用者的決定（D-2）與追蹤條件。
  - **上次 SHOULD FIX 1（串接守門）已處理**：
    - 在 worktree 用 `CARGO_TARGET_DIR=…/scratchpad/mut-target` 做變異實驗，紀錄在 `scratchpad/rev13b/N*.log`。
    - 基準 N0：`--test session_lease_renewal` 1 passed，用時 3.51 秒。
    - N1，`run_turn` 不包 `hold_session_lease`：整合測試 FAILED，停在 `session_lease_renewal.rs:69` 的「回合執行中 session lease 被拿走」。
    - N2，壓縮呼叫不包續租：`lease_stays_held_during_a_long_compaction` FAILED（`session.rs:335`）。
    - N3，壓縮門檻改成 `turn_count < compact_every_turns + 1`，也就是不會壓縮：同一支測試 FAILED，用時 3.02 秒。這證明現行測試確實觸發了壓縮。沒有壓縮時，`prepare` 會立刻返回，2 秒的 lease 沒人續租，第 3 秒就被拿走。
  - **上次 SHOULD FIX 2（30 秒上限）已處理**：N4 拿掉 `.min(30_000)` 後，`heartbeat_is_capped_at_thirty_seconds` FAILED。
  - **上次 SHOULD FIX 3／4（需決策）已依使用者決定處理**：
    - D-1 → SCN-008 小修。
    - D-2 → TBD-5，寫進 `AGENTS.md`、`docs/cli-reference.md`、README。
  - **SCN-008 的紅燈**：N5 拿掉開始前的續租後，`queued_job_whose_lease_was_taken_is_skipped` FAILED，實際值是 `LeaseLost(ExecutionOutput { success: true, message: "不該執行" })`，和實作者記錄的紅燈一致。
  - **整合測試的環境隔離**：
    - `tests/session_lease_renewal.rs` 是獨立的 test binary，裡面只有一個 `#[tokio::test]`，所以修改環境變數只影響這支測試。cargo 會依序執行各個 test binary，不會互相干擾。
    - reviewer 讀了 `run_turn` 經過的程式碼。它會讀寫的路徑只有兩個：測試自己開的記憶庫，以及 `WUKONG_WORKSPACE/.wukong/skills/superpowers`（`skill_assets::materialize_default_runtime_skills`）。
    - 其他讀取的環境變數只影響 prompt 內容或壓縮判斷，例如 `WUKONG_MEMORIA_HINT`、`WUKONG_BIN`、`WUKONG_SESSION_COMPACT_EVERY_TURNS`。
    - backend 是假的，所以 `WUKONG_AGENT_*` 不起作用。`run_turn` 不碰 chat history 和 settings。
    - 實測方式：把 HOME 指到空的暫存目錄、在另一個空目錄執行，並 unset `WUKONG_WORKSPACE`／`WUKONG_MEMORY_DB`，再用 `strace -f` 追蹤 openat、mkdir、unlink、rename。
    - 實測結果：成功開啟或建立的路徑全都在 `/tmp/.tmp*`（tempdir 與 NamedTempFile）底下。HOME 和 cwd 執行後仍是空的，沒有碰到 `~/.wukong`。紀錄在 `rev13b/itest.strace`。
- 相關失敗面（SCN-008）：
  - **續租參數**：
    - 開始前的續租用 `started_at` 和呼叫端的 `lease_secs`，daemon 傳 `--lease-secs`，CLI 傳 300。之後 `execute_with_heartbeat` 從同一個 `started_at` 起算，每 `min(30 秒, lease/3)` 續租一次。
    - 第一個 job 剛被 claim，lease 一定還在自己手上。CLI trigger 是 `claim_job` 之後立刻續租，兩者相隔不到 1 毫秒，lease 有 300 秒，所以必定成功。
    - 只有在 claim 與續租之間的極短空窗被停用或刪除，才會走到 CLI 的 `NotHeld`。這時印「排程正在執行中」並 exit 0，與 `trigger_unavailable_message` 的慣例一致，可以接受。
  - **`renew_job_lease` 只看 `locked_by`，不看 `locked_until`**：
    - 如果 lease 已經過期、但沒有人接手，續租會成功，job 照常執行。探針 P1 證實：回 `Completed(success)`，留下 1 筆 run。
    - 這符合 SCN-008：SCN-008 的前提是「被另一個行程取得」。這時 job 仍然 due，沒有其他人在跑，執行它是正確的。
    - 不會和 claim 發生競態。SQLite 的單一 UPDATE 是原子操作：續租先做，`locked_until` 就被推到未來，claim 的 `locked_until <= now` 條件不成立；claim 先做，`locked_by` 已經換人，續租回 false。
  - **別人取得後又完成**：`complete_claimed_job` 會把 `locked_by` 設成 NULL，並把 `next_run_at` 推到下一次。探針 P2：回 `NotHeld`，`next_run_at` 維持別人推進後的值，沒有 run 紀錄。
  - **別人取得但還沒完成，或中途當掉**：`locked_by` 是別人，回 `NotHeld`。當掉的情況下，lease 到期後會在之後的掃描被重新 claim，不會遺失。
  - **daemon 的 `continue`**：
    - `NotHeld` 沒有 run，也沒有 output，所以不送 Telegram 通知、不印 succeeded／failed，是正確的。
    - `next_run_at` 由真正執行的那一方推進。`run_scan` 沒有錯誤計數，不需要處理。
    - worker_id 是 `schedulerd-<pid>-<uuid>`，同一個 daemon 在單次掃描內不會再 claim 自己的 job，沒有 ABA 問題。
  - **`ClaimedJobOutcome` 的使用者**：只有 `wukong-schedulerd::run_scan` 和 `wukong-cli::trigger_job`，兩處的 match 都列出了全部的分支，沒有 wildcard。`wukong-web` 和其他 crate 都沒有用到。
  - **`queued_job_whose_lease_was_taken_is_skipped` 的 0 秒 lease**：
    - worker-a 在時間 t 以 0 秒 lease claim，`locked_until = t`。worker-b 在 now ≥ t 時 claim，`locked_until <= now` 一定成立。
    - 時間截斷到秒只會讓兩者相等，不會讓條件失敗，所以測試是確定性的；只有系統時鐘倒退時才會失效。
  - **e2e**：
    - 先 `cargo build -p wukong-cli -p wukong-schedulerd`，再重跑 `sh rev13/batch-dup.sh <repo>/target/debug rev13b/e2e-batch repro13`。HOME、DB、workspace 都在輸出目錄底下。
    - run 只有 #1 first（daemon）和 #2 second（trigger）。daemon 印出 `skipped: lease was taken by another worker while queued`，沒有 `scheduler_lease_renew_lost`，與 `repro13/batch-after` 一致。
- 需求、架構、安全、品質：
  - 改動只在既有的 crate 邊界內：scheduler executor、schedulerd、cli、runtime 測試，依賴方向不變。
  - 沒有新的 SQL，重用既有的 `renew_job_lease`（參數綁定）。
  - 依 code-simplify 檢查新增程式碼：
    - 開始前續租只有 5 行，沒有新抽象。
    - `NotHeld` 是沒有資料的 variant，簡單直接。
    - 測試用 `SlowCompaction`／`SlowStep` 這種最小假 backend，沒有過度設計。
    - `GatewayConfig` 字面值在 session 測試和整合測試各出現一次。session.rs 的測試模組原本沒有 config helper，目前不需要抽出來。
  - 註解文字的精確度見 NIT 1。
- 豁免、待確認與限制：
  - TBD-1～5 的狀態與理由都寫在 README，沒有 Gate 豁免。
  - **PR 說明刪掉「SCN-003 串接只靠重現腳本」那條限制，是恰當的**：串接已經有單元與整合測試守門（N1、N2 證實），重現腳本仍列為補充證據。
  - 上次的 NIT 中，實作計畫明列了三項不處理並附理由：`next_run_at > 1` 恆真、1 秒 lease、drain 段觸發不到。另外兩項沒有列：輪替後多一行 log、`kill.sh` 的時序不對稱。五項都不影響行為。
  - **gateway 的 `invalid_listener_releases_child` 判為既有時序問題，判定合理**：
    - `git diff 53f8243 8aa7e9b` 沒有改到 `crates/wukong-gateway`、`crates/wukong-memory`（gateway 唯一的內部依賴）、`Cargo.lock`、根目錄的 `Cargo.toml`，所以 gateway 的 lib test binary 在 BASE 與 HEAD 是同一份程式碼。
    - reviewer 用 HEAD 的 `target/debug/deps/wukong_gateway-4f218df8a45ebffb` 整支執行 15 次，跑了兩輪：
      - 第一輪 3 次失敗，都是 `health_failure_releases_child`。
      - 第二輪 2 次失敗，都是 `invalid_listener_releases_child`，訊息是 `local_process.rs:361` 的 `assertion failed: error.to_string().contains("loopback")`。
    - 單獨執行同一支測試：閒置 20 次、在 12 核心上同時跑 24 個 busy loop 再執行 30 次，全部通過。
    - 結論：這些 local_process 測試在整支 binary 平行執行時不穩定，失敗的不只是 `invalid_listener_releases_child`，訊息也不只「child still alive」一種。
    - 嘗試在 BASE 的 worktree 重新建置 gateway 測試時，`/tmp/claude-1000` 撞到配額（EDQUOT），連結失敗，所以 BASE 那次沒有執行。依上述原始碼完全相同的事實，不影響這個判定。
    - 建議另開 issue 處理 gateway 測試的穩定性，不在本 PR 範圍。
- 自行執行的驗證（2026-10-21，HEAD `8aa7e9b`，工作區乾淨）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test --workspace --locked --no-fail-fast`：exit 0，passed=655、failed=0、ignored=12，與實作者的紀錄一致。這次執行中 `invalid_listener_releases_child` 也是 ok。指定的受影響 crate（runtime、scheduler、cli、schedulerd）都包含在內。
  - 變異實驗 N0～N5：結果見上，worktree 在 `scratchpad/mut13b`。
  - 探針 P1～P5 放在 worktree 的 executor 測試模組（`rev13b/probe_executor.rs`），5 passed：
    - P1：過期但沒人接手，照常執行。
    - P2：被別人取得並完成，跳過，`next_run_at` 不變。
    - P3：排隊中被停用，跳過。
    - P4：排隊中被重新啟用，跳過。
    - P5：排隊中被刪除，跳過。
  - 整合測試的 strace 隔離檢查：見「驗收與證據」。
  - `batch-dup.sh` e2e：見「相關失敗面」。
  - `gh pr checks 20`：`fmt · clippy · test` pass（1m40s）。
  - 收尾：worktree 已移除（`git worktree list` 只剩主工作區）；repo 除了本報告沒有其他改動。唯一刪除的是本次 BASE 建置失敗時，linker 留下的暫存檔（`mut-target/debug/deps/wukong_gateway-*.tmp*`）。scratchpad 中原本就有的檔案都沒有動。

## 流程判定
PASS

沒有 MUST FIX。上次的 SHOULD FIX 1、2 已補上守門測試，變異實驗 N1～N4 證實這些測試在對應的缺陷下會紅燈，壓縮測試也確實觸發了壓縮（N3）。SHOULD FIX 3、4 已依使用者決定處理（SCN-008 與 TBD-5）。SCN-008 有先紅後綠的紀錄（N5 重現），reviewer 另以探針涵蓋過期未接手、被別人完成、停用、重新啟用、刪除等失敗面，並重跑 e2e 確認。Proof 表與 SCN-001～008 的集合相等，全量測試與 CI 都通過。NIT 只涉及註解與 log 的措辭、測試的時間餘裕，以及一項既有行為，不阻擋判定。
