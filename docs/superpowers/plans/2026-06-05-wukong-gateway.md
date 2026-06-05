# wukong-gateway v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `wukong-gateway` v1 CLI assistant — a one-shot `wukong` command that recalls relevant memory, drives a configurable agent CLI (default `opencode run`), prints the response, and persists the turn back into `wukong-memory`.

**Architecture:** A third workspace crate (`wukong-gateway`, lib + bin `wukong`). The `cli`/`config` modules resolve settings; `backend` drives the agent subprocess behind an `AiBackend` trait; `prompt` composes the memory-augmented prompt; `pipeline::run_turn` orchestrates recall → prompt → backend → remember using `wukong-memory` directly as a library.

**Tech Stack:** Rust, tokio, clap (derive), thiserror, `wukong-memory` (path dep). Dev: tempfile.

---

## File Structure

```
crates/wukong-gateway/
├── Cargo.toml              # lib + [[bin]] name="wukong"
└── src/
    ├── lib.rs              # module wiring + re-exports
    ├── error.rs           # GatewayError
    ├── cli.rs              # clap args + prompt_text()
    ├── config.rs           # GatewayConfig::resolve + default helpers
    ├── backend.rs          # AiBackend trait + assemble_argv + AgentCliBackend
    ├── prompt.rs           # compose_prompt
    ├── pipeline.rs         # run_turn orchestration + in-crate integration tests
    └── main.rs             # bin entrypoint (thin)
```

> Note: this adds an `error.rs` not drawn in the spec's layout sketch — `GatewayError` is referenced by both `backend` and `pipeline`, so it gets its own module (mirrors `wukong-memory`). Everything else matches the spec.

Each unit has one responsibility: `backend` owns the subprocess, `prompt` owns string assembly, `pipeline` owns one-turn orchestration, `cli`/`config` own settings.

---

## Task 1: Add crate to workspace and scaffold

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/wukong-gateway/Cargo.toml`
- Create: `crates/wukong-gateway/src/lib.rs`
- Create: `crates/wukong-gateway/src/main.rs`

- [ ] **Step 1: Add the crate as a workspace member and add clap**

In the root `Cargo.toml`, change the `members` line to include the new crate:

```toml
members = ["crates/wukong-memory", "crates/wukong-memoryd", "crates/wukong-gateway"]
```

And add `clap` to `[workspace.dependencies]` (leave the existing entries untouched):

```toml
clap = { version = "4", features = ["derive"] }
```

- [ ] **Step 2: Create the crate manifest**

Create `crates/wukong-gateway/Cargo.toml`:

```toml
[package]
name = "wukong-gateway"
edition.workspace = true
version.workspace = true

[lib]
name = "wukong_gateway"
path = "src/lib.rs"

[[bin]]
name = "wukong"
path = "src/main.rs"

[dependencies]
wukong-memory = { path = "../wukong-memory" }
tokio = { workspace = true }
clap = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Create a minimal lib.rs with a smoke test**

Create `crates/wukong-gateway/src/lib.rs`:

```rust
//! wukong-gateway: CLI assistant gateway over wukong-memory.

#[cfg(test)]
mod smoke_tests {
    #[test]
    fn crate_builds() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 4: Create a placeholder main.rs**

Create `crates/wukong-gateway/src/main.rs`:

```rust
fn main() {
    println!("wukong placeholder");
}
```

- [ ] **Step 5: Verify the workspace builds and the smoke test passes**

Run: `cargo test -p wukong-gateway`
Expected: compiles; `smoke_tests::crate_builds` passes.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/wukong-gateway
git commit -m "chore: scaffold wukong-gateway crate"
```

---

## Task 2: Error type

**Files:**
- Create: `crates/wukong-gateway/src/error.rs`
- Modify: `crates/wukong-gateway/src/lib.rs`

- [ ] **Step 1: Write the error module with a unit test**

