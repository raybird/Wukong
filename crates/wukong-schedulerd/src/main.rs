mod auto_maintenance;
mod notify;

use clap::Parser;
use std::io::Write;
use std::time::Duration;
use tokio::time::interval;
use wukong_chat_history::ChatHistoryStore;
use wukong_gateway::backend::{build_backend_from_env, AgentBackend};
use wukong_gateway::config::{default_scope, GatewayConfig};
use wukong_gateway::workspace_dir;
use wukong_memory::Memory;
use wukong_runtime::util::now_unix;
use wukong_scheduler::{
    ClaimedJobOutcome, ExecutionContext, Job, PermissionPolicy, SchedulerStore,
};
use wukong_tg_client::client::ReqwestTgClient;

#[derive(Debug, Parser)]
#[command(name = "wukong-schedulerd", about = "Wukong scheduler daemon")]
struct Cli {
    /// Override the memory database URL.
    #[arg(long)]
    db: Option<String>,
    /// Override the agent command (whitespace-separated, e.g. "opencode run").
    #[arg(long = "agent-cmd")]
    agent_cmd: Option<String>,
    /// Default base scope for runtime config.
    #[arg(long)]
    scope: Option<String>,
    /// Seconds between scheduler scans.
    #[arg(long, default_value_t = 60)]
    tick_secs: u64,
    /// Seconds before a claimed job lease expires.
    #[arg(long, default_value_t = 300)]
    lease_secs: i64,
    /// Maximum jobs to claim per scan.
    #[arg(long, default_value_t = 10)]
    limit: i64,
    /// Run one scan and exit.
    #[arg(long)]
    once: bool,
    /// Seconds between automatic memory maintenance passes.
    #[arg(long)]
    maintenance_interval_secs: Option<u64>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Err(e) = run(cli).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run(cli: Cli) -> Result<(), String> {
    let mut cfg = resolve_config(&cli);
    let settings_path = wukong_settings::default_settings_path();
    let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
    let agent_settings = wukong_settings::effective_agent_settings(&settings);
    cfg.apply_default_model(agent_settings.default_model.as_deref());
    let memory = wukong_runtime::bootstrap::open_memory_from_env(&cfg.db_url).await?;
    let backend = build_backend_from_env(cfg.agent_command.clone(), workspace_dir());
    let store = SchedulerStore::open(&cfg.db_url)
        .await
        .map_err(|e| e.to_string())?;
    let interrupted = recover_interrupted_runs(&store, cli.once, now_unix()).await?;
    if interrupted > 0 {
        eprintln!("recovered {interrupted} interrupted scheduler run(s)");
    }
    let history = match ChatHistoryStore::open(&cfg.db_url).await {
        Ok(store) => Some(store),
        Err(e) => {
            eprintln!("warning: chat history disabled for scheduler: {e}");
            None
        }
    };
    let worker_id = format!("schedulerd-{}-{}", std::process::id(), uuid::Uuid::new_v4());
    let notifier = build_notifier();

    if cli.once {
        run_scan(
            &store,
            &memory,
            &backend,
            &cfg,
            &worker_id,
            cli.lease_secs,
            cli.limit,
            notifier.as_ref(),
            history.as_ref(),
        )
        .await?;
        run_memory_maintenance(&memory, &backend).await?;
        return Ok(());
    }

    let mut ticks = interval(Duration::from_secs(cli.tick_secs));
    let maintenance_config = auto_maintenance::AutoMaintenanceConfig::from_env();
    let mut maintenance_ticks = interval(Duration::from_secs(
        cli.maintenance_interval_secs
            .unwrap_or(maintenance_config.interval_secs)
            .max(1),
    ));
    loop {
        tokio::select! {
            _ = ticks.tick() => {
                if let Err(e) = run_scan(&store, &memory, &backend, &cfg, &worker_id, cli.lease_secs, cli.limit, notifier.as_ref(), history.as_ref()).await {
                    eprintln!("warning: scheduler scan failed: {e}");
                }
            }
            _ = maintenance_ticks.tick() => {
                if let Err(e) = run_memory_maintenance(&memory, &backend).await {
                    eprintln!("warning: memory maintenance failed: {e}");
                }
            }
            _ = shutdown_signal() => {
                eprintln!("wukong-schedulerd stopping");
                break;
            }
        }
    }
    Ok(())
}

async fn run_memory_maintenance(memory: &Memory, backend: &AgentBackend) -> Result<(), String> {
    let policy = auto_maintenance::AutoMaintenanceConfig::from_env();
    let report = auto_maintenance::run_once(memory, backend, policy).await?;
    if report.summaries_created > 0 || report.memories_pruned > 0 {
        eprintln!(
            "memory maintenance completed scopes={} summaries={} pruned={}",
            report.scopes_checked, report.summaries_created, report.memories_pruned
        );
    }
    Ok(())
}

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

#[allow(clippy::too_many_arguments)]
async fn run_scan(
    store: &SchedulerStore,
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    worker_id: &str,
    lease_secs: i64,
    limit: i64,
    notifier: Option<&ReqwestTgClient>,
    history: Option<&ChatHistoryStore>,
) -> Result<(), String> {
    let now = now_unix();
    let jobs = claim_ready_jobs(store, backend, now, worker_id, lease_secs, limit).await?;
    for job in jobs {
        let ctx = ExecutionContext {
            memory,
            backend,
            base_config: cfg,
            permission_policy: PermissionPolicy::from_env(),
        };
        let output = match wukong_scheduler::run_claimed_job(store, &ctx, &job, worker_id)
            .await
            .map_err(|e| e.to_string())?
        {
            ClaimedJobOutcome::Completed(output) => output,
            ClaimedJobOutcome::LeaseLost(_) => {
                eprintln!(
                    "warning: job {} lease was taken by another worker before completion",
                    job.id
                );
                continue;
            }
        };
        if output.success {
            eprintln!("job {} succeeded", job.id);
        } else {
            eprintln!("job {} failed: {}", job.id, output.message);
        }
        // Best-effort delivery to the originating Telegram chat; a push failure
        // must not fail the job (which already ran and is recorded).
        if let Some(client) = notifier {
            match notify::notify_turn_result_with_history(client, history, &job, &output).await {
                Ok(true) => eprintln!("job {} result delivered to telegram", job.id),
                Ok(false) => {}
                Err(e) => eprintln!("warning: telegram delivery for job {} failed: {e}", job.id),
            }
        }
        let _ = std::io::stderr().flush();
    }
    Ok(())
}

/// Build the optional Telegram notifier. Disabled when `WUKONG_SCHED_NOTIFY=0`
/// or when no Telegram token is configured (the daemon still runs all jobs).
fn build_notifier() -> Option<ReqwestTgClient> {
    if std::env::var("WUKONG_SCHED_NOTIFY").as_deref() == Ok("0") {
        eprintln!("🐵 scheduler 通知停用：WUKONG_SCHED_NOTIFY=0");
        return None;
    }
    let path = wukong_settings::default_settings_path();
    let file = wukong_settings::load_settings(&path).unwrap_or_default();
    let token = wukong_settings::effective_telegram_settings(&file)
        .token
        .trim()
        .to_string();
    if token.is_empty() {
        eprintln!("🐵 scheduler 通知停用：未設定 Telegram token（WUKONG_TG_TOKEN 或 settings）");
        None
    } else {
        Some(ReqwestTgClient::new(&token))
    }
}

async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("install SIGTERM handler");
        tokio::select! {
            _ = tokio::signal::ctrl_c() => {}
            _ = sigterm.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn resolve_config(cli: &Cli) -> GatewayConfig {
    GatewayConfig {
        scope: cli.scope.clone().unwrap_or_else(default_scope),
        db_url: cli
            .db
            .clone()
            .or_else(|| std::env::var("WUKONG_MEMORY_DB").ok())
            .unwrap_or_else(wukong_runtime::util::default_db_url),
        agent_command: cli
            .agent_cmd
            .clone()
            .or_else(|| std::env::var("WUKONG_AGENT_CMD").ok())
            .map(|s| split_ws(&s))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]),
        default_model: None,
        planner_preferences: None,
        thinking: std::env::var("WUKONG_THINKING").as_deref() != Ok("0"),
        recall_top_k: 5,
        stream: false,
    }
}

