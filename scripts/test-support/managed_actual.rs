#![allow(dead_code)]
use wukong_gateway::backend::{build_backend_from_env, AgentBackend};
use wukong_gateway::config::GatewayConfig;

pub fn fixture(scope: &str) -> (tempfile::TempDir, AgentBackend, GatewayConfig) {
    let root = std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap();
    let dir = tempfile::tempdir_in(root).unwrap();
    let command = vec!["opencode".into(), "run".into()];
    let backend = build_backend_from_env(command.clone(), Some(dir.path().to_path_buf()));
    let cfg = GatewayConfig {
        scope: scope.into(),
        db_url: format!("sqlite://{}", dir.path().join("memory.db").display()),
        agent_command: command,
        default_model: None,
        planner_preferences: None,
        thinking: false,
        recall_top_k: 5,
        stream: true,
    };
    (dir, backend, cfg)
}
