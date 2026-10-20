# 審查報告
- 範圍：完整 PR（重點為上一輪審查後的增量 3531cce..eef170d）
- Reviewed BASE SHA：e9af283c65157c263131899396143e02c41ca9f7
- Reviewed HEAD SHA：eef170d019ad05f635e2cd004b949aeeaf0fc801
- Reviewed patch-id：b4ced2fc6c09c07a0d78e496ec265c0aa3648f79（2026-10-20 自行以 `git diff e9af283 eef170d | git patch-id --stable` 重算，一致；本機 HEAD 與 `gh pr view 16` 的 headRefOid 皆為 eef170d）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-20
- Review artifact：docs/issues/issue-0014/review-eef170d.md
- 前一輪報告：docs/issues/issue-0014/review-3531cce.md

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
無。上一輪三項 SHOULD FIX 皆已處理（逐項見「已查核維度 › 驗收與證據」）。

### NICE TO HAVE
1. **探針判讀可補一句「棒數變少」**
   - 位置：`docs/issues/issue-0014/implementation-plan.md` 步驟 7「判讀」；PR #16 說明的探針表格。
   - 觀察：原始輸出顯示 S-progress 第一次觸發有 4/10 排兩棒（`explorer|none,librarian|none`），第二次觸發 10/10 都是一棒（9 `explorer|none`、1 `librarian|none`）。實作計畫的表格完整列出分布，數字與原始輸出一致，沒有隱瞞；但判讀只說「含 explorer 的比例從 4/10 升到 9/10，沒有縮小調查」，PR 說明的表格又把兩棒壓成「含 explorer 查核」。帶查核能力的角色變多、文件角色 librarian 少掉一棒，兩件事同時發生，結論「planner 沒有把重複題當成已回答的追問」仍成立（沒有任何 `oracle|none`，而 #11 實測 `oracle|none` 正是 0 次工具、直接引用上下文的計畫）。建議判讀補一句「平均棒數由 1.4 降為 1.0，少掉的是 librarian 整理棒」，讓讀者自己權衡。
2. **探針的上一回合答案比真實排程短**
   - 位置：`crates/wukong-runtime/tests/planner_probe.rs` 的 `SCHEDULED_CASES`。
   - 觀察：真實排程報告多半很長，`planner_previous_turn` 會截到 800 字並加「…（已截斷）」；探針的答案都是一兩句。限制段已寫「兩個虛構案例」，屬已揭露範圍，只是截斷後的樣貌沒被覆蓋。不影響本次判定。
3. **#12「同一個排程被併行觸發」的機制可再精確一點**（供 #12 重新界定範圍時參考）
   - 位置：`docs/issues/issue-0014/README.md` 關聯議題 #12 一段（已標「尚未查證」，誠實）。
   - 觀察：daemon 的 `claim_due_jobs` 與 CLI `schedule trigger` 的 `claim_job`（`crates/wukong-scheduler/src/store.rs:216`）都要求 `locked_until` 已過期，所以「daemon 正在跑時有人 trigger」在 job lease 有效期內會被擋下（trigger 印「找不到排程」）。真正能併行的路徑是：回合跑超過 job lease（schedulerd `--lease-secs` 預設 300、trigger 寫死 300，且沒有續租），之後另一方 claim 成功。#12 記錄的成功回合長達 796、1506 秒，這條路徑在實務上可達。此現象在本 PR 之前就存在，不是本 PR 引入。
4. **#17 的回連可直接指到 TBD-6**
   - 位置：GitHub #17「關聯」。
   - 觀察：docs/AGENTS.md「待確認事項」要求追蹤 issue「回連此項」。#17 回連到 #14 與 `review-3531cce.md` SHOULD FIX 1，追得回來，但沒有點名 README 的 TBD-6。補一個字眼即可。
