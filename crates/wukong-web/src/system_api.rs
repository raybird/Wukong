use serde::Serialize;
use wukong_scheduler::Job;

#[derive(Debug, Serialize)]
pub struct SystemResponse {
    pub scope: String,
    pub token_enabled: bool,
    pub memory_db: String,
    pub schedule_total: usize,
    pub schedule_enabled: usize,
    pub next_run_at: Option<i64>,
}

pub fn system_response(
    scope: &str,
    token_enabled: bool,
    db_url: &str,
    jobs: &[Job],
) -> SystemResponse {
    SystemResponse {
        scope: scope.to_string(),
        token_enabled,
        memory_db: if db_url.trim().is_empty() {
            "unavailable".to_string()
        } else {
            "configured".to_string()
        },
        schedule_total: jobs.len(),
        schedule_enabled: jobs.iter().filter(|j| j.enabled).count(),
        next_run_at: jobs.iter().filter_map(|j| j.next_run_at).min(),
    }
}
