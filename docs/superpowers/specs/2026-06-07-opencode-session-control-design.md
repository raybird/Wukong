# opencode Session Control 設計(預設 `--thinking`、顯式 session、`/new`、`/compact`)

**日期:** 2026-06-07
**狀態:** 已核可
**前置:** v0.8.0(四柱 + REPL + Telegram + Web Console、`run_turn`、`wukong-render`)。

## 目標

讓 Wukong 以**每 scope 持久、開啟 thinking 的 opencode session** 驅動對話,並在 REPL / Telegram / Web 三個介面提供 session 控制指令 `/new`(開新 context)與 `/compact`(壓縮)。因為 opencode `-c`(continue last session)在「每回合多次呼叫」的架構下不可靠(planner 也會建 session),連續性改以**顯式 session id**(從 opencode JSON 擷取、每 scope 存一份)實現。

## 背景(已對真實 opencode 1.16.2 驗證)

- `opencode run` 旗標:`-c/--continue`、`-s/--session <id>`、`--thinking`(顯示 thinking 區塊)、`--format json`。
- `--format json` 每個事件都帶 `"sessionID":"ses_…"` → 可擷取並用 `-s <id>` 接續。
- `--thinking` 在 json 下產生獨立的 `{"type":"reasoning"}` 事件(與 `{"type":"text"}` 分開)→ 不會污染答案文字。
- **opencode `run` 無 headless 壓縮**:訊息 `/compact` 只被當一般文字送給模型(實測 token 不減、session id 不變、模型仍記得全部歷史)。`/compact` 僅存在於 TUI。本設計仍依使用者決定以**原樣 passthrough** 實作(送 `/compact` 給目前 session),不保證縮減,但無害且未來相容。

## 設計原則

- **連續性用顯式 session id**,不用 `-c`(避免 planner 污染)。
- **planner 無狀態**;只有**最終答案那一棒**接續持久 session。心智模型:持久 session = 使用者看到的對話。
- **指令邏輯集中在引擎層**(`wukong-cli`),三介面共用,避免重複。
- **底層 agent 只以 opencode 為準。**

## 架構總覽

```
使用者輸入 ──(介面解析 slash)──► wukong-cli 指令引擎
   │  一般訊息 → run_turn(planner 無狀態 → 最終棒帶 -s <stored id> --thinking)
   │  /new     → clear_agent_session(scope)            （無模型呼叫）
   │  /compact → backend(-s <stored id> "/compact")     （原樣 passthrough，無 planner/persona）
   ▼
wukong-gateway AgentCliBackend ──► opencode run [-s id] [--thinking] --format json <prompt>
   ▲  擷取每事件的 sessionID → AgentResponse.session_id
   └─ wukong-memory agent_sessions(scope → session_id) 存/取/清
```

## 1. Backend(`wukong-gateway`)

### 型別變更

```rust
pub struct AgentRequest {
    pub prompt: String,
    /// Some(id) → 以 `-s <id>` 接續該 opencode session;None → 不接續(新 session)。
    pub session_id: Option<String>,
    /// true → 傳 `--thinking`。
    pub thinking: bool,
}

pub struct AgentResponse {
    pub text: String,
    /// 從 JSON 事件擷取的 sessionID(供呼叫端存回)。
    pub session_id: Option<String>,
}
```

(移除舊的 `AgentRequest.continue_session`。)

### `assemble_argv`

簽章改為 `assemble_argv(command, session_id: Option<&str>, thinking: bool, prompt) -> Vec<String>`:

```
argv = command.clone()                       // ["opencode","run"]
if let Some(id) = session_id { argv.push("-s"); argv.push(id) }
if thinking { argv.push("--thinking") }
// run_streaming 之後在 prompt 前插 ["--format","json"]
argv.push(prompt)
```

(移除 `continue_args`/`continue_session` 參數。)

### `run` / `run_streaming`

- `AgentCliBackend::run`(非 json 路徑):用新的 `assemble_argv`;`AgentResponse.session_id = None`(無 json 不擷取)。
- `AgentCliBackend::run_streaming`:照舊插 `--format json`、stderr 並行汲取;**新增**:每行解析出 `sessionID`(最後出現者),設 `AgentResponse.session_id`;答案文字仍只折疊 `"type":"text"` 事件。
- trait 預設 `run_streaming` 仍呼叫 `run` 後發單一 Text 事件,`session_id` 透傳。

