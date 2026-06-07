use clap::{Parser, Subcommand};

/// Wukong assistant gateway (one-shot CLI).
#[derive(Parser, Debug)]
#[command(
    name = "wukong",
    about = "Wukong assistant gateway (CLI)",
    args_conflicts_with_subcommands = true,
    subcommand_negates_reqs = true
)]
pub struct Cli {
    /// Memory maintenance subcommands. Absent => chat / REPL.
    #[command(subcommand)]
    pub command: Option<Command>,

    /// The prompt to send to the assistant (joined with spaces). Empty => REPL.
    #[arg(num_args = 0..)]
    pub prompt: Vec<String>,

    /// Override the memory scope (e.g. "project:Foo", "global").
    #[arg(long)]
    pub scope: Option<String>,

    /// Override the memory database URL.
    #[arg(long)]
    pub db: Option<String>,

    /// Override the agent command (whitespace-separated, e.g. "opencode run").
    #[arg(long = "agent-cmd")]
    pub agent_cmd: Option<String>,

    /// Disable activity rendering (spinner + tool events); use plain capture.
    #[arg(long = "no-stream")]
    pub no_stream: bool,

    /// Disable opencode reasoning/thinking output.
    #[arg(long = "no-thinking")]
    pub no_thinking: bool,

    /// Start a fresh opencode session for this scope before the turn.
    #[arg(long = "new")]
    pub new_session: bool,
}

impl Cli {
    /// Join the positional prompt words back into a single string.
    pub fn prompt_text(&self) -> String {
        self.prompt.join(" ")
    }
}

/// Top-level subcommands.
#[derive(Subcommand, Debug)]
pub enum Command {
    /// Memory maintenance operations.
    Memory {
        #[command(subcommand)]
        op: MemoryOp,
    },
}

/// `wukong memory <op>`.
#[derive(Subcommand, Debug)]
pub enum MemoryOp {
    /// Print a health snapshot.
    Snapshot {
        #[arg(long)]
        scope: Option<String>,
    },
    /// Fold scattered events into summaries.
    Consolidate {
        #[arg(long)]
        scope: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Delete consolidated / low-value memories.
    Prune {
        #[arg(long)]
        scope: Option<String>,
        #[arg(long = "dry-run")]
        dry_run: bool,
    },
    /// Rebuild markdown mirror from the DB.
    Export {
        #[arg(long)]
        dir: Option<String>,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prompt_and_flags() {
        let cli = Cli::try_parse_from([
            "wukong", "--scope", "global", "hello", "world",
        ])
        .unwrap();
        assert_eq!(cli.prompt_text(), "hello world");
        assert_eq!(cli.scope.as_deref(), Some("global"));
    }

    #[test]
    fn no_thinking_and_new_flags_parse() {
        let cli = Cli::try_parse_from(["wukong", "--no-thinking", "--new", "hi"]).unwrap();
        assert!(cli.no_thinking);
        assert!(cli.new_session);
        assert_eq!(cli.prompt_text(), "hi");
    }

    #[test]
    fn no_prompt_is_allowed_for_repl() {
        let cli = Cli::try_parse_from(["wukong"]).unwrap();
        assert!(cli.prompt_text().is_empty());
    }

    #[test]
    fn no_stream_flag_parses() {
        let cli = Cli::try_parse_from(["wukong", "--no-stream", "hi"]).unwrap();
        assert!(cli.no_stream);
        assert_eq!(cli.prompt_text(), "hi");
    }

    #[test]
    fn parses_memory_snapshot_subcommand() {
        let cli = Cli::try_parse_from(["wukong", "memory", "snapshot"]).unwrap();
        match cli.command {
            Some(Command::Memory { op: MemoryOp::Snapshot { scope } }) => assert!(scope.is_none()),
            _ => panic!("expected memory snapshot"),
        }
    }

    #[test]
    fn parses_memory_prune_dry_run() {
        let cli = Cli::try_parse_from(["wukong", "memory", "prune", "--dry-run"]).unwrap();
        match cli.command {
            Some(Command::Memory { op: MemoryOp::Prune { dry_run, .. } }) => assert!(dry_run),
            _ => panic!("expected memory prune"),
        }
    }

    #[test]
    fn bare_prompt_has_no_subcommand() {
        let cli = Cli::try_parse_from(["wukong", "hello", "world"]).unwrap();
        assert!(cli.command.is_none());
        assert_eq!(cli.prompt_text(), "hello world");
    }

    #[test]
    fn agent_cmd_override_parses() {
        let cli = Cli::try_parse_from(["wukong", "--agent-cmd", "opencode run", "hi"]).unwrap();
        assert_eq!(cli.agent_cmd.as_deref(), Some("opencode run"));
        assert_eq!(cli.prompt_text(), "hi");
    }
}
