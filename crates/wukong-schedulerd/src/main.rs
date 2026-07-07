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
use wukong_scheduler::{ClaimedJobOutcome, ExecutionContext, SchedulerStore};
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
        return Ok(());
    }

    let mut ticks = interval(Duration::from_secs(cli.tick_secs));
    loop {
        tokio::select! {
            _ = ticks.tick() => {
                if let Err(e) = run_scan(&store, &memory, &backend, &cfg, &worker_id, cli.lease_secs, cli.limit, notifier.as_ref(), history.as_ref()).await {
                    eprintln!("warning: scheduler scan failed: {e}");
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
    let jobs = store
        .claim_due_jobs(now, worker_id, lease_secs, limit)
        .await
        .map_err(|e| e.to_string())?;
    for job in jobs {
        let ctx = ExecutionContext {
            memory,
            backend,
            base_config: cfg,
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
