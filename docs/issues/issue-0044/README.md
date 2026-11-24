# Issue 44 - OpenCode V2 換版：adapter 改寫與部署換版，行為不變

## 文件清單
- [requirement-analysis.md](./requirement-analysis.md)
- [technical-analysis.md](./technical-analysis.md)
- [implementation-plan.md](./implementation-plan.md)

## 概述

GitHub #44。2026-11-24 使用者決定 OpenCode 升到 V2，Wukong 只專注在 harness 做不到的部分，其餘交給 OpenCode V2，並拆成兩個 issue：本 issue 只做 V2 adapter、打包與 installer 的換版，**行為與 V1 時相同**；把 planner、技能、固定提示與壓縮交給 V2 的瘦身在 #45，等本 issue 合併後才開始。

範圍以 #30 [inventory.md 步驟 6](../issue-0030/inventory.md#步驟-6根因排序與升級-gono-go) 的升級前提為起點，介面差異見同檔步驟 4。換版後只支援 V2（不保留 V1 adapter）；release 映像固定 @opencode/cli 2.3.2；Docker installer 升級時自動備份 opencode-state，`--rollback` 連同 opencode.db 一起還原。正式部署（flashaim 的 RunWuKong）的實際換版不在本 issue，合併即結案。

## 快速導覽

### 當前流程
三個入口與 CLI 經 `wukong-gateway` 呼叫 OpenCode 1.x：預設 `opencode run` 走本機按需模式（每次操作起一個 `opencode serve`，用 V1 HTTP API），設 `WUKONG_AGENT_SERVER_URL` 走常駐 server，其他命令走 CLI backend。Installer 升級只備份 release 檔案與 `.env`。現況細節見 [requirement-analysis.md](./requirement-analysis.md)。

### 換版後流程
三條路徑都改用 V2 介面（`/api/...`、新事件、basic auth、`interrupt`、form 回覆）。PATH 上是 V1 時明確報錯。Installer 從 V1 版本升級時先停服務、備份 opencode-state，再啟用新版；啟用失敗或 `--rollback` 時連 opencode.db 一起還原。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| OpenCode 套件 | opencode-ai 1.x，CI 解析 latest | @opencode/cli 2.3.2，固定釘選 |
| 本機按需模式認證 | 移除密碼，不帶認證 | 每次啟動自設密碼，帶 basic auth |
| 健康檢查 | `GET /global/health` | 帶認證的 `GET /api/info` |
| 提問回覆 | question 端點 | form 端點 |
| Docker 升級備份 | release 檔案與 `.env` | 另加 opencode-state（升級前停服務） |
| `--rollback` | 還原 release 檔案與映像 | 另還原 opencode.db |

## 涉及檔案

```text
crates/
├── wukong-gateway/      # 可改：V2 adapter（opencode_server、event_map、local_process、backend、upstream_error、stream）
├── wukong-telegram/     # 可改：提問與權限回覆改走 form／新權限介面時的呼叫端
├── wukong-web/          # 可改：同上
├── wukong-runtime/      # 不可觸及：回合流程、planner、prompt 組裝屬 #45
├── wukong-orchestrator/ # 不可觸及：屬 #45
└── wukong-skills/       # 不可觸及：屬 #45
scripts/
├── docker-entrypoint.sh             # 可改：V2 格式的 opencode.json baseline、server 密碼
├── install.sh                       # 可改：opencode-state 備份、失敗還原、rollback
├── resolve-opencode-version.sh      # 可改：@opencode/cli 與固定釘選
├── test-release-image.sh            # 可改：V2 套件的釘選檢查
└── test-installer-upgrade.sh 等      # 可改：對應回歸測試
release/                 # 可改：package.json、package-lock.json、runtime-inputs.env、data-compatibility.json
docker-compose*.yml、.env.example、Dockerfile*  # 可改：healthcheck、密碼接線、套件路徑
docs/                    # 可改：docker.md、installation.md、根目錄 AGENTS.md、CHANGELOG.md
```

## Gherkin 驗收劇本

