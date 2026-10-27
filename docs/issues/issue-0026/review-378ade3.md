# 審查報告
- 範圍：完整 PR（GitHub raybird/Wukong #27，TARGET `main`，`6609f3f..378ade3`，6 個提交、28 個檔案；PR 的 `headRefOid` 與本機 HEAD 皆為 `378ade3`，PR 狀態 OPEN）
- Reviewed BASE SHA：6609f3f3b21f4ddfad294841bb5aa09c1de62651
- Reviewed HEAD SHA：378ade32eb400e9d2db51d97e2559749354c5fc6
- Reviewed patch-id：8da17837a09c88970395a65877ac7d763132cf30（2026-10-27 以 `git diff 6609f3f 378ade3 | git patch-id --stable` 重算，與 PR 所列一致）
- 獨立 reviewer：Claude Code 隔離 subagent（Claude，獨立審查角色，未參與本 PR 的實作）
- Review artifact：docs/issues/issue-0026/review-378ade3.md

## 問題與風險

### MUST FIX
無

### SHOULD FIX

1. **排程頁 `load()` 重入時會重複掛載選單與事件，可能送出過時的模型**（`crates/wukong-web/static/components/wukong-schedules.js:26-50`）
   - 狀況：卡片的「啟用／停用／刪除」事件在第 28 行就掛上，接著第 32 行 `await loadModelCatalog(...)`。快取為空時這段等待約 1.4 秒，`opencode models` 逾時時長達 5 秒（失敗不快取，每次載入都會再等一次）。使用者在這段時間按下停用或刪除，`act()` 會再呼叫一次 `load()`，重新產生卡片 HTML。兩次 `load()` 都會在 await 之後執行第 33-50 行的迴圈：
     - 第一次 `load()` 的 `turns` 仍包含已刪除的排程，第 34 行 `querySelector` 回傳 `null`，第 35 行 `card.querySelector` 拋出 TypeError（未處理的 promise rejection），迴圈中斷。
     - 沒被刪除的卡片會被兩次 `load()` 各掛一次。畫面上看到的是第二次掛的選單，但「儲存模型」上有兩個 click handler；第一個 handler 綁的是已脫離 DOM 的舊選單，`picker.value()` 回傳的是初始的已存模型。按一次儲存會送出兩個 PATCH（一個舊值、一個新值），最後存下哪個取決於請求到達順序。
   - 影響：機率低（需要在清單載入期間操作），但後果是排程模型可能被存回舊值。存完會重新載入，卡片會顯示實際存下的值，所以看得出來，不會悄悄出錯。
   - 建議：先 `await loadModelCatalog` 再寫入 `innerHTML` 並掛所有事件；或加一個載入世代計數，await 之後發現有更新的 `load()` 就直接 return。修正時補一個前端測試，或在計畫中記錄可重複的手動重現步驟。

2. **`opencode models` 逾時後子行程不會被終止，而 `/api/models` 是頁面載入時一定會走的路徑**（`crates/wukong-web/src/lib.rs:419-426` 呼叫 `run_opencode_diagnostic`，實際執行在 `crates/wukong-gateway/src/backend.rs:442-456`）
   - 狀況：`run_fixed` 用 `cmd.output()`，沒有設定 `kill_on_drop(true)`。`tokio::time::timeout` 到期時 future 被丟棄，但子行程會繼續執行。這個 helper 是既有的，以前只在 System 診斷頁用到；現在 Settings 與排程頁每次載入都會呼叫，而且失敗不快取。如果 `opencode models` 卡住（例如上游目錄抓取沒有回應），每次開頁就多留一個背景 `opencode` 行程。`ModelCatalogCache` 在執行 fetch 期間持有 mutex（`crates/wukong-web/src/models_api.rs:73`），同時進來的請求會依序各等 5 秒。
   - 影響：本次探測沒有觀察到 hang（找不到 binary 與 exit 1 都會立刻返回），所以這是推論出來的風險，不是已重現的缺陷。
   - 建議：在 `run_fixed` 加上 `kill_on_drop(true)`，一行修正，同時涵蓋既有的 System 診斷。這會改到 gateway 的共用 helper，所以修改前依專案規則跑 impact。或者把這個殘餘風險寫進計畫的限制。

