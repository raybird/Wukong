# Scheduler Startup Readiness and Recovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Prevent scheduler jobs from failing during OpenCode startup, reconcile abandoned scheduler runs as interrupted, and deploy the verified fix to the current RunWuKong installation.

**Architecture:** Add a concrete readiness boundary on `AgentBackend`, check it before scheduler claims, and keep generic `AiBackend` implementations unchanged. Add idempotent stale-run recovery in `SchedulerStore`, then layer Docker `service_healthy` ordering over the application-level protection.

**Tech Stack:** Rust, Tokio, SQLx/SQLite, Docker Compose, Bash test scripts

---

## File Map

- Modify `crates/wukong-gateway/src/backend.rs`: expose `AgentBackend::check_ready` and test CLI/server dispatch.
- Modify `crates/wukong-gateway/src/opencode_server.rs`: make the existing health check callable by the backend dispatcher and add a successful-health test server.
- Modify `crates/wukong-scheduler/src/store.rs`: add `RunStatus::Interrupted` and transactional stale-run recovery with store-level tests.
- Modify `crates/wukong-schedulerd/src/main.rs`: recover stale runs on daemon startup and preflight readiness before claims, with daemon-level tests.
- Modify `docker-compose.yml`: add OpenCode healthcheck and healthy dependency conditions.
- Modify `scripts/test-docker-runtime.sh`: assert the healthcheck and all three dependency conditions.
- Modify `/home/raybird/Documents/RunWuKong/docker-compose.yml` during deployment only: mirror the verified Compose readiness changes without touching `.env` or persistent data.
- Create `/tmp/opencode/wukong-readiness.Dockerfile` during deployment only: build local source binaries into the existing runtime image; remove after deployment.

## Task 1: Backend Readiness Boundary

**Files:**
- Modify: `crates/wukong-gateway/src/backend.rs:130-162,438-end`
- Modify: `crates/wukong-gateway/src/opencode_server.rs:93-98,463-527`

- [ ] **Step 1: Add failing readiness dispatch tests**

In `backend.rs`'s existing `tests` module, add imports for `tokio::io::{AsyncReadExt, AsyncWriteExt}` and `tokio::net::TcpListener`, then add this helper and tests:

```rust
async fn health_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0; 1024];
        let _ = socket.read(&mut buf).await.unwrap();
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"healthy\":true}",
            )
            .await
            .unwrap();
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn cli_backend_is_ready_without_launching_command() {
    let backend = AgentBackend::Cli(AgentCliBackend {
        command: vec!["command-that-must-not-run".to_string()],
        workspace: None,
    });

    backend.check_ready().await.unwrap();
}

#[tokio::test]
async fn server_backend_readiness_delegates_to_health_endpoint() {
    let backend = AgentBackend::Server(
        crate::opencode_server::OpencodeServerBackend::from_env(health_server().await, None),
    );

    backend.check_ready().await.unwrap();
}

#[tokio::test]
async fn server_backend_readiness_reports_connection_failure() {
    let backend = AgentBackend::Server(
        crate::opencode_server::OpencodeServerBackend::from_env(
            "http://127.0.0.1:1".to_string(),
            None,
        ),
    );

    let err = backend.check_ready().await.unwrap_err();
    assert!(err.to_string().contains("health_check"), "{err}");
}
```

- [ ] **Step 2: Run the tests and verify the missing method failure**

Run:

```bash
cargo test -p wukong-gateway backend::tests::cli_backend_is_ready_without_launching_command
```

Expected: compilation fails because `AgentBackend::check_ready` does not exist.

- [ ] **Step 3: Expose the server health check and implement dispatch**

Change `OpencodeServerBackend::health_check` visibility in `opencode_server.rs`:

```rust
pub(crate) async fn health_check(&self) -> Result<(), GatewayError> {
    let url = format!("{}/global/health", self.base_url);
    self.send_json("health_check", self.client.get(url))
        .await
        .map(|_| ())
}
```

