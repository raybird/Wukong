# 審查報告
- 範圍：完整 PR（#16，`git diff e9af283..3531cce`，4 個 commit：afe5f7b、58b9c18、bca21c5、3531cce）
- Reviewed BASE SHA：e9af283c65157c263131899396143e02c41ca9f7
- Reviewed HEAD SHA：3531ccee2d485e0cff3954e6e1cf27b75a9f0231
- Reviewed patch-id：164913b490b93593f16434fdccafb5497567ba6a（2026-10-20 自行以 `git diff e9af283 3531cce | git patch-id --stable` 重算，一致；`gh pr view 16` 的 headRefOid 亦為 3531cce）
- 獨立 reviewer：Claude Code 隔離 subagent（general-purpose），審查日期 2026-10-20
- Review artifact：docs/issues/issue-0014/review-3531cce.md

## 問題與風險

### MUST FIX
無。

### SHOULD FIX
1. **刪除排程後，衍生 scope 的 opencode session 被保留期清理永久保護，且未揭露**（`需決策`）
   - 位置：`crates/wukong-scheduler/src/store.rs:136`（`remove_job` 只刪 `scheduler_jobs`）、`crates/wukong-cli/src/main.rs:257`、`crates/wukong-web/src/lib.rs:490`；盤點表 `docs/issues/issue-0014/implementation-plan.md:44`。
   - 影響：本 PR 讓每個 Turn 排程在 `agent_session_state` 擁有自己的 `agent:schedule-<id>` 指標。`schedule rm`／Web 刪除不清這個指標，而 `wukong_runtime::session_retention` 的規則是「被 scope 指向就整棵保留」，所以每個被刪除的排程會留下一棵永遠不會被清掉的 opencode session（含 compaction 子 session），保留期報告會把它們算進「受保護且已超過保留期」。變更前排程共用使用者的 session 指標，沒有這種殘留，這是新的失敗面。盤點表該列只寫了「衍生 scope 的 session 同樣受保護」這一半，沒寫刪除後的另一半；`agent:schedule-<id>` 的記憶列同樣成為孤兒（不再被任何回合召回，但仍參與自動維護、仍出現在不帶 scope 的記憶查詢）。規模受「被刪除的排程數」限制，不影響正確性，因此不列 MUST FIX。
   - 待決問題：刪除排程時要不要一併處理它的執行 scope？
   - 可選做法：(a) 接受為已知限制，在 README `## 待確認事項` 新增一項（含追蹤去向）、CHANGELOG 補一句；(b) 本 PR 內讓 `schedule rm` 與 Web 刪除同時 `clear_agent_session("agent:schedule-<id>")`（會跨 wukong-cli、wukong-web，超出已核准範圍，需新增 Scenario 與核准）；(c) 另開 issue 處理 session 與記憶的清理。
   - reviewer 建議：本 PR 採 (a)，盤點表該列補上刪除後的行為；session／記憶清理走 (c)。
2. **`run_unattended_turn` 的 doc comment 被新函式截走**
   - 位置：`crates/wukong-scheduler/src/executor.rs:182-193`。
   - 影響：`scheduled_turn_scope` 插在 `run_unattended_turn` 的 `///` 區塊與函式本體之間，於是「跑一個無人值守回合，並在回合進行中即時處置 opencode 送出的詢問……」整段變成 `scheduled_turn_scope` 的文件，`run_unattended_turn` 失去文件。clippy 不會抓到，屬手術式修改造成的混亂。
   - 建議：把 `scheduled_turn_scope`（連同它自己的 2026-10-20 註解）移到 182 行那段 `///` 之前。
