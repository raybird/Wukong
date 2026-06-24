use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub telegram: TelegramSettings,
    #[serde(default)]
    pub agent: AgentSettings,
    #[serde(default)]
    pub planner_preferences: PlannerPreferences,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct PlannerPreferences {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub roles: Vec<String>,
    #[serde(default)]
    pub skills: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct TelegramSettings {
    pub token: String,
    pub allowed: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct AgentSettings {
    pub default_model: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings io: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SettingsError>;

pub fn default_settings_path() -> PathBuf {
    std::env::var("WUKONG_SETTINGS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data/settings.json"))
}

pub fn load_settings(path: &Path) -> Result<Settings> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(serde_json::from_str(&raw)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_settings(path: &Path, settings: &Settings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, raw)?;
    Ok(())
}

pub fn effective_telegram_settings(file: &Settings) -> TelegramSettings {
    let token = std::env::var("WUKONG_TG_TOKEN").unwrap_or_else(|_| file.telegram.token.clone());
    let allowed =
        std::env::var("WUKONG_TG_ALLOWED").unwrap_or_else(|_| file.telegram.allowed.clone());
    TelegramSettings { token, allowed }
}

pub fn effective_agent_settings(file: &Settings) -> AgentSettings {
    AgentSettings {
        default_model: file
            .agent
            .default_model
            .as_ref()
            .map(|m| m.trim())
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string()),
    }
}

pub fn effective_planner_preferences(file: &Settings) -> PlannerPreferences {
    PlannerPreferences {
        enabled: file.planner_preferences.enabled,
        roles: normalize_preference_ids(&file.planner_preferences.roles),
        skills: normalize_preference_ids(&file.planner_preferences.skills),
    }
}

fn normalize_preference_ids(values: &[String]) -> Vec<String> {
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

pub fn redact_token(token: &str) -> String {
    if token.is_empty() {
        String::new()
    } else if token.len() <= 8 {
        "********".to_string()
    } else {
        format!("{}...{}", &token[..4], &token[token.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_default_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let settings = load_settings(&path).unwrap();

        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn saves_and_loads_telegram_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        let settings = Settings {
            telegram: TelegramSettings {
                token: "123:abc".to_string(),
                allowed: "42 99".to_string(),
            },
            agent: AgentSettings::default(),
            planner_preferences: PlannerPreferences::default(),
        };

        save_settings(&path, &settings).unwrap();
        let loaded = load_settings(&path).unwrap();

        assert_eq!(loaded, settings);
    }

    #[test]
    fn saves_and_loads_agent_default_model() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings {
            telegram: TelegramSettings::default(),
            agent: AgentSettings {
                default_model: Some("opencode/deepseek-v4-flash-free".to_string()),
            },
            planner_preferences: PlannerPreferences::default(),
        };

        save_settings(&path, &settings).unwrap();
        let loaded = load_settings(&path).unwrap();

        assert_eq!(
            loaded.agent.default_model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
    }

    #[test]
    fn missing_planner_preferences_defaults_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"telegram":{"token":"123:abc","allowed":"42"}}"#).unwrap();

        let loaded = load_settings(&path).unwrap();

        assert_eq!(loaded.planner_preferences, PlannerPreferences::default());
    }

    #[test]
    fn saves_and_loads_planner_preferences() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        let settings = Settings {
            telegram: TelegramSettings::default(),
            agent: AgentSettings::default(),
            planner_preferences: PlannerPreferences {
                enabled: true,
                roles: vec!["fixer".to_string(), "oracle".to_string()],
                skills: vec!["systematic-debugging".to_string()],
            },
        };

        save_settings(&path, &settings).unwrap();
        let loaded = load_settings(&path).unwrap();

        assert_eq!(loaded.planner_preferences, settings.planner_preferences);
    }

    #[test]
    fn effective_planner_preferences_trims_dedupes_and_drops_empty_values() {
        let settings = Settings {
            telegram: TelegramSettings::default(),
            agent: AgentSettings::default(),
            planner_preferences: PlannerPreferences {
                enabled: true,
                roles: vec![
                    " fixer ".to_string(),
                    "".to_string(),
                    "oracle".to_string(),
                    "fixer".to_string(),
                ],
                skills: vec![
                    " systematic-debugging ".to_string(),
                    "systematic-debugging".to_string(),
                    " ".to_string(),
                    "verification-before-completion".to_string(),
                ],
            },
        };

        let effective = effective_planner_preferences(&settings);

        assert!(effective.enabled);
        assert_eq!(effective.roles, vec!["fixer", "oracle"]);
        assert_eq!(
            effective.skills,
            vec!["systematic-debugging", "verification-before-completion"]
        );
    }

    #[test]
    fn missing_agent_settings_defaults_cleanly() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");
        std::fs::write(&path, r#"{"telegram":{"token":"123:abc","allowed":"42"}}"#).unwrap();

        let loaded = load_settings(&path).unwrap();

        assert_eq!(loaded.telegram.token, "123:abc");
        assert_eq!(loaded.agent.default_model, None);
    }

    #[test]
    fn effective_agent_settings_uses_file_model() {
        let settings = Settings {
            telegram: TelegramSettings::default(),
            agent: AgentSettings {
                default_model: Some("opencode/deepseek-v4-flash-free".to_string()),
            },
            planner_preferences: PlannerPreferences::default(),
        };

        let effective = effective_agent_settings(&settings);

        assert_eq!(
            effective.default_model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
    }

    #[test]
    fn env_overrides_file_settings() {
        std::env::set_var("WUKONG_TG_TOKEN", "env-token");
        std::env::set_var("WUKONG_TG_ALLOWED", "7");
        let file = Settings {
            telegram: TelegramSettings {
                token: "file-token".to_string(),
                allowed: "42".to_string(),
            },
            agent: AgentSettings::default(),
            planner_preferences: PlannerPreferences::default(),
        };

        let effective = effective_telegram_settings(&file);

        std::env::remove_var("WUKONG_TG_TOKEN");
        std::env::remove_var("WUKONG_TG_ALLOWED");
        assert_eq!(effective.token, "env-token");
        assert_eq!(effective.allowed, "7");
    }

    #[test]
    fn redacts_saved_token_for_api_responses() {
        assert_eq!(redact_token(""), "");
        assert_eq!(redact_token("short"), "********");
        assert_eq!(redact_token("1234567890"), "1234...7890");
    }
}
