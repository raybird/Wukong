use crate::job::{Job, JobKind, MaintenanceTask};
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_memory::Memory;
use wukong_runtime::maintenance::{memory_consolidate, memory_prune, memory_snapshot};

const SCHEDULED_TURN_AUTONOMY_HINT: &str = "\n\n[無人值守排程]\n這是由 scheduler 觸發的無人值守任務。不得呼叫 question 或要求使用者確認；如果流程、技能或工具規則需要使用者選擇，請直接自動採用推薦選項，並繼續完成任務。";

pub struct ExecutionContext<'a, B: AiBackend + Sync> {
    pub memory: &'a Memory,
    pub backend: &'a B,
    pub base_config: &'a GatewayConfig,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutionOutput {
    pub success: bool,
    pub message: String,
}

pub async fn execute_job<B: AiBackend + Sync>(
    ctx: &ExecutionContext<'_, B>,
    job: &Job,
) -> ExecutionOutput {
    match execute_job_inner(ctx, job).await {
        Ok(message) => ExecutionOutput {
            success: true,
            message,
        },
        Err(err) => ExecutionOutput {
            success: false,
            message: err.to_string(),
        },
    }
}

async fn execute_job_inner<B: AiBackend + Sync>(
    ctx: &ExecutionContext<'_, B>,
    job: &Job,
) -> Result<String, wukong_runtime::WukongError> {
    match &job.kind {
        JobKind::Turn { scope, prompt } => {
            let mut cfg = ctx.base_config.clone();
            cfg.scope = scope.clone();
            let prompt = format!("{prompt}{SCHEDULED_TURN_AUTONOMY_HINT}");
            let out = wukong_runtime::run_turn(
                ctx.memory,
                ctx.backend,
                &cfg,
                &prompt,
                &mut |_| {},
                &mut |_| {},
            )
            .await?;
            Ok(out.text)
        }
        JobKind::Maintenance { scope, task } => match task {
            MaintenanceTask::Snapshot => memory_snapshot(ctx.memory, scope.as_deref()).await,
            MaintenanceTask::Consolidate => {
                let scope = scope.as_deref().unwrap_or(&ctx.base_config.scope);
                memory_consolidate(ctx.memory, ctx.backend, scope, false).await
            }
            MaintenanceTask::Prune => memory_prune(ctx.memory, scope.as_deref(), false).await,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::{JobKind, MaintenanceTask};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<Result<String, GatewayError>>>,
        prompts: Mutex<Vec<String>>,
    }

    impl MockBackend {
        fn new(replies: Vec<Result<&str, GatewayError>>) -> Self {
            Self {
                replies: Mutex::new(replies.into_iter().map(|r| r.map(str::to_string)).collect()),
                prompts: Mutex::new(Vec::new()),
            }
        }
    }

    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.prompts.lock().unwrap().push(req.prompt);
            let reply = self
                .replies
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Ok(String::new()))?;
            Ok(AgentResponse {
                text: reply,
                session_id: Some("ses_new".to_string()),
            })
        }
    }

    async fn open_memory() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    fn cfg(scope: &str) -> GatewayConfig {
        GatewayConfig {
            scope: scope.to_string(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        }
    }

    fn job(kind: JobKind) -> Job {
        Job {
            id: "job-1".to_string(),
            name: "job".to_string(),
            kind,
            cron: "* * * * *".to_string(),
            enabled: true,
            next_run_at: Some(1),
            last_run_at: None,
        }
    }

    #[tokio::test]
    async fn turn_job_overrides_scope() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![Ok("oracle"), Ok("done")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success);
        assert_eq!(out.message, "done");
        assert!(memory
            .agent_session("project:Scheduled")
            .await
            .unwrap()
            .is_some());
        assert_eq!(memory.agent_session("project:Base").await.unwrap(), None);
    }

    #[tokio::test]
    async fn turn_job_returns_failure_message_when_backend_fails() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![Err(GatewayError::AgentFailed {
            code: Some(1),
            stderr: "boom".to_string(),
        })]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(!out.success);
        assert!(out.message.contains("boom"));
    }

    #[tokio::test]
    async fn turn_job_instructs_agent_to_auto_choose_recommended_options() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![Ok("oracle"), Ok("done")]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
        };
        let job = job(JobKind::Turn {
            scope: "project:Scheduled".to_string(),
            prompt: "do it".to_string(),
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success);
        let prompts = backend.prompts.lock().unwrap();
        assert!(prompts.iter().any(|p| p.contains("無人值守排程")));
        assert!(prompts.iter().any(|p| p.contains("自動採用推薦選項")));
    }

    #[tokio::test]
    async fn snapshot_maintenance_returns_text() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
        };
        let job = job(JobKind::Maintenance {
            scope: None,
            task: MaintenanceTask::Snapshot,
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success);
        assert!(out.message.contains("總計:"));
    }

    #[tokio::test]
    async fn prune_maintenance_uses_provided_scope() {
        let memory = open_memory().await;
        let backend = MockBackend::new(vec![]);
        let cfg = cfg("project:Base");
        let ctx = ExecutionContext {
            memory: &memory,
            backend: &backend,
            base_config: &cfg,
        };
        let job = job(JobKind::Maintenance {
            scope: Some("project:Scheduled".to_string()),
            task: MaintenanceTask::Prune,
        });

        let out = execute_job(&ctx, &job).await;

        assert!(out.success);
        assert!(out.message.contains("已刪除"));
    }
}
