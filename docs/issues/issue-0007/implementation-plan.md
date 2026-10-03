# Issue 7 實作與驗證計畫

計畫日期：2026-10-03。風險與首要驗證依 [README](README.md#風險與首要驗證)。Task 為唯一進度來源。

## 設計與使用方式

維持 `AiBackend` 與 runtime 公開契約；以 CLI 現有 JSON stream 支援 scope session／progress，最小修補經重現的程序收尾問題。部署切換僅移除預設 server 依賴，保留非空 URL 的 server 路徑。權限與 question 策略尚未決定，不先實作新的允許策略。

## Phase 1 — 基線與 CLI 能力證據

### Task 1.1 — backend 契約與真實 OpenCode 探測

- 產出與範圍：requirement-analysis.md；隔離資料與設定的 OpenCode probe，不碰既有 session。
- 相依：無。
- 驗收編號：SCN-001。
- 完成判準：盤點六類契約與四入口依賴，保存版本／旗標／權限事件證據，差異有決策或明列待確認。
- 驗證：不改行為；程式路徑與真實 CLI 探測；成功、deny／ask 對照證明判準。
- 狀態：⏳ 進行中。
- 證據：已保存程式盤點與 help 結果於 requirement-analysis.md；旗標 probe 與基線測試執行中（2026-10-03）。

### Task 1.2 — 四入口與多回合整合

- 產出與範圍：在現有入口測試或 integration probe 跑 CLI → runtime、Web HTTP／SSE、Telegram dispatch、Scheduler executor。
- 相依：Task 1.1 的 CLI 契約證據可用；互動策略未決不阻塞純文字成功路徑。
- 驗收編號：SCN-002、SCN-003、SCN-005。
- 完成判準：至少兩回合續接、scope 隔離；Web 事件在完成前抵達；最終輸出、remember／recall 與空輸出 fallback 皆有證據。
- 驗證：真實入口搭配 CLI subprocess；假 agent 可驗證入口 plumbing，但不能取代真實 OpenCode session／權限契約。不同失敗面分別保留 runtime 與入口檢查。
- 狀態：📝 待實作。

### Task 1.3 — CLI failure／timeout／取消程序收尾

- 產出與範圍：Gateway 最小修補與 regression tests；不改 backend API。
- 相依：初始規格已提交且完成目標 symbol impact。
- 驗收編號：SCN-006。
- 完成判準：EOF 後仍執行的程序在 deadline 結束、取消 future 不留下 agent；錯誤與原串流行為保持。
- 驗證：先以子程序 PID 與 bounded wait 重現紅燈；最小修復後同組全綠。此層直接測 subprocess 的外部行為與底層生命週期，合併內外迴圈，不另用 mock child 測同一行為。
- 狀態：📝 待實作。

## Phase 2 — 條件式 CLI-first 切換

SCN-004、SCN-007 尚待 TBD-1；不建立可執行 Task。取得決策後在本節補非互動策略驗證與兩份 Compose／entrypoint／env／操作文件切換 Task；相依為 Phase 1 必要證據全數成立。

## Phase 3 — 相容與交付驗證

### Task 3.1 — server 相容路徑與全量檢查

- 產出與範圍：顯式 server opt-in 驗證與交付證據；常青文件對齊。
- 相依：Phase 1 與核准後的 Phase 2。
- 驗收編號：SCN-008、SCN-009。
- 完成判準：非空 URL 仍選 server；部署 opt-in 可用；cargo test、cargo clippy --all-targets -- -D warnings、format 與 Docker runtime 檢查通過。
- 驗證：backend 選擇與 Compose 展開／啟動分別驗證；交付 HEAD 全量測試。依 code-simplify 審查本次變更一次，no-op 時記理由。
- 狀態：📝 待實作。

## 檢查清單

- [x] SCN-001／002／003／005／006／008／009 各有唯一責任 Task；待核准 SCN 不派可執行 Task。
- [x] 先查權限與 adapter 契約，再做入口切片與條件式切換。
- [ ] 所有必要證據成立、TBD-1 已決定。
- [ ] 交付文件、commit、PR 與獨立 review 使用同一固定範圍。
