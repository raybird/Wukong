//! wukong-orchestrator: role routing engine over wukong-gateway's AiBackend.

pub mod error;
pub mod role;
pub mod router;

pub use error::OrchestratorError;
pub use role::Role;
pub use router::{
    parse_chain, parse_role, parse_skill_chain, plan_chain, plan_skill_chain,
    plan_skill_chain_with_preferences, planning_prompt, route, routing_prompt,
    skill_planning_prompt, skill_planning_prompt_with_preferences, PlannedStep, PlannerPreferenceHint,
    SkillRouteOption,
};

use wukong_gateway::backend::{AgentRequest, AiBackend};

/// Result of one orchestration: which role ran, and what it produced.
#[derive(Debug, Clone)]
pub struct Outcome {
    pub role: Role,
    pub output: String,
}

/// The full result of a collaboration chain: every step in order.
#[derive(Debug, Clone)]
pub struct ChainOutcome {
    pub steps: Vec<Outcome>,
}

impl ChainOutcome {
    /// The last step's output; empty string for an empty chain.
    pub fn final_output(&self) -> &str {
        self.steps.last().map(|o| o.output.as_str()).unwrap_or("")
    }
}

/// Render prior chain steps as a context block to prepend onto the next step's
/// task. Empty slice yields an empty string (so the first step is unchanged).
pub fn chain_context(prior: &[Outcome]) -> String {
    if prior.is_empty() {
        return String::new();
    }
    let mut s = String::from("\n\n[前序協作]\n");
    for o in prior {
        s.push_str(&format!("{}: {}\n", o.role.name(), o.output));
    }
    s
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
            session_id: None,
            thinking: false,
            model: None,
        })
        .await?;
    Ok(Outcome {
        role,
        output: resp.text,
    })
}

/// Plan a role chain, then run each role in order, accumulating prior outputs
/// into each step's prompt. Makes 1 planning call + one call per role.
pub async fn orchestrate_chain(
    backend: &impl AiBackend,
    task: &str,
) -> Result<ChainOutcome, OrchestratorError> {
    let roles = plan_chain(backend, task).await?;
    let mut steps: Vec<Outcome> = Vec::new();
    for role in roles {
        let prompt = format!("{}\n\n[任務]\n{}{}", role.card(), task, chain_context(&steps));
        let resp = backend
            .run(AgentRequest { prompt, session_id: None, thinking: false, model: None })
            .await?;
        steps.push(Outcome { role, output: resp.text });
    }
    Ok(ChainOutcome { steps })
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
            Ok(AgentResponse { text, session_id: None })
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

    #[tokio::test]
    async fn plan_chain_parses_backend_reply() {
        let backend = MockBackend::new(&["explorer, fixer"]);
        let chain = plan_chain(&backend, "build a thing").await.unwrap();
        assert_eq!(chain, vec![Role::Explorer, Role::Fixer]);
    }

    #[tokio::test]
    async fn plan_skill_chain_parses_backend_reply() {
        let backend = MockBackend::new(&["fixer|test-driven-development"]);
        let skills = vec![SkillRouteOption {
            skill_name: "test-driven-development",
            description: "新功能或 bugfix 的測試先行實作",
            primary_role: Role::Fixer,
            collaborator_role: Some(Role::Oracle),
        }];
        let steps = plan_skill_chain(&backend, "fix the bug", &skills).await.unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0].role, Role::Fixer);
        assert_eq!(steps[0].skill_name.as_deref(), Some("test-driven-development"));

        let prompts = backend.prompts.lock().unwrap();
        assert!(prompts[0].contains("test-driven-development"));
    }

    #[tokio::test]
    async fn orchestrate_chain_runs_each_role_in_order() {
        // [0] planner reply, [1] explorer output, [2] fixer output.
        let backend = MockBackend::new(&["explorer, fixer", "找到了", "修好了"]);
        let co = orchestrate_chain(&backend, "fix it").await.unwrap();
        assert_eq!(co.steps.len(), 2);
        assert_eq!(co.steps[0].role, Role::Explorer);
        assert_eq!(co.steps[1].role, Role::Fixer);
        assert_eq!(co.final_output(), "修好了");

        // The second step's prompt carries the first step's output.
        let prompts = backend.prompts.lock().unwrap();
        assert_eq!(prompts.len(), 3); // plan + 2 executes
        assert!(prompts[2].contains("找到了"));
    }

    #[test]
    fn chain_context_empty_for_no_prior() {
        assert_eq!(chain_context(&[]), "");
    }

    #[test]
    fn chain_context_includes_roles_and_outputs() {
        let prior = vec![
            Outcome { role: Role::Explorer, output: "找到了問題".to_string() },
            Outcome { role: Role::Fixer, output: "已修正".to_string() },
        ];
        let c = chain_context(&prior);
        assert!(c.contains("前序協作"));
        assert!(c.contains("explorer"));
        assert!(c.contains("找到了問題"));
        assert!(c.contains("fixer"));
        assert!(c.contains("已修正"));
    }

    #[test]
    fn chain_outcome_final_is_last_step() {
        let co = ChainOutcome {
            steps: vec![
                Outcome { role: Role::Explorer, output: "a".to_string() },
                Outcome { role: Role::Fixer, output: "b".to_string() },
            ],
        };
        assert_eq!(co.final_output(), "b");
    }

    #[test]
    fn chain_outcome_final_empty_for_no_steps() {
        let co = ChainOutcome { steps: vec![] };
        assert_eq!(co.final_output(), "");
    }
}
