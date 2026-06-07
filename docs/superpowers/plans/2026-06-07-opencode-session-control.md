# opencode Session Control Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Drive opencode with persistent per-scope sessions (explicit `-s <id>`) and default `--thinking`, plus `/new` and `/compact` session commands across REPL, Telegram, Web, and one-shot CLI.

**Architecture:** `AgentRequest` carries `session_id`/`thinking`; `AgentResponse` returns the captured `sessionID`. `wukong-memory` stores one opencode session id per scope. `run_turn` threads that id through the final answer step only (planner stays stateless). A shared command engine in `wukong-cli` (`SessionCommand`, `run_session_command`) is wired into every surface.

**Tech Stack:** Rust, tokio, sqlx/SQLite, axum, opencode 1.16.2 (`opencode run -s <id> --thinking --format json`).

**Verification note:** all `cargo`/`clippy` commands assume `~/.cargo/bin/cargo`. Run clippy with `-D warnings`. Tests that hold a `MutexGuard` must not cross `.await` — use a `{ }` block scope (established Wukong gotcha).

---

### Task 1: Backend type migration (session_id + thinking + Reasoning) across the workspace

This is a type change that cascades. Land it all at once so the workspace compiles with existing behavior preserved (no session threading yet; `thinking` resolved from config).

**Files:**
- Modify: `crates/wukong-gateway/src/stream.rs`
- Modify: `crates/wukong-gateway/src/backend.rs`
- Modify: `crates/wukong-gateway/src/summarize.rs`
- Modify: `crates/wukong-gateway/src/config.rs`
- Modify: `crates/wukong-gateway/src/cli.rs`
- Modify: `crates/wukong-orchestrator/src/router.rs`
- Modify: `crates/wukong-cli/src/lib.rs`
- Modify: `crates/wukong-cli/src/render.rs`
- Modify: `crates/wukong-cli/src/repl.rs`
- Modify: `crates/wukong-cli/src/main.rs`
- Modify: `crates/wukong-telegram/src/main.rs`
- Modify: `crates/wukong-telegram/src/dispatch.rs`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Add `Reasoning` event + `parse_session_id` (stream.rs)**

In `crates/wukong-gateway/src/stream.rs`, add the variant to the enum (after `Text`):

```rust
    /// A chunk of reasoning/thinking text (opencode "reasoning" part).
    Reasoning(String),
```

Add a `"reasoning"` arm in `parse_event` (next to `"text"`):

```rust
        "reasoning" => {
            let t = part
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or_default();
            Some(StreamEvent::Reasoning(t.to_string()))
        }
```

Add a new public function below `parse_event`:

```rust
/// Extract the opencode session id from one NDJSON line (top-level `sessionID`).
pub fn parse_session_id(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    v.get("sessionID")?.as_str().map(|s| s.to_string())
}
```

Add tests in the `tests` module:

```rust
    #[test]
    fn parses_reasoning_event() {
        let ev = parse_event(r#"{"type":"reasoning","part":{"type":"reasoning","text":"hmm"}}"#);
        assert_eq!(ev, Some(StreamEvent::Reasoning("hmm".to_string())));
    }

    #[test]
    fn extracts_session_id() {
        assert_eq!(
            parse_session_id(r#"{"type":"text","sessionID":"ses_abc","part":{}}"#),
            Some("ses_abc".to_string())
        );
        assert_eq!(parse_session_id("not json"), None);
        assert_eq!(parse_session_id(r#"{"type":"text"}"#), None);
    }
```

- [ ] **Step 2: Migrate `AgentRequest`/`AgentResponse`/`assemble_argv`/backend (backend.rs)**

Replace the request/response structs:

```rust
/// A request to the AI backend.
#[derive(Debug, Clone)]
pub struct AgentRequest {
    pub prompt: String,
    /// Some(id) → continue this opencode session via `-s <id>`; None → fresh.
    pub session_id: Option<String>,
    /// Pass `--thinking` to surface reasoning blocks.
    pub thinking: bool,
}

/// The backend's textual response.
#[derive(Debug, Clone)]
pub struct AgentResponse {
    pub text: String,
    /// opencode session id captured from the JSON stream (None on the plain path).
    pub session_id: Option<String>,
}
```

Replace `assemble_argv`:

```rust
/// Build the argv handed to the agent subprocess:
/// `command + [-s <id>]? + [--thinking]? + [prompt]`.
pub fn assemble_argv(
    command: &[String],
    session_id: Option<&str>,
    thinking: bool,
    prompt: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = command.to_vec();
    if let Some(id) = session_id {
        argv.push("-s".to_string());
        argv.push(id.to_string());
    }
    if thinking {
        argv.push("--thinking".to_string());
    }
    argv.push(prompt.to_string());
    argv
}
```

Remove the `continue_args` field from `AgentCliBackend`:

```rust
/// Drives a configurable agent CLI as a subprocess (run-and-capture, no shell).
pub struct AgentCliBackend {
    pub command: Vec<String>,
}
```

In `AgentCliBackend::run`, build argv and return a `None` session id:

```rust
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        let argv = assemble_argv(&self.command, req.session_id.as_deref(), req.thinking, &req.prompt);
        let output = Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(Stdio::null())
            .output()
            .await?;
        if !output.status.success() {
            return Err(GatewayError::AgentFailed {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(AgentResponse {
            text: String::from_utf8_lossy(&output.stdout).trim().to_string(),
            session_id: None,
        })
    }
```

