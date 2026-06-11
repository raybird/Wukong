use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct Settings {
    pub telegram: TelegramSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct TelegramSettings {
    pub token: String,
    pub allowed: String,
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
    let allowed = std::env::var("WUKONG_TG_ALLOWED").unwrap_or_else(|_| file.telegram.allowed.clone());
    TelegramSettings { token, allowed }
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
        };

        save_settings(&path, &settings).unwrap();
        let loaded = load_settings(&path).unwrap();

        assert_eq!(loaded, settings);
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