5. 上一輪 NICE TO HAVE 1 仍成立：`MockBackend`（`crates/wukong-scheduler/src/executor.rs`）沒有記錄請求的 `session_id`，SCN-002「最後一棒沒帶 `ses_user` 出去」沒有被直接斷言。增量未改動此處。
6. 上一輪 NICE TO HAVE 2 仍成立：`agent:schedule-*` scope 會各自累積記憶、被自動維護整併，也會出現在 Web／memoryd 的 scope 清單；AGENTS.md 與 `docs/cli-reference.md` 已說明 scope 衍生規則，但沒提到這兩個自然結果。
7. 上一輪 NICE TO HAVE 3 已由探針處理（見下）。

## 已查核維度

- 驗收與證據：
  - 增量範圍：`git diff 3531cce eef170d --stat` 共 6 檔。033ca9b 依 `--name-status` 只新增（A）`docs/issues/issue-0014/review-3531cce.md`，一般檔案模式 100644，父提交為 3531cce。其餘兩個提交（0ba3e65、eef170d）只動 CHANGELOG、`planner_probe.rs`（測試）、`executor.rs`、issue README 與 implementation-plan。
  - 非測試程式碼：`git diff 3531cce eef170d -- crates/wukong-scheduler/src crates/wukong-runtime/src` 只有 `executor.rs` 把 `scheduled_turn_scope`（連同它自己的 2026-10-20 註解）移到 `run_unattended_turn` 的 `///` 之前，五行文件原樣回到 `run_unattended_turn` 上方，函式本體一字未改。上一輪對 SCN-001～005 的程式結論與變異測試（M0、M1）沿用。核准規格 `afe5f7b:docs/issues/issue-0014/README.md` 的 Gherkin 未被增量觸及。
  - SHOULD FIX 1（`需決策`）：README 新增 TBD-6，狀態 `不影響本次交付`，含日期（2026-10-20）、決策來源（使用者選 (a)）、追蹤 #17，符合 docs/AGENTS.md「待確認事項」格式。CHANGELOG 補「已知限制……（#17）」。#17 內容與程式一致：`remove_job` 只 `DELETE FROM scheduler_jobs`（`store.rs:136`）；`referenced_session_ids` 同時讀 `agent_sessions` 與 `agent_session_state`（`crates/wukong-memory/src/store/mod.rs:503`），`session_retention.rs:251` 以它為保護集合；summary 永不被 prune（`crates/wukong-memory/src/lib.rs:828` 測試斷言 "The summary survives (never prunable)"）。#17 的兩個 TBD 與邊界合理。
  - SHOULD FIX 2：已修正，見上。
  - SHOULD FIX 3：README 與 PR 說明都改為「排程回合一律在 `agent:schedule-<job id>`，不論 `--scope` 填什麼都不再與聊天共用 session」，與 `executor.rs:167` 無條件覆寫一致；剩下的同排程併行競爭標為「審查指出、尚未查證」，誠實。舉例措辭的精確度見 NICE 3。
  - NICE TO HAVE 3 → 探針 `scheduled_repeat_plan_distribution`：
    - opt-in：`#[ignore]`，且經 `probe_model()` 未設 `WUKONG_PLANNER_PROBE_MODEL` 即 panic；自行執行三支 ignored 測試都立即 `NotPresent` 失敗，未呼叫模型。
    - `SCHEDULED_HINT` 與 `crates/wukong-scheduler/src/executor.rs:11` 的 `SCHEDULED_TURN_AUTONOMY_HINT` 逐字相同（含開頭 `\n\n`）。輸入組法同 `executor.rs:168`（`format!("{prompt}{HINT}")`）。
    - 上一回合格式：`run_turn` 記憶寫入 `User: {user_text}` 與 `Assistant: {output}`（`turn.rs:360/366`），`user_text` 只切掉 `FILE_RULES_HEADER`，排程提示不會被切掉，所以記憶中的 User 列含 `[無人值守排程]`；`planner_previous_turn` 以 `\n` 串接兩筆（`turn.rs:480`）。探針 `format!("User: {input}\nAssistant: {answer}")` 一致。`preference_hint` 傳 `None`，與 schedulerd 的設定一致（上一輪已查）。唯一差異是答案長度未觸發 800 字截斷（NICE 2）。
    - 原始輸出核對：`scratchpad/probe14/baseline.log`（baseline=true）S-progress 6 `librarian|none`、4 `explorer|none,librarian|none`，S-health 10 `explorer|none`；`repeat.log`（baseline=false）S-progress 9 `explorer|none`、1 `librarian|none`，S-health 10 `explorer|none`；兩檔皆 `test result: ok`，`version.txt` 為 1.18.31。與 implementation-plan 步驟 7 表格完全一致。
    - 判讀與限制：「沒有 `oracle|none`」的判準有依據（#11 步驟 5 實測 `oracle` 為 0 次工具）。限制段明說只測 planner、不測最後一棒會不會沿用 session 裡的舊答案，且只有一個模型、兩個虛構案例、每組 10 樣本，誠實。判讀省略「棒數變少」見 NICE 1。