In `AgentCliBackend::run_streaming`, change the argv build and capture the session id:

```rust
        let mut argv = assemble_argv(&self.command, req.session_id.as_deref(), req.thinking, &req.prompt);
        let prompt = argv.pop().expect("argv always ends with the prompt");
        argv.push("--format".to_string());
        argv.push("json".to_string());
        argv.push(prompt);
```

Add `use crate::stream::parse_session_id;` to the existing `use crate::stream::{parse_event, StreamEvent};` line (make it `use crate::stream::{parse_event, parse_session_id, StreamEvent};`). Track and set the id in the read loop:

```rust
        let mut lines = BufReader::new(stdout).lines();
        let mut full = String::new();
        let mut session_id: Option<String> = None;
        while let Some(line) = lines.next_line().await? {
            if let Some(id) = parse_session_id(&line) {
                session_id = Some(id);
            }
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
```

And the final `Ok(...)`:

```rust
        Ok(AgentResponse { text: full.trim().to_string(), session_id })
```

The default trait `run_streaming` (which calls `run`) is unchanged — it already forwards the `run` response (now carrying `session_id`).

- [ ] **Step 3: Update backend.rs tests**

Replace the `assemble_argv` tests and the `AgentResponse {...}` / `AgentCliBackend {...}` literals:

```rust
    #[test]
    fn assemble_argv_plain() {
        let argv = assemble_argv(&["opencode".to_string(), "run".to_string()], None, false, "hi");
        assert_eq!(argv, vec!["opencode", "run", "hi"]);
    }

    #[test]
    fn assemble_argv_with_session_and_thinking() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            Some("ses_x"),
            true,
            "hi",
        );
        assert_eq!(argv, vec!["opencode", "run", "-s", "ses_x", "--thinking", "hi"]);
    }
```

In `agent_cli_backend_captures_stdout` and `agent_cli_backend_reports_failure`: change `AgentCliBackend { command: vec![...], continue_args: vec![] }` → `AgentCliBackend { command: vec![...] }`, and the `AgentRequest { prompt: ..., continue_session: false }` → `AgentRequest { prompt: ..., session_id: None, thinking: false }`.

In `run_streaming_default_emits_single_text`: `Ok(AgentResponse { text: "whole answer".to_string() })` → add `session_id: None`; the `AgentRequest { prompt: "x".into(), continue_session: false }` → `session_id: None, thinking: false`.

In `agent_cli_run_streaming_parses_ndjson`: `AgentCliBackend { command: vec![...], continue_args: vec![] }` → drop `continue_args`; `AgentRequest { prompt: "ignored".into(), continue_session: false }` → `session_id: None, thinking: false`. Add a `sessionID` to one of the printf JSON lines and assert capture, e.g. change the `step_start` line to `r#"{"type":"step_start","sessionID":"ses_T"}"#` and after the existing assertions add:

```rust
        assert_eq!(resp.session_id, Some("ses_T".to_string()));
```

- [ ] **Step 4: Update summarize.rs**

`AgentRequest { prompt, continue_session: false }` → `AgentRequest { prompt, session_id: None, thinking: false }`. In its test, `Ok(AgentResponse { text: ... })` → add `session_id: None`.

- [ ] **Step 5: Swap `GatewayConfig` fields (config.rs)**

Replace the struct fields `continue_session`/`continue_args` with `thinking`:

```rust
pub struct GatewayConfig {
    pub scope: String,
    pub db_url: String,
    pub agent_command: Vec<String>,
    /// Pass `--thinking` to opencode for conversational turns. Default true.
    pub thinking: bool,
    pub recall_top_k: usize,
    pub stream: bool,
}
```

In `resolve`, delete the `continue_args` block, and replace the `GatewayConfig { ... }` tail:

```rust
        let stream = !cli.no_stream && std::env::var("WUKONG_STREAM").as_deref() != Ok("0");
        let thinking = !cli.no_thinking && std::env::var("WUKONG_THINKING").as_deref() != Ok("0");

        GatewayConfig {
            scope,
            db_url,
            agent_command,
            thinking,
            recall_top_k: 5,
            stream,
        }
```

Update config tests: in `cli_overrides_take_priority`, remove `assert!(cfg.continue_session);` and `assert_eq!(cfg.continue_args, ...)`; add `assert!(cfg.thinking);` and keep the `-c`→ now drop the `"-c", "hi"` args (see Step 6 — `-c` is removed). Change that test's args to end with a positional prompt, e.g. replace `"-c", "hi"` with just `"hi"`. The `no_stream_flag_disables_stream` and `default_scope`/`split_ws` tests are unaffected.

- [ ] **Step 6: Update CLI flags (cli.rs)**

Remove the `continue_session` field/flag; add `--no-thinking` and `--new`:

```rust
    /// Disable activity rendering (spinner + tool events); use plain capture.
    #[arg(long = "no-stream")]
    pub no_stream: bool,

    /// Disable opencode reasoning/thinking output.
    #[arg(long = "no-thinking")]
    pub no_thinking: bool,

    /// Start a fresh opencode session for this scope before the turn.
    #[arg(long = "new")]
    pub new_session: bool,
```

