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

2026-11-24 decompose：README 的風險策略（High，首要驗證 SCN-001）仍成立，Phase 1 先於所有實作。Task ID 固定，不重編。

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

推導行為 (d)（binary 模式 docs）由 5.4 負責；GitHub #44 驗收的版本對照由 6.2 負責；常青文件由 6.3 負責。

## Phase 1 — 首要驗證：V1 session 可在 V2 接續，取得 V2 真實契約樣本

SCN-001 不成立時停在本 Phase，依 acceptance.md 修訂規格並重新核准，Phase 2 以後都不開始。

### Task 1.1 — V1 session 在 V2 遷移後可接續
- 產出與範圍：`docs/issues/issue-0044/scripts/` 的探測腳本與結果紀錄（issue 目錄內新檔 probe-session.md）；用 #30 副本的再複製，隔離容器
- 相依：無
- 驗收編號：SCN-001
- 完成判準：README「完成證據」全部成立：memory.db 的 agent_session_state 指向的每個 session 在 V2 的 `GET /api/session/{id}` 都查得到，`/context` 含遷移前的訊息；對其中一個送 prompt 後 session ID 不變、訊息數增加；不存在的 ID 回查不到（負向對照）
- 驗證：契約探測（真實 V2 對副本的最小請求）。送 prompt 的那一步需要模型，用預設免費模型；額度用完時記為失敗樣本，改天重試，不以列得到 session 代替接續
- 狀態：📝 待實作
- 證據：

### Task 1.2 — V2 CLI 路徑契約
- 產出與範圍：probe-cli.md：`opencode run --standalone --format json` 的事件形狀、session ID 取得方式、`--print-logs` 的上游錯誤輸出、結束後有無殘留程序；不加 `--standalone` 時的程序對照；帶 `--dangerously-skip-permissions` 時的行為（解決 TBD-1）
- 相依：無
- 驗收編號：支援 SCN-010；TBD-1
- 完成判準：每項都有實際命令與輸出；殘留程序以 `ps` 比對前後；TBD-1 記下結論與日期
- 驗證：契約探測，隔離 HOME 與 XDG 目錄
- 狀態：📝 待實作
- 證據：

### Task 1.3 — V2 事件與互動樣本
- 產出與範圍：`crates/wukong-gateway` 測試 fixture 用的 V2 原始樣本（文字串流、思考、工具、完成、中止、上游具名錯誤、permission.asked、form），取自真實 V2 server 的 `/api/event`；以 #30 的 v2-probe.sh 為起點
- 相依：無
- 驗收編號：支援 SCN-002、SCN-005、SCN-006、SCN-007
- 完成判準：每種事件至少一份真實樣本存進 fixture，記取得命令與 V2 版本；上游錯誤樣本來自真實錯誤（額度用完或無效模型），不手寫
- 驗證：等價證據（樣本是之後測試的獨立真相來源，避免同義反覆）
- 狀態：📝 待實作
- 證據：

## Phase 2 — gateway adapter 改用 V2

每個 Task 的單元測試以 Task 1.3 的真實樣本當輸入；既有行為測試不改期望值，只換掉 V1 專屬的介面樣本。完成判準都包含 CI 三道（fmt、clippy、test）本機全綠。

### Task 2.1 — server backend 的連線、認證與 session 管理
- 產出與範圍：`opencode_server.rs`：`/api/...` 路徑、`data` 包裝、basic auth（使用者名稱 opencode）、健康檢查改 `GET /api/info`、session 建立／查詢／刪除／建立時間、列表跟著 cursor 走完
- 相依：1.1、1.3
- 驗收編號：SCN-004、SCN-008
- 完成判準：錯誤密碼時就緒檢查失敗並指出認證被拒，V2 網頁的 200 不算健康；假 server 造 150 個 session 分兩頁以上，最舊的仍被列出（地雷 W-6）；保留期清理與輪替刪除走新的刪除端點
- 驗證：外迴圈為 gateway 整合測試（假 V2 server）；單元測試覆蓋分頁與錯誤解析
- 狀態：📝 待實作
- 證據：

