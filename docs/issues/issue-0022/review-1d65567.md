# 審查報告

- 範圍：完整 PR（GitHub PR #23，issue #22），TARGET=`main`
- Reviewed BASE SHA：`c08c03d3ccc64193ddc89bf01bae2cf8e64f8ab9`
- Reviewed HEAD SHA：`1d65567257e12e657e3c00e49e2b405b500dd949`
- Reviewed patch-id：`062afe1ca7a8d088828c5efece61487eb736ed9e`（2026-10-21 以 `git diff BASE HEAD | git patch-id --stable` 重算，與預期相同；`gh pr view 23` 的 `headRefOid` 與 HEAD 相同，state=OPEN，base=main）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），非原實作者；審查日期 2026-10-21
- Review artifact：`docs/issues/issue-0022/review-1d65567.md`

## 問題與風險

### MUST FIX

- **M-1 清孤兒 scope 的 UUID 判斷太寬，會刪掉還在的排程 id 的其他寫法**（`crates/wukong-scheduler/src/executor.rs:193`）
  - 現況：`uuid::Uuid::parse_str(id).is_ok() && store.get_job(id).await?.is_none()`。`parse_str` 接受大寫、`{...}`、`urn:uuid:...`、沒有連字號的 32 位 hex；`get_job` 用 `WHERE id = ?1` 精確比對，而排程 id 一律是 `Uuid::new_v4().to_string()`（小寫、有連字號，`store.rs:86`）。所以只要後綴是非正規寫法，`get_job` 一定查不到，scope 一定被刪。
  - 實驗（review worktree，未提交）：建立一個**存在中**的排程，用 `Memory::remember`（正式 API，`agent:` 開頭的任何名稱都是合法 scope）寫入以下四個 scope，跑 `clear_orphan_turn_scopes`：
    - `agent:schedule-<live id 轉大寫>`
    - `agent:schedule-{<live id>}`
    - `agent:schedule-urn:uuid:<live id>`
    - `agent:schedule-<live id 去掉連字號>`

    結果 `cleared=4`，四個都變成 `left=(0, None)`。
  - 影響：不可逆的資料刪除，而且 daemon 每次以常駐模式啟動都會跑。排程本身只會寫正規寫法，所以這些 scope 不可能是「排程被刪掉留下的孤兒」。刪掉它們對功能沒有任何幫助，卻正好落在 README「最大風險」寫的「把查不到排程當成排程不存在」，也違反實作自己採用的「不確定就不刪」。SCN-005 寫「後綴不是 uuid → 不變」，`urn:uuid:`／大括號形式不是排程 id 的寫法。觸發機率低，因為需要使用者或外部工具（`--scope`、`wukong-memoryd /v1/remember`）自己命名這種 scope，但修正只要一行。
  - 建議：只接受正規寫法，例如 `uuid::Uuid::parse_str(id).is_ok_and(|u| u.hyphenated().to_string() == id)`；在 `clearing_orphan_scopes_keeps_live_and_non_uuid_scopes` 的 `kept` 加入上面四種寫法（至少包含 live id 的大寫），先確認測試在現行程式碼下紅燈，再修。AGENTS.md 的「只清後綴是 UUID」改成「後綴是排程 id 格式（小寫、有連字號的 UUID）」。

### SHOULD FIX

- **S-1 `--once` 的標記行為與啟動時的檢查分歧**（`crates/wukong-schedulerd/src/main.rs:69`、`:88`、`:173-185`、`:231-237`）`需決策`
  - 現況：`recover_interrupted_runs(once)` 仍然保留「`--once` 不標記」，`startup_recovery_runs_only_for_daemon_mode` 也照這個斷言。但 `--once` 會呼叫 `run_scan`，而 `run_scan` 現在一開頭就無條件 `interrupt_stale_runs`。結果 `--once` 實際上會標記，連 backend 沒準備好時也會。2026-07-11 的原設計（`355b298` 計畫「daemon-only startup reconciliation」「`--once` … exits without job mutation」）因此失效，現有測試卻看不出來。常駐模式下，第一個 tick 會立刻執行，所以啟動時那次檢查也變成重複的。
  - 正確性：只標記 lease 已到期的 run，所以沒有直接風險。屬於重複邏輯分歧，列改善建議。
  - 待決問題：`--once` 該不該標記殘留的 run？
    - (a) 接受（reviewer 建議：條件只看 lease 到期，`--once` 標記是安全的）：刪掉 `recover_interrupted_runs` 與它的測試，或改測 `run_scan` 在 `--once` 下也標記；AGENTS.md 註明。
    - (b) 不接受：`run_scan` 也要依 `once` 決定是否標記。