Update tests: in `parses_prompt_and_flags`, remove `"-c"` from args and `assert!(cli.continue_session)`. Add:

```rust
    #[test]
    fn no_thinking_and_new_flags_parse() {
        let cli = Cli::try_parse_from(["wukong", "--no-thinking", "--new", "hi"]).unwrap();
        assert!(cli.no_thinking);
        assert!(cli.new_session);
        assert_eq!(cli.prompt_text(), "hi");
    }
```

- [ ] **Step 7: Update orchestrator router.rs**

In `route` and `plan_chain`, `AgentRequest { prompt: ..., continue_session: false }` → `AgentRequest { prompt: ..., session_id: None, thinking: false }` (planner is stateless and needs no thinking).

- [ ] **Step 8: Make run_turn compile (cli/lib.rs)**

In `run_turn`, replace the `AgentRequest` built for the execute call:

```rust
        let resp = backend
            .run_streaming(
                AgentRequest {
                    prompt,
                    session_id: None,
                    thinking: cfg.thinking,
                },
                on_event,
            )
            .await?;
```

(Real session threading lands in Task 3.) Update `run_turn` tests' `MockBackend::run` to return `AgentResponse { text, session_id: None }`, and `test_cfg` `GatewayConfig { ... }` to use `thinking: true` instead of `continue_session`/`continue_args` (remove those two fields). Remove any assertions on `continue_session`/`continue_args`.

- [ ] **Step 9: Handle `Reasoning` in render.rs (exhaustive match)**

Add an arm in `StreamRenderer::on_event` before the step arm:

```rust
            StreamEvent::Reasoning(t) => {
                let _ = writeln!(self.err, "  💭 {t}");
                let _ = self.err.flush();
            }
```

- [ ] **Step 10: Update repl.rs tests + main.rs + telegram + web to compile**

