# 互動 REPL + 活動渲染 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 為 `wukong` 加入互動 REPL（多輪、session 接續、記憶累積），並把 execute 步驟的 agent 輸出改為「活動渲染」（spinner + 工具活動 + 文字分流）。

**Architecture:** 分層——`wukong-gateway` 新增 `StreamEvent` + `AiBackend::run_streaming`（懂 opencode `--format json`，預設實作 fallback 成 `run`）；`wukong-cli` 新增 `render`（事件→終端，stdout/stderr 分流）與 `repl`（互動迴圈），`run_turn` execute 改走串流。無 prompt → REPL；有 prompt → 單次。`--no-stream`/`WUKONG_STREAM=0` 退回純文字。

**Tech Stack:** Rust、tokio（process/io）、clap、serde_json（解析 NDJSON）。

**對應 spec：** `docs/superpowers/specs/2026-06-06-repl-streaming-design.md`

**前提：** opencode 不吐 token delta（已實測）；活動渲染顆粒度為「片段」非逐字。底層 agent 一律 opencode。

**慣例：** cargo 指令前綴 `. "$HOME/.cargo/env" &&`；串接測試+commit 用 `set -o pipefail`。Commit 訊息只寫功能描述，不得含 AI 署名。

---

## File Structure

- `crates/wukong-gateway/src/stream.rs` — **新**：`StreamEvent` 列舉、`parse_event`（opencode NDJSON 一行→事件）。
- `crates/wukong-gateway/src/backend.rs` — **改**：`AiBackend::run_streaming`（預設實作）+ `AgentCliBackend` 覆寫。
- `crates/wukong-gateway/src/cli.rs` — **改**：`prompt` 改可選；`--no-stream` 旗標；翻轉 `prompt_is_required` 測試。
- `crates/wukong-gateway/src/config.rs` — **改**：`GatewayConfig.stream` + `resolve`。
- `crates/wukong-gateway/src/lib.rs` — **改**：`pub mod stream;`、re-export。
- `crates/wukong-cli/src/render.rs` — **新**：`StreamRenderer`（事件→注入式 out/err writer）。
- `crates/wukong-cli/src/repl.rs` — **新**：`run_repl`（互動迴圈，注入式輸入以便測試）。
- `crates/wukong-cli/src/lib.rs` — **改**：`run_turn` execute 走 `run_streaming` + 角色行順序；模組宣告。
- `crates/wukong-cli/src/main.rs` — **改**：無 prompt → REPL；單次套用 renderer。

serde_json 需加入 wukong-gateway 相依（workspace 已有 `serde_json`）。

---

### Task 1: StreamEvent 與 parse_event（gateway/stream.rs）

**Files:**
- Create: `crates/wukong-gateway/src/stream.rs`
- Modify: `crates/wukong-gateway/src/lib.rs`
- Modify: `crates/wukong-gateway/Cargo.toml`

- [ ] **Step 1: 加 serde_json 相依**

編輯 `crates/wukong-gateway/Cargo.toml`，在 `[dependencies]` 區（緊接現有依賴）加：

```toml
serde_json = { workspace = true }
```

- [ ] **Step 2: 建 stream.rs（先寫含測試的完整模組）**

建立 `crates/wukong-gateway/src/stream.rs`：

```rust
//! Parsing of opencode `--format json` NDJSON events into render-relevant
//! StreamEvents. opencode emits one JSON object per line.

/// One render-relevant event parsed from the agent's `--format json` stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A chunk of assistant text (opencode "text" part).
    Text(String),
    /// A tool invocation by name (opencode "tool_use").
    ToolUse(String),
    /// A step begins (drives the spinner).
    StepStart,
    /// A step ends.
    StepFinish,
}

/// Parse one NDJSON line into a StreamEvent. Unrecognized or malformed lines
/// return None and are ignored by callers.
pub fn parse_event(line: &str) -> Option<StreamEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    match v.get("type")?.as_str()? {
        "text" => {
            let t = v.get("text").and_then(|t| t.as_str()).unwrap_or_default();
            Some(StreamEvent::Text(t.to_string()))
        }
        "tool_use" => {
            // tool name may live under "name" or "tool"; fall back to "tool".
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .or_else(|| v.get("tool").and_then(|n| n.as_str()))
                .unwrap_or("tool");
            Some(StreamEvent::ToolUse(name.to_string()))
        }
        "step_start" => Some(StreamEvent::StepStart),
        "step_finish" => Some(StreamEvent::StepFinish),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_event() {
        let ev = parse_event(r#"{"type":"text","text":"hello"}"#);
        assert_eq!(ev, Some(StreamEvent::Text("hello".to_string())));
    }

    #[test]
    fn parses_tool_use_with_name_or_tool() {
        assert_eq!(
            parse_event(r#"{"type":"tool_use","name":"read"}"#),
            Some(StreamEvent::ToolUse("read".to_string()))
        );
        assert_eq!(
            parse_event(r#"{"type":"tool_use","tool":"edit"}"#),
            Some(StreamEvent::ToolUse("edit".to_string()))
        );
    }

    #[test]
    fn parses_step_events() {
        assert_eq!(parse_event(r#"{"type":"step_start"}"#), Some(StreamEvent::StepStart));
        assert_eq!(parse_event(r#"{"type":"step_finish"}"#), Some(StreamEvent::StepFinish));
    }

    #[test]
    fn ignores_malformed_and_unknown() {
        assert_eq!(parse_event("not json"), None);
        assert_eq!(parse_event(""), None);
        assert_eq!(parse_event(r#"{"type":"session.updated"}"#), None);
        assert_eq!(parse_event(r#"{"no_type":1}"#), None);
    }
}
```

