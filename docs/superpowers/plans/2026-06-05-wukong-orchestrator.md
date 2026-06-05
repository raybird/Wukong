# wukong-orchestrator v1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the `wukong-orchestrator` v1 — a two-phase "seventy-two transformations" engine that asks an agent CLI which role (Explorer/Oracle/Librarian/Fixer/Designer) best fits a task, then runs that role to produce the answer.

**Architecture:** A fourth workspace crate (`wukong-orchestrator`, lib + thin bin `wukong-orchestrate`). `role` holds the five roles and their cards as data; `router` builds the routing prompt and parses the chosen role; `lib::orchestrate` chains route → execute over the `AiBackend` trait reused from `wukong-gateway`. Depends one-way on `wukong-gateway` (no cycle).

**Tech Stack:** Rust, tokio, clap (derive), thiserror, `wukong-gateway` (path dep for `AiBackend`/`AgentCliBackend`).

---

## File Structure

```
crates/wukong-orchestrator/
├── Cargo.toml          # lib + [[bin]] name="wukong-orchestrate"
└── src/
    ├── lib.rs          # module wiring + re-exports + Outcome + execution_prompt + orchestrate()
    ├── role.rs         # Role enum + name/description/card + all()
    ├── router.rs       # routing_prompt + parse_role + route()
    ├── error.rs        # OrchestratorError
    └── main.rs         # thin demo bin
```

Each unit has one responsibility: `role` is pure data, `router` owns role selection, `lib::orchestrate` chains the two phases, `error` narrows errors. All logic takes the `AiBackend` trait, so a mock backend drives tests.

Dependency direction: `wukong-orchestrator → wukong-gateway` (one-way, no cycle).

---

## Task 1: Add crate to workspace and scaffold

**Files:**
- Modify: `Cargo.toml` (workspace root)
- Create: `crates/wukong-orchestrator/Cargo.toml`
- Create: `crates/wukong-orchestrator/src/lib.rs`
- Create: `crates/wukong-orchestrator/src/main.rs`

- [ ] **Step 1: Add the crate as a workspace member**

In the root `Cargo.toml`, change the `members` line to include the new crate:

```toml
members = ["crates/wukong-memory", "crates/wukong-memoryd", "crates/wukong-gateway", "crates/wukong-orchestrator"]
```

(No new workspace dependencies are needed — `clap`, `tokio`, `thiserror` already exist in `[workspace.dependencies]`.)

- [ ] **Step 2: Create the crate manifest**

Create `crates/wukong-orchestrator/Cargo.toml`:

```toml
[package]
name = "wukong-orchestrator"
edition.workspace = true
version.workspace = true

[lib]
name = "wukong_orchestrator"
path = "src/lib.rs"

[[bin]]
name = "wukong-orchestrate"
path = "src/main.rs"

[dependencies]
wukong-gateway = { path = "../wukong-gateway" }
tokio = { workspace = true }
clap = { workspace = true }
thiserror = { workspace = true }
```

- [ ] **Step 3: Create a minimal lib.rs with a smoke test**

Create `crates/wukong-orchestrator/src/lib.rs`:

```rust
//! wukong-orchestrator: role routing engine over wukong-gateway's AiBackend.

#[cfg(test)]
mod smoke_tests {
    #[test]
    fn crate_builds() {
        assert_eq!(2 + 2, 4);
    }
}
```

- [ ] **Step 4: Create a placeholder main.rs**

Create `crates/wukong-orchestrator/src/main.rs`:

```rust
fn main() {
    println!("wukong-orchestrate placeholder");
}
```

- [ ] **Step 5: Verify the workspace builds and the smoke test passes**

Run: `cargo test -p wukong-orchestrator`
Expected: compiles; `smoke_tests::crate_builds` passes.

- [ ] **Step 6: Commit**

```bash
git add Cargo.toml crates/wukong-orchestrator
git commit -m "chore: scaffold wukong-orchestrator crate"
```

---

## Task 2: Error type

**Files:**
- Create: `crates/wukong-orchestrator/src/error.rs`
- Modify: `crates/wukong-orchestrator/src/lib.rs`

- [ ] **Step 1: Write the error module with a unit test**

Create `crates/wukong-orchestrator/src/error.rs`:

