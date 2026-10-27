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
2. 📝 **SCN-003、SCN-004：排程模型欄位、執行覆寫與 CLI**
   - 相依：無（可與步驟 1 並行）。
   - 產出：`JobKind::Turn.model`；`SchedulerStore` 修改模型；`run_claimed_job` 以排程模型覆寫預設模型；CLI `add-turn --model`、`schedule set-model --id <id> [--model <m>]`（省略 `--model` 即清除）、`schedule list` 顯示模型或「使用預設」；`persona::scheduling_capability_hint` 補上兩個指令。
   - 完成判準：舊版沒有 `model` 的 `config_json` 能讀成 `None`；以假 backend 斷言有模型的排程把該模型送進 `AgentRequest.model`，清除後送出的是當下預設模型；`set-model` 對 Maintenance 排程與不存在的 id 回錯誤。
3. 📝 **SCN-002、SCN-003、SCN-005：Web API 與兩級選單**
   - 相依：步驟 1、步驟 2。
   - 產出：排程列表回傳模型；修改排程模型的 API；`wukong-settings.js`、`wukong-schedules.js` 的兩級選單與手動輸入退路。
   - 完成判準：API 測試涵蓋設定、清除、非 Turn 排程與不存在的 id；前端測試或實機操作確認切換 Provider 會更新模型選項、儲存後重新載入顯示正確、清單 API 失敗時可手動輸入、不在清單的已存值照原值顯示。
4. 📝 **交付、文件與獨立審查**
   - 相依：步驟 1～3。
   - 產出：`docs/cli-reference.md`（`add-turn --model`、`set-model`）、`docs/entrypoints.md`（Web 模型設定）、`CHANGELOG.md` 的 `[Unreleased]`；建立 PR 並取得獨立審查。
   - 完成判準：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 通過，review 報告判定 PASS。

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
