use clap::Parser;
use wukong_gateway::backend::AgentCliBackend;
use wukong_orchestrator::orchestrate_chain;

/// Demo entrypoint: auto-route a task to a role and run it.
#[derive(Parser, Debug)]
#[command(name = "wukong-orchestrate", about = "Route a task to a Wukong role and run it")]
struct Cli {
    /// The task to orchestrate (joined with spaces).
    #[arg(required = true, num_args = 1..)]
    task: Vec<String>,

    /// Override the agent command (whitespace-separated, e.g. "opencode run").
    #[arg(long = "agent-cmd")]
    agent_cmd: Option<String>,
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();

    let command = cli
        .agent_cmd
        .or_else(|| std::env::var("WUKONG_AGENT_CMD").ok())
        .map(|s| s.split_whitespace().map(|t| t.to_string()).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]);

    let backend = AgentCliBackend {
        command,
    };

    match orchestrate_chain(&backend, &cli.task.join(" ")).await {
        Ok(chain) => {
            for step in &chain.steps {
                eprintln!("[role: {}]", step.role.name());
            }
            println!("{}", chain.final_output());
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
