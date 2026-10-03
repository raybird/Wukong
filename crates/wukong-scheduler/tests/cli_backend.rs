#[path = "../../../scripts/test-support/cli_backend.rs"]
mod support;

use wukong_memory::Memory;
use wukong_scheduler::{execute_job, ExecutionContext, Job, JobKind, PermissionPolicy};

// SCN-003：真實 Scheduler executor → run_turn → stdin=null CLI subprocess。
#[tokio::test]
async fn scheduler_executor_completes_cli_turn_and_resumes() {
    let (dir, backend, cfg) = support::fixture("project:issue7-scheduler");
    let memory = Memory::open(&cfg.db_url).await.unwrap();
    let ctx = ExecutionContext {
        memory: &memory,
        backend: &backend,
        base_config: &cfg,
        permission_policy: PermissionPolicy::Reject,
    };
    let job = Job {
        id: "issue7".into(),
        name: "probe".into(),
        kind: JobKind::Turn {
            scope: cfg.scope.clone(),
            prompt: "integration probe".into(),
        },
        cron: "* * * * *".into(),
        enabled: true,
        next_run_at: None,
        last_run_at: None,
    };
    let mut session = None;
    for _ in 0..2 {
        let out = tokio::time::timeout(std::time::Duration::from_secs(5), execute_job(&ctx, &job))
            .await
            .unwrap();
        assert!(out.success, "{}", out.message);
        assert!(out.message.contains("CLI_INTEGRATION_OK"));
        let captured = memory.agent_session(&cfg.scope).await.unwrap().unwrap();
        if let Some(id) = &session {
            assert_eq!(&captured, id);
        }
        session = Some(captured);
    }
    assert!(dir.path().join("finished").exists());
}
