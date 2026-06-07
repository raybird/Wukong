//! wukong-cli: the unified Wukong assistant (金箍棒) tying the three pillars together.

pub mod persona;
pub mod render;
pub mod repl;

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
    on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
    on_role: &mut dyn FnMut(Role),
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

    // 2. Plan an ordered role chain (replaces single-role routing).
    let roles = wukong_orchestrator::plan_chain(backend, input).await?;

    // 3. Run each role in order, accumulating prior outputs into the prompt.
    let mut prior: Vec<wukong_orchestrator::Outcome> = Vec::new();
    for role in roles {
        on_role(role);
        let augmented = format!("{input}{}", wukong_orchestrator::chain_context(&prior));
        let prompt = persona::build_prompt(role, &recall.data, &augmented);
        let resp = backend
            .run_streaming(
                AgentRequest { prompt, session_id: None, thinking: cfg.thinking },
                on_event,
            )
            .await?;
        prior.push(wukong_orchestrator::Outcome { role, output: resp.text });
    }

    // 4. Final output = last step. Fall back safely if the chain was empty.
    let last = prior
        .last()
        .cloned()
        .unwrap_or(wukong_orchestrator::Outcome { role: Role::Oracle, output: String::new() });

    // 5. Persist the turn: user input + the final assistant output only.
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
                    text: format!("Assistant: {}", last.output),
                    importance: None,
                },
            ],
        })
        .await?;

    Ok(TurnOutput {
        role: last.role,
        text: last.output,
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
        session_ids: Mutex<Vec<Option<String>>>,
    }

    impl MockBackend {
        fn new(replies: &[&str]) -> MockBackend {
            MockBackend {
                replies: Mutex::new(replies.iter().map(|s| s.to_string()).collect()),
                prompts: Mutex::new(Vec::new()),
                session_ids: Mutex::new(Vec::new()),
            }
        }
    }

    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.prompts.lock().unwrap().push(req.prompt);
            self.session_ids.lock().unwrap().push(req.session_id);
            let text = self.replies.lock().unwrap().pop_front().unwrap_or_default();
            Ok(AgentResponse { text, session_id: Some("ses_new".to_string()) })
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
            thinking: true,
            recall_top_k: 5,
            stream: true,
        }
    }

    #[tokio::test]
    async fn run_turn_routes_executes_and_persists() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["fixer", "done"]);
        let out = run_turn(&mem, &backend, &test_cfg("project:T"), "fix the bug", &mut |_| {}, &mut |_| {})
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
        run_turn(&mem, &backend, &test_cfg("project:T"), "fix the bug", &mut |_| {}, &mut |_| {})
            .await
            .unwrap();
        let prompts = backend.prompts.lock().unwrap();
        // [0] routing prompt, [1] execution prompt.
        assert_eq!(prompts.len(), 2);
        assert!(prompts[1].contains("孫悟空"));
        assert!(prompts[1].contains("你是 Fixer"));
    }

    #[tokio::test]
    async fn run_turn_runs_multi_role_chain() {
        let mem = open_memory().await;
        // [0] planner -> explorer,fixer ; [1] explorer output ; [2] fixer output
        let backend = MockBackend::new(&["explorer, fixer", "f1", "f2"]);
        let mut roles_seen: Vec<Role> = Vec::new();
        let out = run_turn(
            &mem,
            &backend,
            &test_cfg("project:T"),
            "build and fix",
            &mut |_| {},
            &mut |r| roles_seen.push(r),
        )
        .await
        .unwrap();

        // on_role fired once per step, in order.
        assert_eq!(roles_seen, vec![Role::Explorer, Role::Fixer]);
        // Final output is the last step.
        assert_eq!(out.text, "f2");
        assert_eq!(out.role, Role::Fixer);

        // Second execute prompt carries the first step's output. Scope the
        // guard so it is released before the await below.
        {
            let prompts = backend.prompts.lock().unwrap();
            assert_eq!(prompts.len(), 3); // plan + explorer + fixer
            assert!(prompts[2].contains("f1"));
        }

        // Memory stored the user input and the FINAL assistant output only.
        let r = mem
            .recall(RecallQuery {
                query: "build and fix".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("Assistant: f2")));
        assert!(!r.data.iter().any(|h| h.text.contains("Assistant: f1")));
    }
}