3. **與 #12 的關係描述不正確**
   - 位置：`docs/issues/issue-0014/README.md:104`；PR #16 說明「限制與待確認」最後一項。
   - 影響：兩處都寫「#12 的 SCN-001 前提只剩『使用者明確指定同一 scope』的情況」。但本 PR 後 Turn job 的 `--scope` 只決定擁有者，不論使用者指定什麼，回合一律在 `agent:schedule-<job id>` 執行（`executor.rs:167` 無條件覆寫），使用者沒有任何方式讓排程與聊天共用 session（Q1 的共用旗標沒有被選）。實際上剩下的 lease 競爭是「同一個排程被併行觸發」（daemon 到期執行與 `schedule trigger` 重疊）。這段文字會被拿來重新界定 #12 的範圍，寫錯會誤導下一次決策。
   - 建議：改寫為「排程回合不再與聊天競爭 session lease；剩下的同 scope 競爭只有同一個排程被併行觸發」，PR 說明同步修正。

### NICE TO HAVE
1. SCN-002 的 session 斷言只看事後的 `agent_session_state`，`MockBackend`（`crates/wukong-scheduler/src/executor.rs:354-381`）沒有記錄請求的 `session_id`，所以「最後一棒沒有帶 `ses_user` 出去」沒有直接被斷言。對「整體退回擁有者 scope」這種實際會發生的回歸仍會紅燈（見變異 M0），屬證據持久力，不是假綠燈。可在 mock 記下 `req.session_id`，斷言最後一棒是 `None`。
2. 排程 scope 現在會各自累積記憶並各自被自動維護整併（門檻 40 筆，約 20 次觸發後一次 consolidate 的 LLM 呼叫），Web／memoryd 的 scope 清單也會出現 `agent:schedule-*`。這是設計的自然結果，可在常青文件提一句，避免日後被當成異常。
3. 觀察（非缺失）：#11 讓 planner 帶上一回合以分辨追問；排程每次觸發的提示詞相同，上一回合就是自己上一次的同一題與答案，最後一棒也接續自己的 session。SCN-004 要的正是這種延續，但「planner 把重複題當成已回答的追問而縮小調查」的可能性值得在上線後留意。

## 已查核維度

- 驗收與證據：
  - 核准規格 `afe5f7b:docs/issues/issue-0014/README.md` 與 HEAD 版本的 Gherkin 區段逐字相同；58b9c18、3531cce 只改核准 commit、涉及檔案與狀態欄位。
  - SCN-001：`turn_job_remembers_in_its_own_scope_not_the_owner_scope` 以手寫字面值斷言擁有者 scope 只剩種子 `User: 早安`、`agent:schedule-job-1` 有兩筆。變異 M0 下紅燈（executor.rs:792）。端到端自行重跑一次（見下），記憶全在 `agent:schedule-<uuid>`。成立。
  - SCN-002：任務特別要求確認「不召回」是否只靠上一回合路徑。實作者記錄的紅燈原因確實是上一回合（planner prompt 含 `use tables`），但斷言 `!prompts.iter().any(|p| p.contains("use tables"))` 涵蓋所有棒的 prompt，最後一棒的召回結果也在其中。變異 M1（只把 `turn.rs:177` 召回的 scope 改成 `user:tg-42`，上一回合與 session 維持衍生 scope）下，該測試在 executor.rs:822 的 `use tables` 斷言紅燈，所以召回洩漏有獨立被抓到，不是只靠上一回合。session 部分見 NICE 1。成立。
  - SCN-003：`crates/wukong-schedulerd/src/main.rs` 的 `run_scan` 把 `claim_ready_jobs` 拿到的同一個 `job` 以 `&job` 交給 `run_claimed_job` 與 `notify_turn_result_with_history`；executor 以 `&Job` 接收、只 clone `base_config` 改 `cfg.scope`，沒有改動 job。`notify.rs` 讀 `job.kind` 的 scope。既有 notify 測試直接以 `user:tg-42` 建 Job 驗回送與聊天紀錄，加上這條結構性事實，足以支持「照舊送到擁有者」。CLI `schedule trigger` 本來就不回送，未改變。成立。
  - SCN-004：`each_turn_job_continues_only_its_own_scope` 以記憶（上一回合）驗兩個排程互不相通，M0 下紅燈（executor.rs:857）。session 延續由 `tests/cli_backend.rs` 承擔：假 CLI 在沒有 `-s` 時回傳 `ses_$$`（每次程序不同 pid），第二次觸發取得的 session 與第一次相同，只能來自 `-s` 續接，證據真實。「不同排程 session 互不相通」沒有 session 層級的直接測試，由 scope 隔離推得，可接受。成立。
  - SCN-005：`scheduling_hint_lists_run_history_and_explains_owner_scope` 斷言提示含 `` `wukong schedule runs --id <id>` ``；Gherkin 只要求提示包含指令，agent 是否真的照做無法在無模型下驗證，範圍與規格一致。`ScheduleOp::Runs` 確有 `--id`。成立。
