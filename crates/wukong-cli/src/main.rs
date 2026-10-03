use clap::Parser;
use std::io::{BufRead, Write};
use wukong_cli::repl::{classify_line, LineAction};
use wukong_cli::run_turn;
use wukong_gateway::backend::{build_backend_from_env, AgentBackend, AiBackend};
use wukong_gateway::cli::{
    Cli, Command, MemoryOp, OpencodeOp, ScheduleMaintenanceTaskArg, ScheduleOp,
};
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::workspace_dir;
use wukong_gateway::StreamEvent;
use wukong_memory::Memory;
use wukong_runtime::maintenance::{memory_consolidate, memory_prune, memory_snapshot};
use wukong_runtime::session_retention::{prune_opencode_sessions, render_report, RetentionPolicy};
use wukong_runtime::util::now_unix;
use wukong_scheduler::{
    ExecutionContext, Job, JobKind, MaintenanceTask, NewJob, PermissionPolicy, SchedulerStore,
};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    if let Some(Command::Opencode {
        op: OpencodeOp::Vacuum,
    }) = &cli.command
    {
        // 在開記憶庫之前處理：這個子命令由 opencode-server 容器在啟動 server 前
        // 呼叫，那個容器不該順手建立或開啟 memory.db。
        match wukong_cli::opencode_db::run().await {
            Ok(line) => eprintln!("{line}"),
            Err(line) => {
                eprintln!("{line}");
                std::process::exit(1);
            }
        }
        return;
    }

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
        // 先刪 opencode 那邊的 session，否則它會永遠留在 opencode.db。刪不掉也照樣
        // 清對應：--new 的承諾是這一回合不帶舊 context，留下的無主 session 之後由
        // 保留期清理收掉。
        match memory.agent_session(&cfg.scope).await {
            Ok(Some(session_id)) => {
                if let Err(e) = backend.delete_session(&session_id).await {
                    eprintln!("warning: failed to delete session {session_id}: {e}");
                }
            }
            Ok(None) => {}
            Err(e) => eprintln!("warning: failed to read session: {e}"),
        }
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

    if let Some(Command::Opencode {
        op: OpencodeOp::Prune { dry_run },
    }) = &cli.command
    {
        let policy = RetentionPolicy::from_env();
        // 清理的對錯取決於這份記憶庫是不是 server 實際在用的那一份，所以把它印出來。
        println!("記憶庫：{}", cfg.db_url);
        match prune_opencode_sessions(&memory, &backend, policy, now_unix() * 1000, *dry_run).await
        {
            Ok(report) => {
                println!("{}", render_report(&report, policy));
                if policy.enabled() && (!report.anchored || !report.failed.is_empty()) {
                    std::process::exit(1);
                }
            }
            Err(e) => {
                eprintln!("error: {e}");
                std::process::exit(1);
            }
        }
        return;
    }

    let prompt = cli.prompt_text();
    let (stdin_tx, mut stdin_lines) = tokio::sync::mpsc::unbounded_channel();
    // 2026-10-03：REPL 與 question 共用一個 reader，避免互相預讀答案；
    // 使用獨立 thread，回合逾時不會等待 Tokio 的 blocking stdin 工作。
    std::thread::spawn(move || {
        for line in std::io::stdin().lock().lines() {
            match line {
                Ok(line) => {
                    if stdin_tx.send(line).is_err() {
                        break;
                    }
                }
                _ => break,
            }
        }
    });

    if prompt.is_empty() {
        // No prompt => interactive REPL over real stdin.
        eprintln!("🐵 悟空 REPL。輸入 /exit 或 Ctrl-D 離開。");
        let mut cfg_repl = cfg.clone();
        loop {
            eprint!("悟空 › ");
            let _ = std::io::stderr().flush();
            let Some(line) = stdin_lines.recv().await else {
                eprintln!();
                break; // EOF (Ctrl-D)
            };
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
                    if let Err(e) =
                        run_one(&memory, &backend, &cfg_turn, &input, &mut stdin_lines).await
                    {
                        eprintln!("error: {e}");
                    }
                }
            }
        }
        return;
    }

    // Single shot.
    if let Err(e) = run_one(&memory, &backend, &cfg, &prompt, &mut stdin_lines).await {
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
        permission_policy: PermissionPolicy::from_env(),
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
    stdin_lines: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
) -> Result<(), wukong_cli::WukongError> {
    let mut out = std::io::stdout();
    let mut err = std::io::stderr();
    let mut renderer = wukong_cli::render::StreamRenderer::new(&mut out, &mut err);
    let mut has_text = false;
    let (questions_tx, mut questions_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut sink = |ev: StreamEvent| {
        if let StreamEvent::QuestionRequest(question) = &ev {
            let _ = questions_tx.send(question.clone());
            renderer.on_event(&ev);
            return;
        }
        if let StreamEvent::Text(text) = &ev {
            has_text |= !text.trim().is_empty();
        }
        if cfg.stream {
            renderer.on_event(&ev);
        }
    };
    let turn = async {
        run_turn(memory, backend, cfg, input, &mut sink, &mut |role| {
            eprintln!("🐵 悟空·{}", role.name());
        })
        .await
    };
    let replies = async {
        while let Some(question) = questions_rx.recv().await {
            answer_terminal_question(backend, &question, stdin_lines).await?;
        }
        Ok::<(), wukong_cli::WukongError>(())
    };
    let res = tokio::select! {
        result = turn => result?,
        result = replies => { result?; return Err(to_wukong_error_string("問答通道提前結束".into())); }
    };
    if cfg.stream && has_text {
        println!(); // newline after streamed text
    } else {
        println!("{}", res.text);
    }
    Ok(())
}

async fn answer_terminal_question(
    backend: &AgentBackend,
    request: &wukong_gateway::stream::QuestionRequest,
    stdin_lines: &mut tokio::sync::mpsc::UnboundedReceiver<String>,
) -> Result<(), wukong_cli::WukongError> {
    let mut answers = Vec::new();
    for question in &request.questions {
        for (index, option) in question.options.iter().enumerate() {
            eprintln!("  {}. {} — {}", index + 1, option.label, option.description);
        }
        loop {
            eprint!(
                "回答{}{}（/cancel 取消） › ",
                if question.multiple {
                    "，多選以逗號分隔"
                } else {
                    ""
                },
                if question.custom {
                    "，可輸入自訂文字"
                } else {
                    ""
                }
            );
            let _ = std::io::stderr().flush();
            let line = stdin_lines.recv().await;
            let Some(line) = line.filter(|line| line.trim() != "/cancel") else {
                backend
                    .cancel_question(&request.session_id, &request.request_id)
                    .await?;
                return Ok(());
            };
            let values = if question.multiple {
                line.trim().split(',').map(str::trim).collect::<Vec<_>>()
            } else {
                vec![line.trim()]
            };
            let mut selected = Vec::new();
            let mut valid = true;
            for value in values {
                let option = value
                    .parse::<usize>()
                    .ok()
                    .and_then(|i| i.checked_sub(1))
                    .and_then(|i| question.options.get(i))
                    .or_else(|| question.options.iter().find(|option| option.label == value));
                if let Some(option) = option {
                    selected.push(option.label.clone());
                } else if question.custom && !value.is_empty() {
                    selected.push(value.into());
                } else {
                    valid = false;
                    break;
                }
            }
            if valid {
                answers.push(selected);
                break;
            }
            eprintln!(
                "請輸入有效選項{}。",
                if question.custom {
                    "或自訂文字"
                } else {
                    ""
                }
            );
        }
    }
    backend
        .answer_question(&request.session_id, &request.request_id, answers)
        .await?;
    Ok(())
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
