use chrono::{TimeZone, Utc};
use cron::Schedule;
use std::str::FromStr;

#[derive(Debug, thiserror::Error)]
pub enum SchedulerError {
    #[error("scheduler db: {0}")]
    Sqlx(#[from] sqlx::Error),
    #[error("scheduler json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("cron expression must have 5 fields: {0}")]
    InvalidCronFieldCount(String),
    #[error("invalid cron expression '{expr}': {source}")]
    InvalidCron {
        expr: String,
        source: cron::error::Error,
    },
    #[error("invalid timestamp: {0}")]
    InvalidTimestamp(i64),
    #[error("unknown scheduler job kind: {0}")]
    UnknownJobKind(String),
    #[error("unknown scheduler run status: {0}")]
    UnknownRunStatus(String),
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum JobKind {
    Turn {
        scope: String,
        prompt: String,
    },
    Maintenance {
        scope: Option<String>,
        task: MaintenanceTask,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum MaintenanceTask {
    Snapshot,
    Consolidate,
    Prune,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub kind: JobKind,
    pub cron: String,
    pub enabled: bool,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewJob {
    pub name: String,
    pub kind: JobKind,
    pub cron: String,
}

pub fn validate_cron(expr: &str) -> Result<(), SchedulerError> {
    parse_schedule(expr).map(|_| ())
}

pub fn next_after(expr: &str, after_unix: i64) -> Result<Option<i64>, SchedulerError> {
    let schedule = parse_schedule(expr)?;
    let after = Utc
        .timestamp_opt(after_unix, 0)
        .single()
        .ok_or(SchedulerError::InvalidTimestamp(after_unix))?;
    Ok(schedule.after(&after).next().map(|dt| dt.timestamp()))
}

fn parse_schedule(expr: &str) -> Result<Schedule, SchedulerError> {
    let trimmed = expr.trim();
    let field_count = trimmed.split_whitespace().count();
    if field_count != 5 {
        return Err(SchedulerError::InvalidCronFieldCount(expr.to_string()));
    }
    let with_seconds = format!("0 {trimmed}");
    Schedule::from_str(&with_seconds).map_err(|source| SchedulerError::InvalidCron {
        expr: expr.to_string(),
        source,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_valid_five_field_cron() {
        validate_cron("*/15 * * * *").unwrap();
        validate_cron("0 9 * * 1-5").unwrap();
    }

    #[test]
    fn rejects_invalid_cron() {
        assert!(matches!(
            validate_cron("* * * *"),
            Err(SchedulerError::InvalidCronFieldCount(_))
        ));
        assert!(matches!(
            validate_cron("nope * * * *"),
            Err(SchedulerError::InvalidCron { .. })
        ));
    }

    #[test]
    fn computes_next_timestamp_after_known_time() {
        let after = Utc
            .with_ymd_and_hms(2026, 6, 13, 8, 59, 30)
            .unwrap()
            .timestamp();
        let next = next_after("0 9 * * *", after).unwrap().unwrap();
        assert_eq!(
            next,
            Utc.with_ymd_and_hms(2026, 6, 13, 9, 0, 0)
                .unwrap()
                .timestamp()
        );
    }

    #[test]
    fn serializes_and_deserializes_job_kind() {
        let kind = JobKind::Maintenance {
            scope: Some("project:Wukong".to_string()),
            task: MaintenanceTask::Prune,
        };
        let raw = serde_json::to_string(&kind).unwrap();
        let decoded: JobKind = serde_json::from_str(&raw).unwrap();
        assert_eq!(decoded, kind);
    }
}
