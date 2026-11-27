#![allow(dead_code)]
use wukong_gateway::backend::{build_backend_from_env, AgentBackend};
use wukong_gateway::config::GatewayConfig;

pub fn fixture(scope: &str) -> (tempfile::TempDir, AgentBackend, GatewayConfig) {
    let root = std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap();
    let dir = tempfile::tempdir_in(root).unwrap();
    let command = vec!["opencode".into(), "run".into()];
    let backend = build_backend_from_env(command.clone(), Some(dir.path().to_path_buf())).unwrap();
    let cfg = GatewayConfig {
        scope: scope.into(),
        db_url: format!("sqlite://{}", dir.path().join("memory.db").display()),
        agent_command: command,
        default_model: None,
        thinking: false,
        recall_top_k: 5,
        stream: true,
    };
    (dir, backend, cfg)
}

/// #45 Task 5.1 外迴圈：接 `WUKONG_AGENT_SERVER_URL`／`WUKONG_AGENT_SERVER_PASSWORD` 指向的常駐
/// V2 server，模型取 `WUKONG_ISSUE45_MODEL`（provider/model）。agent 定義由呼叫端寫進 server 的
/// 工作區（`WUKONG_WORKSPACE`），與入口啟動時相同。用法見 docs/issues/issue-0045/scripts/probe-entries-dispatch.sh。
pub fn server_fixture(scope: &str) -> (tempfile::TempDir, AgentBackend, GatewayConfig) {
    let model = std::env::var("WUKONG_ISSUE45_MODEL").expect("set WUKONG_ISSUE45_MODEL");
    let dir = tempfile::tempdir().unwrap();
    let command = vec!["opencode".into(), "run".into()];
    let backend = build_backend_from_env(command.clone(), None).unwrap();
    let cfg = GatewayConfig {
        scope: scope.into(),
        db_url: format!("sqlite://{}", dir.path().join("memory.db").display()),
        agent_command: command,
        default_model: Some(model),
        thinking: false,
        recall_top_k: 5,
        stream: true,
    };
    (dir, backend, cfg)
}

/// 會派給 fixer 的固定合成提示詞（與 probe-turn-dispatch.sh 相同）。
pub const DISPATCH_PROMPT: &str =
    "請派給 fixer 處理：在工作區建立 hello.txt，內容只有一行：hello from wukong。完成後告訴我檔案內容。";
