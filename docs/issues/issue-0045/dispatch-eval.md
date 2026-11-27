# 派工門檻評估（Task 6.1，SCN-003）

> ← 回到 [README](README.md)｜計畫見 [implementation-plan.md](implementation-plan.md#task-61--派工門檻)

2026-11-26～2026-11-27 以真實的 wukong 入口（常駐 server 模式）接隔離容器裡的 V2 2.3.2（映像 `wukong-issue44:v2-local`）實測。只送固定的合成提示詞，憑證只有 NVIDIA 一筆。

## 結論

- **最終定義（f0ce34a）在 NVIDIA nemotron-3.5-super-vl-120b-a12b 上通過門檻（第 7 輪，2026-11-27）**：6 題各 3 次，全部 PASS，18 個回合都成功、整輪 18 分鐘。big-pickle 額度沒恢復，依使用者指示改找回應較快的 NVIDIA 模型；使用者同意以這一輪作為 TBD-10 的補跑。
- **第 4 輪的定義在 opencode/big-pickle 上通過門檻**：6 題各 3 次，全部 PASS。
- **最終定義（f0ce34a）只多了一句「派工後就結束這次回覆、等它回報」**（Task 6.2 為了消掉 primary 的輪詢而加），派工的決定在這句話之前就做完了。在 nemotron 上重跑（第 6 輪）：探索、架構、設計、直接回答通過；實作題 3 次都被限流或逾時打斷而無效。big-pickle 的免費額度同日用完；最終定義的完整一輪由第 7 輪補上。
- **NVIDIA nemotron 沒有跑出完整的一輪。**
  - 第四輪（最終定義）的實作題被上游限流（HTTP 429）打斷；之後重跑的前兩次也是 429，就停掉了。
  - 使用者 2026-11-27 指示「nvidia 限流可以換成 opencode 的模型」，所以改用 big-pickle 完整跑一輪，沒有把不同模型的回合混在同一輪判定。
- **nemotron 上已知的差距：查找題（讀程式碼、寫 README）只有 1/3 派給 librarian。**
  - primary 自己讀完模組、寫好整份 README，再派 fixer「把以下內容寫入檔案」。
  - 文件內容其實是 primary 寫的，交出去的只有存檔，所以選了 fixer。
  - 見 README 的 TBD-9。
- **評估過程中改了 primary 的定義**，依使用者 2026-11-26 的兩次決定：
  - primary 不能改檔；
  - shell 只能跑排程與 memoria 指令；
  - 派工說明加強。
  - 設計記在 AGENTS.md「primary agent 不能改檔、只能跑排程與記憶指令」。

## 方法

腳本：

- [scripts/dispatch-eval.sh](scripts/dispatch-eval.sh)：執行 18 個回合，抽取結果；
- [scripts/dispatch_judge.py](scripts/dispatch_judge.py)：判定。

命令：

```bash
bash docs/issues/issue-0045/scripts/dispatch-eval.sh wukong-issue44:v2-local "$(mktemp -d …/p45eval.XXXX)" \
    …/p45-nvidia-auth.json <MODEL> 3
```

- **種子專案**：一個小型的 Python 待辦清單程式，放在 `sample/`。
  - 模組：`app.py`、`store.py`、`render.py`、`models.py`；
  - 另有 `tests/test_render.py` 與 `web/index.html`。
  - 每次回合前用 `rsync --delete` 把工作區重置成這份種子，保留 `.opencode` 與 `.wukong`。
  - 每次回合用新的 scope 與新的記憶庫，前一題的檔案與記憶都不影響下一題。
- **題目**（括號內為預期的派工對象）：
  - 實作（fixer）：在 `sample/store.py` 加 `count_done(todos)`，並在 `sample/tests/test_store.py` 補單元測試。
  - 探索（explorer）：掃描 `sample/` 的結構，列出每個模組 import 了哪些模組，整理依賴關係。
  - 查找（librarian）：查閱每個模組的程式碼，把用途與對外函式寫成 `sample/README.md`。SCN-003 叫它「資料查找」，但 librarian 的角色是撰寫文件與翻譯，題目照角色設計成整理文件。
  - 架構（oracle）：比較 SQLite 與「JSON 加檔案鎖」兩個方案，不改檔。
  - 設計（designer）：為 `web/index.html` 設計介面，不改檔。
  - 直接回答（不派工）：「台灣的首都是哪一個城市？只回答城市名稱。」
- **抽取**：以唯讀方式開 opencode.db。
  - 派工：`session_v2` 裡 `parent_id` 是本回合 primary session 的子 session，以及它們的 agent。
  - 模型：primary 與子 session 的 assistant 訊息。
  - primary 用過的工具：primary 的 assistant 訊息裡的 tool 項目與狀態。
- **判準**（`dispatch_judge.py`）：
  - 實作題 3/3 派給 fixer，直接回答題 3/3 不派工；
  - 其他四題至少 2/3 派給預期的 subagent；
  - 回合失敗，或用了 MODEL 以外的模型，該次算無效，該題判 FAIL。
- **判準自檢**（`python3 -I dispatch_judge.py --self-check`）：8 項全部 PASS。計畫原本寫用固定回覆的假 agent 做對照組；改成直接餵判準已知答案的紀錄，因為要驗的是判準本身，抽取的部分則由第 1 輪的真實回合驗到（3 次沒派工都被判出來）。確認判準分得出以下情況：
  - 全部正確；
  - 實作題有一次沒派工；
  - 探索題兩次派錯；探索題一次派錯（仍達 2/3，應判 PASS）；
  - 直接回答題派了工；
  - subagent 用了別的模型；
  - 回合失敗；
  - 少一次回合。

## 各輪結果

表格裡的「N/3」是派給預期對象的次數；直接回答題是不派工的次數。

| 輪 | 模型 | primary 的定義 | 實作 | 探索 | 查找 | 架構 | 設計 | 直接回答 |
|---|---|---|---|---|---|---|---|---|
| 1 | nemotron | Task 2 的定義（不限權限） | 0/3 ✗ | 1/3 ✗ | 0/3 ✗ | 3/3 | 2/3 | 3/3 |
| 2 | nemotron | `edit: deny`，派工說明加強 | 1/3 ✗ | 3/3 | 1/3 ✗ | 3/3 | 3/3 | 3/3 |
| 3 | nemotron | 再加 `execute: deny`，shell 只允許排程與 memoria | 3/3 | 3/3 | 1/3 ✗ | 3/3 | 3/3 | 3/3 |
| 4 | nemotron | 第 3 輪＋文件與 fixer 的界線寫清楚 | 無效（429） | 3/3 | 1/3 ✗ | 3/3 | 3/3 | 3/3 |
| 5 | big-pickle | 第 4 輪的定義 | **3/3** | **3/3** | **3/3** | **3/3** | **3/3** | **3/3** |
| 6 | nemotron | 第 4 輪＋「派工後就結束回覆」（f0ce34a，最終定義） | 無效（429、逾時） | 3/3 | 1/3（1 次 429） | 2/3 | 3/3 | 3/3 |
| 7 | super-vl | 同第 6 輪（以 61da5e5 建置，定義與 f0ce34a 相同） | **3/3** | **3/3** | **3/3** | **2/3** | **3/3** | **3/3** |

### 第 1 輪：primary 自己動手

- 定義是 Task 2 的版本，派工措辭沿用 Task 1.4 的抽樣（當時 big-pickle 3/3 派給 fixer）。
- 實作題 3 次都沒派工。以第 1 次為例，primary 依序呼叫了：glob、read（4 次）、edit、write、shell（4 次）。它自己改了 `store.py`、新增測試，再自己跑測試。
- 探索題 1/3、查找題 0/3，也都是 primary 自己讀檔整理。
- Task 3.2 的派工探測 6/6 派給 fixer，但那次的提示詞明寫「請派給 fixer 處理」，所以和這次不能比。

### 第 2 輪：禁止改檔後，改用 shell 寫檔

- 使用者 2026-11-26 選「禁止 primary 改檔，再加強提示」。
  - frontmatter 加 `permission: {edit: deny}`。
  - 用暫時 server 的 `GET /api/agent` 確認：V2 把它列成 `edit` 的 deny 規則；`tools: {write: false, edit: false}` 的寫法也得到同一條規則。
- 探索與設計都升到 3/3。
- 實作 1/3、查找 1/3。沒派工的那幾次，`edit`／`write` 都沒再出現，primary 改用：
  - shell 的 `cat >> sample/store.py << 'EOF'`、`cat > …/README.md << 'EOF'`；
  - `execute` 工具，用 Python 寫檔。

### 第 3 輪：shell 只留白名單

- 使用者 2026-11-26 選「shell 只留白名單、關掉 execute」。
  - V2 的權限依序比對、後面的優先，所以 shell 先拒絕全部，再允許 `wukong schedule *`（長期記憶開啟時另外允許 `memoria *`、`memoria-vector-sync*`）。
  - 內建的 `explore` agent 也是這樣寫的。用暫時 server 確認過，規則順序照寫的排。
- 實作題 3/3 派給 fixer。primary 先試過 shell，都得到 `permission.rejected`，之後就派工。
- 查找題 1/3：3 次都派了 fixer，其中 1 次另外派了 librarian。
- 同一份定義下，session 指令層的探測（`scripts/probe-session-layer.sh`，Task 4.1、4.2 的外迴圈）19/19 PASS：
  - shell 收緊後，模型仍能用 `wukong schedule add-turn` 建出排程；
  - session 輪替、`wukong.scope`、長期記憶能力的開關都正常。

### 第 4 輪：限流

- 把 librarian 的規則改成「新建文件檔也派給 librarian，不交給 fixer」，fixer 改成「文件以外的檔案與執行指令」。
- 實作題 3 次都在派出 fixer 之後，以「上游模型錯誤（上游限流）：provider.rate-limit statusCode=429」失敗。
  - 回合失敗時 Wukong 會刪掉 session，抽取時查不到派工紀錄，判準記為無效回合。
  - 只重跑實作題（`dispatch-eval.sh … 3 implement`），前兩次仍是 429，就停掉了。
- 查找題 1/3（fixer、librarian、fixer）。看 primary 的工具呼叫，3 次的經過都一樣：
  1. 自己讀完各模組，寫好整份 README；
  2. 先試 shell heredoc 寫檔，被權限擋下；
  3. 再派 fixer，交代的是「請將以下內容寫入 …/sample/README.md：…」。

### 第 5 輪：big-pickle

- 先用一則合成請求確認 big-pickle 沒被限流（回 `OK`），再以第 4 輪的定義跑完整的一輪。
- 18 回合全部成功，都只用 `opencode/big-pickle`，6 題全部 PASS：
  - 實作題 3 次都派 fixer，其中第 1 次先派 explorer 看結構；
  - 查找題 3 次都是先派 explorer 讀程式，再派 librarian 寫 README；
  - primary 沒有呼叫任何改檔工具，只有一次讀檔失敗（`read:error`）。

### 第 6 輪：最終定義（nemotron）

- 2026-11-27 Task 6.2 加上「派工後就結束這次回覆、等它回報」之後，用 nemotron 重跑（big-pickle 的免費額度用完，13:12 再確認仍是「Free usage limit reached」）。
- 實作題 3 次都無效：
  - 1 次在 500 秒內沒跑完（`opencode server stream timed out before session became idle`）；
  - 2 次被限流（429）。
- 查找題 1 次被限流，重試 10 次、超過 90 秒仍失敗；其餘 2 次是 fixer、librarian 各 1。
- 架構題 1 次 primary 自己回答（2/3，仍通過）；探索、設計、直接回答都通過。

### 第 7 輪：最終定義（super-vl）

- 2026-11-27 14:38 再確認 big-pickle 仍回「Free usage limit reached」。使用者問 NVIDIA 有沒有別的模型、有沒有回應更快的模型，所以在隔離容器裡列出 NVIDIA 的模型，逐一試跑（每個模型自成一次試跑，沒有混進同一輪判定）：
  - 直接回答題各 1 次：deepseek-v4.1-flash、gpt-oss-20b、gemma-4-31b-it 已下架（410）；glm-5.3-flash 129 秒；minimax-m3.1-flash 14 秒；nemotron-3.5-lightning、laguna-xs-2.1、step-5-preview、nemotron-3.5-super-vl 在 4 秒內。
  - minimax-m3.1-flash 跑了完整一輪的前 9 次就停掉：實作題 3 次都載入技能、派 explorer 之後提出設計並問「請確認是否依上述建議實作？」，沒有派 fixer；查找題 3 次都在 500 秒逾時。第一次嘗試另因主機暫存目錄的磁碟配額用完，18 次全部無效，不列入。
  - 四個快的模型各跑 1 次實作題：super-vl 80 秒派 fixer、函式與測試都寫了；lightning 62 秒派 fixer 但沒寫測試檔；step-5-preview 383 秒；laguna-xs-2.1 沒派工。
- 選 super-vl 跑完整一輪：wukong 以分支 61da5e5 建置，`.opencode/agents/` 的內容與 f0ce34a 相同（之後只改了 `role.rs` 的註解），評估的工作區沒有 AGENTS.md，所以這一輪驗的是 f0ce34a 的最終定義。
- 6 題全部通過，18 個回合都成功，沒有限流或逾時：
  - 查找題 3/3 派給 librarian（2 次先派 explorer 看程式），nemotron 上的 TBD-9 差距在這個模型上沒有出現；
  - 架構題 1 次 primary 自己比較兩個方案（2/3，仍通過）。
- 這是第一次在 super-vl 上評估，結果不和第 5 輪 big-pickle 直接比較；同一題的派工對象與前幾輪一致。

## 原始紀錄

2026-11-27 依 review-65e960a 的建議存進版控，每行一個回合：題目、第幾次、回合是否成功、秒數、派出的 subagent、用到的模型、primary 用過的工具與 session ID。內容只有合成題目的抽取結果，沒有提示詞與回覆內文。

| 輪 | 紀錄 |
|---|---|
| 1 | [dispatch/round-1.jsonl](dispatch/round-1.jsonl) |
| 2 | [dispatch/round-2.jsonl](dispatch/round-2.jsonl) |
| 3 | [dispatch/round-3.jsonl](dispatch/round-3.jsonl) |
| 4 | [dispatch/round-4.jsonl](dispatch/round-4.jsonl)；實作題重跑 2 次仍 429：[dispatch/round-4-implement-retry.jsonl](dispatch/round-4-implement-retry.jsonl) |
| 5 | [dispatch/round-5.jsonl](dispatch/round-5.jsonl) |
| 6 | [dispatch/round-6.jsonl](dispatch/round-6.jsonl) |
| 7 | [dispatch/round-7.jsonl](dispatch/round-7.jsonl) |

重判：`python3 -I docs/issues/issue-0045/scripts/dispatch_judge.py docs/issues/issue-0045/dispatch/round-<N>.jsonl <MODEL>`，MODEL 是 `nvidia/nvidia/nemotron-3-ultra-550b-a55b`（第 5 輪是 `opencode/big-pickle`，第 7 輪是 `nvidia/nvidia/nemotron-3.5-super-vl-120b-a12b`）。2026-11-27 重判 7 輪，結果與上方各輪表格相同。

## 其他確認

- **CLI backend**：它執行 `opencode run --dangerously-skip-permissions`，在 V2 是 `--auto` 的隱藏別名。
  - V2 binary 裡是 `auto: r.auto||r.yolo||r.dangerouslySkipPermissions`；`--auto` 的說明是「Auto-approve permissions that are not explicitly denied」。
  - 所以明確的 deny 規則在 CLI 模式仍然有效。這一點只讀了原始碼，沒有以 CLI 模式跑真實回合。
- **單元測試**（`crates/wukong-runtime/src/agent_assets.rs`）：
  - `only_the_primary_agent_is_denied_file_edits`：primary 的 frontmatter 帶權限，五個 subagent 不帶權限、但帶 `[檔案互動規則]`；
  - `the_primary_shell_only_allows_scheduling_and_memoria`：白名單的 YAML（包括指令路徑含空白與引號時的跳脫），以及 memoria 開關。
  - 兩者都先看到紅燈（斷言失敗；函式還不存在時編譯失敗），實作後通過。
