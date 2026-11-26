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
- 驗證：雙迴圈。外迴圈是真實 V2 加真實模型；內迴圈是 gateway 寫入 entries 的 adapter 測試（假 V2 server）。
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
  - 用真實模型跑一題實作任務，Web 與 Telegram（mock transport）看得到「悟空·fixer」與產出。
- 驗證：雙迴圈。外迴圈是入口測試加真實回合；內迴圈是事件對應的單元測試。
- 狀態：📝 待實作
- 證據：

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
- 狀態：📝 待實作
- 證據：

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
- 狀態：📝 待實作
- 證據：

### Task 6.2 — 效能對照
- 產出與範圍：
  - bench-turn.sh 補上每回合 LLM step 數與 opencode 啟動次數（從 opencode.db 與程序紀錄取得）。
  - 在相同條件下（1.5 CPU、3 GiB、同一個真實模型、全新合成狀態）比較 b65b487 的映像與本分支的映像，short 與 tool 各至少 7 回合。
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
  - 根目錄 README.md：一回合的流程圖、接力與空輸出回退等功能說明（3.1 只先改了不能執行的 `wukong-orchestrate` 指令與 crate 說明）。
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
- [ ] 真實模型的驗證只用合成提示，憑證只有 NVIDIA 一筆或沒有，正式部署沒有被碰
- [ ] 常青文件依 project.md 全部更新