### NICE TO HAVE

3. **Settings 在模型清單載入前，儲存按鈕可以按，但會誤報「模型不可為空」**（`crates/wukong-web/static/components/wukong-settings.js:62,75`）。選單要等 `/api/models` 回來才掛上（最長約 5 秒），這段時間按儲存會因為 `this.modelPicker` 不存在而顯示「模型不可為空。」。改版前文字框是立即可用的。建議載入期間停用按鈕，或顯示「載入模型清單中…」。
4. **PATCH body `{}` 會清除排程模型**（`crates/wukong-web/src/lib.rs:486-488`）。`ScheduleModelRequest.model` 是 `Option<String>`，serde 把缺少的欄位當成 `None`，因此 `{}` 與 `{"model": null}` 等價。文件只寫了 `null`；前端一定會送出 `model` 欄位，所以目前不影響使用。若要嚴格區分，可以要求欄位一定要存在，或在文件中註明。
5. **新增的 PATCH 路由沒有 token 測試**（`crates/wukong-web/src/lib.rs:966-967`）。我核對過，路由註冊在 `route_layer(require_token)` 之前，與既有的 DELETE 共用同一條 `/api/schedules/:id`，所以確實受 token 保護。但測試只有 `models_requires_token_when_set`。可以補一個 PATCH 未帶 token 回 401 的測試，防止日後把路由搬出 protected router。
6. **模型字串的正規化有兩份**：`crates/wukong-scheduler/src/job.rs:70` 的 `normalize_model` 與 `crates/wukong-gateway/src/config.rs:60-65` 的 `apply_default_model` 用的是相同的 trim／filter 邏輯，`assemble_argv` 也會再 trim 一次。scheduler 已經依賴 gateway，可以讓其中一份直接重用另一份；目前行為一致，不影響正確性。

## 已查核維度

### 驗收與證據

規格修訂：`git diff dcb901e 378ade3 -- docs/issues/issue-0026/README.md` 只有回填「核准 commit: dcb901e」這一行，Gherkin 與核准時一致。

| 驗收 | 證據位置 | 審查結論 |
|---|---|---|
| SCN-001 | `crates/wukong-web/src/models_api.rs` tests：fixture 是 opencode 1.19.3 的真實輸出，共 49 行，期望值 `[9, 38, 2]` 是字面值，屬於獨立的真相來源；`lines_without_a_provider_and_model_are_ignored`；快取的命中與過期；`listing_success_is_json_grouped_by_provider`；`lib.rs` 的 `models_requires_token_when_set`（紅燈是 404→401）。計畫步驟 1 有契約探測與端到端紀錄。 | 通過。只用第一個 `/` 切分的寫法已由含多段斜線的真實 ID（`probe26/org/model-a`、`nvidia/deepseek-ai/...`）驗證，不是同義反覆。 |
| SCN-002 | `model-picker.test.mjs`（`modelsFor` 依 Provider 過濾）；計畫步驟 3 的 Chrome 實測。「下一回合生效、不需重啟」是既有行為：我核對了 `chat_api.rs:259`、`telegram/dispatch.rs:1100,1172`、`cli/repl.rs:85` 每回合都會重新讀設定，`schedulerd` 的 `refresh_model_settings` 也有既有測試 `model_settings_follow_changes_and_removal_without_restart`。 | 通過。前端 DOM 層（`mountModelPicker`）沒有自動化測試，以瀏覽器實測作為等價證據，計畫寫得夠具體。 |
| SCN-003 | `crates/wukong-cli/tests/schedule_model.rs`：執行真實 binary，用 `WUKONG_MEMORY_DB` 避開 W-5；`store.rs` 的 `set_turn_model_*`；`executor.rs` 的 `turn_job_uses_its_own_model_instead_of_the_default`：mock 記錄每次 `AgentRequest.model`，斷言最後一棒是排程模型，且全程沒有送出預設模型；`lib.rs` 的 PATCH 測試；`persona.rs` 的 `scheduling_hint_lists_model_commands`；假 agent 實跑時 argv 收到 `--model nvidia/...`。 | 通過。「之後改 Chat 預設模型不影響這個排程」由同一個 executor 測試涵蓋：base cfg 的預設是 `opencode/default`，實際送出的是排程模型。套用範圍我向程式核對過：`turn.rs:260`（最後一棒）、`turn.rs:326`（補救重送）、`session.rs:97`（壓縮）都讀 `cfg.default_model`，而 executor 覆寫的就是這份 cfg；規劃（`orchestrator` 的 `model: None`）與輔助棒（`turn.rs:275`）不指定模型。 |
| SCN-004 | `a_turn_saved_before_the_model_field_still_loads`（直接寫入舊格式 JSON）、`a_turn_without_model_is_stored_in_the_old_format`（紅燈是多了 `"model":null`）、`turn_job_without_model_uses_the_current_default`（守住 `executor.rs:380` 的 `if model.is_some()`：拿掉這個判斷會把預設模型清空，這個測試會紅）；CLI 黑箱測試斷言「使用預設」。 | 通過。「預設也沒設定時不指定模型」沒有專屬測試，但由程式結構保證（`None` 不覆寫，`cfg.default_model` 維持 `None`），既有測試的 `default_model: None` 也會走這條路徑。 |
| SCN-005 | `failures_and_empty_listings_are_not_cached`、`listing_failure_is_reported_as_bad_gateway_with_reason`；`model-picker.test.mjs` 的未列出與空清單案例；Chrome 實測 502 時能手動輸入並儲存、`custom/not-listed` 照原值顯示並有標示。 | 通過。排程卡片在 502 時的退路沒有另外做瀏覽器實測，但與 Settings 共用同一個 `mountModelPicker`，可接受。 |

