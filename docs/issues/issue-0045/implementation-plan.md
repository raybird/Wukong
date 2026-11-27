# 實作計畫

2026-11-26 建立。本計畫是任務的權威來源，GitHub #45 的範圍清單只是建檔時的摘要。

- 風險與首要驗證依 [README](README.md#風險與首要驗證)。
- 現況見 [requirement-analysis.md](./requirement-analysis.md)，取捨見 [technical-analysis.md](./technical-analysis.md)。
- Phase 與 Task 在本檔細化（2026-11-26 decompose）。

所有探測與真實模型驗證都在隔離環境執行：

- 獨立 HOME，憑證只放 NVIDIA 一筆，用預設免費模型時不放憑證；
- 真實模型用 NVIDIA nemotron 或預設免費模型 opencode/big-pickle（README TBD-6），每份紀錄寫明實際用的模型，回合用的不是指定模型時中止；
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
- 隔離環境：獨立 HOME 與 XDG 目錄，憑證只放 NVIDIA 一筆或不放，只送合成提示。
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
- 狀態：✅ 已完成（2026-11-26）
- 證據：[probe-agents.md](probe-agents.md)，`scripts/probe-agents.sh` 20 項全部 PASS，模型 opencode/big-pickle（TBD-6）。
  - 三種模式都讀得到工作區 `.opencode/agents/`。Phase 2 採用這個目錄，不用全域設定目錄：binary 模式寫在那裡會改到使用者自己的 OpenCode。
  - 不存在的 agent 會讓回合失敗（Agent not found），不會改用 build。執行中新增的定義不用重啟就生效。
  - 內建的 explore、general 會和 Wukong 的 subagent 一起出現，可能影響 SCN-003，留給 Phase 2 與 Task 6.1。

### Task 1.2 — 技能的探索路徑與使用
- 產出與範圍：`probe-skills.md`。內容：
  - V2 2.3.2 讀取專案或使用者技能的路徑與 SKILL.md 格式要求，讓技能出現在 `/api/skill`（2026-11-24 專案目錄的 SKILL.md 沒有出現）；
  - prompt 附上 `skills`，或由模型自行載入時，事件裡怎麼看出技能被使用；
  - 八個 Superpowers 技能的 frontmatter 能不能原樣被接受。
- 相依：無
- 驗收編號：SCN-005 的前提（TBD-1）
- 完成判準：
  - 找到可用的路徑與格式，至少一個 Superpowers 技能出現在 `/api/skill`，用真實模型跑 1 回合時事件看得到技能被使用；
  - 或者證實不成立，記錄嘗試過的路徑，回 README 修訂 SCN-005。
- 驗證：契約探測加最小真實請求；必要時讀 V2 的原始碼或 `/openapi.json` 找出探索規則。
- 狀態：✅ 已完成（2026-11-26）
- 證據：[probe-skills.md](probe-skills.md)，`scripts/probe-skills.sh` 15 項全部 PASS，模型 opencode/big-pickle（TBD-6）。
  - 八個 Superpowers 技能原樣放在工作區 `.opencode/skills/` 就出現在 `/api/skill`；`.wukong/skills/superpowers` 讀不到。Phase 2 採用 `.opencode/skills/`。
  - 模型依 description 自己呼叫 `skill` 工具載入技能，事件 `session.tool.called` 的 `input.id` 是技能 id；prompt 附 `skills` 也有效，但內文會寫進對話紀錄。

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
- 狀態：✅ 已完成（2026-11-26）
- 證據：[probe-instructions.md](probe-instructions.md)，`scripts/probe-instructions.sh` 20 項全部 PASS，模型 opencode/big-pickle（TBD-6）。
  - entry 存在 opencode.db，server 重啟後仍在且模型讀得到，每個 session 各自一份；覆寫與刪除只影響那個 key。
  - 寫入後不再改的 entry 不進對話紀錄。模型看過之後才改值，V2 會插一則 `system` 通知到對話紀錄。scope 每個 session 寫入一次不再改，不受影響，不需要 Q4 的退路。
  - 角色卡、輸出要求、檔案互動規則與排程能力的固定說明放 agent 定義，scope 用 entry；長期記憶能力用 entry 或技能，Phase 4 決定。

### Task 1.4 — subagent 的模型、事件與派工抽樣
- 產出與範圍：`probe-subagent.md`，以及 V2 真實事件樣本（存成 fixture，供 5.1 使用）。內容：
  - prompt 指定模型時，subagent 用的模型；subagent 定義沒寫模型時，沿用的是 primary agent 的模型、prompt 指定的模型，還是預設模型；
  - 事件裡辨識 subagent 的欄位（task 工具的參數、子 session 的事件、agent 名稱），以及 subagent 輸出所在的位置；
  - 用真實模型抽樣：一題實作任務跑 3 次，看是否派給 fixer。
- 相依：1.1
- 驗收編號：SCN-013、SCN-002 的前提（TBD-3、TBD-4），SCN-003 的初步訊號
- 完成判準：
  - 模型沿用規則與 subagent 身分欄位都有結論，fixture 已存檔；
  - 派工抽樣的 3 次結果已記錄。抽樣不當門檻，門檻在 6.1。
- 驗證：最小真實請求；fixture 以 #44 `capture-free-limit.py` 的做法擷取事件串流，只含合成提示。
- 狀態：✅ 已完成（2026-11-26）
- 證據：[probe-subagent.md](probe-subagent.md)，`scripts/probe-subagent.sh`；fixture 在 [fixtures/](fixtures/v2-dispatch-fixer.ndjson)。模型用三個免費模型（TBD-6）。
  - subagent 沿用 session 選取的模型；設定檔的模型在 server 上沒有套用，Wukong 不受影響（adapter 本來就逐 session 選取）。
  - 身分欄位：`subagent` 工具的輸入、子 session 的 `session.created`（parentID、agent、model），產出在 primary inbox 的 `synthetic` 項目（metadata.source 為 subagent）。
  - 派工抽樣：big-pickle 3/3 派給 fixer（第一次執行另有 1/1）；deepseek 0/2，primary 自己寫檔。
  - 新發現：subagent 一律在背景執行，primary 先結束一次，回報後再執行一次才產生最終回覆。已補進 Task 3.1；CLI 的 `opencode run` 會等，不需處理。

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
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - 內迴圈：`crates/wukong-runtime/src/agent_assets.rs` 的 6 個單元測試，涵蓋首次寫入、內容沒變不重寫、舊版或改過的會覆寫、同目錄使用者自己的定義不動、人格取自工作區 SOUL.md（沒有時用範本）、description 是單行的 YAML。紅燈：寫入改成空操作時其中 4 個失敗（另 2 個是防呆，空操作下本來就過）。CI 三道全過（`cargo test --workspace --locked` 798 passed、14 ignored）。
  - 外迴圈：`scripts/probe-managed-agents.sh` 用 ignored 測試 `write_definitions_for_probe` 寫出的定義，在 `--network none` 下 22 項全部 PASS：`opencode debug agents`、常駐 server 的 `/api/agent`（wukong 的 prompt 含人格、派工、輸出要求，frontmatter 的註解不進 prompt）、剛啟動 server 的第一個 prompt、CLI `--agent wukong`（對照組 nosuchagent 印出 Agent not found）。
  - 做法上的選擇：
    - 寫入目錄用 backend 執行 opencode 的 `wukong_gateway::workspace_dir()`。技能現在用的 `resolve_skill_workspace()` 在沒設 `WUKONG_WORKSPACE` 時退回目前目錄，與 backend 不一致，留給 Task 2.2。
    - 人格讀工作區的 SOUL.md，使用者改了 SOUL.md 就跟著改；沒有或是空的時用內建範本。
    - 寫入時機是四個入口啟動時（`bootstrap::prepare_agent_definitions`；CLI 只在要跑回合時）。Docker entrypoint 不另外寫：常駐入口一啟動就寫進共用的工作區，V2 不用重啟就會載入（1.1）。
    - 派工規則的措辭沿用 1.4 抽樣時的版本。`[輸出要求]` 在 Phase 4 拿掉每回合附加之前，會同時出現在定義與 prompt。

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
  - 用真實模型跑一題符合技能的任務，事件看得到技能被使用；
  - prompt 不含技能規範指引。
- 驗證：雙迴圈。外迴圈是接真實 V2 的 `#[ignore]` 整合測試；內迴圈是寫出路徑與內容的單元測試。
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - 內迴圈：
    - `skill_assets.rs` 的 4 個單元測試：八個技能寫到 `.opencode/skills/{名稱}/SKILL.md`，內容與內嵌相同；根目錄沒有其他檔案；內容沒變不重寫，改過的會還原；同目錄使用者自己的技能不動；並行寫入不留暫存檔。
    - `turn.rs` 的 `run_turn_leaves_skills_to_v2`：planner 選了技能，prompt 也不含 `[技能規範指引]` 與 SKILL.md 路徑，回合不再建立 `.wukong`。紅燈：改動前因 prompt 含技能區塊而失敗。
    - `scripts/test-docker-runtime.sh` 改成檢查 Dockerfile、Dockerfile.release、entrypoint、release workflow 都不再含 `superpowers`。改動前的 Dockerfile 有 1 行符合，這項檢查會失敗。
    - CI 三道全過（796 passed、15 ignored）。
  - 外迴圈：`scripts/probe-managed-skills.sh`（opencode/big-pickle，TBD-6）12 項全部 PASS。
    - `/api/skill` 列得到 Rust 寫出的八個技能，工作區沒有其他技能。
    - 「請用測試先行的方式寫 add(a, b) 與 pytest 測試」這一題，wukong 用 `skill` 工具載入了 test-driven-development。
  - 發現：V2 會把 `.opencode/skills/` 根目錄的 .md 檔也當成技能（第一次執行時 `SUPERPOWERS-SOURCE.md` 被列成一個技能）。上游出處因此只留在 repo 的 `crates/wukong-skills/assets/superpowers/SOURCE.md`，不寫進工作區。
  - 範圍調整：
    - 技能清單的精簡移到 5.2。現在 planner 與 Web 偏好 API 還在用它。
    - 技能與 agent 定義一起在入口啟動時寫入（`bootstrap::prepare_workspace_assets`），位置是 backend 的工作區（`workspace_dir()`）。原本每回合在 `resolve_skill_workspace()`（會退回目前目錄）寫一次的做法已移除。
    - 映像與 entrypoint 不再帶、不再同步技能，release workflow 也不再複製。既有工作區的 `.wukong/skills/superpowers` 留著不刪：V2 讀不到它，舊範本 AGENTS.md 的引用由 2.3 處理。

### Task 2.3 — workspace/AGENTS.md 範本更新與遷移
- 產出與範圍：
  - 新版 `workspace/AGENTS.md` 範本：拿掉技能動態加載與人格段落（人格改由 2.1 提供），只留專案規則。
  - 遷移規則由 entrypoint 與 install.sh 共用：歷代範本的雜湊清單取自 `git log -- workspace/AGENTS.md` 的每一版。工作區的檔案與任一版相同時，先備份再換成新版；不同時不動，只在 log 提醒有過時的段落。
  - `SOUL.md` 照舊複製。
- 相依：2.1
- 驗收編號：SCN-009
- 完成判準：
  - entrypoint 與 installer 的測試涵蓋三種工作區：全新部署、與舊範本相同（以 ac60ebe 版為例）、改過的；
  - 用真實模型問「你是誰」，三種工作區都以孫悟空回答。
- 驗證：雙迴圈。外迴圈是 `scripts/test-docker-runtime.sh` 或新的腳本測試，加上真實回合；內迴圈是遷移判定的測試。
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - 範本：`workspace/AGENTS.md` 拿掉人格那一句（「你目前作為孫悟空」）與整段「技能動態加載機制」，其餘專案規則不變。V2 本身就會附 skill 的使用指引，範本不再另寫技能段落。
  - 做法調整：遷移改寫在 Rust（`crates/wukong-runtime/src/workspace_template.rs`），由四個入口啟動時的 `bootstrap::prepare_workspace_assets` 執行，不寫在 entrypoint 與 install.sh。
    - Docker 與 binary 兩種部署都跑同一批入口，結果是同一套規則只有一份實作。
    - 歷代範本改用原文比對，不用雜湊清單。原文取自 `git log -- workspace/AGENTS.md` 的三版（ac60ebe、e72cccd、65511d2），放在 `crates/wukong-runtime/assets/retired-agents-md/`。
    - entrypoint 與 installer 照舊只在缺檔時複製範本（SOUL.md 也照舊）。
  - 內迴圈：`workspace_template.rs` 的 5 個單元測試。
    - 新範本不含人格與技能段落。
    - 三版舊範本都先備份成 `AGENTS.md.bak-{版本}` 再換新；再啟動一次判為已是新版。
    - 改過的不動、不備份，回報「技能動態加載機制」過時；沒有過時段落的不回報。
    - 缺檔與新版都不動。
    - 紅燈：把範本換回舊版時有 2 項失敗（新範本檢查，以及 65511d2 被判為已是新版）。
  - 外迴圈：`scripts/probe-agents-md-migration.sh`（opencode/big-pickle，TBD-6）12 項全部 PASS。
    - 三種工作區（全新、與 ac60ebe 相同、ac60ebe 加上自訂規則）各由真實的 `wukong` 入口啟動一次。
    - 結果符合預期：全新的不動、沒有 log；相同的備份後換新，log 寫明備份位置；改過的不動，log 提醒「技能動態加載機制」過時。
    - 再到 V2 以 wukong agent 問「你是誰」，三者都以孫悟空回答。改過的那份同時照自訂規則附上出處，可見它的 AGENTS.md 照常生效。
    - 第一次執行時，改過的工作區回答「俺老孫乃齊天大聖」，判定只找「悟空」兩字而誤判失敗；已改成接受孫悟空的自稱。
  - 升級後 agent 定義自動更新，由 2.1 的「舊內容被換掉」測試涵蓋。
  - 其他檢查：CI 三道全過（801 passed、15 ignored），`scripts/test-docker-runtime.sh` 與 `scripts/test-installer-upgrade.sh` 全過。

## Phase 3 — run_turn 改為單一 primary agent

里程碑：一個回合只以 agent `wukong` 送出一次 prompt；`cargo test --workspace --locked` 全過。

### Task 3.1 — 拿掉 planner 與接力鏈
- 產出與範圍：
  - run_turn 改成：recall，拿 lease，輪替檢查，以 agent `wukong` 送出一次，空輸出補救，remember。
  - 移除 `plan_skill_chain_with_preferences`、`[前序協作]`、輔助棒，以及輔助棒的上一回合補償；保留輪替後第一回合的補償。
  - lease 在任何 LLM 呼叫之前取得。
  - 回合等背景工作（1.4 的發現）：subagent 一律在背景執行，primary 會先結束一次，收到回報後再執行。
    - primary 的執行結束時查 `GET /api/experimental/job?sessionID={id}&descendants=true`，清單空了才算回合結束；
    - 本機按需模式等到那時才關 server；
    - 等待沿用回合的逾時，中斷回合時一併中斷子 session。
  - 空輸出補救只剩修復提示這一條路徑。
  - 改寫 orchestrator、`scripts/test-support/cli_backend.rs` 的假 agent，以及受影響的測試（約 110～130 個）。
- 相依：2.1
- 驗收編號：SCN-004、SCN-012；SCN-001（支援）
- 完成判準：
  - 測試確認以下各項：
    - 同一個 scope 第二個回合被拒絕時，backend 沒有收到任何呼叫；
    - 一般回合只送出一次，指定的是 agent `wukong`；
    - 派工的回合等到 subagent 回報與 primary 的最終回覆才結束（adapter 測試以 1.4 的 fixture 重播）；
    - prompt 不含上一回合；
    - 輪替後的第一回合含上一回合；
    - 空輸出時補送一次修復提示；
    - 上游錯誤照舊往外傳。
  - CI 三道全過。
- 驗證：雙迴圈。外迴圈是入口層的既有測試（CLI、Web、Telegram、排程），改成單次回覆後的行為；內迴圈是 turn.rs 的測試。行為不變的部分（錯誤分類、補救）沿用同一組斷言。
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - run_turn（`crates/wukong-runtime/src/turn.rs`）：
    - 流程改成：拿 lease（`prepare_final_session`，在任何 LLM 呼叫之前），recall，輪替時補上一回合，以 agent `wukong` 送出一次，空輸出補一次修復提示（同一個 session、同一個 agent），remember。
    - 移除 planner（`plan_skill_chain_with_preferences`、`parse_preferred_role`、`planner_preference_hint`、`planner_previous_turn`）、`[前序協作]`、輔助棒及它的上一回合補償。`persona::build_prompt` 一併移除。
    - `TurnOutput` 拿掉 `role`，`WukongError::Orchestrator` 移除。
  - 回合等背景工作（`crates/wukong-gateway/src/opencode_server.rs` 的 `consume_event_stream`）：
    - primary 的執行結束時，有以下任一情況就繼續讀事件：
      - 事件裡看得到還在執行的子 session（`session.execution.started` 之後還沒結束）；
      - primary 的 inbox 有還沒送進執行的項目（`session.inbox.enqueued` 之後還沒 `delivered`）；
      - `GET /api/experimental/job?sessionID={id}&descendants=true` 不是空的。
    - 前兩項是外迴圈發現後補的，見下方外迴圈第 2 點。
    - 子 session 從 `session.created` 的 `parentID` 認出（含子 session 再派出去的）。回合失敗或被取消時，primary 與這些子 session 都送 interrupt（`InterruptOnDrop`）。
    - 等待沿用回合的逾時。本機按需模式在 `run_streaming` 回來之後才關 server，所以已涵蓋背景工作的時間，不必另改。
  - 做法調整：
    - 角色卡（`[角色]`）隨 planner 一起拿掉。人格與派工規則在 agent 定義裡（2.1），固定提示的其餘區塊留給 Phase 4。
    - orchestrator 只留角色定義與 `SkillRouteOption`。demo binary `wukong-orchestrate` 與它的依賴一併移除，根目錄 README 與 AGENTS.md 的指令改成以假 agent 跑入口測試。
    - 接力鏈拿掉後 `on_role`、`on_step` 不再被呼叫：入口暫時不顯示角色，排程的階段固定是「悟空」。改由 subagent 事件產生是 Task 5.1 的範圍；Web 兩個輔助棒卡片的測試標成 ignored，註明 5.1 改寫。
    - 保存的規劃偏好在 5.2 移除之前不影響回合，Web 測試改成確認 prompt 不含偏好。
    - 假 agent（`scripts/test-support/cli_backend.rs`）收到非串流呼叫（原本給 planner 的）時失敗，入口測試因此能抓到殘留的 planner 呼叫。
  - 內迴圈：
    - turn.rs：
      - `a_turn_rejected_by_the_lease_makes_no_backend_call`：第二個回合被拒，backend 沒有收到呼叫；
      - `a_turn_is_one_call_to_the_wukong_agent_and_is_remembered`：一次呼叫，agent `wukong`；
      - `a_continuing_session_gets_no_previous_turn`：prompt 不含上一回合；
      - `an_old_session_rotates_and_the_turn_sees_the_previous_turn`：輪替後的第一回合含上一回合；
      - `run_turn_all_empty_repairs_with_text`、`run_turn_all_empty_repair_empty_returns_sentinel`：修復提示；
      - `failed_turn_releases_session_lease`：上游錯誤照舊往外傳。
      - 紅燈：前三項放在 HEAD 828fe85 的實作上（臨時 worktree）全部失敗。
    - gateway，以 1.4 的真實派工 fixture `server-dispatch-subagent.ndjson` 重播：
      - `a_turn_waits_for_a_running_subagent_even_when_the_job_list_is_empty`；
      - `the_job_list_keeps_the_turn_open_when_the_events_show_no_running_subagent`；
      - `a_report_waiting_in_the_inbox_keeps_the_turn_open`；
      - `a_turn_without_background_work_ends_at_the_first_execution`；
      - `a_failed_turn_interrupts_the_subagent_sessions_too`、`a_cancelled_turn_interrupts_the_subagent_sessions_too`。
      - 紅燈：只看清單的實作上，第 1、3 項失敗，回覆都是「已派給 fixer……等它完成後我會回報結果」。
  - 外迴圈：
    - 入口層的既有測試（CLI、Web、Telegram、排程）改成單次回覆後全過。CLI 入口的 `cli_backend` 測試另外確認每回合只串流呼叫一次，參數帶 `--agent wukong`。
    - 真實 V2：`scripts/probe-turn-dispatch.sh`（opencode/big-pickle，TBD-6）。
      - 做法：真實的 `wukong` 入口接常駐 server，請它派給 fixer 建立 hello.txt。檢查回合等到 fixer 回報與 primary 的最終回覆、只送一次 prompt、primary 是 agent `wukong`。
      - 只看清單的實作跑了 8 次：6 次 9 項全部 PASS，2 次回合在 primary 第一次執行結束時就結束，回覆是派工通知。
      - 那 2 次 fixer 的 session 都還在執行（有 busy、沒有 idle），清單卻是空的。第一次失敗時沒有存事件串流，V2 清單漏掉執行中工作的原因沒有查出來。
      - 依通過那幾次的事件順序（fixer 的 `execution.succeeded` 之後 2 ms 內 primary 的 inbox 收到回報，再開始下一次執行），補上前兩項事件判斷。
      - 補上之後免費模型額度用完（`provider.free-limit` 429），沒有重跑；照 TBD-6 不繞過。殘留：補上的判斷只有 fixture 重播的證據，真實 V2 的重跑等額度恢復後在 3.2 的真實派工一併確認。
  - 其他檢查：CI 三道全過（756 passed、14 ignored）。比 2.3 少的是 planner、接力鏈、`build_prompt` 被刪掉的測試，以及 `planner_probe.rs`。

### Task 3.2 — 指定模型時 subagent 用同一個模型
- 產出與範圍：依 1.4 的結論，讓 primary agent 與 subagent 都用回合指定的模型（Web、排程、`default_model`）。
  - 1.4 的結論：subagent 沿用 session 選取的模型。
  - 做法：定義裡不寫模型，沿用 adapter 現有的逐 session 選取，並確認沒有指定模型的回合（`default_model`）也走選取。
  - 設定檔指定的模型在 server 上沒有套用，不能依賴它。
  - 真實派工時一併以 `scripts/probe-turn-dispatch.sh` 重跑 3.1 補上的事件判斷（3.1 補上之後免費額度用完，沒有重跑）。
- 相依：1.4、3.1
- 驗收編號：SCN-013
- 完成判準：
  - 用真實模型跑一題會派工的任務，事件裡 primary agent 與 subagent 的模型都是指定的那一個；
  - 有單元或 adapter 測試釘住送出的參數。
- 驗證：雙迴圈。外迴圈是真實 V2；內迴圈是 gateway 送出內容的測試。
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - 做法沿用 1.4 的結論，沒有改程式：agent 定義不寫模型，adapter 的 `send_prompt` 每次送 prompt 前都選取回合指定的模型。
    - 新建與續接的 session 都走這條路；subagent 沿用 session 選取的模型（probe-subagent.md）。
    - Web、排程、CLI、Telegram 都以 `apply_default_model` 把設定的模型放進 `cfg.default_model`，run_turn 帶進每次請求。
  - 內迴圈（釘住既有行為）：
    - gateway：`a_new_session_selects_the_agent_and_model_before_the_prompt`（新 session 的建立、選 agent、選模型都在 prompt 之前）。續接的 session 由既有的 `agent_and_model_are_selected_before_the_prompt` 釘住。
    - runtime：`the_repair_prompt_keeps_the_turn_model`（空輸出補送的修復提示也帶同一個模型與 agent）。第一次送出由既有的 `run_turn_passes_default_model` 釘住。
    - 反向自檢：暫時讓 adapter 不選模型，gateway 測試失敗（請求清單沒有 `POST …/model`）；暫時讓修復提示不帶模型，runtime 測試失敗。改回後兩項都通過。
  - 外迴圈：2026-11-26 用 NVIDIA `nvidia/nvidia/nemotron-3-ultra-550b-a55b` 跑 `scripts/probe-turn-dispatch.sh`（新增 AUTH_JSON、MODEL 兩個參數：模型寫進隔離設定檔的 `agent.default_model`，另檢查所有 assistant 訊息的模型）。
    - 同一天免費模型 `opencode/big-pickle` 回 429（`provider.free-limit`），照 TBD-6 改用 nemotron，沒有繞過額度。
    - 先用一個合成請求確認 NVIDIA 憑證可用（稍早回 403，這次回 OK，opencode.db 記的模型是 nemotron）。
    - 連跑 6 次全部通過（13～40 秒），每次 10 項：回合成功、hello.txt 內容正確、只有一個 primary session 與一則 user message、primary 是 agent `wukong`、派給 fixer、fixer 的回報回到 primary、回傳的是 primary 的最終回覆，以及 primary 與 subagent 都跑在 nemotron。
    - 這也是 3.1 事件判斷的真實重跑：之前免費模型 8 次有 2 次只拿到派工通知，這 6 次都等到最終回覆。這幾次沒有記錄背景工作清單是否曾經是空的，所以不能證明事件判斷在那個競態下生效；那部分由 3.1 的重播測試釘住。
    - 環境：隔離 HOME（只放 NVIDIA 一筆憑證）、容器 `wukong-issue44:v2-local`（opencode 2.3.2），只送固定的合成提示。

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
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - `crates/wukong-runtime/src/session.rs`：
    - 移除依回合數壓縮的整條路徑（壓縮、失敗時的退避、404／405 改輪替），`SessionPolicy` 只剩 `lease_secs`。`WUKONG_SESSION_COMPACT_EVERY_TURNS` 不再讀取。
    - 每個回合拿到 lease 之後看 session 開始了多久，超過 7 天就輪替。
    - 使用者手動的 `/compact` passthrough 不變。
  - 做法調整：session 的開始時間記在 Wukong 的記憶庫，每回合不問 backend。
    - 理由：本機按需模式每問一次 backend 就要多啟動一個 opencode serve（約 0.7 秒），每回合都問等於每回合多 0.7 秒。
    - 做法：`agent_session_state` 加 `session_started_at` 欄（Unix 秒），開檔時補上（`crates/wukong-memory/src/store/mod.rs`）。`finish_agent_session_turn` 與 `set_agent_session` 第一次看到這個 `session_id` 時記下，同一個 session 不變。
    - 升級前就在的 session 沒有這一欄，由第一個回合以 `session_created_ms` 查一次 opencode 的 `time.created` 補記。查不到時從那時起算，之後不再查。
    - 副作用：純 CLI backend 原本永遠不輪替，現在 7 天後也輪替。CLI backend 的 `delete_session` 是預設的空操作，舊 session 不會被刪，留在 `opencode.db`，與原本一樣。
    - 沒有呼叫端的 `Memory::defer_agent_session_compaction` 一併移除。`mark_agent_session_compacted` 還用在 CLI 的手動 `/compact`，保留。
  - 測試：
    - session.rs：
      - `a_scope_past_twenty_turns_is_not_compacted`：21 回合後不壓縮（backend 的壓縮一律 panic），也不問 backend；
      - `the_old_compaction_variable_has_no_effect`：設了 `WUKONG_SESSION_COMPACT_EVERY_TURNS=1` 照常運作；
      - `an_old_session_rotates_at_the_start_of_the_next_turn`：開始 8 天的 session 在第 2 回合就輪替；
      - `a_young_session_continues`；
      - `a_session_from_before_the_upgrade_is_aged_by_the_backend_once`、`an_unknown_age_counts_from_now_and_is_not_queried_again`；
      - `lease_stays_held_during_a_slow_age_lookup`、`deciding_to_rotate_leaves_the_pointer_for_the_next_turn`。
      - 紅燈：這組測試寫成舊實作也能編譯的形式，放在舊實作上 7 項失敗：壓縮了、第 2 回合不輪替、升級前的 session 沒有補記。
    - turn.rs：
      - `an_old_session_rotates_and_the_turn_sees_the_previous_turn`：輪替後舊 session 被刪除、回合帶上一回合；
      - `a_young_session_past_twenty_turns_continues_without_compaction`：只呼叫一次、沒有 `/compact`；
      - `a_rotation_turn_that_fails_keeps_the_old_session`。
    - 記憶庫：
      - `session_started_at_is_set_when_a_session_is_first_seen_and_kept_for_it`；
      - `set_agent_session_started_only_touches_the_current_session`；
      - `migrate_adds_session_started_at_to_an_old_session_table`：升級前的資料表開檔時加上這一欄，既有的列是 NULL，其餘不變。
  - 設定與文件（W-3）：
    - `.env.example` 與兩份 compose 拿掉這個變數。`.env.example` 留一段說明，指出它不再讀取。
    - `docs/docker.md` 的變數表與「長期 session 的輪替」、`docs/entrypoints.md`、根目錄 AGENTS.md 的兩條設計細節、entrypoint 的 `compaction` 註解都已更新。
    - 保留期清理不變。
  - 其他檢查：
    - `bash scripts/test-docker-runtime.sh` 通過，37 個 `.env.example` 變數都進得了容器；
    - 兩份 compose 的 YAML 解析正常；
    - CI 三道全過（755 passed、14 ignored）。

### Task 3.4 — CLI backend 與自訂命令
- 產出與範圍：預設的 `opencode run` 帶上 agent `wukong`；使用者自訂的 `WUKONG_AGENT_CMD` 不被加上會改變行為的旗標（沿用 #44 對 `--standalone` 的判定方式）。
- 相依：1.1、3.1
- 驗收編號：SCN-014
- 完成判準：
  - argv 測試涵蓋預設命令與自訂命令；
  - CLI backend 接真實 V2 跑 1 回合，用的是 agent `wukong`。
- 驗證：雙迴圈。外迴圈是 #44 的 `cli-v2-actual.sh` 做法；內迴圈是 backend.rs 的 argv 測試。
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - 預設命令：`opencode run`（可帶 `--model`）走本機按需 backend（`build_backend_from_env`），不經過 CLI 的 argv。
    - 本機按需 backend 把請求交給 server adapter，在送 prompt 前選取 agent。run_turn 已指定 `wukong`（3.1）。
    - 由 gateway 的 `agent_and_model_are_selected_before_the_prompt` 與 turn.rs 的 `a_turn_is_one_call_to_the_wukong_agent_and_is_remembered` 釘住。
  - 自訂命令（走 CLI backend）：`assemble_argv` 比照 #44 對 `--standalone` 的判定。
    - 只在命令是 opencode、而且使用者沒有自己指定 `--agent`／`--agent=` 時加上 `--agent wukong`。
    - 原本一律附加：使用者寫的 `--agent custom` 會多一個 `--agent wukong` 蓋掉它，`printf` 這類非 opencode 的命令也會收到 `--agent`。
  - 內迴圈：backend.rs 的 argv 測試。
    - `a_custom_opencode_command_runs_as_wukong`；
    - `a_custom_command_keeps_its_own_agent`（`--agent custom` 與 `--agent=custom` 兩種寫法）；
    - `a_non_opencode_command_gets_no_agent_flag`。
    - 紅燈：舊實作上後兩項失敗，argv 分別多出 `--agent wukong`。
  - 入口：CLI 入口的 `cli_backend` 測試改成確認使用者自訂、不是 opencode 的假 agent 沒有收到 `--agent`。
  - 外迴圈：2026-11-26 `scripts/probe-cli-agent.sh`，照 #44 `cli-v2-actual.sh` 的做法讓 CLI backend 接容器裡的真實 V2 2.3.2，模型 NVIDIA `nvidia/nvidia/nemotron-3-ultra-550b-a55b`（免費模型同日 429），6 項全過：
    - A. 自訂命令沒有指定 agent：argv 是 `run --dangerously-skip-permissions --standalone --print-logs --log-level error --agent wukong --format json`，opencode.db 記的 assistant 是 agent `wukong`、模型 nemotron，回覆 OK。
    - B. 自訂命令自己寫了 `--agent build`：argv 沒有另加 `--agent wukong`，opencode.db 記的是 agent `build`。
    - 環境：隔離 HOME、只放 NVIDIA 一筆憑證、只送合成提示；容器以 `--rm` 執行，結束時 `docker kill`。
    - 2026-11-26 免費模型額度用完（`provider.free-limit` 429），照 TBD-6 不繞過，等額度恢復。
  - 其他檢查：CI 三道全過（758 passed、14 ignored）。

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
- 驗證：雙迴圈。外迴圈是真實 V2 加真實模型；內迴圈是 gateway 寫入 entries 的 adapter 測試（假 V2 server）。
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - 依 1.2、1.3 的結論選擇：排程能力的固定說明併入 primary agent 的定義，scope 用 entry，不做成技能，也不需要退路。
    - 固定說明是 `persona::scheduling_capability_guide`，寫在 `wukong` 的定義裡（啟動時寫入，`WUKONG_BIN` 在這時讀取）。說明裡的 `--scope` 寫成 `<目前的 scope>`，指向 `[目前的 scope]`。
    - scope 的文字是 `persona::current_scope_instruction`，key 為 `wukong.scope`。
  - gateway：`AgentRequest` 新增 `session_instructions`，`AiBackend::supports_session_instructions` 標出支援的 backend（常駐 server 與本機按需）。
    - adapter 的 `ensure_instructions` 在每次送 prompt 前列出 entries，缺少或值不同才 `PUT`，值相同不重寫（改值會在對話紀錄插通知，probe-instructions.md）。
    - 每回合都確認，所以新 session、輪替出來的 session、升級前就在的 session 都會在第一個回合寫入。
    - 同一個 session 上使用者自己的其他 entry 不動。
  - runtime：支援指令層的 backend，prompt 不再附排程能力也不含 scope；CLI backend 照舊附在 prompt（固定說明加 `[目前的 scope]`，兩者共用同一份文字）。
    - 限制：CLI backend 跑的若是 opencode（`--agent wukong`），agent 定義裡也有固定說明，會讀到兩份。
  - 內迴圈：
    - gateway（假 V2 server 補上 entries 的列出與寫入）：`a_new_session_gets_its_instructions_before_the_prompt`、`an_unchanged_entry_is_not_written_again`、`a_missing_or_stale_entry_is_written_on_the_next_turn`、`a_request_without_instructions_does_not_touch_the_entries`。反向自檢：暫時拿掉 `ensure_instructions` 的呼叫，前三項失敗。
    - runtime：`the_scope_goes_to_the_session_layer_not_the_prompt`（主提示與修復提示都不含排程能力與 scope，兩次都帶 entry）、`the_primary_agent_explains_scheduling_without_a_scope`；兩項先紅燈再綠燈。`run_turn_carries_the_scheduling_capability` 改成確認 CLI 路徑照舊附在 prompt、沒有 entry。
  - 文件：AGENTS.md「排程能力注入」、`docs/cli-reference.md`、`docs/docker.md` 的 `WUKONG_BIN`。
  - 外迴圈：2026-11-26 `scripts/probe-session-layer.sh` 回合 1、2：真實的 wukong 入口（常駐 server 模式）接容器裡的 V2 2.3.2，模型 NVIDIA nemotron（免費模型同日 429）。
    - 回合 1「每天早上 8 點提醒我喝水。」：模型在容器裡執行 `wukong schedule add-turn`，排程「喝水提醒」`0 8 * * *` 的擁有者是本回合的 scope；session 的指令層有 `wukong.scope`，值含這個 scope；opencode.db 的 user message 就是使用者原文。
    - 回合 2：把 `session_started_at` 改成 8 天前（真實的輪替路徑），同一個 scope 要求「每週一早上 9 點提醒我開週會。」：stderr 有 `session_rotation_due`，換成新 session，新 session 也有 `wukong.scope`，第二個排程「週會提醒」`0 9 * * 1` 一樣建在這個 scope。user message 只多了召回的相關記憶（SCN-006 允許），沒有固定區塊。
    - 排程指令用映像內建的 wukong（主機編出的 binary 需要 GLIBC 2.39，映像沒有），寫進同一個記憶庫；agent 的 bash 在容器裡執行，與正式部署相同。
    - 判定的可信度：探測第一版有 `check "…$(…)" $?` 的寫法，`$?` 會被訊息裡的命令替換覆寫，排程數那兩項的判定因此不可信（顯示的數字是對的）。已改成先存結果再判定，之後重跑的結果才列為證據。
  - 其他檢查：CI 三道全過（766 passed、14 ignored）。

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
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - 角色卡：Task 2.1 起已不附在 prompt（subagent 的角色卡在各自的定義裡）。
  - 輸出要求、長期記憶能力：V2 backend 的回合不附在 prompt，改寫在 primary agent `wukong` 的定義裡；CLI backend 照舊附在 prompt。
    - 長期記憶能力依 Phase 4 的決定併入 agent 定義，不用 entry 或技能：和排程能力同一種做法，開關本來就是部署層級（compose 對三個服務一起設定）。
    - 代價：開關改了要重啟才生效（啟動時才寫定義）。改用 entry 的話，切換開關時已經用過的 session 會在對話紀錄留下一則通知。
    - `materialize_agent_definitions` 讀 `WUKONG_MEMORIA_HINT`；測試改用 `write_definitions(dir, false)` 直接傳開關，避免與 turn.rs 切換這個環境變數的測試並行時偶發失敗。
  - 內迴圈：
    - `the_memoria_capability_follows_the_switch`：開啟時 primary agent 的定義有 `[長期記憶能力]`，關閉時沒有。
    - `the_scope_goes_to_the_session_layer_not_the_prompt` 加上「不含 `[輸出要求]`」。
    - `run_turn_advertises_memoria_only_when_enabled` 加上「開啟時 V2 backend 的 prompt 也不含」。CLI 路徑的開關測試照舊。
    - 三項先以「簽名改了、行為沒改」的版本跑出紅燈，再實作到綠燈。
  - Telegram 檔案互動規則（TBD-7，2026-11-26 使用者決定）：規則文字（`persona::file_interaction_rules`）寫進 agent `wukong` 的定義；Telegram 每回合只附一行 `[產出目錄] <路徑>`（`persona::ARTIFACT_DIR_HEADER`），路徑每回合不同，算動態區塊。
    - run_turn 照舊以這一行的標頭切出使用者原文，記憶與召回不含它（`remember` 剝除入口區塊的行為不變）。
    - CLI backend：回合帶有產出目錄時，run_turn 在 prompt 補上規則文字。
    - 舊標頭 `[Wukong 檔案互動規則]` 與常數 `FILE_RULES_HEADER` 移除，已存的記憶本來就不含規則，不受影響。
  - 內迴圈（續）：
    - `the_primary_agent_carries_the_file_rules`、Telegram 的 `the_artifact_instruction_is_only_the_directory_line`，先紅燈再綠燈。
    - `file_rules_reach_the_prompt_but_not_memory_or_recall` 改為 CLI 與 V2 兩組：兩者 prompt 都有產出目錄、記憶只有原文；只有 CLI 的 prompt 有規則文字。先紅燈再綠燈。
    - `without_other_running_turns_prompts_are_unchanged` 改用 V2 backend：它比對兩個回合的完整 prompt，CLI 路徑會被並行切換 `WUKONG_MEMORIA_HINT` 的測試影響（全套測試時失敗過一次，這個競態在改動前就存在）。
  - 文件：AGENTS.md「末棒輸出保證」與 remember 的說明、README.md「輸出要求」、`.env.example` 的 `WUKONG_MEMORIA_HINT` 說明。
  - 其他檢查：CI 三道全過（769 passed、14 ignored）。
  - 外迴圈：2026-11-26 `scripts/probe-session-layer.sh`，模型 NVIDIA nemotron（免費模型同日 429）。
    - SCN-006：4 個回合的 user message 都沒有任何固定區塊的標頭（排程能力、目前的 scope、輸出要求、長期記憶能力、檔案互動規則），只有使用者原文，輪替後的回合另有召回的相關記憶。agent `wukong` 的定義含排程能力、輸出要求、檔案互動規則。派工探測（3.2）的 6 次回合也一樣。入口是 CLI 接常駐 server；Telegram 每回合附的產出目錄那一行由 Task 4.2 的單元測試釘住。
    - SCN-008：回合 3 開啟 `WUKONG_MEMORIA_HINT`、回合 4 關閉，都問模型「系統提示裡有沒有提到 memoria CLI」。第一次跑的答案正好相反（開啟答「沒有」、關閉答「有」），從推理內容看得出模型用的是上一版定義。
      - 原因：wukong 啟動時改寫定義，17 毫秒後就送 prompt（回合 4：`wukong.md` mtime 684.995，agent-switched 685.012）。常駐 V2 server 的檔案監看還沒處理到這次改寫，這個回合就用了快取裡的舊定義。
      - `scripts/probe-agent-reload.sh` 直接對 server 改寫定義，中間隔了 docker exec 啟動 python 的幾百毫秒，就地寫入與 rename 都在下一個 prompt 生效，`/api/agent` 的清單反而晚一點才更新。
      - 對照：回合 3、4 之前各加一個同樣開關的暖身回合，等 2 秒再問，開啟答「有」、關閉答「沒有」。
      - 影響範圍：只有常駐 server 模式，而且回合緊接在定義改寫之後（CLI 單次執行；入口重啟時 Telegram 立刻取到積壓的訊息、schedulerd 立刻有到期的排程）。按需本機模式每回合啟動新的 server，不受影響。定義只在 Wukong 升級、`WUKONG_MEMORIA_HINT`／`WUKONG_BIN`／SOUL.md 改變時才會變。
      - repo 裡的探測沒有暖身回合，修正前回合 3、4 的兩項失敗，作為這個問題的紅燈。
    - 修正（使用者 2026-11-26 選「等 server 讀到再送」，README TBD-8）：
      - runtime：`write_if_changed` 回傳有沒有寫入；`write_definitions` 改寫 primary agent 時回報內文（frontmatter 之後、去掉頭尾空白，與 `/api/agent` 的 `system` 相同）；`materialize_agent_definitions` 記成待確認。V2 路徑的回合送 prompt 前取出（只取一次）並等待。
      - gateway：`AiBackend::wait_for_agent_system` 預設立刻回來；常駐 server backend 每 100 毫秒查一次 `GET /api/agent`，`wukong` 的 `system` 相同才回來，最多 5 秒，逾時由 runtime 印 `agent_definition_not_loaded` 警告後照送。按需本機模式每回合啟動新的 server、CLI backend 每次啟動新程序，都用預設。
      - 內迴圈：
        - gateway：`waits_until_the_server_lists_the_rewritten_definition`（清單依序是沒有自訂 agent、舊內文、新內文，查 3 次後回來）、`gives_up_when_the_server_keeps_the_old_definition`（300 毫秒逾時）。紅燈：空殼沒有發出任何 `/api/agent` 請求、也不會放棄。
        - runtime：`rewriting_the_primary_definition_reports_its_system_text`（第一次寫回報內文、沒變不回報、開關改變再回報）、`a_pending_definition_is_waited_for_and_none_is_not`。紅燈：空殼沒回報、沒等待。`the_system_text_is_what_v2_lists_for_the_definition` 以實測的定義檔與 `/api/agent` 回應為樣本，空殼裡就已實作，一開始就通過，是釘住測試。
      - 外迴圈：修正後 `probe-session-layer.sh` 連跑 3 次，19 項全過，開啟時答「有」、關閉時答「沒有」；回合 4 定義寫入到切換 agent 從 17 毫秒變成 132 毫秒，沒有逾時警告。派工探測（3.2）重跑 1 次照樣全過。
      - 其他檢查：CI 三道全過（769 passed、12 ignored）。

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
  - 用真實模型跑一題實作任務，Web 與 Telegram（mock transport）看得到「悟空·fixer」與產出。
- 驗證：雙迴圈。外迴圈是入口測試加真實回合；內迴圈是事件對應的單元測試。
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - gateway：`StreamEvent` 新增 `SubagentStarted { agent }`、`SubagentReported { agent, output }`，只有 V2 server 的事件會有。
    - 開始：子 session 的 `session.created`，parentID 是本回合的 session，帶 `agent`。
    - 回報：primary inbox 的 synthetic 項目 `<subagent sessionID="…" …>回報</subagent>`，用 sessionID 對回 agent。
    - 只認本回合直接派出的 subagent；其他 session 的子 session、不是 subagent 回報的 synthetic 項目都忽略。
  - runtime：run_turn 把 `SubagentStarted` 轉成 `on_role`、`SubagentReported` 轉成 `on_step`（`skill_name` 為 None），只認 Wukong 的五個角色（V2 內建的 general 等不觸發）；事件本身照樣轉給入口。
  - 入口：
    - Telegram 的狀態泡泡、Web 的 SSE `role`／`step`、產出卡片與 `turn_steps`、CLI 的 stderr 原本就依 `on_role`／`on_step` 顯示，沒有改顯示邏輯，只補上新事件的 match 分支（不處理，交給回呼）。
    - 排程：階段從「第 k 棒 {角色}」改成「悟空·{角色}」（最近派出的 subagent），失敗訊息為「悟空·fixer 失敗，已執行 …」。
  - 內迴圈：
    - gateway：`a_dispatched_subagent_is_reported_with_its_agent_and_output` 以 1.4 的真實樣本（`server-dispatch-subagent.ndjson`）重播整段，得到 fixer 開始與回報內文；`subagents_of_other_sessions_are_ignored`。紅燈：對應前送出 0 個 subagent 事件。
    - runtime：`subagent_events_drive_the_role_and_step_callbacks`（fixer 觸發 role 與 step，general 不觸發，4 個事件都轉給入口）。紅燈：on_role 沒被呼叫。
    - 四個入口：
      - Web：`chat_streams_subagent_role_and_step_before_answer`、`turn_persists_subagent_steps_and_serves_them`（取代 3.1 標成 ignored 的兩個輔助棒測試）；
      - Telegram：`a_dispatched_subagent_shows_in_the_status_bubble_and_live_events`（泡泡出現「🐵　悟空·fixer」，live 事件有 role fixer）；
      - 排程：`the_phase_names_the_dispatched_subagent`，紅燈時訊息是「第 1 棒 fixer 失敗…」；
      - CLI：`role_line` 抽成函式，`a_dispatched_subagent_is_named_on_stderr`。
    - 反向自檢：暫時讓 run_turn 不認任何角色，Web 兩項、Telegram、排程、runtime 的測試都失敗；改回後通過。
  - 文件：`docs/cli-reference.md` 的排程進度與失敗訊息範例。README.md 的時序圖、AGENTS.md「一回合資料流」等接力鏈的描述由 Task 6.5 一起更新。
  - 其他檢查：CI 三道全過（777 passed、12 ignored）。
  - 外迴圈：2026-11-26 模型 NVIDIA nemotron（免費模型同日 429），提示詞都是「請派給 fixer 處理：在工作區建立 hello.txt…」。
    - Web 與 Telegram：`scripts/probe-entries-dispatch.sh` 起容器裡的常駐 V2 2.3.2，以隔離 HOME 執行 `#[ignore]` 的 `web_streams_the_dispatched_subagent`、`telegram_names_the_dispatched_subagent`（`crates/wukong-{web,telegram}/tests/managed_actual.rs`，Telegram 只把 transport 換成 `MockTgClient`）。兩次執行都通過。
      - Web：SSE 依序是 `event: role`（`data: fixer`）、`event: step`（`"role":"fixer"`、`"skill":null`，內容是 fixer 的回報）、`event: answer`；hello.txt 寫在工作區。
      - Telegram：狀態泡泡改成「🐵　悟空·fixer 思考中…」，最後送出的回覆含 `hello from wukong`；成品照 Telegram 的規則寫進產出目錄 `.wukong/artifacts/user_tg-45/1/`。
      - opencode.db 的 assistant 是 `wukong` 與 `fixer`，都是 nemotron。
      - 過程：第一次兩邊都跑在免費模型上（429），因為入口每回合從設定檔取模型，`GatewayConfig.default_model` 不算數；改成把模型寫進隔離設定檔（Web 是 `AppState.settings_path`），之後才是上面的結果。
    - CLI：3.2 的派工探測，stderr 有「🐵 悟空·fixer」。
    - 排程：沒有接真實模型跑，階段名稱由 `the_phase_names_the_dispatched_subagent` 釘住。
    - CI 三道全過（769 passed、14 ignored，多的 2 個是這兩個真實測試）。
  - 程序：gitnexus detect_changes 在提交 c7072c1 之後才跑（應在提交前），結果只有預期的符號（run_turn、事件對應、排程階段、各入口的 match 分支與測試）。

### Task 5.2 — 移除規劃偏好
- 產出與範圍：
  - 移除 `planner_preferences` 的讀寫、Web 的偏好介面與 API，以及 CLI、Telegram 的套用；serde 忽略舊欄位。
  - 技能清單（`crates/wukong-skills/src/catalog.rs`）只留內嵌內容與寫出所需的部分：planner（3.1 移除）與偏好 API 移除後，角色對應、`route_options`、`find` 都不再有人用。從 2.2 移過來，那時兩者都還在用。
  - 更正根目錄 AGENTS.md 裡 settings 的路徑（實際是 `/data/settings.json` 或 `WUKONG_SETTINGS_FILE`）。
- 相依：3.1
- 驗收編號：SCN-011
- 完成判準：
  - 用含舊欄位的設定檔測試 settings 載入，其他欄位照舊；
  - Web 的偏好 API 回 404，前端不再呼叫它；
  - 四個入口的既有測試全過。
- 驗證：雙迴圈。外迴圈是 Web API 與入口測試；內迴圈是 settings 的單元測試。
- 狀態：✅ 已完成（2026-11-26）
- 證據：
  - 移除：
    - settings：`Settings.planner_preferences`、`PlannerPreferences`、`effective_planner_preferences`。`Settings` 沒有 `deny_unknown_fields`，舊欄位由 serde 忽略，struct 上補註解說明不可加。
    - gateway：`GatewayConfig.planner_preferences`、`PlannerPreferenceConfig`、`apply_planner_preferences`；CLI（main.rs、repl.rs、command.rs）、Telegram（dispatch.rs 兩處、main.rs）、Web（chat_api.rs）、schedulerd、排程 executor、runtime 與 scripts/test-support 的建構與套用一併拿掉。
    - Web：`/api/skills/preferences`（GET、PUT）與 `/api/skills/catalog`（只給偏好頁用，回的是角色對應）、`skills_api.rs`、技能偏好頁 `wukong-skills.js` 與它的路由、導覽列「技能」、對話頁的「技能偏好」狀態標籤與 `loadSkillStatus`、slash 選單的 `/skills`（說明是「配置或查看您的角色與技能偏好」）、只給偏好頁用的 `.skill-card`／`.skill-grid` 樣式。
    - 技能清單：`SkillSpec` 只留 `name` 與 `content`；`SkillId`、角色對應（`primary_role`、`collaborator_role`）、`description`、`route_options`、`find` 刪掉，`source_content` 保留。orchestrator 的 `router.rs`（`SkillRouteOption`）一併刪掉；wukong-skills 不再依賴 orchestrator，wukong-web 不再依賴 skills 與 orchestrator（Cargo.lock 跟著少 5 行）。`skill_assets.rs` 的測試改從 `all()` 找技能。刪除前以 grep 確認沒有其他使用者。
  - 測試：
    - 內迴圈：settings `legacy_planner_preferences_field_is_ignored`（含舊欄位的設定檔載入後 telegram、預設模型照舊，再存檔不寫回舊欄位）。紅燈：載入的部分移除前就會過（欄位還在），失敗在再存檔時仍寫出 `planner_preferences`；移除後通過。載入不報錯本身是釘住測試，serde 原本就忽略未知欄位。
    - 外迴圈：Web `skills_preference_api_is_gone`（偏好 API 的 GET、PUT 與技能目錄，有無 token 都回 404；`/components/wukong-skills.js` 回 404；index.html、app.js、wukong-chat.js、chat-thread-header.js 不含 `/api/skills`、`#/skills`、「技能偏好」）。紅燈：`GET /api/skills/preferences` 回 200；移除後通過。`chat_ignores_saved_planner_preferences` 改成 `chat_ignores_legacy_planner_preferences`，直接寫含舊欄位的 JSON，回合照常完成、prompt 不含偏好。
    - 隨功能刪掉的測試 15 個：settings 3、gateway 2、Web 偏好與目錄 API 6、技能清單 4（`SkillId` 兩個、`route_options`、`find`），新增 2 個。四個入口的既有測試全過。
    - 前端：`node --check` 全部元件通過，`node --test` 17 項通過。
  - 文件：根目錄 AGENTS.md 的 `wukong-settings` 一行改成實際的路徑解析（`WUKONG_SETTINGS_FILE`，沒設時固定 `/data/settings.json`，主機上執行也一樣；檔案不存在用預設值；舊欄位照常載入）。`docs/web-console-sop-v1.md` 拿掉技能偏好的段落、smoke 項目與 `node --check wukong-skills.js`，加註 2026-11-26 移除。README.md、docs/*.md 其他提到「偏好」的地方與規劃偏好無關。CHANGELOG 留給 Task 6.5。
  - 程序：提交前跑 gitnexus detect_changes，風險 low、受影響流程 0，變更符號只有預期的 Settings、技能清單、Web 測試、`WukongChat.initialize` 與文件段落（索引是舊的，行號對應略有偏差）。
  - 其他檢查：CI 三道全過（764 passed、0 failed、12 ignored；基線 777 − 刪掉的 15 + 新增的 2 = 764）。

## Phase 6 — 真實模型驗收、效能對照與常青文件

里程碑：真實模型的門檻與效能對照都有持久化結果，常青文件更新完畢。

### Task 6.1 — 派工門檻（固定任務組）
- 產出與範圍：
  - 固定任務組：實作、程式碼探索、資料查找、架構建議、介面設計各一題，加一題直接回答。
  - 執行腳本與結果紀錄 `dispatch-eval.md`，真實模型每題 3 次，記錄每次實際派給誰與用的模型（TBD-6）。
- 相依：4.2、5.1
- 驗收編號：SCN-003
- 完成判準：
  - 實作題 3/3 派給 fixer，直接回答題 3/3 不派工，其他各題至少 2/3 派給預期的 subagent。
  - 判準先以對照組自檢：用固定回覆的假 agent 確認腳本判得出「沒派工」與「派錯」，避免判準失效。
- 驗證：觀測式驗收，附反向自檢。
- 狀態：✅ 已完成
- 證據：[dispatch-eval.md](dispatch-eval.md)，`scripts/dispatch-eval.sh`、`scripts/dispatch_judge.py`（2026-11-26～2026-11-27）
  - 判準自檢 8/8 PASS（`dispatch_judge.py --self-check`）。改用已知答案的紀錄做對照組，取代假 agent；理由見 dispatch-eval.md。
  - **門檻：第 4 輪的定義在 `opencode/big-pickle` 上 6 題全部 PASS**（實作、探索、查找、架構、設計各 3/3 派給預期的 subagent，直接回答 3/3 不派工；18 回合都只用這個模型）。
    - NVIDIA nemotron 在最終定義的那一輪被限流（HTTP 429）打斷實作題，重跑仍 429。
    - 使用者 2026-11-27 指示「nvidia 限流可以換成 opencode 的模型」，所以以 big-pickle 完整跑一輪，沒有混用模型。
  - 評估中改了 primary 的定義（使用者 2026-11-26 的兩次決定，見 dispatch-eval.md 各輪）：
    - 第 1 輪（不限權限）實作 0/3：primary 自己用 edit／write 改檔。
    - 第 2 輪（`edit: deny`）：primary 改用 shell heredoc 與 `execute` 寫檔。
    - 第 3 輪起：`execute` 拒絕，shell 只允許 `<WUKONG_BIN> schedule *`（記憶能力開啟時另允許 memoria 指令）；派工說明要求交代路徑與 `[產出目錄]`；subagent 帶 `[檔案互動規則]`。
    - 單元測試：`only_the_primary_agent_is_denied_file_edits`、`the_primary_shell_only_allows_scheduling_and_memoria`，都先紅燈。
    - shell 收緊後，`probe-session-layer.sh` 19/19 PASS，排程仍建得出來。
    - CI 三道全過（771 passed、14 ignored）。
  - 已知差距：nemotron 上查找題第 3、4 輪都只有 1/3 派給 librarian（primary 自己寫好 README，只派 fixer 存檔）。記為 README TBD-9。
  - 最終定義（f0ce34a，Task 6.2 加上「派工後就結束回覆」）還沒有完整的一輪：2026-11-27 的 nemotron 重跑中實作題 3 次都被限流或逾時打斷（dispatch-eval.md 第 6 輪），big-pickle 的免費額度同日用完。記為 README TBD-10，額度恢復後補跑。2026-11-27 big-pickle 額度仍未恢復，依使用者指示改用 NVIDIA nemotron-3.5-super-vl-120b-a12b 補跑完整一輪（dispatch-eval.md 第 7 輪），6 題全部通過，使用者同意以此結案 TBD-10。

### Task 6.2 — 效能對照
- 產出與範圍：
  - bench-turn.sh 補上每回合 LLM step 數與 opencode 啟動次數（從 opencode.db 與程序紀錄取得）。
  - 在相同條件下（1.5 CPU、3 GiB、同一個真實模型、全新合成狀態）比較 b65b487 的映像與本分支的映像，short 與 tool 各至少 7 回合。
  - 結果寫進 `bench-v2-slim.md`。
- 相依：4.2
- 驗收編號：SCN-001
- 完成判準：
  - 本分支兩種任務的成功率都不低於 b65b487，short 任務每回合 LLM step 數較少，本機按需模式每回合啟動一次 opencode；tool 任務的 step 數照實記錄（2026-11-27 SCN-001 修訂）；
  - step 數的取法先在 b65b487 上驗證：要量得到 planner 那一步。
- 驗證：觀測式驗收，附反向自檢。
- 狀態：✅ 已完成
- 證據：[bench-v2-slim.md](bench-v2-slim.md)，原始結果在 [bench/](bench/)（2026-11-27，NVIDIA nemotron，1.5 CPU、3 GiB，各 7 回合）
  - 反向自檢：在 b65b487 上量得到 planner（每回合 `plan` 2 步、`build` 1 步，啟動 2 次）。planner 的 session 回合後就被刪掉，所以改用 opencode.db 的 trigger 記錄步數。
  - b65b487 與 f0ce34a 對照：
    - short：成功率 7/7 對 7/7，步數 3 → 1，每回合啟動 opencode 2 → 1 次；
    - tool：成功率 7/7 對 7/7，步數 4 → 中位數 7（6～10），照修訂後的 SCN-001 只記錄不設門檻。
  - ef3f74a 時 tool 中位數 8 步，primary 派工後反覆讀檔；依使用者 2026-11-27 的決定加上「派工後就結束回覆」並修訂 SCN-001（f0ce34a）。
  - 映像照 release workflow 的方式建（`scripts/build-image.sh`）。這個腳本遇過一個陷阱：兩個提交共用 cargo target 時，`git archive` 的檔案時間比上一次的產物舊，cargo 會不重編；解壓後改成現在的時間來修正。

### Task 6.3 — 隔離環境的端到端演練
- 產出與範圍：`rehearsal.md`。在獨立的 Docker daemon（#44 的做法）裡，從 v0.25.1 之後的 V2 版本升級到本分支，本機按需與 server 模式各跑一輪。
  - 涵蓋：一般回答、派工給 fixer、追問、技能、排程建立、人格、Telegram（mock transport 或可取得時的真實 bot）與 Web 的顯示。
  - 也涵蓋工作區是舊範本 AGENTS.md 的遷移。
- 相依：6.1、6.2
- 驗收編號：SCN-001、SCN-002、SCN-004～SCN-007、SCN-009（端到端支援）
- 完成判準：每一項都有實際回合的紀錄；沒跑到的項目照實寫出原因。
- 驗證：垂直切片。最需要驗證的是升級後各入口的整合；單元測試與探測都不涵蓋部署之後的組合。
- 狀態：✅ 已完成（2026-11-27）
- 證據：[rehearsal.md](rehearsal.md)
  - 環境：dind 裡從假發版 rc.44（b65b487）以本分支的 installer 升級到 rc.45（f0ce34a），再到 rc.46（e70e4e2）；模型 NVIDIA nemotron，只用合成提示詞。
  - SCN-009：舊範本 AGENTS.md 先備份成 `AGENTS.md.bak-65511d2` 再換新，`.opencode/agents` 6 份定義、`.opencode/skills` 8 個技能；兩種模式問「你是誰」都以孫悟空回答。全新部署與改過的 AGENTS.md 沿用 Task 2.3 的證據。
  - SCN-004：rc.44 建立的 session 在升級後、切到 server 模式後都答對暗號，prompt 沒有補上一回合。
  - SCN-002：兩種模式的建檔都派給 fixer，SSE 有 `role: fixer` 與 fixer 的 step；排程執行中的訊息是「進行中：悟空」。
  - SCN-007：本機模式以對話建立的排程擁有者正確。server 模式在 rc.45 建進 opencode-server 自己的資料庫、永遠不執行（v0.25.1 起的既有問題）；使用者 2026-11-27 選在 #45 修，e70e4e2 讓 opencode-server 共用記憶庫，rc.46 上以對話建立的排程由 schedulerd 執行成功。
  - SCN-005：server 模式的除錯任務中，primary 以 `skill` 工具載入 systematic-debugging 與 test-driven-development，Web 的 SSE 有 `tool: skill`。本機模式 1 次沒載入（題目明講 TDD，模型直接照做）、1 次被 429 打斷。
  - SCN-006：升級後所有回合的 user message 都沒有角色卡與固定區塊，`[相關記憶]`、`[無人值守排程]`、`[其他回合正在…]` 照常附加。
  - 未執行：Telegram（部署沒有 bot token，沿用 Task 5.1 的 mock transport 證據）。

### Task 6.4 — inbox 評估
- 產出與範圍：在實作計畫寫評估結論：V2 session inbox（delivery 設成 queue 或 steer）能否取代或簡化 scope lease 的協調，以及會改變哪些可觀察行為。附 1 次最小真實請求，確認 queue 的行為。
- 相依：3.1
- 驗收編號：TBD-5（不改變行為）
- 完成判準：
  - 結論寫明建議與理由，README 的 TBD-5 已更新；
  - 建議改行為時，列成新的待決事項，不在本 issue 實作。
- 驗證：等價證據，記錄探測命令與輸出。不改變可觀察行為。
- 狀態：✅ 已完成
- 證據：
  - 探測：[probe-inbox.md](probe-inbox.md)（2026-11-26，V2 2.3.2，NVIDIA nemotron，`scripts/probe-inbox.sh`）。
    - prompt 不帶 delivery 時是 `steer`，Wukong 現在送的就是這種。
    - 執行中送出 `delivery: "queue"` 的 prompt：等目前那一步寫完才送進去，接在同一次執行裡回答，不另開一次執行。
  - 結論：**不用 inbox 取代或簡化 scope lease，維持「同一個 scope 已有回合時立即拒絕」（SCN-012）。** 理由：
    - 回合邊界：Wukong 以「primary 執行結束且 inbox 清空」判斷回合結束（Task 3.1）。queue 讓兩個回合併進同一次執行，第一個回合會等到第二則也答完才結束，最後的文字變成第二則的回答；第二個回合看不到自己的執行開始與結束。要用 inbox，回合結束判定與輸出歸屬都得重做。
    - lease 保護的不只 LLM 呼叫：
      - 回合開始時的 session 輪替與舊 session 的刪除；
      - session 指令層的寫入、召回注入；
      - 回合後的記憶寫入；
      - 本機按需模式每回合各啟動一個 `opencode serve`，兩個回合會同時開兩個 server 寫同一份 opencode.db。
      這些都在 V2 之外，inbox 管不到。
    - 被拒絕前不花 LLM 呼叫（SCN-012）是現有行為。改成排隊屬於可觀察行為的改變：
      - 使用者看到的從「立即拒絕」變成「等待後回答」；
      - 排程在忙碌的 scope 不再失敗；
      - 一次執行可能回答多則訊息。
  - 沒有建議改變行為，所以不另列待決事項。之後若想讓忙碌時的訊息排隊而不是拒絕，要先另開 issue 取得核准，並先驗證本次沒驗的部分：多步驟（有工具呼叫）的執行裡，queue 項目在下一步之前就送進去，還是等整次執行結束。

### Task 6.5 — 常青文件
- 產出與範圍：
  - 根目錄 AGENTS.md：架構、一回合資料流、orchestrator 的職責、關鍵設計細節。
  - 根目錄 README.md：一回合的流程圖、接力與空輸出回退等功能說明（3.1 只先改了不能執行的 `wukong-orchestrate` 指令與 crate 說明）。
  - `docs/docker.md`：agent 定義、技能路徑、AGENTS.md 遷移、移除的變數。
  - `docs/entrypoints.md`：角色顯示、偏好移除。
  - `CHANGELOG.md` 的 `[Unreleased]`。
- 相依：6.3
- 驗收編號：全部（文件）
- 完成判準：依 project.md 常青文件表逐項核對，文件內容與產物一致（W-1）。
- 驗證：等價證據，以靜態核對逐項記錄。不改變可觀察行為。
- 狀態：✅ 已完成（2026-11-27）
- 證據：依 `docs/agents/project.md`「常青文件更新責任」逐項靜態核對，內容對照現行程式（`turn.rs` 的 `run_turn`、`bootstrap.rs`、`agent_assets.rs`、`skill_assets.rs`、`workspace_template.rs`、各 crate 的 `Cargo.toml`）：
  - `run_turn` 資料流與相依方向 → 根目錄 `AGENTS.md`：
    - 相依圖照 `Cargo.toml` 改寫（#45 拿掉了 `wukong-orchestrator → wukong-gateway`；`wukong-cli`、`wukong-runtime` 那兩行在 #45 之前就與實際不符，一併修正）；
    - 「一回合資料流」改成 lease → recall → primary agent 一次 → 空輸出補救 → remember，拿掉 planner、逐棒與會話隔離；
    - 新增「Wukong 管理的 agent 定義與技能」；「末棒輸出保證」改成「輸出保證」，描述補送修復提示；同時執行與 lease 續租兩則拿掉「每一棒」「規劃階段」，排程階段補上「悟空·fixer」。
  - compose 與容器 → `docs/docker.md`：新增「工作區裡由 Wukong 管理的檔案」（agent 定義、技能路徑、AGENTS.md 遷移、primary 的權限、`.wukong/skills` 舊路徑、移除的變數）；server 模式段落補上 opencode-server 共用 `/data`；保留期清理與模型兩句拿掉「輔助棒」「最後一棒」。`.env.example` 沒有新增 `WUKONG_*` 變數（`WUKONG_SESSION_COMPACT_EVERY_TURNS` 已在 Task 3.3 註明不再讀取）。
  - Telegram／Web → `docs/entrypoints.md`：狀態泡泡的角色顯示、Web 的 `role`／`step` 與產出卡片、技能偏好頁移除。`docs/web-console-sop-v1.md` 已在 Task 5.2 更新。
  - CLI → `docs/cli-reference.md`：`schedule add-turn` 不再寫 planner；排程模型的套用範圍改成回合、subagent 與補救重送。
  - 記憶 → `docs/memory.md`：召回那句拿掉「最後一棒」「輔助棒」。
  - 功能總覽 → `README.md`：四柱表與架構圖、相依方向、時序圖改成一次呼叫加選擇性派工、「派工與 session」取代「角色協作鏈與會話隔離」、技能段落與 Roadmap。
  - 使用者看得到的變更 → `CHANGELOG.md` `[Unreleased]`：Changed 6 則、Removed 1 則（技能偏好頁）、Fixed 1 則（server 模式的排程）。
  - 安裝與發版：installer 與發版流程沒有變（AGENTS.md 遷移在入口啟動時做），`docs/installation.md`、`docs/release-rehearsal.md` 不用改。
  - crate README：`wukong-orchestrator` 改寫成只剩角色定義；`wukong-cli`、`wukong-telegram` 拿掉協作鏈的描述。`Role::description`、`Role::card` 的 doc comment 改成 subagent 定義（只動註解）。
  - 檢查：以 `planner`、`末棒`、`一棒`、`輔助棒`、`逐棒`、`前序協作`、`技能偏好`、`.wukong/skills` 搜尋常青文件與 crate README，剩下的都是說明「已移除」的句子；`docs/issues/`、`docs/superpowers/`、handover 是歷史紀錄，不改。

### Task 6.6 — 審查建議修正（review-65e960a）
- 產出與範圍：使用者 2026-11-27 選修的 SHOULD FIX 1、2、3、6 與 NICE TO HAVE 9、13（README 待確認事項 TBD-12）。
- 相依：6.5、review-65e960a
- 驗收編號：SCN-002（等待 subagent、排程階段）、SCN-003（原始紀錄）、SCN-009（既有部署的 AGENTS.md）、SCN-012（回合結束的判定）；修正既有行為，不新增驗收
- 完成判準：每項行為修正都有先紅燈的測試；文件與原始紀錄可在版控中查到；CI 三道全綠。
- 驗證：行為修正走紅燈到綠燈；文件以靜態核對；原始紀錄以 `dispatch_judge.py` 重判比對。
- 狀態：✅ 已完成（2026-11-27）
- 證據：
  - SHOULD FIX 1（64e95b8）：V2 2.3.2 的 `Job.Kind` 有 shell、subagent、image、codemode 四種（由 binary 內的 schema 字串確認）。`has_background_jobs` 改成只看 `kind` 為 subagent 的工作，在用戶端過濾。新測試 `background_shells_of_a_subagent_do_not_keep_the_turn_open`：改之前回合一直等到事件串流結束（`event stream ended before session became idle`），改之後 gateway 的 opencode_server 測試 79 項全過。
  - SHOULD FIX 2（0c54bf0）：`workspace/AGENTS.md` 改成主要助手派工、subagent 執行測試與指令；原版本存成 `assets/retired-agents-md/828fe85.md` 並加進 `RETIRED`，已部署的工作區照常備份再換新。新測試 `the_new_template_leaves_files_and_commands_to_subagents` 在舊範本上紅燈、新範本上綠燈。
  - SHOULD FIX 3（0c54bf0）：`OUTDATED` 加上「你目前作為孫悟空」。`a_customized_file_is_left_alone_and_its_outdated_sections_are_reported` 改成期待兩段提醒，改之前 `left: 1 right: 2` 紅燈，改之後 workspace_template 測試 6 項全過。
  - SHOULD FIX 6：各輪原始紀錄存進 [dispatch/](dispatch/)，dispatch-eval.md 新增「原始紀錄」段落；以 `dispatch_judge.py` 重判 6 輪，結果與各輪表格相同。
  - NICE TO HAVE 9（a091c4f）：排程從 `SubagentReported` 事件得知回報，階段顯示最近派出、還沒回報的 subagent，都回報後回到「悟空」。新測試 `the_phase_returns_to_wukong_after_the_subagent_reports` 改之前訊息是「悟空·fixer 失敗」，改之後 scheduler 測試 58 項全過。
  - NICE TO HAVE 13（文件小修）：AGENTS.md 與 docs/docker.md 註明 shell 白名單只是防呆（允許的指令加轉向仍能寫檔）；AGENTS.md 與 docs/docker.md 的入口整理時機改成 Web、Telegram、schedulerd 啟動時，CLI 在跑回合或進 REPL 前（照 `crates/wukong-cli/src/main.rs` 的 `prepare_workspace_assets` 呼叫位置）；README Timeline 的 SCN-001 重新核准列補上舊基線；CHANGELOG 補上回報後失敗的寫法。
  - CI 三道（2026-11-27，a091c4f 加上本 Task 的文件）：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked --no-fail-fast` 774 passed、0 failed、14 ignored。

### Task 6.7 — 增量覆審建議修正（review-623c436）
- 產出與範圍：使用者 2026-11-27 選修的 NICE TO HAVE 1（Telegram 泡泡回報後回到悟空）、4（README 凍結內容的兩處）；2、3 記入待確認（README TBD-13）。
- 相依：6.6、review-623c436
- 驗收編號：SCN-002（入口看得到誰在處理）；修正既有行為，不新增驗收
- 完成判準：Telegram、Web 與排程在 subagent 都回報後都回到悟空，各有先紅燈的測試；CHANGELOG「都一樣」成立；CI 三道全綠。
- 驗證：入口行為走紅燈到綠燈；共用型別的抽出以排程前後同一組測試證明行為不變；文件以靜態核對。
- 狀態：✅ 已完成（2026-11-27）
- 證據：
  - 查 NICE 1 時發現 Web 的 `/chat` 進度與 Telegram 寫給 Web 的即時事件也停在「悟空·fixer」（`wukong-chat.js` 收到 `step` 不改進度），一併修正，CHANGELOG 的「都一樣」才成立。
  - 還沒回報的 subagent 由新的 `wukong_orchestrator::ActiveSubagents` 記住（`wukong_runtime` 轉出），排程、Telegram、Web 共用，不各寫一份；兩項單元測試涵蓋多個 subagent 與同一角色派出多次。
  - 排程的 `Phase::Turn` 改用它：scheduler lib 測試改前改後都是 58 項全過（含 `the_phase_returns_to_wukong_after_the_subagent_reports`）。
  - Telegram：新測試 `the_status_bubble_returns_to_wukong_after_the_subagent_reports` 改之前只有一則「🐵　悟空·fixer 思考中…」的編輯（紅燈），改之後泡泡回到「🐵　思考中…」，即時事件的 role 依序是 `fixer`、空字串；wukong-telegram 測試全過。
  - Web：新測試 `chat_returns_the_role_to_wukong_after_the_subagent_reports` 改之前 `step` 之後沒有 role（紅燈），改之後 `step` 與 `answer` 之間有空的 role。前端的進度文字抽成 `static/lib/chat-progress.mjs` 的 `roleProgressText`，`node --test` 兩項（改之前模組不存在而失敗）；`serves_static_assets_with_content_types` 補上新模組，改之前回 404。wukong-web 測試 83 項全過，`static/lib` 四份 `.test.mjs` 都 0 fail。
  - 既有問題，沒有修：`scripts/test-web-chat-scope.sh` 的 `chat-source` 檢查從 8638096 起就失敗，不在 CI 裡。
  - NICE 4：README 核准紀錄的「TBD-62026-11-27」補上分隔；「重構步驟概要」Phase 3～6 改成 ✅。
  - 文件：CHANGELOG、`docs/entrypoints.md`（Telegram 泡泡、Web 的 `role` 事件）、`crates/wukong-telegram/README.md`。
  - CI 三道（2026-11-27）：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked --no-fail-fast` 778 passed、0 failed、14 ignored。

### Task 6.8 — PR #47 第三方審查修正
- 產出與範圍：第三方審查（[PR #47 留言](https://github.com/raybird/Wukong/pull/47#issuecomment-7214581086)，Reviewed HEAD cf47307，CONDITIONAL PASS）的 P2-1、P2-2，使用者 2026-11-27 決定（README TBD-15）；P3 即 TBD-12 沒選修的第 11 項，已是已知差距。
- 相依：review-4ccfcec
- 驗收編號：SCN-002（派工後等 subagent 回報）；取消與失敗時中斷 session 是 #44 起的既有保障，不在 Gherkin 裡；修正既有行為，不新增驗收
- 完成判準：查背景工作失敗時重試，重試都失敗才以目前的回覆收尾；回合取消或失敗時，串流中沒看到建立事件的 subagent 子 session 也被中斷；各有先紅燈的測試；CI 三道全綠。
- 驗證：gateway 以假 V2 server 走紅燈到綠燈；V2 的中斷語意以 2.3.2 原始碼查證。
- 狀態：✅ 已完成（2026-11-27）
- 證據：
  - V2 中斷語意（在 `wukong-issue44:v2-local` 映像裡對 `/opt/wukong-release/node_modules/@opencode/cli/bin/opencode.exe` 以 `grep -a` 查 2.3.2 的打包原始碼）：
    - `Session.interrupt` 只呼叫 session 執行層的 `interrupt(sessionID)`，沒有處理子 session 或背景工作；
    - 取消背景工作的 `Job.cancelSession` 全檔只有一處呼叫，在 `Session.remove` 裡（`interrupt` → `awaitIdle` → `cancelSession` → 刪除子 session）；
    - 背景 subagent 由 `SubagentJob` 以 `resume(childSessionID)` 執行，只有工作本身被取消（`isCancelled`）時才中斷子 session。
    - 結論：中斷父 session 不會停掉它的背景 subagent，P2-2 的風險成立。
  - P2-1：`has_background_jobs` 查詢失敗時隔 500ms 重試，最多查 3 次；都失敗才照舊收尾並記 `background_jobs_lookup_failed … attempt=n/3`。新測試：
    - `a_failed_job_lookup_is_retried_before_the_turn_ends`：失敗兩次、第三次看到 fixer。改之前回合以「已派給 fixer」收尾（紅燈），改之後等到 fixer 回報，查詢 4 次。
    - `a_job_lookup_that_keeps_failing_ends_the_turn_with_the_current_reply`：三次都失敗時回覆 DONE、查詢 3 次（改之前只查 1 次，紅燈）。
    - 兩項重試測試各跑約 1 秒，會與把回合逾時暫設為 1 秒的測試互相干擾；改為拿同一把 `AGENT_TIMEOUT_ENV_LOCK`，之後整組連跑三次都通過。
  - P2-2：`InterruptOnDrop` 改成記 session ID 與連線資訊（`ServerHandle`）。取消或失敗時：
    1. 先中斷回合的 session 與串流中看到的子 session；
    2. 再查 `GET /api/experimental/job?descendants=true`，補中斷清單裡 subagent 工作的 `childSessionID`。
    - 先停 primary，它才不會在查完清單後又派出新的工作。
    - 新測試 `a_cancelled_turn_interrupts_subagents_the_stream_never_announced`：拿掉子 session 的 `session.created`，清單同時有背景 shell 與 fixer。改之前沒中斷 fixer（紅燈）；改之後中斷 fixer、不碰 shell，而且 primary 的中斷先於清單查詢。
    - 既有的 `a_failed_turn_interrupts_the_subagent_sessions_too`、`a_cancelled_turn_interrupts_the_subagent_sessions_too` 照常通過。
  - 精煉：
    - 清單裡挑 subagent 工作的邏輯抽成 `subagent_jobs`，`has_background_jobs` 與取消共用；
    - basic auth 抽成 `authorize`，backend 與 `ServerHandle` 共用。
  - 文件：根目錄 `AGENTS.md`（server backend 的等待與取消規則）、CHANGELOG。
  - CI 三道（2026-11-27）：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked --no-fail-fast` 781 passed、0 failed、14 ignored。

### Task 6.9 — 增量覆審建議修正（review-364cebf）
- 產出與範圍：使用者 2026-11-27 選修的 NICE TO HAVE 1（每次查背景工作有自己的短逾時）、2（取消時去重有測試）、3（清單查詢共用一份）、5（文件措辭兩處）；4（取消之後 primary 可能被回報喚醒再跑一次）記為已知差距（README TBD-16）。
- 相依：6.8、review-364cebf
- 驗收編號：SCN-002；修正既有行為，不新增驗收
- 完成判準：卡住的清單查詢在單次逾時後放棄，回合不必等到回合逾時；拿掉去重有測試會紅燈；等待與取消只有一份清單查詢；CI 三道全綠。
- 驗證：gateway 以假 V2 server 走紅燈到綠燈；去重以變異測試證明測試有效；文件以靜態核對。
- 狀態：✅ 已完成（2026-11-27）
- 證據：
  - NICE 1：
    - 每次查詢最多等 `JOBS_LOOKUP_TIMEOUT`（10 秒），以 reqwest 的單次請求 `timeout` 實作；backend 的 `jobs_lookup_timeout` 只有測試會改。
    - 假 server 的工作清單腳本多了 `"hang"`：連線開著、一直不回應。
    - 新測試 `a_job_lookup_that_hangs_times_out_on_its_own`：三次都卡住時，回合回 DONE、查詢 3 次。改之前沿用回合逾時，10 秒內等不完（紅燈）。
  - NICE 2：`a_cancelled_turn_interrupts_the_subagent_sessions_too` 讓清單也列出串流看過的 fixer，並斷言 fixer 只被中斷一次。
    - 去重本來就有，所以改了測試就直接通過；把去重判斷改成永遠成立的變異會讓它紅燈，表示這個測試守得住。
  - NICE 3：清單查詢收進 `ServerHandle::background_jobs`，等待（`has_background_jobs`）與取消共用。取消路徑因此也把非 2xx 與不是 JSON 的回應當成失敗、寫 `turn_cancelled_jobs_lookup_failed`，不再靜默當成空清單。
  - NICE 5：Task 6.8 的「`Job.cancelSession` 全檔只出現一次」改成「只有一處呼叫」；Task 6.8 的驗收編號拿掉 SCN-012（它的 Gherkin 沒寫取消中斷）。
  - CI 三道（2026-11-27）：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked --no-fail-fast` 782 passed、0 failed、14 ignored；gateway lib 連跑三次都是 194 passed。

### Task 6.10 — 增量覆審建議修正（review-794daea）
- 產出與範圍：使用者 2026-11-27 選修的 SHOULD FIX 1（清單查詢失敗的原因要留在 log 裡）、併入它的 NICE 1（讀回應的程式與 `send_json` 共用）、NICE 2（TBD-15 影響欄）；NICE 3 記為已知差距（README TBD-17）。
- 相依：6.9、review-794daea
- 驗收編號：SCN-002；修正既有行為，不新增驗收
- 完成判準：清單查詢失敗的錯誤帶出 source 鏈裡的原因（例如逾時）；讀回應、檢查狀態、解析 JSON 只有一份；CI 三道全綠。
- 驗證：gateway 走紅燈到綠燈；`send_json` 的七個呼叫者以 gateway 全部測試前後都通過證明行為不變；文件以靜態核對。
- 狀態：✅ 已完成（2026-11-27）
- 證據：
  - 讀回應那段抽成模組層級的 `request_json`（送出、讀 body、非 2xx 與不是 JSON 都算失敗，錯誤經 `http_error` 展開 source 鏈）：
    - `send_json` 改成授權後呼叫它；
    - `ServerHandle::background_jobs` 改回傳 `GatewayError`，加上單次逾時後呼叫它。
  - 新測試 `a_failed_job_lookup_says_why`：清單查詢卡住、100ms 逾時。
    - 改之前錯誤只有 `error sending request for url (…)`（紅燈）；
    - 改之後含 `background_jobs` 與 `timed out`。
  - NICE 2：README TBD-15 的影響欄改成「SCN-002 與 #44 起取消時中斷 session 的既有保障」，與 Task 6.8 一致。
  - gateway lib 連跑三次都是 195 passed。
  - CI 三道（2026-11-27）：`cargo fmt --all -- --check` 通過；`cargo clippy --all-targets --locked -- -D warnings` 通過；`cargo test --workspace --locked --no-fail-fast` 783 passed、0 failed、14 ignored。

## 測試策略

- **真實模型**：派工、技能遵循、追問、人格，unit test 證明不了，以接真實 V2 加真實模型（TBD-6）的整合測試或演練腳本驗證（`#[ignore]`，與 #44 的 `managed_actual` 相同的做法）。同一題重複執行，記錄每次的結果。
- **事件與 adapter**：
  - 用 Phase 1 擷取的 V2 真實事件存成 fixture，測 subagent 資訊的對應與顯示。
  - 假 agent 改輸出單一 primary agent 的回覆，不再輸出 planner 的格式。
- **既有保障**：空輸出補救、上游錯誤分類、lease、輪替與保留期的測試保留，或改寫成等價的測試。行為不變的部分以改寫前後同一組斷言全綠為證據。
- **部署**：entrypoint 與 installer 的範本遷移跑 `bash scripts/test-docker-runtime.sh` 與對應的 installer 測試（project.md）。
- **CI 三道**：`cargo fmt --all -- --check`、`cargo clippy --all-targets --locked -- -D warnings`、`cargo test --workspace --locked`。

## 檢查清單

- [x] Phase 1 的六項探測都有紀錄，TBD-1～TBD-4 已更新（2026-11-26）
- [x] 每個 Scenario 都有責任 Task（2026-11-26 decompose，見責任對照）
- [x] 真實模型的驗證只用合成提示，憑證只有 NVIDIA 一筆或沒有，正式部署沒有被碰（2026-11-27：Task 6.1～6.3 都在隔離容器或 dind 裡執行，big-pickle 不需要憑證）
- [x] 常青文件依 project.md 全部更新（2026-11-27，Task 6.5）
