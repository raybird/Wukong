use crate::job::{next_after, Job, JobKind, NewJob, SchedulerError};
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Success,
    Failure,
    Interrupted,
}

impl RunStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            RunStatus::Running => "running",
            RunStatus::Success => "success",
            RunStatus::Failure => "failure",
            RunStatus::Interrupted => "interrupted",
        }
    }

    fn parse(raw: &str) -> Result<Self, SchedulerError> {
        match raw {
            "running" => Ok(RunStatus::Running),
            "success" => Ok(RunStatus::Success),
            "failure" => Ok(RunStatus::Failure),
            "interrupted" => Ok(RunStatus::Interrupted),
            other => Err(SchedulerError::UnknownRunStatus(other.to_string())),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobRun {
    pub id: i64,
    pub job_id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub status: RunStatus,
    pub message: String,
}

pub struct SchedulerStore {
    pool: SqlitePool,
}

impl SchedulerStore {
    pub async fn open(db_url: &str) -> Result<Self, SchedulerError> {
        let pool = SqlitePool::connect(db_url).await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scheduler_jobs (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                kind TEXT NOT NULL,
                config_json TEXT NOT NULL,
                cron TEXT NOT NULL,
                enabled INTEGER NOT NULL DEFAULT 1,
                next_run_at INTEGER,
                last_run_at INTEGER,
                locked_by TEXT,
                locked_until INTEGER,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS scheduler_runs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                job_id TEXT NOT NULL,
                started_at INTEGER NOT NULL,
                finished_at INTEGER,
                status TEXT NOT NULL,
                message TEXT NOT NULL DEFAULT '',
                FOREIGN KEY(job_id) REFERENCES scheduler_jobs(id) ON DELETE CASCADE
            )",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    pub async fn add_job(&self, input: NewJob) -> Result<Job, SchedulerError> {
        let now = now_unix();
        let id = uuid::Uuid::new_v4().to_string();
        let next_run_at = next_after(&input.cron, now)?;
        let kind = kind_label(&input.kind);
        let config_json = serde_json::to_string(&input.kind)?;
        sqlx::query(
            "INSERT INTO scheduler_jobs
             (id, name, kind, config_json, cron, enabled, next_run_at, last_run_at, locked_by, locked_until, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 1, ?6, NULL, NULL, NULL, ?7, ?7)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(kind)
        .bind(config_json)
        .bind(&input.cron)
        .bind(next_run_at)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(Job {
            id,
            name: input.name,
            kind: input.kind,
            cron: input.cron,
            enabled: true,
            next_run_at,
            last_run_at: None,
        })
    }

    pub async fn list_jobs(&self) -> Result<Vec<Job>, SchedulerError> {
        let rows = sqlx::query(
            "SELECT id, name, kind, config_json, cron, enabled, next_run_at, last_run_at
             FROM scheduler_jobs ORDER BY created_at, name",
        )
        .fetch_all(&self.pool)
        .await?;
        rows.into_iter().map(row_to_job).collect()
    }

    pub async fn get_job(&self, id: &str) -> Result<Option<Job>, SchedulerError> {
        let row = sqlx::query(
            "SELECT id, name, kind, config_json, cron, enabled, next_run_at, last_run_at
             FROM scheduler_jobs WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        row.map(row_to_job).transpose()
    }

    pub async fn remove_job(&self, id: &str) -> Result<bool, SchedulerError> {
        let result = sqlx::query("DELETE FROM scheduler_jobs WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<bool, SchedulerError> {
        let now = now_unix();
        let next = if enabled {
            let Some(job) = self.get_job(id).await? else {
                return Ok(false);
            };
            next_after(&job.cron, now)?
        } else {
            None
        };
        let result = sqlx::query(
            "UPDATE scheduler_jobs
             SET enabled = ?2, next_run_at = ?3, locked_by = NULL, locked_until = NULL, updated_at = ?4
             WHERE id = ?1",
        )
        .bind(id)
        .bind(if enabled { 1 } else { 0 })
        .bind(next)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn claim_due_jobs(
        &self,
        now: i64,
        worker_id: &str,
        lease_secs: i64,
        limit: i64,
    ) -> Result<Vec<Job>, SchedulerError> {
        let candidates = sqlx::query(
            "SELECT id FROM scheduler_jobs
             WHERE enabled = 1
               AND next_run_at IS NOT NULL
               AND next_run_at <= ?1
               AND (locked_until IS NULL OR locked_until <= ?1)
             ORDER BY next_run_at, created_at
             LIMIT ?2",
        )
        .bind(now)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let mut claimed = Vec::new();
        for row in candidates {
            let id: String = row.get("id");
            let result = sqlx::query(
                "UPDATE scheduler_jobs
                 SET locked_by = ?2, locked_until = ?3, updated_at = ?1
                 WHERE id = ?4
                   AND enabled = 1
                   AND next_run_at IS NOT NULL
                   AND next_run_at <= ?1
                   AND (locked_until IS NULL OR locked_until <= ?1)",
            )
            .bind(now)
            .bind(worker_id)
            .bind(now + lease_secs)
            .bind(&id)
            .execute(&self.pool)
            .await?;
            if result.rows_affected() > 0 {
                if let Some(job) = self.get_job(&id).await? {
                    claimed.push(job);
                }
            }
        }
        Ok(claimed)
    }

    pub async fn claim_job(
        &self,
        id: &str,
        now: i64,
        worker_id: &str,
        lease_secs: i64,
    ) -> Result<Option<Job>, SchedulerError> {
        let result = sqlx::query(
            "UPDATE scheduler_jobs
             SET locked_by = ?2, locked_until = ?3, updated_at = ?1
             WHERE id = ?4
               AND enabled = 1
               AND (locked_until IS NULL OR locked_until <= ?1)",
        )
        .bind(now)
        .bind(worker_id)
        .bind(now + lease_secs)
        .bind(id)
        .execute(&self.pool)
        .await?;
        if result.rows_affected() > 0 {
            self.get_job(id).await
        } else {
            Ok(None)
        }
    }

    pub async fn start_run(&self, job_id: &str, now: i64) -> Result<i64, SchedulerError> {
        let result = sqlx::query(
            "INSERT INTO scheduler_runs (job_id, started_at, finished_at, status, message)
             VALUES (?1, ?2, NULL, ?3, '')",
        )
        .bind(job_id)
        .bind(now)
        .bind(RunStatus::Running.as_str())
        .execute(&self.pool)
        .await?;
        Ok(result.last_insert_rowid())
    }

    pub async fn finish_run(
        &self,
        run_id: i64,
        status: RunStatus,
        message: &str,
        finished_at: i64,
    ) -> Result<(), SchedulerError> {
        sqlx::query(
            "UPDATE scheduler_runs SET status = ?2, message = ?3, finished_at = ?4 WHERE id = ?1",
        )
        .bind(run_id)
        .bind(status.as_str())
        .bind(message)
        .bind(finished_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn complete_job(&self, job: &Job, finished_at: i64) -> Result<(), SchedulerError> {
        let next = if job.enabled {
            next_after(&job.cron, finished_at)?
        } else {
            None
        };
        sqlx::query(
            "UPDATE scheduler_jobs
             SET last_run_at = ?2, next_run_at = ?3, locked_by = NULL, locked_until = NULL, updated_at = ?2
             WHERE id = ?1",
        )
        .bind(&job.id)
        .bind(finished_at)
        .bind(next)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn complete_claimed_job(
        &self,
        job: &Job,
        worker_id: &str,
        finished_at: i64,
    ) -> Result<bool, SchedulerError> {
        let next = if job.enabled {
            next_after(&job.cron, finished_at)?
        } else {
            None
        };
        let result = sqlx::query(
            "UPDATE scheduler_jobs
             SET last_run_at = ?2, next_run_at = ?3, locked_by = NULL, locked_until = NULL, updated_at = ?2
             WHERE id = ?1 AND locked_by = ?4",
        )
        .bind(&job.id)
        .bind(finished_at)
        .bind(next)
        .bind(worker_id)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn interrupt_stale_runs(&self, now: i64) -> Result<u64, SchedulerError> {
        let mut tx = self.pool.begin().await?;
        let result = sqlx::query(
            "UPDATE scheduler_runs
             SET status = ?1, message = ?2, finished_at = ?3
             WHERE status = ?4
               AND EXISTS (
                   SELECT 1 FROM scheduler_jobs
                   WHERE scheduler_jobs.id = scheduler_runs.job_id
                     AND (scheduler_jobs.locked_until IS NULL OR scheduler_jobs.locked_until <= ?3)
               )",
        )
        .bind(RunStatus::Interrupted.as_str())
        .bind("scheduler process ended before the run completed")
        .bind(now)
        .bind(RunStatus::Running.as_str())
        .execute(&mut *tx)
        .await?;
        let changed = result.rows_affected();
        tx.commit().await?;
        Ok(changed)
    }

    pub async fn recent_runs(
        &self,
        job_id: Option<&str>,
        limit: i64,
    ) -> Result<Vec<JobRun>, SchedulerError> {
        let rows = if let Some(job_id) = job_id {
            sqlx::query(
                "SELECT id, job_id, started_at, finished_at, status, message
                 FROM scheduler_runs WHERE job_id = ?1 ORDER BY started_at DESC, id DESC LIMIT ?2",
            )
            .bind(job_id)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        } else {
            sqlx::query(
                "SELECT id, job_id, started_at, finished_at, status, message
                 FROM scheduler_runs ORDER BY started_at DESC, id DESC LIMIT ?1",
            )
            .bind(limit)
            .fetch_all(&self.pool)
            .await?
        };
        rows.into_iter().map(row_to_run).collect()
    }
}

fn row_to_job(row: sqlx::sqlite::SqliteRow) -> Result<Job, SchedulerError> {
    let label: String = row.get("kind");
    let config_json: String = row.get("config_json");
    let kind: JobKind = serde_json::from_str(&config_json)?;
    if kind_label(&kind) != label {
        return Err(SchedulerError::UnknownJobKind(label));
    }
    Ok(Job {
        id: row.get("id"),
        name: row.get("name"),
        kind,
        cron: row.get("cron"),
        enabled: row.get::<i64, _>("enabled") != 0,
        next_run_at: row.get("next_run_at"),
        last_run_at: row.get("last_run_at"),
    })
}

fn row_to_run(row: sqlx::sqlite::SqliteRow) -> Result<JobRun, SchedulerError> {
    let status: String = row.get("status");
    Ok(JobRun {
        id: row.get("id"),
        job_id: row.get("job_id"),
        started_at: row.get("started_at"),
        finished_at: row.get("finished_at"),
        status: RunStatus::parse(&status)?,
        message: row.get("message"),
    })
}

fn kind_label(kind: &JobKind) -> &'static str {
    match kind {
        JobKind::Turn { .. } => "turn",
        JobKind::Maintenance { .. } => "maintenance",
    }
}

fn now_unix() -> i64 {
    chrono::Utc::now().timestamp()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::MaintenanceTask;
    use tempfile::NamedTempFile;

    async fn open_store() -> SchedulerStore {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        SchedulerStore::open(&url).await.unwrap()
    }

    fn new_turn(cron: &str) -> NewJob {
        NewJob {
            name: "turn".to_string(),
            kind: JobKind::Turn {
                scope: "project:T".to_string(),
                prompt: "do it".to_string(),
            },
            cron: cron.to_string(),
        }
    }

    #[tokio::test]
    async fn add_list_get_remove_round_trip() {
        let store = open_store().await;
        let job = store.add_job(new_turn("* * * * *")).await.unwrap();

        assert_eq!(store.get_job(&job.id).await.unwrap(), Some(job.clone()));
        assert_eq!(store.list_jobs().await.unwrap(), vec![job.clone()]);
        assert!(store.remove_job(&job.id).await.unwrap());
        assert_eq!(store.get_job(&job.id).await.unwrap(), None);
    }

    #[tokio::test]
    async fn enabling_recalculates_next_run_and_disabling_prevents_claim() {
        let store = open_store().await;
        let job = store.add_job(new_turn("* * * * *")).await.unwrap();

        assert!(store.set_enabled(&job.id, false).await.unwrap());
        let disabled = store.get_job(&job.id).await.unwrap().unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.next_run_at, None);
        assert!(store
            .claim_due_jobs(i64::MAX / 2, "w1", 60, 10)
            .await
            .unwrap()
            .is_empty());

        assert!(store.set_enabled(&job.id, true).await.unwrap());
        let enabled = store.get_job(&job.id).await.unwrap().unwrap();
        assert!(enabled.enabled);
        assert!(enabled.next_run_at.is_some());
    }

    #[tokio::test]
    async fn claim_is_exclusive_across_workers() {
        let store = open_store().await;
        let job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let now = job.next_run_at.unwrap();

        let first = store.claim_due_jobs(now, "w1", 300, 10).await.unwrap();
        let second = store.claim_due_jobs(now, "w2", 300, 10).await.unwrap();

        assert_eq!(first.len(), 1);
        assert!(second.is_empty());
    }

    #[tokio::test]
    async fn stale_lock_can_be_claimed_after_locked_until() {
        let store = open_store().await;
        let job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let now = job.next_run_at.unwrap();

        assert_eq!(
            store.claim_due_jobs(now, "w1", 10, 10).await.unwrap().len(),
            1
        );
        assert_eq!(
            store
                .claim_due_jobs(now + 11, "w2", 10, 10)
                .await
                .unwrap()
                .len(),
            1
        );
    }

    #[tokio::test]
    async fn completing_job_schedules_next_run() {
        let store = open_store().await;
        let job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let due = job.next_run_at.unwrap();
        let claimed = store
            .claim_due_jobs(due, "w1", 60, 10)
            .await
            .unwrap()
            .remove(0);

        store.complete_job(&claimed, due).await.unwrap();
        let completed = store.get_job(&job.id).await.unwrap().unwrap();

        assert_eq!(completed.last_run_at, Some(due));
        assert!(completed.next_run_at.unwrap() > due);
    }

    #[tokio::test]
    async fn run_history_records_success_and_failure() {
        let store = open_store().await;
        let job = store
            .add_job(NewJob {
                name: "maintenance".to_string(),
                kind: JobKind::Maintenance {
                    scope: None,
                    task: MaintenanceTask::Snapshot,
                },
                cron: "* * * * *".to_string(),
            })
            .await
            .unwrap();

        let run1 = store.start_run(&job.id, 10).await.unwrap();
        store
            .finish_run(run1, RunStatus::Success, "ok", 11)
            .await
            .unwrap();
        let run2 = store.start_run(&job.id, 12).await.unwrap();
        store
            .finish_run(run2, RunStatus::Failure, "bad", 13)
            .await
            .unwrap();

        let runs = store.recent_runs(Some(&job.id), 10).await.unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].status, RunStatus::Failure);
        assert_eq!(runs[0].message, "bad");
        assert_eq!(runs[1].status, RunStatus::Success);
    }

    #[test]
    fn interrupted_status_round_trips() {
        assert_eq!(RunStatus::Interrupted.as_str(), "interrupted");
        assert_eq!(
            RunStatus::parse("interrupted").unwrap(),
            RunStatus::Interrupted
        );
    }

    #[tokio::test]
    async fn interrupt_stale_runs_updates_only_expired_or_missing_leases() {
        let store = open_store().await;
        let expired_job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let active_job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let unlocked_job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let now = expired_job.next_run_at.unwrap();

        store
            .claim_job(&expired_job.id, now, "expired-worker", 10)
            .await
            .unwrap();
        store
            .claim_job(&active_job.id, now, "active-worker", 100)
            .await
            .unwrap();

        let expired_run = store.start_run(&expired_job.id, now).await.unwrap();
        let active_run = store.start_run(&active_job.id, now).await.unwrap();
        let unlocked_run = store.start_run(&unlocked_job.id, now).await.unwrap();
        let before_expired = store.get_job(&expired_job.id).await.unwrap().unwrap();

        let changed = store.interrupt_stale_runs(now + 11).await.unwrap();

        assert_eq!(changed, 2);
        let runs = store.recent_runs(None, 10).await.unwrap();
        let expired = runs.iter().find(|run| run.id == expired_run).unwrap();
        let active = runs.iter().find(|run| run.id == active_run).unwrap();
        let unlocked = runs.iter().find(|run| run.id == unlocked_run).unwrap();
        assert_eq!(expired.status, RunStatus::Interrupted);
        assert_eq!(expired.finished_at, Some(now + 11));
        assert_eq!(
            expired.message,
            "scheduler process ended before the run completed"
        );
        assert_eq!(unlocked.status, RunStatus::Interrupted);
        assert_eq!(active.status, RunStatus::Running);
        assert_eq!(active.finished_at, None);

        let after_expired = store.get_job(&expired_job.id).await.unwrap().unwrap();
        assert_eq!(after_expired.next_run_at, before_expired.next_run_at);
        assert_eq!(after_expired.last_run_at, before_expired.last_run_at);
    }

    #[tokio::test]
    async fn interrupt_stale_runs_is_idempotent_and_preserves_terminal_runs() {
        let store = open_store().await;
        let job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let now = job.next_run_at.unwrap();
        let successful = store.start_run(&job.id, now - 2).await.unwrap();
        store
            .finish_run(successful, RunStatus::Success, "ok", now - 1)
            .await
            .unwrap();
        let stale = store.start_run(&job.id, now).await.unwrap();

        assert_eq!(store.interrupt_stale_runs(now + 1).await.unwrap(), 1);
        assert_eq!(store.interrupt_stale_runs(now + 2).await.unwrap(), 0);

        let runs = store.recent_runs(Some(&job.id), 10).await.unwrap();
        assert_eq!(
            runs.iter().find(|run| run.id == stale).unwrap().status,
            RunStatus::Interrupted
        );
        assert_eq!(
            runs.iter().find(|run| run.id == successful).unwrap().status,
            RunStatus::Success
        );
    }

    #[tokio::test]
    async fn manual_claim_respects_active_lock() {
        let store = open_store().await;
        let job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let now = job.next_run_at.unwrap();

        let claimed = store
            .claim_job(&job.id, now, "manual-1", 300)
            .await
            .unwrap();
        let duplicate = store
            .claim_job(&job.id, now + 1, "manual-2", 300)
            .await
            .unwrap();

        assert!(claimed.is_some());
        assert!(duplicate.is_none());
    }

    #[tokio::test]
    async fn stale_worker_cannot_complete_new_worker_claim() {
        let store = open_store().await;
        let job = store.add_job(new_turn("* * * * *")).await.unwrap();
        let now = job.next_run_at.unwrap();

        let first = store
            .claim_due_jobs(now, "w1", 10, 10)
            .await
            .unwrap()
            .remove(0);
        let second = store
            .claim_due_jobs(now + 11, "w2", 10, 10)
            .await
            .unwrap()
            .remove(0);

        assert!(!store
            .complete_claimed_job(&first, "w1", now + 12)
            .await
            .unwrap());
        assert!(store
            .complete_claimed_job(&second, "w2", now + 13)
            .await
            .unwrap());
        let completed = store.get_job(&job.id).await.unwrap().unwrap();
        assert_eq!(completed.last_run_at, Some(now + 13));
    }
}
