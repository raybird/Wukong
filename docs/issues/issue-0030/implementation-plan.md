# 執行計畫

2026-11-11 建立，2026-11-23 依範圍修訂改寫步驟 1、4、5。風險與首要驗證依 [README](README.md)。本計畫為任務權威來源；GitHub issue 工作清單是建檔摘要。

盤點對象是 flashaim 的 ~/Documents/RunWuKong；raybird-desktop 只引用交接紀錄的既有觀測。V2 候選版為 @opencode/cli 2.3.2（2026-11-23 查 npm latest；GitHub 的 Latest 標記在 v1.19.6）。

## 實作步驟
1. ✅ SCN-001：辨識 flashaim RunWuKong 的版本與資料卷，取得一致性副本。產出版本矩陣／副本驗證；無相依。完成判準：主機名稱、image digest、binary 版本與原始碼分列，副本可讀且不觸發正式資料遷移。
   - 證據（2026-11-23）：[inventory.md「步驟 1」](inventory.md#步驟-1版本矩陣與一致性副本)。映像 v0.25.1（digest 898b73a1…）內含 OpenCode 1.19.6，原始碼釘選 1.17.18 是 release CI 改寫前的值；四個 DB 以唯讀掛載取副本，WAL 反向自檢通過（只有主檔時筆數較少）。
2. ✅ SCN-001：DB/WAL、頁面、各表、最大 session 與工具事件量盤點。依賴 1；產出聚合數值與 SQL。完成判準：用已知筆數對照組證明取數有效，未知不當零值。
   - 證據（2026-11-23）：[inventory.md「步驟 2」](inventory.md#步驟-2資料庫聚合盤點)。對照組 PASS，三種改壞的版本都 FAIL；opencode.db 42 MB 中 event 表占 66%、part 表占 32%，只有 6 個 session，tool part 占 part data 的 81%；保留期清理照設計運作（stale_protected=1、expired=0）；vectors.db 未盤點，列為未知。
3. ✅ SCN-001：EXPLAIN QUERY PLAN 與查詢耗時。依賴 2；產出可重跑命令、資料規模與冷暖測量。
   - 證據（2026-11-23）：[inventory.md「步驟 3」](inventory.md#步驟-3查詢計畫與耗時)。核心查詢都走索引；最貴的是載入最大 session 的 part／event，約 15 ms；正式部署的召回 p95 32 ms。在目前規模下不是瓶頸候選，raybird-desktop 的規模未知。
4. ✅ SCN-002：候選 V2（2.3.2）最小契約與副本遷移／回復探測。依賴 1；涵蓋啟動、health、session、串流、取消、權限、模型、壓縮及清理；產出差異矩陣。失敗即記錄阻礙，不直接更換正式版本。
   - 證據（2026-11-23）：[inventory.md「步驟 4」](inventory.md#步驟-4opencode-v2232最小契約與遷移回復探測)。Wukong 用到的路徑、事件名稱、回應形狀全部改變；啟動訊息與認證不相容；V1 路徑回 200 的網頁 HTML，gateway 的健康檢查會因為不是 JSON 而失敗，compose healthcheck 卻會顯示 healthy。壓縮與換模型只確認端點接受請求。V2 就地、單向遷移同一個 opencode.db，刪掉 V1 的三張表，之後 V1 與 V2 互相看不到對方新寫的 session；回復要還原升級前的備份。遷移與回復由 scripts/v2-migration.sh 重現（不呼叫模型）；契約探測有錯誤密碼的負向對照。
5. ✅ SCN-001／002：同資料、模型與任務的版本對照。依賴 3、4；在 flashaim 的副本上比較 v0.23.0 映像（raybird-desktop 當時的版本）、v0.25.1＋V1、可運作的 V2 路徑。產出分階段耗時、RSS、throttling、寫入等待、DB 增量與全部失敗樣本。V2 契約未通過則記不可比較，不編造效能結果。
   - 證據（2026-11-23）：[inventory.md「步驟 5」](inventory.md#步驟-5版本對照)。short 任務兩版各 3 次；v0.25.1 每回合啟動兩次 opencode（plan、build），合計約 2 秒、約 2.5 CPU 秒；兩次 LLM step 合計 6.6～12.6 秒，占回合耗時的 66～78%；CPU 比 v0.23.0 少約 33%。v0.25.1 的 tool 任務因免費模型額度用完失敗 3 次，保留為失敗樣本。V2 經 Wukong 記為不可比較。未量測項目（v0.25.1 tool 成功樣本、V1／V2 本身對照、大型 session 完整回合、寫入競爭）依使用者決定豁免，見 README 的 Gate 豁免紀錄。
6. ✅ SCN-002：根因排序與升級 go/no-go。依賴 5；交付具版本與證據的決策、正式升級前提與回復方案；需改程式時另定實作範圍。
   - 證據（2026-11-23）：[inventory.md「步驟 6」](inventory.md#步驟-6根因排序與升級-gono-go)。根因依序為上游模型、每回合按需啟動兩次 opencode（約 2 秒、約 2.5 CPU 秒）、opencode.db 成長；升級暫緩（no-go），附將來升級的前提與回復方案；不需要改程式。

## 測試策略
盤點不改產品行為，以可重跑查詢及隔離實驗代替紅綠測試。SCN-001 用已知資料對照避免空結果假綠燈；SCN-002 用失敗契約案例驗證探測器確實報錯。正式部署維持唯讀，需啟停或變更時依 project.md 取得授權。後續程式修改依受影響 crate 與部署腳本補回歸測試。

## 檢查清單
- [x] 舊版與新版證據分開。
- [x] 升級評估與正式部署分開。
- [x] 盤點、對照與升級決策證據齊備（未量測項目見 README 的 Gate 豁免紀錄）。
