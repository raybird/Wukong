# 互動 REPL + 活動渲染設計

> v2 項目 C。為 `wukong` 加入互動 REPL（多輪對話、session 接續、記憶持續累積），並把 execute 步驟的 agent 輸出改為「活動渲染」：等待時顯示 spinner、即時顯示工具/步驟活動、文字片段一到就印。

**狀態：** 設計已與用戶拍板（2026-06-06）。下一步進 writing-plans。

---

## 背景與前提（重要）

實測結論：**opencode 目前不吐 token delta**。三層都驗過——`opencode run`（全緩衝）、`opencode run --format json`（每訊息片段一塊）、`opencode serve` + SSE `/event`（助手 text part 由 `textlen 0` 一次跳滿）。根因在上游 model/provider（deepseek-v4-flash）不 streaming，非傳輸層。

因此本設計**不追求逐字打字機**，而是用 `opencode run --format json` 的**事件**做「活動渲染」：
- 單步問答：spinner 等待 → 整段答案一次印出。
- 多步任務：文字分塊陸續出現 + 工具/步驟活動即時顯示。

底層 agent 一律以 **opencode** 為準（用戶指示，不考慮其他 agent）。

## 目標

1. **互動 REPL**：`wukong`（無 prompt）進入多輪對話迴圈，session 接續，記憶在同一個已開的 `Memory`（含 embedder）持續累積。
2. **活動渲染**：execute 步驟解析 opencode `--format json` 事件，spinner + 工具活動 + 文字分流呈現；單次與 REPL 皆預設開，可關。

## 範圍

- `wukong-gateway`：新增 `stream.rs`（`StreamEvent`、`parse_event`）；`backend.rs` 加 `AiBackend::run_streaming`。
- `wukong-cli`：新增 `render.rs`（`StreamRenderer`）、`repl.rs`（REPL 迴圈）；改 `lib.rs`（execute 走 `run_streaming`）、`main.rs`（無 prompt→REPL）。
- `wukong-gateway::config`：`GatewayConfig` 加 `stream: bool`。

非範圍：token-level 串流、opencode server/SSE 架構、Web/Telegram 進入點、多角色平行。

## 元件設計

### 1. StreamEvent 與解析（`wukong-gateway/src/stream.rs`）

```rust
/// One rendered-relevant event parsed from the agent's `--format json` stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    Text(String),    // a chunk of assistant text (opencode "text" part)
    ToolUse(String), // tool name (opencode "tool_use")
    StepStart,       // step begins (drives the spinner)
    StepFinish,      // step ends
}

/// Parse one NDJSON line from `opencode run --format json` into a StreamEvent.
/// Unrecognized or malformed lines return None and are ignored by callers.
pub fn parse_event(line: &str) -> Option<StreamEvent>;
```

opencode 事件對應（依實測 schema）：`{"type":"text","text":"…"}`→`Text`、`{"type":"tool_use",…}`（取工具名）→`ToolUse`、`{"type":"step_start"}`→`StepStart`、`{"type":"step_finish"}`→`StepFinish`、其餘→`None`。

### 2. AiBackend::run_streaming（`wukong-gateway/src/backend.rs`）

在既有 trait 加一個帶預設實作的方法（不破壞既有實作者）：

```rust
#[allow(async_fn_in_trait)]
pub trait AiBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError>;

    /// Run, invoking `on_event` as events arrive, and return the full response.
    /// Default: call `run`, emit the whole text as a single Text event.
    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        let resp = self.run(req).await?;
        on_event(StreamEvent::Text(resp.text.clone()));
        Ok(resp)
    }
}
```

`AgentCliBackend::run_streaming` 覆寫：在 argv 末段（prompt 之前）插入 `--format json`，以 `tokio::process::Command` spawn、`stdout` 設 piped、逐行讀（`BufReader::lines`），每行 `parse_event` → 命中就 `on_event(...)`，並把 `Text(s)` 累積進緩衝；行讀完後檢查退出碼，非零回 `AgentFailed{code,stderr}`，成功回 `AgentResponse{ text: 累積文字 }`。

> 注意：`--format json` 為 opencode 專屬。預設實作確保 echo/printf/mock 等非 opencode backend 仍可運作（fallback 單一 Text）。route 步驟不受影響（仍走 `run`）。

### 3. StreamRenderer（`wukong-cli/src/render.rs`）

把事件序列轉成終端輸出，**stdout/stderr 分流**以保管線相容：

- `StepStart` → 在 **stderr** 起 braille spinner（`⠋⠙⠹⠸⠼⠴⠦⠧` + 經過秒數）。
- `ToolUse(name)` → 停 spinner，**stderr** 印 `  ▸ 使用工具 {name}`。
- `Text(s)` → 停 spinner，**stdout** 印 `s`。
- `StepFinish` → 收尾（清除 spinner 殘跡）。

為可測，渲染輸出寫入注入的 `out: &mut dyn Write`（stdout 替身）與 `err: &mut dyn Write`（stderr 替身），實機分別接 `io::stdout()`/`io::stderr()`。spinner 在無 TTY（被導向檔案/管線）時退化為不畫動畫，只保留文字事件輸出。

