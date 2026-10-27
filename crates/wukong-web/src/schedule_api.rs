use serde::Serialize;
use wukong_scheduler::{Job, JobKind, MaintenanceTask};

#[derive(Debug, Serialize)]
pub struct ScheduleJobResponse {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub scope: Option<String>,
    pub prompt: Option<String>,
    /// Turn 排程的專屬模型；`None` 代表使用預設模型（issue-0026）。
    pub model: Option<String>,
    pub task: Option<String>,
    pub cron: String,
    pub enabled: bool,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
}

pub fn job_response(job: Job) -> ScheduleJobResponse {
    let (kind, scope, prompt, model, task) = match job.kind {
        JobKind::Turn {
            scope,
            prompt,
            model,
        } => ("turn".to_string(), Some(scope), Some(prompt), model, None),
        JobKind::Maintenance { scope, task } => (
            "maintenance".to_string(),
            scope,
            None,
            None,
            Some(task_label(task).to_string()),
        ),
    };
    ScheduleJobResponse {
        id: job.id,
        name: job.name,
        kind,
        scope,
        prompt,
        model,
        task,
        cron: job.cron,
        enabled: job.enabled,
        next_run_at: job.next_run_at,
        last_run_at: job.last_run_at,
    }
}

fn task_label(task: MaintenanceTask) -> &'static str {
    match task {
        MaintenanceTask::Snapshot => "snapshot",
        MaintenanceTask::Consolidate => "consolidate",
        MaintenanceTask::Prune => "prune",
    }
}