- 相關失敗面：

  | 失敗面 | 輸入／狀態 | 預期 | 現有覆蓋 | 判定 |
  |---|---|---|---|---|
  | 盤點表遺漏的 scope 使用點 | 自行 grep `cfg.scope`／`.scope` 於 runtime turn.rs、session.rs、scheduler、schedulerd main.rs／notify.rs／auto_maintenance.rs／session_retention、wukong-cli trigger／describe、chat history、Telegram dispatch（artifact／附件目錄）、Web schedule_api、memory recall／remember | 回合期間所有讀 `cfg.scope` 處都該用衍生 scope，回送與聊天紀錄用擁有者 | 盤點表 | 回合期間沒有遺漏：Telegram 的 `prepare_artifact_dir`／`prepare_working_attachments`／`record_chat` 只在 Telegram 進入點，排程不經過；`planner_preferences` 由 schedulerd 設 `None` 與 scope 無關；memoria 提示不帶 scope；`workspace_dir()` 是全域。遺漏的是「刪除後」的行為，見 SHOULD FIX 1 |
  | `agent:schedule-<uuid>` 合法性 | job id 由 `store.rs:86` `Uuid::new_v4()` 產生 | `Scope::parse` 接受、DB key 與 lease 正常 | `Scope::parse` 只要求 `agent:` 前綴與非空名稱；端到端實際寫入 `agent:schedule-d3b00a82-…` 與 `agent_session_state` | 合法；scope 不進路徑也不進 opencode 呼叫參數 |
  | 既有排程（擁有者 scope 已有 session 與記憶） | 升級後第一次觸發 | 改在新 scope、沒有上一回合與 session，擁有者 session 不被動到 | SCN-002 測試的前置狀態即「擁有者已有 `ses_user` 與相關記憶」 | 行為正確；CHANGELOG 已揭露「下次觸發即切換、舊紀錄不清除」 |
  | 刪除排程後的孤兒資料 | `schedule rm` 或 Web 刪除已觸發過的排程 | 至少揭露 | 無 | SHOULD FIX 1（需決策） |
  | Maintenance job | `JobKind::Maintenance` | 不受影響 | diff 未動該分支；`prune_maintenance_uses_provided_scope`、`snapshot_maintenance_returns_text` 綠 | 不受影響 |
  | TBD-4 揭露 | 排程回合內再建立排程 | 擁有者為 `agent:schedule-<id>`，`chat_id_from_scope` 回 `None`，不回送也不寫聊天紀錄 | `turn.rs:229` 把 `cfg.scope` 帶進提示 | 揭露準確；補充：提示文字「執行結果會回送給它」在這種情境下不成立，屬同一已接受限制 |
  | 與 #12 的關係 | README／PR 敘述 | 正確描述剩餘競爭 | — | SHOULD FIX 3 |
  | 同一排程併行觸發 | daemon 到期執行與 `schedule trigger` 重疊 | 與變更前同 scope 的行為相同（lease 互斥） | 既有 lease 機制 | 非本 PR 引入；變更前也存在 |

