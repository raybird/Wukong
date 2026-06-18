mod notify;

use clap::Parser;
use std::io::Write;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::interval;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::config::{default_scope, GatewayConfig};
use wukong_gateway::workspace_dir;
use wukong_chat_history::ChatHistoryStore;
use wukong_memory::Memory;
use wukong_scheduler::{execute_job, ExecutionContext, RunStatus, SchedulerStore};
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
    let memory = open_memory(&cfg).await?;
    let backend = AgentCliBackend { command: cfg.agent_command.clone(), workspace: workspace_dir() };
    let store = SchedulerStore::open(&cfg.db_url).await.map_err(|e| e.to_string())?;
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
        run_scan(&store, &memory, &backend, &cfg, &worker_id, cli.lease_secs, cli.limit, notifier.as_ref(), history.as_ref()).await?;
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
    backend: &AgentCliBackend,
    cfg: &GatewayConfig,
    worker_id: &str,
    lease_secs: i64,
    limit: i64,
    notifier: Option<&ReqwestTgClient>,
    history: Option<&ChatHistoryStore>,
) -> Result<(), String> {
    let now = now_unix();
    let jobs = store.claim_due_jobs(now, worker_id, lease_secs, limit).await.map_err(|e| e.to_string())?;
    for job in jobs {
        let started_at = now_unix();
        let run_id = store.start_run(&job.id, started_at).await.map_err(|e| e.to_string())?;
        let ctx = ExecutionContext { memory, backend, base_config: cfg };
        let output = execute_job(&ctx, &job).await;
        let finished_at = now_unix();
        let status = if output.success { RunStatus::Success } else { RunStatus::Failure };
        store
            .finish_run(run_id, status, &output.message, finished_at)
            .await
            .map_err(|e| e.to_string())?;
        if !store.complete_claimed_job(&job, worker_id, finished_at).await.map_err(|e| e.to_string())? {
            eprintln!("warning: job {} lease was taken by another worker before completion", job.id);
            continue;
        }
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
    let token = wukong_settings::effective_telegram_settings(&file).token.trim().to_string();
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

async fn open_memory(cfg: &GatewayConfig) -> Result<Memory, String> {
    let memory = Memory::open(&cfg.db_url).await.map_err(|e| e.to_string())?;
    #[cfg(feature = "embed")]
    let memory = if std::env::var("WUKONG_EMBED").as_deref() == Ok("1") {
        match wukong_memory::FastembedBackend::new() {
            Ok(backend) => memory.with_embedder(std::sync::Arc::new(backend)),
            Err(e) => {
                eprintln!("🐵 語意層停用（模型載入失敗）：{e}");
                memory
            }
        }
    } else {
        memory
    };
    let memory = match std::env::var("WUKONG_MD_DIR") {
        Ok(dir) if !dir.is_empty() => memory.with_markdown(dir),
        _ => memory,
    };
    Ok(memory)
}

fn resolve_config(cli: &Cli) -> GatewayConfig {
    GatewayConfig {
        scope: cli.scope.clone().unwrap_or_else(default_scope),
        db_url: cli
            .db
            .clone()
            .or_else(|| std::env::var("WUKONG_MEMORY_DB").ok())
            .unwrap_or_else(default_db_url),
        agent_command: cli
            .agent_cmd
            .clone()
            .or_else(|| std::env::var("WUKONG_AGENT_CMD").ok())
            .map(|s| split_ws(&s))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]),
        default_model: None,
        thinking: std::env::var("WUKONG_THINKING").as_deref() != Ok("0"),
        recall_top_k: 5,
        stream: false,
    }
}

fn split_ws(s: &str) -> Vec<String> {
    s.split_whitespace().map(|t| t.to_string()).collect()
}

fn default_db_url() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = format!("{home}/.wukong");
    let _ = std::fs::create_dir_all(&dir);
    format!("sqlite://{dir}/memory.db")
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
        assert_eq!(cfg.agent_command, vec!["agent".to_string(), "go".to_string()]);
        assert_eq!(cfg.scope, "project:X");
    }
}