- [ ] **Step 3: 掛上模組並 re-export**

編輯 `crates/wukong-gateway/src/lib.rs`，在 `pub mod prompt;` 後加 `pub mod stream;`，並在 `pub use error::GatewayError;` 後加：

```rust
pub use stream::StreamEvent;
```

- [ ] **Step 4: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-gateway stream::`
Expected: 4 個 stream tests PASS。

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-gateway/src/stream.rs crates/wukong-gateway/src/lib.rs crates/wukong-gateway/Cargo.toml
git commit -m "feat(gateway): add StreamEvent and opencode NDJSON parser"
```

---

### Task 2: AiBackend::run_streaming（gateway/backend.rs）

**Files:**
- Modify: `crates/wukong-gateway/src/backend.rs`

- [ ] **Step 1: 加 trait 方法（預設實作）與 AgentCliBackend 覆寫，含測試**

在 `crates/wukong-gateway/src/backend.rs` 頂端 `use` 區補上：

```rust
use crate::error::GatewayError;
use crate::stream::{parse_event, StreamEvent};
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
```

在 `AiBackend` trait 內，於既有 `run` 之後加帶預設實作的 `run_streaming`：

```rust
    /// Run, invoking `on_event` as events arrive, returning the full response.
    /// Default: call `run`, then emit the whole text as a single Text event.
    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        let resp = self.run(req).await?;
        on_event(StreamEvent::Text(resp.text.clone()));
        Ok(resp)
    }
```

在 `impl AiBackend for AgentCliBackend { ... }` 內，於既有 `run` 之後加覆寫（附加 `--format json`、逐行解析、累積文字）：

```rust
    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        // Build argv then insert `--format json` before the prompt (last arg).
        let mut argv = assemble_argv(
            &self.command,
            &self.continue_args,
            req.continue_session,
            &req.prompt,
        );
        let prompt = argv.pop().expect("argv always ends with the prompt");
        argv.push("--format".to_string());
        argv.push("json".to_string());
        argv.push(prompt);

        let mut child = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()?;

        // Drain stderr concurrently so a large stderr can't deadlock us while
        // we read stdout line-by-line.
        let stderr = child.stderr.take().expect("stderr piped");
        let stderr_task = tokio::spawn(async move {
            use tokio::io::AsyncReadExt;
            let mut buf = String::new();
            let mut rdr = stderr;
            let _ = rdr.read_to_string(&mut buf).await;
            buf
        });

        let stdout = child.stdout.take().expect("stdout piped");
        let mut lines = BufReader::new(stdout).lines();
        let mut full = String::new();
        while let Some(line) = lines.next_line().await? {
            if let Some(ev) = parse_event(&line) {
                if let StreamEvent::Text(t) = &ev {
                    if !full.is_empty() {
                        full.push('\n');
                    }
                    full.push_str(t);
                }
                on_event(ev);
            }
        }

        let status = child.wait().await?;
        let stderr_buf = stderr_task.await.unwrap_or_default();
        if !status.success() {
            return Err(GatewayError::AgentFailed {
                code: status.code(),
                stderr: stderr_buf.trim().to_string(),
            });
        }
        Ok(AgentResponse {
            text: full.trim().to_string(),
        })
    }
```

在 `mod tests` 內新增（用 `printf` 餵假 NDJSON；預設實作 fallback 用既有 echo 模式）：

