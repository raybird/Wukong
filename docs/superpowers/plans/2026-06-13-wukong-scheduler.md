# Wukong Scheduler V1 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use `superpowers:subagent-driven-development` (recommended) or `superpowers:executing-plans` to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a background scheduling subsystem to Wukong with CLI-managed cron jobs and a dedicated daemon executor.

**V1 Scope:**
- Add scheduled conversational turns: `Turn { prompt, scope }`.
- Add scheduled memory maintenance: `Maintenance { task, scope }` for `snapshot`, `consolidate`, and `prune`.
- Add CLI management commands: add/list/remove/enable/disable/trigger.
- Add a dedicated `wukong-schedulerd` binary that runs due jobs.
- Persist job definitions and run history in the existing SQLite memory database.

**Out of Scope for V1:**
- Web settings UI.
- Telegram notifications.
- Event-triggered jobs.
- Fixed role/skill jobs. V1 uses the existing planner in the Wukong turn pipeline.
- Embedded scheduler loops in `wukong-web`, `wukong-telegram`, or REPL.

**Architecture:**
- Create `wukong-runtime` for shared runtime operations currently duplicated or trapped in `wukong-cli`.
- Create `wukong-scheduler` for job model, sqlx-backed store, cron parsing, claiming, and execution orchestration.
- Create `wukong-schedulerd` as the only background executor in V1.
- Extend `wukong` CLI with `schedule` management commands that only mutate/query scheduler state or trigger one job explicitly.

---

## Dependency Direction

```text
wukong-cli ───────────────┐
                          ├──> wukong-runtime ──> shared pipeline dependencies
wukong-schedulerd ────────┘        │
                                   ├──> wukong-memory
                                   ├──> wukong-gateway
                                   └──> wukong-orchestrator / wukong-skills

wukong-cli ───────────────┐
                          ├──> wukong-scheduler ──> wukong-memory store/sqlx types
wukong-schedulerd ────────┘
```

Rules:
- `wukong-scheduler` must not depend on `wukong-cli`.
- `wukong-cli` must not be the owner of shared runtime behavior.
- `wukong-runtime` owns executable behavior that CLI, Web, Telegram, and schedulerd can reuse.
- No crate dependency cycles are allowed.

---

## File Structure

```text
crates/wukong-runtime/
├── Cargo.toml
└── src/
    ├── lib.rs              # shared runtime exports
    ├── turn.rs             # run_turn and session passthrough API
    └── maintenance.rs      # snapshot/consolidate/prune helper API

crates/wukong-scheduler/
├── Cargo.toml
└── src/
    ├── lib.rs              # public scheduler domain API
    ├── job.rs              # Job, JobKind, MaintenanceTask, cron helpers
    ├── store.rs            # sqlx-backed CRUD, run history, claim lock
    └── executor.rs         # execute claimed jobs via wukong-runtime

crates/wukong-schedulerd/
├── Cargo.toml
└── src/main.rs             # daemon loop and runtime wiring

crates/wukong-gateway/src/cli.rs
└── add schedule subcommands to existing clap types

crates/wukong-cli/src/main.rs
└── dispatch schedule commands and reuse wukong-runtime
```

---

## Data Model

Use the existing SQLite database URL from `GatewayConfig::db_url` / `WUKONG_MEMORY_DB`. Use `sqlx`, consistent with `wukong-memory`.

### `scheduler_jobs`

```sql
CREATE TABLE IF NOT EXISTS scheduler_jobs (
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
);
```

### `scheduler_runs`

```sql
CREATE TABLE IF NOT EXISTS scheduler_runs (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    status TEXT NOT NULL,
    message TEXT NOT NULL DEFAULT '',
    FOREIGN KEY(job_id) REFERENCES scheduler_jobs(id) ON DELETE CASCADE
);
```

Notes:
- `next_run_at` is calculated when a job is added or enabled.
- `locked_by` and `locked_until` implement a simple lease to avoid duplicate execution.
- `scheduler_runs.status` is `running`, `success`, or `failure`.

---

## Cron Format

V1 accepts standard 5-field cron from the CLI: `minute hour day-of-month month day-of-week`.

Implementation rule:
- Convert 5-field cron to the `cron` crate format internally by prefixing seconds: `0 <expr>`.
- Reject expressions that do not parse after conversion.
- Store the original user-facing 5-field expression in the DB.

Examples:
- `*/15 * * * *` runs every 15 minutes.
- `0 9 * * 1-5` runs at 09:00 Monday-Friday.

---

## Task 1: Create `wukong-runtime` Crate

