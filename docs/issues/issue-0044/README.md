# Issue 44 - OpenCode V2 換版：adapter 改寫與部署換版，行為不變

## 文件清單
- [requirement-analysis.md](./requirement-analysis.md)
- [technical-analysis.md](./technical-analysis.md)
- [implementation-plan.md](./implementation-plan.md)

## 概述

GitHub #44。2026-11-24 使用者決定 OpenCode 升到 V2，Wukong 只專注在 harness 做不到的部分，其餘交給 OpenCode V2，並拆成兩個 issue：本 issue 只做 V2 adapter、打包與 installer 的換版，**行為與 V1 時相同**；把 planner、技能、固定提示與壓縮交給 V2 的瘦身在 #45，等本 issue 合併後才開始。

範圍以 #30 [inventory.md 步驟 6](../issue-0030/inventory.md#步驟-6根因排序與升級-gono-go) 的升級前提為起點，介面差異見同檔步驟 4。換版後只支援 V2（不保留 V1 adapter）；release 映像固定 @opencode/cli 2.3.2；Docker installer 升級時自動備份 opencode-state，`--rollback` 連同 opencode.db 一起還原。正式部署（flashaim 的 RunWuKong）的實際換版不在本 issue，合併即結案。

## 現況

2026-11-25 第一次完整審查（review-0405603）判定 RETURN TO execute-task；同日修正後增量覆審（0405603..c4d47d8）判定 PASS；三項建議依使用者決定，N-A 記入 TBD-6，N-B、N-C 修正。2026-11-26 處理建議後的增量覆審（c4d47d8..7df8450）判定 PASS，兩項新建議依使用者決定：N-D 修正，N-E 等發版時改（TBD-8）。同日 PR #46 第一輪審查提出兩項 P2、一項 P3：P2 修正（TBD-9、TBD-10），P3 記入 TBD-11，修正後待增量覆審。

**現行 review**: [review-7df8450.md](review-7df8450.md)

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
├── wukong-web/          # 可改：同上；opencode 工具命令（模型清單、系統診斷）
├── wukong-cli/          # 可改：/providers、/models 的 opencode 工具命令
├── wukong-runtime/      # 不可觸及：回合流程、planner、prompt 組裝屬 #45
├── wukong-orchestrator/ # 不可觸及：屬 #45
└── wukong-skills/       # 不可觸及：屬 #45
scripts/
├── docker-entrypoint.sh             # 可改：V2 格式的 opencode.json baseline、server 密碼
├── install.sh                       # 可改：opencode-state 備份、失敗還原、rollback
├── opencode-idle-restart.sh         # 可改：server 模式閒置重啟查的 session 列表（V2 的 /api/session 與認證）
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
    Given .env 設定了 WUKONG_AGENT_SERVER_URL 與 WUKONG_AGENT_SERVER_PASSWORD，compose 把同一組密碼交給 opencode-server 與三個入口
    When 入口啟動並執行回合
    Then 入口的就緒檢查通過，回合成功
    And opencode-server 的 compose healthcheck 帶認證查詢 /api/info，server 實際不可用時判為 unhealthy

  @SCN-017
  Scenario: server 模式沒有設定密碼
    Given 啟用 server 模式，但 .env 沒有 WUKONG_AGENT_SERVER_PASSWORD
    When 以 installer 升級
    Then installer 先備份 .env，再產生一組密碼寫入 WUKONG_AGENT_SERVER_PASSWORD，並在輸出說明
    When 不經 installer、直接以沒有密碼的設定啟動
    Then 入口立即報錯並指出要設定 WUKONG_AGENT_SERVER_PASSWORD，不等到 agent timeout

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
      | 錯誤                              | 分類                         |
      | 額度用完或限流                    | 上游模型錯誤（上游限流）       |
      | 模型已下架                        | 上游模型錯誤（模型已下架）     |
      | 連不到上游，V2 持續重試超過 90 秒 | 上游模型錯誤（上游連不到）     |

  @SCN-016
  Scenario: 上游短暫失敗後恢復
    Given V2 在回合中因上游錯誤開始重試
    When 上游在重試開始後 90 秒內恢復
    Then 回合照常完成並回覆，不被中止

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
- **核准 commit**: 94aaa40
- **核准來源**: 使用者 2026-11-24 對話決定「如果 #30 重新拿出來改成 v2 版本，然後 Wukong 指專注在 harness 做不到的就好，其他回規給 opencode v2 處理」，接受拆成兩個 issue 的建議後要求「先在 github 把這兩個 issue 開起來」，再要求「那是否可以開始 #44」→ 範圍與 SCN-002～SCN-010（行為與 V1 相同）。同日逐題確認：Q1「只支援 V2 (Recommended)」→ 概述、關鍵差異；Q2「固定 2.3.2 (Recommended)」→ SCN-012；Q3「自動備份並支援 rollback (Recommended)」→ SCN-013～SCN-015；Q4「不算，合併即結案」→ 概述（正式換版不在範圍）。同日總結確認推導行為 (a)～(f) 選「照建議全部接受」：(a) 回合結束不留 V2 背景 service → SCN-002、SCN-010；(b) PATH 上是 V1 時明確報錯 → SCN-011；(c) 升級前停服務備份、啟用失敗與 rollback 連 opencode.db 還原、接受備份後對話遺失 → SCN-013～SCN-015；(d) binary 模式 installer 不管 opencode、docs 說明自行升級與備份 → 實作計畫 Phase 5；(e) 首要驗證為 V1 session 能否接續，不能就停下來改規格 → SCN-001 與風險；(f) compose healthcheck 帶認證打 /api/info → SCN-003。同日 Task 1.2 後逐題確認：TBD-3「重試超過 90 秒就中止 (Recommended)」（任何 retry 持續超過 90 秒就中止回合，依 retry 的錯誤類型回報；容得下短暫的網路抖動）→ SCN-007 第三列、SCN-016；TBD-2「在 .env 設定 (Recommended)」（沿用 WUKONG_AGENT_SERVER_PASSWORD，compose 同時傳給 opencode-server 與三個入口，沒設時 server 模式啟動就明確報錯，installer 升級時偵測到 server 模式未設密碼就先備份 .env 再自動產生）→ SCN-003、SCN-017