### StreamEvent

`StreamEvent` 新增 `Reasoning(String)`。`parse_event`:`"type":"reasoning"` → `Reasoning(part.text)`(text 取 `part.text`,缺則空字串)。新增純函式 `parse_session_id(line) -> Option<String>`(解析頂層 `sessionID`)。

## 2. Session-id store(`wukong-memory`)

新增資料表(在既有 SQLite,屬每 scope 編排狀態):

```sql
CREATE TABLE IF NOT EXISTS agent_sessions (
    scope       TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);
```

`Memory` 方法:

```rust
pub async fn agent_session(&self, scope: &str) -> Result<Option<String>, MemoryError>;
pub async fn set_agent_session(&self, scope: &str, session_id: &str) -> Result<(), MemoryError>;
pub async fn clear_agent_session(&self, scope: &str) -> Result<(), MemoryError>;
```

`set_agent_session` 用 UPSERT(`INSERT … ON CONFLICT(scope) DO UPDATE`),`updated_at` 取現在 epoch 秒。

## 3. Turn threading(`wukong-cli::run_turn`)

- `GatewayConfig`:移除 `continue_session`、`continue_args`;新增 `thinking: bool`(預設 true)。
- planner(`plan_chain` 內的 backend 呼叫):`AgentRequest{ session_id: None, thinking: false, … }`。無狀態。
- 角色執行:取 `let stored = memory.agent_session(&cfg.scope).await?;`。
  - **只有最後一棒**帶 `session_id: stored.clone()`、`thinking: cfg.thinking`;其餘棒 `session_id: None`、`thinking: cfg.thinking`(intra-chain 仍靠 `chain_context` 提示傳遞)。
  - 最後一棒回來後:`if let Some(id) = resp.session_id { memory.set_agent_session(&cfg.scope, &id).await?; }`。
  - 單一角色(常見)即同時是最後一棒 → 直接接續並存回。
- `on_event` 收到 `Reasoning` 事件由各介面渲染決定(見 §6)。

> `plan_chain` 目前簽章 `plan_chain(backend, input)`;其內部建構的 `AgentRequest` 改為 `session_id:None, thinking:false`。

## 4. 指令引擎(`wukong-cli`,三介面共用)

新檔 `crates/wukong-cli/src/command.rs`:

```rust
pub enum SessionCommand { New, Compact }

/// 解析指令名(不含 '/')。未知回 None。
pub fn parse_session_command(name: &str) -> Option<SessionCommand> {
    match name { "new" => Some(SessionCommand::New),
                 "compact" => Some(SessionCommand::Compact),
                 _ => None }
}

/// 執行指令,回使用者可見的回覆文字。
pub async fn run_session_command(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    cmd: SessionCommand,
) -> Result<String, WukongError>;
```

行為:
- `New`:`memory.clear_agent_session(&cfg.scope).await?;` 回 `"🐵 已開新 context"`。無模型呼叫。
- `Compact`:
  1. `let sid = memory.agent_session(&cfg.scope).await?;`
  2. `None` → 回 `"🐵 尚無對話可壓縮"`。
  3. `Some(id)` → `backend.run_streaming(AgentRequest{ prompt:"/compact".into(), session_id:Some(id), thinking:false }, &mut |_|{}).await?`(無 planner、無 persona、原樣 passthrough)。
  4. 回傳的 `session_id` 若有則存回(通常不變)。
  5. 回覆 `format!("🐵 已送出壓縮指令：\n{}", resp.text)`(把 opencode 回覆一併呈現)。

## 5. 介面接線

