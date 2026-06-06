use thiserror::Error;

/// Errors from the Telegram transport layer.
#[derive(Debug, Error)]
pub enum TgError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("telegram api error: {0}")]
    Api(String),
}