- 相關失敗面：

  | 失敗面 | 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
  |---|---|---|---|---|
  | 增量夾帶行為變更 | 3531cce..eef170d 的 `crates/*/src` | 只有註解位置 | diff 逐行檢視 | 無行為變更 |
  | 探針誤呼叫真實模型 | `cargo test -- --ignored` 未設模型變數 | 立即失敗 | `probe_model()` expect | 自行執行，3 支皆 NotPresent |
  | 探針與真實輸入脫節 | 排程提示 + autonomy hint；上一回合格式 | 與 executor／run_turn 一致 | 對照原始碼 | 一致；長答案截斷未覆蓋（NICE 2） |
  | 報告與原始輸出不符 | 表格 vs. log | 一致 | 自行計數 | 一致 |
  | 已知限制揭露不實 | TBD-6、#17、CHANGELOG | 與程式行為一致 | 對照 store／retention／prune | 一致 |
  | #12 關係敘述 | README／PR | 正確描述剩餘競爭 | claim 邏輯 | 主結論正確；剩餘競爭的觸發條件可更精確（NICE 3） |

- 需求、架構、安全、品質：增量沒有新需求或架構變動；探針為測試檔，沿用既有探針結構與環境變數，沒有新增相依。doc comment 已歸位。無安全或權限面。
- 豁免、待確認與限制：無 gate 豁免。TBD-4、TBD-5、TBD-6 皆為 `不影響本次交付` 且有日期、決策來源與去向。README Timeline 未記錄審查後的決策（TBD-6、探針），非必要欄位，不列缺失。PR 說明「修正後的增量正在複審」一句在本報告保存後即過時，協調者可同步更新。
- 自行執行的驗證（2026-10-20，工作區 eef170d、clean；只在 repo 本身執行，未建立快照、未共用 target、未執行 cargo clean；target 指紋時間 17:12 晚於所有原始檔，工作區乾淨，快取對應 HEAD 內容）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test -p wukong-scheduler -p wukong-runtime --locked`：runtime lib 79 passed；planner_probe 3 ignored；scheduler lib 29 passed；`tests/cli_backend.rs` 1 passed；managed_actual 1 ignored；exit 0。
  - `cargo test -p wukong-runtime --locked --test planner_probe -- --ignored`（未設模型變數）：`planner_plan_distribution`、`real_followup_turns`、`scheduled_repeat_plan_distribution` 三支皆立即 FAILED，訊息 `set WUKONG_PLANNER_PROBE_MODEL ...: NotPresent`，0.00s。
  - `cargo test --workspace --locked`：exit 0，加總 passed=636、failed=0、ignored=12，與實作者紀錄一致。
  - 未呼叫真實模型，未觸碰 `~/.wukong` 或 `~/Documents/RunWuKong`。

## 流程判定
PASS

理由：增量只有註解歸位、測試檔新增 opt-in 探針與文件更正，不改變上一輪已驗證的程式行為；上一輪三項 SHOULD FIX 皆正確處理（其中 `需決策` 項已有使用者決定、TBD-6 與 #17）；探針與真實輸入一致、表格與原始輸出一致、限制揭露誠實。剩餘項目皆為 NICE TO HAVE。
