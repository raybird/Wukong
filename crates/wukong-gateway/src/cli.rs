use clap::{Parser, Subcommand, ValueEnum};

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
    /// Scheduled job management.
    Schedule {
        #[command(subcommand)]
        op: ScheduleOp,
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

/// `wukong schedule <op>`.
#[derive(Subcommand, Debug)]
pub enum ScheduleOp {
    /// List all scheduled jobs.
    List,
    /// Add a scheduled Wukong turn.
    AddTurn {
        #[arg(long)]
        name: String,
        #[arg(long)]
        cron: String,
        #[arg(long)]
        scope: String,
        #[arg(long)]
        prompt: String,
    },
    /// Add a scheduled memory maintenance job.
    AddMaintenance {
        #[arg(long)]
        name: String,
        #[arg(long)]
        cron: String,
        #[arg(long)]
        scope: Option<String>,
        #[arg(long)]
        task: ScheduleMaintenanceTaskArg,
    },
    /// Remove a scheduled job.
    Rm {
        #[arg(long)]
        id: String,
    },
    /// Enable a scheduled job.
    Enable {
        #[arg(long)]
        id: String,
    },
    /// Disable a scheduled job.
    Disable {
        #[arg(long)]
        id: String,
    },
    /// Trigger a scheduled job immediately.
    Trigger {
        #[arg(long)]
        id: String,
    },
    /// Show recent scheduled job runs.
    Runs {
        #[arg(long)]
        id: Option<String>,
        #[arg(long, default_value_t = 20)]
        limit: i64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum ScheduleMaintenanceTaskArg {
    Snapshot,
    Consolidate,
    Prune,
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

    #[test]
    fn parses_schedule_add_turn() {
        let cli = Cli::try_parse_from([
            "wukong",
            "schedule",
            "add-turn",
            "--name",
            "daily",
            "--cron",
            "0 9 * * *",
            "--scope",
            "project:X",
            "--prompt",
            "review",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Schedule { op: ScheduleOp::AddTurn { name, cron, scope, prompt } }) => {
                assert_eq!(name, "daily");
                assert_eq!(cron, "0 9 * * *");
                assert_eq!(scope, "project:X");
                assert_eq!(prompt, "review");
            }
            _ => panic!("expected schedule add-turn"),
        }
    }

    #[test]
    fn parses_schedule_add_maintenance() {
        let cli = Cli::try_parse_from([
            "wukong",
            "schedule",
            "add-maintenance",
            "--name",
            "nightly",
            "--cron",
            "0 2 * * *",
            "--task",
            "consolidate",
        ])
        .unwrap();
        match cli.command {
            Some(Command::Schedule { op: ScheduleOp::AddMaintenance { task, scope, .. } }) => {
                assert_eq!(task, ScheduleMaintenanceTaskArg::Consolidate);
                assert_eq!(scope, None);
            }
            _ => panic!("expected schedule add-maintenance"),
        }
    }

    #[test]
    fn rejects_invalid_schedule_task() {
        assert!(Cli::try_parse_from([
            "wukong",
            "schedule",
            "add-maintenance",
            "--name",
            "bad",
            "--cron",
            "* * * * *",
            "--task",
            "unknown",
        ])
        .is_err());
    }

    #[test]
    fn schedule_command_does_not_conflict_with_prompt_args() {
        let cli = Cli::try_parse_from(["wukong", "schedule", "list"]).unwrap();
        assert!(matches!(cli.command, Some(Command::Schedule { op: ScheduleOp::List })));
        assert!(cli.prompt_text().is_empty());
    }
}
