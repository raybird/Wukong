# Issue 45 - Wukong 瘦身：planner、技能、固定提示與壓縮交給 OpenCode V2

## 文件清單
- [requirement-analysis.md](./requirement-analysis.md)
- [technical-analysis.md](./technical-analysis.md)
- [implementation-plan.md](./implementation-plan.md)

## 概述

GitHub #45。2026-11-24 使用者決定：OpenCode 升到 V2 之後，Wukong 只做 harness 做不到的部分，其餘交給 OpenCode V2。工作拆成兩個 issue，本 issue 是第二個。前置的 #44 已在 2026-11-26 合併（b65b487）。

本 issue 交給 V2 的部分：

- planner 與角色接力鏈，改成一個 primary agent（悟空）加五個 subagent；
- 技能選擇與注入；
- 每回合附加的固定提示；
- 依回合數的壓縮。

同時修正人格的載入，入口的角色顯示改從 V2 的 subagent 資訊取得，並評估能不能用 inbox 簡化並行回合。

GitHub issue 內文「留在 Wukong」清單裡的行為維持不變。正式部署 RunWuKong 的實際換版不在本 issue。

## 現況

- **現行 review**: review-4ccfcec.md

## 快速導覽

### 當前流程
每回合先以 OpenCode 的 plan agent 另跑一次 LLM，規劃出最多 3 棒「角色|技能」，再逐棒執行。

- 輔助棒沒有 session，要另外補上一回合。
- 角色卡、排程能力、Memoria 能力、輸出要求、檔案互動規則與技能路徑，每回合都附在 user message 裡。
- 每 20 回合由 Wukong 主動壓縮。

細節見 [requirement-analysis.md](./requirement-analysis.md)。

### 瘦身後流程
每回合以 primary agent「wukong」在 scope 的 session 送出一次 prompt。

- 模型自己決定直接回答，或派工給 explorer、oracle、librarian、fixer、designer。
- 技能走 V2 的 skill。
- 人格與固定說明放在 agent 定義與 session 指令層。
- 壓縮交給 OpenCode 自動處理，7 天輪替與保留期仍由 Wukong 負責。

## 關鍵差異
| 項目 | 變更前 | 變更後 |
|------|--------|--------|
| 每回合的 LLM 呼叫 | planner 一次，加上 1～3 棒 | primary agent 一次，視需要派 subagent |
| 本機模式啟動 opencode | 每回合 2 次 | 每回合 1 次 |
| 技能 | planner 選，prompt 附 SKILL.md 路徑 | V2 skill |
| 人格 | workspace/AGENTS.md 第 3 行（SOUL.md 沒有被讀） | Wukong 管理的 agent 定義，含 SOUL.md 內容 |
| 排程能力的 scope | 每回合附在 user message | session 指令層 |
| 壓縮 | Wukong 每 20 回合，加上 OpenCode 自動 | 只有 OpenCode 自動 |
| 規劃偏好設定 | settings.json 與 Web 介面 | 移除，舊欄位讀到時忽略 |
| 入口顯示的角色 | 每一棒的 Role | 悟空，或正在處理的 subagent |

## 涉及檔案

```text
crates/
├── wukong-runtime/      # 可改：run_turn 改為單一 primary agent、拿掉上一回合補償與依回合數壓縮、persona 與 skill_assets 改寫
├── wukong-orchestrator/ # 可改：planner 與接力鏈移除；Role 保留給顯示用或改由 subagent 名稱取代
├── wukong-skills/       # 可改：技能改寫到 V2 的探索路徑
├── wukong-gateway/      # 可改：prompt 指定 agent、session 指令層、事件裡的 subagent 資訊
├── wukong-settings/     # 可改：移除 planner_preferences，舊欄位忽略
├── wukong-web/          # 可改：角色與步驟顯示、移除偏好介面與 API
├── wukong-telegram/     # 可改：狀態泡泡的角色、檔案互動規則改走 session 指令層
├── wukong-scheduler/    # 可改：執行訊息的階段文字
├── wukong-cli/          # 可改：角色顯示、啟動時寫入 agent 定義與技能
├── wukong-schedulerd/   # 可改：啟動時寫入 agent 定義與技能
├── wukong-memory/       # 不可觸及：記憶與 lease 的 schema 不變
└── wukong-memoryd/      # 不可觸及
scripts/
├── docker-entrypoint.sh  # 可改：agent 定義、技能路徑、AGENTS.md 範本遷移
├── install.sh            # 可改：binary 模式的範本遷移（實作在 wukong-runtime，入口啟動時執行，見 Task 2.3）
├── test-docker-runtime.sh # 可改：映像不再帶技能的檢查
└── test-support/         # 可改：假 agent 不再輸出 planner 的格式
workspace/AGENTS.md、workspace/SOUL.md  # 可改：範本內容
Dockerfile、Dockerfile.release、.github/workflows/release.yml  # 可改：映像不再複製技能
docs/                     # 可改：根目錄 AGENTS.md、docker.md、entrypoints.md、CHANGELOG.md
```

