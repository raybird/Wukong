# Issue 26 實作計畫

## 設計方案

1. **核心原則**：向後相容優先。未指定模型時維持現行行為，用執行當下的 `agent.default_model`；既有排程的 `config_json` 沒有模型欄位也要能讀。
2. **模型清單**：`wukong-web` 新增 `GET /api/models`，執行 `opencode models`，依第一個 `/` 前的字分組成 `{ providers: { [provider]: [完整模型 ID] } }`，並快取結果避免每次開頁都等約 1.4 秒（2026-10-27 本機測試部署量測）。取不到時回明確的錯誤狀態，由前端退回手動輸入。
3. **排程模型**：`JobKind::Turn` 加 `#[serde(default)] model: Option<String>`，舊資料讀成 `None`。執行時以排程模型覆寫該回合 `GatewayConfig.default_model`，套用範圍與預設模型相同（產生回覆的那一棒、補救重送與 session 壓縮）。`SchedulerStore` 提供修改既有 Turn 排程模型的方法，CLI 與 Web 共用。
4. **前端**：Settings 與排程卡片使用同一套兩級選單邏輯：Provider → Model，另有「自訂（手動輸入）」；已存值不在清單時照原值顯示並標「不在清單中」。

## 實作步驟

1. ✅ **SCN-001、SCN-005：探測 `opencode models` 契約並實作 `/api/models`**
   - 相依：無。
   - 產出：探測紀錄（輸出格式、耗時、失敗 exit code、臨時 `OPENCODE_CONFIG` 加自訂 Provider 是否列出、server 模式 `GET /provider` 與 Web 容器清單的比對）；解析與分組函式、`GET /api/models`、快取。
   - 完成判準：以真實輸出為 fixture 的解析測試通過；`opencode models` 失敗、逾時或輸出為空時 API 回可辨識的狀態且不 crash；探測結論寫回本步驟。
   - **契約探測**（2026-10-27，本機測試部署 `~/Documents/RunWuKong` 的 `wukong-web` 容器，opencode 1.19.3，CLI 模式；只用 `/tmp` 下的臨時設定檔，用完即刪）：
     - 格式：stdout 一行一個 `provider/model`，成功時 stderr 為空、exit 0；模型 ID 可含多個 `/`（`nvidia/deepseek-ai/deepseek-v4.1-flash`、自訂的 `probe26/org/model-a`），所以只以第一個 `/` 切出 Provider。
     - 耗時：`docker exec wukong-web opencode models` 兩次各 1411、1452 ms。
     - 失敗：`opencode models nope` 印出 `Provider not found` 並 exit 1。
     - 會列出的 Provider：`OPENCODE_CONFIG=/tmp/probe-26.json`（內含自訂 openai-compatible Provider）會列出 `probe26/*`；帶 `NVIDIA_API_KEY` 會列出 38 個 `nvidia/*`。沒有設定或憑證的 Provider 不會出現（未設定時只有 `opencode/*`）。
     - 清單會變動：同一天先後兩次，opencode 自家的免費模型分別是 12 與 9 個，所以快取設 5 分鐘 TTL，不快取到重啟為止。
     - server 模式：沒有啟動 `opencode serve` 比對 `GET /provider`。改以 `docker-compose.yml` 核對：`opencode-server` 與 `wukong-web` 用同一份 image，掛載同樣的 `opencode-config`、`opencode-state` volume，兩邊都沒有傳 Provider 金鑰類環境變數。Web 服務行程由 entrypoint `export OPENCODE_CONFIG`（`scripts/docker-entrypoint.sh:242`），子行程 `opencode models` 跟 `opencode run` 讀到同一份設定。殘餘風險：使用者只在 `opencode-server` 的 override 加金鑰環境變數時，兩邊清單會不一致，由手動輸入兜底（SCN-005）。
   - **紅綠證據**：
     - 單元紅燈：先寫空殼（`parse_models` 回空、快取不快取、錯誤回 200），`cargo test -p wukong-web --lib models_api` → 7 failed／1 passed，全是斷言不符（例如快取呼叫次數 `left: 2 right: 1`、錯誤狀態碼 `left: 200 right: 502`、分組 `left: []`）。`empty_output_yields_no_providers` 在空殼下本來就成立，保留為防護。
     - 路由紅燈：`models_requires_token_when_set` → `left: 404 right: 401`（路由不存在）。
     - 最小實作後全綠：`cargo test -p wukong-web` → lib 85 passed，其他 test target 全綠。
     - 精煉：檢查過一次，沒有需要改的地方（`Default` 實作是為了 clippy 的 `new_without_default`）。綠燈同上。
     - 端到端（本機 opencode 1.18.31，臨時 DB 與設定檔）：`GET /api/models` 回 200，依 Provider 分組 `[opencode 10, nvidia 37, ollama-cloud 23, openai 24]`，第一次 1.16 秒，第二次命中快取 0.0007 秒；`PATH=/usr/bin:/bin`（找不到 opencode）時回 `HTTP 502`，body 是 `io error: No such file or directory (os error 2)`。
     - 靜態檢查：`cargo fmt --all -- --check` 通過；`cargo clippy -p wukong-web --all-targets --locked -- -D warnings` 通過。
   - **測試對應**：`crates/wukong-web/src/models_api.rs` 的 tests（fixture `crates/wukong-web/tests/fixtures/opencode-models-1.19.3.txt`）。SCN-001：`real_output_is_grouped_by_provider_in_listed_order`、`lines_without_a_provider_and_model_are_ignored`、`cache_reuses_a_result_within_ttl`、`cache_refetches_after_ttl`、`listing_success_is_json_grouped_by_provider`，以及 `lib.rs` 的 `models_requires_token_when_set`。SCN-005（API 端）：`empty_output_yields_no_providers`、`failures_and_empty_listings_are_not_cached`、`listing_failure_is_reported_as_bad_gateway_with_reason`；前端退回手動輸入的部分在步驟 3 驗收。
   - 逾時沿用 `run_opencode_diagnostic` 的 5 秒（`SYSTEM_DIAGNOSTIC_TIMEOUT_SECS`），實測約 1.4 秒；逾時時回 502 `command timed out`。