```rust
use thiserror::Error;

/// All errors produced by the orchestrator.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("backend error: {0}")]
    Backend(#[from] wukong_gateway::GatewayError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_error_displays() {
        let inner = wukong_gateway::GatewayError::AgentFailed {
            code: Some(1),
            stderr: "nope".to_string(),
        };
        let err = OrchestratorError::Backend(inner);
        assert!(err.to_string().contains("backend error"));
        assert!(err.to_string().contains("nope"));
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-orchestrator/src/lib.rs` with:

```rust
//! wukong-orchestrator: role routing engine over wukong-gateway's AiBackend.

pub mod error;

pub use error::OrchestratorError;
```

- [ ] **Step 3: Run the test to verify it passes**

Run: `cargo test -p wukong-orchestrator error::`
Expected: `backend_error_displays` passes.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-orchestrator/src/error.rs crates/wukong-orchestrator/src/lib.rs
git commit -m "feat(orchestrator): add OrchestratorError type"
```

---

## Task 3: Role and role cards

**Files:**
- Create: `crates/wukong-orchestrator/src/role.rs`
- Modify: `crates/wukong-orchestrator/src/lib.rs`

- [ ] **Step 1: Write the role module with tests**

Create `crates/wukong-orchestrator/src/role.rs`:

```rust
/// One of the five specialist roles (from tao-of-coding).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    Explorer,
    Oracle,
    Librarian,
    Fixer,
    Designer,
}

impl Role {
    /// All five roles in a fixed order (used by routing and parsing).
    pub fn all() -> [Role; 5] {
        [
            Role::Explorer,
            Role::Oracle,
            Role::Librarian,
            Role::Fixer,
            Role::Designer,
        ]
    }