| Scenario | 核准日期 | 狀態 |
|----------|---------|------|
| SCN-001 | 2026-11-24 | 已核准 |
| SCN-002 | 2026-11-24 | 已核准 |
| SCN-003 | 2026-11-24 | 已核准 |
| SCN-004 | 2026-11-24 | 已核准 |
| SCN-005 | 2026-11-24 | 已核准 |
| SCN-006 | 2026-11-24 | 已核准 |
| SCN-007 | 2026-11-24 | 已核准 |
| SCN-008 | 2026-11-24 | 已核准 |
| SCN-009 | 2026-11-24 | 已核准 |
| SCN-010 | 2026-11-24 | 已核准 |
| SCN-011 | 2026-11-24 | 已核准 |
| SCN-012 | 2026-11-24 | 已核准 |
| SCN-013 | 2026-11-24 | 已核准 |
| SCN-014 | 2026-11-24 | 已核准 |
| SCN-015 | 2026-11-24 | 已核准 |
| SCN-016 | 2026-11-24 | 已核准 |
| SCN-017 | 2026-11-24 | 已核准 |

## 重構步驟概要

任務權威來源為 [implementation-plan.md](./implementation-plan.md)；Phase／Task 由 decompose 細化。

1. ✅ **Phase 1** - 首要驗證：V1 session 在 V2 遷移後能否接續，以及 V2 CLI 路徑的契約
2. ✅ **Phase 2** - gateway adapter 改寫與回歸測試
3. ✅ **Phase 3** - 容器設定：baseline、server 密碼、healthcheck
4. ✅ **Phase 4** - 打包與 release 釘選
5. ✅ **Phase 5** - installer 備份、失敗還原與 rollback
6. ✅ **Phase 6** - 隔離環境端到端演練、版本對照與常青文件

## 風險與首要驗證

