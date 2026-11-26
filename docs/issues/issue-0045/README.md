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
    And 以 bench-turn.sh 的 short 與 tool 任務，在相同條件下對照 #44 合併版本（b65b487）：成功率不低於它，每回合的 LLM step 數少於它

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
- **核准 commit**: `待提交`
- **核准來源**: 範圍與「留在 Wukong」來自 GitHub #45 內文與使用者 2026-11-24 的決定（拆成兩個 issue，#44 合併後開始）；2026-11-26 使用者要求「好開始 /dev-cycle #45」。同日逐題確認：Q1「NVIDIA 為準（建議）」→ Background、SCN-003；Q2「移除（建議）」→ SCN-011；Q3「Wukong 管理、自動更新（建議）」→ SCN-009；Q4「session 指令層（建議）」→ SCN-007。同日總結確認推導行為（派工門檻、輪替後仍補上一回合、保留的動態區塊、舊壓縮變數忽略且 7 天輪替改每回合檢查、lease 在任何 LLM 呼叫前取得、subagent 沿用指定模型、自訂命令不加旗標、inbox 只評估）選「全部核准（建議）」→ SCN-001～SCN-014。同日 NVIDIA 憑證被拒（403）後，使用者表示「使用免費模型驗證也可以」→ Background、TBD-6

| Scenario | 核准日期 | 狀態 |
|----------|---------|------|
| SCN-001 | 2026-11-26 | 已核准 |
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
3. 📝 **Phase 3** - run_turn 改為單一 primary agent，拿掉 planner、接力鏈與依回合數壓縮
4. 📝 **Phase 4** - 固定提示移到 agent 定義與 session 指令層
5. 📝 **Phase 5** - 入口的角色顯示與規劃偏好移除
6. 📝 **Phase 6** - 真實模型驗收、效能對照與常青文件

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
| TBD-5 | 範圍第 7 項：用 V2 的 session inbox（delivery 設成 queue）簡化同一 scope 並行回合的 lease | 待確認 | 只做評估，結論記入實作計畫；要改變「立即拒絕」的行為時另外取得核准；關聯 SCN-012 |
| TBD-6 | 預設免費模型的派工與技能遵循表現 | 已解決（2026-11-26）：NVIDIA 憑證被拒（403）後，使用者表示「使用免費模型驗證也可以」。驗收可以用 NVIDIA nemotron 或預設免費模型 opencode/big-pickle判定，每份紀錄寫明實際用的模型；對照組（例如 SCN-001 的 b65b487）用同一個模型。免費模型額度用完時照實記下，不繞過額度限制 | 正式部署若用預設免費模型，派工表現可能與驗收時不同；關聯 SCN-003 |

## Timeline

| 日期 | 異動 |
|------|------|
| 2026-11-26 | 建立；逐題確認 Q1～Q4（驗證模型、規劃偏好、agent 定義的部署方式、scope 的傳遞） |
| 2026-11-26 | SCN-001～SCN-014 全部核准 |
| 2026-11-26 | Background 修訂：驗證模型改為 NVIDIA nemotron 或預設免費模型（TBD-6），沿用核准；舊基線 18377ce |
| 2026-11-26 | Phase 1 探測完成，TBD-1～TBD-4 成立；另發現 subagent 一律在背景執行，回合結束的判定補進 Task 3.1 |
| 2026-11-26 | Phase 2 完成：agent 定義、技能寫到工作區 `.opencode/`；AGENTS.md 遷移改由入口啟動時執行（不在 entrypoint、install.sh）；技能清單的精簡移到 Task 5.2 |
| 2026-11-26 | Task 3.1 完成：回合只以 agent `wukong` 送出一次，planner 與接力鏈移除；回合等 subagent 回報才結束。真實 V2 有兩次背景工作清單漏掉執行中的 fixer，改為先看事件 |

---
**建立日期**: 2026-11-26  
**分級**: Large（runtime、orchestrator、skills、gateway、三個入口與排程的顯示、部署設定）  
**風險**: High\
**狀態**: 實作中