## Gherkin 驗收劇本

```gherkin
Feature: Wukong 只做 harness 做不到的部分，其餘交給 OpenCode V2

  Background:
    Given 映像內含 @opencode/cli 2.3.2
    And 真實模型的驗證以 NVIDIA nemotron 或預設的免費模型判定，紀錄寫明實際用的模型，在隔離容器以合成提示執行，不用正式對話

  @SCN-001
  Scenario: 一般回合只呼叫一次 primary agent
    Given 本機按需模式與常駐 server 模式各一組
    When 使用者送出不需要工具的問題
    Then 回合不另跑規劃，以 primary agent「wukong」回答
    And 本機按需模式每回合只啟動一次 opencode
    And 以 bench-turn.sh 的 short 與 tool 任務，在相同條件下對照 #44 合併版本（b65b487）：兩種任務的成功率都不低於它，short 任務每回合的 LLM step 數少於它
    And tool 任務每回合的 LLM step 數照實記錄：primary agent 不能改檔，建檔要派給 fixer，步數可以高於 b65b487

  @SCN-002
  Scenario: 需要實作的任務派給 fixer，入口看得到是誰在處理
    When 使用者要求建立或修改工作區裡的檔案
    Then primary agent 派工給 fixer subagent，檔案內容符合要求
    And Telegram 的狀態泡泡與 Web 的進度顯示正在處理的角色（悟空，或悟空·fixer）
    And Web 顯示 subagent 的產出卡片，排程的執行訊息顯示目前的角色

  @SCN-003
  Scenario: 五個 subagent 都可以被派工
    Given 一組固定任務：實作、程式碼探索、資料查找、架構建議、介面設計各一題，另有一題直接回答的問題
    When 每題各執行 3 次
    Then 實作題 3 次都派給 fixer，直接回答題 3 次都不派工
    And 其他各題至少 2 次派給預期的 subagent，每次的實際派工都記錄下來

  @SCN-004
  Scenario: 追問在拿掉上一回合補償後仍然答對
    Given 上一回合在同一個 scope 討論過某個具體內容
    When 使用者追問「剛剛那個」並要求延伸
    Then 回答延續上一回合的內容
    And 送出的 prompt 不含 Wukong 補上的上一回合
    When session 因 7 天輪替換成新的
    Then 新 session 的第一個回合仍補上上一回合，追問照樣答對

  @SCN-005
  Scenario: 技能由 V2 的 skill 系統提供
    When 使用者的任務符合某個 Superpowers 技能（例如先寫測試再實作）
    Then 模型透過 V2 的 skill 機制載入該技能，事件中看得到技能被使用
    And prompt 不再出現技能規範指引與 SKILL.md 路徑，工作區的 AGENTS.md 也沒有對應段落

  @SCN-006
  Scenario: 固定提示不再寫進每回合的 user message
    When 在 Telegram 與 Web 各執行數個回合
    Then opencode.db 裡這些回合的 user message 不含角色卡、排程能力、長期記憶能力、輸出要求、檔案互動規則
    And 召回的相關記憶、其他回合正在執行的提示、排程的無人值守提示照舊附加

  @SCN-007
  Scenario: 排程能力以目前的 scope 建立排程
    When 使用者在某個 scope 要求「每天早上提醒我」
    Then 模型以 wukong schedule add-turn 建立排程，擁有者是這個 scope
    And scope 由 session 指令層提供，不出現在 user message
    When session 輪替成新的
    Then 新 session 同樣拿得到 scope

  @SCN-008
  Scenario: 長期記憶能力只在開啟時提供
    Given WUKONG_MEMORIA_HINT 開啟與關閉各一組
    When 執行一個回合
    Then 開啟時模型拿得到長期記憶能力的說明，關閉時拿不到

  @SCN-009
  Scenario: 人格明確載入，既有部署收得到新定義
    Given 一個全新部署，一個工作區 AGENTS.md 與歷代範本完全相同的既有部署，一個改過 AGENTS.md 的既有部署
    When 以新版啟動並問「你是誰」
    Then 三者都以孫悟空的人格回答，人格來自 Wukong 管理的 agent 定義，含 SOUL.md 的內容
    And 與範本相同的 AGENTS.md 先備份再換成新版，改過的不動，log 提醒其中過時的段落
    And 升級後 agent 定義跟著新版更新，不必手動處理

  @SCN-010
  Scenario: Wukong 不再依回合數壓縮
    Given 一個 scope 已執行超過 20 回合
    When 執行下一個回合
    Then Wukong 不主動壓縮 session，OpenCode 的自動壓縮設定照舊
    And 設了 WUKONG_SESSION_COMPACT_EVERY_TURNS 也不報錯，只是沒有作用
    And session 超過 7 天時，在回合開始時輪替，舊 session 照舊刪除；保留期清理不變

  @SCN-011
  Scenario: 規劃偏好設定移除
    Given 設定檔仍有 planner_preferences 欄位
    When Web、Telegram、CLI 與 schedulerd 啟動並執行回合
    Then 都正常運作，不報錯，其他設定照舊生效
    And Web 不再有技能偏好的介面與 API

  @SCN-012
  Scenario: harness 的既有保障不變
    When 回合的最終輸出是空的
    Then Wukong 在同一個 session 補送一次修復提示，仍為空時回「(本回合未產生文字輸出)」
    When 上游限流、模型下架或連不到
    Then 回合失敗，錯誤分類與 #44 時相同，排程的執行訊息照舊顯示分類
    When 同一個 scope 已有回合在執行
    Then 新的回合照舊被拒絕，而且在被拒絕前不花掉任何 LLM 呼叫

  @SCN-013
  Scenario: 指定模型的回合，subagent 也用同一個模型
    Given 排程或 Web 指定了模型
    When primary agent 派工給 subagent
    Then primary agent 與 subagent 都用指定的模型，不悄悄改用預設模型

  @SCN-014
  Scenario: CLI backend 與自訂命令
    Given WUKONG_AGENT_CMD 是預設的 opencode run，或使用者自訂的命令
    When 執行回合
    Then 預設命令以 primary agent「wukong」執行
    And 使用者自訂的命令維持原本的參數，不被加上會改變行為的旗標
```