- **S-2 Maintenance 排程也改成開始時推進**（`crates/wukong-scheduler/src/executor.rs:230`）`需決策`
  - 現況：`run_claimed_job` 不分 Turn／Maintenance 都會 `advance_next_run`。snapshot／prune／consolidate 中途被中斷時，現在也是跳到下一個 cron 時間點，不再於 lease 到期後補跑。
  - 範圍落差：核准的 SCN-001 Given 是「Turn 排程」，README 變更後流程寫「回合開始執行時」，Q2 的理由（避免重複送出訊息、重複寫檔）也只適用於回合；維護作業大多是冪等的，補跑反而有利。CHANGELOG 寫的是泛稱「排程」，AGENTS.md 沒提到 Maintenance。
  - 待決問題：維護排程中斷後要不要也跳過？
    - (a) 接受，在 CHANGELOG／AGENTS.md 註明；
    - (b) 只對 `JobKind::Turn` 推進。

    reviewer 傾向 (a)：維護作業短、週期常見，差異小。但這是核准範圍外的行為，應該明確記錄。
- **S-3「任一查詢出錯就整批不刪」沒有測試守著**（`crates/wukong-scheduler/src/executor.rs:183-199`；AGENTS.md 與 PR 說明都寫了這句）
  - 變異 ME：把刪除移進迴圈，查到一個孤兒就立刻刪。結果兩支清孤兒測試**都仍是綠的**。原因是 `clearing_orphan_scopes_deletes_nothing_when_jobs_cannot_be_read` 只有一個候選，又把整張 `scheduler_jobs` 刪掉，第一次查詢就出錯，分不出「先查完再刪」與「邊查邊刪」。
  - 屬於證據持久力，本次綠燈仍有效。
  - 建議：準備兩個孤兒 scope，讓排序在後面那個的 `get_job` 出錯（例如插入一筆 id 是該 UUID、`kind` 與 `config_json` 不一致的排程列，讓 `row_to_job` 回錯誤）。斷言前面那個也沒被刪。否則就把文件裡這句刪掉。

### NICE TO HAVE

- **N-1** `crates/wukong-scheduler/src/executor.rs:388-390`：原本屬於 `scheduled_turn_scope` 的 doc comment（`2026-10-20（issue-0014）…`），現在因為中間插入了 `const TURN_SCOPE_PREFIX`，變成附在常數上。建議把常數移到註解之前，或補上自己的註解。
- **N-2** `executor.rs:229-230`：`advance_next_run` 在 `start_run` 之後失敗（資料庫錯誤）時，run 會停在 running，要等 lease 到期才被標成 interrupted，訊息卻是「scheduler process ended before the run completed」，與實際原因不符；`run_scan` 也會因為 `?` 放棄這批其餘的排程（這部分和 `start_run` 失敗時一樣，是既有模式）。可以考慮推進失敗時先 `finish_run` 成 failure。`next_after` 回 `None` 時會把 `next_run_at` 設成 NULL，但已經取得的排程實務上碰不到這種情況。
- **N-3** `crates/wukong-schedulerd/src/main.rs:231-234`：`interrupt_stale_runs` 出錯（例如 SQLITE_BUSY）會讓整輪掃描都不取得排程。建議只印警告、繼續取得，讓標記失敗不影響排程執行。
- **N-4** 文件：
  - 啟動清理也會清掉 #17 之前就被刪除的排程留下的 scope（開發機），`docs/issues/issue-0017/README.md:148` 的 TBD-4（「開發機上已經存在的孤兒 scope 不處理」）實際上已被 #22 處理，但沒有標註；
  - CHANGELOG 括號內只寫「排程在執行中被刪除、行程又剛好結束時留下的」，比實際範圍窄。
- **N-5** 揭露：有兩個執行者同時存在時（daemon 在掃描，同時 CLI `schedule trigger` 正在跑；或者有多個 daemon），回合執行中被停用或重新啟用的排程（`set_enabled` 會清 lease），會在 daemon 下一輪掃描時被標成 interrupted，但它其實還在跑。之後 `finish_run` 不檢查狀態、會覆寫，所以最終狀態正確，只是中途的顯示有誤。改動前只會在 daemon 啟動時發生，現在每個 tick 都可能發生。建議在 AGENTS.md 加一句。

## 已查核維度

### 驗收與證據

