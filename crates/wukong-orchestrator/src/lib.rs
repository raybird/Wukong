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
