# 實作計畫

2026-11-24 建立。本計畫是任務的權威來源；GitHub #44 的範圍清單是建檔摘要。風險與首要驗證依 [README](README.md#風險與首要驗證)，現況與取捨見 [requirement-analysis.md](./requirement-analysis.md)、[technical-analysis.md](./technical-analysis.md)。Phase 內的 Task 由 decompose 細化在本檔。

所有探測與演練用 #30 一致性副本的再複製、隔離容器（不需要模型的探測加 `--network none`），正式部署 RunWuKong 不碰（project.md gate）。

## 設計方案

- `AiBackend` trait 對 runtime 的介面不變；V2 的差異收在 `wukong-gateway` 內。
- `OpencodeServerBackend` 改打 `/api/...`、解 `data` 包裝與 cursor 分頁；prompt 改為送出後等事件；事件對應（`opencode_server/event_map.rs`）改用 V2 事件名稱；取消、壓縮、換模型、權限、form 換成 V2 端點。
- `LocalProcessBackend` 等 `server listening on`，自設密碼並帶 basic auth。
- `AgentCliBackend` 的 opencode 路徑加 `--standalone`，NDJSON 解析改成 V2 形狀；假 agent 不加任何 opencode 旗標（維持現狀）。
- 啟動或第一次呼叫前檢查 `opencode --version`，1.x 時回傳明確錯誤（SCN-011）。
- installer 依 `data-compatibility.json` 的 `backupRequired` 與 `affectedState` 備份 volume，備份路徑記進 `.wukong-release`。

## 實作步驟

2026-11-24 decompose：README 的風險策略（High，首要驗證 SCN-001）仍成立，Phase 1 先於所有實作。Task ID 固定，不重編。同日規格修訂（SCN-003、SCN-007 修訂，SCN-016、SCN-017 新增）後補拆：Task 2.3、3.2 的範圍與完成判準跟著更新，兩者都尚未開始，沒有舊證據失效。

### 責任對照

| Scenario | 責任 Task | 支援 |
|---|---|---|
| SCN-001 | 1.1 | 6.1 |
| SCN-002 | 2.4 | 1.3、2.2、6.1 |
| SCN-003 | 3.2 | 2.1、6.1 |
| SCN-004 | 2.1 | 3.2 |
| SCN-005 | 2.3 | 1.3、6.1 |
| SCN-006 | 2.5 | 1.3、6.1 |
| SCN-007 | 2.3 | 1.3 |
| SCN-008 | 2.1 | 6.1 |
| SCN-009 | 3.1 | 6.1 |
| SCN-010 | 2.6 | 1.2 |
| SCN-011 | 2.7 | — |
| SCN-012 | 4.1 | — |
| SCN-013 | 5.1 | 6.1 |
| SCN-014 | 5.2 | — |
| SCN-015 | 5.3 | 6.1 |
| SCN-016 | 2.3 | 1.3 |
| SCN-017 | 3.2 | 5.1 |

推導行為 (d)（binary 模式 docs）由 5.4 負責；GitHub #44 驗收的版本對照由 6.2 負責；常青文件由 6.3 負責。

## Phase 1 — 首要驗證：V1 session 可在 V2 接續，取得 V2 真實契約樣本

SCN-001 不成立時停在本 Phase，依 acceptance.md 修訂規格並重新核准，Phase 2 以後都不開始。

### Task 1.1 — V1 session 在 V2 遷移後可接續
- 產出與範圍：`docs/issues/issue-0044/scripts/` 的探測腳本與結果紀錄（issue 目錄內新檔 probe-session.md）；用 #30 副本的再複製，隔離容器
- 相依：無
- 驗收編號：SCN-001
- 完成判準：README「完成證據」全部成立：memory.db 的 agent_session_state 指向的每個 session 在 V2 的 `GET /api/session/{id}` 都查得到，`/context` 含遷移前的訊息；對其中一個送 prompt 後 session ID 不變、訊息數增加；不存在的 ID 回查不到（負向對照）
- 驗證：契約探測（真實 V2 對副本的最小請求）。2026-11-24 改為全程 `--network none`：接續與否看 prompt 有沒有進同一個 session、context 有沒有保留原本的訊息，不需要模型回答，對話內容也不必送到外部模型
- 狀態：✅ 已完成
- 證據：2026-11-24 [probe-session.md](probe-session.md)，[scripts/probe-session.sh](scripts/probe-session.sh)。4 個 scope 的 session 在 V2 都回 200，`/context` 訊息數與 V1 最後一次壓縮後的訊息數相同（27、66、2、24）；不存在的 ID 回 404；對其中一個送 prompt，回應的 sessionID 等於原 session，context 從 27 則變 29 則且原本的 27 則原樣保留。附帶觀察：沒有網路時 session 60 秒後仍在執行、沒有錯誤（README TBD-3）

### Task 1.2 — V2 CLI 路徑契約
- 產出與範圍：probe-cli.md：`opencode run --standalone --format json` 的事件形狀、session ID 取得方式、`--print-logs` 的上游錯誤輸出、結束後有無殘留程序；不加 `--standalone` 時的程序對照；帶 `--dangerously-skip-permissions` 時的行為（解決 TBD-1）
- 相依：無
- 驗收編號：支援 SCN-010；TBD-1
- 完成判準：每項都有實際命令與輸出；殘留程序以 `ps` 比對前後；TBD-1 記下結論與日期
- 驗證：契約探測，隔離 HOME 與 XDG 目錄
- 狀態：✅ 已完成
- 證據：2026-11-24 [probe-cli.md](probe-cli.md)，[scripts/probe-cli.sh](scripts/probe-cli.sh)。V2 拒絕 `--log-level ERROR`（只收小寫）；`--format json` 的文字、思考、步驟事件與 V1 同名，錯誤改為 `error.type`；`-s` 接續同一 session；`--standalone` 結束後沒有殘留程序；TBD-1 已解決（`--dangerously-skip-permissions` 仍可用，效果同 `--auto`）；上游連不到時 V2 一直送 `retry` 不結束，V1 則 65 秒後回錯誤（TBD-3）

### Task 1.3 — V2 事件與互動樣本
- 產出與範圍：`crates/wukong-gateway` 測試 fixture 用的 V2 原始樣本（文字串流、思考、工具、完成、中止、上游具名錯誤、permission.asked、form），取自真實 V2 server 的 `/api/event`；以 #30 的 v2-probe.sh 為起點
- 相依：無
- 驗收編號：支援 SCN-002、SCN-005、SCN-006、SCN-007
- 完成判準：每種事件至少一份真實樣本存進 fixture，記取得命令與 V2 版本；上游錯誤樣本來自真實錯誤（額度用完或無效模型），不手寫
- 驗證：等價證據（樣本是之後測試的獨立真相來源，避免同義反覆）
- 狀態：✅ 已完成
- 證據：2026-11-24 [probe-events.md](probe-events.md)，[scripts/capture-events.sh](scripts/capture-events.sh)、[scripts/capture-scenario.py](scripts/capture-scenario.py)。六個 server 情境都在真實 V2 上通過（文字加工具、權限、form、中止、模型不存在、上游連不到的 retry），樣本存在 `crates/wukong-gateway/tests/fixtures/opencode-v2/`（6 份 server、5 份 CLI）；V1 到 V2 的事件對照表在 probe-events.md。上游錯誤樣本為真實的 provider.no-route 與 provider.transport；限流與模型下架沒有真實樣本，改以 openapi 的 `status` 欄位判定，列為 Task 2.3 的已知缺口

## Phase 2 — gateway adapter 改用 V2

每個 Task 的單元測試以 Task 1.3 的真實樣本當輸入；既有行為測試不改期望值，只換掉 V1 專屬的介面樣本。完成判準都包含 CI 三道（fmt、clippy、test）本機全綠。