本次重跑（2026-10-27，HEAD `378ade3`，工作區乾淨）：
- `cargo fmt --all -- --check` → exit 0
- `cargo clippy --all-targets --locked -- -D warnings` → exit 0
- `cargo test --workspace --locked --no-fail-fast` → 跑了 3 次，每次都是 704 passed／0 failed／12 ignored，exit 0。實作者回報的那一次未辨識失敗，本次沒有重現。
- `node --test crates/wukong-web/static/lib/*.test.mjs crates/wukong-web/static/components/*.test.mjs` → 17 pass／0 fail

### 相關失敗面

| 輸入／狀態 | 預期 | 既有覆蓋 | 結論 |
|---|---|---|---|
| 升級前存的 `config_json` 沒有 `model` | 讀成 `None`，用當下的預設模型 | `a_turn_saved_before_the_model_field_still_loads` | 通過 |
| 降版：新版寫入帶 `model` 的 JSON，舊 binary 讀取 | 舊版忽略未知欄位（`JobKind` 沒有 `deny_unknown_fields`），沒指定時格式與舊版相同 | `a_turn_without_model_is_stored_in_the_old_format` | 通過 |
| `set-model` 傳入空白、`None`、維護排程、不存在的 id | 空白視同清除；分別回 `NotTurn`／`NotFound`；維護排程的資料不變 | store 單元測試、CLI 黑箱測試、HTTP 測試（400／404） | 通過 |
| 排程被取得之後才改模型 | 這一次用取得時的快照，下一次才用新模型 | 由程式結構保證（`run_claimed_job` 拿到的是 Job 快照）；`set_turn_model` 不碰 lease 欄位 | 可接受 |
| `set_turn_model` 先讀後寫，沒有包在交易裡 | `config_json` 只有這裡會改，其他寫入者（enable、claim、lease）不碰這一欄；中途被刪除時 `rows_affected=0`，回 `NotFound` | 無專屬測試 | 可接受 |
| `opencode models` 找不到 binary、exit 非 0、逾時 | 回 502 並附原因；不快取；前端退回手動輸入 | API 測試；端到端實測（找不到 binary 回 502） | API 層通過；逾時後子行程殘留見 SHOULD FIX 2 |
| 同時有多個 `/api/models` 請求 | 共用同一次 fetch | mutex 持有期間執行 fetch，邏輯正確；沒有並行測試 | 可接受；失敗時會依序各等一次（SHOULD FIX 2） |
| 已存模型不在清單、清單為空、API 失敗、沒有已存值 | 分別是：手動輸入並標示、手動輸入、手動輸入、停在第一個 Provider 且尚未選模型 | `model-picker.test.mjs` 的 6 個案例；瀏覽器實測 | 通過 |
| 排程頁在清單載入期間被操作 | 選單與事件各掛一次 | 無 | SHOULD FIX 1 |
| XSS：模型 ID、job id 含 HTML 或引號 | 跳脫後顯示 | `option` 用 `textContent`；卡片用 `escapeHTML`（會跳脫 `"` 與 `'`）；selector 用 `CSS.escape` | 通過 |
| PATCH 未帶 token | 401 | 路由在 `route_layer(require_token)` 之內（`lib.rs:966-973`）；middleware 會在 Json extractor 之前執行 | 通過；缺測試，見 NICE TO HAVE 5 |
| 模型字串以 `--` 開頭（argv 注入） | — | 與既有 `agent.default_model`（`put_model_settings` 同樣不驗證格式）相同，都只有持 token 的使用者能設定；argv 以獨立元素傳遞，不經 shell | 不是這次新增的暴露面，不列為發現 |

