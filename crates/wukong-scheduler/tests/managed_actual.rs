#[path = "../../../scripts/test-support/managed_actual.rs"]
mod support;
use wukong_memory::Memory;
use wukong_scheduler::{execute_job, ExecutionContext, Job, JobKind, PermissionPolicy};

// SCN-004：真實 OpenCode bash 權限與檔案副作用，不以成功輸出代替權限證據。
#[tokio::test]
#[ignore = "requires isolated OpenCode and local model fixture"]
async fn scheduler_managed_reject_and_allow_once_have_distinct_effects() {
    let marker = std::path::PathBuf::from(std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap())
        .join("permission-marker");
    for policy in [PermissionPolicy::Reject, PermissionPolicy::AllowOnce] {
        let (_dir, backend, cfg) = support::fixture("project:issue7-managed-scheduler");
        let memory = Memory::open(&cfg.db_url).await.unwrap();
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
            permission_policy: policy,
        };
        let mut job = Job {
            id: "issue7".into(),
            name: "probe".into(),
            kind: JobKind::Turn {
                scope: cfg.scope.clone(),
                prompt: "PERMISSION_PROBE".into(),
            },
            cron: "* * * * *".into(),
            enabled: true,
            next_run_at: None,
            last_run_at: None,
        };
        for _ in 0..2 {
            if marker.exists() {
                std::fs::remove_file(&marker).unwrap();
            }
            let out =
                tokio::time::timeout(std::time::Duration::from_secs(20), execute_job(&ctx, &job))
                    .await
                    .unwrap();
            assert!(out.success, "{}", out.message);
            match policy {
                PermissionPolicy::Reject => {
                    assert!(!marker.exists());
                    assert!(out.message.contains("已自動拒絕"), "{}", out.message);
                }
                PermissionPolicy::AllowOnce => {
                    assert_eq!(
                        std::fs::read_to_string(&marker).unwrap(),
                        "MANAGED_PERMISSION_OK"
                    );
                    assert!(out.message.contains("已自動允許"), "{}", out.message);
                }
            }
        }
        job.kind = JobKind::Turn {
            scope: cfg.scope.clone(),
            prompt: "QUESTION_PROBE".into(),
        };
        let out = tokio::time::timeout(std::time::Duration::from_secs(20), execute_job(&ctx, &job))
            .await
            .unwrap();
        assert!(out.success, "{}", out.message);
        assert!(out.message.contains("已自動拒絕 que_"), "{}", out.message);
    }
}