### Task 2.1 — server backend 的連線、認證與 session 管理
- 產出與範圍：`opencode_server.rs`：`/api/...` 路徑、`data` 包裝、basic auth（使用者名稱 opencode）、健康檢查改 `GET /api/info`、session 建立／查詢／刪除／建立時間、列表跟著 cursor 走完
- 相依：1.1、1.3
- 驗收編號：SCN-004、SCN-008
- 完成判準：錯誤密碼時就緒檢查失敗並指出認證被拒，V2 網頁的 200 不算健康；假 server 造 150 個 session 分兩頁以上，最舊的仍被列出（地雷 W-6）；保留期清理與輪替刪除走新的刪除端點
- 驗證：外迴圈為 gateway 整合測試（假 V2 server）；單元測試覆蓋分頁與錯誤解析
- 狀態：✅ 已完成
- 證據：2026-11-24。先以 [probe-list.md](probe-list.md)（[scripts/probe-list.sh](scripts/probe-list.sh)）確認 V2 的分頁、認證與網頁 200 行為，再改 `opencode_server.rs`：就緒檢查打 `GET /api/info`、401 時回報「認證被拒」並指出 `WUKONG_AGENT_SERVER_PASSWORD`；basic auth 使用者名稱固定 opencode（V2 不認其他名稱，`WUKONG_AGENT_SERVER_USERNAME` 在 server backend 不再讀取，本機模式的對應由 Task 2.4 處理、說明由 Task 6.3 補）；session 建立、查詢建立時間、刪除改走 `/api/session`，回應讀 `data`；列表每頁 1000 筆照 cursor 走到空頁或沒有 `next`，100 頁仍未結束就停下並標記截斷。測試用的假 V2 server 在 `crates/wukong-gateway/src/opencode_server/fake_v2.rs`（每頁最多 50 筆，逼呼叫端照 cursor 走）。
  - 外迴圈：`crates/wukong-schedulerd/tests/retention_daemon.rs` 跑真的 schedulerd，假 V2 server 有 120 個 session、過期的兩個在最後一頁，日誌 `listed=120 truncated=false`、`deleted=1`，只刪 `DELETE /api/session/ses_orphan`，仍被 scope 指向的 `ses_kept` 保留（SCN-008）；`crates/wukong-cli/tests/opencode_prune.rs`、`new_session_flag.rs` 的刪除改走 `/api/session/{id}`
  - 單元／整合：`opencode_server` 的 `health_check_asks_api_info`、`health_check_reports_a_rejected_password`（錯誤密碼與沒有密碼，SCN-004）、`health_check_does_not_take_the_web_page_for_a_healthy_server`、`list_sessions_follows_the_cursor_to_the_oldest_session`（151 個 session、4 頁以上、最舊的與子 session 都列出，W-6）、`list_sessions_stops_when_the_cursor_never_ends`、`list_sessions_ends_on_a_page_without_a_next_cursor`、`session_created_ms_reads_time_created_from_the_session`、`sessions_are_created_and_deleted_through_the_api_routes`
  - 紅燈：同日把新測試放到改動前的 f3c9f1d 上跑（暫存 worktree），gateway 4 支、retention_daemon 2 支、opencode_prune 5 支、new_session_flag 2 支全部失敗（V1 路徑 404、`/global/health` 回 404 或網頁）
  - 綠燈：`cargo test` 全 workspace 765 passed、0 failed；`cargo clippy --all-targets -- -D warnings`、`cargo fmt --all -- --check` 通過
  - SCN-008 的「壓縮照常執行」走 compact 端點，隨 Task 2.2 改寫與驗證；輪替的建立時間查詢與刪除在本 Task 驗證

### Task 2.2 — prompt 與事件串流
- 產出與範圍：`opencode_server.rs`、`opencode_server/event_map.rs`：prompt 改 `POST /api/session/{id}/prompt` 後等事件，文字、思考、工具事件對應到既有 `StreamEvent`，完成判定改用 V2 的 execution 事件；附件、換模型（`POST /api/session/{id}/model`）；壓縮改 `compact`
- 相依：2.1
- 驗收編號：支援 SCN-002、SCN-008（壓縮）
- 完成判準：以真實樣本重播，入口收到的 `StreamEvent` 序列與回覆文字與 V1 同情境相同；附件與模型參數出現在送出的請求裡
- 驗證：單元測試（樣本重播）加整合測試
- 狀態：✅ 已完成
- 證據：2026-11-24。先以 [probe-prompt.md](probe-prompt.md) 實測附件（file 與 data URI）、選 agent、換模型、以權限關掉 question 工具、閒置時壓縮的事件流程，再改：
  - `opencode_server/event_map.rs` 改寫成 V2 對應器（`ServerEventMapper`）：`step.started`／`step.ended` → 步驟、`reasoning.delta` → 思考、`tool.input.started` 記名稱後在 `tool.called` 送出工具（question 工具不送，由 form 顯示）、`text.ended` 記到回合結束取最後一則 assistant 訊息的文字（文字不在串流中送出，與 V1 server 路徑相同）、`execution.succeeded` 結束、`execution.failed`／`compaction.failed`／`execution.interrupted` 讓回合失敗、`form.created` 與 `permission.asked` 轉成既有的 `QuestionRequest`；其他 session 的事件一律忽略
  - `opencode_server.rs`：prompt 前依需要 `POST …/agent`、`POST …/model`，再 `POST …/prompt`（文字與 `files[].uri`）；`tool_overrides` 只在建立 session 時轉成權限規則（V2 的 session 權限會一直留著，帶覆寫的呼叫都用新的暫時 session）；非串流的 `run` 與 `run_ephemeral` 改走事件串流；串流中斷時改送 `interrupt`；壓縮改 `POST …/compact` 後等事件，不再需要模型；`upstream_error.rs` 加 `parse_v2_error`，`aborted` 分類為回合被中止（完整的 V2 錯誤分類由 Task 2.3 處理）
  - 單元：`event_map` 以真實樣本重播（`server-text-tool`、`server-permission`、`server-form`、`server-interrupt`、`server-no-route`），期望值取自樣本內容（shell 指令 `echo probe`、回覆 DONE、form 的題目與選項）
  - 整合（假 V2 server 送真實樣本）：`run_streaming_replays_a_real_v2_turn`、`run_waits_for_the_events_too`、`agent_and_model_are_selected_before_the_prompt`、`attachments_are_sent_as_file_uris_in_the_prompt`、`tool_overrides_become_permissions_of_the_ephemeral_session`、`a_missing_session_is_replaced_by_a_new_one`、`run_streaming_interrupts_the_session_when_the_event_stream_dies`、`compaction_waits_for_the_compaction_to_finish`（SCN-008 的壓縮）、`a_failed_compaction_is_an_error`
  - 紅燈：同日把上列 9 支整合測試放到改動前的 8e9dee9 上跑（暫存 worktree），9 支全部失敗
  - 綠燈：`cargo test` 全 workspace 759 passed、0 failed（移除 6 支只驗 V1 回應形狀的測試：`/session/{id}/message` 列表、同步回應的文字與模型擷取、summarize）；clippy、fmt 通過
  - 端到端：probe-prompt.md「端到端」一節，真實 V2 上兩個回合與一次壓縮都成功，暫時 session 都刪除
  - 範圍調整：`form.created`、`permission.asked` 轉成 `QuestionRequest` 屬於事件串流，在本 Task 完成；Task 2.5 只剩回覆端點與呼叫端。本機模式測試的 python 假 server 只換了路由與事件，stderr 錯誤行的判斷是否仍適用 V2 由 Task 2.4 實測