Add this inherent implementation immediately after `build_backend_from_env` in `backend.rs`:

```rust
impl AgentBackend {
    pub async fn check_ready(&self) -> Result<(), GatewayError> {
        match self {
            AgentBackend::Cli(_) => Ok(()),
            AgentBackend::Server(backend) => backend.health_check().await,
        }
    }
}
```

- [ ] **Step 4: Run all gateway tests**

Run:

```bash
cargo test -p wukong-gateway
```

Expected: all gateway tests pass, including the three new readiness tests and existing `health_check_decode_error_includes_phase`.

- [ ] **Step 5: Commit the readiness boundary**

```bash
git add crates/wukong-gateway/src/backend.rs crates/wukong-gateway/src/opencode_server.rs
git commit -m "feat(gateway): expose backend readiness check"
```

## Task 2: Interrupted Run Status and Recovery

**Files:**
- Modify: `crates/wukong-scheduler/src/store.rs:4-28,240-270,316-341,384-end`

- [ ] **Step 1: Add failing status and stale-recovery tests**

Add these tests to `store.rs`'s existing `tests` module:

```rust
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
```

- [ ] **Step 2: Run the focused tests and verify they fail**

Run:

```bash
cargo test -p wukong-scheduler interrupted_status_round_trips
cargo test -p wukong-scheduler interrupt_stale_runs_updates_only_expired_or_missing_leases
```

Expected: compilation fails because `RunStatus::Interrupted` and `SchedulerStore::interrupt_stale_runs` do not exist.

- [ ] **Step 3: Add the interrupted status**

Update `RunStatus` and its string mapping:

```rust
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
```

- [ ] **Step 4: Implement transactional stale-run recovery**

Add this method before `recent_runs`:

```rust
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
```

- [ ] **Step 5: Run scheduler store tests**

Run:

```bash
cargo test -p wukong-scheduler
```

Expected: all scheduler tests pass; the new tests report two interrupted stale rows, one preserved active row, unchanged schedule fields, and an idempotent second recovery.

- [ ] **Step 6: Commit run recovery**

```bash
git add crates/wukong-scheduler/src/store.rs
git commit -m "feat(scheduler): recover interrupted runs"
```

## Task 3: Scheduler Preflight and Startup Recovery

**Files:**
- Modify: `crates/wukong-schedulerd/src/main.rs:7-14,51-103,105-158,224-end`

- [ ] **Step 1: Add test helpers and failing preflight tests**

Import `Job` with the existing scheduler imports:

```rust
use wukong_scheduler::{ClaimedJobOutcome, ExecutionContext, Job, SchedulerStore};
```

In the existing `tests` module, add:

```rust
use tempfile::NamedTempFile;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use wukong_gateway::backend::AgentBackend;
use wukong_scheduler::{JobKind, NewJob, RunStatus};

async fn open_store() -> (NamedTempFile, SchedulerStore) {
    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    let store = SchedulerStore::open(&url).await.unwrap();
    (file, store)
}

async fn health_server() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut buf = [0; 1024];
        let _ = socket.read(&mut buf).await.unwrap();
        socket
            .write_all(
                b"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 16\r\nConnection: close\r\n\r\n{\"healthy\":true}",
            )
            .await
            .unwrap();
    });
    format!("http://{addr}")
}

async fn due_job(store: &SchedulerStore) -> Job {
    store
        .add_job(NewJob {
            name: "due".to_string(),
            kind: JobKind::Turn {
                scope: "project:test".to_string(),
                prompt: "run".to_string(),
            },
            cron: "* * * * *".to_string(),
        })
        .await
        .unwrap()
}

#[tokio::test]
async fn unavailable_backend_leaves_due_job_unclaimed() {
    let (_file, store) = open_store().await;
    let job = due_job(&store).await;
    let backend = AgentBackend::Server(
        wukong_gateway::opencode_server::OpencodeServerBackend::from_env(
            "http://127.0.0.1:1".to_string(),
            None,
        ),
    );

    let err = claim_ready_jobs(&store, &backend, job.next_run_at.unwrap(), "worker", 300, 10)
        .await
        .unwrap_err();

    assert!(err.contains("health_check"), "{err}");
    assert!(store.recent_runs(None, 10).await.unwrap().is_empty());
    assert_eq!(store.get_job(&job.id).await.unwrap().unwrap(), job);
    assert_eq!(
        store
            .claim_due_jobs(job.next_run_at.unwrap(), "other", 300, 10)
            .await
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
async fn retained_due_job_is_claimed_after_backend_recovers() {
    let (_file, store) = open_store().await;
    let job = due_job(&store).await;
    let unavailable = AgentBackend::Server(
        wukong_gateway::opencode_server::OpencodeServerBackend::from_env(
            "http://127.0.0.1:1".to_string(),
            None,
        ),
    );
    assert!(
        claim_ready_jobs(&store, &unavailable, job.next_run_at.unwrap(), "worker", 300, 10)
            .await
            .is_err()
    );

    let available = AgentBackend::Server(
        wukong_gateway::opencode_server::OpencodeServerBackend::from_env(
            health_server().await,
            None,
        ),
    );
    let claimed = claim_ready_jobs(
        &store,
        &available,
        job.next_run_at.unwrap(),
        "worker",
        300,
        10,
    )
    .await
    .unwrap();

    assert_eq!(claimed, vec![job]);
}

#[tokio::test]
async fn startup_recovery_runs_only_for_daemon_mode() {
    let (_file, store) = open_store().await;
    let job = due_job(&store).await;
    let run = store.start_run(&job.id, 10).await.unwrap();

    assert_eq!(recover_interrupted_runs(&store, true, 20).await.unwrap(), 0);
    assert_eq!(recover_interrupted_runs(&store, false, 20).await.unwrap(), 1);
    assert_eq!(
        store
            .recent_runs(Some(&job.id), 10)
            .await
            .unwrap()
            .into_iter()
            .find(|item| item.id == run)
            .unwrap()
            .status,
        RunStatus::Interrupted
    );
}
```

`wukong_gateway::opencode_server` is already a public module, so these tests construct `OpencodeServerBackend` directly and do not mutate process environment variables.

- [ ] **Step 2: Run the focused tests and verify missing helpers**

Run:

```bash
cargo test -p wukong-schedulerd unavailable_backend_leaves_due_job_unclaimed
cargo test -p wukong-schedulerd startup_recovery_runs_only_for_daemon_mode
```

Expected: compilation fails because `claim_ready_jobs` and `recover_interrupted_runs` do not exist.

- [ ] **Step 3: Add preflight claim and recovery helpers**

Add these functions before `run_scan`:

```rust
async fn recover_interrupted_runs(
    store: &SchedulerStore,
    once: bool,
    now: i64,
) -> Result<u64, String> {
    if once {
        return Ok(0);
    }
    store
        .interrupt_stale_runs(now)
        .await
        .map_err(|e| e.to_string())
}

async fn claim_ready_jobs(
    store: &SchedulerStore,
    backend: &AgentBackend,
    now: i64,
    worker_id: &str,
    lease_secs: i64,
    limit: i64,
) -> Result<Vec<Job>, String> {
    backend.check_ready().await.map_err(|e| e.to_string())?;
    store
        .claim_due_jobs(now, worker_id, lease_secs, limit)
        .await
        .map_err(|e| e.to_string())
}
```

- [ ] **Step 4: Wire startup recovery and scan ordering**

After `SchedulerStore::open` succeeds in `run`, invoke recovery before notifier setup and before the `--once` branch:

```rust
let interrupted = recover_interrupted_runs(&store, cli.once, now_unix()).await?;
if interrupted > 0 {
    eprintln!("recovered {interrupted} interrupted scheduler run(s)");
}
```