```gherkin
Feature: Wukong 在 OpenCode V2 上維持 V1 時的行為

  Background:
    Given 映像內含 @opencode/cli 2.3.2
    And 驗證用的資料都是 #30 一致性副本的再複製，在隔離容器執行

  @SCN-001
  Scenario: V1 時期的對話在換版後接續
    Given 一份 V1 opencode.db 副本，某個 scope 記錄的 session 有先前的對話
    When 以 V2 版 Wukong 在該 scope 執行下一個回合
    Then 回合沿用同一個 session，不另建新的
    And 該 session 的 context 含有換版前的對話

  @SCN-002
  Scenario: 本機按需模式的一般回合
    Given WUKONG_AGENT_CMD 為 opencode run，未設定 WUKONG_AGENT_SERVER_URL
    When 使用者送出單棒回合、多棒回合、帶附件的回合，以及指定模型的回合
    Then 每個回合都得到文字回覆，session 被記錄供下一回合接續
    And 入口照常收到串流的思考與工具進度
    And 回合結束後容器內沒有殘留的 opencode 程序

  @SCN-003
  Scenario: 常駐 server 模式
    Given opencode-server 容器以密碼啟動，入口設定 WUKONG_AGENT_SERVER_URL 與同一組密碼
    When 入口啟動並執行回合
    Then 入口的就緒檢查通過，回合成功
    And opencode-server 的 compose healthcheck 帶認證查詢 /api/info，server 實際不可用時判為 unhealthy

  @SCN-004
  Scenario: server 密碼錯誤
    Given 入口設定的密碼與 opencode-server 不同
    When 入口做就緒檢查
    Then 就緒檢查失敗，錯誤指出認證被拒
    And 不因 V2 的網頁回 200 就判為健康

  @SCN-005
  Scenario: 取消執行中的回合
    Given 一個正在串流回答的回合
    When 使用者從入口取消
    Then 回合中止並回報已取消，不等到 agent timeout

  @SCN-006
  Scenario: 提問與權限回覆
    Given agent 在回合中提出問題或權限詢問
    When 使用者在 Telegram 或 Web 回答、自訂回答或取消
    Then 回答送達 V2，回合繼續或結束，與 V1 時相同
    And 無人值守的排程回合不會停在權限詢問直到逾時

  @SCN-007
  Scenario Outline: 上游錯誤照原本分類
    Given 上游回傳 <錯誤>
    When 回合執行
    Then Wukong 回報 <分類>，不變成逾時，也不把 exit 0 或降級文字當成成功

    Examples:
      | 錯誤               | 分類                   |
      | 額度用完或限流     | 上游模型錯誤（上游限流） |
      | 模型已下架         | 模型下架               |

  @SCN-008
  Scenario: session 維護
    Given V2 中有超過 100 個 session，其中最舊的已過保留期
    When schedulerd 執行保留期清理，以及回合到達壓縮或 7 天輪替時點
    Then 最舊的過期 session 被列出並刪除，仍被 scope 指向的保留
    And 壓縮照常執行，輪替後新 session 接手、舊 session 被刪除

  @SCN-009
  Scenario: opencode 設定 baseline
    Given entrypoint 寫入的 baseline 與使用者的 user.json
    When V2 啟動並執行工具
    Then V2 正常啟動，沒有設定驗證錯誤
    And 對絕對路徑的遞迴刪除被拒絕、/workspace 內的刪除允許、存取 /tmp 不需詢問
    And user.json 的設定仍覆蓋 baseline

  @SCN-010
  Scenario: CLI backend 與假 agent
    Given WUKONG_AGENT_CMD 為帶額外旗標的 opencode run，或 --agent-cmd 指定 printf fixer 這類假 agent
    When 執行回合
    Then opencode 路徑取得回覆與 session，結束後沒有殘留的背景 service
    And 假 agent 照常完成整個流程

  @SCN-011
  Scenario: PATH 上的 opencode 是 V1
    Given PATH 上的 opencode 版本是 1.x
    When 入口或 CLI 執行回合
    Then 立即失敗並指出需要 OpenCode V2 與偵測到的版本，不等到 agent timeout

  @SCN-012
  Scenario: release 映像的 OpenCode 版本
    Given npm 上有比 2.3.2 新的 @opencode/cli
    When release CI 建置映像並產生 manifest
    Then 映像內的 opencode 是 2.3.2，釘選檔、manifest 與映像實際版本一致

  @SCN-013
  Scenario: 從 V1 版本升級時備份 opencode-state
    Given 一個以 V1 版本 Wukong 運作的 Docker 部署
    When 執行 installer 升級到本版
    Then 啟用新版前先停止服務並備份 opencode-state，備份位置記在 release metadata
    And 新版啟用後服務正常

  @SCN-014
  Scenario: 新版啟用失敗時還原
    Given 升級過程中新版啟用或健康檢查失敗
    When installer 處理失敗
    Then release 檔案與 opencode.db 都還原成升級前的備份，服務回到前一版

  @SCN-015
  Scenario: rollback 回前一版
    Given 已升級到本版並在 V2 上產生新對話
    When 執行 installer --rollback
    Then 映像與 opencode.db 回到升級前的備份，V1 時期的對話可以接續
    And 備份之後的對話不保留，installer 與 docs 都說明這一點
```

