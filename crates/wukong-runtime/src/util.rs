//! Small env/time helpers shared by the CLI, Web, Telegram, and scheduler
//! surfaces. These were previously copy-pasted into each entrypoint; hosting a
//! single copy here removes the "fix it in N places" hazard. Crates that sit
//! *below* `wukong-runtime` in the dependency graph (`wukong-memory`,
//! `wukong-chat-history`) cannot depend back on it and keep their own local
//! copies by necessity.

use std::path::PathBuf;

/// Current UNIX time in whole seconds, saturating to 0 before the epoch.
pub fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// Root directory for chat attachment uploads: `<workspace>/.wukong/uploads`,
/// where `<workspace>` is `WUKONG_WORKSPACE` or the current directory.
pub fn upload_root() -> PathBuf {
    std::env::var("WUKONG_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join(".wukong")
        .join("uploads")
}

/// Default memory database URL: `sqlite://$HOME/.wukong/memory.db`, creating the
/// directory best-effort. Falls back to `.` when `HOME` is unset.
pub fn default_db_url() -> String {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
    let dir = format!("{home}/.wukong");
    let _ = std::fs::create_dir_all(&dir);
    format!("sqlite://{dir}/memory.db")
}

/// Resolve the memory database URL from `WUKONG_MEMORY_DB`, defaulting to
/// [`default_db_url`].
pub fn db_url_from_env() -> String {
    std::env::var("WUKONG_MEMORY_DB").unwrap_or_else(|_| default_db_url())
}

/// Resolve the underlying agent CLI command from `WUKONG_AGENT_CMD`
/// (whitespace-separated), defaulting to `["opencode", "run"]`.
pub fn agent_command_from_env() -> Vec<String> {
    std::env::var("WUKONG_AGENT_CMD")
        .ok()
        .map(|s| {
            s.split_whitespace()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_unix_is_after_2020() {
        // 2020-01-01T00:00:00Z. A sanity floor that never trips on a sane clock.
        assert!(now_unix() > 1_577_836_800);
    }

    #[test]
    fn upload_root_ends_with_wukong_uploads() {
        let p = upload_root();
        assert!(p.ends_with("uploads"));
        assert!(p.to_string_lossy().contains(".wukong"));
    }

    #[test]
    fn default_db_url_is_sqlite_memory_db() {
        let url = default_db_url();
        assert!(url.starts_with("sqlite://"));
        assert!(url.ends_with("/memory.db"));
    }

    #[test]
    fn agent_command_from_env_never_empty() {
        // Regardless of ambient env, the resolved command is non-empty.
        assert!(!agent_command_from_env().is_empty());
    }
}