```rust
    #[tokio::test]
    async fn run_streaming_default_emits_single_text() {
        // echo backend has no override path of its own; AgentCliBackend overrides,
        // so test the DEFAULT impl via a minimal mock.
        struct Plain;
        impl AiBackend for Plain {
            async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
                Ok(AgentResponse { text: "whole answer".to_string() })
            }
        }
        let mut events = Vec::new();
        let resp = Plain
            .run_streaming(
                AgentRequest { prompt: "x".into(), continue_session: false },
                &mut |e| events.push(e),
            )
            .await
            .unwrap();
        assert_eq!(resp.text, "whole answer");
        assert_eq!(events, vec![StreamEvent::Text("whole answer".to_string())]);
    }

    #[tokio::test]
    async fn agent_cli_run_streaming_parses_ndjson() {
        // A fake "agent" that ignores args and prints NDJSON events to stdout.
        // `printf` receives our canned stream as its format string; argv tail
        // (--format json <prompt>) is harmless extra args to printf.
        let backend = AgentCliBackend {
            command: vec![
                "printf".to_string(),
                "%s\\n".to_string(),
                r#"{"type":"step_start"}"#.to_string(),
                r#"{"type":"tool_use","name":"read"}"#.to_string(),
                r#"{"type":"text","text":"hello"}"#.to_string(),
                r#"{"type":"step_finish"}"#.to_string(),
            ],
            continue_args: vec![],
        };
        let mut events = Vec::new();
        let resp = backend
            .run_streaming(
                AgentRequest { prompt: "ignored".into(), continue_session: false },
                &mut |e| events.push(e),
            )
            .await
            .unwrap();
        assert_eq!(resp.text, "hello");
        assert_eq!(
            events,
            vec![
                StreamEvent::StepStart,
                StreamEvent::ToolUse("read".to_string()),
                StreamEvent::Text("hello".to_string()),
                StreamEvent::StepFinish,
            ]
        );
    }
```

> 說明：`printf "%s\n" A B C D` 會把每個額外引數各印成一行；argv 尾端被覆寫加上的 `--format`、`json`、`<prompt>` 也只是多餘的 `%s\n` 引數（再多印幾行非 JSON，被 `parse_event` 忽略），不影響前述事件序列與累積文字。

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-gateway backend::`
Expected: 既有 4 + 新 2 = 6 PASS。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-gateway/src/backend.rs
git commit -m "feat(gateway): add run_streaming with opencode json event parsing"
```

---

### Task 3: Cli prompt 改可選 + --no-stream（gateway/cli.rs）

**Files:**
- Modify: `crates/wukong-gateway/src/cli.rs`

- [ ] **Step 1: 改 prompt 為可選、加 no_stream 欄位、翻轉/新增測試**

在 `crates/wukong-gateway/src/cli.rs`，把 `prompt` 欄位的屬性由 `#[arg(required = true, num_args = 1..)]` 改為可選：

```rust
    /// The prompt to send to the assistant (joined with spaces). Empty => REPL.
    #[arg(num_args = 0..)]
    pub prompt: Vec<String>,
```

在 `agent_cmd` 欄位之後新增：

```rust
    /// Disable activity rendering (spinner + tool events); use plain capture.
    #[arg(long = "no-stream")]
    pub no_stream: bool,
```

把既有 `prompt_is_required` 測試整段替換為「無 prompt 允許（給 REPL）」：

```rust
    #[test]
    fn no_prompt_is_allowed_for_repl() {
        let cli = Cli::try_parse_from(["wukong"]).unwrap();
        assert!(cli.prompt_text().is_empty());
    }

    #[test]
    fn no_stream_flag_parses() {
        let cli = Cli::try_parse_from(["wukong", "--no-stream", "hi"]).unwrap();
        assert!(cli.no_stream);
        assert_eq!(cli.prompt_text(), "hi");
    }
```

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-gateway cli::`
Expected: PASS（`parses_prompt_and_flags`、`agent_cmd_override_parses`、`no_prompt_is_allowed_for_repl`、`no_stream_flag_parses`）。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-gateway/src/cli.rs
git commit -m "feat(gateway): make prompt optional for REPL and add --no-stream"
```

---

### Task 4: GatewayConfig.stream（gateway/config.rs）

**Files:**
- Modify: `crates/wukong-gateway/src/config.rs`

- [ ] **Step 1: 加 stream 欄位與解析，更新測試**

在 `crates/wukong-gateway/src/config.rs` 的 `GatewayConfig` 結構末欄（`recall_top_k` 之後）加：

```rust
    pub recall_top_k: usize,
    /// Activity rendering (spinner + tool events). Default true; off via
    /// `--no-stream` or `WUKONG_STREAM=0`.
    pub stream: bool,
}
```

在 `resolve` 內，於建構 `GatewayConfig { ... }` 之前計算 `stream`：

```rust
        let stream = !cli.no_stream && std::env::var("WUKONG_STREAM").as_deref() != Ok("0");
```