## Gherkin 核准紀錄
- **核准 commit**: `待提交`
- **核准來源**: 使用者 2026-11-24 對話決定「如果 #30 重新拿出來改成 v2 版本，然後 Wukong 指專注在 harness 做不到的就好，其他回規給 opencode v2 處理」，接受拆成兩個 issue 的建議後要求「先在 github 把這兩個 issue 開起來」，再要求「那是否可以開始 #44」→ 範圍與 SCN-002～SCN-010（行為與 V1 相同）。同日逐題確認：Q1「只支援 V2 (Recommended)」→ 概述、關鍵差異；Q2「固定 2.3.2 (Recommended)」→ SCN-012；Q3「自動備份並支援 rollback (Recommended)」→ SCN-013～SCN-015；Q4「不算，合併即結案」→ 概述（正式換版不在範圍）。同日總結確認推導行為 (a)～(f) 選「照建議全部接受」：(a) 回合結束不留 V2 背景 service → SCN-002、SCN-010；(b) PATH 上是 V1 時明確報錯 → SCN-011；(c) 升級前停服務備份、啟用失敗與 rollback 連 opencode.db 還原、接受備份後對話遺失 → SCN-013～SCN-015；(d) binary 模式 installer 不管 opencode、docs 說明自行升級與備份 → 實作計畫 Phase 5；(e) 首要驗證為 V1 session 能否接續，不能就停下來改規格 → SCN-001 與風險；(f) compose healthcheck 帶認證打 /api/info → SCN-003

全部 Scenario 於 2026-11-24 核准。

## 重構步驟概要

任務權威來源為 [implementation-plan.md](./implementation-plan.md)；Phase／Task 由 decompose 細化。

1. 📝 **Phase 1** - 首要驗證：V1 session 在 V2 遷移後能否接續，以及 V2 CLI 路徑的契約
2. 📝 **Phase 2** - gateway adapter 改寫與回歸測試
3. 📝 **Phase 3** - 容器設定：baseline、server 密碼、healthcheck
4. 📝 **Phase 4** - 打包與 release 釘選
5. 📝 **Phase 5** - installer 備份、失敗還原與 rollback
6. 📝 **Phase 6** - 隔離環境端到端演練、版本對照與常青文件

## 風險與首要驗證

- **最大風險**：V2 就地、單向遷移共用的 opencode.db 之後，Wukong 記錄的 scope 對 session 的對應失效（V1 的 session ID 在 V2 讀不到或不能接續），換版當下所有 scope 失去對話接續；此外遷移會刪掉 V1 的三張表，回復只能靠備份。
- **風險等級與理由**：High。涉及不可逆的資料遷移與三個入口的核心外部契約，V1 session 在 V2 的可接續性尚未驗證（#30 只確認 V2 列得到原本的 6 個 session）。
- **首要驗證**：在 #30 副本的再複製上讓 V2 完成遷移，對 Wukong 記錄的 session ID 查 `GET /api/session/{id}` 與 `GET /api/session/{id}/context`，並送一則 prompt 確認接續到同一個 session；全程不碰正式資料。
- **選擇理由**：這是契約與資料樣態的問題，最直接的證據是對真實副本做最小請求；adapter 寫完才發現不能接續，整個換版的做法要改。
- **完成證據**：副本中每個被 scope 指向的 session 在 V2 都查得到，context 含換版前的訊息，對其中一個送出 prompt 後 session ID 不變、訊息數增加；另以一個不存在的 session ID 做負向對照，確認探測不會假綠燈。不成立時停在 Phase 1 修訂規格。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|------|------|------|------|
| TBD-1 | 使用者自訂的 WUKONG_AGENT_CMD 若仍帶 V1 才有的旗標（例如 --dangerously-skip-permissions），V2 的 opencode run 會拒絕還是忽略；installer 現有的 .env 遷移只改寫原封未動的舊範本那一行 | 待確認 | 未確認時，自訂命令的使用者升級後回合可能一律失敗；關聯 SCN-010、Phase 1 |
| TBD-2 | 常駐 server 模式的密碼怎麼提供：V2 沒設密碼時會自己產生隨機密碼，入口無從得知；要沿用 WUKONG_AGENT_SERVER_PASSWORD 由使用者在 .env 設定，還是由 entrypoint 產生並存在共用 volume | 待確認 | 決定前不實作 Phase 3 的 server 密碼接線；影響 server 模式使用者升級時要不要改 .env；關聯 SCN-003、SCN-004 |

## Timeline

| 日期 | 異動 |
|------|------|
| 2026-11-24 | 建立；同日逐題確認與總結確認，全部 Scenario 核准 |

---
**建立日期**: 2026-11-24  
**分級**: Large（gateway、三個入口、打包、release CI、compose 與 installer）  
**風險**: High\
**狀態**: 待實作
