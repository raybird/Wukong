# Issue 24 - 排程模型額度耗盡與上游錯誤被掩蓋為逾時

## 概述

[GitHub #24](https://github.com/raybird/Wukong/issues/24)。2026-10-26 調查確認正式排程使用的 `opencode/big-pickle` 達免費用量上限；上游已拒絕請求，排程卻等待約 1200 秒才顯示逾時。追蹤模型額度處置，以及立即回報真實失敗原因的修正。

事故證據與已知限制見 [調查紀錄](../../2026-10-26-scheduler-quota-handover.md)。

## 文件清單

- [implementation-plan.md](implementation-plan.md)

## 關鍵差異

| 項目 | 變更前 | 目標 |
|---|---|---|
| 額度不足 | 等到 agent timeout 才顯示逾時 | 收到上游錯誤即失敗，保留額度原因 |
| 模型可用性 | 已確認達免費用量上限 | 經核准處置後取得真實請求及排程成果證據 |

## 涉及檔案

- `crates/wukong-gateway/src/local_process.rs`：本機 OpenCode 回合生命週期及逾時，依契約調查結果決定是否修改。
- `crates/wukong-gateway/src/opencode_server.rs`、`crates/wukong-gateway/src/opencode_server/`：SSE／訊息錯誤處理及契約測試。
- `crates/wukong-gateway/src/upstream_error.rs`：錯誤解析，僅在實際 payload 證明有缺口時修改。
- `crates/wukong-scheduler/src/executor.rs`：排程結果整合測試，優先沿用既有正式邏輯。
- `docs/docker.md`：額度失敗診斷方式。
- `CHANGELOG.md`：修正後使用者可見行為。
- 正式部署 `/data/settings.json`：僅在模型處置方案選定且另行授權後調整；不在本次建檔中操作。

## Gherkin 驗收劇本

```gherkin
Feature: 排程正確回報模型錯誤並恢復模型服務

  @SCN-001
  Scenario: 額度不足立即失敗並顯示原因
    Given 排程使用的模型會回覆免費額度不足錯誤
    When OpenCode 收到上游拒絕
    Then 回合在收到錯誤後結束而不等待 agent timeout
    And 排程結果為 failure 且包含額度不足的原因

  @SCN-002
  Scenario: 正常回覆與真正無回應仍正確處理
    Given 模型可正常回覆或完全沒有回應
    When 排程呼叫模型
    Then 正常回覆可完成回合
    And 完全沒有回應的回合仍受既有逾時保護

  @SCN-003
  Scenario: 核准模型處置後恢復正式排程
    Given 使用者已選定恢復額度或替代模型並核准正式部署變更
    When 隔離 scope 的最小真實請求與指定正式排程執行完成
    Then 記錄實際使用模型與真實回覆
    And 指定排程有本次執行產生的預期成果與可查核證據
```

## Gherkin 核准紀錄

- **核准 commit**: SCN-001／SCN-002：`3e12253`；SCN-003：待提交
- **核准來源**: 2026-10-26 使用者「依照建議建立 issue」，承接模型額度處置與立即回報上游錯誤兩項建議；SCN-002 為回歸保護。2026-10-26 使用者另以「scn-003 也核准」核准 SCN-003 驗收內容。具體模型處置方案仍依 TBD-1 選定，驗收核准不代表已取得正式恢復證據。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-26 | 已核准 |
| SCN-002 | 2026-10-26 | 已核准 |
| SCN-003 | 2026-10-26 | 已核准 |

## 實作步驟來源

任務與驗證證據統一維護於 [implementation-plan.md](implementation-plan.md)。

## 風險與首要驗證

- **最大風險**：OpenCode 1.19.3 在額度不足時送出的 SSE／訊息契約尚未確認，單憑文字日誌修改 parser 可能無法修復真正缺口。
- **風險等級與理由**：High；核心外部失敗行為尚未確認。影響集中於既有 gateway 與排程驗證，規模為 Medium。
- **首要驗證**：隔離環境重現並取得真實失敗 payload、事件順序及是否進入 idle，建立修正前會失敗的契約測試。
- **選擇理由**：直接確認錯誤從何處遺失，比先修改 timeout 或猜測錯誤分類更能解除未知。
- **完成證據**：保留去識別 payload 與重現命令；測試能區分額度拒絕、正常回覆及無回應。正式資料庫與工作目錄不承擔破壞性實驗。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-1 | 選定恢復原模型額度或具體替代模型及其正式處置範圍 | 已解決 | 2026-10-26 使用者選定切換為 `nvidia/deepseek-ai/deepseek-v4.1-flash`；原設定備份至 `/data/settings.json.bak-20261026`，最小真實請求探測通過，正式排程已由該模型接手正常運作 |
| TBD-2 | OpenCode 1.19.3 的額度失敗事件契約 | 已解決 | 2026-10-26 確認收到 stream error 時 SSE 靜默掛起，由本機 `--print-logs` 與 stderr 監聽擷取 |

## Timeline

| 日期 | 異動 | 負責人 |
|---|---|---|
| 2026-10-26 | 依唯讀事故調查建立 #24、驗收範圍與風險優先計畫；模型處置待決策 | Codex |
| 2026-10-26 | 實作完成，獨立代碼審查 PASS（review-66ad52a.md），已建立 PR #25 | Antigravity |
| 2026-10-26 | 使用者「scn-003 也核准」；SCN-003 改為已核准，補入依賴模型處置決策的驗證步驟 | Codex |
| 2026-10-26 | 依使用者指示選定方案一切換為 NVIDIA 模型；最小真實請求與正式排程驗證通過，完成收尾 | Antigravity |

---
**建立日期**: 2026-10-26  
**分級**: Medium  
**風險**: High  
**狀態**: 已完成（PR #25 等待合併；SCN-003 正式模型已恢復）
