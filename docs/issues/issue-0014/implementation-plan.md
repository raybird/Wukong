# Issue 14 實作計畫

風險與首要驗證見 [README](./README.md#風險與首要驗證)。

## 現況（2026-10-20 讀碼確認）

- `crates/wukong-runtime/src/persona.rs` 的 `scheduling_capability_hint(scope, bin)` 讓 agent 以 `--scope "<當前 scope>"` 建立排程，除非使用者另外指定。
- `JobKind::Turn { scope, prompt }`（`crates/wukong-scheduler/src/job.rs`）序列化在 `scheduler_jobs.config_json`，沒有獨立的 scope 欄位。
- `execute_job_inner`（`crates/wukong-scheduler/src/executor.rs`）對 Turn job 設 `cfg.scope = scope.clone()` 後執行無人值守回合，所以召回、session lease、記憶寫入都落在 job scope。
- `notify_turn_result` 與 `notify_turn_result_with_history`（`crates/wukong-schedulerd/src/notify.rs`）直接讀 `job.kind` 的 scope，以 `chat_id_from_scope` 還原 chat id 並寫入該 scope 的聊天紀錄；不經過 `cfg.scope`。
- `wukong schedule runs --id <id> --limit <n>`（`crates/wukong-cli/src/main.rs`）已能印出每次執行的完整輸出（`scheduler_runs.message` 即回合輸出文字）。
- job id 是 UUID v4，`agent:schedule-<uuid>` 可通過 `Scope::parse`。

## 設計

- **擁有者與執行 scope 分開**：`JobKind::Turn.scope` 的資料不變，語意改為擁有者 scope。executor 對 Turn job 改設 `cfg.scope = format!("agent:schedule-{}", job.id)`（集中在一個小函式，供測試與文件引用）。回送與聊天紀錄本來就讀 `job.kind` 的 scope，不需修改。
- **不讀使用者記憶**（Q2）：`Scope::ancestry` 不變，排程回合自然只看自己的 scope 與 global。
- **查結果**（Q3）：排程能力提示新增一行 `{bin} schedule runs --id <id>`，並把規則改寫成「`--scope` 是排程的擁有者，結果會回送給它」。
- **既有排程**：不遷移資料。下次觸發即改用衍生 scope；它們原本與使用者共用的 session 不再被接續。
- **不做**：add-turn 的共用旗標（Q1 未選）、既有記憶清理（Q4）、召回範圍變更。

## 實作步驟

所有步驟在 SCN-001～005 核准（TBD-3）後才開始。

1. ✅ **首要驗證：盤點 scope 使用點，鎖住回送行為**（SCN-003）
   - 產出：本檔新增盤點表，列出一個 Turn job 執行期間讀 `cfg.scope` 或 job scope 的每個位置（`run_turn` 召回／記憶寫入、session lease 與 `agent_session_state`、排程能力提示、檔案互動規則、`session_retention`、回送、聊天紀錄），以及改動後各自該用哪個 scope。
   - 產出：`notify.rs` 的 characterization 測試確認回送與聊天紀錄只依 `job.kind` 的 scope（若既有測試已涵蓋，記錄其名稱，不重寫）。
   - 相依：TBD-3。
   - 完成判準：盤點表中每一列都有結論；出現盤點前未預期、必須改動的使用點時，回 README 更新涉及檔案與風險後再繼續。
   - 盤點結果（2026-10-20，`grep` `cfg.scope`／`.scope` 於 `wukong-runtime`、`wukong-scheduler`、`wukong-schedulerd`、`wukong-cli`）：

     | 使用點 | 位置 | 改動後使用 | 結論 |
     |---|---|---|---|
     | 召回範圍 | `crates/wukong-runtime/src/turn.rs` `run_turn_traced_with_attachments`（`RecallQuery.scope`） | 衍生 scope | SCN-002 要的行為 |
     | 上一回合（輔助棒與 planner） | 同上，`previous_turn_records` | 衍生 scope | SCN-004 要的行為 |
     | 記憶寫入 | 同上，`turn_key` 與 `RememberInput.scope` | 衍生 scope | SCN-001 要的行為 |
     | session lease／續接／compaction | `turn.rs` 與 `crates/wukong-runtime/src/session.rs` | 衍生 scope | SCN-002、SCN-004 要的行為 |
     | 排程能力提示 | `turn.rs` → `persona::scheduling_capability_hint` | 衍生 scope | TBD-4 已接受的限制 |
     | Telegram 回送、聊天紀錄 | `crates/wukong-schedulerd/src/main.rs` 傳 `&job` 給 `notify_turn_result_with_history`，讀 `job.kind` 的 scope | 擁有者 scope | 不經過 `cfg.scope`，不需修改 |
     | `schedule trigger` | `crates/wukong-cli/src/main.rs` `trigger_job` → `run_claimed_job` | 與 daemon 同一條 executor 路徑 | 不回送，不需修改 |
     | `schedule list` 顯示 | `describe_job_kind` | 擁有者 scope | 不需修改 |
     | Maintenance job | `executor.rs` `JobKind::Maintenance` | 原 scope | 不在本 issue 範圍，未改 |
     | session 保留期清理 | `wukong_runtime::session_retention` | 依 `agent_sessions` 指向判斷，與 scope 名稱無關 | 衍生 scope 的 session 同樣受保護 |

     沒有盤點前未預期的使用點；檔案互動規則只由 Telegram 進入點附加，排程回合不帶。
   - 回送的 characterization：`crates/wukong-schedulerd/src/notify.rs` 既有測試 `delivers_success_result_as_html_with_header`、`records_scheduled_telegram_result_in_chat_history`、`delivers_brief_plain_text_on_failure`、`skips_non_telegram_scope` 直接以 job 的 scope 建 `Job` 呼叫回送，已鎖住「回送與聊天紀錄只依 job scope」，不另寫新測試。步驟 3 之後全綠（見步驟 6）。
2. ✅ **紅燈：executor 的 scope 行為**（SCN-001、SCN-002、SCN-004）
   - 產出：`executor.rs` 測試，用假 backend 與臨時記憶庫執行 Turn job，斷言：
     - 記憶寫入 `agent:schedule-<job id>`，擁有者 scope 的記憶筆數不變（SCN-001）；
     - 擁有者 scope 有相關記憶時，排程回合的 prompt 不含該記憶；session lease 落在衍生 scope（SCN-002）；
     - 兩個 job 各觸發兩次，第二次看得到自己第一次的上一回合、看不到另一個 job 的（SCN-004）。
   - 期望值手寫（例如 `"agent:schedule-job-1"`），不由被測函式產生。
   - 相依：步驟 1。完成判準：在現行程式碼下這些測試以斷言失敗（不是編譯失敗）。
   - 產出：`turn_job_remembers_in_its_own_scope_not_the_owner_scope`（SCN-001）、`turn_job_neither_recalls_nor_resumes_the_owner_scope`（SCN-002）、`each_turn_job_continues_only_its_own_scope`（SCN-004）。原有的 `turn_job_overrides_scope` 斷言 session 落在 job scope，正是本 issue 改變的行為，由 SCN-002 的測試取代。
   - 紅燈（2026-10-20）：`cargo test -p wukong-scheduler --lib executor::tests` 11 passed、3 failed：
     - SCN-001：`user:tg-42` 的 event 為 `["Assistant: done", "User: summarize atlas progress…", "User: 早安"]`，期望只有 `["User: 早安"]`。
     - SCN-002：planner prompt 含擁有者 scope 的 `use tables`（經上一回合帶入）。
     - SCN-004：`alpha` 第二次觸發的 prompt 含 `beta-first`。
3. ✅ **最小實作** — executor 改用衍生 scope。
   - 先跑 `gitnexus_impact`（`execute_job_inner`）並回報風險等級。
   - 相依：步驟 2。完成判準：步驟 2 的測試轉綠；`cargo test -p wukong-scheduler`、`-p wukong-schedulerd` 全綠。
   - 影響分析（2026-10-20）：`gitnexus_impact(execute_job_inner)` 為 LOW（直接呼叫者 `execute_job`；上游 `run_claimed_job` → `run_scan`、CLI `trigger_job`）。
   - 實作：`execute_job_inner` 的 Turn 分支改設 `cfg.scope = scheduled_turn_scope(&job.id)`（`agent:schedule-<job id>`）。
   - 綠燈：步驟 2 的三支測試通過。`crates/wukong-scheduler/tests/cli_backend.rs` 的 `scheduler_executor_completes_cli_turn_and_resumes` 原本讀 job scope 的 session 而失敗，改讀 `agent:schedule-issue7`，並斷言擁有者 scope 沒有 session。它驗證的「兩次觸發接續同一個 session」保留下來，作為 SCN-004 的 session 證據。`cargo test -p wukong-scheduler -p wukong-schedulerd -p wukong-cli` 全綠。
4. ✅ **排程能力提示**（SCN-005）
   - 產出：`persona.rs` 測試先紅：提示包含 `schedule runs --id`，規則說明 `--scope` 是擁有者；再改提示轉綠。
   - 相依：無（可與步驟 2、3 並行）。完成判準：`cargo test -p wukong-runtime persona::` 綠。
   - 影響分析：`gitnexus_impact(scheduling_capability_hint)` 為 CRITICAL（所有進入點每一回合的最後一棒都會注入，18 條流程）。只改提示文字、不改簽名；既有兩支提示測試仍綠。
   - 紅燈：`scheduling_hint_lists_run_history_and_explains_owner_scope` 失敗（8 passed、1 failed）。綠燈：提示新增 `{bin} schedule runs --id <id>`，並說明 `--scope` 是擁有者、排程回合在獨立 scope 執行、查排程結果要用指令不要憑記憶；9 passed。`--id` 旗標已對照 `crates/wukong-gateway/src/cli.rs` 的 `ScheduleOp::Runs`。
5. ✅ **假 agent 端到端** — 以 `--agent-cmd` 假 agent 走一次 `wukong schedule add-turn` → `schedule trigger`，臨時記憶庫：`wukong memory` 查詢顯示記憶落在 `agent:schedule-<id>`、擁有者 scope 無新增，`schedule runs --id` 印出該次輸出。
   - 相依：步驟 3、4。完成判準：記錄命令與輸出；指定資料庫用 `WUKONG_MEMORY_DB`（地雷 W-5）。
   - 執行（2026-10-20）：臨時目錄，`HOME`、`WUKONG_MEMORY_DB`、`WUKONG_WORKSPACE` 都指向它，`WUKONG_AGENT_CMD="printf oracle"`、`WUKONG_AGENT_SERVER_URL=` 空值。依序 `wukong schedule add-turn --name e2e --cron "0 9 * * *" --scope "user:tg-42" --prompt "summarize atlas progress"` → `schedule trigger --id <id>`（exit 0）→ `memory snapshot` → `schedule runs --id <id>` → `schedule list`。
   - 結果：`memory snapshot` 總計 2 筆，全部在 `agent:schedule-813cae4f-…`，`user:tg-42` 沒有任何記憶；`schedule runs --id` 印出 `status=success` 與該次輸出；`schedule list` 仍顯示 `kind=turn(scope=user:tg-42)`。
   - 限制：`printf` 假 agent 不輸出 opencode 的 JSON 串流，回合文字是 fallback `(本回合未產生文字輸出)`，所以這一步只證明 scope 與指令可用，不證明回答內容。Telegram 回送沒有在端到端執行（需要 token），由 SCN-003 的單元測試承擔。
6. ✅ **常青文件與交付**
   - 根目錄 `AGENTS.md`「關鍵設計細節」的「排程能力注入」「排程結果回送」補上擁有者／執行 scope 的區分；`CHANGELOG.md` `[Unreleased]` 新增 Changed 條目；`docs/cli-reference.md` 若描述 `add-turn --scope` 的語意則同步。
   - 全量 `cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`；`gitnexus_detect_changes`。
   - 相依：步驟 1～5。
   - 文件：根目錄 `AGENTS.md`「排程能力注入」「排程結果回送」、`docs/cli-reference.md`「排程子命令」、`CHANGELOG.md` `[Unreleased]` Changed 已更新。
   - 全量檢查（2026-10-20）：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked` passed=636 failed=0 ignored=11（main 為 633：新增 3 支 executor 測試與 1 支提示測試，移除 1 支被取代的測試）。
   - `gitnexus_detect_changes`：實際改動限於 `execute_job_inner`、新函式 `scheduled_turn_scope`、`scheduling_capability_hint` 的文字與上述測試；其餘列出的符號只是行號位移。
   - code-simplify：衍生規則集中在 `scheduled_turn_scope` 一處；測試共用 `owned_turn_job`、`seed_event`、`event_texts`、`run_turn_job`，沒有重複。no-op。

## 驗收對照
| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | 單元（executor，假 backend） | 步驟 2／3；步驟 5 端到端 |
| SCN-002 | 單元（executor，假 backend） | 步驟 2／3 |
| SCN-003 | 單元（notify） | 步驟 1 的 characterization 測試，步驟 3 後仍綠 |
| SCN-004 | 單元（executor，假 backend） | 步驟 2／3 |
| SCN-005 | 單元（persona） | 步驟 4；步驟 5 確認指令可用 |

正式部署的觀測不列為驗收：改動後排程結果是否仍送達，以步驟 5 與 SCN-003 的測試為準；上線後若要觀察，再依 AGENTS.md 第六節先定義對照組。
