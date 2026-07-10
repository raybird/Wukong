# Scheduler Startup Readiness and Recovery Design

## Context

On 2026-07-10, the host restarted at approximately 18:30 and 20:54 local time. Docker restored the Wukong services together because they use `restart: unless-stopped`. The scheduler's first Tokio interval tick fired immediately, while the OpenCode server needed another one to three seconds before it listened on port 4096.

The resulting sequence exposed three related gaps:

1. Compose `depends_on` waits only for the OpenCode container to start, not for `/global/health` to succeed.
2. The scheduler claims due jobs before checking whether its configured backend is ready.
3. A host interruption can leave `scheduler_runs.status = 'running'` indefinitely.

Runs 285, 286, 287, and 289 were recorded as failures because the OpenCode health request could not connect. Run 288 was interrupted by the second host restart and remains recorded as running. Run 289 later claimed the same overdue job and advanced it to its next cron occurrence, so run 288 can be corrected to `interrupted` but must not be replayed automatically.

## Goals

- Prevent dependent Docker services from starting before the OpenCode server is healthy.
- Keep due scheduler jobs unclaimed when the configured agent backend is temporarily unavailable.
- Retry retained due jobs naturally on a later scheduler scan without recording a failure or advancing their cron schedule.
- Reconcile stale running scheduler records to an explicit `interrupted` terminal status.
- Preserve existing behavior for failures that occur after a job actually begins execution.
- Apply the fix to the canonical source repository and then rebuild the current `~/Documents/RunWuKong` deployment.

## Non-Goals

- Do not add a generic retry framework for all gateway or agent failures.
- Do not retry a job after execution has begun; retrying an operation with side effects could duplicate work.
- Do not automatically replay historical failed runs 285, 286, 287, or 289.
- Do not automatically replay historical interrupted run 288 because its job was subsequently claimed and rescheduled by run 289.
- Do not change cron expressions, scheduler scan frequency, or lease duration.
- Do not introduce a database schema migration solely for the new status; `scheduler_runs.status` is already unconstrained text.

## Design

### Docker Readiness

Add a healthcheck to `opencode-server` that requests its existing unauthenticated health endpoint:

```text
GET http://localhost:4096/global/health
```

The check should use `curl -fsS`, a short startup interval, a bounded timeout, and enough retries for a normal cold start. Change `wukong-telegram`, `wukong-web`, and `wukong-schedulerd` to long-form `depends_on` entries with `condition: service_healthy`.

This is the first line of defense for the packaged Docker topology. It does not replace application-level readiness because the scheduler may run outside Compose or the server may become unavailable later.

### Backend Readiness Boundary

Add an inherent readiness method to `AgentBackend` rather than expanding the generic `AiBackend` trait:

- `AgentBackend::Cli` returns ready immediately because there is no persistent remote service to probe before a command is launched.
- `AgentBackend::Server` delegates to the existing OpenCode `/global/health` request.

Keep the readiness method on the concrete dispatch enum so existing test backends and other `AiBackend` implementations do not need boilerplate readiness implementations. The existing per-run OpenCode health check remains in place to guard the race between preflight and execution.

### Scheduler Scan Ordering

Change `run_scan` to check backend readiness before `claim_due_jobs`:

```text
tick
  -> backend readiness check
  -> if unavailable: return scan warning without database mutation
  -> if ready: claim due jobs
  -> execute and persist each claimed job as today
```

When readiness fails:

- Do not claim any jobs.
- Do not insert a scheduler run.
- Do not advance `next_run_at`.
- Do not send a Telegram job-failure notification.
- Return an error to the existing scheduler loop, which logs one warning for that scan.

The next tick repeats the preflight. Once the backend is healthy, the still-due jobs are claimed and run normally.

The `--once` path follows the same ordering. If its backend is unavailable, it exits unsuccessfully without mutating job state.

### Interrupted Run Recovery

Extend `RunStatus` with `Interrupted`, serialized as `interrupted` and accepted by status parsing.

Add a `SchedulerStore` recovery operation that runs once during scheduler daemon startup, after the store opens and before the first scan. It finds running records whose associated job no longer has a valid lease at the recovery timestamp:

```text
scheduler_runs.status = 'running'
AND (
  scheduler_jobs.locked_until IS NULL
  OR scheduler_jobs.locked_until <= recovery_time
)
```

The recovery operation updates matching rows in one transaction:

- `status = 'interrupted'`
- `finished_at = recovery_time`
- `message = 'scheduler process ended before the run completed'`

It does not alter `next_run_at`, `last_run_at`, or job locks. Existing claim semantics already allow an expired lease to be reclaimed. Leaving schedule fields unchanged ensures a future interrupted occurrence remains due unless a later run has already claimed and advanced that job.

Recovery must be idempotent: a second call finds no `running` rows that were already changed to `interrupted`.

For the current deployment, run 288 will become `interrupted`. It will not be replayed because run 289 already advanced the same job to its next cron occurrence.

## Data Flow

```text
Docker starts opencode-server
  -> Compose healthcheck waits for /global/health
  -> dependent services start after healthy
  -> schedulerd opens SchedulerStore
  -> stale expired running records become interrupted
  -> immediate first tick checks AgentBackend readiness
  -> due jobs are claimed only when ready
  -> normal execution records success or failure and advances cron
```

## Error Handling

- A Compose healthcheck failure keeps dependent services pending instead of starting them against an unavailable server.
- A scheduler readiness failure is a scan-level infrastructure error, not a job execution failure.
- A readiness failure after a prior successful preflight is still handled by the existing execution path and recorded as a normal failure. This narrow race is intentionally not retried because execution may already have side effects.
- A stale-run recovery database error fails scheduler startup rather than silently leaving inconsistent history.
- Active leases are not interrupted. This avoids changing records that another scheduler process may still own.
- CLI backend command availability is not preflighted. Process spawn errors remain execution failures, matching current behavior.

## Tests

### Gateway

- Server backend readiness succeeds when `/global/health` returns successful JSON.
- Server backend readiness returns the existing phase-specific gateway error when the health request fails.
- CLI backend readiness returns success without launching a command.

### Scheduler Store

- `RunStatus::Interrupted` serializes to and parses from `interrupted`.
- Recovery marks an expired-lease running record as interrupted and sets its finish time and message.
- Recovery marks a running record interrupted when the corresponding job has no lease.
- Recovery leaves a running record with an active lease unchanged.
- Recovery leaves success, failure, and already-interrupted records unchanged.
- Running recovery twice is idempotent.
- Recovery does not modify job schedule fields.

### Scheduler Daemon

- An unavailable server backend prevents `claim_due_jobs` effects: no run row, no lock, and no schedule advancement.
- A later scan after readiness recovers claims the retained due job.
- `--once` returns an error without mutating the due job when readiness fails.
- Startup invokes interrupted-run recovery before the first scan.

### Compose and Regression Verification

Run:

```bash
cargo test -p wukong-gateway
cargo test -p wukong-scheduler
cargo test -p wukong-schedulerd
cargo test --workspace
docker compose config
```

The full workspace test protects existing CLI, Web, Telegram, scheduler, and gateway behavior.

## Deployment

After source verification:

1. Copy the verified `docker-compose.yml` into `~/Documents/RunWuKong` without overwriting `.env`, workspace content, or persistent volumes.
2. Preserve the currently deployed image under a rollback tag.
3. Build all four Wukong binaries from `~/Documents/RCodes/Wukong` in a temporary multi-stage Dockerfile. Its builder stage compiles the canonical working tree; its runtime stage starts from the existing Wukong runtime image and replaces only `wukong`, `wukong-telegram`, `wukong-web`, and `wukong-schedulerd`.
4. Tag that locally built runtime image as `wukong:latest`. Do not modify the release-oriented project Dockerfile, which intentionally downloads published binaries.
5. Recreate the current services with `docker compose up -d --no-build`, preventing Compose from replacing the local source build with binaries from the latest published release.
6. Verify that `opencode-server` becomes healthy before dependent services start.
7. Verify `/global/health` from the scheduler container.
8. Verify run 288 is `interrupted`, not replayed, and existing jobs retain their expected next cron times.
9. Observe at least one scheduler scan with no new startup health-check failure.

The temporary deployment Dockerfile lives outside both repositories and is removed after the image is built. No direct SQLite edit is part of deployment; startup recovery performs the status correction through application code. A later official Wukong release can replace this local image through the normal release-bundle upgrade path.

## Compatibility

- Existing databases remain readable because the status column is text and no table shape changes.
- Older binaries reading a database after an `interrupted` row is written will reject that unknown status. Deployment must therefore upgrade all Wukong components that read scheduler history together.
- Non-Docker installations gain scheduler preflight and stale-run recovery without depending on Compose.
