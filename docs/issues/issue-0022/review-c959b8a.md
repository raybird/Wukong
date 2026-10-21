# 審查報告

- 範圍：增量 diff（f2be20f..c959b8a，完整 PR 審查見 review-1d65567.md）。GitHub PR #23（issue #22），TARGET=`main`
- Reviewed BASE SHA：`c08c03d3ccc64193ddc89bf01bae2cf8e64f8ab9`
- Reviewed HEAD SHA：`c959b8a8d2ae8009018d0bea2b1e404cf56d319d`
- Reviewed patch-id：`efd4174a78a8fbf26d7abc9b3811856cf89d6a8a`（完整 PR `c08c03d..c959b8a`）。增量 `f2be20f..c959b8a` 的 patch-id 為 `9f82a483707b73e58a4f1c680e6f9db674212a60`。兩者都在 2026-10-21 以 `git diff A B | git patch-id --stable` 重算，與預期相同。`gh pr view 23` 的 `headRefOid` 等於 HEAD，state=OPEN，base=main
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），非原實作者；審查日期 2026-10-21
- Review artifact：`docs/issues/issue-0022/review-c959b8a.md`

## 問題與風險

### MUST FIX

無。

### SHOULD FIX

- **S-4 刪掉啟動時的標記之後，設定檔讀不到時殘留的 run 不會被標記；「第一輪在啟動時立刻執行」不成立**（`crates/wukong-schedulerd/src/main.rs:126-133`、`:212-213`；`AGENTS.md:287`；`docs/issues/issue-0022/implementation-plan.md:145`；PR 說明「每輪掃描都標記殘留的 run」那段）`需決策`
  - 現況：D-1 刪掉 `recover_interrupted_runs` 的前提是「常駐模式的第一輪在啟動時立刻執行，所以啟動時的檢查是重複的」。前一份審查（S-1）也這樣寫。這個前提有兩個例外：
    1. tick 分支先呼叫 `refresh_model_settings`，失敗就 `continue`，`run_scan` 不會執行，所以也不會標記。設定檔一直壞掉時，殘留的 run 會一直停在 running。改動前，啟動時的檢查不受設定檔影響。
    2. `tokio::select!` 沒有 `biased;`，同時就緒的分支是隨機挑的。`maintenance_ticks` 的第一次 tick 也是立刻就緒，所以有可能先跑完一輪記憶維護（consolidate 會呼叫 LLM），之後才輪到掃描。這是 tokio 文件寫的行為，這次沒有實測。
  - 實驗（真實行程，`~/.cache/wukong-review23b-exp`）：分別用 base `c08c03d` 與 HEAD `c959b8a` 建置 `wukong-schedulerd`。在新的資料庫放一筆排程（lease 已到期，`next_run_at` 在未來，不會被取得）和一筆 running run。以常駐模式啟動（`--tick-secs 3600 --agent-cmd "printf x"`），4 秒後查 run 狀態，再用 PID 停掉。

    | 版本 | 設定檔 | run 狀態 | daemon 輸出 |
    |---|---|---|---|
    | base | 不存在 | interrupted | `recovered 1 interrupted scheduler run(s)` |
    | base | `{not json` | interrupted | `recovered 1 …`、`settings reload failed` |
    | HEAD | 不存在 | interrupted | `marked 1 stale scheduler run(s) as interrupted` |
    | HEAD | `{not json` | **running** | 只有 `settings reload failed` |

  - 影響：只影響顯示。設定檔壞掉時本來就不會取得任何排程（既有行為），修好之後的下一輪就會標記。不會重跑，也不會遺失資料，所以不列 MUST FIX。但 AGENTS.md、`run_scan` 的註解、plan 步驟 6 與 PR 說明都寫了「第一輪在啟動時立刻執行」，而且 D-1 是根據這句話做的決定，算是沒有證據的「因為」。
  - 待決問題：設定檔讀不到時，殘留的 run 要不要照樣標記？
    - (a) 接受（reviewer 傾向）：程式不改，把上述四處改成「常駐模式第一次掃描的 tick 在啟動時就緒；設定檔重新讀取失敗的期間不掃描，所以也不標記；可能先跑完一輪記憶維護」。
    - (b) 不接受：tick 分支在 `refresh_model_settings` 之前就標記，或者恢復啟動時的那次檢查。並補一支測試，在設定檔壞掉時也會標記。

