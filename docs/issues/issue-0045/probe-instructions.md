# 探測：session instruction entries（Task 1.3）

> ← 回到 [README](README.md)｜計畫見 [implementation-plan.md](implementation-plan.md#task-13--session-instruction-entries)

2026-11-26 在隔離容器實測。環境與 [probe-agents.md](probe-agents.md#環境) 相同：

- 映像 `wukong-issue44:v2-local`，opencode v2.3.2；
- 沒有憑證；
- 模型是預設免費模型 `opencode/big-pickle`，共 3 個回合；
- 只送合成提示，entry 的內容也是合成的。

## 結論

- **讀寫的 API**：
  - `PUT /api/experimental/session/{id}/instructions/entries/{key}` 寫入，body 是 `{"value": 任意 JSON}`，成功時回 204。
  - 同一個 key 再 PUT 一次就是覆寫。
  - `GET …/instructions/entries` 列出 key 與 value。
  - `DELETE …/entries/{key}` 回 204，只刪那一個 key。
  - key 要符合 `^[a-z0-9][a-z0-9._-]*$`，不符合時回 400（例如 `Bad_Key`）。
- **持續存在**：
  - 存在 opencode.db 的 `instruction_entry` 與 `instruction_blob`。
  - server 重啟後仍在，模型也讀得到，所以本機按需模式（每回合起一個新 server）可以用。
  - 每個 session 各自一份，新 session 是空的。
- **模型讀得到**：
  - 寫入後的下一個回合，模型直接引用了 entry 的內容。
  - 只寫在 entry 裡、重啟前從沒問過的值，重啟後第一次問，模型也答得出來。
  - 回合開始時的 `session.instructions.updated` 事件，delta 裡會列出 `api/{key}`（只有雜湊）。和它並列的是 V2 內建的 `core/codemode`、`core/skill-guidance`、`core/date`、`core/environment`。
- **對話紀錄**：要分兩種情況。
  - 第一次寫入、之後不再改的 entry，不會變成 `/message` 或 `session_message` 裡的任何訊息。它和 V2 內建的指令一樣附在模型的指令層。
  - entry 已經被模型看過之後才改值，V2 會在下一步插入一則 `system` 訊息，這則訊息會留在對話紀錄裡：
    - `metadata.notice` 是 `instructions`；
    - 內文是「The context under "wukong.scope" changed and supersedes the previous value」加上新值。
  - OpenAPI 對 DELETE 的說明是「removal is announced to the model at the next step boundary」，應該也會插一則同樣的訊息。這次沒有再跑回合確認。
  - 模型的 reasoning 有時會引用它看到的指令內容，這是模型自己的輸出，存在 assistant 訊息裡。這個情況和 entry 的機制無關。
- **對 SCN-007 的意思**：TBD-2 成立，採用 2026-11-26 Q4 的做法，不需要退路。
  - Wukong 在建立 session 時寫入 scope，例如 key `wukong.scope`。
  - 一個 scope 對應一個 session，寫入後不再改，所以不會在對話紀錄留下通知。
  - 輪替出新 session 時重寫一次。
- **對 SCN-006、SCN-008 的意思**：
  - 不隨回合變的固定區塊放進 agent 定義：角色卡、輸出要求、檔案互動規則，以及排程能力的固定說明。
  - 只有隨 session 不同的內容用 entry：scope。
  - 長期記憶能力只在 WUKONG_MEMORIA_HINT 開啟時提供，可以有兩種做法：
    - 建立 session 時寫成 entry；
    - 做成技能，依開關寫入或移除。
  - 用 entry 時，對已經用過的 session 切換開關，下一個回合會在對話紀錄留下一次通知。
  - 用技能時會不會留下通知，這次沒有測。
  - Phase 4 決定用哪一種。

## 命令

```bash
work=$(mktemp -d "$SCRATCH/p45n.XXXX")
bash docs/issues/issue-0045/scripts/probe-instructions.sh \
    wukong-issue44:v2-local "$work" "$SCRATCH/p45-empty-auth.json" opencode/big-pickle
```

- 腳本：[scripts/probe-instructions.sh](scripts/probe-instructions.sh)。
- 判斷 entry 有沒有進對話紀錄，用的是整句內容（例如 `The session codename is QX-91.`），模型的回覆只會有代號。
- 資料庫以唯讀方式開啟，用系統的 `/usr/bin/python3` 讀。

## 輸出

```text
== 1. 寫入與讀取
session ses_de9784158ffebpROthcXjLzIIp
PASS PUT a new entry returns 204 (204 )
PASS PUT a second key returns 204 (204 )
  200 {"data":[{"key":"wukong.color","value":"The session color is teal-47."},{"key":"wukong.scope","value":"The session codename is QX-91."}]}
PASS GET lists both entries with their values
PASS a key outside [a-z0-9._-] is rejected with 400 (400)
== 2. 模型讀得到，內容不進對話紀錄
PASS the model answers from the entry (QX-91 [model=opencode/big-pickle])
  entry text in /message: assistant.reasoning
PASS the entry is not a message in /message (the model's reasoning may quote it)
PASS the turn announces session.instructions.updated
  session.instructions.updated delta keys: ['api/wukong.color', 'api/wukong.scope', 'core/codemode', 'core/date', 'core/environment', 'core/skill-guidance']
== 3. 覆寫
PASS PUT on the same key replaces the entry (204 )
PASS the next turn sees the new value (ZB-22 [model=opencode/big-pickle])
  new value in /message: assistant.reasoning system(instructions)
PASS changing an entry that was already announced adds a system notice to the history
== 4. server 重啟（本機按需模式每回合起新 server）
PASS after restart both entries are still listed
PASS after restart the model answers from an entry it was never asked about (teal-47 [model=opencode/big-pickle])
  unchanged entry in /message: assistant.reasoning
PASS an entry that never changes leaves no message in the history
== 5. 每個 session 各自一份；刪除
PASS a new session has no entries (200 {"data":[]})
PASS DELETE returns 204 (204 )
PASS after DELETE only the other key is left (200 {"data":[{"key":"wukong.color","value":"The session color is teal-47."}]})
== 6. opencode.db（唯讀）
  'The session codename is QX-91.' in: instruction_blob=1 session_message:assistant
PASS the value is stored in instruction_blob
  'The session codename is ZB-22.' in: instruction_blob=1 session_message:system session_message:assistant
PASS the value is stored in instruction_blob
  'The session color is teal-47.' in: instruction_blob=1 instruction_entry=1 session_message:assistant
PASS the value is stored in instruction_blob
PASS the unchanged entry is in no user/system message, part or inbox row (instruction_blob=1 instruction_entry=1 session_message:assistant)
fails=0
```

- 這份輸出省略了 v2turn.py 每回合印的 JSON 摘要行。
- `session_message:assistant` 是模型 reasoning 引用的內容，見結論。

## 腳本的自檢

第一次執行時有兩個問題。2026-11-26 都已修正，修正後重跑才是上面的輸出。

- 資料庫檢查用的是主機 pyenv 的 python，它沒有 sqlite3 模組。查詢全部失敗，檢查卻顯示 PASS。現在改用系統的 python3，查詢失敗時檢查也會失敗。
- 原本判斷「/message 不含 entry 內容」時，把 assistant 的 reasoning 也算進去，而 reasoning 引用了內容，所以判為 FAIL。現在分開標出位置，只有 user、system 訊息或 assistant 的回覆文字含有它才算。也是這時才看到覆寫會插入 system 通知。