- 需求、架構、安全、品質：
  - 需求：行為與 Q1～Q4 決定一致；不做共用旗標、不清舊記憶、不改召回範圍，皆有紀錄。
  - 架構：只在 `wukong-scheduler` 的 executor 改一行設定、`wukong-runtime` 的 persona 改提示文字；沒有新增相依，沿用既有 scheduler → runtime 方向；無 schema 變更。
  - 安全：無權限或資料外洩面；排程回合讀不到使用者記憶反而縮小了暴露面。
  - 品質：`scheduled_turn_scope` 單一用途但為規則命名、被 AGENTS.md 引用，可接受；測試 helper（`owned_turn_job`、`seed_event`、`event_texts`、`run_turn_job`）各被三支以上測試共用，沒有過度設計。唯一品質缺失是 doc comment 錯置（SHOULD FIX 2）。被移除的 `turn_job_overrides_scope` 斷言的正是本 issue 改變的行為，移除理由成立，`project:Base` 不被寫入的斷言保留在 SCN-002 測試中。
  - 文件：AGENTS.md「排程能力注入」「排程結果回送」、`docs/cli-reference.md`、CHANGELOG 與程式行為一致。
- 豁免、待確認與限制：無 gate 豁免。TBD-4、TBD-5 狀態「不影響本次交付」且有理由與追蹤去向，誠實。實作者揭露的端到端限制（`printf` 假 agent 只走 fallback 文字、Telegram 回送未端到端）屬實，不構成缺失。
- 自行執行的驗證（2026-10-20，工作區為 3531cce、clean）：
  - `cargo fmt --all -- --check`：exit 0。
  - `cargo clippy --all-targets --locked -- -D warnings`：exit 0。
  - `cargo test -p wukong-scheduler`：lib 29 passed；`tests/cli_backend.rs` 1 passed。
  - `cargo test -p wukong-runtime --lib persona::`：9 passed。
  - `cargo test -p wukong-schedulerd`：22 + 2 passed。
  - `cargo test --workspace --locked`：passed=636、failed=0、ignored=11，與 PR 記錄一致。
  - 變異測試（以 `git archive 3531cce` 解到 scratchpad 的兩份快照）：
    - M0：executor 改回 `cfg.scope = <擁有者 scope>` → `cargo test -p wukong-scheduler`：26 passed、3 failed（SCN-001／002／004 三支）。
    - M1：只把 `turn.rs` 召回 scope 寫死成 `user:tg-42` → `cargo test -p wukong-scheduler --lib executor::tests`：13 passed、1 failed（SCN-002，executor.rs:822 的 `use tables` 斷言）。
    - 註：變異快照與 repo 共用 target 目錄，git archive 的檔案 mtime 讓 cargo 一度誤用舊產物；之後已 `cargo clean -p wukong-scheduler -p wukong-runtime` 並在 repo 重跑 `cargo test -p wukong-scheduler` 與 persona 測試，全綠，確認 repo 產物回到 HEAD 版本。
  - 假 agent 端到端：`HOME`、`WUKONG_MEMORY_DB`、`WUKONG_WORKSPACE` 指向 scratchpad 臨時目錄，`WUKONG_AGENT_CMD="printf oracle"`、`WUKONG_AGENT_SERVER_URL=`，未在子命令前放全域旗標。`schedule add-turn --scope user:tg-42` → `schedule trigger --id <id>` 兩次（exit 0）→ `sqlite3` 查 `memories`：只有 `agent:schedule-d3b00a82-…|4`；`agent_session_state` 只有該 scope 一列；`schedule runs --id` 列出兩次 `status=success`；`schedule list` 仍顯示 `kind=turn(scope=user:tg-42)`。

## 流程判定
PASS

理由：無 MUST FIX；SCN-001～005 皆有真實紅燈或結構性證據，變異測試確認關鍵斷言抓得到「退回擁有者 scope」與「只有召回洩漏」兩種回歸。三項 SHOULD FIX 為未揭露的殘留資料（需決策）、doc comment 錯置與 #12 關係敘述錯誤，皆不影響本次行為正確性，建議合併前一併處理。
