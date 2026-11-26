# 探測：subagent 的模型、事件與派工抽樣（Task 1.4）

> ← 回到 [README](README.md)｜計畫見 [implementation-plan.md](implementation-plan.md#task-14--subagent-的模型事件與派工抽樣)

2026-11-26 在隔離容器實測。環境與 [probe-agents.md](probe-agents.md#環境) 相同：

- 映像 `wukong-issue44:v2-local`，opencode v2.3.2；
- 沒有憑證；
- 只送合成提示。

這次用了三個免費模型，彼此不同，才分得出各回合沿用的是哪一個（README TBD-6）：

- 設定檔指定 `opencode/glm-5.3-flash-free`；
- session 選取 `opencode/deepseek-v4.1-flash-free`；
- V2 預設 `opencode/big-pickle`。

工作區放了 Phase 2 要寫的定義的雛形：primary agent `wukong`，以及五個 subagent（explorer、oracle、librarian、fixer、designer），描述沿用 `crates/wukong-orchestrator/src/role.rs` 的角色卡。

## 結論

### subagent 一律在背景執行：影響 Phase 3 的回合結束判定

這是這次最重要的發現，計畫原本沒有預期到。

- **V2 的說明**：V2 派工用的工具名稱是 `subagent`。V2 內建的工具說明寫著：
  > Subagents always run in the background: the call returns immediately with a sessionID, and you receive a `<subagent>` message each time the child goes idle
- **實測的經過**：
  1. primary 呼叫 `subagent` 後，工具立刻回成功。
  2. primary 通常以「已派工給 fixer，等它回報」結束這次執行，送出 `session.execution.succeeded`。這時 fixer 還在工作。
  3. fixer 停下來時，V2 把它的回報以 `synthetic` 項目放進 primary 的 inbox，喚醒 primary 再執行一次。
  4. 第二次執行才產生最終回覆。
  - 4 次派工全部是這樣，primary 都執行了 2 次。
- **對 Wukong 的影響**：
  - 常駐 server adapter 現在以第一個 `session.execution.*` 當作回合結束（`crates/wukong-gateway/src/opencode_server.rs`）。照現在的做法，派工的回合只會拿到「已派工」那句話。
  - 本機按需模式在回合結束後就關掉 server（`crates/wukong-gateway/src/local_process.rs`），會把還在工作的 subagent 一起殺掉。
  - 2026-11-26 第一次執行探測時，客戶端也是在第一次結束就停止。那個回合的 fixer 一直到下一個回合期間才做完，primary 也是在那時被喚醒。
- **回合要怎麼判定結束**：
  - primary 的執行結束時，查 `GET /api/experimental/job?sessionID={id}&descendants=true`。這個端點列出「還在執行或回報還沒被讀取」的背景工作。
  - 有工作時繼續讀事件，等到 primary 下一次執行結束、清單是空的，才算回合結束。
  - [scripts/v2turn.py](scripts/v2turn.py) 的 `--wait-jobs` 就是這樣做的。這次 4 次派工都正確等到最終回覆。
  - 這個做法要寫進 Phase 3 的 run_turn 與 adapter：
    - 等待要有上限；
    - 中斷時要一併處理子 session。
  - 這屬於實作方式，不改變任何 Scenario 的可觀察結果。SCN-002 要求入口看得到 fixer 的產出，本來就隱含要等它完成。
- **CLI 會等**：`opencode run --agent wukong` 會等 subagent 回報，primary 再次執行完才結束。
  - 輸出包含兩段：「已派工……等它回報後我再確認結果」和「fixer 已完成，我也覆核過」。
  - 結束碼是 0，檔案已建立。
  - CLI backend 不需要另外處理。

### subagent 用的模型（TBD-3）

- **沿用 primary 的模型**：subagent 沿用 primary session 選取的模型。
  - session 以 `POST /api/session/{id}/model` 選取 deepseek，fixer 的子 session 也跑在 deepseek 上。
  - 子 session 的 `session.created` 事件的 `model` 欄位是 deepseek，fixer 的 assistant 訊息也是。設定檔的 glm 與預設的 big-pickle 都沒有出現。
- **對 SCN-013 的意思**：Wukong 的 server adapter 已經在每個 session 選取模型（`opencode_server.rs` 的 `select_model`）。subagent 定義不必寫模型。
- **設定檔的模型沒有套用**：用 `OPENCODE_CONFIG_CONTENT` 在設定裡指定模型時，server 上的回合仍跑在預設的 big-pickle。
  - 這個情況出現在 build 與 wukong 兩個 agent。
  - `GET /api/model/default` 不帶 location 時回 glm，帶工作區的 location 時回 big-pickle。看起來這個環境變數的設定只套用到全域，沒有套用到工作區。
  - 原因沒有再追。Wukong 沒有用這個環境變數指定模型，不受影響。
  - 這也解釋了 2026-11-26 稍早的探測為什麼誤用了免費模型。之後的探測都以選取的方式指定模型。

### 事件裡的 subagent 身分（TBD-4）

| 要知道的事 | 從哪裡拿 |
|---|---|
| primary 派工了 | primary session 的 `session.tool.input.started`，`data.name` 為 `subagent` |
| 派給誰、交代什麼 | 同一個工具呼叫的 `session.tool.called`，`data.input` 為 `{"agent": "fixer", "description": …, "prompt": …}` |
| 子 session 開始 | `session.created`，`data.parentID` 是 primary 的 session，帶 `agent`（fixer）、`model`、`title`（等於 description） |
| 子 session 在做什麼 | 事件的 `data.sessionID` 是子 session 的 id，例如 `session.tool.called` 帶它呼叫的 write、shell |
| subagent 的產出 | primary session 的 `session.inbox.enqueued`，`data.item.type` 為 `synthetic`，`payload.metadata` 為 `{"source": "subagent", "childID": …, "agent": "fixer", "state": "completed"}`。`payload.text` 是包在 `subagent` 標籤裡的回報全文。primary 的訊息列表裡也有一則同樣內容的 `synthetic` 訊息 |
| 回合真正結束 | 見上一節：primary 執行結束時，背景工作清單是空的 |

- 入口要顯示「悟空·fixer」，用 `session.created` 的 `agent`，或 `subagent` 工具輸入的 `agent`。
- Web 的產出卡片，用 `synthetic` 回報的 text。
- 完整的事件串流存成 fixture，供 Task 5.1 使用：
  - [fixtures/v2-dispatch-fixer.ndjson](fixtures/v2-dispatch-fixer.ndjson)：每行一個事件，`_t` 是距送出 prompt 的秒數；
  - [fixtures/v2-dispatch-fixer.json](fixtures/v2-dispatch-fixer.json)：primary 與子 session 的狀態與訊息。
  - 內容只有合成提示與模型的回覆，沒有憑證或主機路徑。

### 派工抽樣（SCN-003 的初步訊號，不當門檻）

實作題「在工作區建立 {名稱}.txt，內容只有一行：hello from probe」。提示裡沒有指明要派工。

| 模型 | 次數 | 派給 fixer | 檔案正確 |
|---|---|---|---|
| opencode/big-pickle（V2 預設） | 3 | 3/3 | 3/3 |
| opencode/big-pickle（第一次執行，模型來自預設） | 1 | 1/1 | 1/1 |
| opencode/deepseek-v4.1-flash-free | 2 | 0/2：primary 自己用 write 工具寫檔 | 2/2 |

- 派不派工跟模型有關：big-pickle 4/4 都派工，deepseek 2 次都自己做。
- Task 6.1 的門檻要固定一個模型來量。
- 2 次 deepseek 的結果出自 2026-11-26 第一次執行探測，那時腳本還沒有等背景工作。但這 2 次沒有派工，所以不受影響。

## 命令

```bash
work=$(mktemp -d "$SCRATCH/p45v.XXXX")
bash docs/issues/issue-0045/scripts/probe-subagent.sh wukong-issue44:v2-local "$work" \
    "$SCRATCH/p45-empty-auth.json" opencode/glm-5.3-flash-free opencode/deepseek-v4.1-flash-free opencode/big-pickle 3
```

- 腳本：[scripts/probe-subagent.sh](scripts/probe-subagent.sh)。
- 回合由 [scripts/v2turn.py](scripts/v2turn.py) 送出。`--wait-jobs` 會等背景工作，結果裡的 `children` 是子 session 的狀態與訊息。

## 輸出

```text
config model: opencode/glm-5.3-flash-free ; selected: opencode/deepseek-v4.1-flash-free ; sample: opencode/big-pickle ; GET /api/model/default: opencode/big-pickle
== 1. 只靠設定檔指定模型時，回合用哪個模型
  build agent ran on: opencode/big-pickle
  wukong agent ran on: opencode/big-pickle
FAIL a session created after the server settled uses the config model for build and wukong (opencode/big-pickle / opencode/big-pickle)
== 2. 選取模型後明確派給 fixer：subagent 的模型、背景執行與回合結束
  primary tool subagent {"agent": "fixer", "description": "Create dispatch.txt probe file", "prompt": "在工作區 /p/ws 建立檔案 dispatch.txt，內容只有一行：hello from probe\n（檔案結尾可有換行，但不要有其他內容、不要加註解或標題
  primary tool shell {"command": "cat -A /p/ws/dispatch.txt; wc -c /p/ws/dispatch.txt"}
  primary session.execution.succeeded at 5.14s, background jobs then: [['subagent', 'fixer', 'running']]
  primary session.execution.succeeded at 11.7s, background jobs then: []
  child ses_de972586effeSd8dv5TyYgP2bk: fixer@opencode/deepseek-v4.1-flash-free
PASS the primary agent runs on the selected model (opencode/deepseek-v4.1-flash-free)
PASS the child session runs as fixer (fixer)
  child model: opencode/deepseek-v4.1-flash-free (selected opencode/deepseek-v4.1-flash-free, config opencode/glm-5.3-flash-free, default opencode/big-pickle)
PASS the file the subagent was asked for exists
PASS the primary runs again after the subagent reports (2 executions)
  final reply: fixer 已完成，我也驗證過了：  - 路徑：`/p/ws/dispatch.txt` - 內容：`hello from probe`（單行，結尾一個換行，共 17 bytes） - 無其他內容或多餘空行
== 3. 派工抽樣：實作題 3 次（opencode/big-pickle，不指明要派工）
  sample-1: subagents=[fixer] primary=opencode/big-pickle child=opencode/big-pickle executions=2 file_ok=yes
  sample-2: subagents=[fixer] primary=opencode/big-pickle child=opencode/big-pickle executions=2 file_ok=yes
  sample-3: subagents=[fixer] primary=opencode/big-pickle child=opencode/big-pickle executions=2 file_ok=yes
  dispatched to fixer: 3/3
== 4. CLI（opencode run --agent wukong）會不會等 subagent 做完
  rc=0 output: 已派工給 fixer（session `ses_de9716b55ffe7Afr38Atwo0Q8O`），任務是在 `/p/ws` 建立 `cli.txt`，內容僅一行 `hello from probe`。等它回報後我再確認結果。 fixer 已完成，我也覆核過：  - 檔案：`/p/ws/cli.txt` - 內容：`hello from probe`（單行，結尾一個換行，共 17 bytes
PASS after opencode run exits, the file the subagent was asked for exists
fails=1
```

- 唯一的 FAIL 是第 1 段：設定檔指定的模型沒有套用。這是記錄下來的發現，見上面「設定檔的模型沒有套用」，不是腳本的錯誤。
- 第一次執行探測的輸出（客戶端還不會等背景工作，children 也沒有依 parentID 篩選）保留在 scratchpad 的 `p45u-probe-subagent.log`，不作為證據。上面 deepseek 那 2 次的結果，是從當時的工具呼叫紀錄讀出來的。