### 4. REPL（`wukong-cli/src/repl.rs`）

```rust
pub async fn run_repl(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
) -> Result<(), WukongError>;
```

迴圈：
1. 印提示符 `悟空 › ` 到 stderr，讀一行 stdin。
2. EOF（Ctrl-D）或 `/exit`、`/quit` → 結束。
3. 空行 → 略過。
4. `/scope <x>` → 切換本 session 後續回合的記憶 scope（更新一份本地 cfg 副本的 `scope`）。
5. 其他 → 跑一回合（`run_turn`）。第 1 回合 `continue_session=false`，之後回合 `true`（帶 opencode `-c`，延續 agent 上下文）。
6. 回合出錯 → 印錯誤到 stderr，**不中斷迴圈**，回到提示符。

REPL 全程共用同一個已開的 `Memory`（含 embedder）；記憶跨回合累積。

### 5. run_turn 接線（`wukong-cli/src/lib.rs`）

`run_turn` execute 步驟：當 `cfg.stream` 為真，呼叫 `backend.run_streaming(req, &mut on_event)`，`on_event` 由 renderer 提供（文字即時印到 stdout）；為假則 `backend.run(req)`，由呼叫端（main/REPL）事後印文字。回傳值 `TurnOutput{ role, text }` 不變（text 仍為完整累積文字，供 remember 與回傳）。

為避免重複印：串流模式下文字已在事件回呼即時印出，呼叫端不再印；非串流模式由呼叫端印。以 `cfg.stream` 區分。

角色行 `🐵 悟空·{role}` 一律在 route 完成後、execute 開始前印到 stderr（確保串流模式下角色出現在答案文字之前）。串流模式由 `run_turn` 在路由後即印；非串流模式維持現狀（呼叫端印）—— 統一改由 `run_turn` 於串流模式負責印角色，避免順序錯亂。

### 6. main 與 config（`wukong-cli/src/main.rs`、`wukong-gateway/src/config.rs`）

- `GatewayConfig` 加 `stream: bool`；`resolve`：預設 `true`，`--no-stream` 旗標或 `WUKONG_STREAM=0` 設為 `false`。
- `main`：解析 CLI → 開 Memory（+ 選用 embedder，沿用 v0.2.0）→ 建 backend。若 `cli.prompt_text()` 為空 → `run_repl(...)`；否則跑單次回合（沿用現有流程，但 execute 依 `cfg.stream` 決定是否串流渲染）。

## 資料流（REPL，串流開）

```
悟空 › 幫我修這個 bug
  recall ─────────────► hits[]
  route  (run, 純文字) ─► role = Fixer        stderr: 🐵 悟空·fixer
  execute (run_streaming, opencode --format json):
       step_start ─► stderr spinner ⠹ 2.1s
       tool_use   ─► stderr  ▸ 使用工具 read
       text       ─► stdout  <答案文字>
       step_finish
  remember ───────────► User + Assistant 落盤
悟空 › _
```

## 錯誤處理

- agent 非零退出 → `AgentFailed{code,stderr}`；REPL 內印錯誤後續行，不結束迴圈。
- `--format json` 壞行 / 未知事件 → `parse_event` 回 `None`，靜默忽略。
- 無 TTY 時 spinner 不畫動畫，文字事件照常輸出（管線/重導向安全）。
- Ctrl-D 離開 REPL；`/exit`、`/quit` 同。

## 測試策略

- **gateway**
  - `parse_event`：text / tool_use / step_start / step_finish / 壞行（None）/ 未知 type（None）。
  - `AgentCliBackend::run_streaming`：以 `printf` 餵預先寫好的多行 NDJSON，斷言事件序列與累積文字；非零退出回 `AgentFailed`。
  - 預設實作 fallback：mock backend（無覆寫）→ 單一 `Text` 事件、文字等於 `run` 結果。
- **cli**
  - `StreamRenderer`：注入 `Vec<u8>` 當 out/err，餵事件序列，斷言 stdout 只含文字、stderr 含工具活動。
  - REPL：注入腳本化 stdin（`&str` 行）+ mock backend，斷言多回合執行、`/exit` 結束、空行略過、第 2 回合 `continue_session=true`、記憶跨回合累積、回合錯誤不中斷。
  - `run_turn` 串流/非串流兩路徑。
- **回歸**：`--no-stream` 與既有 run_turn/gateway 測試全綠。

## 對使用者的影響

- `wukong`（無參數）→ 進入互動 REPL（新行為；原本無參數會因缺 prompt 而報錯/無動作）。
- `wukong "問題"` → 單次回合（行為不變，但預設加上 spinner/活動渲染；`--no-stream` 可回到純文字）。
- 管線使用（`wukong "x" > out.txt`）：答案文字仍只在 stdout，活動/ spinner 在 stderr，且無 TTY 時 spinner 不畫動畫，輸出乾淨。