- **REPL**(`repl.rs::classify_line`):`/new`、`/compact` → 新 `LineAction::Command(SessionCommand)`;loop 中呼叫 `run_session_command` 並印回覆。`/exit`、`/quit`、`/scope` 維持。
- **Telegram**(`dispatch.rs` 的 `MessageAction::Command` 分支):`parse_session_command(&name)` 命中 → `run_session_command` → `send_message` 回覆;未命中 → 維持「指令 /{name} 尚未支援」。
- **Web**(`wukong-web` 的 `/chat?q=`):`q` 去空白後以 `/` 開頭 → 取第一個 token 當指令名,`parse_session_command` 命中 → `run_session_command` → 以一個 `answer` 事件(`to_web_html(reply)`)+ `done` 回傳;未命中的 slash → `answer`「指令尚未支援」+ `done`;非 slash → 既有 turn 流程。
- **一次性 CLI**(`wukong "prompt"`,`cli.rs` + `main.rs`):
  - 連續性現為預設(每 scope 存 session id);新增 `--new`(turn 前先 `clear_agent_session`)、`--no-thinking`(關 thinking)。
  - 移除舊的 `-c`/continue 旗標(連續性已是預設)。`GatewayConfig::resolve` 對應更新:`thinking = !cli.no_thinking && env WUKONG_THINKING != "0"`。

## 6. Thinking 顯示

- 一般 turn 預設帶 `--thinking`(可由 `--no-thinking` / `WUKONG_THINKING=0` 關閉)。
- **REPL**:`render.rs::StreamRenderer` 處理 `StreamEvent::Reasoning` → 輸出到 stderr(淡色/前綴,如 `💭`),與既有 tool 活動同分流。
- **Telegram / Web**:本版忽略 `Reasoning` 事件,只呈現最終答案(顯示 thinking 為後續增強)。

## 錯誤處理

- `/compact` 無 session → 友善回覆,不呼叫模型。
- `/compact` 或最終棒的 opencode 失敗 → 回報錯誤;**`/new` 以外不在失敗時清掉既有 session**(避免誤丟)。
- 最終棒回應缺 `sessionID` → 不存(下回合自然開新 session),不 crash。
- session store 讀寫錯誤 → 併入 `WukongError::Memory`。

## 測試策略

- **gateway**:`assemble_argv` 各組合(無 session/有 session/有 thinking/皆有,順序正確);`parse_session_id` 解析;`parse_event` 對 `reasoning` 事件 → `Reasoning`;`run_streaming`(用 `printf` 假 agent 餵 NDJSON,含 `sessionID`)擷取 `AgentResponse.session_id`。
- **memory**:`set/get/clear agent_session` round-trip;UPSERT 覆寫;清不存在的 scope 不報錯。
- **cli**:
  - `run_turn` 把 `agent_session(scope)` 帶進最終棒、並把回傳 id 存回(MockBackend 回固定 session_id;斷言 `memory.agent_session` 更新)。
  - 多角色鏈:只有最後一棒帶 session_id(MockBackend 記錄每次 `req.session_id`,斷言前面為 None、最後為 stored)。
  - `parse_session_command`:`new`/`compact`/未知。
  - `run_session_command`:`New` 清 session(先設再清,斷言變 None);`Compact` 無 session → 固定回覆且不呼叫 backend;`Compact` 有 session → 呼叫 backend 一次、prompt 為 `/compact`、session_id 為 stored、回覆含 backend 文字。
- **repl**:`classify_line` 認得 `/new`、`/compact`(回 `Command`);loop 跑指令不當成 turn。
- **telegram**:Command 分支 `new`/`compact` → 呼叫引擎並回覆(MockTgClient 斷言送出回覆);未知指令仍「尚未支援」。
- **web**:`GET /chat?q=/new`(MockBackend)→ SSE 含 `event: answer`(含「已開新 context」)+ `event: done`,且**未**觸發 turn 的 role 事件;`q=/compact` 同理走 passthrough。
- 真實 opencode 為手動煙霧(REPL 連續兩回合確認接續同 session;`/new` 後 token/context 重置;`/compact` 送出且顯示回覆;`--thinking` 在 REPL 看到 💭 活動)。

## 非目標(YAGNI)

- 清理 planner 與中間棒留下的暫時 opencode session(會累積,同今日狀況;之後可加 `opencode session delete` 掃除)。
- 讓每一棒鏈角色都接續同一 session(只接最後一棒)。
- Telegram / Web 顯示 thinking。
- `/model` 切換模型、`/compact` 真壓縮(待 opencode 開放 headless API)。
- `/compact` 訊息可設定化(本版固定送 `/compact`)。
