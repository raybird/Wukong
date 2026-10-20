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

1. 📝 **首要驗證：盤點 scope 使用點，鎖住回送行為**（SCN-003）
   - 產出：本檔新增盤點表，列出一個 Turn job 執行期間讀 `cfg.scope` 或 job scope 的每個位置（`run_turn` 召回／記憶寫入、session lease 與 `agent_session_state`、排程能力提示、檔案互動規則、`session_retention`、回送、聊天紀錄），以及改動後各自該用哪個 scope。
   - 產出：`notify.rs` 的 characterization 測試確認回送與聊天紀錄只依 `job.kind` 的 scope（若既有測試已涵蓋，記錄其名稱，不重寫）。
   - 相依：TBD-3。
   - 完成判準：盤點表中每一列都有結論；出現盤點前未預期、必須改動的使用點時，回 README 更新涉及檔案與風險後再繼續。
2. 📝 **紅燈：executor 的 scope 行為**（SCN-001、SCN-002、SCN-004）
   - 產出：`executor.rs` 測試，用假 backend 與臨時記憶庫執行 Turn job，斷言：
     - 記憶寫入 `agent:schedule-<job id>`，擁有者 scope 的記憶筆數不變（SCN-001）；
     - 擁有者 scope 有相關記憶時，排程回合的 prompt 不含該記憶；session lease 落在衍生 scope（SCN-002）；
     - 兩個 job 各觸發兩次，第二次看得到自己第一次的上一回合、看不到另一個 job 的（SCN-004）。
   - 期望值手寫（例如 `"agent:schedule-job-1"`），不由被測函式產生。
   - 相依：步驟 1。完成判準：在現行程式碼下這些測試以斷言失敗（不是編譯失敗）。
3. 📝 **最小實作** — executor 改用衍生 scope。
   - 先跑 `gitnexus_impact`（`execute_job_inner`）並回報風險等級。
   - 相依：步驟 2。完成判準：步驟 2 的測試轉綠；`cargo test -p wukong-scheduler`、`-p wukong-schedulerd` 全綠。
4. 📝 **排程能力提示**（SCN-005）
   - 產出：`persona.rs` 測試先紅：提示包含 `schedule runs --id`，規則說明 `--scope` 是擁有者；再改提示轉綠。
   - 相依：無（可與步驟 2、3 並行）。完成判準：`cargo test -p wukong-runtime persona::` 綠。
5. 📝 **假 agent 端到端** — 以 `--agent-cmd` 假 agent 走一次 `wukong schedule add-turn` → `schedule trigger`，臨時記憶庫：`wukong memory` 查詢顯示記憶落在 `agent:schedule-<id>`、擁有者 scope 無新增，`schedule runs --id` 印出該次輸出。
   - 相依：步驟 3、4。完成判準：記錄命令與輸出；指定資料庫用 `WUKONG_MEMORY_DB`（地雷 W-5）。
6. 📝 **常青文件與交付**
   - 根目錄 `AGENTS.md`「關鍵設計細節」的「排程能力注入」「排程結果回送」補上擁有者／執行 scope 的區分；`CHANGELOG.md` `[Unreleased]` 新增 Changed 條目；`docs/cli-reference.md` 若描述 `add-turn --scope` 的語意則同步。
   - 全量 `cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`；`gitnexus_detect_changes`。
   - 相依：步驟 1～5。

## 驗收對照
| Scenario | 層級 | 證據 |
|---|---|---|
| SCN-001 | 單元（executor，假 backend） | 步驟 2／3；步驟 5 端到端 |
| SCN-002 | 單元（executor，假 backend） | 步驟 2／3 |
| SCN-003 | 單元（notify） | 步驟 1 的 characterization 測試，步驟 3 後仍綠 |
| SCN-004 | 單元（executor，假 backend） | 步驟 2／3 |
| SCN-005 | 單元（persona） | 步驟 4；步驟 5 確認指令可用 |

正式部署的觀測不列為驗收：改動後排程結果是否仍送達，以步驟 5 與 SCN-003 的測試為準；上線後若要觀察，再依 AGENTS.md 第六節先定義對照組。