## Gherkin 核准紀錄
- **核准 commit**: f0ce34ad4769324e73e406dbc2bcb15703169d34
- **核准來源**: 範圍與「留在 Wukong」來自 GitHub #45 內文與使用者 2026-11-24 的決定（拆成兩個 issue，#44 合併後開始）；2026-11-26 使用者要求「好開始 /dev-cycle #45」。同日逐題確認：Q1「NVIDIA 為準（建議）」→ Background、SCN-003；Q2「移除（建議）」→ SCN-011；Q3「Wukong 管理、自動更新（建議）」→ SCN-009；Q4「session 指令層（建議）」→ SCN-007。同日總結確認推導行為（派工門檻、輪替後仍補上一回合、保留的動態區塊、舊壓縮變數忽略且 7 天輪替改每回合檢查、lease 在任何 LLM 呼叫前取得、subagent 沿用指定模型、自訂命令不加旗標、inbox 只評估）選「全部核准（建議）」→ SCN-001～SCN-014。同日 NVIDIA 憑證被拒（403）後，使用者表示「使用免費模型驗證也可以」→ Background、TBD-6。2026-11-27 修訂：Task 6.2 實測 tool 任務步數 4 → 中位數 8（primary 不能改檔，建檔要派給 fixer），使用者選「消掉輪詢，修訂 tool 的步數條件（建議）」→ SCN-001 的步數條件只適用 short 任務，tool 任務照實記錄；同日使用者指示「nvidia 限流可以換成 opencode 的模型」→ SCN-003 以 opencode/big-pickle 驗收（Task 6.1）。