- **規格修訂**：`git diff c8efc62 1d65567 -- docs/issues/issue-0022/README.md` 只改了核准 commit 回填、核准來源補一句、狀態表、TBD-AC／TBD-3、Timeline、檔尾狀態。Gherkin 區塊沒有任何差異，核准有效。
- **SCN-001**：`a_turn_interrupted_after_it_started_is_not_run_again`。
  - 模擬方式：`tokio::time::timeout(300ms)` 丟掉 future，SlowBackend 延遲 1000ms，所以丟掉時一定已經 `start_run` 並推進、還沒 `finish_run`。斷言 `next_run_at == next_after(YEARLY, started)` 與 `interrupt_stale_runs == 1` 分別證明推進已發生、run 停在 running，模擬確實落在「start_run 之後、finish_run 之前」。
  - 舊程式碼下會在 `claim_due_jobs` 非空時紅燈。
  - 真實行程：讀了實作者 scratchpad 的 `repro22/{before,after}/summary.txt` 與 `d1.err`、`d2.err`。main 產生了 run 2；本分支不再取得，`next_run_at=1798794000`。
  - 結論：成立。
- **SCN-002**：`a_claimed_turn_that_never_started_is_claimed_again`，characterization，斷言重新取得而且 `next_run_at` 不變；實作者的 M6（改成取得時推進）會讓它紅燈。成立。
- **SCN-003**：
  - 完成時間推算：`a_completed_turn_is_rescheduled_from_its_finish_time`。期望值 `next_after(YEARLY, finished)` 與 `complete_claimed_job` 的算法同源，但這是 characterization，舊程式碼本來就是這個值，不算同義反覆的假綠燈。
  - 手動 trigger：`manually_triggering_a_job_that_is_not_due_keeps_its_next_run`。推導：`next_run_at` 是 `next_after(cron, t0)`，t0 < started < next_run_at，所以 `next_after(cron, started) = next_run_at`。CLI `trigger_job` 走同一個 `run_claimed_job`（`crates/wukong-cli/src/main.rs`），worker 的 lease 是 `MANUAL_TRIGGER_LEASE_SECS = 300`。
  - 結論：成立。
- **SCN-004**：`each_scan_interrupts_runs_whose_lease_expired`。
  - 用連不上的 backend 證明標記發生在取得之前。
  - 變異 MC（拿掉標記）：本次重跑紅燈，`left: (Running, "")`。
  - 真實行程：d2 印出 `marked 1 stale scheduler run(s) as interrupted`。
  - 結論：成立。
- **SCN-005**：
  - memory 的 `scopes_with_prefix` 測試：期望值手寫，涵蓋大小寫、`_`／`%`、四張表。
  - executor：五種 scope 都逐一比對；查詢出錯時不刪。
  - daemon：`--once` 不清。
  - 變異 MB（`get_job` 出錯當成不存在）：本次重跑紅燈。
  - 真實行程：`d1.err` 有 `cleared 1 orphan scheduled-turn scope(s)`，「印出數量」有證據。
  - 結論：核准表列的情況成立，但 M-1 指出非正規 UUID 寫法會誤刪，S-3 指出整批不刪沒有測試。
- **全量檢查**：
  - 本次在獨立 worktree 重跑 `cargo test --locked -p wukong-memory -p wukong-scheduler -p wukong-schedulerd -p wukong-cli`，全過：memory 82、scheduler 49，其餘 integration 全過。
  - `cargo fmt --all -- --check` 通過；三個 crate 的 `cargo clippy --all-targets -- -D warnings` 通過。
  - 全 workspace 675 支的數字沿用實作者紀錄，本次沒有重跑全量。

### 相關失敗面