2. ✅ **SCN-003、SCN-004：排程模型欄位、執行覆寫與 CLI**
   - 相依：無（可與步驟 1 並行）。
   - 產出：`JobKind::Turn.model`；`SchedulerStore` 修改模型；`run_claimed_job` 以排程模型覆寫預設模型；CLI `add-turn --model`、`schedule set-model --id <id> [--model <m>]`（省略 `--model` 即清除）、`schedule list` 顯示模型或「使用預設」；`persona::scheduling_capability_hint` 補上兩個指令。
   - 完成判準：舊版沒有 `model` 的 `config_json` 能讀成 `None`；以假 backend 斷言有模型的排程把該模型送進 `AgentRequest.model`，清除後送出的是當下預設模型；`set-model` 對 Maintenance 排程與不存在的 id 回錯誤。
   - **實作**：`JobKind::Turn` 加 `#[serde(default, skip_serializing_if = "Option::is_none")] model`，沒指定時 `config_json` 跟舊版一字不差，降版也讀得懂。`normalize_model` 把空白視為沒指定。`SchedulerStore::set_turn_model` 回 `Updated`／`NotFound`／`NotTurn`。`execute_job_inner` 在排程有模型時以 `apply_default_model` 覆寫該回合的設定，因此套用範圍跟預設模型一樣（最後一棒、補救重送、壓縮）。`set-model` 對不存在的 id 與維護排程照既有 `enable`／`disable` 的慣例印出原因、exit 0，分別是「找不到排程」與「維護排程不使用模型，無法設定」。
   - **紅綠證據**（2026-10-27）：
     - 結構變更（加欄位，22 個建構處補 `model: None`）後，`cargo test --workspace --locked` 全綠，確認沒有改變既有行為。
     - 外迴圈紅燈：`cargo test -p wukong-cli --test schedule_model` → 2 failed（`left: "turn(scope=project:T)" right: "turn(scope=project:T,model=nvidia/x/y)"`；`left: "找不到排程" right: "維護排程不使用模型，無法設定"`）。
     - 單元紅燈（`--no-fail-fast`）：
       - `a_turn_without_model_is_stored_in_the_old_format`：`left` 多了 `"model":null`。
       - `set_turn_model_sets_trims_and_clears_the_model`：`left: NotFound right: Updated`。
       - `set_turn_model_rejects_maintenance_and_unknown_jobs`：`left: NotFound right: NotTurn`。
       - `turn_job_uses_its_own_model_instead_of_the_default`：`left: Some("opencode/default") right: Some("nvidia/x/y")`。
       - `scheduling_hint_lists_model_commands`：斷言失敗。
     - 空殼階段就綠的防護：`a_turn_saved_before_the_model_field_still_loads`（serde 會把缺少的 `Option` 欄位當成 `None`）、`turn_job_without_model_uses_the_current_default`（現行行為）、`parses_schedule_add_turn_model`、`parses_schedule_set_model_with_and_without_model`（clap 宣告式定義，行為由外迴圈 binary 測試承擔）。
     - 最小實作後全綠：`cargo test -p wukong-scheduler -p wukong-runtime -p wukong-cli -p wukong-gateway --locked --no-fail-fast` 全部 ok（scheduler lib 57、runtime lib 86、gateway lib 139、`schedule_model` 2）。
     - 精煉：檢查過一次，沒有需要改的地方。綠燈同上。
     - 全 workspace：`cargo fmt --all -- --check` 通過、`cargo clippy --all-targets --locked -- -D warnings` exit 0、`cargo test --workspace --locked` 702 passed／0 failed。
     - 真實路徑：用臨時 DB 與一個會記錄 argv 的假 agent（`WUKONG_AGENT_CMD="<scratchpad>/fake-agent.sh run"`，設定檔預設模型 `opencode/default`）實跑 `schedule add-turn --model nvidia/deepseek-ai/deepseek-v4.1-flash` → `schedule list` 顯示 `kind=turn(scope=project:T,model=nvidia/deepseek-ai/deepseek-v4.1-flash)`；`schedule trigger` 時 agent 收到 `--model nvidia/deepseek-ai/deepseek-v4.1-flash`，沒有收到 `opencode/default`。`set-model --id <id>` 清除後再 trigger，agent 收到 `--model opencode/default`。
   - **影響分析**（補跑：應在修改前跑，實際在實作後才跑）：`JobKind` LOW、`execute_job_inner` LOW（上游 `run_scan`）、`scheduling_capability_hint` CRITICAL（每回合最後一棒都會注入，這次多一行說明如何指定模型；runtime 既有 86 個測試全綠）。
   - 常青文件（`docs/cli-reference.md`）依計畫在步驟 4 更新。
