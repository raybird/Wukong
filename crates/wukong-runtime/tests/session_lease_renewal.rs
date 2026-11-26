// issue-0013 SCN-003：run_turn 在整個執行期間續租 session lease。
// session lease 的長度只能從環境變數讀，所以放在獨立的 test binary，並且只有這一支測試，
// 改環境變數不會影響其他測試。
use std::time::Duration;
use wukong_gateway::backend::{AgentRequest, AgentResponse, AiBackend};
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::GatewayError;
use wukong_memory::Memory;

/// 回合唯一的一次呼叫睡 3.5 秒，比 2 秒的 lease 久。
struct SlowStep;

impl AiBackend for SlowStep {
    async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        tokio::time::sleep(Duration::from_millis(3500)).await;
        Ok(AgentResponse {
            text: "完成".to_string(),
            session_id: Some("ses_slow".to_string()),
        })
    }
}

#[tokio::test]
async fn run_turn_keeps_the_session_lease_past_its_length() {
    let workspace = tempfile::tempdir().unwrap();
    std::env::set_var("WUKONG_WORKSPACE", workspace.path());
    std::env::set_var("WUKONG_SESSION_LEASE_SECS", "2");
    let db = tempfile::NamedTempFile::new().unwrap();
    let memory = Memory::open(&format!("sqlite://{}", db.path().display()))
        .await
        .unwrap();
    let cfg = GatewayConfig {
        scope: "project:T".to_string(),
        db_url: String::new(),
        agent_command: vec![],
        default_model: None,
        thinking: true,
        recall_top_k: 5,
        stream: true,
    };
    let backend = SlowStep;
    let (mut on_event, mut on_role) = (|_| {}, |_| {});

    let turn = wukong_runtime::run_turn(
        &memory,
        &backend,
        &cfg,
        "長任務",
        &mut on_event,
        &mut on_role,
    );
    let intruder = async {
        tokio::time::sleep(Duration::from_millis(3000)).await;
        memory
            .acquire_agent_session_lease("project:T", "other", 2)
            .await
            .unwrap()
    };
    let (out, stolen) = tokio::join!(turn, intruder);

    assert!(
        stolen.is_none(),
        "回合執行中 session lease 被拿走：{stolen:?}"
    );
    assert_eq!(out.unwrap().text, "完成");
    assert_eq!(
        memory.agent_session("project:T").await.unwrap().as_deref(),
        Some("ses_slow")
    );
}