### Task 2.3 — 取消與上游錯誤分類
- 產出與範圍：取消改 `interrupt` 並辨認 interrupted 事件；`upstream_error.rs`、`stream.rs` 改讀 V2 錯誤的 `error.type`；server 與 CLI 兩條路徑追蹤 `retry` 事件，從第一次 retry 起持續超過 90 秒就中止回合（server 送 interrupt、CLI 結束子程序），依 retry 的錯誤類型回報，新增「上游連不到」分類
- 相依：2.2
- 驗收編號：SCN-005、SCN-007、SCN-016
- 完成判準：取消後回合回報已取消，不等 agent timeout；額度用完分類為上游限流、模型下架分類為模型已下架、重試超過 90 秒分類為上游連不到，exit 0 或降級文字不被當成功（地雷 W-7）；重試在 90 秒內恢復時回合照常完成；拿掉分類或中止邏輯時測試會紅
- 驗證：單元測試以 Task 1.2、1.3 的真實 retry 樣本與暫停的 tokio 時間推進；整合測試以 `--network none` 的真實 V2 確認約 90 秒內結束（V1 對照約 65 秒）
- 狀態：✅ 已完成
- 證據：2026-11-24
  - 錯誤類型：從 2.3.2 執行檔取出 V2 的 provider 錯誤對應（`jn()` 同時帶上游的 `http.status`），`upstream_error.rs` 的 `classify` 對應如下，410、429 仍先依狀態碼判定。新增「上游連不到」分類（`Unreachable`）
    - `rate-limit`、`quota`、`free-limit`、`go-limit`、`budget-limit`、`model-budget-limit`、`chatgpt-limit` → 上游限流
    - `no-route` → 模型已下架
    - `transport`、`connect`、`timeout` → 上游連不到
  - 錯誤解析：`parse_error_field` 遇到 V2 的 `{type, message, status}` 改用 `parse_v2_error`，CLI 的 `error` 行因此分得出類型
  - 重試中止：新增 `RetryWatch`（`upstream_error.rs`）
    - 計時：第一次重試起算，有實際輸出（思考、文字、工具、步驟結束）就重新起算；`step.started`／`step_start` 重試前後都有，不算恢復。超過 90 秒時以最後一次重試的錯誤回報
    - 兩條路徑：server 的事件串流讀 `session.retry.scheduled`，CLI 讀 `retry` 行，都在 `select!` 等時限。server 中止後由呼叫端送 interrupt，CLI 結束子程序
    - CLI 的非串流 `run`：遇到 opencode 指令也改讀 `--format json` 事件。路由與規劃走這條，原本純文字輸出看不到重試；session ID 照舊不回傳。自訂的非 opencode 指令仍讀純文字
  - 取消（SCN-005）：Wukong 的取消是丟掉回合的 future（例如排程的提問回覆失敗時中止回合）。server 後端在送出 prompt 後掛上 `InterruptOnDrop`，future 在完成前被丟掉時，背景送 `POST …/interrupt`；`execution.interrupted` 回報為「回合被中止」
  - 單元：分類與時限各一組
    - `v2_error_types_keep_their_categories`、`parses_the_v2_cli_error_line`（真實 `cli-no-route` 樣本）
    - `retries_that_last_too_long_give_up_as_unreachable`、`a_recovery_restarts_the_clock`、`not_retrying_never_expires`：以暫停的 tokio 時間驗證 90 秒，錯誤取自真實 `cli-retry-transport` 樣本；gateway 的 dev-dependency 加上 tokio `test-util`
  - 整合：server 與 CLI 各一組
    - server（假 V2 送真實樣本）：`a_retry_lasting_too_long_is_interrupted_as_unreachable`、`a_retry_that_recovers_in_time_lets_the_turn_finish`（第一次重試到結束超過時限，中間恢復輸出）、`dropping_the_turn_interrupts_the_session`、`an_interrupted_execution_reports_the_turn_as_aborted`
    - CLI（`sh` 重播真實樣本）：`agent_cli_gives_up_on_retries_that_last_too_long`、`agent_cli_finishes_a_turn_that_recovers_from_retries`、`agent_cli_reports_the_v2_error_type`、`opencode_plain_run_also_gives_up_on_retries`、`opencode_plain_run_reads_the_answer_from_events`
    - 真實 I/O 下暫停 tokio 時間會快轉觸發 reqwest 的逾時（實測健康檢查以 operation timed out 失敗），所以整合測試改用縮短的時限（400ms），90 秒本身由單元測試驗證
  - 紅燈：新測試用到的介面在 e165545 不存在，改用同日拿掉單一行為的方式，在暫存 worktree 各跑一次相關測試，每項都紅
    - 拿掉 server 的重試中止 → 1 支紅；拿掉 CLI 的重試中止 → 1 支紅
    - 拿掉 V2 類型分類 → 5 支紅；拿掉 V2 錯誤形狀解析 → 3 支紅
    - drop 時不送 interrupt → 1 支紅
    - 非串流 `run` 不讀事件 → 2 支紅
    - 恢復時不重新起算 → server 與 CLI 的恢復測試各紅
  - 綠燈：`cargo test` 全 workspace 771 passed、0 failed，gateway 連跑 3 次都通過；clippy、fmt 通過
  - 端到端：見 [probe-retry.md](probe-retry.md)
    - 真實 V2 斷網時 server 與 CLI 都在 91 秒以「上游連不到」結束；改動前 CLI 在 400 秒的外部 timeout 時仍沒有輸出
    - 斷網後 30 秒恢復網路，回合 51 秒照常回覆（SCN-016）
  - 殘餘：沒有真實的 429、410 樣本，限流與下架的判定依 V2 原始碼的對應與 `status`，沒有實際觸發過

### Task 2.4 — 本機按需模式
- 產出與範圍：`local_process.rs`：等 `server listening on`，每次啟動自設隨機密碼並帶 basic auth，操作結束後關閉程序
- 相依：2.1、2.2
- 驗收編號：SCN-002
- 完成判準：以真實 V2 2.3.2 跑整合測試：單棒、多棒、附件、指定模型的回合都得到回覆並記錄 session；回合結束後沒有殘留的 opencode 程序（前後 `ps` 比對）；密碼不出現在命令列參數
- 驗證：外迴圈為接真實 V2 的整合測試（需要模型的項目用預設免費模型）；單元測試覆蓋啟動偵測與逾時
- 狀態：✅ 已完成
- 證據：2026-11-24
  - 實測：V2 `serve` 接受原本的旗標（`--port 0`、`--print-logs`），啟動字串改為 `server listening on`；斷網重試期間 stderr 沒有 `stream error`，V1 時期的 stderr 判斷保留不會誤判。見 [probe-local.md](probe-local.md)
  - `local_process.rs` 改動：
    - 啟動偵測改認 V2 的字串
    - 每次啟動以兩個 uuid v4 產生 64 字元的隨機密碼，只經 `OPENCODE_SERVER_PASSWORD` 環境變數交給 serve，`for_local_process` 帶同一組密碼送 basic auth
    - 不再把 `WUKONG_AGENT_SERVER_PASSWORD`／`USERNAME` 帶進本機 serve，文件說明由 Task 6.3 補
    - gateway 依賴加上 `uuid`（workspace 既有）
  - 單元：`each_launch_gets_its_own_password_through_the_environment`，以 python 假 server 驗三件事：沒帶正確 basic auth 就回 401、兩次啟動的密碼不同且至少 32 字元、`$@` 不含密碼。其餘假 server 的啟動字串改為 V2 格式；啟動偵測與逾時沿用 `invalid_listener_releases_child`、`health_failure_releases_child`、`cancelled_startup_releases_child`
  - 紅燈：同日把新的 tests 模組放到改動前的 94926d7（暫存 worktree）上跑，8 支失敗（認不得 V2 啟動字串、沒有密碼）
  - 綠燈：`cargo test` 全 workspace 774 passed、0 failed、13 ignored；clippy、fmt 通過
  - 外迴圈：接真實 V2 2.3.2，見 probe-local.md
    - `local_v2_actual.rs`（`#[ignore]`，以 `scripts/local-v2-actual.sh` 執行）：單棒並串流工具、接續、附件、指定模型、暫時回合、壓縮與刪除都通過；每步之後沒有殘留程序；回合中 serve 的命令列不含密碼
    - 經 wukong CLI 的多棒回合（Explorer → Fixer）與接續回合通過，session 有記錄，結束後沒有殘留程序
  - 殘餘：wukong 被外部訊號結束時（例如 `timeout` 的 SIGTERM），自己 process group 裡的 serve 會留下來；V1 時就是如此，不在本 Task 範圍