### Task 2.2 — prompt 與事件串流
- 產出與範圍：`opencode_server.rs`、`opencode_server/event_map.rs`：prompt 改 `POST /api/session/{id}/prompt` 後等事件，文字、思考、工具事件對應到既有 `StreamEvent`，完成判定改用 V2 的 execution 事件；附件、換模型（`POST /api/session/{id}/model`）；壓縮改 `compact`
- 相依：2.1
- 驗收編號：支援 SCN-002
- 完成判準：以真實樣本重播，入口收到的 `StreamEvent` 序列與回覆文字與 V1 同情境相同；附件與模型參數出現在送出的請求裡
- 驗證：單元測試（樣本重播）加整合測試
- 狀態：📝 待實作
- 證據：

### Task 2.3 — 取消與上游錯誤分類
- 產出與範圍：取消改 `interrupt` 並辨認 interrupted 事件；`upstream_error.rs`、`stream.rs` 改讀 V2 具名錯誤的結構化欄位
- 相依：2.2
- 驗收編號：SCN-005、SCN-007
- 完成判準：取消後回合回報已取消，不等 agent timeout；額度用完樣本分類為上游限流，模型下架分類為模型下架，exit 0 或降級文字不被當成功（地雷 W-7）；拿掉分類邏輯時測試會紅
- 驗證：單元測試（真實樣本）加整合測試
- 狀態：📝 待實作
- 證據：

### Task 2.4 — 本機按需模式
- 產出與範圍：`local_process.rs`：等 `server listening on`，每次啟動自設隨機密碼並帶 basic auth，操作結束後關閉程序
- 相依：2.1、2.2
- 驗收編號：SCN-002
- 完成判準：以真實 V2 2.3.2 跑整合測試：單棒、多棒、附件、指定模型的回合都得到回覆並記錄 session；回合結束後沒有殘留的 opencode 程序（前後 `ps` 比對）；密碼不出現在命令列參數
- 驗證：外迴圈為接真實 V2 的整合測試（需要模型的項目用預設免費模型）；單元測試覆蓋啟動偵測與逾時
- 狀態：📝 待實作
- 證據：

### Task 2.5 — 提問與權限回覆
- 產出與範圍：gateway 的回覆路由改 V2 的 form 與 permission 端點；Telegram（`dispatch.rs`）與 Web 的呼叫端跟著改，使用者操作不變
- 相依：2.2
- 驗收編號：SCN-006
- 完成判準：回答、自訂回答、取消各送到正確端點且回合繼續或結束；無法辨識的權限選項仍視為錯誤；無人值守路徑在 baseline 權限下不出現詢問（地雷 W-8）；既有 Telegram 問答測試不改期望值
- 驗證：gateway 單元測試加 Telegram／Web 既有測試
- 狀態：📝 待實作
- 證據：

### Task 2.6 — CLI backend
- 產出與範圍：`backend.rs` 的 `assemble_argv` 與 `stream.rs`：opencode 路徑加 `--standalone`，NDJSON 改 V2 形狀；TBD-1 的結論要求的處理
- 相依：1.2、2.3
- 驗收編號：SCN-010
- 完成判準：帶額外旗標的 `opencode run` 取得回覆與 session，結束後沒有背景 service 程序；`printf fixer` 假 agent 的既有測試照過，argv 不含任何 opencode 旗標
- 驗證：單元測試（argv 與 NDJSON 樣本）加接真實 V2 的整合測試
- 狀態：📝 待實作
- 證據：

### Task 2.7 — V1 版本偵測
- 產出與範圍：在 opencode 類 backend 第一次呼叫前檢查版本，1.x 時回傳明確錯誤
- 相依：1.1
- 驗收編號：SCN-011
- 完成判準：PATH 上放回報 1.x 的假 opencode 時，回合立即失敗，錯誤含「需要 OpenCode V2」與偵測到的版本，耗時遠低於 agent timeout；2.x 時通過
- 驗證：單一迴圈：整合測試以假 opencode 執行檔覆蓋這一層，沒有另外的底層失敗面
- 狀態：📝 待實作
- 證據：