fn split_ws(s: &str) -> Vec<String> {
    s.split_whitespace().map(|t| t.to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    use wukong_scheduler::{Job, JobKind, NewJob, RunStatus};

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

        let err = claim_ready_jobs(
            &store,
            &backend,
            job.next_run_at.unwrap(),
            "worker",
            300,
            10,
        )
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
        assert!(claim_ready_jobs(
            &store,
            &unavailable,
            job.next_run_at.unwrap(),
            "worker",
            300,
            10
        )
        .await
        .is_err());

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
        assert_eq!(
            recover_interrupted_runs(&store, false, 20).await.unwrap(),
            1
        );
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

    #[test]
    fn parses_once_flag_and_tuning() {
        let cli = Cli::try_parse_from([
            "wukong-schedulerd",
            "--once",
            "--tick-secs",
            "5",
            "--lease-secs",
            "30",
            "--limit",
            "2",
        ])
        .unwrap();
        assert!(cli.once);
        assert_eq!(cli.tick_secs, 5);
        assert_eq!(cli.lease_secs, 30);
        assert_eq!(cli.limit, 2);
    }

    #[test]
    fn parses_maintenance_interval_override() {
        let cli = Cli::try_parse_from(["wukong-schedulerd", "--maintenance-interval-secs", "30"])
            .unwrap();
        assert_eq!(cli.maintenance_interval_secs, Some(30));
    }

    #[test]
    fn resolve_config_uses_cli_overrides() {
        let cli = Cli::try_parse_from([
            "wukong-schedulerd",
            "--db",
            "sqlite://x.db",
            "--agent-cmd",
            "agent go",
            "--scope",
            "project:X",
        ])
        .unwrap();
        let cfg = resolve_config(&cli);
        assert_eq!(cfg.db_url, "sqlite://x.db");
        assert_eq!(
            cfg.agent_command,
            vec!["agent".to_string(), "go".to_string()]
        );
        assert_eq!(cfg.scope, "project:X");
    }
}