### 需求、架構、安全、品質（含跨 Task 重複與過度設計）

- **需求**：Q1～Q3 的決策都已落實。Web 只在既有卡片上指定或清除模型，沒有新增「建立排程」的入口，沒有超出範圍。
- **架構**：沒有改任何 `Cargo.toml`，沒有新增相依。`wukong-web` 與 `wukong-cli` 原本就依賴 `wukong-scheduler`，`set_turn_model` 放在 store，CLI 與 Web 共用，符合根目錄 `AGENTS.md` 的單向相依。
- **安全**：新路由 `/api/models` 與 `PATCH /api/schedules/:id` 都在 token middleware 之內；`/lib/model-picker.mjs` 是靜態資源，與其他 lib 一樣是公開的。
- **文件（W-1，向程式核對）**：
  - 「快取 5 分鐘」對應 `MODELS_CACHE_TTL = 300s`。
  - 套用範圍與「規劃與輔助棒不指定模型」對應前面列出的 `turn.rs`、`session.rs`、`orchestrator` 各處。
  - CLI 錯誤訊息與 `set_model_message` 一致。
  - `schedule list` 的格式與 `describe_job_kind` 一致；`turn(scope=` 格式沒有任何程式或腳本在解析。
  - 全部屬實。`README.md`「涉及檔案」沒有列出 `models_api.rs`、`schedule_api.rs`、`model-picker.mjs`、`web-console-sop-v1.md`，只是清單略有落後，不影響行為。
- **跨 Task 重複**：Settings 與排程頁共用 `model-picker.mjs`，沒有重複。模型正規化的重複見 NICE TO HAVE 6。`run_opencode_diagnostic` 是重用既有 helper，做法合理（副作用見 SHOULD FIX 2）。
- **過度設計**：沒有。`ModelCatalogCache` 只有單一快取項、TTL 與「失敗不快取」，都對應到探測結果（清單會變動）與 SCN-005。

### 豁免、待確認與限制

- 沒有 Gate 豁免。
- 實作者揭露的限制（依 verification.md 不構成缺失）：
  - server 模式沒有實際啟動來比對 `GET /provider`；
  - 清單沒有過濾非對話模型；
  - 沒有在手機寬度實測；
  - 步驟 2 的 impact 分析是改完才補跑的（`scheduling_capability_hint` 為 CRITICAL，只多了一行提示，runtime 測試全綠）；
  - 一次未辨識的測試失敗。本次跑 3 次都沒有重現，請在 CI 上留意。
- W-9：本機 clippy 綠燈不代表 CI 綠燈，推送後仍要看 CI 結果。
- 計畫步驟 4 還是 ⏳，README 狀態還是「實作中」，這是交付前的正常狀態。

## 流程判定
PASS

（沒有 MUST FIX；五個核准的驗收都有真實證據，相關失敗面都有覆蓋，或已列為不阻擋的建議。SHOULD FIX 1、2 建議在合併前處理，或由使用者決定接受。修正後 HEAD 會改變，需要重新審查。）
