# Issue 0030 — Runtime 效能盤點與 OpenCode 2 升級評估

## 概述
[GitHub #30](https://github.com/raybird/Wukong/issues/30) 追蹤 raybird-desktop 的舊部署效能症狀、資料庫盤點及 V2 升級可行性。歷史事實見 [交接紀錄](../../2026-11-11-runtime-performance-handover.md)。

2026-11-23 範圍修訂：盤點與對照改在 flashaim 的 RunWuKong（v0.25.1）上做，raybird-desktop 只保留交接紀錄的既有觀測，不再取數；根因結論限定在 flashaim 上的新版。V2 候選版改為 2.3.2。

## 現況

**現行 review**: review-4be256c.md

## 文件清單
- [implementation-plan.md](implementation-plan.md)
- [inventory.md](inventory.md)：盤點報告與可重跑的取數腳本（scripts/）

## 涉及檔案
- 本目錄與交接紀錄：盤點證據與決策；交接紀錄只加 2026-11-23 的追補段落。
- 唯讀評估：release/runtime-inputs.env、Dockerfile、scripts/resolve-opencode-version.sh、crates/wukong-gateway/src/local_process.rs、crates/wukong-gateway/src/opencode_server.rs、crates/wukong-runtime/src/session_retention.rs、crates/wukong-memory/src/store/mod.rs。
- 盤點對象：flashaim 的 ~/Documents/RunWuKong（compose 專案 runwukong）；本 issue 只讀取，量測在一致性副本上做。

## Gherkin 驗收劇本
```gherkin
Feature: Runtime 效能與升級評估
  @SCN-001
  Scenario: 建立可追溯的本機盤點
    Given flashaim 的 RunWuKong 部署版本與原始碼版本分開辨識
    And raybird-desktop 舊部署只有交接紀錄的既有觀測
    When 在一致性副本上盤點版本、日誌與資料庫並驗證取數方法
    Then 報告包含來源主機、日期、聚合數值與限制
    And 無法取得的資料列為未知而非健康
    And raybird-desktop 的觀測不當成 flashaim 的量測結果
  @SCN-002
  Scenario: 評估 OpenCode 2
    Given 已確認候選版本與正式資料的隔離副本
    When 比較 API 契約、資料相容性與效能
    Then 產出升級或暫緩的證據及回復方案
    And 不相容或測試失敗會明列為阻礙
```

## Gherkin 核准紀錄
- **核准 commit**: 83d3670
- **核准來源**: 2026-11-11 使用者：「這個盤點開一個 github issue 把目前這台機器的緣由放上去，然後 opencopde 有 2 版看是否順勢安排升級」；涵蓋盤點與評估計畫，非正式部署升級授權。2026-11-23 逐題確認，使用者兩題都選建議答案：第 1 題「改盤點 flashaim（範圍修訂）」，SCN-001 改盤點 flashaim 的 RunWuKong（v0.25.1），raybird-desktop 只保留交接紀錄的既有觀測、不再取數，根因結論限定在新版（SCN-001 重新核准）；第 2 題「改用 2.3.2」，V2 候選版由 2.2.7 改為 2.3.2（SCN-002 文字不變）。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-11-23 | 已核准 |
| SCN-002 | 2026-11-11 | 已核准 |

## 風險與首要驗證
- 最大風險：V2 核心 API 不相容或改寫正式資料。
- 風險等級與理由：High，主版本契約及遷移尚未確認。
- 首要驗證：flashaim 部署的版本辨識、一致性副本與隔離契約探測。
- 選擇理由：直接驗證版本與資料邊界，避免以升級取代調查。
- 完成證據：版本矩陣、副本完整性、契約成功／失敗結果；正式資料未被試驗改寫。

## 待確認事項
| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-1 | 容器內 OpenCode binary 版本及 DB 分布 | 已解決：2026-11-23 flashaim 的 v0.25.1 內含 OpenCode 1.19.6，四個資料庫的分布見 inventory.md 步驟 1、2 | SCN-001，於盤點取得 |
| TBD-2 | V2 API／資料遷移與回復是否相容 | 已解決：2026-11-23 不相容；API、事件、啟動與認證都變了，V2 就地單向遷移 opencode.db，回復要還原備份，見 inventory.md 步驟 4 | SCN-002，先在隔離環境驗證 |
| TBD-3 | 正式升級與 adapter 改造範圍 | 不影響本次交付：追蹤：無。2026-11-23 go/no-go 為暫緩：V1 仍在維護，release CI 只解析 opencode-ai，不會自動升到改名為 @opencode/cli 的 V2；將來要升級時另開 issue，前提與回復方案在 inventory.md 步驟 6 | 評估後定案，尚未列為實作工作；SCN-002 |
| TBD-4 | raybird-desktop 舊部署（v0.23.0）的症狀根因 | 不影響本次交付：追蹤：無。2026-11-23 使用者選擇改盤點 flashaim；那台機器連不到，舊部署停機且早於 #24／#28 的修正，本次只判斷新版是否仍有同類問題 | SCN-001 |
| TBD-5 | dev-rules-kit 候選版（bd001e6，流程契約 2.7）要不要跟著本 issue 的 squash 進 main（review-4be256c 的 SHOULD FIX 5） | 不影響本次交付：追蹤：dev-rules-kit issue #11（候選版試用與發版）。2026-11-23 使用者選「照現狀合併並在 PR 明列」：PR 說明與 squash 訊息寫明內含候選版；上游發版後若與 da632e9 不同，到 main 執行 init-project.py --update | 不影響 SCN-001、SCN-002 |

## Gate 豁免紀錄

| 日期 | 豁免項目 | 使用者原話 | 殘餘風險 |
|---|---|---|---|
| 2026-11-23 | 計畫步驟 5 的部分量測：v0.25.1 tool 任務的成功樣本、OpenCode V1 與 V2 本身的效能對照、大型 session 的完整回合、多入口同時寫入的 SQLite 等待 | 免費模型額度用完後的逐題確認，選「以現有資料收尾」 | v0.25.1 跑工具的回合耗時、V2 本身是否較快、上游處理長上下文的耗時、寫入競爭都仍未知；go/no-go 不依賴這些項目 |

## Timeline
- 2026-11-11：依使用者要求建立 GitHub #30 與盤點／評估計畫。
- 2026-11-23：規格修訂，SCN-001 改盤點 flashaim 並重新核准，V2 候選版改為 2.3.2；舊核准基線 c4f2e9e（核准 commit 當時為待提交，依核准來源行推導）。
- 2026-11-23：免費模型額度用完，使用者決定步驟 5 以現有資料收尾，未量測項目記入 Gate 豁免紀錄。

**建立日期**: 2026-11-11
**分級**: Medium
**風險**: High
**狀態**: 待審查