### Task 2.5 — 提問與權限回覆
- 產出與範圍：gateway 的回覆路由改 V2 的 form 與 permission 端點（事件轉成提問已在 Task 2.2 完成）；Telegram（`dispatch.rs`）與 Web 的呼叫端跟著改，使用者操作不變
- 相依：2.2
- 驗收編號：SCN-006
- 完成判準：回答、自訂回答、取消各送到正確端點且回合繼續或結束；無法辨識的權限選項仍視為錯誤；無人值守路徑在 baseline 權限下不出現詢問（地雷 W-8）；既有 Telegram 問答測試不改期望值
- 驗證：gateway 單元測試加 Telegram／Web 既有測試
- 狀態：✅ 已完成
- 證據：2026-11-24
  - 實測：在真實 V2 2.3.2 上試了五種回覆：自訂回答、多選、取消提問、允許一次、拒絕權限。對照組是 V1 1.19.6 的取消與拒絕。見 [probe-reply.md](probe-reply.md)。結論：
    - 回答送 `POST /api/session/{sid}/form/{id}/reply`，body 為 `{"answer":{<key>: 值}}`，多選送陣列
    - 取消提問改為 `DELETE …/form/{id}`，V2 沒有 reject 端點
    - 權限送 `POST /api/session/{sid}/permission/{id}/reply`，body 為 `{"decision": …}`
    - 取消與拒絕之後，V2 以 `execution.interrupted`（reason `shutdown`）結束執行；V1 則是不帶錯誤的 `session.idle`
  - 改動：
    - `opencode_server.rs`：
      - 串流看到 `form.created` 時記下欄位（key、是否多選、選項的 label 與 value），該 session 的事件串流結束時清掉（drop guard，提早回傳錯誤也會清）
      - `reply_question` 把入口的選項標籤換成 value，對不到的當成自訂文字；多選送陣列；沒看過的 form 用 `q{i}` 鍵
      - `reject_question` 改為 DELETE form
      - `reply_permission` 多帶 session，body 改為 `decision`
      - 移除本機模式專用的 `reply_local_question`，本機與 server 模式走同一組 V2 端點
    - `event_map.rs`：看到 `form.cancelled`，或 reply 為 `reject` 的 `permission.replied` 之後，再收到 `execution.interrupted` 時視為正常結束，與 V1 相同。使用者按停止（reason `user`）仍是中止
    - `backend.rs`、`local_process.rs`：權限回覆傳入 session。Telegram、Web 與排程的呼叫端簽章不變，沒有改動（`git diff --stat` 在這三個 crate 為空）
    - 假 V2 加上 form、permission 的回覆路由與 DELETE form；新增四個真實樣本 fixture
  - 單元與元件測試：
    - `answers_follow_the_fields_of_the_real_form`、`option_labels_are_sent_as_their_values`、`reply_routes_follow_the_v2_contract`：取代原本 V1 的 body 與 URL 測試
    - `plain_question_cancel_deletes_the_form`
    - 重播真實樣本、經 `AgentBackend` 回覆的五支測試：`an_answer_is_sent_to_the_form_and_the_turn_goes_on`（另驗回合結束後 form 已清掉）、`a_custom_answer_is_sent_as_text`、`a_cancelled_question_ends_the_turn_like_v1`、`an_allowed_permission_lets_the_turn_finish`、`a_rejected_permission_ends_the_turn_like_v1`
    - 無法辨識的權限選項仍是錯誤：`unrecognized_permission_answer_is_an_error` 未改
    - 地雷 W-8：排程的無人值守處置（預設拒絕、可設為允許一次）走同一組 `answer_question`／`cancel_question`。拒絕與允許一次的 V2 行為由上面兩支權限測試驗證；排程既有測試照過
  - 紅燈：同日把新測試與 fixture 放到改動前的 d98a28b（暫存 worktree）上跑，9 支失敗。另做兩項消融：拿掉清除 form 的 guard，`an_answer_is_sent…` 失敗；拿掉 interrupted 視為正常結束的對應，兩支 `…_like_v1` 失敗
  - 綠燈：`cargo test` 全 workspace 780 passed、0 failed、13 ignored；其中 Telegram、Web、排程 195 passed，期望值未改。clippy、fmt 通過
  - 經 gateway 接真實 V2 的回合未完成：新增 `local_question_and_permission_replies_against_real_v2`（`#[ignore]`，以 `scripts/local-v2-actual.sh … replies` 執行）。17:36（台北）執行時，免費模型額度已用完，第一個回合就收到真實的 `provider.free-limit` 429，Wukong 正確分類為「上游限流」，這也是 SCN-007 第一次取得的真實 429。這個測試不在本 Task 的必要驗證內，額度恢復後重跑，最遲在 Task 6.1 演練時補上

### Task 2.6 — CLI backend
- 產出與範圍：`backend.rs` 的 `assemble_argv` 與 `stream.rs`：opencode 路徑加 `--standalone`，NDJSON 改 V2 形狀；TBD-1 的結論要求的處理
- 相依：1.2、2.3
- 驗收編號：SCN-010
- 完成判準：帶額外旗標的 `opencode run` 取得回覆與 session，結束後沒有背景 service 程序；`printf fixer` 假 agent 的既有測試照過，argv 不含任何 opencode 旗標
- 驗證：單元測試（argv 與 NDJSON 樣本）加接真實 V2 的整合測試
- 狀態：🚧 進行中（只差接真實模型的回合，等免費額度恢復）
- 證據：2026-11-24
  - 實測：見 [probe-cli.md](probe-cli.md)「Task 2.6」一節
    - 容器裡有背景 service 時，不加 `--standalone` 的 `run` 會連上它；加了才起私有 `serve --stdio`
    - `--log-level error` 傳不到私有 serve，stderr 每次約 43 行 INFO
    - V2 `--format json` 的事件名稱與 V1 相同（`text`、`reasoning`、`tool_use`、`step_start`、`step_finish`，`part.tool`，最上層 `sessionID`），`stream.rs` 的解析不必改；錯誤與重試的解析已在 Task 2.3 完成
  - 改動（`backend.rs`）：
    - `assemble_argv` 在 opencode 命令後加 `--standalone --print-logs --log-level error`；`WUKONG_AGENT_CMD` 已有 `--standalone` 或 `--server` 時不加 `--standalone`
    - `run` 以非 0 結束時，回報的 stderr 先濾掉 INFO 以下的日誌行
  - TBD-1 的結論：`--dangerously-skip-permissions` 照常可用，不改寫使用者的命令
  - 測試：
    - 6 支 `assemble_argv` 測試改為新旗標
    - 新增 `assemble_argv_keeps_the_server_choice_of_a_custom_command`：改動前也會通過，因為舊碼從不加 `--standalone`；它防的是之後重複加，或蓋掉使用者的 `--server`
    - 新增 `v2_run_output_streams_steps_reasoning_tools_and_text`：重播 V2 真實輸出 `cli-text-reasoning`、`cli-tool-completed`。改動前也會通過，證明解析不必改
    - 新增 `a_failed_v2_run_reports_stderr_without_info_logs`：重播真實 stderr `cli-stderr-offline.txt`
    - `printf fixer`、`echo` 假 agent 的既有測試照過，argv 不含任何 opencode 旗標（`assemble_argv_omits_opencode_flags_for_fake_agents`，以及各入口 crate 的 `tests/cli_backend.rs`）
  - 紅燈：改動前同一組測試 8 支失敗（7 支 argv、1 支 stderr）
  - 綠燈：`cargo test` 全 workspace 783 passed、0 failed、14 ignored；clippy、fmt 通過
  - 外迴圈（部分）：
    - wukong CLI 經 wrapper 接斷網容器裡的真實 V2，容器裡有背景 service。V2 接受新 argv 並起了私有 serve；規劃那一棒重試超過 90 秒後以「上游連不到」中止
    - 尚缺：帶額外旗標的 `opencode run` 取得回覆與 session、以 `-s` 接續、結束後沒有背景 service 程序。這幾項需要模型，免費額度用完（見 Task 2.5），恢復後補跑才能標已完成。2026-11-25 Task 4.1 建出 V2 映像後，曾提議改用斷網容器裡的假模型驗證這幾項；使用者選擇照原計畫等真實模型，所以本 Task 維持進行中

### Task 2.7 — V1 版本偵測
- 產出與範圍：在 opencode 類 backend 第一次呼叫前檢查版本，1.x 時回傳明確錯誤
- 相依：1.1
- 驗收編號：SCN-011
- 完成判準：PATH 上放回報 1.x 的假 opencode 時，回合立即失敗，錯誤含「需要 OpenCode V2」與偵測到的版本，耗時遠低於 agent timeout；2.x 時通過
- 驗證：單一迴圈：整合測試以假 opencode 執行檔覆蓋這一層，沒有另外的底層失敗面
- 狀態：✅ 已完成
- 證據：2026-11-24
  - 實際的 `opencode --version` 輸出：V1 1.19.6 印 `1.19.6`，V2 2.3.2 印 `opencode v2.3.2`
  - 改動：新增 `opencode_version.rs` 的 `ensure_v2`，放在 CLI backend 的 opencode 回合（`run_streaming_with_retry_limit`，非串流的 `run` 也走這裡）與本機模式 `execute`（回合、列表、刪除、壓縮共用）的最前面
    - 以 `<執行檔> --version` 取最後一段的主版號，小於 2 時回 `AgentFailed`：「需要 OpenCode V2（2.x），<執行檔> 回報的版本是 <版本>…」
    - 確認是 V2 的執行檔記在程序內，之後不再檢查
    - 執行不了、逾時（10 秒）、非 0 結束或輸出看不懂時照常呼叫，讓原本的錯誤浮出來。所以 `sh` 做的假 opencode 不受影響
    - 假 agent（`printf fixer`）不是 opencode，不檢查
  - 測試：
    - 整合測試 `crates/wukong-cli/tests/opencode_version.rs`，以 wukong CLI binary 加假 opencode 執行：
      - 回報 `1.19.6` 時，本機按需模式（`opencode run`）與 CLI backend（多帶一個旗標）都在 10 秒內失敗，錯誤含「需要 OpenCode V2」與 `1.19.6`，`run` 一次都沒被呼叫
      - 回報 `opencode v2.3.2` 時照常完成
    - 單元測試 `reads_the_major_version_of_real_outputs` 以兩版的實際輸出為輸入
    - 本機模式既有測試的假執行檔改為回答 `--version`
  - 紅燈：改動前整合測試失敗，本機模式只報「本機 OpenCode 未回報啟動位址便結束。」，看不出版本
  - 綠燈：`cargo test` 全 workspace 785 passed、0 failed、14 ignored；clippy、fmt 通過
  - 實測：wukong CLI 經 wrapper 接映像內建的真實 V1 1.19.6（斷網容器）。兩條路徑都在約 0.5 秒內以上述訊息失敗，opencode 只被呼叫 `--version` 一次
    - 第一次實測時發現 V2 印的是 `opencode v2.3.2`，原本的解析看不懂這個格式。照失效方向不會擋下 V2，但也不會記成已確認，每次呼叫都會多跑一次 `--version`；已改為取最後一段並補進單元測試
  - 範圍外：server 模式連的是另一個容器的 server，不經過 PATH 上的 opencode，不在本 Task 檢查