## Phase 3 — 容器設定

### Task 3.1 — V2 格式的 opencode 設定 baseline
- 產出與範圍：`scripts/docker-entrypoint.sh` 的 baseline 改 V2 格式（權限動作名稱、external_directory、compaction、tool_output、watcher）
- 相依：1.1
- 驗收編號：SCN-009
- 完成判準：真實 V2 以 baseline 啟動成功；對絕對路徑的遞迴刪除被拒（拿掉規則時同一命令會通過的反向自檢）、/workspace 內刪除允許、/tmp 存取不詢問；user.json 的鍵覆蓋 baseline；`bash scripts/test-docker-runtime.sh` 通過
- 驗證：容器層測試（真實 V2 映像）
- 狀態：📝 待實作
- 證據：

### Task 3.2 — 常駐 server 模式的密碼與 healthcheck
- 產出與範圍：compose 三個檔（含 release、memoria overlay）的密碼接線與 opencode-server healthcheck（帶認證打 `/api/info`）、`.env.example`
- 相依：2.1；TBD-2 的決定（開始前要有結論）
- 驗收編號：SCN-003
- 完成判準：入口與 opencode-server 用同一組密碼時就緒檢查通過、回合成功；server 停止或密碼錯誤時 healthcheck 判 unhealthy；每個服務都接到變數（地雷 W-3，`test-docker-runtime.sh`）
- 驗證：容器層整合測試
- 狀態：📝 待實作
- 證據：

## Phase 4 — 打包與 release

### Task 4.1 — @opencode/cli 2.3.2 固定釘選
- 產出與範圍：`release/` 的 package.json、package-lock.json、runtime-inputs.env；`scripts/resolve-opencode-version.sh`、`scripts/test-release-image.sh`、`Dockerfile.release`、`Dockerfile`
- 相依：1.1
- 驗收編號：SCN-012
- 完成判準：npm 有更新版時解析結果仍是 2.3.2；本機建置的映像內 `opencode --version` 為 2.3.2，與釘選檔、manifest 一致（向產物要答案，地雷 W-1）；`test-release-image.sh` 通過，並在版本不符時失敗
- 驗證：產物檢查
- 狀態：📝 待實作
- 證據：

## Phase 5 — installer 備份、失敗還原與 rollback

### Task 5.1 — 從 V1 版本升級時備份 opencode-state
- 產出與範圍：`release/data-compatibility.json`（affectedState 含 opencode-state、backupRequired true）；`scripts/install.sh` 依 backupRequired 停服務、備份 volume、把位置記進 `.wukong-release`
- 相依：4.1
- 驗收編號：SCN-013
- 完成判準：installer 測試：升級時先停服務再備份，備份內容與升級前的 opencode.db 一致，metadata 記錄位置；backupRequired 為 false 時不備份（對照）
- 驗證：installer 腳本測試（fixture）
- 狀態：📝 待實作
- 證據：

### Task 5.2 — 新版啟用失敗時還原 opencode.db
- 產出與範圍：`scripts/install.sh` 的啟用失敗路徑
- 相依：5.1
- 驗收編號：SCN-014
- 完成判準：模擬啟用失敗時，release 檔案與 opencode.db 都回到升級前；不還原 opencode.db 的版本會讓測試失敗（反向自檢）
- 驗證：installer 腳本測試
- 狀態：📝 待實作
- 證據：

### Task 5.3 — `--rollback` 還原映像與 opencode.db
- 產出與範圍：`scripts/install.sh` 的 `rollback_docker`；installer 輸出說明備份之後的對話不保留
- 相依：5.1
- 驗收編號：SCN-015
- 完成判準：rollback 後映像、release 檔案與 opencode.db 都是升級前的備份；備份缺漏時拒絕並說明原因；不還原 opencode.db 的版本會讓測試失敗（反向自檢）
- 驗證：installer 腳本測試
- 狀態：📝 待實作
- 證據：

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