| Scenario | 核准日期 | 狀態 |
|----------|---------|------|
| SCN-001 | 2026-11-27 | 已核准（修訂） |
| SCN-002 | 2026-11-26 | 已核准 |
| SCN-003 | 2026-11-26 | 已核准 |
| SCN-004 | 2026-11-26 | 已核准 |
| SCN-005 | 2026-11-26 | 已核准 |
| SCN-006 | 2026-11-26 | 已核准 |
| SCN-007 | 2026-11-26 | 已核准 |
| SCN-008 | 2026-11-26 | 已核准 |
| SCN-009 | 2026-11-26 | 已核准 |
| SCN-010 | 2026-11-26 | 已核准 |
| SCN-011 | 2026-11-26 | 已核准 |
| SCN-012 | 2026-11-26 | 已核准 |
| SCN-013 | 2026-11-26 | 已核准 |
| SCN-014 | 2026-11-26 | 已核准 |

## 重構步驟概要
1. ✅ **Phase 1** - V2 能力的契約探測（首要驗證）
2. ✅ **Phase 2** - primary agent、subagent 與技能的定義與部署
3. ✅ **Phase 3** - run_turn 改為單一 primary agent，拿掉 planner、接力鏈與依回合數壓縮
4. ✅ **Phase 4** - 固定提示移到 agent 定義與 session 指令層
5. ✅ **Phase 5** - 入口的角色顯示與規劃偏好移除
6. ✅ **Phase 6** - 真實模型驗收、效能對照與常青文件

## 風險與首要驗證

- **最大風險**：瘦身依賴的 V2 能力有四項尚未確認：
  1. 專案技能會不會出現在 V2 的 skill 清單，模型會不會用到。2026-11-24 探測時，專案目錄的 SKILL.md 沒有出現在 /api/skill。
  2. session instruction entries 能不能持續存在、不進對話紀錄。這個 API 是 experimental。
  3. subagent 會不會沿用指定的模型。
  4. 事件裡有沒有 subagent 的身分資訊，可以拿來顯示角色。

  另外，路由改由模型決定後，模型會不會正確派工，要用真實模型才知道。
- **風險等級與理由**：High。核心外部行為尚未確認，影響三個入口、排程與部署。任一能力不成立時，對應的範圍要改做法：範圍第 2 項整個建立在第一項上。
- **首要驗證**：Phase 1 在隔離環境（獨立 HOME，憑證只放 NVIDIA 一筆或不放，合成提示）對 V2 2.3.2 做契約探測與最小真實請求。逐項確認上面四項能力，以及 agent 定義放在哪個目錄才會被本機按需、常駐 server 與 CLI 三種模式讀到。另抽樣確認 primary agent 會派工給 fixer。
- **選擇理由**：這是外部契約的問題，最直接的證據是對真實 V2 送最小請求。先改 run_turn 才發現 skill 或指令層不能用，Phase 3、4 都要重做。
- **完成證據**：每項能力都有成立或不成立的探測紀錄（命令與輸出）。不成立的項目已回到規格修訂，或依 Q4 的退路處理。之後才開始 Phase 2。

## 待確認事項

