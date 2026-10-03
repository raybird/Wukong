# Issue 7 實作與驗證計畫

計畫日期：2026-10-03。風險與首要驗證依 [README](README.md#風險與首要驗證)。Task 為唯一進度來源。

## 設計與使用方式

維持 `AiBackend` 與 runtime 公開契約；既有 JSON stream 的非互動證據保留。2026-10-03 使用者要求問答並指出常駐 server 資源佔用，採每次執行啟動本機 OpenCode server API 程序、收尾回收，重用現有 server adapter；保留任意命令的純 CLI 與顯式遠端 server 路徑。不以純文字測試宣稱互動等價。

## Phase 1 — 基線與 CLI 能力證據

### Task 1.1 — backend 契約與真實 OpenCode 探測

- 產出與範圍：requirement-analysis.md；隔離資料與設定的 OpenCode probe，不碰既有 session。
- 相依：無。
- 驗收編號：SCN-001。
- 完成判準：盤點六類契約與四入口依賴，保存版本／旗標／權限事件證據，差異有決策或明列待確認。
- 驗證：不改行為；程式路徑與真實 CLI 探測；成功、deny／ask 對照證明判準。
- 狀態：✅ 已完成（2026-10-03）。
- 證據：2026-10-03 基線 `b986493` 的程式盤點與版本／旗標探測見 requirement-analysis.md；`python3 docs/issues/issue-0007/probe-opencode.py` 的 deny／ask／非互動對照與 session 對照已取得，見下方 Task 1.2。互動差異明列為 TBD-1，不代表已核准切換。

### Task 1.2 — 四入口與多回合整合

- 產出與範圍：在現有入口測試或 integration probe 跑 CLI → runtime、Web HTTP／SSE、Telegram dispatch、Scheduler executor。
- 相依：Task 1.1 的 CLI 契約證據可用；互動策略未決不阻塞純文字成功路徑。
- 驗收編號：SCN-002、SCN-003、SCN-005。
- 完成判準：至少兩回合續接、scope 隔離；Web 事件在完成前抵達；最終輸出、remember／recall 與空輸出 fallback 皆有證據。
- 驗證：真實入口搭配 CLI subprocess；假 agent 可驗證入口 plumbing，但不能取代真實 OpenCode session／權限契約。不同失敗面分別保留 runtime 與入口檢查。
- 狀態：✅ 已完成（2026-10-03）。
- 證據（工作區：9935708 後的 CLI 修補與新增測試）：
  - `cargo test -p wukong-cli -p wukong-web -p wukong-telegram -p wukong-scheduler --test cli_backend`：exit 0，5 項通過。CLI binary 同 scope 兩回合 session 相同、不同 scope 新建；前回合 Assistant 文本進入召回 prompt；Telegram dispatch 與 Scheduler executor 都續接兩回合。
  - 真實 OpenCode `1.18.31` 搭本機 OpenAI-compatible fixture，沿用上述相同 5 項測試全部通過：CLI 2 項／Scheduler 1 項／Telegram 1 項／Web 1 項。可重跑命令為 `python3 docs/issues/issue-0007/probe-opencode.py`；資料與設定全部在印出的 `/tmp/issue7-opencode-*`。不啟動常駐 opencode serve，未用既有認證或 session。
  - session 對照：真實 session `ses_efe6184b2ffeXLY8SCl0jYsRPi` 兩回合均回 `HISTORY_PRESENT`；不帶 -s 的新 session `ses_efe6177dfffez4cAGbng9VVVhn` 回 `NO_HISTORY`。判準由 provider 實際收到的歷史決定，不能只靠 JSON sessionID 相同判綠。
  - Web 在完成 marker 尚不存在時收到 `event: tool`，之後收到 answer 與 done、沒有 error。若串流延遲到 child 結束才交付，marker 斷言會失敗；HTTP 200 不是唯一判準。
  - SCN-005 紅燈：`cargo test -p wukong-cli --test cli_backend cli_empty -- --nocapture` exit 101，stdout 只有換行而 runtime 已產生 sentinel。修補 `run_one` 在未收到非空 Text 時印出 `TurnOutput.text` 後同組全綠；正常串流回答斷言只出現一次，避免重複輸出。
  - 空輸出測試先用 User token 查詢 Assistant sentinel，因 User／Assistant 分別存放而失敗；改用已知 Assistant 文字「本回合未產生文字輸出」查詢後能召回 sentinel。這次是測試目標修正，不是產品回歸，也不作紅燈證據。
  - 分層：CLI 真正 binary 的外部 stdout 測試直接覆蓋丟棄 fallback 的失敗面，無其他內部邏輯改動，合併內外迴圈；四入口整合與真實 OpenCode 契約探針另保留。
  - 限制：模型輸出可控制，沒有對外真實 LLM 請求；Telegram 替換 transport，沒有發送外部訊息；Scheduler 測 executor 而非真實 cron 等候；Web 使用真實 Axum router／body stream，沒有瀏覽器 UI 或部署容器。這些限制不等同 deployment SCN-007 已通過。

### Task 1.3 — CLI failure／timeout／取消程序收尾

- 產出與範圍：Gateway 最小修補與 regression tests；不改 backend API。
- 相依：初始規格已提交且完成目標 symbol impact。
- 驗收編號：SCN-006。
- 完成判準：EOF 後仍執行的程序在 deadline 結束、取消 future 不留下 agent；錯誤與原串流行為保持。
- 驗證：先以子程序 PID 與 bounded wait 重現紅燈；最小修復後同組全綠。此層直接測 subprocess 的外部行為與底層生命週期，合併內外迴圈，不另用 mock child 測同一行為。
- 狀態：✅ 已完成（2026-10-03）。
- 證據（工作區：9935708 後的 Gateway 修補）：
  - GitNexus upstream impact：`AgentCliBackend.run`／`run_streaming` 各 LOW、3 個直接測試呼叫者、0 個圖上 process；trait 動態呼叫仍須由 runtime／入口測試保障。
  - 紅燈：`cargo test -p wukong-gateway agent_cli_ -- --nocapture` exit 101；6 通過、2 失敗。EOF 後等待超過外層 3 秒；取消 future 後 plain／streaming 兩個模式 PID 均存活。失敗測試在斷言前清理自己的 probe child。
  - 最小修補：兩條 spawn 設 kill_on_drop；stream EOF 後 child.wait 繼續共用回合 deadline。未改公開 trait 或 server 行為。
  - 綠燈：`cargo test -p wukong-gateway -p wukong-runtime -p wukong-scheduler` exit 0，127／69／27 項（223）通過；含新 EOF timeout、取消存活與舊 stderr／NDJSON／server regression。
  - 沙箱內一輪因 socket PermissionDenied 失敗，且 /proc PID namespace 使存活判準不可靠；不是產品紅燈。以上紅綠都在允許 socket 的同一主機環境取得。
  - `cargo clippy -p wukong-gateway -p wukong-runtime -p wukong-scheduler --all-targets -- -D warnings`、`cargo fmt --all -- --check`、`git diff --check` exit 0。
  - code-simplify：production 改動已為 builder 開關與既有 deadline 共用，no-op；沒有為此重構無關模組。取消測試保障直接 agent child，未宣稱涵蓋任意工具自行 daemonize 的孫程序。

## Phase 2 — 互動能力與條件式 CLI-first 切換

### Task 2.1 — CLI 雙向問答契約與整合

- 產出與範圍：固定版本官方控制通道查核、真實 question／reply／cancel 探針；方案成立後才修改 Gateway 與互動入口。
- 相依：2026-10-03 修訂 SCN-004／007／010 規格提交；固定版本啟停、續接與問答探針成立後才改產品。
- 驗收編號：SCN-010。
- 完成判準：CLI、Web、Telegram 的問題可回答或取消，回覆不跨回合；具真實 OpenCode 工具執行結果，不能只測偽造 question 事件。
- 驗證：先查外部契約，再跑隔離資料的真實工具問答與各入口；對照回答、取消與錯誤／逾時清理。Scheduler 權限由後續 SCN-004 Task 負責。
- 狀態：⏳ 進行中（2026-10-03；唯讀契約查核已完成，尚未實作）。
- 證據：requirement-analysis.md「互動要求與控制通道查核」。run 禁止 question，ACP 未轉送 question；尚無可直接採用的完整純 stdio 通道，不將原始碼查核寫成真實問答通過。
- 規格提交：2026-10-03 `c25fb47`；新增 SCN-010 已依使用者原話核准。文件核對 Scenario 與表格集合一致、SCN-010 唯一責任 Task 為 2.1，`git diff --check` 通過；本次未修改產品程式，不重跑程式測試。

### Task 2.2 — 無人值守處置

- 產出與範圍：Scheduler 使用每回合本機控制程序，沿用 Reject／AllowOnce 與 question 拒絕策略。
- 相依：Task 2.1 控制通道與回覆路由成立。
- 驗收編號：SCN-004。
- 完成判準：Reject 無工具副作用、AllowOnce 可執行且不永久授權、一般 question 拒絕；回覆失敗與逾時不留程序。
- 驗證：真實 OpenCode 許可工具對照與 executor 整合，測試實際副作用及退出，不只看回傳碼。
- 狀態：📝 待實作。

### Task 2.3 — 部署改為閒置無控制程序

- 產出與範圍：兩份 Compose、必要 entrypoint 設定、env 與操作文件；獨立 server 只在 opt-in 啟動。
- 相依：Phase 1、Task 2.1／2.2。
- 驗收編號：SCN-007。
- 完成判準：容器真實回合含問答可完成、續接 session；收尾後 PID 與埠消失，閒置沒有 opencode 控制程序；服務限制承擔本機執行成本。
- 驗證：Compose 展開加 Docker 真實程序／session／問答測試、錯誤及逾時回收；不以配置文字宣稱資源已釋放。
- 狀態：📝 待實作。

## Phase 3 — 相容與交付驗證

### Task 3.1 — server 相容路徑與全量檢查

- 產出與範圍：顯式 server opt-in 驗證與交付證據；常青文件對齊。
- 相依：Phase 1 與核准後的 Phase 2。
- 驗收編號：SCN-008、SCN-009。
- 完成判準：非空 URL 仍選 server；部署 opt-in 可用；cargo test、cargo clippy --all-targets -- -D warnings、format 與 Docker runtime 檢查通過。
- 驗證：backend 選擇與 Compose 展開／啟動分別驗證；交付 HEAD 全量測試。依 code-simplify 審查本次變更一次，no-op 時記理由。
- 狀態：📝 待實作。

## 檢查清單

- [x] SCN-001 至 SCN-010 各有唯一責任 Task。
- [x] 先查權限與 adapter 契約，再做入口切片與條件式切換。
- [ ] 所有必要證據成立（TBD-1／2 已於 2026-10-03 解決）。
- [ ] 交付文件、commit、PR 與獨立 review 使用同一固定範圍。

## 工作區交付檢查（2026-10-03）

Task 1.1／1.2／1.3 已完成；2026-10-03 Task 2.1／2.2／2.3 尚未完成。SCN-008 的顯式 server 選擇仍維持原程式，部署 opt-in 與全量交付有效性由 Task 3.1 在切換後驗證。

- 最終 production code 與新增測試：`cargo test` exit 0，全部 workspace 測試通過；`cargo clippy --all-targets -- -D warnings` 首輪指出新 CLI 測試不必要的右側參照，改成等價比較後 exit 0。此修改只有比較表達式，CLI 整合 2 項另重跑；其餘全量結果可重用。
- 已保存的 `python3 docs/issues/issue-0007/probe-opencode.py` exit 0；permission 3 組、session 3 回合對照與真實 OpenCode 四入口共 5 項均通過。最終續接 session 為 `ses_efe5a176affeQr3vl0lvWd5Ofc`；新 session 對照為 `ses_efe5a0b07ffec2ThOuHhsL22We`。
- `cargo fmt --all -- --check`、共用 fixture `rustfmt --edition 2021 --check scripts/test-support/cli_backend.rs`、`git diff --check` 通過；純文件連結存在且核准集合／責任 Task 映射正確。初始文件提交使用 Markdown 雙空白換行，被 diff --check 指出；後續改為無尾端空白，不將初次檢查寫成通過。
- 精煉：Web 測試刪除無作用的區塊；production code 保留最小修補。保存探針只做 AST 相同的排版，再實際重跑全套真實探針通過。

尚未建立 PR、未執行獨立審查、未合併；不以能力證據宣稱 CLI-first 完成。
