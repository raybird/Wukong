//! wukong-cli: the unified Wukong assistant (金箍棒) tying the three pillars together.

pub mod persona;
pub mod render;

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
            stream: true,
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
