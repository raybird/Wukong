# Issue 7 — CLI-first execution

## 概述

依 [GitHub issue 7](https://github.com/raybird/Wukong/issues/7)，驗證 OpenCode CLI 能支援共用 runtime 的必要行為後，才將部署預設切回 CLI；保留 server 作為明確啟用的相容路徑。不刪除 server、不重設 Gateway、不引入 provider、不重構 session cleanup。

## 文件清單

- [requirement-analysis.md](requirement-analysis.md)：依程式盤點契約與差異。
- [implementation-plan.md](implementation-plan.md)：唯一任務來源與驗證證據。
- [probe-opencode.py](probe-opencode.py)：隔離真實 OpenCode 契約與四入口探針。

- [technical-analysis.md](technical-analysis.md)：按需本機控制程序的決策與邊界。
- [verification.md](verification.md)：2026-10-04 交付驗證與限制。
- [probe-managed.py](probe-managed.py)：真實 OpenCode 問答、入口與 Docker 探針。

## 關鍵差異

| 項目 | 現況 | 目標 |
|---|---|---|
| 一般程序 | 未設 server URL 即使用 CLI | 一般 opencode run 按需啟動控制程序；自訂命令維持純 CLI |
| Docker Web／Telegram／Scheduler | URL 與 depends_on 預設要求 server | 證據成立後預設 CLI |
| server adapter | 支援互動 question／permission 回覆與 session 管理 | 保留顯式啟用 |
| 本機執行互動策略 | 純 run 無回覆通道 | 每回合啟動本機 server API 控制程序，結束後回收 |

## 涉及檔案

```text
crates/
├── wukong-cli/src/main.rs             # 完全沒有串流文字時輸出 runtime fallback
├── wukong-cli/tests/cli_backend.rs    # 真正 binary、scope 與記憶驗證
├── wukong-gateway/src/backend.rs       # backend 選擇與 CLI 程序生命週期
├── wukong-gateway/src/local_process.rs # 按需控制程序與問答路由
├── wukong-gateway/src/opencode_server.rs # 本機 adapter 的設定與官方 question endpoint
├── wukong-runtime/                    # CLI session／記憶／末棒整合驗證
├── wukong-web/                        # 真實 HTTP／SSE 入口驗證
├── wukong-telegram/                   # 訊息 dispatch 入口驗證
└── wukong-scheduler/                  # 無人值守入口驗證
docker-compose{,.release}.yml          # 條件成立後調整預設與 server opt-in
docker-compose.memoria.yml             # runtime 掛到真正執行 agent 的各入口
scripts/install.sh                     # 停止同 project 已停用的 server
scripts/test-docker-runtime.sh         # 部署行為驗證
scripts/test-support/{cli_backend,managed_actual}.rs # 四入口共用 fixture
scripts/test-installer-upgrade.sh       # profile 停用與 opt-in 升級對照
.env.example                          # 部署參數
docs/{docker,entrypoints}.md           # 使用與相容路徑
docs/issues/issue-0007/                # 範圍與證據
```

具體測試檔由各 Task 選擇最小既有驗證入口；不修改記憶 schema、Telegram transport 或 server cleanup。

## Gherkin 驗收劇本

```gherkin
Feature: 可驗證的 CLI-first execution
  @SCN-001
  Scenario: 先取得 backend 行為基線
    Given CLI 與 server adapter 的實作與四入口呼叫點
    When 盤點 session、stream、tool、permission、failure 與 lifecycle
    Then 每項差異具有可追溯的程式或執行證據
    And server-only 差異在切換前明確決定

  @SCN-002
  Scenario: 同 scope 多回合續接
    Given CLI backend 與隔離的記憶資料庫
    When 同一 scope 執行至少兩回合
    Then 後一回合使用前一回合捕獲的 session
    And 不同 scope 不共享 session

  @SCN-003
  Scenario: 四條入口可執行 CLI 回合
    Given CLI backend
    When 由 CLI、Web、Telegram、Scheduler 各自執行真實入口
    Then 各入口取得最終輸出
    And Web 在回合完成前可觀察到 progress 或 streaming event

  @SCN-004
  Scenario: 無人值守權限不等待互動 stdin
    Given Scheduler 使用 CLI backend 且 stdin 不可互動
    When 工具遇到權限或 question
    Then 權限依既有 Reject 或顯式 AllowOnce 策略處置
    And 一般 question 在無人值守時拒絕
    And 不永久等待詢問

  @SCN-005
  Scenario: 末棒輸出與記憶不受切換破壞
    Given CLI backend
    When 正常回合或末棒沒有文字輸出
    Then directive 與 fallback 保證非空最終輸出
    And remember 與 recall 仍正常

  @SCN-006
  Scenario: 失敗與逾時釋放 CLI 程序
    Given CLI 程序失敗或停止輸出而未結束
    When 收到錯誤或達到回合 deadline
    Then 回合以錯誤正常結束
    And 不留下永久執行的 agent child process

  @SCN-007
  Scenario: 預設部署不需要常駐 server
    Given 每回合程序的問答與生命週期證據成立
    When 使用預設部署設定啟動服務
    Then 不需要常駐獨立 opencode-server 服務
    And 回合需要時啟動容器內本機控制程序並在收尾後回收
    And 閒置時沒有該控制程序佔用資源
    And 文件與實際預設相符

  @SCN-008
  Scenario: 保留明確 server 相容路徑
    Given 使用者顯式設定 server URL 與啟用 server 服務
    When 執行 Agent turn
    Then 仍使用 server backend

  @SCN-009
  Scenario: 交付版本通過必要檢查
    Given 完整交付版本
    When 執行 cargo test 與 cargo clippy --all-targets -- -D warnings
    Then 所有測試通過且沒有 lint 警告

  @SCN-010
  Scenario: CLI backend 保留互動問答
    Given 使用者從互動入口執行 CLI backend 回合
    When Agent 提出 question
    Then 使用者可看到問題並回答或取消
    And 回覆送回原回合且 Agent 能繼續完成
    And 不同回合的問題與答案不交叉
```

## Gherkin 核准紀錄

- **核准 commit**: 335f987
- **核准來源**: 2026-10-03 使用者 `/dev-cycle issue:7` 指向既有 issue 範圍；引用 issue 原文：「server backend 暫時保留為 optional / fallback」、「僅在上述證據成立後」切換預設。
- **SCN-004／007／010 核准來源**: 2026-10-03 使用者要求「Cli 也要支援互動問答才行」，查詢 Docker 每回合方案後說明「主要原本是一直啟用一個 server 會直接佔用資源」。依此採每回合本機 server API 程序，保留互動與既有排程權限策略，閒置回收；不是純 run 支援問答，也不新增自動放行權限。

| Scenario | 核准日期 | 狀態 |
|---|---|---|
| SCN-001 | 2026-10-03 | 已核准 |
| SCN-002 | 2026-10-03 | 已核准 |
| SCN-003 | 2026-10-03 | 已核准 |
| SCN-004 | 2026-10-03 | 已核准 |
| SCN-005 | 2026-10-03 | 已核准 |
| SCN-006 | 2026-10-03 | 已核准 |
| SCN-007 | 2026-10-03 | 已核准 |
| SCN-008 | 2026-10-03 | 已核准 |
| SCN-009 | 2026-10-03 | 已核准 |
| SCN-010 | 2026-10-03 | 已核准 |

## 風險與首要驗證

- **最大風險**：CLI 是否支援 session、串流與無人值守權限；切換可能失去 server-only 互動，或讓程序永久等待。
- **風險等級與理由**：High；涉及權限與四入口核心外部行為尚未確認。
- **首要驗證**：先盤點 adapter 契約，探測真實 OpenCode 版本／旗標與非互動行為，再跑最窄的真實入口。
- **選擇理由**：權限契約探測直接降低安全未知；四入口切片另驗證 callback／session 在整合路徑中是否成立，單元測試無法代替入口證據。
- **完成證據**：可重複命令、實際事件順序、至少兩回合 session、權限成功與拒絕對照、逾時後程序消失；不得以 HTTP 200 或最終非空文字取代串流與權限證據。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|---|---|---|---|
| TBD-1 | 2026-10-03 使用者要求 CLI 也支援互動問答，非互動降級方案不採用 | 已解決 | 新增 SCN-010；切換前必須取得互動證據 |
| TBD-2 | 2026-10-03 使用者指出核心目的是避免常駐 server 佔用資源，採容器內每回合啟動本機 server API 程序、結束回收；不維護第三方修改版 | 已解決 | SCN-004／007／010；先驗證問答、續接與程序收尾，再切部署 |

## Timeline

| 日期 | 異動 | 負責人 |
|---|---|---|
| 2026-10-03 | 從 issue 7 建立範圍與計畫；確認 server-only 互動需決策 | Codex |
| 2026-10-03 | 使用者要求 CLI 保留互動問答；新增 SCN-010，查核固定版本 run／ACP 控制通道 | Codex |
| 2026-10-03 | 使用者確認核心問題為常駐 server 資源佔用；規格改為每回合啟停本機控制程序 | Codex |
| 2026-10-04 | 按需控制程序、CLI 問答與部署預設完成；真實四入口、Docker 及全量驗證通過，證據保存於 verification.md | Codex |

---
**建立日期**: 2026-10-03
**分級**: Large — 四入口與部署跨模組驗證
**風險**: High
**狀態**: 實作與交付驗證完成；準備 PR 與獨立審查
