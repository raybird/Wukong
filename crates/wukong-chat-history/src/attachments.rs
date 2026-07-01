use serde::Serialize;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatAttachment {
    pub id: i64,
    pub message_id: i64,
    pub scope: String,
    pub source: String,
    pub original_name: String,
    pub stored_name: String,
    pub relative_path: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub telegram_file_id: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChatAttachment {
    pub message_id: i64,
    pub scope: String,
    pub source: String,
    pub original_name: String,
    pub stored_name: String,
    pub relative_path: String,
    pub mime_type: Option<String>,
    pub size_bytes: i64,
    pub sha256: Option<String>,
    pub telegram_file_id: Option<String>,
    pub created_at: i64,
}

pub fn sanitize_scope(scope: &str) -> String {
    let out: String = scope
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if out.trim_matches('_').is_empty() {
        "scope".to_string()
    } else {
        out
    }
}

pub fn sanitize_filename(name: &str) -> String {
    let file_name = Path::new(name)
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("attachment")
        .trim();
    let cleaned: String = file_name
        .chars()
        .map(|c| {
            if c == '/' || c == '\\' || c.is_control() {
                '_'
            } else {
                c
            }
        })
        .collect();
    let cleaned = cleaned.trim_matches('.').trim();
    if cleaned.is_empty() {
        "attachment".to_string()
    } else {
        cleaned.to_string()
    }
}

pub fn relative_attachment_path(scope: &str, message_id: i64, filename: &str) -> String {
    format!(
        "{}/{}/{}",
        sanitize_scope(scope),
        message_id,
        sanitize_filename(filename)
    )
}

pub fn resolve_under_upload_root(root: &Path, relative_path: &str) -> Option<PathBuf> {
    let rel = Path::new(relative_path);
    if rel.is_absolute() {
        return None;
    }
    if rel.components().any(|c| {
        matches!(
            c,
            Component::ParentDir | Component::RootDir | Component::Prefix(_)
        )
    }) {
        return None;
    }
    Some(root.join(rel))
}
