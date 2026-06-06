use clap::Parser;
use std::io::{BufRead, Write};
use wukong_cli::repl::{classify_line, LineAction};
use wukong_cli::run_turn;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::cli::Cli;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::StreamEvent;
use wukong_memory::Memory;

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

    let backend = AgentCliBackend {
        command: cfg.agent_command.clone(),
        continue_args: cfg.continue_args.clone(),
    };

    let prompt = cli.prompt_text();

    if prompt.is_empty() {
        // No prompt => interactive REPL over real stdin.
        eprintln!("🐵 悟空 REPL。輸入 /exit 或 Ctrl-D 離開。");
        let stdin = std::io::stdin();
        let mut cfg_repl = cfg.clone();
        cfg_repl.continue_session = false;
        let mut first = true;
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
                    cfg_repl.continue_session = !first;
                    first = false;
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