**Files:**
- Create: `crates/wukong-runtime/Cargo.toml`
- Create: `crates/wukong-runtime/src/lib.rs`
- Create: `crates/wukong-runtime/src/turn.rs`
- Create: `crates/wukong-runtime/src/maintenance.rs`
- Modify: root `Cargo.toml`
- Modify: `crates/wukong-cli/Cargo.toml`
- Modify: `crates/wukong-cli/src/lib.rs`
- Modify: `crates/wukong-cli/src/main.rs`

- [ ] **Step 1: Create crate skeleton**

Add `wukong-runtime` to workspace members.

Dependencies:
- `wukong-memory`
- `wukong-gateway`
- `wukong-orchestrator`
- `wukong-skills`
- `thiserror`

- [ ] **Step 2: Move shared turn API**

Move the existing `WukongError`, `TurnOutput`, `run_turn`, and `run_turn_session_passthrough` from `wukong-cli/src/lib.rs` into `wukong-runtime/src/turn.rs`.

Keep behavior unchanged:
- recall relevant memory
- plan role/skill chain
- stream backend events
- preserve final opencode session per scope
- remember user input and final assistant output

- [ ] **Step 3: Re-export runtime APIs from `wukong-cli` for compatibility inside the repo**

In `wukong-cli/src/lib.rs`, keep CLI-specific modules and re-export runtime types/functions:

```rust
pub use wukong_runtime::{run_turn, run_turn_session_passthrough, TurnOutput, WukongError};
```

- [ ] **Step 4: Extract maintenance helpers**

Create runtime helpers matching current memory API exactly:

```rust
pub async fn memory_snapshot(memory: &Memory, scope: Option<&str>) -> Result<String, WukongError>;
pub async fn memory_consolidate(
    memory: &Memory,
    backend: &AgentCliBackend,
    scope: &str,
    dry_run: bool,
) -> Result<String, WukongError>;
pub async fn memory_prune(
    memory: &Memory,
    scope: Option<&str>,
    dry_run: bool,
) -> Result<String, WukongError>;
```

These helpers should return human-readable text so CLI and scheduler can both persist/report results.

- [ ] **Step 5: Update CLI memory command dispatch**

Replace duplicated memory operation logic in `wukong-cli/src/main.rs` with calls to `wukong-runtime::maintenance` helpers.

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p wukong-runtime
cargo test -p wukong-cli
```

Expected: all existing CLI/runtime behavior remains unchanged.

---

## Task 2: Create `wukong-scheduler` Job Model

**Files:**
- Create: `crates/wukong-scheduler/Cargo.toml`
- Create: `crates/wukong-scheduler/src/lib.rs`
- Create: `crates/wukong-scheduler/src/job.rs`
- Modify: root `Cargo.toml`

- [ ] **Step 1: Create crate skeleton**

Dependencies:
- `chrono`
- `cron`
- `serde`
- `serde_json`
- `thiserror`
- `uuid`

If `chrono` or `uuid` are not already workspace dependencies, add them at the workspace root.

- [ ] **Step 2: Define job types**

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum JobKind {
    Turn { scope: String, prompt: String },
    Maintenance { scope: Option<String>, task: MaintenanceTask },
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
```

- [ ] **Step 3: Add cron helper API**

```rust
pub fn validate_cron(expr: &str) -> Result<(), SchedulerError>;
pub fn next_after(expr: &str, after_unix: i64) -> Result<Option<i64>, SchedulerError>;
```

Rules:
- Accept only 5-field expressions from users.
- Prefix `0 ` internally before using `cron::Schedule`.
- Return a helpful validation error when parse fails.

- [ ] **Step 4: Test job/cron behavior**

Tests:
- accepts valid 5-field cron
- rejects invalid cron
- computes a next timestamp after a known time
- serializes/deserializes `JobKind`

- [ ] **Step 5: Verify**

Run:

```bash
cargo test -p wukong-scheduler job
```

---

## Task 3: Implement Scheduler Store with sqlx

**Files:**
- Create: `crates/wukong-scheduler/src/store.rs`
- Modify: `crates/wukong-scheduler/src/lib.rs`

- [ ] **Step 1: Add dependencies**

Use the workspace `sqlx` dependency with SQLite runtime features already present in the repo.

- [ ] **Step 2: Implement `SchedulerStore`**

```rust
pub struct SchedulerStore {
    pool: sqlx::SqlitePool,
}
```

Constructor:

```rust
pub async fn open(db_url: &str) -> Result<Self, SchedulerError>;
```

`open` must create `scheduler_jobs` and `scheduler_runs` with idempotent `CREATE TABLE IF NOT EXISTS` statements.

- [ ] **Step 3: Implement CRUD**

