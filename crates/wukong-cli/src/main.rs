use clap::Parser;
use wukong_cli::run_turn;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::cli::Cli;
use wukong_gateway::config::GatewayConfig;
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

    match run_turn(&memory, &backend, &cfg, &cli.prompt_text()).await {
        Ok(out) => {
            eprintln!("🐵 悟空·{}", out.role.name());
            println!("{}", out.text);
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
