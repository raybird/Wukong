//! wukong-memoryd: axum HTTP transport over wukong-memory.

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use std::sync::Arc;
use wukong_memory::{Memory, MemoryError, RecallQuery, RememberInput};

/// Server configuration sourced from the environment.
pub struct Config {
    pub db_url: String,
    pub port: u16,
}

impl Config {
    /// Build config from env vars, with sensible defaults:
    /// WUKONG_MEMORY_DB (default $HOME/.wukong/memory.db) and
    /// WUKONG_MEMORY_PORT (default 3917).
    pub fn from_env() -> Config {
        let db_url = match std::env::var("WUKONG_MEMORY_DB") {
            Ok(v) => v,
            Err(_) => {
                let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
                let dir = format!("{home}/.wukong");
                let _ = std::fs::create_dir_all(&dir);
                format!("sqlite://{dir}/memory.db")
            }
        };
        let port = std::env::var("WUKONG_MEMORY_PORT")
            .ok()
            .and_then(|p| p.parse().ok())
            .unwrap_or(3917);
        Config { db_url, port }
    }
}

/// Newtype so we can implement axum's IntoResponse for the library error
/// (orphan rule: MemoryError and IntoResponse are both foreign here).
pub struct AppError(MemoryError);

impl From<MemoryError> for AppError {
    fn from(e: MemoryError) -> Self {
        AppError(e)
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let status = match &self.0 {
            MemoryError::InvalidScope(_) | MemoryError::InvalidQuery(_) => StatusCode::BAD_REQUEST,
            MemoryError::NotFound => StatusCode::NOT_FOUND,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        let body = Json(serde_json::json!({ "error": self.0.to_string() }));
        (status, body).into_response()
    }
}

async fn health() -> impl IntoResponse {
    Json(serde_json::json!({ "status": "ok" }))
}

async fn stats(State(mem): State<Arc<Memory>>) -> Result<impl IntoResponse, AppError> {
    Ok(Json(mem.stats().await?))
}

async fn snapshot(State(mem): State<Arc<Memory>>) -> Result<impl IntoResponse, AppError> {
    Ok(Json(mem.snapshot(None).await?))
}

async fn remember(
    State(mem): State<Arc<Memory>>,
    Json(input): Json<RememberInput>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(mem.remember(input).await?))
}

async fn recall(
    State(mem): State<Arc<Memory>>,
    Json(query): Json<RecallQuery>,
) -> Result<impl IntoResponse, AppError> {
    Ok(Json(mem.recall(query).await?))
}

/// Build the axum router over a shared Memory instance.
pub fn build_router(mem: Arc<Memory>) -> Router {
    Router::new()
        .route("/v1/health", get(health))
        .route("/v1/stats", get(stats))
        .route("/v1/snapshot", get(snapshot))
        .route("/v1/remember", post(remember))
        .route("/v1/recall", post(recall))
        .with_state(mem)
}