```rust
pub async fn add_job(&self, input: NewJob) -> Result<Job, SchedulerError>;
pub async fn list_jobs(&self) -> Result<Vec<Job>, SchedulerError>;
pub async fn get_job(&self, id: &str) -> Result<Option<Job>, SchedulerError>;
pub async fn remove_job(&self, id: &str) -> Result<bool, SchedulerError>;
pub async fn set_enabled(&self, id: &str, enabled: bool) -> Result<bool, SchedulerError>;
```

Rules:
- `add_job` validates cron and initializes `next_run_at`.
- `set_enabled(true)` recalculates `next_run_at` from now.
- `set_enabled(false)` clears lock fields and may keep `next_run_at` unchanged or set it null; pick one and test it. Recommended: set null while disabled.

- [ ] **Step 4: Implement due job claiming**

```rust
pub async fn claim_due_jobs(
    &self,
    now: i64,
    worker_id: &str,
    lease_secs: i64,
    limit: i64,
) -> Result<Vec<Job>, SchedulerError>;
```

Rules:
- Claim only `enabled = 1` jobs whose `next_run_at <= now`.
- Do not claim rows whose `locked_until > now`.
- Set `locked_by = worker_id`, `locked_until = now + lease_secs` atomically.
- Safe V1 implementation can fetch candidates then update each row with a guarded `WHERE` clause and keep only rows updated by this worker.

- [ ] **Step 5: Implement run history and completion**

```rust
pub async fn start_run(&self, job_id: &str, now: i64) -> Result<i64, SchedulerError>;
pub async fn finish_run(&self, run_id: i64, status: RunStatus, message: &str, finished_at: i64) -> Result<(), SchedulerError>;
pub async fn complete_job(&self, job: &Job, finished_at: i64) -> Result<(), SchedulerError>;
pub async fn recent_runs(&self, job_id: Option<&str>, limit: i64) -> Result<Vec<JobRun>, SchedulerError>;
```

`complete_job` must:
- set `last_run_at`
- compute and set the next run from the previous finish time
- clear `locked_by` and `locked_until`

- [ ] **Step 6: Tests**

Tests:
- add/list/get/remove round trip
- enabling recalculates `next_run_at`
- disabling prevents claim
- claim is exclusive across two worker IDs
- stale lock can be claimed after `locked_until`
- completing a job schedules the next run
- run history records success/failure

- [ ] **Step 7: Verify**

Run:

```bash
cargo test -p wukong-scheduler store
```

---

## Task 4: Implement Scheduler Executor

**Files:**
- Create: `crates/wukong-scheduler/src/executor.rs`
- Modify: `crates/wukong-scheduler/Cargo.toml`

- [ ] **Step 1: Add dependencies**

Add dependencies:
- `wukong-runtime`
- `wukong-memory`
- `wukong-gateway`

- [ ] **Step 2: Define execution API**

```rust
pub struct ExecutionContext<'a> {
    pub memory: &'a wukong_memory::Memory,
    pub backend: &'a wukong_gateway::backend::AgentCliBackend,
    pub base_config: &'a wukong_gateway::config::GatewayConfig,
}

pub struct ExecutionOutput {
    pub success: bool,
    pub message: String,
}

pub async fn execute_job(ctx: &ExecutionContext<'_>, job: &Job) -> ExecutionOutput;
```

- [ ] **Step 3: Implement turn jobs**

For `JobKind::Turn { scope, prompt }`:
- clone `base_config`
- override `cfg.scope = scope.clone()`
- run `wukong_runtime::run_turn`
- use no-op stream and role callbacks
- return final text on success
- return error string on failure

- [ ] **Step 4: Implement maintenance jobs**

For `JobKind::Maintenance`:
- `Snapshot` calls `memory_snapshot`
- `Consolidate` calls `memory_consolidate` with `dry_run = false`
- `Prune` calls `memory_prune` with `dry_run = false`

- [ ] **Step 5: Tests**

Use a mock backend where possible. If `AgentCliBackend` makes mocking hard, split executor over a small runtime trait local to `wukong-scheduler` so tests do not shell out to opencode.