3. ✅ **SCN-002、SCN-003、SCN-005：Web API 與兩級選單**
   - 相依：步驟 1、步驟 2。
   - 產出：排程列表回傳模型；修改排程模型的 API；`wukong-settings.js`、`wukong-schedules.js` 的兩級選單與手動輸入退路。
   - 完成判準：API 測試涵蓋設定、清除、非 Turn 排程與不存在的 id；前端測試或實機操作確認切換 Provider 會更新模型選項、儲存後重新載入顯示正確、清單 API 失敗時可手動輸入、不在清單的已存值照原值顯示。
   - **實作**：
     - `GET /api/schedules` 每筆多了 `model`。
     - 新增 `PATCH /api/schedules/:id`，body `{"model": "<id>" | null}`：null 或空白就改回預設；回 200、404，維護排程回 400。路由走既有的 token middleware。
     - 新模組 `static/lib/model-picker.mjs`：純邏輯 `initialPickerState`、`modelsFor`、`pickedModel`，加上 DOM 的 `mountModelPicker` 與 `loadModelCatalog`，Settings 和排程卡片共用。
     - Settings 的文字框換成兩級選單。
     - 排程頁只有對話排程的卡片多了「模型」列、兩級選單、「儲存模型」和「改回預設」；沒有指定模型時「改回預設」是停用的。
   - **紅綠證據**（2026-10-27）：
     - API 紅燈：先放空殼（PATCH 回 501、列表 `model` 固定 null、`/lib/model-picker.mjs` 沒有路由），`cargo test -p wukong-web --lib` → 3 failed。`schedule_model_can_be_set_and_cleared_over_http` 是 `left: 501 right: 200`，`schedule_model_rejects_maintenance_and_unknown_jobs` 是 `left: 501 right: 400`，`serves_static_assets_with_content_types` 是 `left: 404 right: 200`。
     - 前端紅燈：空殼模組下 `node --test crates/wukong-web/static/lib/model-picker.test.mjs` → 6 個全 fail，失敗訊息都是 `ERR_ASSERTION`。
     - 最小實作後全綠：`cargo test -p wukong-web --lib` 87 passed；`node --test` model-picker 6 passed。
     - 精煉：檢查過一次，沒有需要改的地方。綠燈同上。
     - 全 workspace：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` exit 0；`node --test crates/wukong-web/static/lib/*.test.mjs crates/wukong-web/static/components/*.test.mjs` 17 passed。`cargo test --workspace --locked` 第一次跑出 1 個失敗、在 188 passed 時停下；失敗名稱被我當時的彙總指令濾掉，無法辨識是哪一個。之後以 `--no-fail-fast` 連跑三次，都是 704 passed／0 failed，沒有重現。殘餘風險：既有測試中可能有一個時序相關的 flaky 測試，留給 review 與 CI 觀察。
     - 瀏覽器實測（Chrome，本機臨時 wukong-web 搭本機 opencode 1.18.31，臨時 DB 有三個對話排程和一個維護排程）：
       - SCN-002：Settings 一開就停在已存的 `opencode` → `big-pickle`。切到 `nvidia` 後模型選單只剩 38 個 `nvidia/*` 加「請選擇模型」，沒有 `opencode/*`。選 `deepseek-ai/deepseek-v4.1-flash` 儲存後，設定檔 `agent.default_model` 變成 `nvidia/deepseek-ai/deepseek-v4.1-flash`，頁面也顯示這個值。
       - SCN-003：維護排程卡片沒有模型選單。有模型的卡片停在 `nvidia` 和該模型。在「晚報-用預設」選 `opencode/big-pickle` 按儲存、在「晨報-有模型」按「改回預設」後，`wukong schedule list` 分別顯示 `model=opencode/big-pickle` 和 `model=使用預設`。
       - SCN-005：已存值 `custom/not-listed` 的卡片切到手動輸入，保留原值，並標示「目前的模型不在清單中。」。用 `PATH=/usr/bin:/bin` 重啟讓 `/api/models` 回 502 後，Settings 隱藏兩個下拉選單，手動輸入框保留原值並顯示「無法取得模型清單，請手動輸入。」；手動輸入 `opencode/manual-typed` 儲存後寫進設定檔。過程中 console 沒有錯誤。
     - 未驗證：手機寬度的版面。樣式用 `flex-wrap` 讓欄位換行，但沒有實際在窄螢幕看過。
4. ✅ **交付、文件與獨立審查**
   - 相依：步驟 1～3。
   - 產出：`docs/cli-reference.md`（`add-turn --model`、`set-model`）、`docs/entrypoints.md`（Web 模型設定）、`CHANGELOG.md` 的 `[Unreleased]`；建立 PR 並取得獨立審查。
   - 完成判準：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 通過，review 報告判定 PASS。
   - **文件**（2026-10-27，✅）：更新了 `docs/cli-reference.md`（範例、指令表、排程語意的模型一節）、`docs/entrypoints.md`（Web 模型設定、`/set_models` 的影響範圍），以及 `docs/web-console-sop-v1.md`（Settings 與 Schedules 流程、smoke checklist）。`CHANGELOG.md` 的 `[Unreleased]` 加了兩筆 Added。`README.md` 沒有描述模型設定，不需要改。文件裡的事實都已向程式核對：
     - 快取 5 分鐘：`models_api::MODELS_CACHE_TTL`。
     - 套用範圍含補救重送：`turn.rs` 的 repair 請求用 `cfg.default_model`。
     - 套用範圍含壓縮：`run_turn` 用同一份 cfg 呼叫 `prepare_final_session`，裡面的 `compact_session` 帶 `cfg.default_model`。
     - 錯誤訊息文字：`set_model_message`。
   - **PR 與第一輪審查**（2026-10-27）：PR #27 以 `378ade3` 建立。獨立審查判定 PASS，報告見 `review-378ade3.md`（`5a76caa`）。報告沒有 MUST FIX，列了兩項 SHOULD FIX，使用者決定「先修再合併」。
   - **審查後修正**（2026-10-27，✅）：
     - **S-1 排程頁重複載入**（`wukong-schedules.js`）：
       - 紅燈：在 Chrome 對臨時服務實測。假 `opencode` 先 `sleep 3` 再回空清單，所以每次載入清單都要 3 秒；兩個排程都先設成 `custom/old`。清單還沒載完時按「停用」，等 8 秒後在最後一張卡片輸入 `opencode/new-model` 並按「儲存模型」，結果送出 2 個 PATCH：`{"model":"custom/old"}` 與 `{"model":"opencode/new-model"}`。
       - 前一次在兩個排程都沒有模型時做的同樣操作，只送出 1 個 PATCH。原因是過期的選單 `value()` 回空字串，只會顯示「請先選擇模型」，所以重現時必須讓排程先有模型。
       - 修正：`load()` 加上載入世代計數，在每個 `await` 之後檢查，過期的那次直接放棄。
       - 綠燈：同一個腳本只送出 1 個 PATCH，內容是 `opencode/new-model`。刪除情境（覆寫 `confirm`，清單載完前刪掉第一張卡片）結果是剩 1 張卡片、選單只掛一次、沒有未處理的錯誤。這個情境沒有先跑紅燈，它和停用情境是同一個根因。
     - **S-2 `run_fixed` 子行程**（`backend.rs`）：
       - 先跑 GitNexus impact。結果是 CRITICAL，直接呼叫者有 2 個：CLI `/providers`、`/models`（`command.rs`），以及 Web `run_opencode_diagnostic`。三者都是在逾時時放棄等待，加上 `kill_on_drop` 不會改變正常完成時的行為。
       - 紅燈：`run_fixed_kills_the_child_when_the_caller_gives_up` 跑 `sh -c "sleep 1; touch <marker>"`，100 ms 後放棄等待，1.5 秒後 marker 仍然出現，斷言失敗（"the abandoned child kept running"）。
       - 修正：加上 `kill_on_drop(true)`。
       - 綠燈：wukong-gateway lib 測試 140 passed。
     - **N-3 Settings 清單載完前就能儲存**（`wukong-settings.js`）：「儲存模型」按鈕一開始設成 `disabled`，選單掛上後再依 `editable` 啟用。Chrome 實測：進入頁面 300 ms 時是停用，4.8 秒後已啟用。
     - 整體檢查：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked --no-fail-fast` 705 passed、0 failed；`node --test` 17 passed。`detect_changes` 判定 medium，範圍是 3 個檔案。
     - 沒有處理的 NICE TO HAVE：N-4（PATCH body 送 `{}` 等於清除模型）、N-5（PATCH 沒有 401 測試）、N-6（正規化邏輯寫了兩份）。這三項不在使用者同意的範圍內，保留在審查報告中。
   - **第二輪審查**（2026-10-27）：`240f8a1` 判定 PASS，沒有 MUST FIX 或 SHOULD FIX。報告見 `review-240f8a1.md`（提交 `f74abd0`），驗證腳本確認 `f74abd0` 只新增了這份報告。報告另外列了兩項 NICE TO HAVE：一是 `fetch` 拋出時排程頁沒有錯誤訊息，這是改版前就有的問題；二是本步驟 S-2 段落的影響分析描述不精確，結論仍然成立。兩項都沒有處理，保留在報告中。
   - **合併**：PR #27 於 2026-10-28 squash 合併為 `ec9cd97`，合併時的 head 是 `f74abd0`，與審查範圍一致。GitHub #26 已關閉。

## 使用方式對照

| 介面 | 變更前 | 變更後 |
|---|---|---|
| Web Settings | 文字框手動輸入模型 | 先選 Provider 再選 Model，可切換手動輸入 |
| Web Schedules | 只能啟用、停用、刪除 | 卡片顯示模型，可用兩級選單指定或清除 |
| CLI `schedule add-turn` | `--name`、`--cron`、`--scope`、`--prompt` | 另有選用的 `--model <model>` |
| CLI `schedule set-model` | 無 | `--id <id> [--model <model>]`，省略 `--model` 即回到預設 |

## 測試策略

| 驗收 | 層級 | 判準 |
|---|---|---|
| SCN-001 | 單元／API 測試 | 真實輸出 fixture 正確分組；快取命中時不再執行命令 |
| SCN-002 | API 測試＋前端 | 儲存後 `agent.default_model` 更新；選單依 Provider 連動 |
| SCN-003 | 排程單元測試＋CLI 測試 | 排程模型進到 `AgentRequest.model`；`list` 與 Web 顯示該模型 |
| SCN-004 | 反序列化與回歸測試 | 舊 `config_json` 讀成 `None`，執行時送出當下預設模型 |
| SCN-005 | API 測試＋前端 | 清單失敗時回可辨識狀態、前端可手動輸入；不在清單的值不被替換 |