Create `crates/wukong-gateway/src/error.rs`:

```rust
use thiserror::Error;

/// All errors produced by the gateway.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("memory error: {0}")]
    Memory(#[from] wukong_memory::MemoryError),
    #[error("agent command failed (code {code:?}): {stderr}")]
    AgentFailed { code: Option<i32>, stderr: String },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_failed_message_includes_stderr() {
        let err = GatewayError::AgentFailed {
            code: Some(2),
            stderr: "boom".to_string(),
        };
        assert!(err.to_string().contains("boom"));
        assert!(err.to_string().contains("2"));
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-gateway/src/lib.rs` with:

```rust
//! wukong-gateway: CLI assistant gateway over wukong-memory.

pub mod error;

pub use error::GatewayError;
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cargo test -p wukong-gateway error::`
Expected: `agent_failed_message_includes_stderr` passes.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-gateway/src/error.rs crates/wukong-gateway/src/lib.rs
git commit -m "feat(gateway): add GatewayError type"
```

---

## Task 3: CLI parsing

**Files:**
- Create: `crates/wukong-gateway/src/cli.rs`
- Modify: `crates/wukong-gateway/src/lib.rs`

- [ ] **Step 1: Write the CLI module with parsing tests**

Create `crates/wukong-gateway/src/cli.rs`:

```rust
use clap::Parser;

/// Wukong assistant gateway (one-shot CLI).
#[derive(Parser, Debug)]
#[command(name = "wukong", about = "Wukong assistant gateway (CLI)")]
pub struct Cli {
    /// The prompt to send to the assistant (joined with spaces).
    #[arg(required = true, num_args = 1..)]
    pub prompt: Vec<String>,

    /// Continue the previous agent session (passes the continue flag through).
    #[arg(short = 'c', long = "continue")]
    pub continue_session: bool,

    /// Override the memory scope (e.g. "project:Foo", "global").
    #[arg(long)]
    pub scope: Option<String>,

    /// Override the memory database URL.
    #[arg(long)]
    pub db: Option<String>,

    /// Override the agent command (whitespace-separated, e.g. "opencode run").
    #[arg(long = "agent-cmd")]
    pub agent_cmd: Option<String>,
}