## Phase 3 — 容器設定

### Task 3.1 — V2 格式的 opencode 設定 baseline
- 產出與範圍：`scripts/docker-entrypoint.sh` 的 baseline 改 V2 格式（權限動作名稱、external_directory、compaction、tool_output、watcher）
- 相依：1.1
- 驗收編號：SCN-009
- 完成判準：真實 V2 以 baseline 啟動成功；對絕對路徑的遞迴刪除被拒（拿掉規則時同一命令會通過的反向自檢）、/workspace 內刪除允許、/tmp 存取不詢問；user.json 的鍵覆蓋 baseline；`bash scripts/test-docker-runtime.sh` 通過
- 驗證：容器層測試（真實 V2 映像）
- 狀態：✅ 已完成
- 證據：2026-11-25
  - 實測 V2 2.3.2 怎麼讀 V1 寫法的 baseline（`opencode debug config`）：
    - 仍可啟動，`permission` 轉成 `permissions` 有序清單（`bash` → `shell`），`snapshot` 轉成 `snapshots`
    - `compaction.prune` 與 `tail_turns` 被默默丟掉，沒有任何錯誤或日誌（V2 原始碼標為 omitted unsupported legacy setting）
    - V2 預設權限是 `* * allow`，加上讀 `.env` 時詢問；外部目錄預設不詢問（Task 1.2 第 8 段讀 `/etc/hostname` 直接完成，原因在此）
  - 改動：
    - `scripts/docker-entrypoint.sh` 的 baseline 改寫成 V2 原生格式：`snapshots`、`permissions` 清單（規則與順序同原本，`/workspace` 的 allow 在 deny 之後）、`tool_output` 與 `watcher` 不變
    - `compaction` 改成 `{"auto": true, "background": false}`：V2 沒有 `prune`、`tail_turns` 的對應設定。`background` 預設是開，會在 session 接近上限時先在背景寫摘要，摘要沒用到也一樣花 token，V1 沒有這個行為，所以關掉
    - 同檔註解改寫為 V2 的情況；`AGENTS.md` 的「容器內 opencode 權限」「opencode 設定分層」兩條同步更新
    - `scripts/test-docker-runtime.sh` 原本 grep V1 的字串，改成解析 baseline：沒有 V1 的鍵、有 `/tmp` 放行與 rm deny、`/workspace` 的 allow 排在最後一條 deny 之後
  - 測試：新增容器層測試 `scripts/test-opencode-baseline.sh IMAGE [V2_PREFIX]`（driver 為 `scripts/test-support/opencode-baseline-driver.py`）
    - 容器以 `--network none` 執行本 repo 的 entrypoint，再以 wukong 使用者執行 driver。設定檔與 `OPENCODE_CONFIG` 都是 entrypoint 實際寫出的結果
    - driver 內建只綁 127.0.0.1 的 OpenAI 相容假模型，第一次請求要求呼叫指定工具。真實 opencode 照常解析設定、比對權限、執行工具，不連外部模型，不受免費額度影響
    - 11 項檢查：
      - V2 讀入的 baseline 與檔案內容相同（沒有鍵被改或丟掉）；`user.json` 是設定來源之一
      - `rm -rf /tmp/victim` 被拒，加 `--dangerously-skip-permissions` 也被拒，目錄仍在；反向自檢：拿掉 deny 規則後同一命令通過，目錄被刪
      - `rm -rf /workspace/victim` 通過
      - read 工具讀 `/tmp/probe.txt` 不詢問、取得內容；反向自檢：`/tmp` 規則改成 ask 時同一讀取被拒（「This non-interactive run cannot ask the user for permission…」），證明這項抓得到詢問
      - `user.json` 用 V2 的 `permissions` 或沿用 V1 的 `permission.bash` 都能讓同一個 rm 通過；`user.json` 的 `tool_output.max_lines: 3` 讓 `seq 1 50` 的輸出從 50 行變成 3 行
    - 紅燈：改動前的 baseline 有 3 項失敗。「讀入相同」報 `compaction` 被改成 `{"auto": true}`、`permission` 與 `snapshot` 被轉換；兩個反向自檢以 V2 格式改寫規則，對 V1 寫法無效。權限行為本身靠 V2 的自動轉換仍然正確
    - 綠燈：`bash scripts/test-opencode-baseline.sh ghcr.io/raybird/wukong:v0.25.1 <V2 2.3.2 prefix>` 11 passed、0 failed，約 14 秒；`bash scripts/test-docker-runtime.sh` 通過。在 baseline 放回 V1 的 `snapshot` 鍵時，`test-docker-runtime.sh` 失敗並指出該鍵
  - 限制：
    - 最初以 v0.25.1 映像加掛 V2 prefix 執行。2026-11-25 Task 4.1 在本機建出 V2 release 映像（`wukong-issue44:v2-local`）後，以 `bash scripts/test-opencode-baseline.sh wukong-issue44:v2-local`（不加掛）重跑，11 passed、0 failed
    - 權限以 `opencode run` 驗證，本機與 server 模式的 `serve` 用同一份設定與同一套規則比對；`serve` 遇到 ask 會轉給使用者回答，那條路徑屬 Task 2.5
    - 給使用者的文件（`user.json` 可用 V2 的 `permissions`、V1 寫法仍有效、`tail_turns` 不再生效）留給 Task 6.3