Tests:
- turn job overrides scope
- turn job returns failure message when backend fails
- snapshot maintenance returns text
- prune/consolidate use the provided scope

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p wukong-scheduler executor
```

---

## Task 5: Add CLI Schedule Commands

**Files:**
- Modify: `crates/wukong-gateway/src/cli.rs`
- Modify: `crates/wukong-cli/src/main.rs`
- Modify: `crates/wukong-cli/Cargo.toml`

- [ ] **Step 1: Extend clap types**

Add top-level command:

```rust
Command::Schedule { op: ScheduleOp }
```

Add subcommands:

```rust
pub enum ScheduleOp {
    List,
    AddTurn { name: String, cron: String, scope: String, prompt: String },
    AddMaintenance { name: String, cron: String, scope: Option<String>, task: ScheduleMaintenanceTaskArg },
    Rm { id: String },
    Enable { id: String },
    Disable { id: String },
    Trigger { id: String },
    Runs { id: Option<String>, limit: i64 },
}
```

CLI names:
- `wukong schedule list`
- `wukong schedule add-turn --name ... --cron ... --scope ... --prompt ...`
- `wukong schedule add-maintenance --name ... --cron ... --task snapshot|consolidate|prune [--scope ...]`
- `wukong schedule rm --id ...`
- `wukong schedule enable --id ...`
- `wukong schedule disable --id ...`
- `wukong schedule trigger --id ...`
- `wukong schedule runs [--id ...] [--limit 20]`

- [ ] **Step 2: Implement dispatch in CLI main**

Before prompt/REPL handling, dispatch `Command::Schedule`.

Rules:
- Use `SchedulerStore::open(&cfg.db_url)`.
- `list` prints ID, enabled, cron, next_run_at, name, kind.
- `add-*` prints created job ID and next run.
- `trigger` executes exactly one job immediately in the current process and records a run. It should not require schedulerd to be running.
- `trigger` must not alter the cron schedule unless execution completes; after completion it should update next run like daemon execution.

- [ ] **Step 3: Tests**

Tests:
- clap parses all schedule subcommands
- invalid task is rejected by clap
- schedule command does not conflict with prompt args

- [ ] **Step 4: Verify**

Run:

```bash
cargo test -p wukong-gateway cli::tests
cargo test -p wukong-cli
```

---

## Task 6: Add `wukong-schedulerd` Binary

**Files:**
- Create: `crates/wukong-schedulerd/Cargo.toml`
- Create: `crates/wukong-schedulerd/src/main.rs`
- Modify: root `Cargo.toml`

- [ ] **Step 1: Create daemon crate**

Dependencies:
- `clap`
- `tokio`
- `uuid`
- `wukong-gateway`
- `wukong-memory`
- `wukong-runtime`
- `wukong-scheduler`

- [ ] **Step 2: Define daemon CLI**

Options:
- `--db <url>` override memory DB URL
- `--agent-cmd <cmd>` override agent command
- `--scope <scope>` default base scope for runtime config
- `--tick-secs <n>` default `60`
- `--lease-secs <n>` default `300`
- `--limit <n>` max jobs per tick, default `10`
- `--once` run one scan and exit, useful for tests/systemd timers/manual smoke tests

- [ ] **Step 3: Implement runtime wiring**

Daemon startup must:
- resolve `GatewayConfig` from CLI/env defaults
- open `Memory`
- apply embed/markdown configuration the same way CLI does if applicable
- create `AgentCliBackend`
- open `SchedulerStore`
- create stable `worker_id`, e.g. hostname/process/uuid

- [ ] **Step 4: Implement tick loop**

Every tick:
- get current unix timestamp
- `claim_due_jobs(now, worker_id, lease_secs, limit)`
- for each claimed job:
  - `start_run`
  - `execute_job`
  - `finish_run`
  - `complete_job`
- log failures to stderr, but keep daemon alive

Signal handling:
- handle Ctrl-C / SIGTERM and exit gracefully between jobs.
- V1 does not need to cancel an already-running opencode process mid-turn.

- [ ] **Step 5: Tests**

Unit-test loop internals where possible. Add at least one integration-style test using `--once` and a temp SQLite DB if practical.

- [ ] **Step 6: Verify**

Run:

```bash
cargo test -p wukong-schedulerd
cargo check -p wukong-schedulerd
```

---

## Task 7: Docker and Release Packaging

**Files:**
- Modify: `Dockerfile`
- Modify: `docker-compose.yml`
- Modify: `.github/workflows/release.yml`
- Modify: `scripts/install.sh` if bundle validation lists binaries explicitly

- [ ] **Step 1: Include `wukong-schedulerd` in release artifacts**

Update release workflow packaging loops to include:

```text
wukong-schedulerd
```

- [ ] **Step 2: Include `wukong-schedulerd` in Docker image**

Update Dockerfile downloader and COPY steps to include the new binary.

- [ ] **Step 3: Add optional compose service**

Add service:

```yaml
wukong-schedulerd:
  profiles: ["scheduler"]
  build:
    context: .
    dockerfile: Dockerfile
  image: wukong:latest
  container_name: wukong-schedulerd
  environment:
    - USER_ID=${USER_ID:-1000}
    - GROUP_ID=${GROUP_ID:-1000}
    - WUKONG_AGENT_CMD=${WUKONG_AGENT_CMD:-opencode run}
    - WUKONG_WORKSPACE=/workspace
    - WUKONG_MEMORY_DB=sqlite:///data/memory.db
    - WUKONG_THINKING=${WUKONG_THINKING:-1}
  volumes:
    - ${WUKONG_HOST_WORKSPACE:-./workspace}:/workspace
    - opencode-config:/home/wukong/.config/opencode
    - wukong-data:/data
  command: ["wukong-schedulerd"]
  restart: unless-stopped