| 編號 | 事項 | 狀態 | 影響 |
|------|------|------|------|
| TBD-1 | V2 的 skill 探索路徑與格式：2026-11-24 探測時，專案目錄的 SKILL.md 沒有出現在 /api/skill；prompt 附 skills 時模型會不會用到也未確認 | 已解決（2026-11-26，probe-skills.md）：八個 Superpowers 技能原樣放在工作區 .opencode/skills 就出現在 /api/skill，Wukong 現在的 .wukong/skills/superpowers 讀不到；2026-11-24 沒有出現是 server 剛啟動時的競態。模型會依 description 自己呼叫 skill 工具載入，prompt 附 skills 也有效。範圍第 2 項照原做法進行 | Phase 1 證實不成立時，範圍第 2 項的做法要回到規格修訂再決定；關聯 SCN-005 |
| TBD-2 | session instruction entries（experimental）能不能持續存在、不進對話紀錄、模型讀得到 | 已解決（2026-11-26，probe-instructions.md）：三項都成立，server 重啟後仍在且模型讀得到，每個 session 各自一份。例外是模型看過之後才改值時，V2 會插一則 system 通知到對話紀錄；scope 每個 session 寫入一次不再改，不受影響。採用 Q4 的做法，不需要退路 | 不成立時依 2026-11-26 Q4 的退路，每回合只附一行 scope；關聯 SCN-006、SCN-007、SCN-009 |
| TBD-3 | subagent 會不會沿用 primary agent 或 prompt 指定的模型 | 已解決（2026-11-26，probe-subagent.md）：subagent 沿用 session 選取的模型；Wukong 的 adapter 本來就逐 session 選取，定義裡不寫模型。設定檔指定的模型在 server 上沒有套用，不能依賴 | 不會時，Wukong 要在定義或 prompt 裡為 subagent 指定模型；關聯 SCN-013 |
| TBD-4 | V2 事件裡的 subagent 身分資訊（task 工具的參數、子 session 的事件） | 已解決（2026-11-26，probe-subagent.md）：subagent 工具的輸入帶 agent，子 session 的 session.created 帶 parentID、agent 與 model，產出以 synthetic 項目回到 primary 的 inbox；fixture 已存檔。另發現 subagent 一律在背景執行，回合要等背景工作清空才算結束，已補進 Task 3.1 | 拿不到時，角色顯示要另找來源；關聯 SCN-002 |
| TBD-5 | 範圍第 7 項：用 V2 的 session inbox（delivery 設成 queue）簡化同一 scope 並行回合的 lease | 已解決（2026-11-26，[probe-inbox.md](probe-inbox.md)、計畫 Task 6.4）：不採用，維持 lease 與立即拒絕。queue 的項目等目前那一步寫完，接在同一次執行裡回答，兩個回合會併成一次執行，回合邊界與輸出歸屬都會錯；lease 另外保護輪替、記憶寫入與本機模式的 server 啟動，這些 inbox 管不到 | 不改變行為，不另列待決事項；關聯 SCN-012 |
| TBD-6 | 預設免費模型的派工與技能遵循表現 | 已解決（2026-11-26）：NVIDIA 憑證被拒（403）後，使用者表示「使用免費模型驗證也可以」。驗收可以用 NVIDIA nemotron 或預設免費模型 opencode/big-pickle判定，每份紀錄寫明實際用的模型；對照組（例如 SCN-001 的 b65b487）用同一個模型。免費模型額度用完時照實記下，不繞過額度限制 | 正式部署若用預設免費模型，派工表現可能與驗收時不同；關聯 SCN-003 |
| TBD-7 | SCN-006 的「檔案互動規則」：Telegram 的產出目錄每回合不同（`artifacts/<scope>/<turn_id>`），不能整段放進 agent 定義 | 已解決（2026-11-26，使用者選「規則進定義，只留路徑（建議）」）：規則文字寫進 agent `wukong` 的定義，user message 只留一行 `[產出目錄] <路徑>`，視為和召回記憶一樣的動態區塊；SCN-006 的「檔案互動規則」指規則文字。沒有採用的做法：每個 scope 固定一個產出目錄（要改檔案回傳流程）、路徑也放 entry（每回合在對話紀錄多一則通知） | Task 4.2 的外迴圈檢查 user message 時，產出目錄那一行屬於動態區塊；關聯 SCN-006 |
| TBD-8 | 常駐 V2 server 在 agent 定義改寫後幾十毫秒內送出的回合，仍用舊定義（Task 4.2 外迴圈：WUKONG_MEMORIA_HINT 開關的答案相反） | 已解決（2026-11-26）：使用者選「等 server 讀到再送」。入口改寫過 primary agent 的定義時，下一個回合送 prompt 前輪詢 /api/agent，等 wukong 的 system 與新內文相同才送，最多 5 秒，逾時警告後照送；按需本機模式與 CLI backend 不用等 | 只影響常駐 server 模式、緊接在定義改寫之後的回合（CLI 單次執行、入口重啟時積壓的訊息或到期的排程）；SCN-008 |
| TBD-9 | NVIDIA nemotron 上，查找題（讀程式碼、寫 README）只有 1/3 派給 librarian：primary 自己讀完模組、寫好整份 README，只把存檔派給 fixer（[dispatch-eval.md](dispatch-eval.md) 第 3、4 輪） | 已解決（2026-11-27，使用者選「記為已知差距，不修」）：這是 nemotron 的模型行為，文件內容仍由悟空產生、只有存檔派錯角色，不影響結果；big-pickle 上 3/3 正確 | SCN-003 以 big-pickle 通過（使用者 2026-11-27 指示 NVIDIA 限流時改用 opencode 的模型）；nemotron 的這個差距沒有修。要不要再改措辭（例如「文件內容由 librarian 撰寫，不要自己寫好再請別人存檔」），或修訂查找題的門檻，由使用者決定 |
| TBD-10 | 最終的 agent 定義（f0ce34a：多了「派工後就結束這次回覆、等它回報」）還沒有完整跑過一輪派工評估：2026-11-27 nemotron 的實作題 3 次都被限流或逾時打斷，big-pickle 的免費額度同日用完（[dispatch-eval.md](dispatch-eval.md) 第 6 輪） | 已解決（2026-11-27）：big-pickle 額度到 14:38 仍未恢復，使用者要求改試 NVIDIA 上回應較快的模型；以 nemotron-3.5-super-vl-120b-a12b 用最終定義跑完整一輪，6 題全部通過（[dispatch-eval.md](dispatch-eval.md) 第 7 輪），使用者選「採用 super-vl 這輪」，big-pickle 不再補跑 | TBD-6 約定的驗證模型另加 super-vl（使用者 2026-11-27 指示），整輪只用這一個模型；結果不和第 5 輪 big-pickle 直接比較 |
| TBD-11 | review-65e960a 需決策：Task 3.3 在 wukong-memory 新增 `agent_session_state.session_started_at` 欄位（7 天輪替不必每回合問 backend），涉及檔案表把 wukong-memory 列為不可觸及 | 已解決（2026-11-27，使用者選「接受並記錄」）：保留這個欄位與遷移，不改回每回合查 backend | 既有資料庫自動加欄位；舊列沒有值時，下一回合向 backend 查一次 session 開始時間補上（`backfill_session_started`）；涉及檔案表的「不可觸及」以這一列為例外 |
| TBD-12 | review-65e960a 的 SHOULD FIX 與 NICE TO HAVE 怎麼處理 | 已解決（2026-11-27，使用者決定）：修正 SHOULD FIX 1（背景工作只算 subagent）、2（工作區範本改成派工）、3（人格那句的過時提醒）、6（原始派工紀錄進版控），NICE TO HAVE 9（排程階段回報後回到悟空）、13（文件小修），見計畫 Task 6.6；SHOULD FIX 4、5 由 TBD-11、TBD-10 處理。NICE TO HAVE 7（`ObservedStep.skill_name` 固定是 None）、8（固定提示有兩條組裝路徑）、10 併入 13 的文件說明、11（只有搶先改寫定義的入口會等 server 重新載入）、12（subagent 被限流時回合仍算成功）沒有選，留作已知差距 | 沒選的各項不影響驗收；修正後依「補跑後增量覆審」的決定，TBD-10 補跑後送增量覆審 |
| TBD-13 | review-623c436 的 NICE TO HAVE 怎麼處理 | 已解決（2026-11-27，使用者決定）：修正 1（Telegram 泡泡回報後回到悟空；查證時發現 Web 也一樣，一併修正）與 4（README 凍結內容兩處），見計畫 Task 6.7；2（新的工作區 AGENTS.md 範本沒有真實模型證據：派工評估的工作區不放 AGENTS.md）與 3（用名稱找角色的寫法有兩份：`executor.rs`、`turn.rs`，目前一致）記為已知差距 | 2、3 不影響驗收；修正後再送增量覆審 |
| TBD-14 | review-4ccfcec 的 NICE TO HAVE 怎麼處理 | 已解決（2026-11-27，使用者選「三項都記入待確認」）：1（CHANGELOG、docs/entrypoints.md 寫回報後回到「悟空」，Telegram 與 Web 實際顯示「🐵 思考中…」，只有排程字面是「悟空」）、2（「回報後取目前角色」的三行在 Telegram 與 Web 各一份，可讓 `ActiveSubagents::reported` 回傳目前角色）、3（`wukong-orchestrator` 的模組說明與 AGENTS.md 的 crate 表沒提到 `ActiveSubagents`）都記為已知差距，不修 | 都不影響行為與驗收；PASS 沿用 review-4ccfcec |
| TBD-15 | PR #47 第三方審查（[留言](https://github.com/raybird/Wukong/pull/47#issuecomment-7214581086)，Reviewed HEAD cf47307，CONDITIONAL PASS）的 P2-1、P2-2、P3 怎麼處理 | 已解決（2026-11-27，使用者決定）：P2-1 查背景工作失敗時有限重試，仍失敗以目前的回覆收尾；P2-2 查證 V2 2.3.2 中斷父 session 不會取消背景 subagent，取消時改從背景工作清單補中斷子 session；修正見計畫 Task 6.8，之後送增量覆審，再回覆 PR 請第三方做第二輪。P3（agent 定義同步狀態只在單一行程內）就是 TBD-12 沒選修的第 11 項，維持已知差距 | SCN-002、SCN-012 的既有保障補強，不新增驗收 |