### Task 3.2 — 常駐 server 模式的密碼與 healthcheck
- 產出與範圍：compose 三個檔（含 release、memoria overlay）把 `WUKONG_AGENT_SERVER_PASSWORD` 交給 opencode-server（作為 `OPENCODE_SERVER_PASSWORD`）與三個入口，opencode-server healthcheck 帶認證打 `/api/info`；server 模式沒有密碼時入口啟動即報錯；`scripts/install.sh` 的 `.env` 遷移在 server 模式未設密碼時先備份再產生；`.env.example`
- 相依：2.1
- 驗收編號：SCN-003、SCN-017
- 完成判準：入口與 opencode-server 用同一組密碼時就緒檢查通過、回合成功；server 停止或密碼錯誤時 healthcheck 判 unhealthy；沒有密碼時入口立即報錯並指出變數名稱；installer 測試確認 server 模式未設密碼時產生密碼、已設時不改、非 server 模式不加；每個服務都接到變數（地雷 W-3，`test-docker-runtime.sh`）
- 驗證：容器層整合測試
- 狀態：✅ 已完成
- 證據：2026-11-25
  - 改動：
    - `build_backend_from_env` 改回傳 `Result`：設了 `WUKONG_AGENT_SERVER_URL` 卻沒有 `WUKONG_AGENT_SERVER_PASSWORD`（或為空）時回錯誤，訊息指出變數名稱。CLI、web、telegram、schedulerd 四個入口啟動時印出錯誤並結束；改簽章讓編譯器確保每個入口都處理（GitNexus impact：CRITICAL，6 個直接呼叫端，都是入口與測試）
    - `docker-compose.yml`、`docker-compose.release.yml`：opencode-server 拿 `OPENCODE_SERVER_PASSWORD=${WUKONG_AGENT_SERVER_PASSWORD:-}`，`wukong`、`wukong-telegram`、`wukong-web`、`wukong-schedulerd` 拿 `WUKONG_AGENT_SERVER_PASSWORD=${WUKONG_AGENT_SERVER_PASSWORD:-}`。memoria overlay 不用改，疊上後每個服務都拿到同一組（`docker compose config` 確認）
    - opencode-server healthcheck 改為帶 basic auth 查 `/api/info`。認證 header 以 base64 經 stdin 交給 curl（`-H @-`），密碼不出現在任何程序的參數裡（主機上的 `ps` 看得到容器內程序的參數），也不必處理特殊字元
    - `scripts/install.sh` 的 `.env` 遷移：URL 指向內建 opencode-server 且 server profile 會啟用、密碼未設或為空時，先備份再產生 48 字元 hex 密碼（只寫進 `.env`，輸出只說明已產生，不印密碼）；已設時不動；本機模式不加；指向其他 server 時不產生（對不上那台 server），只印警告
    - `.env.example` 說明這個變數
  - 測試：
    - 單元 `backend_from_env_uses_cli_when_server_url_missing` 加上沒有密碼時回錯誤並指出變數名稱。紅燈：簽章未改時編譯失敗；拿掉檢查的消融版本失敗於「server mode without a password must not build a backend」
    - 整合 `crates/wukong-cli/tests/server_password.rs`：`wukong` binary 在 server 模式沒有密碼時 10 秒內失敗、stderr 含 `WUKONG_AGENT_SERVER_PASSWORD`。紅燈：改動前的程式改去連線，只報 `health_check failed … Connection refused`，看不出原因
    - 既有的 `new_session_flag`、`opencode_prune`、`retention_daemon` 測試用不驗證的假 server，原本刻意不帶密碼，現在改帶一組
    - `scripts/test-docker-runtime.sh` 新增解析 YAML 的檢查：兩個 compose 的 opencode-server 與四個入口都接到同一個變數、healthcheck 帶密碼查 `/api/info`、overlay 沒有改寫密碼變數。紅燈：改 compose 前 12 項失敗；原本比對 `/global/health` 字串的兩項移除
    - `scripts/test-installer-upgrade.sh docker-env-server-password`：未設時產生（48 hex、一行、輸出不含密碼、備份等於原檔、只多一行）、再升級不變、空值就地補上、已設時不動且不備份、本機模式不加、其他 server 不改並提醒。紅燈：改 installer 前失敗於「no password was generated」。原 `docker-env-migration` 第一個案例改為已設密碼，維持「只補 profile」的期望
  - 容器層實測：[scripts/server-password.sh](scripts/server-password.sh)（v0.25.1 映像加掛 V2 2.3.2，本 repo 的 entrypoint，server 容器 `--network none`，模型是容器內的假模型；本機建出的 `target/debug/wukong` 經 docker exec 轉接連進容器）。密碼、環境變數與 healthcheck 命令都從 release compose 的 `docker compose config` 取出。8 項全部通過，約 15 秒：
    - compose 把同一組密碼交給 opencode-server 與三個入口
    - 同一組密碼的 server，healthcheck 判 healthy；入口以同一組密碼就緒檢查通過、回合完成（rc=0，回覆 PONG）；入口換一組密碼時失敗並指出「認證被拒」
    - 反向：healthcheck 帶錯的密碼時 curl 收到 401 而失敗
    - `.env` 沒設密碼（server 拿到空字串、V2 自己產生隨機密碼）時判 unhealthy；同一個 server 上舊的 `/global/health` 檢查仍然成功，證明舊檢查抓不到
    - 沒有 server 回應時同一個 healthcheck 失敗
  - 綠燈：`cargo test --workspace` 786 passed、0 failed、14 ignored；clippy、fmt 通過；`bash scripts/test-docker-runtime.sh`、`bash scripts/test-installer-upgrade.sh all` 通過
  - 限制：容器層實測最初同 Task 3.1 以加掛 V2 的方式執行；Task 4.1 建出 V2 release 映像後，腳本改收 `IMAGE [V2_PREFIX]`，以 `bash docs/issues/issue-0044/scripts/server-password.sh wukong-issue44:v2-local` 重跑，8 項全部通過。給使用者的升級說明（server 模式要設密碼、installer 會自動產生）留給 Task 6.3

## Phase 4 — 打包與 release

### Task 4.1 — @opencode/cli 2.3.2 固定釘選
- 產出與範圍：`release/` 的 package.json、package-lock.json、runtime-inputs.env；`scripts/resolve-opencode-version.sh`、`scripts/test-release-image.sh`、`Dockerfile.release`、`Dockerfile`
- 相依：1.1
- 驗收編號：SCN-012
- 完成判準：npm 有更新版時解析結果仍是 2.3.2；本機建置的映像內 `opencode --version` 為 2.3.2，與釘選檔、manifest 一致（向產物要答案，地雷 W-1）；`test-release-image.sh` 通過，並在版本不符時失敗
- 驗證：產物檢查
- 狀態：✅ 已完成
- 證據：2026-11-25
  - 前提：當天 `npm view @opencode/cli version` 是 2.4.1，SCN-012 的「npm 上有比 2.3.2 新的版本」是實際情況
  - 改動：
    - `release/package.json` 改依賴 `@opencode/cli` 2.3.2，`package-lock.json` 以 `npm install --package-lock-only --omit=dev` 重產（含 12 個平台子套件與各自的 integrity）；`runtime-inputs.env` 的 `OPENCODE_VERSION=2.3.2`、`OPENCODE_INTEGRITY` 為 npm 上 `@opencode/cli@2.3.2` 的值
    - `scripts/resolve-opencode-version.sh` 不再取 npm latest：沒有 `OPENCODE_VERSION_PIN` 時沿用 `runtime-inputs.env` 的版本，只重新取 integrity 並重寫三個檔；指定 1.x 時拒絕（「Wukong requires OpenCode V2」）。套件名稱改 `@opencode/cli`
    - `scripts/test-release-image.sh`：`pins` 改檢查 `@opencode/cli` 且主版號必須是 2；`smoke` 在映像內執行 `opencode --version`，與 `runtime-inputs.env` 不同就失敗（向產物要答案，地雷 W-1）。release CI 在發布 tag 之前就跑 smoke，所以版本不符的映像不會被發布
    - `Dockerfile`（開發用）改裝 `@opencode/cli`，`OPENCODE_VERSION` 預設 2.3.2（原為 latest）；`Dockerfile.release` 不用改（`node_modules/.bin/opencode` 照舊）；release workflow 的步驟名稱改為「Verify the pinned opencode (V2)」
    - `docs/release-rehearsal.md` 的「OpenCode 版本」一節改寫成固定釘選、換版流程與只收 2.x 的 pin
  - 測試：
    - 新增 `scripts/test-resolve-opencode-version.sh`：在暫存目錄複製 release/ 與腳本，假 npm 回報 latest 為 2.9.9。確認沒指定時釘選不動（runtime-inputs.env 與原檔相同）、指定 1.19.6 時失敗並指出要 V2 且不改檔、指定 2.3.2 時通過。紅燈：改動前的腳本去查 `opencode-ai` 的 latest
    - 以真的 npm 執行 `bash scripts/resolve-opencode-version.sh`：「keeping the committed pin 2.3.2」，重產的 lockfile 與提交的內容逐位元組相同
    - `bash scripts/test-release-image.sh smoke ghcr.io/raybird/wukong:v0.25.1`（V1 映像）失敗：「image runs opencode '1.19.6', but release/runtime-inputs.env pins 2.3.2」
  - 產物檢查：照 release workflow 的方式在本機建映像——musl binaries 在 `rust:1.96-bookworm` 容器裡裝 musl-tools 後以 `cargo build --release --locked --target x86_64-unknown-linux-musl` 建出，release-context 依 workflow 的清單逐檔複製，`docker build -f Dockerfile.release` 帶入 runtime-inputs.env 的 build args，得到 `wukong-issue44:v2-local`（2.09 GB，v0.25.1 為 1.77 GB）
    - `bash scripts/test-release-image.sh smoke wukong-issue44:v2-local` 通過；映像內 `opencode --version` 為 `opencode v2.3.2`，`/usr/local/bin/opencode` 指向 `@opencode/cli/bin/opencode.exe`
    - 以 `scripts/generate-release-manifest.sh` 照 workflow 的參數產生 manifest：manifest、映像、package.json、lockfile 的版本都是 2.3.2，lockfile 的 integrity 與 manifest 相同
    - 開發用 `Dockerfile` 也建過一次（建置中的 `opencode --version` 步驟通過），映像內同樣是 `opencode v2.3.2`；驗完已刪除
    - 用這個映像重跑 Task 3.1（11 passed）與 Task 3.2（8 項通過）的容器層測試，都不再加掛 V2
  - 其他檢查：`test-release-image.sh all`、`test-release-workflow.sh`、`test-release-manifest.sh`、`test-docker-runtime.sh`、`test-installer-upgrade.sh all` 通過
  - 觀察：npm 把四個 linux-x64 變體（glibc／musl × 一般／baseline）全部裝進映像，`node_modules` 969 MB；V1 1.19.6 也是四個都裝（838 MB），所以只多約 130 MB，維持原樣