並在回傳的結構字面量末加 `stream,`：

```rust
        GatewayConfig {
            scope,
            db_url,
            agent_command,
            continue_args,
            continue_session: cli.continue_session,
            recall_top_k: 5,
            stream,
        }
```

更新 `cli_overrides_take_priority` 測試，在最後加一行斷言：

```rust
        assert_eq!(cfg.continue_args, vec!["-c".to_string()]);
        assert!(cfg.stream); // default on when --no-stream absent
    }

    #[test]
    fn no_stream_flag_disables_stream() {
        let cli = Cli::try_parse_from(["wukong", "--no-stream", "hi"]).unwrap();
        let cfg = GatewayConfig::resolve(&cli);
        assert!(!cfg.stream);
    }
```

> 注意：`test_cfg` helper 在 `wukong-cli/src/lib.rs` 的測試也建構 `GatewayConfig`，Task 7 會補上 `stream` 欄位以免編譯失敗。

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-gateway config::`
Expected: PASS（含 `no_stream_flag_disables_stream`）。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-gateway/src/config.rs
git commit -m "feat(gateway): add stream config resolved from --no-stream and WUKONG_STREAM"
```

---

### Task 5: StreamRenderer（cli/render.rs）

**Files:**
- Create: `crates/wukong-cli/src/render.rs`
- Modify: `crates/wukong-cli/src/lib.rs`（加 `pub mod render;`）

- [ ] **Step 1: 掛模組**

在 `crates/wukong-cli/src/lib.rs` 的 `pub mod persona;` 後加：

```rust
pub mod render;
```

- [ ] **Step 2: 建 render.rs（注入式 writer，含測試）**

建立 `crates/wukong-cli/src/render.rs`：

```rust
//! Render StreamEvents to a terminal: assistant text to stdout, activity
//! (tools, spinner cues) to stderr. Writers are injected for testability.

use std::io::Write;
use wukong_gateway::StreamEvent;

/// Routes streamed events to two writers. `out` receives assistant text
/// (pipe-friendly); `err` receives activity lines (tools, etc.).
pub struct StreamRenderer<'a> {
    out: &'a mut dyn Write,
    err: &'a mut dyn Write,
}

impl<'a> StreamRenderer<'a> {
    pub fn new(out: &'a mut dyn Write, err: &'a mut dyn Write) -> Self {
        Self { out, err }
    }

    /// Handle one event. Text → out; ToolUse → err; step events are spinner
    /// cues handled by the live UI (no-op for buffered writers here).
    pub fn on_event(&mut self, ev: &StreamEvent) {
        match ev {
            StreamEvent::Text(t) => {
                let _ = write!(self.out, "{t}");
                let _ = self.out.flush();
            }
            StreamEvent::ToolUse(name) => {
                let _ = writeln!(self.err, "  ▸ 使用工具 {name}");
                let _ = self.err.flush();
            }
            StreamEvent::StepStart | StreamEvent::StepFinish => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_goes_to_out_tools_to_err() {
        let mut out: Vec<u8> = Vec::new();
        let mut err: Vec<u8> = Vec::new();
        {
            let mut r = StreamRenderer::new(&mut out, &mut err);
            r.on_event(&StreamEvent::StepStart);
            r.on_event(&StreamEvent::ToolUse("read".to_string()));
            r.on_event(&StreamEvent::Text("hello ".to_string()));
            r.on_event(&StreamEvent::Text("world".to_string()));
            r.on_event(&StreamEvent::StepFinish);
        }
        assert_eq!(String::from_utf8(out).unwrap(), "hello world");
        assert_eq!(String::from_utf8(err).unwrap(), "  ▸ 使用工具 read\n");
    }
}
```

> 說明：本任務的 spinner 為「實機 live UI」職責，緩衝測試只驗證 text/tool 分流；step 事件在這裡是 no-op，spinner 動畫由 main/REPL 在實機以 stderr 直接驅動（無 TTY 時不畫），不納入單元測試以免依賴計時。

- [ ] **Step 3: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-cli render::`
Expected: 1 PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-cli/src/render.rs crates/wukong-cli/src/lib.rs
git commit -m "feat(cli): add StreamRenderer routing text to stdout and tools to stderr"
```

---

### Task 6: run_turn 走 run_streaming + on_role 回呼（cli/lib.rs）

**Files:**
- Modify: `crates/wukong-cli/src/lib.rs`

- [ ] **Step 1: 改 run_turn 簽名（加 on_event + on_role）與內文**

在 `crates/wukong-cli/src/lib.rs`，把 `run_turn` 簽名改為同時接受事件回呼與角色回呼（角色在 route 後即回呼，確保串流時角色行可在文字之前印）：

