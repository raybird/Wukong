use clap::Parser;
use std::io::{BufRead, Write};
use wukong_cli::repl::{classify_line, LineAction};
use wukong_cli::run_turn;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::cli::{Cli, Command, MemoryOp};
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::summarize::OpencodeSummarizer;
use wukong_gateway::StreamEvent;
use wukong_memory::{ConsolidatePolicy, Memory, PrunePolicy};

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    let cfg = GatewayConfig::resolve(&cli);

    let memory = match Memory::open(&cfg.db_url).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: failed to open memory: {e}");
            std::process::exit(1);
        }
    };

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

    let backend = AgentCliBackend {
        command: cfg.agent_command.clone(),
    };

    if let Some(Command::Memory { op }) = &cli.command {
        if let Err(e) = run_memory_op(&memory, &backend, &cfg, op).await {
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
                LineAction::Turn(input) => {
                    if let Err(e) = run_one(&memory, &backend, &cfg_repl, &input).await {
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

/// Run one turn, rendering per `cfg.stream`. The role header prints to stderr
/// right after routing (before streamed text); answer text goes to stdout.
async fn run_one(
    memory: &Memory,
    backend: &AgentCliBackend,
    cfg: &GatewayConfig,
    input: &str,
) -> Result<(), wukong_cli::WukongError> {
    if cfg.stream {
        let mut sink = |ev: StreamEvent| match ev {
            StreamEvent::Text(t) => {
                print!("{t}");
                let _ = std::io::stdout().flush();
            }
            StreamEvent::ToolUse(n) => {
                eprintln!("  ▸ 使用工具 {n}");
            }
            StreamEvent::Reasoning(t) => {
                eprintln!("  💭 {t}");
            }
            _ => {}
        };
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

/// Dispatch a `wukong memory <op>` maintenance command.
async fn run_memory_op(
    memory: &Memory,
    backend: &AgentCliBackend,
    cfg: &GatewayConfig,
    op: &MemoryOp,
) -> Result<(), wukong_cli::WukongError> {
    match op {
        MemoryOp::Snapshot { scope } => {
            let snap = memory.snapshot(scope.as_deref()).await?;
            println!("總計: {}", snap.total);
            println!("依範圍:");
            for s in &snap.by_scope {
                println!("  {} = {}", s.scope, s.count);
            }
            println!("依類型:");
            for k in &snap.by_kind {
                println!("  {} = {}", k.kind.as_str(), k.count);
            }
            println!(
                "年齡: <1d={} <7d={} <30d={} older={}",
                snap.age.last_day, snap.age.last_week, snap.age.last_month, snap.age.older
            );
            println!("embedding 覆蓋: {}/{}", snap.embedding.embedded, snap.embedding.total);
            println!("consolidation 候選: {}", snap.consolidation_candidates);
            println!("prune 候選: {}", snap.prune_candidates);
        }
        MemoryOp::Consolidate { scope, dry_run } => {
            let scope = scope.clone().unwrap_or_else(|| cfg.scope.clone());
            let policy = ConsolidatePolicy::default();
            if *dry_run {
                let plan = memory.plan_consolidation(&scope, &policy).await?;
                println!("[dry-run] 將產生 {} 筆摘要:", plan.batches.len());
                for (i, b) in plan.batches.iter().enumerate() {
                    println!("  批 {}: {} 筆來源 {:?}", i + 1, b.len(), b);
                }
            } else {
                let summarizer = OpencodeSummarizer::new(backend);
                let ids = memory.consolidate(&scope, &policy, &summarizer).await?;
                println!("已建立 {} 筆摘要: {:?}", ids.len(), ids);
            }
        }
        MemoryOp::Prune { scope, dry_run } => {
            let policy = PrunePolicy::default();
            if *dry_run {
                let ids = memory.plan_prune(scope.as_deref(), &policy).await?;
                println!("[dry-run] 將刪除 {} 筆: {:?}", ids.len(), ids);
            } else {
                let n = memory.prune(scope.as_deref(), &policy).await?;
                println!("已刪除 {n} 筆");
            }
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
