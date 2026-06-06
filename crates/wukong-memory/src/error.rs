use thiserror::Error;

/// All errors produced by the memory core.
#[derive(Debug, Error)]
pub enum MemoryError {
    #[error("database error: {0}")]
    Db(#[from] sqlx::Error),
    #[error("not found")]
    NotFound,
    #[error("invalid scope: {0}")]
    InvalidScope(String),
    #[error("invalid query: {0}")]
    InvalidQuery(String),
    #[error("serialization error: {0}")]
    Serialize(#[from] serde_json::Error),
    #[error("embedding error: {0}")]
    Embed(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Convenience result alias used across the crate.
pub type Result<T> = std::result::Result<T, MemoryError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_scope_message_includes_input() {
        let err = MemoryError::InvalidScope("bogus".to_string());
        assert_eq!(err.to_string(), "invalid scope: bogus");
    }

    #[test]
    fn embed_error_message_includes_detail() {
        let err = MemoryError::Embed("model load failed".to_string());
        assert_eq!(err.to_string(), "embedding error: model load failed");
    }
}
