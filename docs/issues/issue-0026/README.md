# Issue 26 - 支援對話與排程獨立模型設定及 Provider 兩級連動選單

## 概述

[GitHub #26](https://github.com/raybird/Wukong/issues/26)。2026-10-26 使用者提出需求：在 Settings 中調整 Chat 的預設對話模型，且每個排程都能獨立挑選模型；並搭配 Docker 容器內可載入的 Provider 使用 Select 挑選後，再連動列出可以切換的模型。

2026-10-27 核對現況：`agent.default_model` 是唯一的預設模型，Web 對話、Telegram、CLI REPL 與 schedulerd 共用，各自在回合開始前（schedulerd 在取得排程前）重新讀取，Settings 改了下一回合就生效；它只套用在產生回覆的那一棒（含補救重送與 session 壓縮），規劃與輔助棒不指定模型。Web 排程頁只能啟用、停用、刪除，CLI 也只有 `add-turn` 新增，兩者都沒有編輯排程的入口。

## 關鍵差異

| 項目 | 變更前 | 變更後 |
|---|---|---|
| 模型設定粒度 | 只有 `agent.default_model` | `agent.default_model` 作為 Chat 預設模型與排程 fallback；各 Turn 排程可另存專屬模型 |
| Settings 模型介面 | 文字輸入框，需手動輸入完整識別碼 | Provider → Model 兩級選單，保留「自訂（手動輸入）」 |
| 排程模型 | `JobKind::Turn { scope, prompt }`，一律用預設模型 | 新增選用的 `model`；未設定時用當下預設模型 |
| 排程改模型的入口 | 無 | CLI `add-turn --model`、`schedule set-model`；Web 排程卡片上的兩級選單 |
| 可用模型探索 | 前端無從得知容器內有哪些模型 | 後端以 `opencode models` 取得清單並依 Provider 分組 |

## 涉及檔案

- `crates/wukong-scheduler/src/job.rs`、`store.rs`、`executor.rs`：`JobKind::Turn` 加選用 `model`、修改既有排程模型、執行時以排程模型覆寫預設模型。
- `crates/wukong-cli/src/`（`ScheduleOp` 定義與 `main.rs`）：`add-turn --model`、新增 `set-model`、`list` 顯示模型。
- `crates/wukong-runtime/src/persona.rs`：排程能力提示補上指定與修改模型的指令。
- `crates/wukong-web/src/lib.rs`：`GET /api/models`、修改排程模型的 API、排程列表回傳模型。
- `crates/wukong-web/static/components/wukong-settings.js`、`wukong-schedules.js`：兩級連動選單。
- `docs/cli-reference.md`、`docs/entrypoints.md`、`CHANGELOG.md`：常青文件與更動紀錄。

## Gherkin 驗收劇本

```gherkin
Feature: 支援對話與排程獨立模型設定及 Provider 兩級連動挑選

  @SCN-001
  Scenario: 後端依 Provider 分組提供容器內可用的模型清單
    Given 容器內 OpenCode 的 `opencode models` 列出一或多個 `provider/model`
    When 前端呼叫模型清單 API
    Then 回傳依 Provider 分組的清單
    And 每個模型帶有 Provider 識別碼與完整模型 ID（`provider/model`）

  @SCN-002
  Scenario: Settings 以兩級連動選單設定 Chat 預設模型
    Given 使用者開啟 Web Console 的 Settings 頁面
    When 選擇某個 Provider
    Then 模型選單只列出該 Provider 的模型
    When 選定模型並儲存
    Then `agent.default_model` 更新為該模型
    And Web、Telegram 的下一個回合與未指定模型的排程都改用它，不需重啟

  @SCN-003
  Scenario: 每個排程可指定並修改自己的模型
    Given 一個 Turn 排程
    When 以 CLI `schedule add-turn --model`、CLI `schedule set-model`，或 Web 排程卡片的兩級選單指定模型
    Then CLI `schedule list` 與 Web 排程頁顯示該排程的模型
    And 排程執行時，產生回覆的那一棒使用該模型（套用範圍與預設模型相同）
    And 之後修改 Chat 預設模型不影響這個排程

  @SCN-004
  Scenario: 未指定模型的排程沿用預設模型
    Given 升級前建立、儲存資料沒有模型欄位的排程，或模型已被清除的排程
    When 該排程被觸發執行
    Then 使用執行當下的 `agent.default_model`；預設模型也未設定時，與現行一樣不指定模型
    And 排程列表顯示為「使用預設」
    And 執行紀錄與回報維持正常

  @SCN-005
  Scenario: 模型清單取不到或已存模型不在清單時仍可設定
    Given 模型清單 API 失敗或回傳空清單
    When 使用者在 Settings 或排程卡片設定模型
    Then 選單退回手動輸入，輸入的模型仍可儲存
    Given 已存的模型不在目前的清單中
    When 使用者開啟 Settings 或排程頁
    Then 照原值顯示並標示「不在清單中」，不被自動替換成其他模型
```

## Gherkin 核准紀錄

- **核准 commit**: 待提交
- **核准來源**:
  - 2026-10-26 使用者需求原話：「需要能在 setting 調整 chat 的預設對話模型，然後每個排程都能獨立挑選模型，需要搭配 docker 內可以載入的 provider 使用 select 挑選後在列出可以切換的模型」。
  - 2026-10-27 逐題確認（dev-cycle 對話）：Q1 選「沿用同一個（建議）」——Chat 預設模型即 `agent.default_model`，同時是未指定模型排程的 fallback（SCN-002、SCN-004）；Q2 選「再加 CLI 改模型指令」——Web 只在既有排程卡片上指定或清除模型，CLI `add-turn` 加 `--model`，另新增 `schedule set-model --id <id> [--model <m>]`（SCN-003、SCN-004）；Q3 選「opencode models＋可手動（建議）」——後端執行 `opencode models` 依 Provider 分組並快取，保留手動輸入，清單取不到時退回手動輸入，已存模型不在清單時照原值顯示並標示（SCN-001、SCN-005）。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-27 | 已核准 |
| SCN-002 | 2026-10-27 | 已核准 |
| SCN-003 | 2026-10-27 | 已核准 |
| SCN-004 | 2026-10-27 | 已核准 |
| SCN-005 | 2026-10-27 | 已核准 |

## 實作步驟來源

任務與驗證證據維護於 [implementation-plan.md](implementation-plan.md)。

## 文件清單

- [implementation-plan.md](implementation-plan.md)

## 風險與首要驗證

- **最大風險**：`opencode models` 列出的清單與實際能執行的模型不一致。正式主機的 nvidia 這類自訂 Provider 是否會列出、server 模式下 Web 容器看到的清單是否等於 `opencode serve` 能用的，都還沒確認；不一致時使用者會從清單選到跑不起來的模型，或找不到正在用的模型。
- **風險等級與理由**：Medium。外部 CLI 輸出契約有未知，但失敗只影響選單內容，手動輸入與既有 fallback 可以兜底，不涉及資料損失或權限；排程資料存在 `config_json`，加選用欄位不需 schema 遷移。
- **首要驗證**：在本機測試部署（`~/Documents/RunWuKong`，CLI 模式）與臨時 OpenCode 設定目錄做契約探測：記錄 `opencode models` 的輸出格式、耗時、失敗時的 exit code，以及在臨時 `OPENCODE_CONFIG` 加一個自訂 Provider 後是否列出；再比對 server 模式的 `GET /provider` 與 Web 容器 `opencode models` 是否一致。
- **選擇理由**：清單的來源決定 SCN-001 的實作方式與 SCN-005 的觸發條件，先確認契約比先寫 UI 更能避免返工；探測只讀或使用臨時設定，不動正式部署。
- **完成證據**：探測命令、去識別輸出與結論寫進 implementation-plan 步驟 1；以真實輸出為 fixture 的解析測試，涵蓋正常、空輸出與 exit code 非 0。

## Timeline

| 日期 | 事件 | 記錄人 |
|---|---|---|
| 2026-10-26 | 依使用者指示建立 GitHub #26 與初始規格架構 | Antigravity |
| 2026-10-27 | 核對現況後逐題確認 Q1～Q3；規格修訂為 SCN-001～005 並核准 | Claude Code |

---
**建立日期**: 2026-10-26  
**分級**: Medium — 跨 wukong-scheduler、wukong-cli、wukong-runtime、wukong-web，沿既有相依方向，不改 schema  
**風險**: Medium  
**狀態**: 實作中