Replace the direct claim at the start of `run_scan`:

```rust
let now = now_unix();
let jobs = claim_ready_jobs(store, backend, now, worker_id, lease_secs, limit).await?;
```

Do not catch readiness errors inside `run_scan`; the existing daemon loop logs `warning: scheduler scan failed`, while `--once` propagates the error and exits without job mutation.

- [ ] **Step 5: Run all schedulerd and scheduler tests**

Run:

```bash
cargo test -p wukong-schedulerd
cargo test -p wukong-scheduler
```

Expected: all tests pass. The unavailable test proves no run, lock, or schedule mutation; the recovery test proves daemon-only startup reconciliation.

- [ ] **Step 6: Commit daemon behavior**

```bash
git add crates/wukong-schedulerd/src/main.rs
git commit -m "fix(scheduler): wait for backend before claiming jobs"
```

## Task 4: Docker Healthy Dependency Ordering

**Files:**
- Modify: `scripts/test-docker-runtime.sh:9-19,21-61`
- Modify: `docker-compose.yml:39-57,59-88,90-145,147-174`

- [ ] **Step 1: Add failing Docker runtime assertions**

Add this helper after `require_in_file` in `scripts/test-docker-runtime.sh`:

```bash
require_count_in_file() {
    local pattern="$1"
    local expected="$2"
    local file="$3"
    local message="$4"
    local actual

    actual=$(grep -Fc -- "$pattern" "$file" || true)
    if [[ "$actual" != "$expected" ]]; then
        echo "FAIL: $message" >&2
        echo "expected $expected occurrences of '$pattern', found $actual" >&2
        exit 1
    fi
}
```

Add these assertions before the scheduler profile check:

```bash
require_in_file "curl -fsS http://localhost:4096/global/health || exit 1" "$compose_file" \
    "opencode server must expose a Compose healthcheck"
require_count_in_file "condition: service_healthy" 3 "$compose_file" \
    "web, telegram, and scheduler must wait for a healthy opencode server"
```

- [ ] **Step 2: Run the script and verify it fails**

Run:

```bash
bash scripts/test-docker-runtime.sh
```

Expected: FAIL because the OpenCode healthcheck pattern is absent.

- [ ] **Step 3: Add OpenCode healthcheck**

Add under `opencode-server`, before `restart` or immediately after it:

```yaml
    healthcheck:
      test:
        [
          "CMD-SHELL",
          "curl -fsS http://localhost:4096/global/health || exit 1",
        ]
      interval: 2s
      timeout: 2s
      retries: 30
      start_period: 2s
```

- [ ] **Step 4: Require healthy dependency for all consumers**

Replace each short-form dependency in `wukong-telegram`, `wukong-web`, and `wukong-schedulerd` with:

```yaml
    depends_on:
      opencode-server:
        condition: service_healthy
```

- [ ] **Step 5: Validate the script and resolved Compose model**

Run:

```bash
bash scripts/test-docker-runtime.sh
docker compose config
```

Expected: script prints `docker runtime persistence checks passed`; Compose exits 0 and shows `condition: service_healthy` for exactly three services.

- [ ] **Step 6: Commit Docker ordering**

```bash
git add docker-compose.yml scripts/test-docker-runtime.sh
git commit -m "fix(docker): wait for opencode server readiness"
```

## Task 5: Full Verification and Change Review

**Files:**
- Verify only; no planned source edits.

- [ ] **Step 1: Format and lint**

Run:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: both commands exit 0 with no formatting diff or Clippy warning.

- [ ] **Step 2: Run targeted and full test suites**

Run:

```bash
cargo test -p wukong-gateway
cargo test -p wukong-scheduler
cargo test -p wukong-schedulerd
bash scripts/test-docker-runtime.sh
cargo test --workspace
docker compose config
```

