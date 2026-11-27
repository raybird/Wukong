# 探測：agent 定義的位置與指定方式（Task 1.1）

> ← 回到 [README](README.md)｜計畫見 [implementation-plan.md](implementation-plan.md#task-11--agent-定義的位置與指定方式)

2026-11-26 在隔離容器實測。

## 結論

- **Phase 2 採用的目錄**：工作區的 `.opencode/agents/`（容器裡是 `/workspace/.opencode/agents/`）。
  - 三種執行模式都會讀到：本機按需、常駐 server、CLI。
  - 全域設定目錄 `~/.config/opencode/agents/` 也會讀到，但不採用。binary 模式的 HOME 是使用者自己的家目錄，寫在那裡會改到使用者在 Wukong 以外使用的 OpenCode。
- **格式**：markdown，frontmatter 寫 `description` 與 `mode: primary` 或 `mode: subagent`，內文就是 prompt。檔名（不含 `.md`）就是 agent 的 id。
- **讀取位置**：以下三處都會載入，primary 與 subagent 都一樣。
  - 工作區的 `.opencode/agents/`；
  - 工作區的 `.opencode/agent/`（單數）；
  - 全域的 `~/.config/opencode/agents/`。
- **指定 agent**：
  - 常駐 server：`POST /api/session/{id}/agent` 帶 `{"agent": "名稱"}` 後，下一個 prompt 由該 agent 回答。
  - CLI：`opencode run --agent 名稱`。
  - 沒有指定時以內建的 `build` 執行。
- **不存在的 agent**：
  - 常駐 server：選取仍回 204，但下一個回合以 `session.execution.failed` 結束，錯誤訊息是 `Agent not found: "nosuchagent"`。V2 不會改用 build。
  - CLI：結束碼是 1。
- **執行中新增定義**：常駐 server 執行中新增定義檔，等 2 秒後送出的回合就用到它，不用重啟。V2 會監看檔案。
- **啟動時的競態**：
  - server 剛就緒時，第一個 `GET /api/agent` 看不到自訂定義。這次是空清單，先前看過只有內建 agent 的情形。幾秒後才完整。
  - 第一個 prompt 則會等定義載入完成。3 個剛啟動的 server，第一個請求就是 prompt，都沒有出現 Agent not found。
  - 所以本機按需模式（每回合起一個新的 server）不受影響。但 Wukong 不能用「剛啟動時列一次 agent 清單」來判斷定義有沒有寫好。
- **內建 agent**：
  - primary：build、plan、compaction、summary、title。
  - subagent：explore、general。
  - 內建的 explore、general 會和 Wukong 的五個 subagent 同時出現在模型可以派工的清單裡，可能搶走派工。這會影響 SCN-003，留給 Phase 2 與 Task 6.1 處理。這次沒有驗證能不能停用它們。

## 環境

- **映像**：`wukong-issue44:v2-local`（af56722acade），`opencode v2.3.2`，內容與 #44 合併的 b65b487 相同。
- **隔離**：
  - 獨立的 HOME 與工作區，都在 scratchpad 的暫存目錄。
  - auth.json 內容是 `{}`，沒有任何憑證。
  - 不需要模型的第 1、4 段加 `--network none`。
- **模型**：預設免費模型 `opencode/big-pickle`（README TBD-6）。
  - 腳本會從每個回合的 assistant 訊息讀出實際用的模型，不是指定的模型就中止。
  - 這次需要模型的回合共 6 個：server 4 個、CLI 2 個。不存在的 agent 那兩個回合在呼叫模型前就失敗。
- **提示**：只送合成提示 `Say hello in five words.`。測試用的 agent 要求回覆以固定代號開頭，用來辨認是哪個 agent 回答。

## 命令

```bash
work=$(mktemp -d "$SCRATCH/p45c.XXXX")
echo '{}' > "$SCRATCH/p45-empty-auth.json"
bash docs/issues/issue-0045/scripts/probe-agents.sh \
    wukong-issue44:v2-local "$work" "$SCRATCH/p45-empty-auth.json" opencode/big-pickle
```

- 腳本：[scripts/probe-agents.sh](scripts/probe-agents.sh)。
- 每個 server 回合由 [scripts/v2turn.py](scripts/v2turn.py) 在容器內送出，事件與訊息寫到工作目錄的 `out/`。

## 輸出

```text
== 1. opencode debug agents / config（--network none）
config sources: /p/home/.config/opencode, /p/ws/.opencode
build:primary compaction:primary explore:subagent general:subagent plan:primary summary:primary title:primary wkfixer:subagent wkglobal:subagent wkgprimary:primary wkold:subagent wkprimary:primary
PASS debug agents lists wkprimary:primary
PASS debug agents lists wkfixer:subagent
PASS debug agents lists wkold:subagent
PASS debug agents lists wkgprimary:primary
PASS debug agents lists wkglobal:subagent
== 2. 常駐 server（cwd 是工作區，與 compose 的 opencode-server 相同）
first GET /api/agent?location[directory]=/p/ws:
settled GET /api/agent?location[directory]=/p/ws: build compaction explore general plan summary title wkfixer wkglobal wkgprimary wkold wkprimary
PASS /api/agent lists project (agents/ and agent/) and global agents once loaded
PASS server: selecting the project primary agent is used (PRIMARY-OK Hello there, nice to meet. [model=opencode/big-pickle])
PASS server: selecting the global primary agent is used (GLOBAL-OK Hello there, nice to meet. [model=opencode/big-pickle])
PASS server: without selecting, the turn runs as build (Hello, how are you today? [model=opencode/big-pickle])
PASS server: an unknown agent fails the turn with 'Agent not found' (select still returns 204)
PASS server: a definition added while the server runs is used without restart (LATE-OK Hello, how are you today? [model=opencode/big-pickle])
== 3. CLI backend（opencode run --standalone --agent，cwd 是工作區）
PASS cli: --agent project primary (PRIMARY-OK Hello there, nice to meet.)
PASS cli: --agent global primary (GLOBAL-OK Hello there, how are you?)
PASS cli: --agent unknown exits non-zero (rc=1)
== 4. 本機按需模式的情形：剛啟動的 server，第一個請求就是 prompt（--network none，不需要模型）
PASS fresh server 1: the first prompt resolves the project agent (no 'Agent not found', a step started)
PASS fresh server 2: the first prompt resolves the project agent (no 'Agent not found', a step started)
PASS fresh server 3: the first prompt resolves the project agent (no 'Agent not found', a step started)
fails=0
```

這份輸出省略了 v2turn.py 每回合印的 JSON 摘要行。

## 腳本的自檢

先前的版本有兩個問題，會讓失敗的檢查顯示成通過。2026-11-26 都已修正。

- `check "…$(reply …)" $?` 裡的 `$(...)` 會把 `$?` 重設成 0，任何失敗都會顯示 PASS。現在先把結束碼存進 `rc` 再組訊息。
- `reply` 取到的是 reasoning 的文字，不是回覆本身。現在只取 assistant 訊息裡 type 為 text 的部分，並附上實際用的模型。

## 附記：模型設定

- 2026-11-26 稍早的手動探測，在容器裡用 `OPENCODE_CONFIG_CONTENT` 指定了 NVIDIA 模型。實際的回合卻用了預設的 `opencode/big-pickle`，NVIDIA 憑證本身則回 403。
- 這次用的正是預設模型，所以看不出那個設定有沒有生效。原因留給 Task 1.4 查。它和 SCN-013「subagent 沿用指定模型」在同一個問題上。
