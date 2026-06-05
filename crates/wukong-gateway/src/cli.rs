use clap::Parser;

/// Wukong assistant gateway (one-shot CLI).
#[derive(Parser, Debug)]
#[command(name = "wukong", about = "Wukong assistant gateway (CLI)")]
pub struct Cli {
    /// The prompt to send to the assistant (joined with spaces).
    #[arg(required = true, num_args = 1..)]
    pub prompt: Vec<String>,

    /// Continue the previous agent session (passes the continue flag through).
    #[arg(short = 'c', long = "continue")]
    pub continue_session: bool,

    /// Override the memory scope (e.g. "project:Foo", "global").
    #[arg(long)]
    pub scope: Option<String>,

    /// Override the memory database URL.
    #[arg(long)]
    pub db: Option<String>,

    /// Override the agent command (whitespace-separated, e.g. "opencode run").
    #[arg(long = "agent-cmd")]
    pub agent_cmd: Option<String>,
}

impl Cli {
    /// Join the positional prompt words back into a single string.
    pub fn prompt_text(&self) -> String {
        self.prompt.join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_prompt_and_flags() {
        let cli = Cli::try_parse_from([
            "wukong", "-c", "--scope", "global", "hello", "world",
        ])
        .unwrap();
        assert_eq!(cli.prompt_text(), "hello world");
        assert!(cli.continue_session);
        assert_eq!(cli.scope.as_deref(), Some("global"));
    }

    #[test]
    fn prompt_is_required() {
        let result = Cli::try_parse_from(["wukong"]);
        assert!(result.is_err());
    }

    #[test]
    fn agent_cmd_override_parses() {
        let cli = Cli::try_parse_from(["wukong", "--agent-cmd", "opencode run", "hi"]).unwrap();
        assert_eq!(cli.agent_cmd.as_deref(), Some("opencode run"));
        assert_eq!(cli.prompt_text(), "hi");
    }
}