Expected: every command exits 0; no test failures.

- [ ] **Step 3: Inspect the complete implementation diff**

Run:

```bash
git status --short
git diff 0e1fd37 -- crates/wukong-gateway/src/backend.rs crates/wukong-gateway/src/opencode_server.rs crates/wukong-scheduler/src/store.rs crates/wukong-schedulerd/src/main.rs docker-compose.yml scripts/test-docker-runtime.sh
```

Expected: only the planned readiness, recovery, tests, and Compose changes appear. Existing unrelated `AGENTS.md`, `CLAUDE.md`, release spec, and `scratch/` changes remain untouched.

- [ ] **Step 4: Run GitNexus change detection**

Run `gitnexus_detect_changes({scope: "compare", base_ref: "0e1fd37", repo: "Wukong"})`.

Expected: affected flows are limited to gateway server readiness, scheduler scan/execution, scheduler run history, and Docker configuration. Investigate any unrelated high-risk flow before deployment.

## Task 6: Deploy the Verified Source Build

**Files:**
- Modify outside source repo: `/home/raybird/Documents/RunWuKong/docker-compose.yml`
- Create temporarily: `/tmp/opencode/wukong-readiness.Dockerfile`
- Persistent data preserved: Docker volume `runwukong_wukong-data`

- [ ] **Step 1: Capture rollback state and current schedules**

Run:

```bash
docker image tag wukong:latest wukong:pre-startup-readiness-fix
docker compose ps -a
docker image inspect wukong:latest --format '{{.Id}} {{.Created}}'
```

Run from `/home/raybird/Documents/RunWuKong`. Expected: all four long-running services are listed and the rollback image tag is created without changing containers.

Verify the approved temp parent, create a snapshot directory, copy SQLite's database/WAL/SHM files, and query them with host `sqlite3`:

```bash
ls -ld /tmp/opencode
mkdir /tmp/opencode/runwukong-before-readiness
docker cp wukong-schedulerd:/data/memory.db /tmp/opencode/runwukong-before-readiness/memory.db
docker cp wukong-schedulerd:/data/memory.db-wal /tmp/opencode/runwukong-before-readiness/memory.db-wal
docker cp wukong-schedulerd:/data/memory.db-shm /tmp/opencode/runwukong-before-readiness/memory.db-shm
sqlite3 -header -column /tmp/opencode/runwukong-before-readiness/memory.db \
  "SELECT id, name, next_run_at, last_run_at FROM scheduler_jobs ORDER BY id; SELECT id, job_id, status, started_at, finished_at FROM scheduler_runs WHERE id >= 285 ORDER BY id;"
```

```sql
SELECT id, name, next_run_at, last_run_at FROM scheduler_jobs ORDER BY id;
SELECT id, job_id, status, started_at, finished_at
FROM scheduler_runs
WHERE id >= 285
ORDER BY id;
```

Expected: run 288 is `running`; preserve this output for post-deploy comparison. Remove `/tmp/opencode/runwukong-before-readiness` after the post-deploy comparison.

- [ ] **Step 2: Mirror only the verified Compose changes into RunWuKong**

Update `/home/raybird/Documents/RunWuKong/docker-compose.yml` with the exact OpenCode healthcheck and three long-form `service_healthy` dependencies committed in Task 4. Do not replace the entire file because the deployed bundle may intentionally differ in unrelated version-specific settings. Run:

```bash
docker compose config
```

Expected: exit 0 with the deployed `.env`; no environment or volume definition changes.

- [ ] **Step 3: Build a temporary source-based runtime image**

Create `/tmp/opencode/wukong-readiness.Dockerfile` with:

