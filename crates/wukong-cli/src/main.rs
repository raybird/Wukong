use clap::Parser;
use std::io::{BufRead, Write};
use wukong_cli::repl::{classify_line, LineAction};
use wukong_cli::run_turn;
use wukong_gateway::backend::{build_backend_from_env, AgentBackend};
use wukong_gateway::cli::{Cli, Command, MemoryOp, ScheduleMaintenanceTaskArg, ScheduleOp};
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::workspace_dir;
use wukong_gateway::StreamEvent;
use wukong_memory::Memory;
use wukong_runtime::maintenance::{memory_consolidate, memory_prune, memory_snapshot};
use wukong_runtime::util::now_unix;
use wukong_scheduler::{ExecutionContext, Job, JobKind, MaintenanceTask, NewJob, SchedulerStore};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let mut cfg = GatewayConfig::resolve(&cli);
    let settings_path = wukong_settings::default_settings_path();
    let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
    apply_settings_to_config(&mut cfg, &settings);

    let memory = match wukong_runtime::bootstrap::open_memory_from_env(&cfg.db_url).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: failed to open memory: {e}");
            std::process::exit(1);
        }
    };

    let backend = build_backend_from_env(cfg.agent_command.clone(), workspace_dir());

    if cli.new_session {
        if let Err(e) = memory.clear_agent_session(&cfg.scope).await {
            eprintln!("warning: failed to reset session: {e}");
        }
    }

    if let Some(Command::Memory { op }) = &cli.command {
        if let Err(e) = run_memory_op(&memory, &backend, &cfg, op).await {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Some(Command::Schedule { op }) = &cli.command {
        if let Err(e) = run_schedule_op(&memory, &backend, &cfg, op).await {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
        return;
    }

    let prompt = cli.prompt_text();

    if prompt.is_empty() {
        // No prompt => interactive REPL over real stdin.
        eprintln!("🐵 悟空 REPL。輸入 /exit 或 Ctrl-D 離開。");
        let stdin = std::io::stdin();
        let mut cfg_repl = cfg.clone();
        loop {
            eprint!("悟空 › ");
            let _ = std::io::stderr().flush();
            let mut line = String::new();
            if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
                eprintln!();
                break; // EOF (Ctrl-D)
            }
            match classify_line(&line) {
                LineAction::Exit => break,
                LineAction::Skip => continue,
                LineAction::SetScope(s) => {
                    cfg_repl.scope = s;
                }
                LineAction::Command(cmd) => {
                    let settings_path = wukong_settings::default_settings_path();
                    match wukong_cli::run_session_command(
                        &memory,
                        &backend,
                        &cfg_repl,
                        &settings_path,
                        cmd,
                    )
                    .await
                    {
                        Ok(reply) => println!("{reply}"),
                        Err(e) => eprintln!("error: {e}"),
                    }
                }
                LineAction::Turn(input) => {
                    // Reload persisted settings each turn so a meta-command like
                    // /set_models applies to the very next question (scope from
                    // /scope is preserved via the cfg_repl clone).
                    let mut cfg_turn = cfg_repl.clone();
                    let settings =
                        wukong_settings::load_settings(&wukong_settings::default_settings_path())
                            .unwrap_or_default();
                    apply_settings_to_config(&mut cfg_turn, &settings);
                    if let Err(e) = run_one(&memory, &backend, &cfg_turn, &input).await {
                        eprintln!("error: {e}");
                    }
                }
            }
        }
        return;
    }

    // Single shot.
    if let Err(e) = run_one(&memory, &backend, &cfg, &prompt).await {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

async fn run_schedule_op(
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    op: &ScheduleOp,
) -> Result<(), wukong_cli::WukongError> {
    let store = SchedulerStore::open(&cfg.db_url)
        .await
        .map_err(to_wukong_error)?;
    match op {
        ScheduleOp::List => {
            for job in store.list_jobs().await.map_err(to_wukong_error)? {
                println!(
                    "{}\tenabled={}\tcron={}\tnext={}\tname={}\tkind={}",
                    job.id,
                    job.enabled,
                    job.cron,
                    format_opt_ts(job.next_run_at),
                    job.name,
                    describe_job_kind(&job.kind),
                );
            }
        }
        ScheduleOp::AddTurn {
            name,
            cron,
            scope,
            prompt,
        } => {
            let job = store
                .add_job(NewJob {
                    name: name.clone(),
                    kind: JobKind::Turn {
                        scope: scope.clone(),
                        prompt: prompt.clone(),
                    },
                    cron: cron.clone(),
                })
                .await
                .map_err(to_wukong_error)?;
            println!(
                "已建立排程: {} next={}",
                job.id,
                format_opt_ts(job.next_run_at)
            );
        }
        ScheduleOp::AddMaintenance {
            name,
            cron,
            scope,
            task,
        } => {
            let job = store
                .add_job(NewJob {
                    name: name.clone(),
                    kind: JobKind::Maintenance {
                        scope: scope.clone(),
                        task: map_maintenance_task(*task),
                    },
                    cron: cron.clone(),
                })
                .await
                .map_err(to_wukong_error)?;
            println!(
                "已建立排程: {} next={}",
                job.id,
                format_opt_ts(job.next_run_at)
            );
        }
        ScheduleOp::Rm { id } => {
            let removed = store.remove_job(id).await.map_err(to_wukong_error)?;
            println!(
                "{}",
                if removed {
                    "已刪除排程"
                } else {
                    "找不到排程"
                }
            );
        }
        ScheduleOp::Enable { id } => {
            let changed = store.set_enabled(id, true).await.map_err(to_wukong_error)?;
            println!(
                "{}",
                if changed {
                    "已啟用排程"
                } else {
                    "找不到排程"
                }
            );
        }
        ScheduleOp::Disable { id } => {
            let changed = store
                .set_enabled(id, false)
                .await
                .map_err(to_wukong_error)?;
            println!(
                "{}",
                if changed {
                    "已停用排程"
                } else {
                    "找不到排程"
                }
            );
        }
        ScheduleOp::Trigger { id } => {
            let worker_id = format!("manual-{}", std::process::id());
            let Some(job) = store
                .claim_job(id, now_unix(), &worker_id, 300)
                .await
                .map_err(to_wukong_error)?
            else {
                println!("找不到排程");
                return Ok(());
            };
            trigger_job(&store, memory, backend, cfg, &job, &worker_id).await?;
        }
        ScheduleOp::Runs { id, limit } => {
            for run in store
                .recent_runs(id.as_deref(), *limit)
                .await
                .map_err(to_wukong_error)?
            {
                println!(
                    "{}\tjob={}\tstatus={}\tstarted={}\tfinished={}\t{}",
                    run.id,
                    run.job_id,
                    run.status.as_str(),
                    run.started_at,
                    format_opt_ts(run.finished_at),
                    run.message.replace('\n', " "),
                );
            }
        }
    }
    Ok(())
}

async fn trigger_job(
    store: &SchedulerStore,
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    job: &Job,
    worker_id: &str,
) -> Result<(), wukong_cli::WukongError> {
    let ctx = ExecutionContext {
        memory,
        backend,
        base_config: cfg,
    };
    match wukong_scheduler::run_claimed_job(store, &ctx, job, worker_id)
        .await
        .map_err(to_wukong_error)?
    {
        wukong_scheduler::ClaimedJobOutcome::Completed(output) => {
            println!("{}", output.message);
            if output.success {
                Ok(())
            } else {
                Err(to_wukong_error_string(output.message))
            }
        }
        wukong_scheduler::ClaimedJobOutcome::LeaseLost(_) => Err(to_wukong_error_string(
            "排程 lease 已被其他 worker 接手".to_string(),
        )),
    }
}

fn map_maintenance_task(task: ScheduleMaintenanceTaskArg) -> MaintenanceTask {
    match task {
        ScheduleMaintenanceTaskArg::Snapshot => MaintenanceTask::Snapshot,
        ScheduleMaintenanceTaskArg::Consolidate => MaintenanceTask::Consolidate,
        ScheduleMaintenanceTaskArg::Prune => MaintenanceTask::Prune,
    }
}

fn describe_job_kind(kind: &JobKind) -> String {
    match kind {
        JobKind::Turn { scope, .. } => format!("turn(scope={scope})"),
        JobKind::Maintenance { scope, task } => {
            format!(
                "maintenance(task={task:?},scope={})",
                scope.as_deref().unwrap_or("<all>")
            )
        }
    }
}

fn format_opt_ts(ts: Option<i64>) -> String {
    ts.map(|v| v.to_string()).unwrap_or_else(|| "-".to_string())
}

fn to_wukong_error(err: wukong_scheduler::SchedulerError) -> wukong_cli::WukongError {
    to_wukong_error_string(err.to_string())
}

fn to_wukong_error_string(message: String) -> wukong_cli::WukongError {
    wukong_cli::WukongError::from(wukong_memory::MemoryError::Other(message))
}

/// Run one turn, rendering per `cfg.stream`. The role header prints to stderr
/// right after routing (before streamed text); answer text goes to stdout.
async fn run_one(
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    input: &str,
) -> Result<(), wukong_cli::WukongError> {
    if cfg.stream {
        let mut out = std::io::stdout();
        let mut err = std::io::stderr();
        let mut renderer = wukong_cli::render::StreamRenderer::new(&mut out, &mut err);
        let mut sink = |ev: StreamEvent| renderer.on_event(&ev);
        run_turn(memory, backend, cfg, input, &mut sink, &mut |role| {
            eprintln!("🐵 悟空·{}", role.name());
        })
        .await?;
        println!(); // newline after streamed text
        Ok(())
    } else {
        let res = run_turn(memory, backend, cfg, input, &mut |_| {}, &mut |role| {
            eprintln!("🐵 悟空·{}", role.name());
        })
        .await?;
        println!("{}", res.text);
        Ok(())
    }
}

fn apply_settings_to_config(cfg: &mut GatewayConfig, settings: &wukong_settings::Settings) {
    let agent_settings = wukong_settings::effective_agent_settings(settings);
    cfg.apply_default_model(agent_settings.default_model.as_deref());
    let planner_preferences = wukong_settings::effective_planner_preferences(settings);
    cfg.apply_planner_preferences(
        planner_preferences.enabled,
        planner_preferences.roles,
        planner_preferences.skills,
    );
}

/// Dispatch a `wukong memory <op>` maintenance command.
async fn run_memory_op(
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    op: &MemoryOp,
) -> Result<(), wukong_cli::WukongError> {
    match op {
        MemoryOp::Snapshot { scope } => {
            println!("{}", memory_snapshot(memory, scope.as_deref()).await?);
        }
        MemoryOp::Consolidate { scope, dry_run } => {
            let scope = scope.clone().unwrap_or_else(|| cfg.scope.clone());
            println!(
                "{}",
                memory_consolidate(memory, backend, &scope, *dry_run).await?
            );
        }
        MemoryOp::Prune { scope, dry_run } => {
            println!(
                "{}",
                memory_prune(memory, scope.as_deref(), *dry_run).await?
            );
        }
        MemoryOp::Export { dir } => {
            let dir = dir
                .clone()
                .or_else(|| std::env::var("WUKONG_MD_DIR").ok())
                .ok_or_else(|| {
                    wukong_cli::WukongError::from(wukong_memory::MemoryError::Other(
                        "未指定輸出目錄,請用 --dir 或設 WUKONG_MD_DIR".to_string(),
                    ))
                })?;
            memory.export(&dir).await?;
            println!("已匯出 markdown 至 {dir}");
        }
    }
    Ok(())
}