    /// Lowercase identifier.
    pub fn name(&self) -> &'static str {
        match self {
            Role::Explorer => "explorer",
            Role::Oracle => "oracle",
            Role::Librarian => "librarian",
            Role::Fixer => "fixer",
            Role::Designer => "designer",
        }
    }

    /// One-line role description, listed in the routing prompt.
    pub fn description(&self) -> &'static str {
        match self {
            Role::Explorer => "結構洞察，快速掃描專案結構、理解檔案關聯與依賴。",
            Role::Oracle => "架構專家，擅長重構、決策分析與技術取捨。",
            Role::Librarian => "文件專家，負責撰寫文件、翻譯與註解。",
            Role::Fixer => "實作專家，程式碼修正、單元測試補全、語法修正，高效交付可運作的程式。",
            Role::Designer => "設計專家，負責 UI/UX 與前端體驗。",
        }
    }

    /// Role system prompt prepended when executing as this role.
    pub fn card(&self) -> &'static str {
        match self {
            Role::Explorer => {
                "你是 Explorer，結構洞察專家。負責快速掃描專案結構、追蹤依賴關係，揭開陌生程式碼的面貌。"
            }
            Role::Oracle => {
                "你是 Oracle，架構專家。擅長重構、決策分析與技術取捨；當架構混亂或 Bug 難解時提供可執行方案。"
            }
            Role::Librarian => {
                "你是 Librarian，文件專家。負責撰寫文件、API 註解與翻譯，條理分明。"
            }
            Role::Fixer => {
                "你是 Fixer，實作專家。負責程式碼修正、單元測試補全、語法修正，以最高效率交付可運作的程式。"
            }
            Role::Designer => {
                "你是 Designer，設計專家。負責 UI/UX 介面結構、互動與視覺一致性。"
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique() {
        let names: Vec<&str> = Role::all().iter().map(|r| r.name()).collect();
        let mut deduped = names.clone();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(names.len(), deduped.len());
        assert_eq!(names.len(), 5);
    }

    #[test]
    fn cards_and_descriptions_non_empty() {
        for r in Role::all() {
            assert!(!r.card().is_empty());
            assert!(!r.description().is_empty());
        }
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-orchestrator/src/lib.rs` with:

```rust
//! wukong-orchestrator: role routing engine over wukong-gateway's AiBackend.

pub mod error;
pub mod role;

pub use error::OrchestratorError;
pub use role::Role;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wukong-orchestrator role::`
Expected: both role tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-orchestrator/src/role.rs crates/wukong-orchestrator/src/lib.rs
git commit -m "feat(orchestrator): add Role type and role cards"
```

---

## Task 4: Router

**Files:**
- Create: `crates/wukong-orchestrator/src/router.rs`
- Modify: `crates/wukong-orchestrator/src/lib.rs`

- [ ] **Step 1: Write the router module with tests**

Create `crates/wukong-orchestrator/src/router.rs`:

```rust
use crate::error::OrchestratorError;
use crate::role::Role;
use wukong_gateway::backend::{AgentRequest, AiBackend};

/// Build the routing prompt: list the roles and ask for exactly one name.
pub fn routing_prompt(task: &str) -> String {
    let mut s = String::from(
        "You are a router. Pick the single best role to handle the task.\nRoles:\n",
    );
    for role in Role::all() {
        s.push_str(&format!("- {}: {}\n", role.name(), role.description()));
    }
    s.push_str("\nReply with ONLY the role name (one lowercase word).\n\n[Task]\n");
    s.push_str(task);
    s
}

/// Parse the routed role from the backend's reply. Scans in `Role::all()`
/// order and returns the first role whose name appears (case-insensitive);
/// falls back to `Role::Oracle` when none match.
pub fn parse_role(response: &str) -> Role {
    let lower = response.to_lowercase();
    for role in Role::all() {
        if lower.contains(role.name()) {
            return role;
        }
    }
    Role::Oracle
}

/// Phase 1: ask the backend which role should handle the task.
pub async fn route(backend: &impl AiBackend, task: &str) -> Result<Role, OrchestratorError> {
    let resp = backend
        .run(AgentRequest {
            prompt: routing_prompt(task),
            continue_session: false,
        })
        .await?;
    Ok(parse_role(&resp.text))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_prompt_lists_roles_and_task() {
        let p = routing_prompt("refactor the parser");
        for role in Role::all() {
            assert!(p.contains(role.name()), "missing role {}", role.name());
        }
        assert!(p.contains("refactor the parser"));
    }

    #[test]
    fn parse_role_matches_exact_name() {
        assert_eq!(parse_role("fixer"), Role::Fixer);
    }

    #[test]
    fn parse_role_is_case_insensitive() {
        assert_eq!(parse_role("FIXER"), Role::Fixer);
    }

    #[test]
    fn parse_role_finds_name_in_sentence() {
        assert_eq!(parse_role("I'd pick oracle for this"), Role::Oracle);
    }

    #[test]
    fn parse_role_falls_back_to_oracle() {
        assert_eq!(parse_role("garbage with no role"), Role::Oracle);
    }
}
```

- [ ] **Step 2: Wire the module into lib.rs**

Replace the contents of `crates/wukong-orchestrator/src/lib.rs` with:

```rust
//! wukong-orchestrator: role routing engine over wukong-gateway's AiBackend.

pub mod error;
pub mod role;
pub mod router;

pub use error::OrchestratorError;
pub use role::Role;
```

- [ ] **Step 3: Run the tests to verify they pass**

Run: `cargo test -p wukong-orchestrator router::`
Expected: all 5 router tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/wukong-orchestrator/src/router.rs crates/wukong-orchestrator/src/lib.rs
git commit -m "feat(orchestrator): add LLM router with fallback"
```

---

## Task 5: Orchestrate (two-phase)

**Files:**
- Modify: `crates/wukong-orchestrator/src/lib.rs`

- [ ] **Step 1: Add Outcome, execution_prompt, and orchestrate with integration tests**

Replace the contents of `crates/wukong-orchestrator/src/lib.rs` with:

```rust
//! wukong-orchestrator: role routing engine over wukong-gateway's AiBackend.

pub mod error;
pub mod role;
pub mod router;

pub use error::OrchestratorError;
pub use role::Role;
pub use router::{parse_role, route, routing_prompt};

use wukong_gateway::backend::{AgentRequest, AiBackend};

/// Result of one orchestration: which role ran, and what it produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub role: Role,
    pub output: String,
}

/// Build the execution prompt: the role card, then the task.
pub fn execution_prompt(role: Role, task: &str) -> String {
    format!("{}\n\n[任務]\n{}", role.card(), task)
}

/// Route the task to a role, then run that role to produce the answer.
/// Makes exactly two backend calls (route, then execute).
pub async fn orchestrate(
    backend: &impl AiBackend,
    task: &str,
) -> Result<Outcome, OrchestratorError> {
    let role = route(backend, task).await?;
    let prompt = execution_prompt(role, task);
    let resp = backend
        .run(AgentRequest {
            prompt,
            continue_session: false,
        })
        .await?;
    Ok(Outcome {
        role,
        output: resp.text,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    /// Replies with scripted responses in order; records every prompt seen.
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

    #[tokio::test]
    async fn orchestrate_routes_then_executes() {
        let backend = MockBackend::new(&["fixer", "done"]);
        let outcome = orchestrate(&backend, "fix the bug").await.unwrap();
        assert_eq!(outcome.role, Role::Fixer);
        assert_eq!(outcome.output, "done");
    }

    #[tokio::test]
    async fn execute_prompt_carries_role_card() {
        let backend = MockBackend::new(&["fixer", "done"]);
        orchestrate(&backend, "fix the bug").await.unwrap();
        let prompts = backend.prompts.lock().unwrap();
        // Two calls: [0] routing, [1] execution.
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].contains("你是 Fixer"));
        assert!(prompts[1].contains("fix the bug"));
    }

    #[tokio::test]
    async fn unparseable_route_falls_back_to_oracle() {
        let backend = MockBackend::new(&["no role here", "answer"]);
        let outcome = orchestrate(&backend, "ponder").await.unwrap();
        assert_eq!(outcome.role, Role::Oracle);
    }
}
```

- [ ] **Step 2: Run the tests to verify they pass**

Run: `cargo test -p wukong-orchestrator`
Expected: all role + router + the 3 orchestrate tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/wukong-orchestrator/src/lib.rs
git commit -m "feat(orchestrator): add two-phase orchestrate"
```

---

## Task 6: Demo binary and final verification

**Files:**
- Modify: `crates/wukong-orchestrator/src/main.rs`

- [ ] **Step 1: Implement the demo binary**

Replace the contents of `crates/wukong-orchestrator/src/main.rs` with:

```rust
use clap::Parser;
use wukong_gateway::backend::AgentCliBackend;
use wukong_orchestrator::orchestrate;

/// Demo entrypoint: auto-route a task to a role and run it.
#[derive(Parser, Debug)]
#[command(name = "wukong-orchestrate", about = "Route a task to a Wukong role and run it")]
struct Cli {
    /// The task to orchestrate (joined with spaces).
    #[arg(required = true, num_args = 1..)]
    task: Vec<String>,

    /// Override the agent command (whitespace-separated, e.g. "opencode run").
    #[arg(long = "agent-cmd")]
    agent_cmd: Option<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let command = cli
        .agent_cmd
        .or_else(|| std::env::var("WUKONG_AGENT_CMD").ok())
        .map(|s| s.split_whitespace().map(|t| t.to_string()).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]);

    let backend = AgentCliBackend {
        command,
        continue_args: vec![],
    };

    match orchestrate(&backend, &cli.task.join(" ")).await {
        Ok(outcome) => {
            eprintln!("[role: {}]", outcome.role.name());
            println!("{}", outcome.output);
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
Expected: all wukong-memory + wukong-memoryd + wukong-gateway + wukong-orchestrator tests pass, no failures.

- [ ] **Step 3: Run clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: no warnings.

- [ ] **Step 4: Manual smoke test with a fake agent**

Run:
```bash
cargo run -q -p wukong-orchestrator --bin wukong-orchestrate -- --agent-cmd "printf fixer" "fix the failing test"
```
Expected: stderr shows `[role: fixer]` and stdout shows `fixer` (the fake agent `printf fixer` returns the literal "fixer" for both the routing call and the execution call, so routing resolves to Fixer and the executed output is "fixer"). This proves both backend calls happen and routing parses correctly.

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-orchestrator/src/main.rs
git commit -m "feat(orchestrator): add wukong-orchestrate demo binary"
```

---

## Acceptance Criteria (from spec)

1. `cargo test` green (unit + mock integration). — Tasks 2-5
2. `cargo clippy --all-targets -- -D warnings` clean. — Task 6 Step 3
3. `wukong-orchestrate --agent-cmd "printf fixer" "fix the bug"` prints `[role: fixer]` and runs both phases. — Task 6 Step 4
4. `parse_role` falls back to `Oracle` on no match. — Task 4 (`parse_role_falls_back_to_oracle`) + Task 5 (`unparseable_route_falls_back_to_oracle`)
5. `orchestrate` makes exactly two backend calls (route + execute). — Task 5 (`execute_prompt_carries_role_card` asserts `prompts.len() == 2`)
```
