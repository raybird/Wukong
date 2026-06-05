# wukong (金箍棒) v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the unified `wukong` CLI (金箍棒) — one command that recalls memory, routes the task to a role, runs the agent in the Wukong persona with role + memory context, and persists the turn.

**Architecture:** A new top crate `wukong-cli` (lib + bin `wukong`) depending on all three pillars. `persona` builds the persona+role+memory prompt; `lib::run_turn` chains recall → route → execute → remember. Reuses gateway's `Cli`/`GatewayConfig`/`AiBackend`/`AgentCliBackend`/`compose_prompt`, orchestrator's `route`/`Role`, and memory's `Memory`. Gateway's own `wukong` bin is retired to avoid a name collision.

**Tech Stack:** Rust, tokio, clap (derive), thiserror, `wukong-memory` + `wukong-gateway` + `wukong-orchestrator` (path deps). Dev: tempfile.

---

## File Structure

```
crates/wukong-cli/
├── Cargo.toml          # [lib] wukong_cli + [[bin]] name="wukong"
└── src/
    ├── lib.rs          # WukongError + TurnOutput + run_turn + re-exports
    ├── persona.rs      # WUKONG_PERSONA + build_prompt
    └── main.rs         # thin bin: parse → open memory → run_turn → print

crates/wukong-gateway/Cargo.toml   # remove the [[bin]] section
crates/wukong-gateway/src/main.rs  # delete
```

`wukong-cli → { wukong-memory, wukong-gateway, wukong-orchestrator }` (one-way, no cycle). Maximal reuse: gateway's `cli::Cli` + `config::GatewayConfig` are used directly (same flags), so `wukong-cli` adds no CLI/config of its own.

---

## Task 1: Retire gateway's binary

**Files:**
- Modify: `crates/wukong-gateway/Cargo.toml`
- Delete: `crates/wukong-gateway/src/main.rs`

- [ ] **Step 1: Remove the [[bin]] section from gateway's manifest**

In `crates/wukong-gateway/Cargo.toml`, delete these three lines (the whole `[[bin]]` block):

```toml
[[bin]]
name = "wukong"
path = "src/main.rs"
```

Leave the `[lib]`, `[package]`, `[dependencies]`, and `[dev-dependencies]` sections unchanged.

- [ ] **Step 2: Delete the gateway binary entrypoint**

```bash
git rm crates/wukong-gateway/src/main.rs
```

- [ ] **Step 3: Verify gateway still builds and tests pass as a lib**

Run: `cargo test -p wukong-gateway`
Expected: compiles (now lib-only); all existing gateway tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-gateway/Cargo.toml
git commit -m "refactor(gateway): retire wukong bin (now lib-only)"
```

---

## Task 2: Scaffold the wukong-cli crate

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/wukong-cli/Cargo.toml`
- Create: `crates/wukong-cli/src/lib.rs`
- Create: `crates/wukong-cli/src/main.rs`

- [ ] **Step 1: Add the crate as a workspace member**

In the root `Cargo.toml`, change the `members` line to include the new crate:

```toml
members = ["crates/wukong-memory", "crates/wukong-memoryd", "crates/wukong-gateway", "crates/wukong-orchestrator", "crates/wukong-cli"]
```

- [ ] **Step 2: Create the crate manifest**

Create `crates/wukong-cli/Cargo.toml`:

```toml
[package]
name = "wukong-cli"
edition.workspace = true
version.workspace = true

[lib]
name = "wukong_cli"
path = "src/lib.rs"

[[bin]]
name = "wukong"
path = "src/main.rs"

[dependencies]
wukong-memory = { path = "../wukong-memory" }
wukong-gateway = { path = "../wukong-gateway" }
wukong-orchestrator = { path = "../wukong-orchestrator" }
tokio = { workspace = true }
clap = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

- [ ] **Step 3: Create a minimal lib.rs with a smoke test**

Create `crates/wukong-cli/src/lib.rs`:

```rust
//! wukong-cli: the unified Wukong assistant (金箍棒) tying the three pillars together.

