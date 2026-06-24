use crate::cli::Cli;

/// Fully resolved runtime configuration (CLI > env > defaults).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannerPreferenceConfig {
    pub preferred_roles: Vec<String>,
    pub preferred_skills: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct GatewayConfig {
    pub scope: String,
    pub db_url: String,
    pub agent_command: Vec<String>,
    pub default_model: Option<String>,
    pub planner_preferences: Option<PlannerPreferenceConfig>,
    /// Pass `--thinking` to opencode for conversational turns. Default true;
    /// off via `--no-thinking` or `WUKONG_THINKING=0`.
    pub thinking: bool,
    pub recall_top_k: usize,
    /// Activity rendering (spinner + tool events). Default true; off via
    /// `--no-stream` or `WUKONG_STREAM=0`.
    pub stream: bool,
}

impl GatewayConfig {
    /// Resolve config from parsed CLI args, falling back to env then defaults.
    pub fn resolve(cli: &Cli) -> GatewayConfig {
        let scope = cli.scope.clone().unwrap_or_else(default_scope);

        let db_url = cli
            .db
            .clone()
            .or_else(|| std::env::var("WUKONG_MEMORY_DB").ok())
            .unwrap_or_else(default_db_url);

        let agent_command = cli
            .agent_cmd
            .clone()
            .or_else(|| std::env::var("WUKONG_AGENT_CMD").ok())
            .map(|s| split_ws(&s))
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]);

        let stream = !cli.no_stream && std::env::var("WUKONG_STREAM").as_deref() != Ok("0");
        let thinking = !cli.no_thinking && std::env::var("WUKONG_THINKING").as_deref() != Ok("0");

        GatewayConfig {
            scope,
            db_url,
            agent_command,
            default_model: None,
            planner_preferences: None,
            thinking,
            recall_top_k: 5,
            stream,
        }
    }

    pub fn apply_default_model(&mut self, model: Option<&str>) {
        self.default_model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string());
    }

    pub fn apply_planner_preferences(
        &mut self,
        enabled: bool,
        roles: Vec<String>,
        skills: Vec<String>,
    ) {
        if !enabled {
            self.planner_preferences = None;
            return;
        }
        let preferred_roles = normalize_preference_ids(roles);
        let preferred_skills = normalize_preference_ids(skills);
        if preferred_roles.is_empty() && preferred_skills.is_empty() {
            self.planner_preferences = None;
            return;
        }
        self.planner_preferences = Some(PlannerPreferenceConfig {
            preferred_roles,
            preferred_skills,
        });
    }
}

fn normalize_preference_ids(values: Vec<String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for value in values {
        let value = value.trim();
        if value.is_empty() || normalized.iter().any(|existing| existing == value) {
            continue;
        }
        normalized.push(value.to_string());
    }
    normalized
}

fn split_ws(s: &str) -> Vec<String> {
    s.split_whitespace().map(|t| t.to_string()).collect()
}

/// Derive the default scope from the current directory name.
pub fn default_scope() -> String {
    std::env::current_dir()
        .ok()
        .and_then(|p| p.file_name().map(|n| n.to_string_lossy().to_string()))
        .filter(|n| !n.is_empty())
        .map(|n| format!("project:{n}"))
        .unwrap_or_else(|| "global".to_string())
}

fn default_db_url() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = format!("{home}/.wukong");
    let _ = std::fs::create_dir_all(&dir);
    format!("sqlite://{dir}/memory.db")
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn cli_overrides_take_priority() {
        // All overridable fields set on the CLI => env is never consulted.
        let cli = Cli::try_parse_from([
            "wukong",
            "--scope",
            "project:Explicit",
            "--db",
            "sqlite://x.db",
            "--agent-cmd",
            "my-agent go",
            "hi",
        ])
        .unwrap();
        let cfg = GatewayConfig::resolve(&cli);
        assert_eq!(cfg.scope, "project:Explicit");
        assert_eq!(cfg.db_url, "sqlite://x.db");
        assert_eq!(
            cfg.agent_command,
            vec!["my-agent".to_string(), "go".to_string()]
        );
        assert_eq!(cfg.recall_top_k, 5);
        assert!(cfg.thinking); // default on when --no-thinking absent
        assert!(cfg.stream); // default on when --no-stream absent
    }

    #[test]
    fn no_stream_flag_disables_stream() {
        let cli = Cli::try_parse_from(["wukong", "--no-stream", "hi"]).unwrap();
        let cfg = GatewayConfig::resolve(&cli);
        assert!(!cfg.stream);
    }

    #[test]
    fn apply_default_model_sets_config_model() {
        let mut cfg = GatewayConfig {
            scope: "global".to_string(),
            db_url: "sqlite://x.db".to_string(),
            agent_command: vec!["opencode".to_string(), "run".to_string()],
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        };

        cfg.apply_default_model(Some("opencode/deepseek-v4-flash-free"));

        assert_eq!(
            cfg.default_model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
    }

    #[test]
    fn apply_planner_preferences_sets_none_when_disabled_or_empty() {
        let mut cfg = GatewayConfig {
            scope: "global".to_string(),
            db_url: "sqlite://x.db".to_string(),
            agent_command: vec!["opencode".to_string(), "run".to_string()],
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        };

        cfg.apply_planner_preferences(
            false,
            vec!["fixer".to_string()],
            vec!["systematic-debugging".to_string()],
        );
        assert!(cfg.planner_preferences.is_none());

        cfg.apply_planner_preferences(true, vec![" ".to_string()], vec![]);
        assert!(cfg.planner_preferences.is_none());
    }

    #[test]
    fn apply_planner_preferences_sets_normalized_values() {
        let mut cfg = GatewayConfig {
            scope: "global".to_string(),
            db_url: "sqlite://x.db".to_string(),
            agent_command: vec!["opencode".to_string(), "run".to_string()],
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        };

        cfg.apply_planner_preferences(
            true,
            vec![
                " fixer ".to_string(),
                "fixer".to_string(),
                "oracle".to_string(),
            ],
            vec![" systematic-debugging ".to_string()],
        );

        let prefs = cfg.planner_preferences.unwrap();
        assert_eq!(prefs.preferred_roles, vec!["fixer", "oracle"]);
        assert_eq!(prefs.preferred_skills, vec!["systematic-debugging"]);
    }

    #[test]
    fn default_scope_is_project_prefixed() {
        // cargo runs tests with CWD at the crate root, which has a name.
        assert!(default_scope().starts_with("project:"));
    }

    #[test]
    fn split_ws_splits_on_whitespace() {
        assert_eq!(
            split_ws("opencode  run"),
            vec!["opencode".to_string(), "run".to_string()]
        );
        assert!(split_ws("   ").is_empty());
    }
}
