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

1. 📝 **Phase 1：首要驗證（SCN-001，另蒐集 SCN-010、TBD-1 的契約）**。產出：探測腳本與結果記錄在本 issue 目錄；無相依。
   - 在副本上讓 V2 遷移，對 memory.db 的 agent_session_state 指向的每個 session ID 查 `GET /api/session/{id}` 與 `/context`，再對其中一個送 prompt，確認 session ID 不變、訊息數增加；以不存在的 ID 做負向對照。
   - 實測 V2 `opencode run --standalone --format json` 的事件形狀、session ID 的取得方式、結束後有無殘留程序，以及帶 `--dangerously-skip-permissions` 時的行為（TBD-1）。
   - 完成判準：README「完成證據」全部成立。SCN-001 不成立時停在本 Phase，修訂規格並重新核准，不進 Phase 2。
2. 📝 **Phase 2：gateway adapter（SCN-002、SCN-004～SCN-008、SCN-010、SCN-011）**。產出：`wukong-gateway` 的 V2 adapter 與回歸測試；Telegram、Web 的回覆呼叫端；依賴 Phase 1。
   - 回歸測試要覆蓋：cursor 分頁超過 100 筆（地雷 W-6）、V2 具名錯誤的分類（地雷 W-7）、權限回覆路由（地雷 W-8）、取消、錯誤密碼、V1 版本偵測、假 agent 不帶 opencode 旗標。
   - 完成判準：CI 三道（fmt、clippy、test）本機全綠；既有的行為測試不改期望值，只改 V1 專屬的介面樣本。
3. 📝 **Phase 3：容器設定（SCN-003、SCN-009）**。產出：V2 格式的 baseline、server 密碼接線、compose healthcheck、`.env.example`；依賴 Phase 1，server 密碼接線另依 TBD-2 的決定。
   - 完成判準：V2 以 baseline 啟動成功；rm 黑名單有反向自檢（拿掉該規則時同一命令會通過）；healthcheck 在 server 停止或密碼錯誤時判 unhealthy；`bash scripts/test-docker-runtime.sh` 通過（地雷 W-3）。
4. 📝 **Phase 4：打包與 release（SCN-012）**。產出：`release/` 三個釘選檔、`resolve-opencode-version.sh`、`test-release-image.sh`、`Dockerfile*`；依賴 Phase 2、3。
   - 完成判準：在 npm 有更新版的情況下解析結果仍是 2.3.2；本機建置的映像內 `opencode --version` 為 2.3.2，與釘選檔、manifest 一致（向產物要答案，地雷 W-1）。
5. 📝 **Phase 5：installer 備份、失敗還原與 rollback（SCN-013～SCN-015）**。產出：`install.sh`、`data-compatibility.json`、`test-installer-upgrade.sh` 等測試；binary 模式的 docs 說明；依賴 Phase 4。
   - 完成判準：測試涵蓋升級備份、啟用失敗還原、rollback 還原三條路徑，各有「沒還原 opencode.db 就失敗」的反向自檢；`scripts/release.sh` 的 `run_required_checks` 全過。
6. 📝 **Phase 6：端到端演練與版本對照**。產出：演練紀錄、`scripts/bench-turn.sh` 的 V1 與 V2 對照、常青文件更新；依賴 Phase 5。
   - 在隔離環境用副本跑 V1 版本 → installer 升級 → 三個入口各跑回合（含提問、取消、排程）→ `--rollback` → 確認 V1 時期對話可接續。
   - bench 記錄耗時、CPU 與 opencode.db 增量；V2 不要求更快，退步要說明。
   - 更新 `docs/docker.md`、`docs/installation.md`、根目錄 `AGENTS.md`（架構、關鍵設計細節、容器內 opencode 權限）、`CHANGELOG.md` 的 `[Unreleased]`。
   - 完成判準：SCN-001～SCN-015 都有對應證據；沒做到的項目明列，不寫成通過。

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