## Phase 5 — installer 備份、失敗還原與 rollback

### Task 5.1 — 從 V1 版本升級時備份 opencode-state
- 產出與範圍：`release/data-compatibility.json`（affectedState 含 opencode-state、backupRequired true）；`scripts/install.sh` 依 backupRequired 停服務、備份 volume、把位置記進 `.wukong-release`
- 相依：4.1
- 驗收編號：SCN-013
- 完成判準：installer 測試：升級時先停服務再備份，備份內容與升級前的 opencode.db 一致，metadata 記錄位置；backupRequired 為 false 時不備份（對照）
- 驗證：installer 腳本測試（fixture）
- 狀態：✅ 已完成
- 證據：2026-11-25
  - 改動：
    - `release/data-compatibility.json`：`affectedState` 為 `["opencode-state"]`、`backupRequired` 為 true（`rollbackSafeTo` 由 Task 5.3 處理）
    - `scripts/install.sh`：新增 `required_state_backups`（從 release manifest 的 dataCompatibility 取出要備份的名稱）與 `backup_docker_state`（以 compose 標籤 `com.docker.compose.project`／`com.docker.compose.volume` 找 volume，用剛拉下且比對過 digest 的新版映像 `docker run --rm --network none -v VOL:/state:ro --entrypoint tar … -C /state -cf - .` 打包到 `.wukong-backups/<前一版>-<時間>/volumes/<名稱>.tar`，再以 `tar -tf` 確認可讀）。`install_docker` 在比對 digest 之後、換上新版檔案之前，若有前一版且新版要求備份，先 `docker compose -p P --profile server stop` 停掉所有服務（含已停用 profile 的 opencode-server）再備份；備份失敗時以 `docker compose -p P start` 重新啟動原本的服務並中止，release 檔案與 metadata 不動。成功時 `.wukong-release` 多一個 `stateBackups`（名稱 → 相對路徑）
    - `docs/docker.md` 補上這個升級行為
  - 測試：`scripts/test-installer-upgrade.sh` 的 fake docker 加上 named volume 模擬（`$WUKONG_TEST_VOLUMES/<名稱>` 目錄；`volume ls` 依標籤回答，`run` 把 volume 掛載點換成該目錄後在主機上執行，tar 真的打包）；fixture 以 `FIXTURE_STATE_BACKUP=1` 產生要求備份的 release。新增 `docker-state-backup` case（併入 `all`）：
    - 對照：不要求備份的升級沒有停服務、沒有 `docker run`、metadata 沒有 `stateBackups`、沒有 volumes 目錄
    - 備份失敗（`docker run` 失敗）：installer 失敗，`.wukong-release` 與 docker-compose.yml 與升級前相同，log 有 `docker compose -p wukong start`、沒有 `up -d --force-recreate`
    - 要求備份的升級：log 順序為停止所有服務 → 讀 opencode-state → `up -d --force-recreate`；metadata 記錄 `.wukong-backups/v9.9.8-*/volumes/opencode-state.tar`；解開後的 opencode.db（64 KiB 隨機內容）與升級前逐位元組相同，`opencode.db-wal` 與子目錄也在
    - 紅燈：實作前「never ran: stop every service」。突變自檢：拿掉失敗時的 `start` →「stopped services were not started again」；停服務不帶 `--profile server` →「never ran: stop every service」
  - 真實 docker：建立帶 compose 標籤的測試 volume `wk44probe_opencode-state`，以 uid 1000 寫入 WAL 模式 SQLite（5000 筆）與子目錄，直接呼叫 installer 的 `backup_docker_state`（映像 `wukong-issue44:v2-local`）。tar 內檔案擁有者保留為 wukong（uid 1000），解開後 `pragma integrity_check` 為 ok、筆數 5000；不存在的 volume 名稱印出提示並略過。測試 volume 已刪除
  - 其他檢查：`test-installer-upgrade.sh all`、`test-release-manifest.sh`（另以真實 data-compatibility.json 產 manifest 通過）、`test-release-workflow.sh`、`test-release-image.sh all`、`test-docker-runtime.sh` 通過
  - 限制：SCN-013 的「新版啟用後服務正常」在 fixture 層只確認 `up -d` 與 `ps` 成功；以真實映像走完整升級留給 Task 6.1 的演練

### Task 5.2 — 新版啟用失敗時還原 opencode.db
- 產出與範圍：`scripts/install.sh` 的啟用失敗路徑
- 相依：5.1
- 驗收編號：SCN-014
- 完成判準：模擬啟用失敗時，release 檔案與 opencode.db 都回到升級前；不還原 opencode.db 的版本會讓測試失敗（反向自檢）
- 驗證：installer 腳本測試
- 狀態：✅ 已完成
- 證據：2026-11-25
  - 改動（`scripts/install.sh`）：
    - 找 volume 的邏輯抽成 `state_volume`（依 compose 標籤，比對到多個時失敗），`backup_docker_state` 改用它
    - 新增 `restore_docker_state`：輸入 Task 5.1 記下的 `{"<名稱>": "<tar>"}`，先以 `tar -tf` 確認備份可讀，再用新版映像 `docker run --rm -i --network none -v VOL:/state --entrypoint sh … -c 'find /state -mindepth 1 -delete && tar -C /state -xpf -' < tar` 清空後整包解回，新版寫入或新增的檔案都不留下
    - 啟用失敗路徑：有 state 備份時先 `docker compose -p P --profile server stop` 停掉新版所有服務，再還原 volume，之後才還原 .env 與 release 檔案並啟動前一版。還原失敗時仍換回前一版的檔案，但不啟動任何版本（前一版可能讀不了已遷移的資料），中止訊息列出備份檔路徑供手動還原。metadata 兩種情況都不前進
    - `docs/docker.md` 補上失敗時的還原行為
  - 測試：fake docker 加上 `FIXTURE_MIGRATING_TAG`（啟動該版時改寫 opencode.db 並新增 `v2-only`，模擬 V2 第一次啟動的遷移）與 `FIXTURE_RESTORE_EXIT`（還原的 `docker run` 失敗）。新增 `docker-state-restore` case（併入 `all`）：
    - 健康檢查失敗：opencode-state 與升級前 `diff -r` 相同（`v2-only` 消失、opencode.db 回到升級前的 64 KiB 隨機內容），`.wukong-release` 與 docker-compose.yml 與升級前相同；log 順序為新版 `up` → 停止所有服務 → 還原 `docker run -i` → 前一版 `up`
    - 還原失敗：release 檔案與 metadata 仍回到升級前，`up -d --force-recreate` 只出現新版那一次，輸出含 `volumes/opencode-state.tar`
    - 紅燈：實作前「opencode-state was not restored to the backup」（diff 顯示 opencode.db 不同、多出 v2-only）。反向自檢（完成判準要求）：拿掉還原 →「opencode-state was not restored to the backup」；還原前不停服務 → 順序斷言失敗；還原失敗仍啟動前一版 →「a release was started on an unrestored opencode-state」
  - 真實 docker：測試 volume `wk44probe_opencode-state` 放入 uid 1000 的 WAL 模式 SQLite（5000 筆）與子目錄，呼叫 installer 的 `backup_docker_state`，再以 uid 1000 模擬 V2（新增資料表 v2、刪光 t、新增檔案 v2-only），之後 `restore_docker_state`。還原後 `find -printf "%p %u:%g %m %s"` 清單與 opencode.db 的 sha256 都與備份前相同，`integrity_check` ok、t 為 5000 筆、沒有 v2 資料表。測試 volume 已刪除
  - 其他檢查：`test-installer-upgrade.sh all`、`test-release-workflow.sh`、`test-docker-runtime.sh` 通過
  - 限制：「服務回到前一版」在 fixture 層以前一版的 `up -d` 確認；以真實映像走完整失敗回復留給 Task 6.1

