# 實作計畫

2026-11-26 建立。本計畫是任務的權威來源，GitHub #45 的範圍清單只是建檔時的摘要。

- 風險與首要驗證依 [README](README.md#風險與首要驗證)。
- 現況見 [requirement-analysis.md](./requirement-analysis.md)，取捨見 [technical-analysis.md](./technical-analysis.md)。
- Phase 內的 Task 由 decompose 細化在本檔。

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

Phase 1 是首要驗證，先於所有實作。後續 Phase 的 Task 等 Phase 1 結論出來、Scenario 核准後，由 decompose 細化。

### Phase 1 — V2 能力的契約探測（首要驗證）

- **產出**：`probe-*.md` 探測紀錄與探測腳本，放在 `docs/issues/issue-0045/`。逐項寫出成立或不成立，附命令與輸出。
- **驗收編號**：SCN-002、SCN-005、SCN-007、SCN-009、SCN-013、SCN-014 的前提（TBD-1～TBD-4）。
- **相依**：Scenario 核准。
- **探測項目**：
  1. agent 與 subagent 的定義放在哪個目錄、什麼格式，才會被本機按需、常駐 server 與 CLI 三種模式都讀到；prompt 指定 `agent` 時是否生效。
  2. 專案技能的探索路徑與格式：是否出現在 `/api/skill`；附上 `skills` 或由模型自行載入時，事件裡看不看得到。
  3. session instruction entries：寫入後是否持續存在、不進對話紀錄、模型讀得到；輪替出來的新 session 要重寫。
  4. subagent 的模型：在 primary agent 或 prompt 指定模型時，subagent 用的是哪一個。
  5. 事件裡的 subagent 身分資訊（task 工具的參數、子 session 的事件），以及 subagent 輸出的位置。
  6. 用 NVIDIA nemotron 抽樣：實作題會不會派給 fixer。
- **完成判準**：
  - 六項都有成立或不成立的紀錄。
  - 不成立的項目已依 README 待確認事項的影響處理：回到規格修訂，或採用已決定的退路。
  - README 的 TBD-1～TBD-4 已更新狀態。

### Phase 2 — agent、subagent 與技能的定義與部署

- **產出**：
  - Wukong 管理的 agent 與 subagent 定義（含 SOUL.md 內容）與寫入邏輯；
  - 技能寫到 V2 的探索路徑；
  - AGENTS.md 範本遷移，含歷代範本的雜湊清單；
  - 新版的 `workspace/AGENTS.md` 範本。
- **驗收編號**：SCN-005、SCN-009、SCN-014。
- **相依**：Phase 1。
- **完成判準**：
  - 三種模式都讀得到定義與技能。
  - 全新部署、與範本相同、改過 AGENTS.md 三種工作區的行為符合 SCN-009，有自動化測試（entrypoint 與 installer 各一組）。

### Phase 3 — run_turn 改為單一 primary agent

- **產出**：
  - run_turn 以 agent `wukong` 執行一次。
  - 移除 planner、接力鏈與輔助棒的上一回合補償；保留輪替後的補償。
  - lease 在任何 LLM 呼叫前取得。
  - 拿掉依回合數壓縮，7 天輪替改在每回合開始時檢查。
  - orchestrator、假 agent 與受影響的測試（約 110～130 個）改寫。
- **驗收編號**：SCN-001、SCN-004、SCN-010、SCN-012、SCN-013。
- **相依**：Phase 2。
- **完成判準**：
  - `cargo test --workspace --locked` 全過。
  - 空輸出補救、上游錯誤分類、lease 立即拒絕、輪替的既有測試都保留，或改寫成等價的測試。
  - 新增的測試有紅燈紀錄。

### Phase 4 — 固定提示移到 agent 定義與 session 指令層

- **產出**：
  - 角色卡、輸出要求、排程能力、長期記憶能力、Telegram 檔案互動規則，不再附在 user message。
  - 排程能力的說明改成 skill，scope 走 session 指令層，或退路的一行。
- **驗收編號**：SCN-006、SCN-007、SCN-008。
- **相依**：Phase 3。
- **完成判準**：
  - 以接真實 V2 的整合測試查 opencode.db，user message 不含上述區塊。
  - 排程以正確的 scope 建立。
  - `WUKONG_MEMORIA_HINT` 開與關的差異成立。

### Phase 5 — 入口的角色顯示與規劃偏好移除

- **產出**：
  - Telegram、Web、CLI、排程的角色顯示改從 V2 的 subagent 資訊取得；Web 的步驟卡片改顯示 subagent 的產出。
  - 移除 `planner_preferences` 與 Web 的偏好介面、API。
- **驗收編號**：SCN-002、SCN-011。
- **相依**：Phase 3。
- **完成判準**：
  - 四個入口的顯示有測試。
  - 含舊欄位的設定檔不報錯。
  - Web 的偏好 API 已移除，前端不再呼叫它。

### Phase 6 — 真實模型驗收、效能對照與常青文件

- **產出**：
  - SCN-003 的固定任務組與結果；
  - SCN-001 的效能對照：bench-turn.sh，與 b65b487 在相同條件下比較，加上每回合 LLM step 數的取法；
  - 隔離環境的端到端演練（本機按需與 server 模式）；
  - 範圍第 7 項 inbox 的評估結論（TBD-5）；
  - 常青文件：根目錄 AGENTS.md 的架構與一回合資料流，並更正 settings 的路徑；docker.md、entrypoints.md、CHANGELOG。
- **驗收編號**：SCN-001、SCN-002、SCN-003，以及全部 Scenario 的端到端支援。
- **相依**：Phase 4、Phase 5。
- **完成判準**：
  - SCN-003 的門檻達成，結果持久化。
  - 效能對照的成功率不低於 b65b487，LLM step 數較少。
  - 常青文件依 project.md 的觸發條件全部更新。

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
- [ ] 每個 Scenario 都有責任 Task（decompose 時建立責任對照）
- [ ] 真實模型的驗證只用合成提示與 NVIDIA 憑證，正式部署沒有被碰
- [ ] 常青文件依 project.md 全部更新