```rust
pub async fn run_turn(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    on_role: &mut dyn FnMut(Role),
) -> Result<TurnOutput, WukongError> {
```

把 route 段改為路由後立即回呼角色：

```rust
    // 2. Route the task to a role.
    let role = wukong_orchestrator::route(backend, input).await?;
    on_role(role);
```

把 execute 段由 `run` 改為 `run_streaming`：

```rust
    // 4. Execute (streamed): events flow to the caller-provided sink.
    let resp = backend
        .run_streaming(
            AgentRequest {
                prompt,
                continue_session: cfg.continue_session,
            },
            on_event,
        )
        .await?;
```

（recall、build_prompt、remember、回傳 `TurnOutput{role,text}` 皆不變。）

- [ ] **Step 2: 更新 lib.rs 既有測試的 run_turn 呼叫與 test_cfg**

更新 `mod tests` 內兩處 `run_turn` 呼叫，補上兩個忽略回呼：

`run_turn_routes_executes_and_persists`：

```rust
        let out = run_turn(&mem, &backend, &test_cfg("project:T"), "fix the bug", &mut |_| {}, &mut |_| {})
            .await
            .unwrap();
```

`execution_prompt_carries_persona_and_role`：

```rust
        run_turn(&mem, &backend, &test_cfg("project:T"), "fix the bug", &mut |_| {}, &mut |_| {})
            .await
            .unwrap();
```

並更新 `test_cfg` helper 補上 `stream` 欄位：

```rust
    fn test_cfg(scope: &str) -> GatewayConfig {
        GatewayConfig {
            scope: scope.to_string(),
            db_url: String::new(),
            agent_command: vec![],
            continue_args: vec![],
            continue_session: false,
            recall_top_k: 5,
            stream: true,
        }
    }
```

> 說明：MockBackend 未覆寫 `run_streaming`，走預設實作（呼叫 `run` 後送單一 `Text`），故既有斷言（route=fixer、text=done、persona prompt）不受影響；`&mut |_| {}` 忽略事件與角色。`Role` 已在 lib.rs 頂端 `use wukong_orchestrator::Role;` 匯入（既有）。

- [ ] **Step 3: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-cli` （含 lib + render）
Expected: 既有 2 個 run_turn 測試 + render 測試全 PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-cli/src/lib.rs
git commit -m "feat(cli): stream execute via run_streaming with event and role sinks"
```

---

### Task 7: REPL 迴圈（cli/repl.rs）

**Files:**
- Create: `crates/wukong-cli/src/repl.rs`
- Modify: `crates/wukong-cli/src/lib.rs`（加 `pub mod repl;`）

- [ ] **Step 1: 掛模組**

在 `crates/wukong-cli/src/lib.rs` 的 `pub mod render;` 後加：

```rust
pub mod repl;
```

- [ ] **Step 2: 建 repl.rs（迴圈核心抽出為可測函式 + 行處理）**

建立 `crates/wukong-cli/src/repl.rs`。核心是把「一行輸入 → 動作」抽成可測的純函式 `classify_line`，迴圈體 `run_repl_loop` 以注入的輸入行迭代器 + 事件回呼測試：