- **最大風險**：V2 就地、單向遷移共用的 opencode.db 之後，Wukong 記錄的 scope 對 session 的對應失效（V1 的 session ID 在 V2 讀不到或不能接續），換版當下所有 scope 失去對話接續；此外遷移會刪掉 V1 的三張表，回復只能靠備份。
- **風險等級與理由**：High。涉及不可逆的資料遷移與三個入口的核心外部契約，V1 session 在 V2 的可接續性尚未驗證（#30 只確認 V2 列得到原本的 6 個 session）。
- **首要驗證**：在 #30 副本的再複製上讓 V2 完成遷移，對 Wukong 記錄的 session ID 查 `GET /api/session/{id}` 與 `GET /api/session/{id}/context`，並送一則 prompt 確認接續到同一個 session；全程不碰正式資料。
- **選擇理由**：這是契約與資料樣態的問題，最直接的證據是對真實副本做最小請求；adapter 寫完才發現不能接續，整個換版的做法要改。
- **完成證據**：副本中每個被 scope 指向的 session 在 V2 都查得到，context 含換版前的訊息，對其中一個送出 prompt 後 session ID 不變、訊息數增加；另以一個不存在的 session ID 做負向對照，確認探測不會假綠燈。不成立時停在 Phase 1 修訂規格。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|------|------|------|------|
| TBD-1 | 使用者自訂的 WUKONG_AGENT_CMD 若仍帶 V1 才有的旗標（例如 --dangerously-skip-permissions），V2 的 opencode run 會拒絕還是忽略；installer 現有的 .env 遷移只改寫原封未動的舊範本那一行 | 已解決（2026-11-24 Task 1.2，probe-cli.md）：V2 2.3.2 接受該旗標，效果與 --auto 相同，自訂命令不需改寫；另發現 V2 拒絕大寫的 --log-level ERROR，由 Task 2.6 改成小寫 | 未確認時，自訂命令的使用者升級後回合可能一律失敗；關聯 SCN-010、Phase 1 |
| TBD-2 | 常駐 server 模式的密碼怎麼提供：V2 沒設密碼時會自己產生隨機密碼，入口無從得知；要沿用 WUKONG_AGENT_SERVER_PASSWORD 由使用者在 .env 設定，還是由 entrypoint 產生並存在共用 volume | 已解決（2026-11-24 使用者選「在 .env 設定」）：沿用 WUKONG_AGENT_SERVER_PASSWORD，compose 傳給 opencode-server 與三個入口，沒設時明確報錯，installer 升級時自動產生；見 SCN-003、SCN-017 | 決定前不實作 Phase 3 的 server 密碼接線；影響 server 模式使用者升級時要不要改 .env；關聯 SCN-003、SCN-004 |
| TBD-3 | 上游連不到時 V2 不回錯誤：2026-11-24 Task 1.1（server 路徑）60 秒後 session 仍在執行；Task 1.2（CLI 路徑）一直送 retry 事件（error.type 為 provider.transport），120 秒內 7 次仍未結束。同條件下 V1 1.19.6 約 65 秒以 UnknownError 結束。Wukong 要自己收斂，收斂條件與回報方式待使用者決定 | 已解決（2026-11-24 使用者選「重試超過 90 秒就中止」）：任何 retry 持續超過 90 秒就中止回合，依錯誤類型回報；見 SCN-007、SCN-016 | 不處理時，上游連不到的回合要等到 agent timeout（預設 20 分鐘）才失敗；關聯 SCN-007、Task 1.3、Task 2.3 |
| TBD-4 | SCN-005 寫的是「使用者從入口取消執行中的回合」，但 Web、Telegram、CLI 都沒有取消執行中回合的操作（V1 時也沒有）；review-0405603 的 `需決策` | 已解決（2026-11-25 使用者選「記入待確認」）：規格不動，以 gateway 層證據驗收：回合的 future 被丟掉時送 interrupt（`dropping_the_turn_interrupts_the_session`），V2 回報中止時分類為「回合被中止」；入口層的取消操作不在本 issue | 入口日後新增取消操作時，要另外驗證端到端；關聯 SCN-005 |
| TBD-5 | 演練中 free-limit 429 約 161 秒才回報，與「重試超過 90 秒就中止」沒有對照清楚（review-0405603 S3） | 已解決（2026-11-25 實測，`docs/issues/issue-0044/scripts/capture-free-limit.py`）：V2 對 free-limit 不重試，第一步就 `step.failed`、`execution.failed`，1.1～1.8 秒結束，沒有 retry 事件；樣本存為 fixture `server-free-limit.ndjson`，以 `a_real_free_limit_fails_at_once_as_rate_limited` 釘住。三次擷取依序為 429、成功、429，間隔幾分鐘，額度的限流是斷續的；161 秒最可能是那一棒前幾步成功、之後某一步才碰到 429，但演練資料已清除，無法再對證。重試計時只在 `step.ended` 等有輸出的事件時重新起算，真實的 transport 重試樣本在兩次 retry 之間沒有這類事件 | 無：90 秒中止只針對 V2 實際會重試的錯誤；關聯 SCN-007 |
| TBD-6 | 從 V1 升級後第一次啟動時，兩個 `--standalone` 命令同時執行，其中一個會失敗一次（`Standalone server exited before reporting readiness`）；Web 系統診斷以 `tokio::join!` 同時跑 `auth list` 與 `models`（review-c4d47d8 N-A） | 不影響本次交付（2026-11-25 使用者選「記入待確認」，2026-11-26 依 review-7df8450 N-D 改寫狀態）：追蹤：無，只在尚未遷移的 V1 DB 上發生；重新整理就正常，遷移照常完成，session 都在，`integrity_check` ok。已遷移、全新安裝、或與 serve 同時啟動時都正常 | 升級後第一個動作若是打開 Web 系統診斷，Providers 或 Models 其中一項會顯示一次錯誤；關聯 SCN-001 |
| TBD-7 | review-c4d47d8 的其他建議：N-B CHANGELOG 兩條寫成分支上的中間狀態；N-C 新建 `.env` 的 0600 沒有測試斷言，另有實作計畫一處紅燈案例編號寫錯 | 已解決（2026-11-25 使用者選兩項都修正）：CHANGELOG 改寫成與 v0.25.1 比較的差異；`test_docker` 補上全新部署的 `.env` 為 0600 的斷言，並更正計畫裡的案例編號（見 implementation-plan.md R7） | 無 |
| TBD-8 | CHANGELOG 閒置重啟那條舉的例子（沒有密碼、密碼被拒）在 v0.25.1 不會發生，v0.25.1 以前就會印成同一句的只有回應看不懂與請求失敗（review-7df8450 N-E） | 不影響本次交付（2026-11-26 使用者選「發版時一起改」）：追蹤：無，只是發版說明的措辭，不影響行為與驗收；發版前把 Unreleased 整理成版本區塊是 CHANGELOG.md 開頭寫明的固定步驟，屆時依此改寫 | 無 |
| TBD-9 | PR #46 第一輪審查（2026-11-26）的 P2：P2-1 升級後只以 `docker compose ps` 的退出狀態判定啟用成功，看不出服務是否健康；P2-2 備份清單以 process substitution 讀入，解析失敗被吞掉，可能在沒有備份的情況下升級 | 已解決（2026-11-26 使用者選兩項都修正）：啟用改用 `up --wait --wait-timeout 180`，Compose 不支援時在改動前中止；清單先存進變數、成功才讀入；見 implementation-plan.md R8、R9。同日使用者另決定：刻意回報 unhealthy 的 Web 降級模式也算失敗，失敗訊息提示用 `docker compose ps` 查是哪個服務 | 無 |
| TBD-10 | `up --wait` 涵蓋不到的兩種情況：按需模式下 V2 要到第一個回合才遷移 `opencode.db`，啟用時的健康等待驗不到遷移；沒有 healthcheck 的 wukong-telegram、wukong-schedulerd 啟動就退出、由重啟政策拉起時，實測 7 次有 2 次被判成功（R8） | 不影響本次交付（2026-11-26 使用者選 P2-1「修正」時一併決定記入）：追蹤：無，按需模式的遷移失敗會在第一個回合報錯，升級前的備份仍在，`--rollback` 會連同 `opencode.db` 還原（SCN-015）；那兩個服務不遷移 `opencode.db`，以前的 `ps` 檢查同樣看不出它們退出 | 按需模式升級後，遷移問題要到第一個回合才看得到；關聯 SCN-014 |
| TBD-11 | PR #46 第一輪審查 P3：`opencode_version.rs` 以執行檔路徑字串快取「已確認是 V2」，長期執行中把同一路徑換回 V1 時會略過版本檢查 | 不影響本次交付（2026-11-26 使用者選「記入待確認」）：追蹤：無，只快取成功結果是刻意的設計（換版後不必重啟）；換回 V1 時第一個回合仍會失敗，只是錯誤訊息不會指出版本，不影響資料；Docker 映像內的執行檔不會在執行中被換掉 | binary 模式在服務執行中換回 V1 時，錯誤訊息看不出原因；關聯 SCN-011 |

## Timeline

| 日期 | 異動 |
|------|------|
| 2026-11-24 | 建立；同日逐題確認與總結確認，全部 Scenario 核准 |
| 2026-11-24 | 規格修訂：Task 1.2 發現 V2 上游連不到時不結束，依 TBD-3 決定修訂 SCN-007、新增 SCN-016；依 TBD-2 決定修訂 SCN-003、新增 SCN-017；同日核准，原基線 abd4b89 |
| 2026-11-26 | PR #46 第一輪審查：P2-1、P2-2 修正（TBD-9），P3 記入 TBD-11 |

---
**建立日期**: 2026-11-24  
**分級**: Large（gateway、三個入口、打包、release CI、compose 與 installer）  
**風險**: High\
**狀態**: 實作完成，待審查