```dockerfile
FROM rust:bookworm AS builder
WORKDIR /src
COPY . .
RUN cargo build --release --locked \
    -p wukong-cli \
    -p wukong-telegram \
    -p wukong-web \
    -p wukong-schedulerd

FROM wukong:pre-startup-readiness-fix
COPY --from=builder /src/target/release/wukong /usr/local/bin/wukong
COPY --from=builder /src/target/release/wukong-telegram /usr/local/bin/wukong-telegram
COPY --from=builder /src/target/release/wukong-web /usr/local/bin/wukong-web
COPY --from=builder /src/target/release/wukong-schedulerd /usr/local/bin/wukong-schedulerd
```

Run from `/home/raybird/Documents/RCodes/Wukong`:

```bash
docker build -f /tmp/opencode/wukong-readiness.Dockerfile -t wukong:readiness-local .
docker run --rm wukong:readiness-local wukong --help
docker run --rm wukong:readiness-local wukong-schedulerd --help
docker image tag wukong:readiness-local wukong:latest
```

Expected: build exits 0 and both binary smoke tests print usage. Remove `/tmp/opencode/wukong-readiness.Dockerfile` after the image is tagged.

- [ ] **Step 4: Recreate services without invoking the release Dockerfile**

Run from `/home/raybird/Documents/RunWuKong`:

```bash
docker compose up -d --no-build --force-recreate
docker compose ps -a
```

Expected: `opencode-server` reaches healthy before Web, Telegram, and schedulerd start; Web becomes healthy; no service enters a restart loop.

- [ ] **Step 5: Verify readiness and startup logs**

Run:

```bash
docker exec wukong-schedulerd curl -fsS http://opencode-server:4096/global/health
docker compose logs --no-color --timestamps --since 10m opencode-server wukong-schedulerd
```

Expected: health JSON reports healthy; logs show OpenCode listening before scheduler work and contain no `opencode server health_check failed` job failure. The scheduler logs `recovered 1 interrupted scheduler run(s)` if run 288 was still stale at startup.

- [ ] **Step 6: Verify database recovery without replay**

Create a second snapshot using the same three-file copy procedure, then query:

```bash
mkdir /tmp/opencode/runwukong-after-readiness
docker cp wukong-schedulerd:/data/memory.db /tmp/opencode/runwukong-after-readiness/memory.db
docker cp wukong-schedulerd:/data/memory.db-wal /tmp/opencode/runwukong-after-readiness/memory.db-wal
docker cp wukong-schedulerd:/data/memory.db-shm /tmp/opencode/runwukong-after-readiness/memory.db-shm
sqlite3 -header -column /tmp/opencode/runwukong-after-readiness/memory.db \
  "SELECT id, job_id, status, message, started_at, finished_at FROM scheduler_runs WHERE id >= 285 ORDER BY id; SELECT id, name, next_run_at, last_run_at FROM scheduler_jobs ORDER BY id;"
```

```sql
SELECT id, job_id, status, message, started_at, finished_at
FROM scheduler_runs
WHERE id >= 285
ORDER BY id;
SELECT id, name, next_run_at, last_run_at FROM scheduler_jobs ORDER BY id;
```

Expected: run 288 is `interrupted` with message `scheduler process ended before the run completed`; runs 285-287 and 289 remain `failure`; no replacement run for 288 was inserted by recovery; job schedule timestamps match the pre-deploy snapshot except for jobs legitimately executed after that snapshot.

After comparison, remove both `/tmp/opencode/runwukong-before-readiness` and `/tmp/opencode/runwukong-after-readiness`.

- [ ] **Step 7: Observe a scan and preserve rollback instructions**

Wait at least one configured scheduler tick (default 60 seconds), then run:

```bash
docker compose logs --no-color --timestamps --since 2m wukong-schedulerd
docker compose ps -a
```

Expected: no startup readiness failure and all services remain running/healthy.

If deployment verification fails, restore without touching volumes:

```bash
docker image tag wukong:pre-startup-readiness-fix wukong:latest
docker compose up -d --no-build --force-recreate
```

Do not use `docker compose down -v`; the `runwukong_wukong-data` volume must remain intact.
