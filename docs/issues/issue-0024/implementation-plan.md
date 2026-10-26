# Issue 24 實作計畫

## 設計方案

沿既有 gateway → runtime → scheduler 錯誤傳遞路徑修正；先確認 OpenCode 真實契約，再決定最小修改位置。`LocalProcessBackend` 雖由 `opencode run` 設定選入，實際啟動本機 `opencode serve`，透過 server backend 執行請求，不能當成純 CLI stdout 問題處理。風險與首要驗證以 [README](README.md#風險與首要驗證) 為準。

2026-10-26 尚無真實 SSE payload 證據；不預先新增錯誤碼、模型 fallback 或重試機制。

## 實作步驟

1. 📝 **SCN-001、SCN-002：確認契約並重現**
   - 相依：實作前提交已核准規格，修改 symbol 前執行 GitNexus impact 並回報範圍。
   - 產出：隔離 scope／工作目錄中的 OpenCode 1.19.3 去識別事件與訊息樣本，以及能重現額度錯誤被掩蓋的契約測試。
   - 完成判準：記錄啟動／請求命令、版本、事件順序、原始錯誤來源及修正前紅燈；確認是漏接、解析失敗或上游根本未送事件，避免由日誌推定。
2. 📝 **SCN-001、SCN-002：最小修正與回歸驗證**
   - 相依：步驟 1。
   - 產出：必要的 gateway 修改與排程整合測試；保持正常回覆與真正無回應的既有行為。
   - 完成判準：額度錯誤在測試截止時間前回傳原始原因，scheduler 寫入 failure；正常回覆成功、無回應會逾時。記錄紅綠重構證據，並執行 `cargo test -p wukong-gateway -p wukong-scheduler`。
3. 📝 **SCN-001、SCN-002：交付與文件**
   - 相依：步驟 2。
   - 產出：更新 Docker 診斷文件、CHANGELOG、原任務證據與獨立 review。
   - 完成判準：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked` 通過；依 review-evidence 流程審查，提交前執行 GitNexus detect_changes。若實際修改 compose／entrypoint，另跑 `bash scripts/test-docker-runtime.sh`。

SCN-003 尚待 TBD-1，不安排可執行部署任務。決策後補入同一清單，記錄模型選擇、正式變更授權、最小真實請求及指定排程的成果判準；不得用恢復模型額度替代 SCN-001 的修正驗證。

## 使用方式對照

額度拒絕由「等待 1200 秒後只見逾時」改為「回合立即失敗並保留上游額度原因」。排程指令與正常回覆的使用方式維持既有介面。

## 測試策略

| 驗收 | 層級 | 判準 |
|---|---|---|
| SCN-001 | 真實契約探測、gateway 契約測試、scheduler 整合 | 不待 timeout、錯誤原因保留、run 為 failure；契約與寫入分屬不同失敗面，保留兩層 |
| SCN-002 | gateway 回歸與 scheduler 整合 | 正常成功／無回應逾時；既有測試足夠時重用並記錄對應 |
| SCN-003 | 待核准後的正式觀測 | 先以已知額度失敗樣本與正常樣本驗證判準，再記錄指定 run、模型、查詢原文與本次產出；錯誤時必須缺少成功回覆或預期成果，不能只看 healthy／success |

## 檢查清單

- [ ] 真實失敗契約與修正前紅燈已保存。
- [ ] 已核准範圍的紅綠重構證據與回歸檢查齊備。
- [ ] TBD-1 有決策與授權後才執行模型處置。
- [ ] 獨立審查與全部必要驗收完成後才結案。
