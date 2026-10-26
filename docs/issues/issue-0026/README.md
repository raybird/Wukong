# Issue 26 - 支援對話與排程獨立模型設定及 Provider 兩級連動選單

## 概述

[GitHub #26](https://github.com/raybird/Wukong/issues/26)。2026-10-26 使用者提出需求：在 Settings 中調整 Chat 的預設對話模型，且每個排程都能獨立挑選模型；並搭配 Docker 容器內可載入的 Provider 使用 Select 挑選後，再連動列出可以切換的模型。

## 任務分級與風險

- **分級**：Medium（涉及 `wukong-settings`、`wukong-scheduler`、`wukong-web` 與 `wukong-cli` 多個模組，結構複雜度中等）。
- **風險**：中風險（需保障既有排程的 SQLite/JSON 向後相容性，未指定模型時無縫 Fallback；且需探測 OpenCode Provider/Model 動態清單 API 契約）。

## 文件清單

- [implementation-plan.md](implementation-plan.md)

## 關鍵差異

| 項目 | 變更前 | 目標 |
|---|---|---|
| 模型設定粒度 | 全域共用單一 `agent.default_model` | Chat 與各排程（Turn Job）可獨立配置專屬模型 |
| 模型設定介面 | 純文字輸入框，需手動輸入完整識別碼 | 提供「選擇 Provider → 連動列出可用 Model」的兩級 Select 選單，並保留手動輸入彈性 |
| 排程資料結構 | `JobKind::Turn { scope, prompt }` 無模型欄位 | `JobKind::Turn { scope, prompt, model }`，向後相容支援空值 fallback |
| 可用模型探索 | 前端無法得知容器內已載入哪些模型 | 後端提供 API 動態查詢 OpenCode 支援之 Provider 與模型分組清單 |

## 涉及檔案

- `crates/wukong-settings/src/lib.rs`：Settings 資料結構與向後相容解析。
- `crates/wukong-scheduler/src/job.rs`、`store.rs`、`executor.rs`：`JobKind::Turn` 擴充 `model` 欄位與執行時模型套用邏輯。
- `crates/wukong-cli/src/schedule.rs`：CLI `wukong schedule add-turn` 增加 `--model` 選項。
- `crates/wukong-web/src/lib.rs`：新增模型清單 API 與排程建立/修改 API 欄位支援。
- `crates/wukong-web/static/components/wukong-settings.js`：Chat 預設模型兩級連動下拉選單。
- `crates/wukong-web/static/components/wukong-schedules.js`：排程建立/編輯對話框之兩級連動選單。
- `docs/docker.md`、`CHANGELOG.md`：文件與更動紀錄。

## Gherkin 驗收劇本

```gherkin
Feature: 支援對話與排程獨立模型設定及 Provider 兩級連動挑選

  @SCN-001
  Scenario: 後端動態提供可用 Provider 與模型清單
    Given 容器內 OpenCode 已配置一或多個可用 Provider（如 opencode, nvidia 等）
    When 前端呼叫模型清單 API
    Then 回傳按 Provider 分組的可用模型清單
    And 包含提供商識別碼與完整模型 ID

  @SCN-002
  Scenario: Web Settings 支援兩級連動挑選 Chat 預設模型
    Given 使用者開啟 Web Console 的 Settings 頁面
    When 選擇特定 Provider 下拉選項
    Then 模型下拉選單動態更新為該 Provider 可用的模型清單
    And 儲存後更新 Chat 預設對話模型並即時生效

  @SCN-003
  Scenario: 每個排程支援獨立設定專屬模型
    Given 使用者建立或編輯排程回合
    When 透過 Web 介面（兩級連動選單）或 CLI 旗標指定特定模型
    Then 該排程儲存其獨立的模型設定
    And 該排程執行時使用其專屬模型而非預設模型

  @SCN-004
  Scenario: 既有排程與未指定模型時向後相容 Fallback
    Given 既有未設定專屬模型的排程
    When 該排程被排程守護行程觸發執行
    Then 自動回退使用全域/預設對話模型
    And 執行記錄與回報維持正常
```

## Gherkin 核准紀錄

- **核准 commit**: 待提交
- **核准來源**: 2026-10-26 使用者明確要求：「需要能在 setting 調整 chat 的預設對話模型，然後每個排程都能獨立挑選模型，需要搭配 docker 內可以載入的 provider 使用 select 挑選後在列出可以切換的模型」；本次依此建立規格。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-26 | 待核准 |
| SCN-002 | 2026-10-26 | 待核准 |
| SCN-003 | 2026-10-26 | 待核准 |
| SCN-004 | 2026-10-26 | 待核准 |

## 實作步驟來源

- Medium 任務：實作步驟維護於 [implementation-plan.md](implementation-plan.md)。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-1 | OpenCode 取得可用模型清單之最佳方式（CLI `opencode models` vs HTTP `/model` 端點） | 待確認 | SCN-001 後端實作方式；先以真實契約探測確認響應速度與快取策略 |
| TBD-2 | 非預期或離線 Provider 之自訂模型字串輸入保留 | 待確認 | SCN-002／SCN-003 前端是否允許手動輸入模式（預設保留以容納特殊模型） |

## Timeline

| 日期 | 事件 | 記錄人 |
|---|---|---|
| 2026-10-26 | 依使用者指示建立 GitHub #26 與初始規格架構 | Antigravity |

---
**建立日期**: 2026-10-26  
**最後更新**: 2026-10-26  