```rust
//! Interactive REPL: multi-turn loop sharing one Memory, with session
//! continuation after the first turn and minimal meta-commands.

use crate::{run_turn, WukongError};
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::StreamEvent;
use wukong_memory::Memory;

/// What a single REPL input line means.
#[derive(Debug, PartialEq)]
pub enum LineAction {
    Exit,
    Skip,
    SetScope(String),
    Turn(String),
}

/// Classify one raw input line into an action.
pub fn classify_line(line: &str) -> LineAction {
    let t = line.trim();
    if t.is_empty() {
        return LineAction::Skip;
    }
    match t {
        "/exit" | "/quit" => LineAction::Exit,
        _ => {
            if let Some(rest) = t.strip_prefix("/scope ") {
                let s = rest.trim();
                if s.is_empty() {
                    LineAction::Skip
                } else {
                    LineAction::SetScope(s.to_string())
                }
            } else {
                LineAction::Turn(t.to_string())
            }
        }
    }
}

/// Run turns over a sequence of input lines (injectable for tests). Returns the
/// number of turns executed. `on_event` receives streamed events per turn;
/// `on_role` is called once per turn with the chosen role name (for the header).
pub async fn run_repl_loop<I>(
    memory: &Memory,
    backend: &impl AiBackend,
    base_cfg: &GatewayConfig,
    lines: I,
    on_event: &mut dyn FnMut(StreamEvent),
    on_role: &mut dyn FnMut(&str),
) -> Result<usize, WukongError>
where
    I: IntoIterator<Item = String>,
{
    let mut cfg = base_cfg.clone();
    cfg.continue_session = false; // first turn fresh
    let mut turns = 0usize;
    for line in lines {
        match classify_line(&line) {
            LineAction::Exit => break,
            LineAction::Skip => continue,
            LineAction::SetScope(s) => {
                cfg.scope = s;
            }
            LineAction::Turn(input) => {
                // Forward the routed role (as name) to the loop's on_role sink.
                run_turn(memory, backend, &cfg, &input, on_event, &mut |r| {
                    on_role(r.name())
                })
                .await?;
                turns += 1;
                cfg.continue_session = true; // subsequent turns continue session
            }
        }
    }
    Ok(turns)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<String>>,
        continue_flags: Mutex<Vec<bool>>,
    }
    impl MockBackend {
        fn new(replies: &[&str]) -> Self {
            Self {
                replies: Mutex::new(replies.iter().map(|s| s.to_string()).collect()),
                continue_flags: Mutex::new(Vec::new()),
            }
        }
    }
    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.continue_flags.lock().unwrap().push(req.continue_session);
            let text = self.replies.lock().unwrap().pop_front().unwrap_or_default();
            Ok(AgentResponse { text })
        }
    }

    async fn open_memory() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    fn cfg() -> GatewayConfig {
        GatewayConfig {
            scope: "project:T".to_string(),
            db_url: String::new(),
            agent_command: vec![],
            continue_args: vec![],
            continue_session: false,
            recall_top_k: 5,
            stream: true,
        }
    }

    #[test]
    fn classify_line_cases() {
        assert_eq!(classify_line("  "), LineAction::Skip);
        assert_eq!(classify_line("/exit"), LineAction::Exit);
        assert_eq!(classify_line("/quit"), LineAction::Exit);
        assert_eq!(classify_line("/scope global"), LineAction::SetScope("global".to_string()));
        assert_eq!(classify_line("/scope   "), LineAction::Skip);
        assert_eq!(classify_line("fix the bug"), LineAction::Turn("fix the bug".to_string()));
    }

    #[tokio::test]
    async fn loop_runs_turns_until_exit_and_continues_session() {
        let mem = open_memory().await;
        // route+execute per turn => 2 replies per turn; 2 turns then /exit.
        let backend = MockBackend::new(&["fixer", "ans1", "oracle", "ans2"]);
        let lines = vec![
            "first question".to_string(),
            "".to_string(), // skipped
            "second question".to_string(),
            "/exit".to_string(),
            "ignored after exit".to_string(),
        ];
        let mut roles = Vec::new();
        let turns = run_repl_loop(
            &mem,
            &backend,
            &cfg(),
            lines,
            &mut |_| {},
            &mut |r| roles.push(r.to_string()),
        )
        .await
        .unwrap();
        assert_eq!(turns, 2);
        // route reply "fixer" => Role::Fixer (name "fixer"); "oracle" => "oracle".
        assert_eq!(roles, vec!["fixer".to_string(), "oracle".to_string()]);
        // continue_session flags: route+execute of turn1 = false,false; turn2 = true,true
        let flags = backend.continue_flags.lock().unwrap().clone();
        assert_eq!(flags, vec![false, false, true, true]);
    }

    #[tokio::test]
    async fn turn_persists_memory_across_loop() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer", "done"]);
        run_repl_loop(&mem, &backend, &cfg(), vec!["fix it".to_string(), "/exit".to_string()], &mut |_| {}, &mut |_| {})
            .await
            .unwrap();
        let r = mem
            .recall(wukong_memory::RecallQuery {
                query: "fix it".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: wukong_memory::RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("User: fix it")));
    }
}
```

> 路由說明：`parse_role` 大小寫無關掃描角色名，對未知文字回退 `Oracle`。route 回覆 `"fixer"`→`Role::Fixer`（`name()`=`"fixer"`）、`"oracle"`→`Role::Oracle`（`name()`=`"oracle"`），故 roles 斷言為 `["fixer","oracle"]`。

- [ ] **Step 3: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-cli repl::`
Expected: `classify_line_cases`、`loop_runs_turns_until_exit_and_continues_session`、`turn_persists_memory_across_loop` 全 PASS。

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-cli/src/repl.rs crates/wukong-cli/src/lib.rs
git commit -m "feat(cli): add REPL loop with session continuation and meta-commands"
```

---

### Task 8: main 接線（無 prompt→REPL，單次套用 renderer）

**Files:**
- Modify: `crates/wukong-cli/src/main.rs`

`run_turn` 在 Task 6 已具備 `on_event` + `on_role` 雙回呼，故 main 可直接乾淨接線：角色行由 `on_role` 在 route 後印到 stderr（位於串流文字之前），文字由事件回呼即時印到 stdout。