```

V1 keeps schedulerd behind the `scheduler` profile so existing Docker installs do not start background AI jobs unless the user opts in.

- [ ] **Step 4: Update entrypoint**

Allow direct dispatch:

```bash
wukong-schedulerd)
    exec gosu wukong "$@"
    ;;
```

- [ ] **Step 5: Verify**

Run:

```bash
cargo check --workspace
docker compose config
```

If Docker build is practical locally, also run:

```bash
docker compose build wukong-schedulerd
```

---

## Task 8: Documentation

**Files:**
- Modify: `README.md`
- Optionally create: `crates/wukong-scheduler/README.md`
- Optionally create: `crates/wukong-schedulerd/README.md`

- [ ] **Step 1: Document CLI usage**

Add examples:

```bash
wukong schedule add-turn \
  --name "daily project check" \
  --cron "0 9 * * 1-5" \
  --scope project:Wukong \
  --prompt "Review recent memories and suggest today's highest-impact task."

wukong schedule add-maintenance \
  --name "nightly consolidate" \
  --cron "0 2 * * *" \
  --scope project:Wukong \
  --task consolidate

wukong schedule list
wukong-schedulerd
```

- [ ] **Step 2: Document Docker usage**

If schedulerd uses a profile, document:

```bash
docker compose --profile scheduler up -d
```

Also document that scheduled turn jobs require configured OpenCode provider/auth inside the shared `opencode-config` volume.

- [ ] **Step 3: Document operational semantics**

Include:
- cron format is 5-field UTC in V1 to avoid timezone ambiguity.
- daemon must be running for cron jobs to execute.
- `trigger` can run a job immediately without daemon.
- multiple daemon instances use DB leases to avoid duplicate execution.

- [ ] **Step 4: Verify docs commands**

Run parse checks where possible:

```bash
cargo run -p wukong-cli -- schedule --help
cargo run -p wukong-cli -- schedule add-turn --help
cargo run -p wukong-schedulerd -- --help
```

---

## Task 9: Final Verification

- [ ] **Step 1: Full test suite**

Run:

```bash
cargo test
```

- [ ] **Step 2: Workspace check**

Run:

```bash
cargo check --workspace
```

- [ ] **Step 3: Manual smoke: schedule lifecycle**

With a temp DB or local dev DB:

```bash
wukong schedule add-maintenance --name smoke-snapshot --cron "* * * * *" --task snapshot
wukong schedule list
wukong schedule trigger --id <id>
wukong schedule runs --id <id>
wukong schedule disable --id <id>
wukong schedule rm --id <id>
```

- [ ] **Step 4: Manual smoke: daemon once**

Create a due maintenance job, then run:

```bash
wukong-schedulerd --once
wukong schedule runs --limit 5
```

Expected: one run recorded, job next run updated, no duplicate run on immediate second `--once`.

- [ ] **Step 5: GitNexus change detection before commit**

Run:

```text
gitnexus_detect_changes(scope="all", repo="Wukong")
```

Review changed symbols and affected flows before committing.

---

## Implementation Notes

- Keep V1 boring and reliable. Do not add Web UI, Telegram notification, or event trigger hooks during this implementation.
- Prefer UTC timestamps and unix seconds for all scheduler DB fields.
- Do not store role/skill in V1 jobs. The existing runtime planner remains responsible for role/skill selection.
- Avoid long transactions around job execution. Claim in DB, release the transaction, execute, then record completion.
- If the daemon crashes mid-job, the lease expires and a later daemon can retry. V1 accepts at-least-once execution semantics.
- If exactly-once behavior becomes necessary later, add per-job idempotency keys or job-specific dedupe semantics in V2.

---

## Future V2 Ideas

- Web Console schedule management.
- Telegram notification channel for job success/failure.
- Event-triggered jobs from memory events or external webhooks.
- Fixed role/skill execution once runtime exposes an explicit planned-turn API.
- Timezone support per job.
- Retry policy and max failure count.
- Pause/unpause all jobs.
