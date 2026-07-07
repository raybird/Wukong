//! wukong-memoryd: axum HTTP transport over wukong-memory.

use axum::extract::rejection::JsonRejection;
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
    pub host: String,
    pub token: Option<String>,
}

impl Config {
    /// Build config from env vars, with sensible defaults:
    /// WUKONG_MEMORY_DB (default $HOME/.wukong/memory.db),
    /// WUKONG_MEMORY_PORT (default 3917), WUKONG_MEMORY_HOST (default
    /// 127.0.0.1 — do not expose memory unauthenticated by default), and
    /// WUKONG_MEMORY_TOKEN (optional bearer token; when set it is required).
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
        let host = std::env::var("WUKONG_MEMORY_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let token = std::env::var("WUKONG_MEMORY_TOKEN")
            .ok()
            .filter(|t| !t.is_empty());
        Config {
            db_url,
            port,
            host,
            token,
        }
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
    input: std::result::Result<Json<RememberInput>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let Json(input) = input.map_err(|err| AppError(MemoryError::InvalidQuery(err.to_string())))?;
    Ok(Json(mem.remember(input).await?))
}

async fn recall(
    State(mem): State<Arc<Memory>>,
    query: std::result::Result<Json<RecallQuery>, JsonRejection>,
) -> Result<impl IntoResponse, AppError> {
    let Json(query) = query.map_err(|err| AppError(MemoryError::InvalidQuery(err.to_string())))?;
    Ok(Json(mem.recall(query).await?))
}

/// Constant-time byte comparison for token checks.
fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b) {
        diff |= x ^ y;
    }
    diff == 0
}

/// Require `Authorization: Bearer <token>` on protected routes when a token is
/// configured. `/v1/health` is left open for liveness checks.
async fn require_token(
    State(token): State<Option<String>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    let Some(expected) = token.as_deref() else {
        return next.run(request).await;
    };
    let provided = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .map(|s| s.trim());
    if provided
        .map(|p| ct_eq(p.as_bytes(), expected.as_bytes()))
        .unwrap_or(false)
    {
        next.run(request).await
    } else {
        StatusCode::UNAUTHORIZED.into_response()
    }
}

/// Build the axum router over a shared Memory instance. When `token` is `Some`,
/// every route except `/v1/health` requires a matching bearer token.
pub fn build_router(mem: Arc<Memory>, token: Option<String>) -> Router {
    let protected = Router::new()
        .route("/v1/stats", get(stats))
        .route("/v1/snapshot", get(snapshot))
        .route("/v1/remember", post(remember))
        .route("/v1/recall", post(recall))
        .route_layer(axum::middleware::from_fn_with_state(token, require_token));
    Router::new()
        .route("/v1/health", get(health))
        .merge(protected)
        .with_state(mem)
}
