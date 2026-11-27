# Task 6.3：隔離環境的端到端演練

> ← 回到 [README](README.md)｜計畫見 [implementation-plan.md](implementation-plan.md#task-63--隔離環境的端到端演練)

- **日期**：2026-11-27 13:12～13:49（台北）
- **環境**：獨立的 Docker daemon（`docker:27-dind`，容器 `wk45reh-dind`）。裡面只有演練部署與一個 `registry:2`。不在主機的 Docker 上演練，理由與 #44 相同（[issue-0044/rehearsal.md](../issue-0044/rehearsal.md)「為什麼不在主機的 Docker 上演練」）。
- **版本**：v0.25.1 之後還沒有 V2 的正式版本（b65b487 沒有 tag），所以起點與終點都是假的發版。映像都用 [scripts/build-image.sh](scripts/build-image.sh) 照 release workflow 的方式在本機建，再經 dind 裡的 registry 取得真實 digest。

  | 假發版 | 提交 | 說明 |
  |---|---|---|
  | v0.26.0-rc.44 | b65b487 | #44 的合併版本，升級的起點 |
  | v0.26.0-rc.45 | f0ce34a | 本分支的最終 agent 定義 |
  | v0.26.0-rc.46 | e70e4e2 | 演練中修好 server 模式的排程之後（見下方「發現與修正」） |

- **模型**：`nvidia/nvidia/nemotron-3-ultra-550b-a55b`，寫在部署的 `user.json`；憑證只把 nvidia 一筆放進 `opencode-state` volume。
- **資料**：全新部署，只送固定的合成提示詞（暗號 ZEBRA-45 等），正式部署的資料一律不用。
- **腳本**：
  - [scripts/rehearsal-env.sh](scripts/rehearsal-env.sh)：建 dind、準備映像與發版檔案，curl／docker shim 讓 installer 讀假的發版；
  - [scripts/rehearsal-state.sh](scripts/rehearsal-state.sh)：唯讀查 `memory.db` 與 `opencode.db`，每個 scope 列出 session、子 session 的 agent、user message 裡出現的固定區塊標頭；
  - Web 回合沿用 #44 的 [rehearsal-webturn.sh](../issue-0044/scripts/rehearsal-webturn.sh)（`webturn NAME SCOPE PROMPT`，SSE 存成 `out/NAME.sse`）。
  - 原始輸出（SSE、升級 log）留在主機的 `target/issue45-reh/out/`，不進版控。

## 結果

| 驗收 | 本機按需模式（rc.45） | server 模式（rc.45／rc.46） |
|---|---|---|
| SCN-009 升級與人格 | ✓ 舊範本 AGENTS.md 先備份再換新；問「你是誰」以孫悟空回答 | ✓ 以孫悟空回答 |
| SCN-004 追問（跨升級） | ✓ 升級前建立的 session 答對 ZEBRA-45 | ✓ 同一個 session 再答對一次 |
| SCN-002 派工給 fixer | ✓ 檔案內容正確，SSE 有 `role: fixer` 與 fixer 的 step | ✓ 同左 |
| SCN-005 技能 | ✗ 兩次都沒載入技能（見下方） | ✓ primary 以 `skill` 工具載入 systematic-debugging、test-driven-development |
| SCN-007 以對話建立排程 | ✓ 擁有者是目前的 scope | rc.45 ✗（建在錯的資料庫）→ rc.46 ✓，schedulerd 實際執行成功 |
| SCN-006 user message 的固定區塊 | ✓ 升級後的回合都沒有 | ✓ 同左 |
| SCN-001 不另跑規劃 | ✓ 直接回答的回合只有 primary 一則回覆，沒有 planner | ✓ 同左 |
| Telegram | 未執行：部署沒有 bot token | 未執行：同左 |

## 經過

### 1. rc.44 的基準（13:13～13:16）

- 以 rc.44 自己的 installer 全新安裝（本機模式），三個服務都 healthy。
- 工作區的 AGENTS.md 與 b65b487 的範本（65511d2 版）逐位元組相同，sha256 開頭 `4fb7d830`，正是要驗遷移的狀態。
- Web 回合：
  - 「請記住這個暗號：ZEBRA-45。只回覆 NOTED。」→ `NOTED`；
  - 「剛剛的暗號是什麼？」→ `ZEBRA-45`。
  - SSE 有 `role: oracle`：b65b487 的 planner 替回合選了角色。
  - 這兩則 user message 開頭是 Oracle 的角色卡，後面附 `[輸出要求]`、`[排程能力]`，就是 #45 要移除的固定區塊。
- 用 CLI 建一個每 3 分鐘的排程 `reh-sched`，升級前成功執行 1 次（13:15:36，回 `SCHEDULED-OK`）。

### 2. 升級到 rc.45（13:16）

```bash
cd /reh/deploy && PATH=/reh/bin:$PATH bash /reh/install-v0.26.0-rc.45.sh --mode docker --upgrade --version v0.26.0-rc.45
```

- installer 備份 `opencode-state`，三個服務換成 rc.45，`Wukong upgrade completed: v0.26.0-rc.45`。
- **SCN-009**：
  - log：`/workspace/AGENTS.md 與舊範本 65511d2 相同，已備份到 /workspace/AGENTS.md.bak-65511d2 並換成新版`。
  - 備份的 sha256 開頭仍是 `4fb7d830`；新的 AGENTS.md 與本分支的 `workspace/AGENTS.md` 相同。
  - `.opencode/agents/` 有 6 份定義（wukong 與五個 subagent）。`wukong.md` 的 frontmatter 是 `edit: deny`、`execute: deny`、shell 先拒絕全部再允許 `wukong schedule *`（這個部署沒開長期記憶），內文以 SOUL.md 開頭。
  - `.opencode/skills/` 有 8 個 Superpowers 技能。
  - rc.44 建立的排程在升級後照常執行（13:18、13:21 都成功）。
- 全新部署與「改過的 AGENTS.md」兩種工作區，這次演練沒有另外建部署（容器名稱固定，同一個 daemon 只能有一組）。這兩種由 Task 2.3 的 `workspace_template.rs` 單元測試與真實回合涵蓋。

### 3. 本機按需模式的回合（rc.45）

| 回合 | 提示詞 | 結果 |
|---|---|---|
| l-who | 你是誰？請用一句話回答。 | 「我是齊天大聖孫悟空，亦是鬥戰勝佛，……」6 秒 |
| l-old | 升級前我給過你一個暗號，是什麼？ | `ZEBRA-45`（scope `user:reh-old`，沿用 rc.44 建立的 session）|
| l-fixer | 在工作區建立 hello.txt，內容只有一行：hello from rc45 | 派給 fixer；檔案內容正確；SSE 依序 `role: fixer`、`step`（`"role":"fixer"`）、`answer` |
| l-skill | 用測試驅動開發建立 calc.py 的 add 與 test_calc.py | 派給 fixer，先寫測試、看到失敗再實作，5 passed；但沒有任何 session 呼叫 `skill` 工具 |
| l-skill2 | calc.py 的 add(2, "3") 丟出 TypeError，請找出原因並修好 | 199 秒後被 NVIDIA 限流（429）打斷 |
| l-sched | 每天早上 8 點提醒我喝水。 | primary 以 shell 執行 `wukong schedule add-turn`，排程「每日喝水提醒」`0 8 * * *`，scope `user:reh-remind` |

- **SCN-002 的顯示**：Web 前端送出時先顯示「🐵 收到，思考中…」，收到 `role` 事件後換成「🐵 悟空·fixer 思考中…」，fixer 的 step 顯示成「🔍 悟空·fixer 的產出」卡片（`wukong-chat.js`）。rc.45 由 primary 自己回答時沒有 `role` 事件，進度停在「悟空」那一句；rc.44 的 `role: oracle` 是 planner 選的角色，planner 已移除。
- **SCN-001**：l-who 的 session 只有 1 則 user、1 則 assistant 訊息，沒有 planner 的暫時 session。每回合只啟動一次 opencode 由 Task 6.2 量測（[bench-v2-slim.md](bench-v2-slim.md)）。
- 排程 `reh-sched` 13:24 的那次被 429 打斷，之後停用，避免和演練的回合搶 NVIDIA 的額度。

### 4. 切到 server 模式（13:26）

- `.env` 加上 `COMPOSE_PROFILES=server`、`WUKONG_AGENT_SERVER_URL=http://opencode-server:4096`，密碼以 `secrets.token_urlsafe` 直接產生寫進 `.env`，沒有印出。
- `docker compose -p wukong up -d`：多出 `wukong-opencode-server`（healthy），其他三個服務重建。

| 回合 | 結果 |
|---|---|
| s-who | 「我是孫悟空（齊天大聖、鬥戰勝佛），一位全知全能的助手。」3 秒 |
| s-old | `ZEBRA-45`（同一個 rc.44 session，第 4 個回合） |
| s-fixer | 派給 fixer，server.txt 內容正確，SSE 有 `role: fixer` |
| s-sched | 「每週一早上 9 點提醒我寫週報」：模型執行 `wukong schedule add-turn`，輸出「已建立排程: bfe0028e-…」，回覆「搞定！」。**但 Web 與 schedulerd 都列不出這個排程** |

### 5. 發現與修正：server 模式以對話建立的排程不會執行

- 在 opencode-server 容器裡執行 `wukong schedule list`，才看得到 bfe0028e 這筆。
- 原因：server 模式下 agent 的 shell 在 opencode-server 容器裡跑。這個容器沒掛 `wukong-data`、沒設 `WUKONG_MEMORY_DB`，`wukong schedule add-turn` 寫進容器自己的預設資料庫。模型回「已建立」，排程卻永遠不會執行。
- v0.25.1、b65b487 的 `docker-compose.release.yml` 也一樣，是既有問題，不是 #45 造成的；本機模式的 shell 在入口容器裡跑，不受影響。
- 使用者 2026-11-27 選「在 #45 修」。e70e4e2：
  - 兩個 compose 檔的 opencode-server 掛 `wukong-data:/data`，設 `WUKONG_MEMORY_DB=sqlite:///data/memory.db`，與 schedulerd 相同；entrypoint 本來就會建立 `/data` 並改擁有者。
  - `scripts/test-docker-runtime.sh` 新增檢查：修正前 4 項 FAIL（兩個檔案各缺環境變數與掛載），修正後通過。`test-release-workflow.sh`、`test-installer-upgrade.sh all` 也通過；疊上 `docker-compose.memoria.yml` 後 `/data` 與 `/memoria` 都在。
- 建 rc.46 再升級（13:38）：installer 保留 `.env` 的 server 模式，opencode-server 的掛載多了 `/data`，環境變數有 `WUKONG_MEMORY_DB`。
- 重驗：「請建立一個每 3 分鐘執行一次的排程，名稱 srv46，……」
  - 模型建出 b4bb1057，**從 Web 容器列得出來**，scope 是 `user:reh-srv46`；
  - 13:39:58 schedulerd 執行，執行中的訊息是「進行中：悟空，已執行 0 秒」，18 秒後 `status=success`；
  - 模型沒有照指示只回 `SRV46-OK`，而是寫了一段狀態說明。這是模型遵循度，排程本身正常。驗完就停用。

### 6. 技能（SCN-005）

- 本機模式的 l-skill：題目明寫「測試驅動開發」，fixer 也照著先紅後綠，但 primary 與 fixer 都沒呼叫 `skill` 工具。技能的 description 是英文（「Use when implementing any feature or bugfix, before writing implementation code」），模型直接照題目做，沒有去載入。
- 改用符合 `systematic-debugging` description 的除錯任務：本機模式那次被 429 打斷（l-skill2）；server 模式（rc.46，s-skill，490 秒）成功：
  - primary 的工具依序是 glob、read、shell（被權限擋下）……之後呼叫 `skill`，`input.id` 為 `systematic-debugging`，接著 `test-driven-development`，兩次都 completed；
  - 再派兩次 fixer：補測試、修 calc.py；
  - Web 的 SSE 有 `event: tool`、`data: skill` 兩次，進度顯示「使用工具 skill…」。
  - step 事件的 `skill` 欄位是 null：`run_turn` 的 `ObservedStep.skill_name` 固定為 None（Task 5.1），技能使用要看 tool 事件。
- primary 在派工後仍讀了幾次檔、試了幾次 shell，也有兩次只帶 sessionID 呼叫 subagent 工具（參數不合法，回 error）。這是在查 fixer 的進度，「派工後就結束回覆」那句沒有被完全遵守；回合仍成功。
- 結論：SCN-005 的「事件中看得到技能被使用」在部署環境裡成立（server 模式 1 次）。模型會不會載入技能取決於任務是否貼近 description，「先寫測試」這種明講做法的題目反而沒有載入。

### 7. user message 的固定區塊（SCN-006、SCN-004）

以 rehearsal-state.sh 與直接讀 `session_message` 確認：

- rc.44 的回合：開頭是角色卡，附 `[輸出要求]`、`[排程能力]`（含 `--scope "user:reh-old"`）。
- 升級後的所有回合（本機與 server 模式）：
  - 沒有角色卡，也沒有 `[排程能力]`、`[長期記憶能力]`、`[輸出要求]`、`[檔案互動規則]`、`[派工]`；
  - 新 scope 的第一則 user message 只有使用者原文（例如 l-fixer 的 51 個字）；
  - 同一個 session 之後的回合附 `[相關記憶]` 與 `[使用者輸入]`，排程回合另附 `[無人值守排程]`，同時有其他回合在跑時附 `[其他回合正在…]`。這些是 SCN-006 要保留的動態區塊。
- SCN-004：升級後的追問（l-old、s-old）沒有 Wukong 補上的上一回合區塊，模型從 session 的對話紀錄答對。7 天輪替後補上一回合由 Task 3.1 的測試涵蓋（輪替後的第一回合含上一回合）。
- 附帶觀察（既有行為，沒改）：排程 session 的 `[相關記憶]` 裡，召回的 User 文字含 `[無人值守排程]` 那段提示，代表記憶存的是附加提示之後的文字。rc.44 已經如此。

## 沒有涵蓋的項目

- **Telegram**：部署沒有 bot token，`wukong-telegram` 一直是「等待設定：請在 Web /settings 填入 Telegram bot token」。Telegram 的狀態泡泡（「🐵　悟空·fixer」）與每回合只附 `[產出目錄]` 一行，由 Task 5.1 的 `telegram_names_the_dispatched_subagent`（真實 V2 加 `MockTgClient`）與 Task 4.2 的單元測試涵蓋。
- **SCN-009 的全新部署與改過的 AGENTS.md**：見步驟 2。
- **本機模式的技能載入**：兩次都沒有看到（1 次沒載入、1 次被 429 打斷）。