impl Cli {
    /// Join the positional prompt words back into a single string.
    pub fn prompt_text(&self) -> String {
        self.prompt.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prompt_and_flags() {
        let cli = Cli::try_parse_from([
            "wukong", "-c", "--scope", "global", "hello", "world",
        ])
        .unwrap();
        assert_eq!(cli.prompt_text(), "hello world");
        assert!(cli.continue_session);
        assert_eq!(cli.scope.as_deref(), Some("global"));
    }

    #[test]
    fn prompt_is_required() {
        let result = Cli::try_parse_from(["wukong"]);
        assert!(result.is_err());
    }

    #[test]
    fn agent_cmd_override_parses() {
        let cli = Cli::try_parse_from(["wukong", "--agent-cmd", "opencode run", "hi"]).unwrap();
        assert_eq!(cli.agent_cmd.as_deref(), Some("opencode run"));
        assert_eq!(cli.prompt_text(), "hi");
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-gateway/src/lib.rs` with:

```rust
//! wukong-gateway: CLI assistant gateway over wukong-memory.

pub mod cli;
pub mod error;

pub use error::GatewayError;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wukong-gateway cli::`
Expected: all 3 cli tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-gateway/src/cli.rs crates/wukong-gateway/src/lib.rs
git commit -m "feat(gateway): add CLI argument parsing"
```

---

## Task 4: Config resolution

**Files:**
- Create: `crates/wukong-gateway/src/config.rs`
- Modify: `crates/wukong-gateway/src/lib.rs`

- [ ] **Step 1: Write the config module with tests**

Create `crates/wukong-gateway/src/config.rs`:

```rust
use crate::cli::Cli;

/// Fully resolved runtime configuration (CLI > env > defaults).
#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub scope: String,
    pub db_url: String,
    pub agent_command: Vec<String>,
    pub continue_args: Vec<String>,
    pub continue_session: bool,
    pub recall_top_k: usize,
}

impl GatewayConfig {
    /// Resolve config from parsed CLI args, falling back to env then defaults.
    pub fn resolve(cli: &Cli) -> GatewayConfig {
        let scope = cli.scope.clone().unwrap_or_else(default_scope);

        let db_url = cli
            .db
            .clone()
            .or_else(|| std::env::var("WUKONG_MEMORY_DB").ok())
            .unwrap_or_else(default_db_url);

        let agent_command = cli
            .agent_cmd
            .clone()
            .or_else(|| std::env::var("WUKONG_AGENT_CMD").ok())
            .map(|s| split_ws(&s))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]);

        let continue_args = std::env::var("WUKONG_AGENT_CONTINUE_ARGS")
            .ok()
            .map(|s| split_ws(&s))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["-c".to_string()]);

        GatewayConfig {
            scope,
            db_url,
            agent_command,
            continue_args,
            continue_session: cli.continue_session,
            recall_top_k: 5,
        }
    }
}

fn split_ws(s: &str) -> Vec<String> {
    s.split_whitespace().map(|t| t.to_string()).collect()
}

/// Derive the default scope from the current directory name.
pub fn default_scope() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .filter(|n| !n.is_empty())
        .map(|n| format!("project:{n}"))
        .unwrap_or_else(|| "global".to_string())
}

fn default_db_url() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = format!("{home}/.wukong");
    let _ = std::fs::create_dir_all(&dir);
    format!("sqlite://{dir}/memory.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_overrides_take_priority() {
        // All overridable fields set on the CLI => env is never consulted.
        let cli = Cli::try_parse_from([
            "wukong",
            "--scope",
            "project:Explicit",
            "--db",
            "sqlite://x.db",
            "--agent-cmd",
            "my-agent go",
            "-c",
            "hi",
        ])
        .unwrap();
        let cfg = GatewayConfig::resolve(&cli);
        assert_eq!(cfg.scope, "project:Explicit");
        assert_eq!(cfg.db_url, "sqlite://x.db");
        assert_eq!(cfg.agent_command, vec!["my-agent".to_string(), "go".to_string()]);
        assert!(cfg.continue_session);
        assert_eq!(cfg.recall_top_k, 5);
        // continue_args default
        assert_eq!(cfg.continue_args, vec!["-c".to_string()]);
    }

    #[test]
    fn default_scope_is_project_prefixed() {
        // cargo runs tests with CWD at the crate root, which has a name.
        assert!(default_scope().starts_with("project:"));
    }

    #[test]
    fn split_ws_splits_on_whitespace() {
        assert_eq!(split_ws("opencode  run"), vec!["opencode".to_string(), "run".to_string()]);
        assert!(split_ws("   ").is_empty());
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-gateway/src/lib.rs` with:

```rust
//! wukong-gateway: CLI assistant gateway over wukong-memory.

pub mod cli;
pub mod config;
pub mod error;

pub use error::GatewayError;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wukong-gateway config::`
Expected: all 3 config tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-gateway/src/config.rs crates/wukong-gateway/src/lib.rs
git commit -m "feat(gateway): add config resolution"
```

---

## Task 5: AiBackend trait and agent CLI driver

**Files:**
- Create: `crates/wukong-gateway/src/backend.rs`
- Modify: `crates/wukong-gateway/src/lib.rs`

- [ ] **Step 1: Write the backend module with tests**

Create `crates/wukong-gateway/src/backend.rs`:

```rust
use crate::error::GatewayError;
use std::process::Stdio;
use tokio::process::Command;

/// A request to the AI backend.
pub struct AgentRequest {
    pub prompt: String,
    pub continue_session: bool,
}

/// The backend's textual response.
pub struct AgentResponse {
    pub text: String,
}

/// Pluggable AI backend. v1 ships `AgentCliBackend`.
#[allow(async_fn_in_trait)]
pub trait AiBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError>;
}

/// Build the argv handed to the agent subprocess:
/// `command + (continue_args if continue_session) + [prompt]`.
pub fn assemble_argv(
    command: &[String],
    continue_args: &[String],
    continue_session: bool,
    prompt: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = command.to_vec();
    if continue_session {
        argv.extend(continue_args.iter().cloned());
    }
    argv.push(prompt.to_string());
    argv
}

/// Drives a configurable agent CLI as a subprocess (run-and-capture, no shell).
pub struct AgentCliBackend {
    pub command: Vec<String>,
    pub continue_args: Vec<String>,
}

impl AiBackend for AgentCliBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        let argv = assemble_argv(
            &self.command,
            &self.continue_args,
            req.continue_session,
            &req.prompt,
        );
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
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn assemble_argv_without_continue() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            &["-c".to_string()],
            false,
            "hi",
        );
        assert_eq!(argv, vec!["opencode", "run", "hi"]);
    }

    #[test]
    fn assemble_argv_with_continue() {
        let argv = assemble_argv(
            &["opencode".to_string(), "run".to_string()],
            &["-c".to_string()],
            true,
            "hi",
        );
        assert_eq!(argv, vec!["opencode", "run", "-c", "hi"]);
    }

    #[tokio::test]
    async fn agent_cli_backend_captures_stdout() {
        // `echo <prompt>` prints the prompt back; verifies capture + trim.
        let backend = AgentCliBackend {
            command: vec!["echo".to_string()],
            continue_args: vec![],
        };
        let resp = backend
            .run(AgentRequest {
                prompt: "hello wukong".to_string(),
                continue_session: false,
            })
            .await
            .unwrap();
        assert_eq!(resp.text, "hello wukong");
    }

    #[tokio::test]
    async fn agent_cli_backend_reports_failure() {
        // `false` exits non-zero with no output.
        let backend = AgentCliBackend {
            command: vec!["false".to_string()],
            continue_args: vec![],
        };
        let err = backend
            .run(AgentRequest {
                prompt: "x".to_string(),
                continue_session: false,
            })
            .await
            .unwrap_err();
        assert!(matches!(err, GatewayError::AgentFailed { .. }));
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-gateway/src/lib.rs` with:

```rust
//! wukong-gateway: CLI assistant gateway over wukong-memory.

pub mod backend;
pub mod cli;
pub mod config;
pub mod error;

pub use error::GatewayError;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wukong-gateway backend::`
Expected: all 4 backend tests pass (`echo` and `false` are standard on Linux/macOS).

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-gateway/src/backend.rs crates/wukong-gateway/src/lib.rs
git commit -m "feat(gateway): add AiBackend trait and agent CLI driver"
```

---

## Task 6: Prompt composition

**Files:**
- Create: `crates/wukong-gateway/src/prompt.rs`
- Modify: `crates/wukong-gateway/src/lib.rs`

- [ ] **Step 1: Write the prompt module with tests**

Create `crates/wukong-gateway/src/prompt.rs`:

```rust
use wukong_memory::RecallHit;

/// Compose the final prompt: when there are recall hits, prepend a memory
/// context block; otherwise return the user input unchanged.
pub fn compose_prompt(hits: &[RecallHit], input: &str) -> String {
    if hits.is_empty() {
        return input.to_string();
    }
    let mut s = String::from("[相關記憶]\n");
    for h in hits {
        s.push_str(&format!("- ({}) {}\n", h.scope, h.text));
    }
    s.push_str("\n[使用者輸入]\n");
    s.push_str(input);
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use wukong_memory::MemoryKind;

    fn hit(scope: &str, text: &str) -> RecallHit {
        RecallHit {
            id: 1,
            scope: scope.to_string(),
            kind: MemoryKind::Note,
            text: text.to_string(),
            score: 1.0,
        }
    }

    #[test]
    fn no_hits_returns_input_unchanged() {
        assert_eq!(compose_prompt(&[], "just this"), "just this");
    }

    #[test]
    fn hits_are_prepended_as_context() {
        let hits = vec![hit("project:Wukong", "decided to use Rust")];
        let out = compose_prompt(&hits, "what did we decide?");
        assert!(out.contains("[相關記憶]"));
        assert!(out.contains("(project:Wukong) decided to use Rust"));
        assert!(out.contains("[使用者輸入]"));
        assert!(out.contains("what did we decide?"));
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-gateway/src/lib.rs` with:

```rust
//! wukong-gateway: CLI assistant gateway over wukong-memory.

pub mod backend;
pub mod cli;
pub mod config;
pub mod error;
pub mod prompt;

pub use error::GatewayError;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wukong-gateway prompt::`
Expected: both prompt tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-gateway/src/prompt.rs crates/wukong-gateway/src/lib.rs
git commit -m "feat(gateway): add prompt composition"
```

---

## Task 7: Turn pipeline

**Files:**
- Create: `crates/wukong-gateway/src/pipeline.rs`
- Modify: `crates/wukong-gateway/src/lib.rs`

- [ ] **Step 1: Write the pipeline module with integration tests**

Create `crates/wukong-gateway/src/pipeline.rs`:

```rust
use crate::backend::{AgentRequest, AiBackend};
use crate::config::GatewayConfig;
use crate::error::GatewayError;
use crate::prompt::compose_prompt;
use wukong_memory::{Memory, MemoryItem, MemoryKind, RecallMode, RecallQuery, RememberInput};

/// Run one assistant turn: recall relevant memory, compose the prompt, invoke
/// the backend, persist the turn, and return the response text.
pub async fn run_turn(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
) -> Result<String, GatewayError> {
    let recall = memory
        .recall(RecallQuery {
            query: input.to_string(),
            top_k: cfg.recall_top_k,
            scope: Some(cfg.scope.clone()),
            mode: RecallMode::Hybrid,
        })
        .await?;

    let prompt = compose_prompt(&recall.data, input);

    let resp = backend
        .run(AgentRequest {
            prompt,
            continue_session: cfg.continue_session,
        })
        .await?;

    memory
        .remember(RememberInput {
            scope: cfg.scope.clone(),
            session_id: None,
            items: vec![
                MemoryItem {
                    kind: MemoryKind::Event,
                    text: format!("User: {input}"),
                    importance: None,
                },
                MemoryItem {
                    kind: MemoryKind::Event,
                    text: format!("Assistant: {}", resp.text),
                    importance: None,
                },
            ],
        })
        .await?;

    Ok(resp.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AgentResponse;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    /// Records the prompt it receives and returns a canned reply.
    struct MockBackend {
        captured: Mutex<Option<String>>,
        reply: String,
    }

    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            *self.captured.lock().unwrap() = Some(req.prompt);
            Ok(AgentResponse {
                text: self.reply.clone(),
            })
        }
    }

    async fn open_memory() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    fn test_cfg(scope: &str) -> GatewayConfig {
        GatewayConfig {
            scope: scope.to_string(),
            db_url: String::new(),
            agent_command: vec![],
            continue_args: vec![],
            continue_session: false,
            recall_top_k: 5,
        }
    }

    #[tokio::test]
    async fn run_turn_returns_reply_and_persists_turn() {
        let mem = open_memory().await;
        let backend = MockBackend {
            captured: Mutex::new(None),
            reply: "pong".to_string(),
        };
        let out = run_turn(&mem, &backend, &test_cfg("project:T"), "ping")
            .await
            .unwrap();
        assert_eq!(out, "pong");

        // The turn was persisted and is recallable.
        let r = mem
            .recall(RecallQuery {
                query: "ping".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("User: ping")));
        assert!(r.data.iter().any(|h| h.text.contains("Assistant: pong")));
    }

    #[tokio::test]
    async fn prior_memory_is_injected_into_prompt() {
        let mem = open_memory().await;
        mem.remember(RememberInput {
            scope: "project:T".to_string(),
            session_id: None,
            items: vec![MemoryItem {
                kind: MemoryKind::Event,
                text: "earlier decision about Rust".to_string(),
                importance: None,
            }],
        })
        .await
        .unwrap();

        let backend = MockBackend {
            captured: Mutex::new(None),
            reply: "ok".to_string(),
        };
        run_turn(&mem, &backend, &test_cfg("project:T"), "tell me about rust")
            .await
            .unwrap();

        let captured = backend.captured.lock().unwrap().clone().unwrap();
        assert!(captured.contains("[相關記憶]"));
        assert!(captured.contains("earlier decision about Rust"));
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-gateway/src/lib.rs` with:

```rust
//! wukong-gateway: CLI assistant gateway over wukong-memory.

pub mod backend;
pub mod cli;
pub mod config;
pub mod error;
pub mod pipeline;
pub mod prompt;

pub use error::GatewayError;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wukong-gateway pipeline::`
Expected: both pipeline tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-gateway/src/pipeline.rs crates/wukong-gateway/src/lib.rs
git commit -m "feat(gateway): add turn pipeline"
```

---

## Task 8: Binary entrypoint and final verification

**Files:**
- Modify: `crates/wukong-gateway/src/main.rs`

- [ ] **Step 1: Implement the binary entrypoint**

Replace the contents of `crates/wukong-gateway/src/main.rs` with:

```rust
use clap::Parser;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::cli::Cli;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::pipeline::run_turn;
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

    let backend = AgentCliBackend {
        command: cfg.agent_command.clone(),
        continue_args: cfg.continue_args.clone(),
    };

    match run_turn(&memory, &backend, &cfg, &cli.prompt_text()).await {
        Ok(text) => println!("{text}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 2: Run the full workspace test suite**

Run: `cargo test`
Expected: all wukong-memory + wukong-memoryd + wukong-gateway tests pass, no failures.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Manual smoke test with a fake agent**

Run:
```bash
WUKONG_MEMORY_DB="sqlite://$PWD/scratch.db" cargo run -q -p wukong-gateway -- --agent-cmd "echo" --scope "project:Smoke" "remember I like Rust"
```
Expected: prints `remember I like Rust` (echo replays the composed prompt's tail / input).

Then verify the turn was persisted:
```bash
WUKONG_MEMORY_DB="sqlite://$PWD/scratch.db" cargo run -q -p wukong-gateway -- --agent-cmd "echo" --scope "project:Smoke" "what do I like"
```
Expected: the composed prompt echoed back now contains a `[相關記憶]` block mentioning `User: remember I like Rust`.

Clean up: `rm -f scratch.db scratch.db-shm scratch.db-wal`

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-gateway/src/main.rs
git commit -m "feat(gateway): add wukong binary entrypoint"
```

---

## Acceptance Criteria (from spec)

1. `cargo test` green (unit + integration). — Tasks 2-7
2. `cargo clippy --all-targets -- -D warnings` clean. — Task 8 Step 3
3. `wukong --agent-cmd "echo" "hi"` runs and persists the turn (recallable afterward). — Task 8 Step 4
4. Default scope derived from cwd as `project:<dir>`, `--scope` overrides. — Task 4 (`default_scope_is_project_prefixed`, `cli_overrides_take_priority`)
5. `-c`/`--continue` inserts the continue args into the agent argv. — Task 5 (`assemble_argv_with_continue`)
6. Non-zero agent exit → `AgentFailed`, and `main` exits with code 1. — Task 5 (`agent_cli_backend_reports_failure`) + Task 8 main mapping
```