- `crates/wukong-cli/src/repl.rs`: `MockBackend::run` returns `AgentResponse { text, session_id: None }` (the struct also tracks `continue_flags` from `req.continue_session` — replace that field's use: drop `continue_flags` entirely and the `loop_runs_turns_until_exit_and_continues_session` assertion on `flags`; keep the turn-count and roles assertions). `cfg()` `GatewayConfig` → `thinking: true` (drop continue fields). In `run_repl_loop`, delete the two `cfg.continue_session = ...` lines (continuation is now via memory; Task 3/5 revisit).
- `crates/wukong-cli/src/main.rs`: `AgentCliBackend { command: ..., continue_args: ... }` → `{ command: cfg.agent_command.clone() }`. In the REPL loop, delete `cfg_repl.continue_session = false;` and `cfg_repl.continue_session = !first; first = false;` (keep the `first` var removal too). The inline sink in `run_one` (both branches): add before `_ => {}`:

```rust
            StreamEvent::Reasoning(t) => {
                eprintln!("  💭 {t}");
            }
```

- `crates/wukong-telegram/src/main.rs`: `AgentCliBackend { command: agent_command, continue_args: vec![] }` → `{ command: agent_command }`; `base_cfg` `GatewayConfig { ... continue_session: false, ... }` → replace `continue_session: false,` with `thinking: true,` and remove `continue_args`/any `agent_command: vec![]` stays.
- `crates/wukong-telegram/src/dispatch.rs` tests: `MockBackend::run` → `AgentResponse { text, session_id: None }`; `base_cfg()` `GatewayConfig` → `thinking: true` (drop continue fields).
- `crates/wukong-web/src/main.rs`: `AgentCliBackend { command: agent_command, continue_args: vec![] }` → `{ command: agent_command }`.
- `crates/wukong-web/src/lib.rs`: the inline `GatewayConfig { ... continue_session: false, continue_args: vec![], ... }` in `chat` → replace those two with `thinking: true,`. Tests' `MockBackend::run` → `AgentResponse { text, session_id: None }`.

- [ ] **Step 11: Build and test the whole workspace**

Run: `~/.cargo/bin/cargo test --workspace 2>&1 | tail -30`
Expected: PASS (behavior preserved; new stream tests green).
Run: `~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -10`
Expected: no warnings.

- [ ] **Step 12: Commit**

```bash
git add -A
git commit -m "refactor(gateway): explicit session_id + thinking on AgentRequest; capture sessionID"
```

---

### Task 2: Per-scope session-id store (wukong-memory)

**Files:**
- Modify: `crates/wukong-memory/src/store/mod.rs`
- Modify: `crates/wukong-memory/src/lib.rs`

- [ ] **Step 1: Write the failing test (lib.rs tests)**

Add to the test module in `crates/wukong-memory/src/lib.rs` (find the existing `#[cfg(test)] mod tests`; if helpers like `open_memory` exist there, reuse them — otherwise use the `Memory::open` with a NamedTempFile pattern used by other crates):

```rust
    #[tokio::test]
    async fn agent_session_round_trip() {
        let mem = Memory::open("sqlite::memory:").await.unwrap();
        assert_eq!(mem.agent_session("global").await.unwrap(), None);
        mem.set_agent_session("global", "ses_1").await.unwrap();
        assert_eq!(mem.agent_session("global").await.unwrap(), Some("ses_1".to_string()));
        // UPSERT overwrites.
        mem.set_agent_session("global", "ses_2").await.unwrap();
        assert_eq!(mem.agent_session("global").await.unwrap(), Some("ses_2".to_string()));
        // Clear, including a no-op clear of an unknown scope.
        mem.clear_agent_session("global").await.unwrap();
        assert_eq!(mem.agent_session("global").await.unwrap(), None);
        mem.clear_agent_session("nope").await.unwrap();
    }
```

(If `sqlite::memory:` is not used elsewhere, mirror the existing tests' temp-file open instead.)

- [ ] **Step 2: Run the test to verify it fails**

Run: `~/.cargo/bin/cargo test -p wukong-memory agent_session_round_trip 2>&1 | tail -15`
Expected: FAIL — `no method named agent_session`.

- [ ] **Step 3: Add the table to SCHEMA (store/mod.rs)**

Append inside the `SCHEMA` raw string (after the `memories_ad` trigger):

```sql
CREATE TABLE IF NOT EXISTS agent_sessions (
    scope       TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);
```

- [ ] **Step 4: Add Store methods (store/mod.rs)**

Add to `impl Store` (near `delete_memories`):

```rust
    /// Read the stored opencode session id for a scope.
    pub async fn agent_session(&self, scope: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT session_id FROM agent_sessions WHERE scope = ?1")
            .bind(scope)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.map(|r| r.get::<String, _>("session_id")))
    }

    /// Upsert the opencode session id for a scope.
    pub async fn set_agent_session(&self, scope: &str, session_id: &str, now: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO agent_sessions(scope, session_id, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(scope) DO UPDATE SET session_id = ?2, updated_at = ?3",
        )
        .bind(scope)
        .bind(session_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Remove any stored session id for a scope (no-op if absent).
    pub async fn clear_agent_session(&self, scope: &str) -> Result<()> {
        sqlx::query("DELETE FROM agent_sessions WHERE scope = ?1")
            .bind(scope)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
```

(Confirm `use sqlx::Row;` is already imported in this file — it is, per the existing `r.get` usage.)

- [ ] **Step 5: Add Memory delegators (lib.rs)**

Find the helper that produces the current epoch seconds (`now_unix()`, used by `remember`). Add to `impl Memory`:

```rust
    /// Stored opencode session id for a scope (None if none).
    pub async fn agent_session(&self, scope: &str) -> Result<Option<String>> {
        self.store.agent_session(scope).await
    }

    /// Set/overwrite the opencode session id for a scope.
    pub async fn set_agent_session(&self, scope: &str, session_id: &str) -> Result<()> {
        self.store.set_agent_session(scope, session_id, now_unix()).await
    }

    /// Clear the stored opencode session id for a scope.
    pub async fn clear_agent_session(&self, scope: &str) -> Result<()> {
        self.store.clear_agent_session(scope).await
    }
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `~/.cargo/bin/cargo test -p wukong-memory agent_session_round_trip 2>&1 | tail -15`
Expected: PASS.
Run: `~/.cargo/bin/cargo clippy -p wukong-memory --all-targets -- -D warnings 2>&1 | tail -8`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/wukong-memory
git commit -m "feat(memory): per-scope opencode session-id store"
```

---

### Task 3: Thread the session through run_turn's final step

**Files:**
- Modify: `crates/wukong-cli/src/lib.rs`

- [ ] **Step 1: Write the failing test (lib.rs tests)**

Add a test that the stored session is passed into the final step and the returned id is persisted. The `MockBackend` must record `req.session_id` and return a fixed id. Update `MockBackend` to also capture session ids and emit a session id:

In the `MockBackend` struct add `session_ids: Mutex<Vec<Option<String>>>` (init empty in `new`), and in `run` push `req.session_id.clone()` and return `AgentResponse { text, session_id: Some("ses_new".to_string()) }`.

Then add:

```rust
    #[tokio::test]
    async fn run_turn_threads_session_into_final_step() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        // planner -> single role; execute returns text.
        let backend = MockBackend::new(&["oracle", "answer"]);
        run_turn(&mem, &backend, &test_cfg("project:T"), "hi", &mut |_| {}, &mut |_| {})
            .await
            .unwrap();
        {
            let ids = backend.session_ids.lock().unwrap();
            // [0] planner = None, [1] final execute = stored session.
            assert_eq!(ids[0], None);
            assert_eq!(ids[1], Some("ses_old".to_string()));
        }
        // Returned id persisted.
        assert_eq!(mem.agent_session("project:T").await.unwrap(), Some("ses_new".to_string()));
    }

    #[tokio::test]
    async fn run_turn_threads_only_final_chain_step() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_old").await.unwrap();
        // planner -> explorer, fixer ; explorer output ; fixer output (final).
        let backend = MockBackend::new(&["explorer, fixer", "e1", "f2"]);
        run_turn(&mem, &backend, &test_cfg("project:T"), "go", &mut |_| {}, &mut |_| {})
            .await
            .unwrap();
        let ids = backend.session_ids.lock().unwrap();
        // [0] planner None, [1] explorer None, [2] fixer (final) = stored.
        assert_eq!(ids, &vec![None, None, Some("ses_old".to_string())]);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `~/.cargo/bin/cargo test -p wukong-cli run_turn_threads 2>&1 | tail -20`
Expected: FAIL (final step currently passes `session_id: None`; nothing persisted).

- [ ] **Step 3: Implement threading in run_turn**

Before the role loop, fetch the stored session and compute the final index:

```rust
    // 3. Run each role in order, accumulating prior outputs into the prompt.
    let stored = memory.agent_session(&cfg.scope).await?;
    let n_roles = roles.len();
    let mut prior: Vec<wukong_orchestrator::Outcome> = Vec::new();
    let mut captured_session: Option<String> = None;
    for (i, role) in roles.into_iter().enumerate() {
        on_role(role);
        let augmented = format!("{input}{}", wukong_orchestrator::chain_context(&prior));
        let prompt = persona::build_prompt(role, &recall.data, &augmented);
        let is_final = i + 1 == n_roles;
        let session_id = if is_final { stored.clone() } else { None };
        let resp = backend
            .run_streaming(
                AgentRequest { prompt, session_id, thinking: cfg.thinking },
                on_event,
            )
            .await?;
        if is_final {
            captured_session = resp.session_id.clone();
        }
        prior.push(wukong_orchestrator::Outcome { role, output: resp.text });
    }

    // Persist the (possibly new) opencode session id for this scope.
    if let Some(id) = captured_session {
        memory.set_agent_session(&cfg.scope, &id).await?;
    }
```

(Replace the existing `for role in roles { ... first ... }` block, including removing the now-unused `first` variable.)

- [ ] **Step 4: Run to verify pass**

Run: `~/.cargo/bin/cargo test -p wukong-cli 2>&1 | tail -20`
Expected: PASS (all cli tests, including the two new ones).
Run: `~/.cargo/bin/cargo clippy -p wukong-cli --all-targets -- -D warnings 2>&1 | tail -8`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-cli
git commit -m "feat(cli): thread per-scope opencode session through run_turn's final step"
```

---

### Task 4: Shared command engine (`SessionCommand`, `run_session_command`)

**Files:**
- Create: `crates/wukong-cli/src/command.rs`
- Modify: `crates/wukong-cli/src/lib.rs` (module decl + re-export)

- [ ] **Step 1: Create the module with failing tests**

Create `crates/wukong-cli/src/command.rs`:

```rust
//! Session control commands shared by every surface (REPL, Telegram, Web, CLI).

use crate::{run_turn_session_passthrough, WukongError};
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_memory::Memory;

/// A session control command parsed from a leading-slash input.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    /// Start a fresh opencode session for this scope.
    New,
    /// Passthrough `/compact` to the current opencode session.
    Compact,
}

/// Map a command name (without the leading '/') to a SessionCommand.
pub fn parse_session_command(name: &str) -> Option<SessionCommand> {
    match name {
        "new" => Some(SessionCommand::New),
        "compact" => Some(SessionCommand::Compact),
        _ => None,
    }
}

/// Execute a session command, returning user-facing reply text.
pub async fn run_session_command(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    cmd: SessionCommand,
) -> Result<String, WukongError> {
    match cmd {
        SessionCommand::New => {
            memory.clear_agent_session(&cfg.scope).await?;
            Ok("🐵 已開新 context".to_string())
        }
        SessionCommand::Compact => {
            match memory.agent_session(&cfg.scope).await? {
                None => Ok("🐵 尚無對話可壓縮".to_string()),
                Some(id) => {
                    let text = run_turn_session_passthrough(backend, &id).await?;
                    Ok(format!("🐵 已送出壓縮指令：\n{text}"))
                }
            }
        }
    }
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
        prompts: Mutex<Vec<String>>,
        sessions: Mutex<Vec<Option<String>>>,
    }
    impl MockBackend {
        fn new(r: &[&str]) -> Self {
            Self {
                replies: Mutex::new(r.iter().map(|s| s.to_string()).collect()),
                prompts: Mutex::new(Vec::new()),
                sessions: Mutex::new(Vec::new()),
            }
        }
    }
    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.prompts.lock().unwrap().push(req.prompt);
            self.sessions.lock().unwrap().push(req.session_id);
            let text = self.replies.lock().unwrap().pop_front().unwrap_or_default();
            Ok(AgentResponse { text, session_id: None })
        }
    }

    async fn open_memory() -> Memory {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        Memory::open(&url).await.unwrap()
    }

    fn cfg() -> GatewayConfig {
        GatewayConfig {
            scope: "global".to_string(),
            db_url: String::new(),
            agent_command: vec![],
            thinking: true,
            recall_top_k: 5,
            stream: false,
        }
    }

    #[test]
    fn parses_known_commands() {
        assert_eq!(parse_session_command("new"), Some(SessionCommand::New));
        assert_eq!(parse_session_command("compact"), Some(SessionCommand::Compact));
        assert_eq!(parse_session_command("model"), None);
    }

    #[tokio::test]
    async fn new_clears_session() {
        let mem = open_memory().await;
        mem.set_agent_session("global", "ses_1").await.unwrap();
        let backend = MockBackend::new(&[]);
        let reply = run_session_command(&mem, &backend, &cfg(), SessionCommand::New).await.unwrap();
        assert!(reply.contains("已開新"));
        assert_eq!(mem.agent_session("global").await.unwrap(), None);
        assert!(backend.prompts.lock().unwrap().is_empty()); // no model call
    }

    #[tokio::test]
    async fn compact_without_session_does_not_call_backend() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["ignored"]);
        let reply = run_session_command(&mem, &backend, &cfg(), SessionCommand::Compact).await.unwrap();
        assert!(reply.contains("尚無對話"));
        assert!(backend.prompts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn compact_passes_slash_compact_to_stored_session() {
        let mem = open_memory().await;
        mem.set_agent_session("global", "ses_42").await.unwrap();
        let backend = MockBackend::new(&["compacted ok"]);
        let reply = run_session_command(&mem, &backend, &cfg(), SessionCommand::Compact).await.unwrap();
        assert!(reply.contains("compacted ok"));
        {
            let prompts = backend.prompts.lock().unwrap();
            assert_eq!(prompts.len(), 1);
            assert_eq!(prompts[0], "/compact");
        }
        let sessions = backend.sessions.lock().unwrap();
        assert_eq!(sessions[0], Some("ses_42".to_string()));
    }
}
```

- [ ] **Step 2: Add the passthrough helper + module wiring (lib.rs)**

In `crates/wukong-cli/src/lib.rs`, add the module declaration near the top module list:

```rust
pub mod command;
```

Re-export the command API (so surfaces call `wukong_cli::parse_session_command` / `run_session_command`):

```rust
pub use command::{parse_session_command, run_session_command, SessionCommand};
```

Add the passthrough helper (used by `/compact`) at the end of `lib.rs`:

```rust
/// Send a raw `/compact` message to a specific opencode session (no planner,
/// no persona) and return its text reply.
pub async fn run_turn_session_passthrough(
    backend: &impl AiBackend,
    session_id: &str,
) -> Result<String, WukongError> {
    let resp = backend
        .run_streaming(
            AgentRequest {
                prompt: "/compact".to_string(),
                session_id: Some(session_id.to_string()),
                thinking: false,
            },
            &mut |_| {},
        )
        .await?;
    Ok(resp.text)
}
```

(`AgentRequest` and `AiBackend` are already imported in `lib.rs`.)

- [ ] **Step 3: Run tests**

Run: `~/.cargo/bin/cargo test -p wukong-cli command:: 2>&1 | tail -20`
Expected: PASS.
Run: `~/.cargo/bin/cargo clippy -p wukong-cli --all-targets -- -D warnings 2>&1 | tail -8`
Expected: no warnings.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-cli
git commit -m "feat(cli): shared /new + /compact session command engine"
```

---

### Task 5: Wire commands into the REPL

**Files:**
- Modify: `crates/wukong-cli/src/repl.rs`
- Modify: `crates/wukong-cli/src/main.rs`

- [ ] **Step 1: Write the failing test (repl.rs)**

Add a `LineAction::Command(SessionCommand)` case and a loop test. First add to the test module:

```rust
    #[test]
    fn classify_line_recognizes_session_commands() {
        assert_eq!(classify_line("/new"), LineAction::Command(SessionCommand::New));
        assert_eq!(classify_line("/compact"), LineAction::Command(SessionCommand::Compact));
        assert_eq!(classify_line("/model gpt"), LineAction::Skip); // unknown slash → skip
    }

    #[tokio::test]
    async fn loop_runs_new_command_and_clears_session() {
        let mem = open_memory().await;
        mem.set_agent_session("project:T", "ses_1").await.unwrap();
        let backend = MockBackend::new(&[]);
        let turns = run_repl_loop(
            &mem, &backend, &cfg(),
            vec!["/new".to_string(), "/exit".to_string()],
            &mut |_| {}, &mut |_| {},
        )
        .await
        .unwrap();
        assert_eq!(turns, 0);
        assert_eq!(mem.agent_session("project:T").await.unwrap(), None);
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `~/.cargo/bin/cargo test -p wukong-cli classify_line_recognizes 2>&1 | tail -15`
Expected: FAIL — `Command` variant missing.

- [ ] **Step 3: Extend LineAction + classify_line + loop (repl.rs)**

Add `use crate::command::{parse_session_command, run_session_command, SessionCommand};` to the imports.

Add the variant:

```rust
pub enum LineAction {
    Exit,
    Skip,
    SetScope(String),
    Command(SessionCommand),
    Turn(String),
}
```

In `classify_line`, before the final `else` that returns `Turn`, handle other slashes:

```rust
            } else if let Some(rest) = t.strip_prefix('/') {
                let name = rest.split_whitespace().next().unwrap_or("");
                match parse_session_command(name) {
                    Some(cmd) => LineAction::Command(cmd),
                    None => LineAction::Skip, // unknown meta-command
                }
            } else {
                LineAction::Turn(t.to_string())
            }
```

In `run_repl_loop`, add the match arm:

```rust
            LineAction::Command(cmd) => {
                let reply = run_session_command(memory, backend, &cfg, cmd).await?;
                on_event(StreamEvent::Text(format!("{reply}\n")));
            }
```

- [ ] **Step 4: Run to verify pass**

Run: `~/.cargo/bin/cargo test -p wukong-cli 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Wire main.rs REPL loop**

In `crates/wukong-cli/src/main.rs`, add `LineAction::Command` handling in the inline REPL `match classify_line(&line)`:

```rust
                LineAction::Command(cmd) => {
                    match wukong_cli::run_session_command(&memory, &backend, &cfg_repl, cmd).await {
                        Ok(reply) => println!("{reply}"),
                        Err(e) => eprintln!("error: {e}"),
                    }
                }
```

Add `LineAction` is already imported; ensure `use wukong_cli::repl::{classify_line, LineAction};` remains. (No automated test for main.rs; covered by manual smoke.)

- [ ] **Step 6: clippy + commit**

Run: `~/.cargo/bin/cargo clippy -p wukong-cli --all-targets -- -D warnings 2>&1 | tail -8`
Expected: no warnings.

```bash
git add crates/wukong-cli
git commit -m "feat(cli): /new and /compact in the REPL"
```

---

### Task 6: Wire commands into Telegram

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs`

- [ ] **Step 1: Write the failing test (dispatch.rs tests)**

```rust
    #[tokio::test]
    async fn new_command_clears_session_and_replies() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        mem.set_agent_session(&scope_for_chat(12), "ses_1").await.unwrap();
        let backend = MockBackend::new(&[]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "/new".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| s.text.contains("已開新")));
        assert_eq!(mem.agent_session(&scope_for_chat(12)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn unknown_command_still_unsupported() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "/model gpt".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| s.text.contains("尚未支援")));
    }
```

- [ ] **Step 2: Run to verify failure**

Run: `~/.cargo/bin/cargo test -p wukong-telegram new_command_clears 2>&1 | tail -15`
Expected: FAIL (current code always replies "尚未支援", session not cleared).

- [ ] **Step 3: Implement the Command branch**

Replace the `MessageAction::Command { name, .. }` arm in `handle_message`:

```rust
        MessageAction::Command { name, .. } => {
            let mut cfg = base_cfg.clone();
            cfg.scope = scope_for_chat(chat_id);
            match wukong_cli::parse_session_command(&name) {
                Some(cmd) => {
                    let reply = match wukong_cli::run_session_command(mem, backend, &cfg, cmd).await {
                        Ok(t) => t,
                        Err(e) => format!("⚠️ 失敗：{e}"),
                    };
                    let _ = client.send_message(chat_id, &reply).await;
                }
                None => {
                    let _ = client
                        .send_message(chat_id, &format!("指令 /{name} 尚未支援"))
                        .await;
                }
            }
        }
```

- [ ] **Step 4: Run + clippy**

Run: `~/.cargo/bin/cargo test -p wukong-telegram 2>&1 | tail -20`
Expected: PASS.
Run: `~/.cargo/bin/cargo clippy -p wukong-telegram --all-targets -- -D warnings 2>&1 | tail -8`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-telegram
git commit -m "feat(telegram): /new and /compact via shared command engine"
```

---

### Task 7: Wire commands into the Web console

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Write the failing test (web tests)**

```rust
    #[tokio::test]
    async fn chat_new_command_clears_session() {
        let app_state = state(None, &[]).await;
        app_state.memory.set_agent_session("global", "ses_1").await.unwrap();
        let app = build_router(app_state.clone());
        let resp = app
            .oneshot(Request::builder().uri("/chat?q=/new").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("event: answer"), "missing answer:\n{body}");
        assert!(body.contains("已開新"), "missing reply:\n{body}");
        assert!(!body.contains("event: role"), "should not run a turn:\n{body}");
        assert!(body.contains("event: done"));
        assert_eq!(app_state.memory.agent_session("global").await.unwrap(), None);
    }
```

(`AppState` derives `Clone` already; `state(...)` returns it by value — clone before moving into `build_router`.)

- [ ] **Step 2: Run to verify failure**

Run: `~/.cargo/bin/cargo test -p wukong-web chat_new_command 2>&1 | tail -15`
Expected: FAIL (slash currently treated as a normal turn → role events present, session not cleared).

- [ ] **Step 3: Implement slash dispatch inside the spawned thread**

In the `chat` handler, inside `rt.block_on(async move { ... })`, right after the `let cfg = GatewayConfig { ... };` line and before the existing `run_turn` call, add the slash branch:

```rust
                let trimmed = q.trim();
                if let Some(rest) = trimmed.strip_prefix('/') {
                    let name = rest.split_whitespace().next().unwrap_or("").to_string();
                    let reply = match wukong_cli::parse_session_command(&name) {
                        Some(cmd) => match wukong_cli::run_session_command(mem.as_ref(), backend.as_ref(), &cfg, cmd).await {
                            Ok(t) => t,
                            Err(e) => format!("⚠️ 失敗：{e}"),
                        },
                        None => format!("指令 /{name} 尚未支援"),
                    };
                    let _ = tx.send(SseMsg::Answer(wukong_render::to_web_html(&reply)));
                    let _ = tx.send(SseMsg::Done);
                    return;
                }
```

(The `role_tx`/`run_turn` block below stays for non-slash input. Note `cfg` must be declared before this branch; if it currently lives after, move its declaration above.)

- [ ] **Step 4: Run + clippy**

Run: `~/.cargo/bin/cargo test -p wukong-web 2>&1 | tail -20`
Expected: PASS.
Run: `~/.cargo/bin/cargo clippy -p wukong-web --all-targets -- -D warnings 2>&1 | tail -8`
Expected: no warnings.

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-web
git commit -m "feat(web): /new and /compact via leading-slash dispatch over SSE"
```

---

### Task 8: One-shot CLI `--new` wiring, docs, finish

**Files:**
- Modify: `crates/wukong-cli/src/main.rs`
- Modify: `README.md`

- [ ] **Step 1: Honor `--new` for one-shot and REPL start (main.rs)**

After `memory` is built and before dispatching the prompt/REPL, clear the session if requested:

```rust
    if cli.new_session {
        if let Err(e) = memory.clear_agent_session(&cfg.scope).await {
            eprintln!("warning: failed to reset session: {e}");
        }
    }
```

(Place this after the `memory` bindings and before the `if let Some(Command::Memory ...)` block. For one-shot it resets before the turn; harmless for REPL/memory ops.)

- [ ] **Step 2: Build + smoke the binary help**

Run: `~/.cargo/bin/cargo build -p wukong-cli 2>&1 | tail -8`
Expected: compiles.
Run: `~/.cargo/bin/cargo run -p wukong-cli -- --help 2>&1 | grep -E "new|thinking|stream"`
Expected: shows `--new`, `--no-thinking`, `--no-stream`.

- [ ] **Step 3: Update README**

In `README.md`, add to the development/usage area (near the REPL or Telegram sections) a short block (Taiwan Traditional Chinese, matching the file):

```markdown
### opencode session 控制

- 預設以**每 scope 持久的 opencode session** 接續對話,並帶 `--thinking`。
- `/new`:開新 context(清掉該 scope 的 session)。REPL/Telegram/Web 皆可;一次性 CLI 用 `wukong --new "…"`。
- `/compact`:把 `/compact` passthrough 給目前 session(REPL/Telegram/Web)。
- `--no-thinking` 或 `WUKONG_THINKING=0` 關閉 thinking。
```

- [ ] **Step 4: Manual opencode smoke (if available)**

Run the REPL with real opencode and verify: two turns continue the same session (second answer recalls the first); `/new` resets; `/compact` returns a reply; `--thinking` shows `💭` activity lines. If opencode is unavailable here, note it and rely on the automated suite.

```bash
WUKONG_AGENT_CMD="opencode run" ~/.cargo/bin/cargo run -p wukong-cli
```

- [ ] **Step 5: Commit docs + main wiring**

```bash
git add crates/wukong-cli/src/main.rs README.md
git commit -m "feat(cli): --new one-shot session reset; document session control"
```

- [ ] **Step 6: Full workspace verification**

Run: `~/.cargo/bin/cargo test --workspace 2>&1 | tail -30`
Expected: all pass.
Run: `~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -10`
Expected: no warnings.

- [ ] **Step 7: Finish the branch**

Announce: "I'm using the finishing-a-development-branch skill to complete this work." Then follow superpowers:finishing-a-development-branch (verify tests → present options → execute choice). Expect the Wukong cadence: merge → push → 孫悟空-themed release (next version after v0.8.0).

---

## Self-Review

**Spec coverage:**
- Backend `AgentRequest{session_id,thinking}` / `AgentResponse{session_id}` / `assemble_argv` / capture / `StreamEvent::Reasoning` → Task 1. ✔
- Session-id store table + `agent_session`/`set`/`clear` → Task 2. ✔
- run_turn: planner stateless, final-step-only threading, store returned id → Task 3. ✔
- Command engine `SessionCommand`/`parse_session_command`/`run_session_command` (New clears; Compact passthrough `/compact` no `--thinking`) → Task 4. ✔
- Surfaces: REPL (Task 5), Telegram (Task 6), Web (Task 7), one-shot CLI `--new`/`--no-thinking` + removed `-c` (Tasks 1, 8). ✔
- Thinking display in REPL (`render.rs` + main.rs sink) → Task 1 (steps 9, 10). ✔
- GatewayConfig `thinking` (default on, `--no-thinking`/`WUKONG_THINKING=0`) → Task 1. ✔
- Error handling: `/compact` no session → friendly reply (Task 4); missing sessionID → not stored (Task 3 only stores `Some`); failures bubble as `WukongError` (Tasks 4/6/7 surface). ✔
- Tests: gateway (argv/session/reasoning/capture), memory (round-trip), cli (threading + command engine), repl/telegram/web wiring → all tasks. ✔
- Non-goals (orphan cleanup, every-role threading, TG/Web thinking display, `/model`, configurable compact message) — none implemented. ✔

**Placeholder scan:** no TBD/TODO; every code step shows full code; the one cross-task reference (`run_turn_session_passthrough`) is defined in Task 4 Step 2.

**Type consistency:** `AgentRequest { prompt, session_id: Option<String>, thinking: bool }` and `AgentResponse { text, session_id: Option<String> }` used identically in Tasks 1/3/4. `GatewayConfig` (with `thinking`, no `continue_session`/`continue_args`) consistent across Tasks 1/3/4 and all surface literals. `SessionCommand { New, Compact }`, `parse_session_command`, `run_session_command`, `run_turn_session_passthrough` signatures match between definition (Task 4) and callers (Tasks 5/6/7). Memory methods `agent_session`/`set_agent_session`/`clear_agent_session` consistent (Task 2 def; Tasks 3/4/5/6/7/8 callers). `LineAction::Command(SessionCommand)` consistent (Task 5).