### Task 5.3 — `--rollback` 還原映像與 opencode.db
- 產出與範圍：`scripts/install.sh` 的 `rollback_docker`；installer 輸出說明備份之後的對話不保留
- 相依：5.1
- 驗收編號：SCN-015
- 完成判準：rollback 後映像、release 檔案與 opencode.db 都是升級前的備份；備份缺漏時拒絕並說明原因；不還原 opencode.db 的版本會讓測試失敗（反向自檢）
- 驗證：installer 腳本測試
- 狀態：✅ 已完成
- 證據：2026-11-25
  - 前提：`release/data-compatibility.json` 的 `rollbackSafeTo` 自 5b6c45f（2026-07）起停在 `v0.17.1`，`rollback_docker` 要求它等於前一版，所以從 v0.25.1 升到 V2 版後的 `--rollback` 會被拒絕。改為 `v0.25.1`（目前最新 release，也是 RunWuKong 的版本）。限制：這個欄位只能寫一個版本，從更早的版本直接升到 V2 版的部署，rollback 仍會被拒絕；V2 版發布前若又發了 V1 版，要改成那一版（Task 6.3 的發版說明要提）
  - 改動（`scripts/install.sh` 的 `rollback_docker`）：
    - 動部署之前先檢查：讀 `.wukong-release` 的 `stateBackups` 與 `dataCompatibility`；要求備份的名稱沒有備份、而這個部署有該 volume 時拒絕；每份備份要 `tar -tf` 可讀，否則拒絕並指出是哪一份。都說明「nothing was changed」
    - 前一版映像改在目前版本仍在執行時，以備份裡的 docker-compose.yml 拉取（`-f "$backup/docker-compose.yml"`），並比對 `docker image inspect` 的 digest 與升級前記錄的 `previousImageDigest`，不符就拒絕（原本是換檔後才拉、不比對）
    - 有 state 備份時：印出「<名稱>: restoring the backup <路徑>; conversations after it was taken are not kept」→ 停止所有服務 → 以 `backup_docker_state` 把目前內容另存到 `.wukong-backups/<前一版>-rollback-<時間>/volumes/`（失敗時重新啟動目前版本並中止）→ `restore_docker_state` 還原（失敗時服務維持停止，訊息列出兩邊的備份位置）→ 換回前一版的檔案並啟動
    - `docs/docker.md`：rollback 不動 volumes 的說法改為「升級時備份過的 volume 例外」，並寫出升級後的對話不保留、目前內容另存的位置與拒絕條件
  - 測試：新增 `docker-state-rollback` case（併入 `all`），fixture 從 v9.9.9 升到要求備份、啟動時會遷移 opencode.db 的 v9.9.7，再執行 `--rollback`：
    - 成功：opencode-state 與升級前 `diff -r` 相同、docker-compose.yml 與升級前相同、metadata 為 v9.9.9（previousVersion v9.9.7）、有檢查 v9.9.9 映像；輸出含備份路徑與「conversations after it was taken are not kept」；log 順序為 pull → 停止所有服務 → 還原 → `up -d`；另存的 tar 解開後與 rollback 前的 V2 狀態相同
    - 備份 tar 被刪：rollback 失敗、輸出指出 opencode-state，metadata、compose 與 volume 都不變，沒有停止或啟動服務
    - 前一版映像 digest 不符：rollback 失敗、輸出提到 digest，部署與 volume 不變，沒有停止或啟動服務
    - 紅燈：實作前「opencode-state is not the pre-upgrade backup」（diff 顯示 opencode.db 不同、多出 v2-only）。反向自檢：拿掉還原（完成判準要求）→「opencode-state is not the pre-upgrade backup」；拿掉備份可讀性檢查 →「a refused rollback stopped or started services」；拿掉停服務 →「never ran: stop every service」；拿掉 digest 比對 →「rollback went ahead with a different image」
  - 其他檢查：`test-installer-upgrade.sh all`（含既有 `docker-rollback`、`compatibility`）、`test-release-manifest.sh`、`test-release-workflow.sh`、`test-release-image.sh all`、`test-docker-runtime.sh` 通過
  - 限制：備份與還原函式已在 Task 5.1、5.2 以真實 docker 驗過；以真實映像從 V2 版 rollback 到 v0.25.1、並確認 V1 時期的對話可以接續，留給 Task 6.1 的演練

### Task 5.4 — binary 模式的升級說明
- 產出與範圍：`docs/installation.md`：binary 模式要自行安裝 @opencode/cli 2.3.2、升級前備份 opencode 的資料目錄、V2 的單向遷移與回復方式
- 相依：2.7
- 驗收編號：推導行為 (d)
- 完成判準：文件寫出安裝命令、備份與還原路徑，並連到 SCN-011 的錯誤訊息
- 驗證：等價證據（純文件）：照文件在隔離 HOME 實際執行一次
- 狀態：📝 待實作
- 證據：

## Phase 6 — 端到端演練、版本對照與文件

### Task 6.1 — 副本上的端到端演練
- 產出與範圍：rehearsal.md：隔離環境用副本跑 V1 版本 → installer 升級 → 三個入口各跑回合（含提問、取消、排程）→ `--rollback` → 確認 V1 時期對話可接續
- 相依：2.1～2.7、3.1、3.2、5.1～5.3
- 驗收編號：支援 SCN-001～SCN-003、SCN-005、SCN-006、SCN-008、SCN-009、SCN-013、SCN-015
- 完成判準：每一步有命令與觀察結果；失敗步驟照實記錄，不寫成通過
- 驗證：端到端觀測，每項附「失敗時會看到什麼」
- 狀態：📝 待實作
- 證據：

### Task 6.2 — V1 與 V2 的版本對照
- 產出與範圍：用 `docs/issues/issue-0030/scripts/bench-turn.sh` 對 v0.25.1 映像與本版映像各跑一組
- 相依：4.1、2.4
- 驗收編號：GitHub #44 驗收（版本對照）
- 完成判準：記錄耗時、CPU 與 opencode.db 增量，失敗樣本保留；V2 退步時寫出原因或未知
- 驗證：基準測試
- 狀態：📝 待實作
- 證據：

### Task 6.3 — 常青文件
- 產出與範圍：`docs/docker.md`、根目錄 `AGENTS.md`（架構、關鍵設計細節、容器內 opencode 權限）、`.env.example`、`CHANGELOG.md` 的 `[Unreleased]`
- 相依：6.1
- 驗收編號：project.md 常青文件更新責任
- 完成判準：文件描述的介面、變數與 6.1 演練時的實際行為一致
- 驗證：等價證據（純文件）：逐項對照演練紀錄
- 狀態：📝 待實作
- 證據：

## 使用方式對照

| 情境 | 變更前 | 變更後 |
|---|---|---|
| 預設部署 | `WUKONG_AGENT_CMD=opencode run` 使用 opencode-ai 1.x | 同一設定，映像內為 @opencode/cli 2.3.2 |
| server 模式 | 密碼選用 | 必須有密碼（來源依 TBD-2） |
| binary 模式 | PATH 上的 opencode 1.x | 使用者自行安裝 V2；PATH 上是 1.x 時明確報錯 |
| Docker 升級 | 不備份 volume | 停服務、備份 opencode-state 後才啟用 |

## 測試策略

| 驗收 | 層級與策略 |
|---|---|
| SCN-001 | 契約探測：真實 V2 對副本的最小請求，加負向對照 |
| SCN-002、SCN-005～SCN-008、SCN-010、SCN-011 | gateway 單元與整合測試（假 HTTP server 改成 V2 形狀）；Phase 6 以真實 V2 端到端補驗 |
| SCN-003、SCN-004、SCN-009 | 容器層測試：真實 V2 映像，含反向自檢 |
| SCN-012 | 產物檢查：建置後的映像與 manifest |
| SCN-013～SCN-015 | installer 腳本測試（fixture）加 Phase 6 的副本演練 |

需要模型的項目（多棒回合、上游錯誤的實際觸發）用預設免費模型；額度用完的失敗樣本要保留，不能只算成功樣本（#30 步驟 5）。

## 檢查清單

- [ ] SCN-001 成立後才進 Phase 2
- [ ] 所有探測與演練都用副本，正式部署未被開啟
- [ ] TBD-1、TBD-2 在依賴它們的 Task 開始前有結論
- [ ] 常青文件依 project.md 更新