- [ ] **Step 1: 改寫 main.rs（單次 + REPL 共用 run_one）**

完整替換 `crates/wukong-cli/src/main.rs` 為（保留 v0.2.0 的 `#[cfg(feature = "embed")]` embedder 接線；若該區塊與此處不同，僅保留 embedder 那段、其餘照本檔）：

```rust
use clap::Parser;
use std::io::{BufRead, Write};
use wukong_cli::repl::{classify_line, LineAction};
use wukong_cli::run_turn;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::cli::Cli;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::StreamEvent;
use wukong_memory::Memory;

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let cfg = GatewayConfig::resolve(&cli);

    let memory = match Memory::open(&cfg.db_url).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: failed to open memory: {e}");
            std::process::exit(1);
        }
    };

    #[cfg(feature = "embed")]
    let memory = if std::env::var("WUKONG_EMBED").as_deref() == Ok("1") {
        match wukong_memory::FastembedBackend::new() {
            Ok(backend) => memory.with_embedder(std::sync::Arc::new(backend)),
            Err(e) => {
                eprintln!("🐵 語意層停用（模型載入失敗）：{e}");
                memory
            }
        }
    } else {
        memory
    };

    let backend = AgentCliBackend {
        command: cfg.agent_command.clone(),
        continue_args: cfg.continue_args.clone(),
    };

    let prompt = cli.prompt_text();

    if prompt.is_empty() {
        // No prompt => interactive REPL over real stdin.
        eprintln!("🐵 悟空 REPL。輸入 /exit 或 Ctrl-D 離開。");
        let stdin = std::io::stdin();
        let mut cfg_repl = cfg.clone();
        cfg_repl.continue_session = false;
        let mut first = true;
        loop {
            eprint!("悟空 › ");
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
                eprintln!();
                break; // EOF (Ctrl-D)
            }
            match classify_line(&line) {
                LineAction::Exit => break,
                LineAction::Skip => continue,
                LineAction::SetScope(s) => {
                    cfg_repl.scope = s;
                }
                LineAction::Turn(input) => {
                    cfg_repl.continue_session = !first;
                    first = false;
                    if let Err(e) = run_one(&memory, &backend, &cfg_repl, &input).await {
                        eprintln!("error: {e}");
                    }
                }
            }
        }
        return;
    }

    // Single shot.
    if let Err(e) = run_one(&memory, &backend, &cfg, &prompt).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Run one turn, rendering per `cfg.stream`. The role header prints to stderr
/// right after routing (before streamed text); answer text goes to stdout.
async fn run_one(
    memory: &Memory,
    backend: &AgentCliBackend,
    cfg: &GatewayConfig,
    input: &str,
) -> Result<(), wukong_cli::WukongError> {
    if cfg.stream {
        let mut sink = |ev: StreamEvent| match ev {
            StreamEvent::Text(t) => {
                print!("{t}");
                let _ = std::io::stdout().flush();
            }
            StreamEvent::ToolUse(n) => {
                eprintln!("  ▸ 使用工具 {n}");
            }
            _ => {}
        };
        run_turn(memory, backend, cfg, input, &mut sink, &mut |role| {
            eprintln!("🐵 悟空·{}", role.name());
        })
        .await?;
        println!(); // newline after streamed text
        Ok(())
    } else {
        let res = run_turn(memory, backend, cfg, input, &mut |_| {}, &mut |role| {
            eprintln!("🐵 悟空·{}", role.name());
        })
        .await?;
        println!("{}", res.text);
        Ok(())
    }
}
```

> 說明：串流模式下角色行先由 `on_role` 印到 stderr，再由事件回呼把文字即時印到 stdout，最後補一個換行。非串流模式角色行先印、再一次印完整文字。REPL 第一回合 `continue_session=false`，之後 `true`。`/scope` 即時改本 session scope。

- [ ] **Step 2: 編譯與既有測試**

