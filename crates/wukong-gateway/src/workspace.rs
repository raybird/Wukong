use std::path::PathBuf;

/// Resolve the agent workspace directory.
///
/// Reads `WUKONG_WORKSPACE` env var; falls back to `~/.wukong/workspace`.
/// The directory is created if it doesn't exist.
pub fn workspace_dir() -> Option<PathBuf> {
    let dir = std::env::var("WUKONG_WORKSPACE")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
            PathBuf::from(format!("{home}/.wukong/workspace"))
        });
    let _ = std::fs::create_dir_all(&dir);
    Some(dir)
}