### NICE TO HAVE

- **N-6** `crates/wukong-scheduler/src/executor.rs:232-233` 的註解寫「維護排程補跑不會重複對外的動作」，大致成立，但有一個例外。`Memory::consolidate`（`crates/wukong-memory/src/lib.rs:498-523`）對每一批都是先 `insert_memory` 寫入摘要，再 `mark_consolidated`，兩個語句不在同一個交易裡，之後還會 append 到 markdown sink。如果剛好在兩者之間中斷，補跑時會再摘要同一批，產生重複的摘要列。中斷當下那一批的 LLM 摘要呼叫也會重做一次。這是既有行為：改動前所有排程中斷後都會補跑，所以不是本 PR 造成的退步，而且窗口只有毫秒等級。建議把註解改成「維護作業只動本機記憶庫，補跑的代價小」之類比較精確的說法，或者不要寫理由。
- **N-7** `docs/issues/issue-0022/implementation-plan.md:19` 步驟 3 還寫著「在 `recover_interrupted_runs` 旁呼叫 `clear_orphan_turn_scopes`」。這是步驟 3 當時的紀錄，步驟 6 已經寫了刪除，所以不會誤導。要不要加註，由實作者決定。

## 已查核維度

### 前次發現的處理

| 編號 | 處理 | 查核 | 判定 |
|---|---|---|---|
| M-1 | `executor.rs:194` 改成 `Uuid::parse_str(id).is_ok_and(\|u\| u.hyphenated().to_string() == id)`；`kept` 加入存在中排程 id 的大寫、`{}`、`urn:uuid:`、不含連字號四種寫法 | 見下方「M-1 的新判斷」 | 已修正 |
| S-1 | 依 D-1 刪掉 `recover_interrupted_runs` 與測試；`--once` 經由 `run_scan` 標記 | `run_scan` 的標記仍由 `each_scan_interrupts_runs_whose_lease_expired` 守著。拿掉標記的變異讓它紅燈（`left: (Running, "")`）。前提的例外見 S-4 | 已處理，衍生 S-4 |
| S-2 | 依 D-2 只對 `JobKind::Turn` 推進（`executor.rs:234`）；新測試 `an_interrupted_maintenance_job_is_claimed_again` | 見下方「S-2」 | 已處理 |
| S-3 | 換成 `clearing_orphan_scopes_deletes_nothing_when_a_job_cannot_be_read` | 見下方「S-3」 | 已處理 |
| N-1 | doc comment 移回 `scheduled_turn_scope`（`executor.rs:396-398`） | 讀碼確認 | 已處理 |
| N-2 | 不處理 | 推進失敗只會發生在 `start_run` 剛成功之後的資料庫錯誤。後果是 run 等到 lease 到期才被標記，訊息不精確，而且 `run_scan` 放棄這批，和 `start_run` 失敗時一樣。理由成立 | 接受 |
| N-3 | 不處理 | 標記出錯（例如 SQLITE_BUSY）時整輪不取得排程，下一個 tick 會重試；和 `claim_due_jobs` 出錯時一樣。理由成立 | 接受 |
| N-4 | CHANGELOG 範圍改寫；`issue-0017/README.md:148` TBD-4 加註 | 讀碼確認 | 已處理 |
| N-5 | `AGENTS.md:287` 補上兩個執行者同時在跑的情況 | 和 `set_enabled` 清 lease（`store.rs:161-165`）、`finish_run` 不檢查狀態直接覆寫（`store.rs:263-278`）一致 | 已處理 |

### 驗收與證據