| 失敗面 | 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|---|
| 前綴比對太寬 | `AGENT:SCHEDULE-…`、`agent:a_1`／`agent:a%1` | 不列出 | memory 測試，期望值手寫；實作者 M3 | 通過 |
| 後綴寫法太寬 | live id 的大寫、`{}`、`urn:uuid:`、無連字號 | 不刪 | 無；本次 probe 證實會刪 | **M-1** |
| 查詢出錯 | `scheduler_jobs` 被刪 | 回錯誤、不刪 | 有；MB 紅燈 | 通過 |
| 部分出錯 | 第二個候選查詢出錯 | 第一個也不刪（文件宣稱） | 無；ME 存活 | S-3 |
| 新增排程與清理競態 | 啟動清理時正好新增排程 | 不刪 | scope 只會在排程存在後由回合寫入，id 不重用；先列 scope 再查排程，查到就留 | 推論，無風險 |
| 排程被持有時刪除與清理競態 | CLI trigger 執行中被刪，daemon 同時啟動清理 | 最終清乾淨 | 回合結束寫回後，`run_claimed_job` 的 `clear_scope_if_deleted` 會再清 | 推論，可接受 |
| 兩份資料庫 | daemon、CLI、Web 各自開 `SchedulerStore` | 同一個 DB | daemon 的 `SchedulerStore` 與 `Memory` 都用 `cfg.db_url`（`main.rs:64-68`）；CLI `run_schedule_op` 與 Web 也用各自的 `db_url` | 通過 |
| `delete_scope` 比對方式 | — | 只用等號 | `store/mod.rs` 的 `delete_scope` 是 `WHERE scope = ?1` | 通過 |
| 推進條件 | 手動 trigger、`complete_claimed_job` 之後 | 下次執行時間不變 | SCN-003 兩支測試；完成後照舊以 `finished_at` 推算 | 通過 |
| cron 計算失敗、推進失敗 | `next_after` 出錯或資料庫錯誤 | run 不卡住 | cron 在 `add_job` 已驗證；資料庫錯誤時 run 停到 lease 到期 | N-2 |
| 執行中停用、刪除、改 cron | `set_enabled` 清 lease、`remove_job` | 不重設 next_run_at、不誤推進 | 推進與完成都要求 `locked_by = worker`；沒有改 cron 的 API | 通過（條件未測，已揭露） |
| 每輪標記誤傷活著的 run | 同一個 daemon 內排隊中的排程 | 不誤標 | 主迴圈依序 await `run_scan`，執行中不會掃描；排隊中的排程沒有 running run | 通過 |
| 每輪標記誤傷活著的 run | 兩個執行者，lease 被 `set_enabled` 清掉 | — | `finish_run` 會覆寫，最終正確 | N-5 |
| 每輪標記誤傷活著的 run | 續租連續失敗超過 lease 時間 | 被標成 interrupted，但完成時覆寫 | — | 可接受 |
| `--once` | 殘留的 running run | 依原設計不動 | 現在會標記 | S-1 |
| Maintenance 排程中斷 | snapshot 中途行程結束 | 核准範圍未定義 | 現在跳到下一個 cron | S-2 |

### 需求、架構、安全、品質

- **架構**：沿既有方向 schedulerd → scheduler → memory，沒有反向依賴；沒有 schema 變更，W-4 不適用。
- **安全**：主要是不可逆刪除，見 M-1。查詢出錯時的失效方向正確。
- **重複邏輯**：
  - `clear_orphan_turn_scopes` 與 `clear_scope_if_deleted` 的失效方向不同，不合併合理；
  - `recover_interrupted_runs` 與 `run_scan` 的標記已經分歧，見 S-1；
  - 沒有過度設計。
- **文件**：
  - AGENTS.md 的敘述與程式一致，但「只清後綴是 UUID」需要隨 M-1 精確化，「查詢出錯就整個不刪」沒有測試（S-3）；
  - 「當掉的回合留下的 session lease 最多占用 900 秒」有真實行程 before 組的 run 2 失敗訊息佐證；
  - CHANGELOG 已揭露一次性提醒的代價。

### 豁免、待確認與限制

- 無 gate 豁免。
- TBD-1、TBD-2、TBD-3 維持「不影響本次交付」，理由與 Q2 及既有行為相符。
- 實作者已揭露 `advance_next_run` 的 `locked_by` 條件沒有測試，不構成缺失。
- 真實行程的原始紀錄放在 session scratchpad（`/tmp/...`），不會長期保存；implementation-plan 已摘錄數值。

### 本次實驗

都在 `~/.cache/wukong-review23-wt` 進行，target 為 `~/.cache/wukong-review23-target`，完成後已移除。

| 實驗 | 結果 |
|---|---|
| probe：非正規 UUID 寫法 | 4 個 scope 全部被刪（M-1） |
| MB：`get_job` 出錯當成不存在 | `clearing_orphan_scopes_deletes_nothing_when_jobs_cannot_be_read` 紅燈 |
| MC：`run_scan` 拿掉標記 | `each_scan_interrupts_runs_whose_lease_expired` 紅燈 |
| ME：邊查邊刪 | 兩支清孤兒測試仍綠（S-3） |

## 流程判定

RETURN TO execute-task

理由：M-1 是 High 風險不可逆刪除路徑上的邏輯錯誤。修正 M-1 並補上會先紅燈的測試後重新審查；S-1、S-2 交協調者依 acceptance.md 逐題確認。
