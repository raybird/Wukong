# 實作計畫

2026-11-26 建立。本計畫是任務的權威來源，GitHub #45 的範圍清單只是建檔時的摘要。

- 風險與首要驗證依 [README](README.md#風險與首要驗證)。
- 現況見 [requirement-analysis.md](./requirement-analysis.md)，取捨見 [technical-analysis.md](./technical-analysis.md)。
- Phase 與 Task 在本檔細化（2026-11-26 decompose）。

所有探測與真實模型驗證都在隔離環境執行：

- 獨立 HOME，只放 NVIDIA 一筆憑證；
- 只送合成提示；
- 不用正式部署的副本，正式部署 RunWuKong 也不碰（project.md gate；不把正式對話送給外部模型）。

## 設計方案

- **agent 定義**：一個 primary agent `wukong`，人格與輸出要求寫在它的 prompt 裡，內容併入 SOUL.md。另有五個 subagent：explorer、oracle、librarian、fixer、designer，描述沿用現在的角色卡。
  - 定義由 Wukong 寫入，並隨版本更新。寫在哪個目錄，依 Phase 1 的探測結果決定。
- **run_turn 的資料流**：
  1. recall
  2. 準備 scope 的 session（拿 lease、7 天輪替）
  3. 以 agent `wukong` 送出一次 prompt
  4. 空輸出補救
  5. remember
- **拿掉的部分**：planner、接力鏈、`[前序協作]`、輔助棒的上一回合補償、依回合數壓縮。
  - 輪替後第一個回合補上上一回合的做法保留（SCN-004）。
  - lease 改在任何 LLM 呼叫之前取得；現在 planner 就在 lease 之前執行（SCN-012）。
- **session 指令層**：建立或輪替 session 時寫入以下內容（Q4）：
  - scope 與排程能力的用法；
  - Telegram 的檔案互動規則；
  - `WUKONG_MEMORIA_HINT` 開啟時的長期記憶能力。

  Phase 1 驗證不過時，退回每回合只附一行 scope。
- **技能**：Superpowers 技能寫到 V2 的 skill 探索路徑。拿掉 `[技能規範指引]`，以及 AGENTS.md 範本裡對應的段落。
- **角色顯示**：`on_role`、`on_step` 改由 gateway 從 V2 事件裡的 subagent 資訊產生，入口的顯示介面盡量不變。
- **規劃偏好**：移除 `planner_preferences`；serde 忽略舊欄位。
- **AGENTS.md 範本遷移**：entrypoint 與 installer 共用同一份歷代範本雜湊清單。工作區的檔案與清單中任一版完全相同時，先備份再換成新版；不同時不動，只在 log 提醒。

## 實作步驟

2026-11-26 decompose：README 的風險策略（High，首要驗證是 Phase 1 的契約探測）仍成立，Phase 1 先於所有實作。Task ID 固定，不重編。Phase 1 的結論要改變做法時，先回 README 修訂規格，再修訂受影響的 Task。

### 責任對照

| Scenario | 責任 Task | 支援 |
|---|---|---|
| SCN-001 | 6.2 | 3.1、6.3 |
| SCN-002 | 5.1 | 1.4、6.1、6.3 |
| SCN-003 | 6.1 | 1.4、2.1 |
| SCN-004 | 3.1 | 6.3 |
| SCN-005 | 2.2 | 1.2、6.3 |
| SCN-006 | 4.2 | 4.1、6.3 |
| SCN-007 | 4.1 | 1.3、6.3 |
| SCN-008 | 4.2 | 1.3 |
| SCN-009 | 2.3 | 2.1、6.3 |
| SCN-010 | 3.3 | — |
| SCN-011 | 5.2 | — |
| SCN-012 | 3.1 | — |
| SCN-013 | 3.2 | 1.4 |
| SCN-014 | 3.4 | 1.1 |

待確認事項：TBD-1 由 1.2，TBD-2 由 1.3，TBD-3 與 TBD-4 由 1.4，TBD-5 由 6.4 結論。

## Phase 1 — V2 能力的契約探測（首要驗證）

里程碑：Phase 2 之後依賴的六項 V2 能力，都有成立或不成立的實測紀錄。

探測的共同條件：
- 隔離環境：獨立 HOME 與 XDG 目錄，只放 NVIDIA 一筆憑證，只送合成提示。
- 不需要模型的項目加 `--network none`。
- 腳本放 `docs/issues/issue-0045/scripts/`，紀錄寫成 `probe-*.md`。

### Task 1.1 — agent 定義的位置與指定方式
- 產出與範圍：`probe-agents.md`。內容：
  - agent 與 subagent 定義的目錄與格式，在本機按需、常駐 server（工作區掛在 server 的專案目錄）、CLI（`opencode run --agent`）三種模式下是否都會載入；
  - prompt 或 session 指定 `agent` 是否生效；
  - 定義檔在 server 執行中改寫後，要不要重啟才生效。
- 相依：無
- 驗收編號：SCN-014、SCN-009 的前提
- 完成判準：三種模式各有一個成立或不成立的結論，附命令與輸出，並寫出 Phase 2 採用的目錄。
- 驗證：最小真實請求。不需要模型的部分讀 `/api/agent` 一類的端點；需要模型的部分各跑 1 回合，從事件確認用的是哪個 agent。
- 狀態：📝 待實作
- 證據：

### Task 1.2 — 技能的探索路徑與使用
- 產出與範圍：`probe-skills.md`。內容：
  - V2 2.3.2 讀取專案或使用者技能的路徑與 SKILL.md 格式要求，讓技能出現在 `/api/skill`（2026-11-24 專案目錄的 SKILL.md 沒有出現）；
  - prompt 附上 `skills`，或由模型自行載入時，事件裡怎麼看出技能被使用；
  - 八個 Superpowers 技能的 frontmatter 能不能原樣被接受。
- 相依：無
- 驗收編號：SCN-005 的前提（TBD-1）
- 完成判準：
  - 找到可用的路徑與格式，至少一個 Superpowers 技能出現在 `/api/skill`，模型用 NVIDIA 跑 1 回合時事件看得到技能被使用；
  - 或者證實不成立，記錄嘗試過的路徑，回 README 修訂 SCN-005。
- 驗證：契約探測加最小真實請求；必要時讀 V2 的原始碼或 `/openapi.json` 找出探索規則。
- 狀態：📝 待實作
- 證據：

### Task 1.3 — session instruction entries
- 產出與範圍：`probe-instructions.md`。內容：
  - `PUT /api/experimental/session/{id}/instructions/entries/{key}` 寫入後，重新讀取、server 重啟後是否仍在；
  - 是否出現在 session 的訊息紀錄與 opencode.db 的 user message；
  - 模型在下一個回合能否引用其中的內容（例如寫入一個 scope 字串，請模型回報）；
  - 刪除與覆寫的行為；本機按需模式每次都啟動新 server，entries 是否仍跟著 session。
- 相依：無
- 驗收編號：SCN-006、SCN-007、SCN-008 的前提（TBD-2）
- 完成判準：每一項都有結論；不成立時記下採用 2026-11-26 Q4 的退路（每回合附一行 scope），並寫明哪些區塊仍可放進 agent 定義。
- 驗證：最小真實請求加 opencode.db 唯讀查詢。
- 狀態：📝 待實作
- 證據：

### Task 1.4 — subagent 的模型、事件與派工抽樣
- 產出與範圍：`probe-subagent.md`，以及 V2 真實事件樣本（存成 fixture，供 5.1 使用）。內容：
  - prompt 指定模型時，subagent 用的模型；subagent 定義沒寫模型時，沿用的是 primary agent 的模型、prompt 指定的模型，還是預設模型；
  - 事件裡辨識 subagent 的欄位（task 工具的參數、子 session 的事件、agent 名稱），以及 subagent 輸出所在的位置；
  - 用 NVIDIA 抽樣：一題實作任務跑 3 次，看是否派給 fixer。
- 相依：1.1
- 驗收編號：SCN-013、SCN-002 的前提（TBD-3、TBD-4），SCN-003 的初步訊號
- 完成判準：
  - 模型沿用規則與 subagent 身分欄位都有結論，fixture 已存檔；
  - 派工抽樣的 3 次結果已記錄。抽樣不當門檻，門檻在 6.1。
- 驗證：最小真實請求；fixture 以 #44 `capture-free-limit.py` 的做法擷取事件串流，只含合成提示。
- 狀態：📝 待實作
- 證據：

## Phase 2 — agent、subagent 與技能的定義與部署

里程碑：三種模式都讀得到 Wukong 管理的 agent、subagent 與技能，既有部署的 AGENTS.md 依規則遷移。

### Task 2.1 — Wukong 管理的 agent 與 subagent 定義
- 產出與範圍：
  - 定義內容：primary agent `wukong`，prompt 含 SOUL.md 的人格與現在 `[輸出要求]` 的內容；五個 subagent 的描述沿用 `Role::card()` 與 `description()`，可以調整措辭。
  - 寫入邏輯：寫到 1.1 決定的位置，沿用 `skill_assets.rs` 的原子替換與內容比對，內容沒變時不寫；入口啟動時寫入，Docker 的 entrypoint 也寫。
  - 不動 Rust 的回合流程。
- 相依：1.1
- 驗收編號：SCN-009、SCN-003（支援）
- 完成判準：
  - 單元測試確認首次寫入、內容沒變時不重寫、新版內容會覆寫；
  - 1.1 的探測腳本改用寫出來的定義後，三種模式都列得到 `wukong` 與五個 subagent。
- 驗證：雙迴圈。外迴圈是探測腳本對真實 V2 列出 agent；內迴圈是寫入邏輯的單元測試。
- 狀態：📝 待實作
- 證據：

### Task 2.2 — 技能改走 V2 的 skill
- 產出與範圍：
  - 八個 Superpowers 技能寫到 1.2 決定的路徑與格式；
  - 移除 `[技能規範指引]`（persona.rs）與 `.wukong/skills/superpowers` 的寫入；
  - Dockerfile 與 entrypoint 跟著調整。
  - 技能清單（catalog）只保留內嵌與寫出所需的部分。
- 相依：1.2
- 驗收編號：SCN-005
- 完成判準：
  - 真實 V2 的 `/api/skill` 列得到八個技能；
  - 用 NVIDIA 跑一題符合技能的任務，事件看得到技能被使用；
  - prompt 不含技能規範指引。
- 驗證：雙迴圈。外迴圈是接真實 V2 的 `#[ignore]` 整合測試；內迴圈是寫出路徑與內容的單元測試。
- 狀態：📝 待實作
- 證據：

### Task 2.3 — workspace/AGENTS.md 範本更新與遷移
- 產出與範圍：
  - 新版 `workspace/AGENTS.md` 範本：拿掉技能動態加載與人格段落（人格改由 2.1 提供），只留專案規則。
  - 遷移規則由 entrypoint 與 install.sh 共用：歷代範本的雜湊清單取自 `git log -- workspace/AGENTS.md` 的每一版。工作區的檔案與任一版相同時，先備份再換成新版；不同時不動，只在 log 提醒有過時的段落。
  - `SOUL.md` 照舊複製。
- 相依：2.1
- 驗收編號：SCN-009
- 完成判準：
  - entrypoint 與 installer 的測試涵蓋三種工作區：全新部署、與舊範本相同（以 ac60ebe 版為例）、改過的；
  - 用 NVIDIA 問「你是誰」，三種工作區都以孫悟空回答。
- 驗證：雙迴圈。外迴圈是 `scripts/test-docker-runtime.sh` 或新的腳本測試，加上真實回合；內迴圈是遷移判定的測試。
- 狀態：📝 待實作
- 證據：

## Phase 3 — run_turn 改為單一 primary agent

里程碑：一個回合只以 agent `wukong` 送出一次 prompt；`cargo test --workspace --locked` 全過。

### Task 3.1 — 拿掉 planner 與接力鏈
- 產出與範圍：
  - run_turn 改成：recall，拿 lease，輪替檢查，以 agent `wukong` 送出一次，空輸出補救，remember。
  - 移除 `plan_skill_chain_with_preferences`、`[前序協作]`、輔助棒，以及輔助棒的上一回合補償；保留輪替後第一回合的補償。
  - lease 在任何 LLM 呼叫之前取得。
  - 空輸出補救只剩修復提示這一條路徑。
  - 改寫 orchestrator、`scripts/test-support/cli_backend.rs` 的假 agent，以及受影響的測試（約 110～130 個）。
- 相依：2.1
- 驗收編號：SCN-004、SCN-012；SCN-001（支援）
- 完成判準：
  - 測試確認以下各項：
    - 同一個 scope 第二個回合被拒絕時，backend 沒有收到任何呼叫；
    - 一般回合只送出一次，指定的是 agent `wukong`；
    - prompt 不含上一回合；
    - 輪替後的第一回合含上一回合；
    - 空輸出時補送一次修復提示；
    - 上游錯誤照舊往外傳。
  - CI 三道全過。
- 驗證：雙迴圈。外迴圈是入口層的既有測試（CLI、Web、Telegram、排程），改成單次回覆後的行為；內迴圈是 turn.rs 的測試。行為不變的部分（錯誤分類、補救）沿用同一組斷言。
- 狀態：📝 待實作
- 證據：

### Task 3.2 — 指定模型時 subagent 用同一個模型
- 產出與範圍：依 1.4 的結論，讓 primary agent 與 subagent 都用回合指定的模型（Web、排程、`default_model`）。可能的做法是 prompt 帶模型、定義裡不寫模型，或寫入時帶模型，依 1.4 的結論選擇。
- 相依：1.4、3.1
- 驗收編號：SCN-013
- 完成判準：
  - 用 NVIDIA 跑一題會派工的任務，事件裡 primary agent 與 subagent 的模型都是指定的那一個；
  - 有單元或 adapter 測試釘住送出的參數。
- 驗證：雙迴圈。外迴圈是真實 V2；內迴圈是 gateway 送出內容的測試。
- 狀態：📝 待實作
- 證據：

### Task 3.3 — 拿掉依回合數壓縮
- 產出與範圍：
  - 移除 `WUKONG_SESSION_COMPACT_EVERY_TURNS` 的壓縮路徑；設了這個變數也不報錯，只是沒有作用。
  - 7 天輪替改在每回合開始時檢查。
  - 保留期清理不變。
  - `.env.example`、compose、docs 跟著更新（W-3）。
- 相依：3.1
- 驗收編號：SCN-010
- 完成判準：
  - 測試確認以下各項：
    - 21 回合以後不呼叫 compact；
    - 設了舊變數照常運作；
    - 建立超過 7 天的 session 在下一個回合輪替，舊 session 被刪除。
  - `bash scripts/test-docker-runtime.sh` 通過。
- 驗證：單迴圈。session.rs 與 turn.rs 的測試涵蓋同一個可觀察層級（是否呼叫 compact、是否輪替），沒有另一層整合責任。
- 狀態：📝 待實作
- 證據：

### Task 3.4 — CLI backend 與自訂命令
- 產出與範圍：預設的 `opencode run` 帶上 agent `wukong`；使用者自訂的 `WUKONG_AGENT_CMD` 不被加上會改變行為的旗標（沿用 #44 對 `--standalone` 的判定方式）。
- 相依：1.1、3.1
- 驗收編號：SCN-014
- 完成判準：
  - argv 測試涵蓋預設命令與自訂命令；
  - CLI backend 接真實 V2 跑 1 回合，用的是 agent `wukong`。
- 驗證：雙迴圈。外迴圈是 #44 的 `cli-v2-actual.sh` 做法；內迴圈是 backend.rs 的 argv 測試。
- 狀態：📝 待實作
- 證據：

## Phase 4 — 固定提示移到 agent 定義與 session 指令層

里程碑：每回合的 user message 只剩使用者輸入與動態區塊。

### Task 4.1 — session 指令層與排程能力
- 產出與範圍：
  - 建立或輪替 session 時，以 session instruction entries 寫入 scope；1.3 不成立時採用退路，每回合附一行 scope。
  - `[排程能力]` 的說明改成技能，或併入 agent 定義，依 1.2 與 1.3 的結論選擇。
  - 新 session 與輪替出來的 session 都要寫入。
- 相依：1.3、3.1
- 驗收編號：SCN-007；SCN-006（支援）
- 完成判準：
  - 接真實 V2 的測試：要求「每天早上提醒我」，`wukong schedule list` 出現擁有者為該 scope 的排程；
  - 輪替後再做一次，結果相同；
  - user message 不含 scope（退路時只多一行 scope）。
- 驗證：雙迴圈。外迴圈是真實 V2 加 NVIDIA；內迴圈是 gateway 寫入 entries 的 adapter 測試（假 V2 server）。
- 狀態：📝 待實作
- 證據：

### Task 4.2 — 其餘固定區塊移出 user message
- 產出與範圍：
  - 角色卡、輸出要求、長期記憶能力（`WUKONG_MEMORIA_HINT` 開啟時）、Telegram 檔案互動規則，移到 agent 定義或 session 指令層。
  - 相關記憶、其他回合正在執行、排程的無人值守提示維持原樣。
  - `remember` 剝除入口區塊的行為維持原樣。
- 相依：4.1
- 驗收編號：SCN-006、SCN-008
- 完成判準：
  - 接真實 V2 的測試：查 opencode.db，Telegram 與 Web 路徑的 user message 不含上述區塊，動態區塊仍在；
  - `WUKONG_MEMORIA_HINT` 開啟時，模型能回報長期記憶能力的指令，關閉時不能。
- 驗證：雙迴圈。外迴圈查 opencode.db 的整合測試；內迴圈是 persona 與 prompt 組裝的單元測試。
- 狀態：📝 待實作
- 證據：

## Phase 5 — 入口的角色顯示與規劃偏好移除

里程碑：四個入口顯示的角色來自 V2 的 subagent 資訊；規劃偏好不再存在。

### Task 5.1 — 角色顯示改從 subagent 資訊取得
- 產出與範圍：
  - gateway 從 V2 事件取出 subagent 的身分與產出，觸發 `on_role` 與 `on_step`（或改名後的等價回呼）。primary agent 自己處理時顯示「悟空」。
  - Telegram 的狀態泡泡、Web 的 SSE `role`／`step` 與產出卡片、`turn_steps`、CLI 的 stderr、排程執行訊息的階段文字（不再寫「第 k 棒」）。
- 相依：1.4、3.1
- 驗收編號：SCN-002
- 完成判準：
  - 用 1.4 的真實事件 fixture 測 gateway 的對應；
  - 四個入口各有顯示測試；
  - 用 NVIDIA 跑一題實作任務，Web 與 Telegram（mock transport）看得到「悟空·fixer」與產出。
- 驗證：雙迴圈。外迴圈是入口測試加真實回合；內迴圈是事件對應的單元測試。
- 狀態：📝 待實作
- 證據：

### Task 5.2 — 移除規劃偏好
- 產出與範圍：
  - 移除 `planner_preferences` 的讀寫、Web 的偏好介面與 API，以及 CLI、Telegram 的套用；serde 忽略舊欄位。
  - 更正根目錄 AGENTS.md 裡 settings 的路徑（實際是 `/data/settings.json` 或 `WUKONG_SETTINGS_FILE`）。
- 相依：3.1
- 驗收編號：SCN-011
- 完成判準：
  - 用含舊欄位的設定檔測試 settings 載入，其他欄位照舊；
  - Web 的偏好 API 回 404，前端不再呼叫它；
  - 四個入口的既有測試全過。
- 驗證：雙迴圈。外迴圈是 Web API 與入口測試；內迴圈是 settings 的單元測試。
- 狀態：📝 待實作
- 證據：

## Phase 6 — 真實模型驗收、效能對照與常青文件

里程碑：真實模型的門檻與效能對照都有持久化結果，常青文件更新完畢。

### Task 6.1 — 派工門檻（固定任務組）
- 產出與範圍：
  - 固定任務組：實作、程式碼探索、資料查找、架構建議、介面設計各一題，加一題直接回答。
  - 執行腳本與結果紀錄 `dispatch-eval.md`，NVIDIA nemotron 每題 3 次，記錄每次實際派給誰。
  - 預設免費模型額度允許時另跑一組，只記錄（TBD-6）。
- 相依：4.2、5.1
- 驗收編號：SCN-003
- 完成判準：
  - 實作題 3/3 派給 fixer，直接回答題 3/3 不派工，其他各題至少 2/3 派給預期的 subagent。
  - 判準先以對照組自檢：用固定回覆的假 agent 確認腳本判得出「沒派工」與「派錯」，避免判準失效。
- 驗證：觀測式驗收，附反向自檢。
- 狀態：📝 待實作
- 證據：

### Task 6.2 — 效能對照
- 產出與範圍：
  - bench-turn.sh 補上每回合 LLM step 數與 opencode 啟動次數（從 opencode.db 與程序紀錄取得）。
  - 在相同條件下（1.5 CPU、3 GiB、NVIDIA nemotron、全新合成狀態）比較 b65b487 的映像與本分支的映像，short 與 tool 各至少 7 回合。
  - 結果寫進 `bench-v2-slim.md`。
- 相依：4.2
- 驗收編號：SCN-001
- 完成判準：
  - 本分支的成功率不低於 b65b487，每回合 LLM step 數較少，本機按需模式每回合啟動一次 opencode；
  - step 數的取法先在 b65b487 上驗證：要量得到 planner 那一步。
- 驗證：觀測式驗收，附反向自檢。
- 狀態：📝 待實作
- 證據：

### Task 6.3 — 隔離環境的端到端演練
- 產出與範圍：`rehearsal.md`。在獨立的 Docker daemon（#44 的做法）裡，從 v0.25.1 之後的 V2 版本升級到本分支，本機按需與 server 模式各跑一輪。
  - 涵蓋：一般回答、派工給 fixer、追問、技能、排程建立、人格、Telegram（mock transport 或可取得時的真實 bot）與 Web 的顯示。
  - 也涵蓋工作區是舊範本 AGENTS.md 的遷移。
- 相依：6.1、6.2
- 驗收編號：SCN-001、SCN-002、SCN-004～SCN-007、SCN-009（端到端支援）
- 完成判準：每一項都有實際回合的紀錄；沒跑到的項目照實寫出原因。
- 驗證：垂直切片。最需要驗證的是升級後各入口的整合；單元測試與探測都不涵蓋部署之後的組合。
- 狀態：📝 待實作
- 證據：

### Task 6.4 — inbox 評估
- 產出與範圍：在實作計畫寫評估結論：V2 session inbox（delivery 設成 queue 或 steer）能否取代或簡化 scope lease 的協調，以及會改變哪些可觀察行為。附 1 次最小真實請求，確認 queue 的行為。
- 相依：3.1
- 驗收編號：TBD-5（不改變行為）
- 完成判準：
  - 結論寫明建議與理由，README 的 TBD-5 已更新；
  - 建議改行為時，列成新的待決事項，不在本 issue 實作。
- 驗證：等價證據，記錄探測命令與輸出。不改變可觀察行為。
- 狀態：📝 待實作
- 證據：

### Task 6.5 — 常青文件
- 產出與範圍：
  - 根目錄 AGENTS.md：架構、一回合資料流、orchestrator 的職責、關鍵設計細節。
  - `docs/docker.md`：agent 定義、技能路徑、AGENTS.md 遷移、移除的變數。
  - `docs/entrypoints.md`：角色顯示、偏好移除。
  - `CHANGELOG.md` 的 `[Unreleased]`。
- 相依：6.3
- 驗收編號：全部（文件）
- 完成判準：依 project.md 常青文件表逐項核對，文件內容與產物一致（W-1）。
- 驗證：等價證據，以靜態核對逐項記錄。不改變可觀察行為。
- 狀態：📝 待實作
- 證據：

## 測試策略

- **真實模型**：派工、技能遵循、追問、人格，unit test 證明不了，以接真實 V2 加 NVIDIA nemotron 的整合測試或演練腳本驗證（`#[ignore]`，與 #44 的 `managed_actual` 相同的做法）。同一題重複執行，記錄每次的結果。
- **事件與 adapter**：
  - 用 Phase 1 擷取的 V2 真實事件存成 fixture，測 subagent 資訊的對應與顯示。
  - 假 agent 改輸出單一 primary agent 的回覆，不再輸出 planner 的格式。
- **既有保障**：空輸出補救、上游錯誤分類、lease、輪替與保留期的測試保留，或改寫成等價的測試。行為不變的部分以改寫前後同一組斷言全綠為證據。
- **部署**：entrypoint 與 installer 的範本遷移跑 `bash scripts/test-docker-runtime.sh` 與對應的 installer 測試（project.md）。
- **CI 三道**：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`。

## 檢查清單

- [ ] Phase 1 的六項探測都有紀錄，TBD-1～TBD-4 已更新
- [x] 每個 Scenario 都有責任 Task（2026-11-26 decompose，見責任對照）
- [ ] 真實模型的驗證只用合成提示與 NVIDIA 憑證，正式部署沒有被碰
- [ ] 常青文件依 project.md 全部更新