#[cfg(test)]
mod smoke_tests {
    #[test]
    fn crate_builds() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 4: Create a placeholder main.rs**

Create `crates/wukong-cli/src/main.rs`:

```rust
fn main() {
    println!("wukong placeholder");
}
```

- [ ] **Step 5: Verify the workspace builds and the smoke test passes**

Run: `cargo test -p wukong-cli`
Expected: compiles; `smoke_tests::crate_builds` passes.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/wukong-cli
git commit -m "chore: scaffold wukong-cli crate"
```

---

## Task 3: Persona and prompt builder

**Files:**
- Create: `crates/wukong-cli/src/persona.rs`
- Modify: `crates/wukong-cli/src/lib.rs`

- [ ] **Step 1: Write the persona module with tests**

Create `crates/wukong-cli/src/persona.rs`:

```rust
use wukong_memory::RecallHit;
use wukong_orchestrator::Role;

/// The light-touch Sun Wukong persona, prepended to every execution prompt.
pub const WUKONG_PERSONA: &str =
    "你是孫悟空（齊天大聖、鬥戰勝佛），一位全知全能的助手。\
     以略帶豪氣、機敏的口吻回應，但內容務必專業、精準、可執行。";

/// Build the execution prompt: persona + role card + (memory context + input).
/// The memory/input section reuses gateway's compose_prompt.
pub fn build_prompt(role: Role, hits: &[RecallHit], input: &str) -> String {
    let body = wukong_gateway::prompt::compose_prompt(hits, input);
    format!("{WUKONG_PERSONA}\n\n{}\n\n{body}", role.card())
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
    fn build_prompt_includes_persona_role_and_input() {
        let p = build_prompt(Role::Fixer, &[], "fix the bug");
        assert!(p.contains("孫悟空"));
        assert!(p.contains("你是 Fixer"));
        assert!(p.contains("fix the bug"));
    }

    #[test]
    fn build_prompt_includes_memory_block_when_hits_present() {
        let hits = vec![hit("project:Wukong", "earlier decision")];
        let p = build_prompt(Role::Oracle, &hits, "what now?");
        assert!(p.contains("[相關記憶]"));
        assert!(p.contains("earlier decision"));
        assert!(p.contains("你是 Oracle"));
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-cli/src/lib.rs` with:

```rust
//! wukong-cli: the unified Wukong assistant (金箍棒) tying the three pillars together.

pub mod persona;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wukong-cli persona::`
Expected: both persona tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-cli/src/persona.rs crates/wukong-cli/src/lib.rs
git commit -m "feat(wukong): add persona and prompt builder"
```

---

## Task 4: Error type and unified turn pipeline

**Files:**
- Modify: `crates/wukong-cli/src/lib.rs`

- [ ] **Step 1: Add WukongError, TurnOutput, and run_turn with integration tests**

Replace the contents of `crates/wukong-cli/src/lib.rs` with:

```rust
//! wukong-cli: the unified Wukong assistant (金箍棒) tying the three pillars together.

pub mod persona;

use thiserror::Error;
use wukong_gateway::backend::{AgentRequest, AiBackend};
use wukong_gateway::config::GatewayConfig;
use wukong_memory::{Memory, MemoryItem, MemoryKind, RecallMode, RecallQuery, RememberInput};
use wukong_orchestrator::Role;

/// All errors produced by the unified turn.
#[derive(Debug, Error)]
pub enum WukongError {
    #[error("memory error: {0}")]
    Memory(#[from] wukong_memory::MemoryError),
    #[error("orchestrator error: {0}")]
    Orchestrator(#[from] wukong_orchestrator::OrchestratorError),
    #[error("backend error: {0}")]
    Backend(#[from] wukong_gateway::GatewayError),
}

/// The result of one unified turn.
#[derive(Debug, Clone)]
pub struct TurnOutput {
    pub role: Role,
    pub text: String,
}

/// One unified Wukong turn: recall → route → execute (persona + role + memory)
/// → remember. Makes two backend calls (route, then execute).
pub async fn run_turn(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
) -> Result<TurnOutput, WukongError> {
    // 1. Recall relevant memory for this scope.
    let recall = memory
        .recall(RecallQuery {
            query: input.to_string(),
            top_k: cfg.recall_top_k,
            scope: Some(cfg.scope.clone()),
            mode: RecallMode::Hybrid,
        })
        .await?;

    // 2. Route the task to a role.
    let role = wukong_orchestrator::route(backend, input).await?;

    // 3. Build the persona + role + memory prompt.
    let prompt = persona::build_prompt(role, &recall.data, input);

    // 4. Execute.
    let resp = backend
        .run(AgentRequest {
            prompt,
            continue_session: cfg.continue_session,
        })
        .await?;

    // 5. Persist the turn.
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

    Ok(TurnOutput {
        role,
        text: resp.text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    /// Replies with scripted responses in order; records prompts seen.
    struct MockBackend {
        replies: Mutex<VecDeque<String>>,
        prompts: Mutex<Vec<String>>,
    }

    impl MockBackend {
        fn new(replies: &[&str]) -> MockBackend {
            MockBackend {
                replies: Mutex::new(replies.iter().map(|s| s.to_string()).collect()),
                prompts: Mutex::new(Vec::new()),
            }
        }
    }

    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.prompts.lock().unwrap().push(req.prompt);
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
    async fn run_turn_routes_executes_and_persists() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer", "done"]);
        let out = run_turn(&mem, &backend, &test_cfg("project:T"), "fix the bug")
            .await
            .unwrap();
        assert_eq!(out.role, Role::Fixer);
        assert_eq!(out.text, "done");

        // Turn persisted and recallable.
        let r = mem
            .recall(RecallQuery {
                query: "fix the bug".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("User: fix the bug")));
    }

    #[tokio::test]
    async fn execution_prompt_carries_persona_and_role() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer", "done"]);
        run_turn(&mem, &backend, &test_cfg("project:T"), "fix the bug")
            .await
            .unwrap();
        let prompts = backend.prompts.lock().unwrap();
        // [0] routing prompt, [1] execution prompt.
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].contains("孫悟空"));
        assert!(prompts[1].contains("你是 Fixer"));
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p wukong-cli`
Expected: persona tests + the 2 run_turn tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-cli/src/lib.rs
git commit -m "feat(wukong): add unified turn pipeline"
```

---

## Task 5: Binary entrypoint and final verification

**Files:**
- Modify: `crates/wukong-cli/src/main.rs`

- [ ] **Step 1: Implement the binary entrypoint**

Replace the contents of `crates/wukong-cli/src/main.rs` with:

```rust
use clap::Parser;
use wukong_cli::run_turn;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::cli::Cli;
use wukong_gateway::config::GatewayConfig;
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
        Ok(out) => {
            eprintln!("🐵 悟空·{}", out.role.name());
            println!("{}", out.text);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
```

- [ ] **Step 2: Run the full workspace test suite**

Run: `cargo test`
Expected: all crates' tests pass (memory, memoryd, gateway, orchestrator, wukong-cli), no failures.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Verify only one `wukong` binary exists in the workspace**

Run: `cargo build 2>&1 | grep -c "error" ; ls target/debug/wukong`
Expected: builds cleanly; `target/debug/wukong` exists. (Gateway no longer produces a `wukong` bin, so there is no `cargo run --bin wukong` ambiguity.)

- [ ] **Step 5: Manual smoke test with a fake agent**

Run:
```bash
cargo run -q -p wukong-cli --bin wukong -- --agent-cmd "printf fixer" --db "sqlite://$PWD/scratch.db" --scope "project:Smoke" "hello wukong"
```
Expected: stderr shows `🐵 悟空·fixer`; stdout shows `fixer` (the fake agent `printf fixer` returns "fixer" for both the routing call → routes to Fixer, and the execution call → output "fixer").

Then verify the turn was persisted:
```bash
cargo run -q -p wukong-cli --bin wukong -- --agent-cmd "printf fixer" --db "sqlite://$PWD/scratch.db" --scope "project:Smoke" "what did I say"
```
Expected: stderr `🐵 悟空·fixer`; the execution prompt (not printed, but the recall ran) now has prior memory. To inspect persistence directly, the first run's `User: hello wukong` is recallable — confirmed by the run_turn integration test; manual proof is that the command exits 0 both times.

Clean up: `rm -f scratch.db scratch.db-shm scratch.db-wal`

- [ ] **Step 6: Commit**

```bash
git add crates/wukong-cli/src/main.rs
git commit -m "feat(wukong): add unified wukong binary entrypoint"
```

---

## Acceptance Criteria (from spec)

1. `cargo test` green (new crate + existing three pillars). — Tasks 1-5
2. `cargo clippy --all-targets -- -D warnings` clean. — Task 5 Step 3
3. Workspace produces a single `wukong` bin (gateway no longer produces one). — Task 1 + Task 5 Step 4
4. `wukong --agent-cmd "printf fixer" ... "hello"` shows `🐵 悟空·fixer`, runs both phases, persists the turn. — Task 5 Step 5
5. Execution prompt carries persona + role card + memory context. — Task 4 (`execution_prompt_carries_persona_and_role`) + Task 3 (`build_prompt_includes_memory_block_when_hits_present`)
```
