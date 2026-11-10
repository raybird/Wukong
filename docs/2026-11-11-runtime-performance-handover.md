# Runtime 效能盤點交接

記錄日期：2026-11-11。追蹤：[GitHub #30](https://github.com/raybird/Wukong/issues/30)。


觀測日期：2026-11-10；主機名稱於 2026-11-11 確認為 raybird-desktop。部署目錄為 ~/Documents/RunWuKong（不是原始碼目錄）。

- Web、Telegram、Scheduler 使用 ghcr.io/raybird/wukong:v0.23.0；觀測時皆停止約 8 天。不能把舊部署問題當成新版仍未修復的證據。
- 三入口實際限制：各 1.5 CPU、2 GiB RAM、256 PID；WUKONG_AGENT_SERVER_URL 空值，使用按需本機 OpenCode。共用 /data、OpenCode state 與 /workspace。
- Web、Telegram ExitCode=143；Scheduler ExitCode=137，但三者 OOMKilled=false，不能將 137 直接判定為 OOM。
- 舊 opencode-server 容器是 Wukong v0.21.11，觀測時已停止約 5 週，不代表目前執行路徑。
- Scheduler 最近 150 行日誌的篩選結果中有 11 筆本機 OpenCode 逾時，整個任務耗時 1,218～2,390 秒；另有 4 筆規劃階段 HTTP 429，耗時 71～79 秒。這是尾端樣本，不是完整錯誤率；未取帶時間戳的原始紀錄，不能推定發生日期。
- 主機停機狀態可用 RAM 約 23 GiB、swap 使用 0；所在 ext4 分割區使用 81%、可用 88 GiB。這些數字不能還原故障當下的壓力。
- 現有容器沒有 Memoria overlay 掛載；不能將其向量模型成本算進已確認瓶頸。
- 已用 docker ps -a、docker inspect 資源／狀態／掛載白名單欄位，以及 docker logs --tail 150 wukong-schedulerd 篩選錯誤。未啟動容器、未改設定、未清資料。

## 已修復事項與版本邊界
先對照 #24（上游錯誤被掩蓋成逾時）、#28（本機模式保留期清理、長期 session 輪替、live events 清理），以及 #11、#13、#22。這些 issue 已關閉，需驗證新版產物與實際部署狀態，不重複假設它們尚未修復。

2026-11-11 原始碼基線：c50d2048ead1364d1841f3834a228b3019fb2feb。前次原始碼檢查的 release/runtime-inputs.env 固定 OpenCode 1.17.18；這不是容器內 OpenCode 的實測版本，盤點須另外取得 binary 版本與 image digest。

## 假設，尚非根因
- OpenCode 長期 session、事件或工具輸出可能造成資料量與讀取成本增加。
- 本機 backend 對執行與 session 控制操作按需啟動程序，可能重複支付啟動／載入成本。
- 共用 SQLite 可能寫入競爭；Wukong Store 已有 WAL、NORMAL、5 秒 busy timeout 與部分索引，不能僅因使用 SQLite 就判定架構不足。
- 1.5 CPU／2 GiB 限制是否造成 throttling 或記憶體壓力，仍缺實測。
- 上游相似案例僅供定位，不代表本機命中或 V2 已修復：
  - https://github.com/anomalyco/opencode/issues/16729
  - https://github.com/anomalyco/opencode/issues/46341

## OpenCode 2 升級評估
2026-11-11 查證：
- 官方已發布 https://github.com/anomalyco/opencode/releases/tag/v2.2.7 ，列為本次候選版本，不假定永遠是最新。
- 官方安裝套件為 @opencode/cli：https://opencode.ai/docs/
- 官方遷移文件明列 server API／client、plugin API 的破壞性變更：https://opencode.ai/docs/migrate-v1/
- Wukong 按需本機模式也經由 server API，不能只替換 opencode-ai 的版本號。
- 需驗證封裝／版本解析／integrity、啟動偵測、health、session CRUD、prompt／串流事件、取消、壓縮、模型選擇、權限回覆與 retention 契約。
- V1／V2 設定位置共用；用獨立設定與資料副本測試，保留原始備份。不得假設 V2 寫入後可由 V1 直接回退。
- 建議安排為本 issue 的評估工作；實際 adapter 改造與正式升級在相容性／效能證據後另定範圍，不以升級代替根因調查。