## Timeline

| 日期 | 異動 |
|------|------|
| 2026-11-26 | 建立；逐題確認 Q1～Q4（驗證模型、規劃偏好、agent 定義的部署方式、scope 的傳遞） |
| 2026-11-26 | SCN-001～SCN-014 全部核准 |
| 2026-11-26 | Background 修訂：驗證模型改為 NVIDIA nemotron 或預設免費模型（TBD-6），沿用核准；舊基線 18377ce |
| 2026-11-26 | Phase 1 探測完成，TBD-1～TBD-4 成立；另發現 subagent 一律在背景執行，回合結束的判定補進 Task 3.1 |
| 2026-11-26 | Phase 2 完成：agent 定義、技能寫到工作區 `.opencode/`；AGENTS.md 遷移改由入口啟動時執行（不在 entrypoint、install.sh）；技能清單的精簡移到 Task 5.2 |
| 2026-11-26 | Task 3.1 完成：回合只以 agent `wukong` 送出一次，planner 與接力鏈移除；回合等 subagent 回報才結束。真實 V2 有兩次背景工作清單漏掉執行中的 fixer，改為先看事件 |
| 2026-11-26 | Task 3.3 完成：Wukong 不再依回合數壓縮；7 天輪替改在每回合開始時檢查，session 開始時間記在記憶庫（`agent_session_state.session_started_at`），每回合不問 backend |
| 2026-11-26 | Task 3.4 內迴圈完成：自訂的 opencode 命令沒指定 agent 時以 `wukong` 執行，使用者指定的 agent 照舊，非 opencode 命令不加 `--agent`；3.2、3.4 的真實 V2 驗證等免費模型額度恢復 |
| 2026-11-26 | Task 3.2 內迴圈完成，測試釘住每次送 prompt 前都選取指定模型；Task 4.1 內迴圈完成，V2 backend 的 scope 改放 session 指令層的 entry `wukong.scope`，排程能力的固定說明併入 agent 定義，CLI backend 照舊附在 prompt；重跑派工探測仍是免費額度 429 |
| 2026-11-26 | Task 4.2 內迴圈完成：V2 backend 的輸出要求、長期記憶能力、檔案互動規則移進 agent 定義，Telegram 每回合只附產出目錄一行（TBD-7，使用者決定），CLI backend 照舊附在 prompt |
| 2026-11-26 | Task 5.1 內迴圈完成：gateway 從 V2 事件取出 subagent 的開始與回報，觸發入口的角色顯示與產出卡片；排程的階段改成「悟空·fixer」，不再寫「第 k 棒」 |
| 2026-11-26 | Task 5.2 完成：規劃偏好的設定、套用、Web 的技能偏好頁與 API（含技能目錄）移除，舊 API 回 404；舊設定檔的 `planner_preferences` 照常載入、存檔不再寫回；技能清單只留名稱與內容；AGENTS.md 的 settings 路徑更正 |
| 2026-11-26 | Task 3.2 完成：免費模型額度用完，改用 NVIDIA nemotron（憑證已恢復可用）跑派工探測 6 次全部通過，primary 與 subagent 都跑在指定模型 |
| 2026-11-26 | Task 3.4、4.1 完成：以 NVIDIA nemotron 跑真實 V2，CLI backend 以 agent wukong 執行且不蓋過使用者的 --agent；排程建在正確的 scope，輪替後的新 session 也拿得到 scope。Task 4.2 的 SCN-008 發現常駐 server 在定義改寫後立刻送出的回合仍用舊定義，待決定處理方式 |
| 2026-11-26 | Task 4.2 完成：依使用者決定（TBD-8），入口改寫 agent 定義後，常駐 server 讀到新定義才送下一個回合；長期記憶能力的開關以真實模型驗證 |
| 2026-11-26 | Task 5.1 完成：Web 與 Telegram（mock transport）接真實 V2 與 NVIDIA nemotron，派工時看得到「悟空·fixer」與 fixer 的產出 |
| 2026-11-26 | Task 6.4 完成：評估 V2 session inbox，結論是不用它取代 scope lease，維持立即拒絕（TBD-5 已解決） |
| 2026-11-27 | Task 6.1 完成：SCN-003 派工門檻以 opencode/big-pickle 6 題全部通過（NVIDIA 限流，依使用者指示改用 opencode 的模型）；依使用者決定，primary agent 不能改檔，shell 只能跑排程與 memoria 指令；nemotron 查找題的差距記為 TBD-9 |
| 2026-11-27 | 規格修訂並重新核准 SCN-001：步數少於 b65b487 只要求 short 任務，tool 任務照實記錄（primary 不能改檔、建檔要派工的代價）；primary 的派工說明加上「派工後就結束這次回覆、等回報」；舊基線 f9bc86e |
| 2026-11-27 | Task 6.2 完成：對照 b65b487，short 任務步數 3 → 1、opencode 啟動 2 → 1，兩種任務成功率都是 7/7；tool 任務步數中位數 7（b65b487 為 4），照修訂後的 SCN-001 記錄 |
| 2026-11-27 | Task 6.3 完成：dind 裡從假發版 rc.44（b65b487）升級到本分支，本機與 server 模式各跑一輪（[rehearsal.md](rehearsal.md)）。發現 server 模式以對話建立的排程寫進 opencode-server 自己的資料庫、永遠不執行（v0.25.1 起的既有問題），使用者選在 #45 修（e70e4e2）；Telegram 因沒有 bot token 未執行 |
| 2026-11-27 | Task 6.5 完成：根目錄 AGENTS.md、README.md、docs/docker.md、docs/entrypoints.md、docs/cli-reference.md、docs/memory.md、crate README 與 CHANGELOG `[Unreleased]` 改成單次呼叫加派工的現況 |
| 2026-11-27 | 使用者確認：Task 5.2 維持移除整個 Skills 分頁、`/api/skills/*` 與 `/skills` 指令；TBD-9 記為已知差距不修；TBD-10 先送審，big-pickle 額度恢復後補跑 |
| 2026-11-27 | 完整交付範圍 f9bc86e..65e960a 獨立審查 PASS（review-65e960a.md，無 MUST FIX，SHOULD FIX 6 項、NICE TO HAVE 7 項）；核准 commit 回填推導出的基線 f0ce34a |
| 2026-11-27 | 使用者對審查建議的決定記為 TBD-11、TBD-12；Task 6.6 修正選修的六項（64e95b8、0c54bf0、a091c4f 與本次文件提交），CI 三道全綠；PASS 的範圍不含這些修正，TBD-10 補跑後送增量覆審 |
| 2026-11-27 | TBD-10 已解決：big-pickle 額度未恢復，依使用者指示改用 NVIDIA nemotron-3.5-super-vl-120b-a12b 以最終定義補跑完整一輪，6 題全部通過（dispatch-eval.md 第 7 輪）；接著送 d900bda 之後的增量覆審 |
| 2026-11-27 | 增量覆審 65e960a..623c436 PASS（review-623c436.md，無 MUST FIX、SHOULD FIX，NICE TO HAVE 4 項）；上一份 PASS 選修的六項與 TBD-10、TBD-11 都確認處理完成 |
| 2026-11-27 | 使用者對增量覆審建議的決定記為 TBD-13；Task 6.7 修正 NICE TO HAVE 1、4（Telegram、Web 在 subagent 回報後回到悟空，README 凍結內容兩處），再送增量覆審 |
| 2026-11-27 | 增量覆審 623c436..4ccfcec PASS（review-4ccfcec.md，無 MUST FIX、SHOULD FIX，NICE TO HAVE 3 項） |
| 2026-11-27 | 使用者對 review-4ccfcec 建議的決定記為 TBD-14（三項都不修）；沒有待觀察項目，狀態寫成已完成，合併後生效 |
| 2026-11-27 | PR #47 第三方審查 CONDITIONAL PASS（P2 兩項、P3 一項）；使用者的決定記為 TBD-15，Task 6.8 修正 P2-1、P2-2，CI 三道全綠；狀態改回審查中，送增量覆審 |

---
**建立日期**: 2026-11-26  
**分級**: Large（runtime、orchestrator、skills、gateway、三個入口與排程的顯示、部署設定）  
**風險**: High\
**狀態**: 審查中