- **規格修訂**：`c8efc62` 與 `c959b8a` 的 README Gherkin 區塊逐字相同（抽出 ```` ```gherkin ```` 區塊後 `diff`，沒有差異）。這次只改了「變更後流程」「關鍵差異」，在待確認事項加了 D-1、D-2，並更新 Timeline。核准有效。
- **D-2 與 SCN-001 是否一致**：SCN-001 的 Given 是「一個到期的 Turn 排程」，沒有提到維護排程。D-2 讓維護排程恢復成 base 的行為（中斷後補跑），所以不需要修訂規格，也不違反其他 Scenario。SCN-002、SCN-003 沒有指定排程種類，維護排程在兩者下的行為和 base 相同。
- **SCN-001**：`a_turn_interrupted_after_it_started_is_not_run_again` 仍綠。把推進條件反轉成只對維護排程的變異讓它紅燈（`executor.rs:1040`，lease 到期後又取得），所以 `matches!` 確實作用在 Turn 上。
- **SCN-004**：見上表 S-1。
- **SCN-005**：見下方 M-1、S-3。
- **f2be20f 是否符合 review-evidence.md「本機 artifact 提交」**：父提交是 `1d65567`，`git diff --name-status 1d65567 f2be20f` 只有 `A docs/issues/issue-0022/review-1d65567.md`，模式 100644。`verify-artifact.py --reviewed 1d65567… --head f2be20f` 輸出「OK：只有新增本次報告」。但前次判定是 RETURN，不是 PASS，所以這項檢查只說明 f2be20f 本身沒有夾帶其他變更，不沿用任何 PASS。
- **全量檢查**（本次在獨立 worktree 重跑，`CARGO_TARGET_DIR=~/.cache/wukong-review23b-target`）：
  - `cargo test --locked -p wukong-memory -p wukong-scheduler -p wukong-schedulerd`：memory 82、scheduler lib 50、schedulerd 24，以及各 integration 測試全過，和 plan 步驟 6 記錄的數字相同。
  - `cargo fmt --all -- --check` 通過；`cargo clippy --locked -p wukong-scheduler -p wukong-schedulerd -p wukong-memory --all-targets -- -D warnings` 通過。
  - `cargo test --workspace --locked --no-fail-fast`：exit 0，passed=675、failed=0、ignored=12，和 plan 步驟 6 的紀錄相同。

### M-1 的新判斷

- **排程 id 的寫法**：`scheduler_jobs` 只有 `store.rs:91` 的 `add_job` 一處 `INSERT`，id 是 `Uuid::new_v4().to_string()`（小寫、有連字號）。從建立 scheduler 的 `58889bc` 起，`store.rs` 每一個版本都只有這一處 INSERT，id 一直是這個寫法（逐一 `git show` 核對）。CLI（`wukong-cli/src/main.rs:218`、`:241`）和 Web 都經由 `add_job` 建立排程；Web 直接呼叫 `add_job` 的地方只出現在測試裡。repo 裡沒有匯入、遷移或其他寫入 `scheduler_jobs.id` 的程式或腳本（`git grep scheduler_jobs`，排除 docs）。所以舊資料裡的 id 也都是正規寫法。
- **誤刪**：正規寫法以外的後綴一律保留。逐一變異，每一種寫法都會讓 `clearing_orphan_scopes_keeps_live_and_non_uuid_scopes` 紅燈：接受任何能解析的寫法（`left: 5`）、忽略大小寫、多接受 simple、braced、urn（各 `left: 2`）。所以四種寫法都各自有測試守著。
- **漏清**：只有不是 `add_job` 產生的 scope 才會被保留，而排程不會寫出這種 scope，所以不會漏掉真正的孤兒。
- 使用者用 `--scope agent:schedule-<任意正規 UUID>` 自己命名、又不對應任何排程的 scope 會被清掉。這是 SCN-005 第一列核准的行為，不算缺失。

### S-2

- **正常完成與手動 trigger**：推進只在 Turn 時呼叫。維護排程完成後仍由 `complete_claimed_job` 以完成時間推算，和 base 相同；手動 trigger 的維護排程不推進，也和 base 相同。Turn 的手動 trigger 由 `manually_triggering_a_job_that_is_not_due_keeps_its_next_run` 守著，仍綠。
- **新測試的手法**：
  - 記憶庫是 WAL，`busy_timeout` 5 秒（`wukong-memory/src/store/mod.rs:117`、`:125`）。另一條連線 `BEGIN IMMEDIATE` 拿住寫入鎖後，prune 的刪除會在 busy handler 裡等到 5 秒才失敗，比 300ms 的 timeout 長得多。
  - 如果 prune 很快就失敗或完成，`timeout(...).is_err()` 會紅燈；如果 timeout 早在 `start_run` 之前觸發，`only_run` 會紅燈。所以時序出問題時只會偶發紅燈，不會假綠。
  - 再取得的時間用固定的 `started + 6`。續租發生在 300ms 內，`locked_until` 最多是 `started + 6`，條件 `locked_until <= now` 仍成立。
  - 排程庫和記憶庫是兩個不同的暫存檔，寫入鎖不影響排程庫。
- **鎖會不會殘留**：持鎖的連線沒有 commit，測試結束時隨 pool 一起關閉，交易就回滾。被丟掉的 prune 語句還在 sqlx 的 worker thread 上，最多等 5 秒，但它碰的是這支測試自己的暫存檔，不影響其他測試。
- **重複執行**：單獨跑 30 次全綠；8 份 scheduler lib 測試同時跑（每份 16 threads）× 3 輪，全綠。
- **變異**：無條件推進讓新測試紅燈（`executor.rs:1107`，`left: 0, right: 1`）；只對維護排程推進讓新測試與 SCN-001 都紅燈。

### S-3

- 新測試讓排在後面的排程列 `kind` 與 `config_json` 對不上，使 `get_job` 出錯，測試裡也先 `assert!(store.get_job(..).is_err())` 確認。`scopes_with_prefix` 依 `ORDER BY scope` 排序（`wukong-memory/src/store/mod.rs:792`），所以 `00000000-0000-4000-8000-…` 一定排在隨機產生的 v4 id 前面。
- 變異結果（都確認了是哪一個斷言紅的）：
  - ME 邊查邊刪：在逐一檢查 scope 的斷言紅燈，`agent:schedule-00000000-…` 變成 `(0, None)`；
  - MB 出錯當成不存在：在 `is_err()` 斷言紅燈；
  - MK 跳過出錯的候選、其餘照刪：在 `is_err()` 斷言紅燈。
- 刪掉舊測試（刪整張表）沒有失去保障：舊測試能抓到的變異（出錯當成不存在），新測試也都抓得到；新測試另外還抓得到邊查邊刪。兩者都不涉及 `scopes_with_prefix` 本身出錯的情況，所以這方面沒有差別。

### 相關失敗面

| 失敗面 | 輸入／狀態 | 預期 | 覆蓋 | 判定 |
|---|---|---|---|---|
| 非正規寫法的後綴 | 存在中排程 id 的大寫、`{}`、`urn:uuid:`、無連字號 | 不刪 | 測試；五個變異都紅燈 | 通過 |
| 排程 id 的來源 | 舊資料、CLI、Web、匯入 | 都是小寫、有連字號 | 只有 `add_job` 寫入，歷史上的每個版本都一樣 | 通過 |
| 部分查詢出錯 | 第二個候選的 `get_job` 出錯 | 第一個也不刪 | 新測試；ME、MK 紅燈 | 通過 |
| 維護排程中途中斷 | prune 執行中 future 被丟掉 | lease 到期後補跑，`next_run_at` 不變 | 新測試；MS1、MS2 紅燈；重複跑 54 次全綠 | 通過 |
| 維護排程補跑的副作用 | consolidate 在寫入摘要與標記來源之間中斷 | — | 既有行為，窗口毫秒等級 | N-6 |
| 維護排程正常完成、手動 trigger | — | 和 base 相同 | 讀碼：不呼叫推進，其餘路徑沒變 | 通過 |
| `--once` 的標記 | 殘留的 running run | 標記（D-1） | `--once` 呼叫同一個 `run_scan`，由 `each_scan_…` 守著 | 通過 |
| 常駐模式啟動、設定檔讀不到 | 殘留的 running run | base 會在啟動時標記 | 真實行程：HEAD 停在 running | S-4 |
| 常駐模式啟動、維護 tick 先被選到 | 殘留的 running run | 立刻標記 | `select!` 隨機挑選分支，標記會晚一輪維護的時間；未實測 | S-4 |
| 依賴舊的輸出 | `recovered N interrupted scheduler run(s)` | 沒有部署腳本依賴 | `git grep` 只出現在 2026-07-10 的歷史計畫文件裡 | 通過 |

### 需求、架構、安全、品質

- **架構**：依賴方向不變，沒有 schema 變更。
- **安全**：不可逆刪除的判斷收窄成正規寫法，失效方向是保留，符合「不確定就不刪」。
- **重複邏輯**：重複的啟動檢查已經刪掉；`due_yearly_turn` 抽出 `due_yearly_job`，供兩支測試共用，沒有過度設計。
- **文件**：
  - AGENTS.md、CHANGELOG、README、PR 說明對 M-1、D-2、N-5 的描述和程式一致。
  - 「常駐模式的第一輪在啟動時立刻執行」不精確，見 S-4。
  - 「維護排程補跑不會重複對外的動作」有例外，見 N-6。

### 豁免、待確認與限制

- 無 gate 豁免。D-1、D-2 已記錄使用者 2026-10-21 的決定；S-4 會影響 D-1 的前提，交由協調者確認。
- TBD-1～TBD-3 的狀態維持不變，這次的改動不影響它們。
- 本次沒有重跑 issue 實作者的 kill -9 真實行程對照，SCN-001 與 SCN-005 的真實行程證據沿用前次審查的查核結果。這次的改動不碰那條路徑：Turn 的推進只多了種類判斷，清孤兒只把條件收窄。

### 本次實驗

都在 `~/.cache/wukong-review23b-wt`（detached `c959b8a`）進行，target 是 `~/.cache/wukong-review23b-target`，真實行程的資料放在 `~/.cache/wukong-review23b-exp`。完成後都已移除。

| 實驗 | 結果 |
|---|---|
| MM1 後綴只要能解析就接受 | keeps 測試紅燈（`left: 5`） |
| MM2～MM5 忽略大小寫／接受 simple／braced／urn | keeps 測試各自紅燈（`left: 2`） |
| MS1 無條件推進 | 維護排程測試紅燈 |
| MS2 只對維護排程推進 | 維護排程測試與 SCN-001 紅燈 |
| ME 邊查邊刪 | 查詢出錯測試在逐一檢查 scope 的斷言紅燈 |
| MB 出錯當成不存在 | 查詢出錯測試在 `is_err()` 斷言紅燈 |
| MK 跳過出錯的候選 | 查詢出錯測試在 `is_err()` 斷言紅燈 |
| MC `run_scan` 拿掉標記 | `each_scan_interrupts_runs_whose_lease_expired` 紅燈 |
| 維護排程測試重複執行 | 單獨 30 次 + 並行 24 次，全綠 |
| 真實行程：設定檔壞掉時啟動 | base 標記；HEAD 停在 running（S-4） |

## 流程判定

PASS

理由：前次的 M-1 已修正，四種非正規寫法各自都有測試守著；S-1～S-3 依使用者的決定處理，都有會紅燈的測試。這次沒有 MUST FIX。S-4 是 D-1 前提的例外，只影響設定檔壞掉時的顯示，交由協調者依 acceptance.md 確認要不要處理；N-6、N-7 不阻擋。