Run: `. "$HOME/.cargo/env" && set -o pipefail && cargo build 2>&1 | tail -5 && cargo test -p wukong-cli 2>&1 | tail -15`
Expected: 編譯成功；cli 測試（lib + render + repl）全綠。

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-cli/src/main.rs
git commit -m "feat(cli): enter REPL when no prompt and render activity per stream config"
```

---

### Task 9: 全工作區回歸 + 手動煙霧

**Files:** （無新檔）

- [ ] **Step 1: 全工作區測試 + clippy**

Run: `. "$HOME/.cargo/env" && set -o pipefail && cargo test 2>&1 | grep -E "test result|error\[" | tail -25 && cargo clippy --all-targets -- -D warnings 2>&1 | tail -4`
Expected: 全綠、零警告。

- [ ] **Step 2: 手動煙霧——單次串流（opencode）**

Run:
```bash
. "$HOME/.cargo/env" && rm -f /tmp/wk-c.db* && \
cargo build && \
./target/debug/wukong --db "sqlite:///tmp/wk-c.db" --scope global --agent-cmd "opencode run" "用一句話自我介紹"
```
Expected: stderr 顯示 `🐵 悟空·<role>`（在答案之前）與可能的 `▸ 使用工具…`；stdout 顯示答案文字。`rm -f /tmp/wk-c.db*` 清理。

- [ ] **Step 3: 手動煙霧——REPL 多輪 + 管線非串流**

Run:
```bash
. "$HOME/.cargo/env" && rm -f /tmp/wk-c2.db* && \
printf '%s\n' "你好，記住我喜歡簡潔回答" "剛剛我說我喜歡什麼？" "/exit" | \
./target/debug/wukong --db "sqlite:///tmp/wk-c2.db" --scope global --agent-cmd "opencode run"
echo "--- 非串流管線 ---"
./target/debug/wukong --no-stream --db "sqlite:///tmp/wk-c2.db" --scope global --agent-cmd "opencode run" "一句話總結" > /tmp/wk-c2.out
cat /tmp/wk-c2.out
rm -f /tmp/wk-c2.db* /tmp/wk-c2.out
```
Expected: REPL 連續兩輪有回答、第二輪能延續；`--no-stream` 單次把純文字答案寫入檔案（stdout 乾淨）。

- [ ] **Step 4: Commit（若手動煙霧促成任何修正）**

```bash
git add -A && git commit -m "test: smoke-verify REPL and streaming with opencode" || echo "no changes"
```

---

## 完成後

全部任務完成、`cargo test` 全綠後，套用 **superpowers:finishing-a-development-branch** 收尾。

文件更新（非阻塞，可併入收尾）：
- 根 `README.md`：使用段加入「互動 REPL（無參數進入）」與「活動渲染／`--no-stream`」；roadmap 移除「互動 REPL／串流」一項（或標註串流受 opencode 限制為片段級）。
- `crates/wukong-cli/README.md`：補 REPL 與串流旗標說明。

---

## Self-Review

**1. Spec coverage：**
- StreamEvent + parse_event → Task 1 ✓
- run_streaming（預設實作 + AgentCliBackend 覆寫，--format json）→ Task 2 ✓
- StreamRenderer（stdout/stderr 分流）→ Task 5 ✓
- REPL（迴圈、/exit /quit /scope、空行、session 接續、記憶累積、錯誤不中斷）→ Task 7（核心）+ Task 8（main 實機 stdin、錯誤續行）✓
- run_turn 走 run_streaming + on_role → Task 6 ✓
- 無 prompt→REPL、有 prompt→單次 → Task 8 ✓
- stream config（--no-stream / WUKONG_STREAM=0）→ Task 3（旗標）+ Task 4（resolve）✓
- 角色行在文字之前（串流）→ Task 6（on_role 於 route 後回呼）+ Task 8（main 以 on_role 印 header）✓
- 錯誤處理（AgentFailed、壞行忽略、無 TTY）→ Task 2（AgentFailed）、Task 1（壞行 None）、Task 5/8（spinner 為 live、無 TTY 不畫）✓
- 測試策略（parse_event、run_streaming、render、REPL、回歸）→ Task 1/2/5/7/9 ✓

**2. Placeholder scan：** 無 TBD/TODO；每個改碼步驟均附完整程式碼與預期輸出。

**3. Type consistency：**
- `run_turn` 簽名（Task 6 起）：`(memory, backend, cfg, input, on_event: &mut dyn FnMut(StreamEvent), on_role: &mut dyn FnMut(Role))`——所有呼叫端（lib.rs 測試於 Task 6、repl.rs 於 Task 7、main.rs 於 Task 8）皆以此簽名呼叫。
- `GatewayConfig.stream: bool`（Task 4）在 lib.rs `test_cfg`（Task 6）、repl.rs `cfg()`（Task 7）皆補上。
- `StreamEvent`（gateway，Task 1）於 backend（Task 2）、render（Task 5）、repl（Task 7）、main（Task 8）一致使用，經 `wukong_gateway::StreamEvent` re-export。
- `classify_line`/`LineAction`（Task 7）由 main.rs（Task 8）重用，變體名一致（Exit/Skip/SetScope/Turn）。
- `Role::name()`（orchestrator）用於 header；roles 斷言在 Task 7 Step 3 以實際輸出校正（fixer/oracle）。
