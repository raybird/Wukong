//! wukong-web: a zero-build browser console for Wukong. Reuses run_turn and
//! streams role progress + the rendered answer over SSE.

pub mod memory_api;
pub mod schedule_api;
pub mod skills_api;
pub mod system_api;

mod chat_api;
mod static_assets;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::Json;
use chrono::{NaiveDate, TimeZone, Utc};
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use wukong_gateway::backend::AiBackend;
use wukong_memory::{Memory, RecallMode, RecallQuery};
use wukong_scheduler::SchedulerStore;
use wukong_settings::TelegramSettings;

use crate::static_assets::INDEX_HTML;

/// Shared router state. Generic over the backend so tests inject a mock.
pub struct AppState<B: AiBackend> {
    pub memory: Arc<Memory>,
    pub backend: Arc<B>,
    pub scope: String,
    pub db_url: String,
    pub token: Option<String>,
    pub settings_path: std::path::PathBuf,
}

// Manual Clone: Arc fields clone cheaply and B need not be Clone.
impl<B: AiBackend> Clone for AppState<B> {
    fn clone(&self) -> Self {
        Self {
            memory: self.memory.clone(),
            backend: self.backend.clone(),
            scope: self.scope.clone(),
            db_url: self.db_url.clone(),
            token: self.token.clone(),
            settings_path: self.settings_path.clone(),
        }
    }
}

/// Serve the SPA shell at `/`, injecting the token (if configured) so the
/// bundled UI can authenticate.
async fn index<B>(State(state): State<AppState<B>>) -> axum::response::Html<String>
where
    B: AiBackend + Send + Sync + 'static,
{
    let html = match &state.token {
        Some(t) => {
            // Serialize as a JSON string literal, then neutralize the sequences
            // that could break out of the inline <script> (`</script>`, HTML
            // metacharacters). `\uXXXX` renders back to the original char in JS.
            let json = serde_json::to_string(t).unwrap_or_else(|_| "null".to_string());
            let safe = json
                .replace('<', "\\u003c")
                .replace('>', "\\u003e")
                .replace('&', "\\u0026");
            INDEX_HTML.replace(
                "window.WUKONG_TOKEN = null;",
                &format!("window.WUKONG_TOKEN = {safe};"),
            )
        }
        None => INDEX_HTML.to_string(),
    };
    axum::response::Html(html)
}

pub type WebQuestionFuture<'a> =
    Pin<Box<dyn Future<Output = Result<(), wukong_gateway::GatewayError>> + Send + 'a>>;

pub trait WebQuestionResponder {
    fn reply_web_question<'a>(
        &'a self,
        session_id: &'a str,
        request_id: &'a str,
        answers: Vec<Vec<String>>,
    ) -> WebQuestionFuture<'a>;

    fn reject_web_question<'a>(
        &'a self,
        session_id: &'a str,
        request_id: &'a str,
    ) -> WebQuestionFuture<'a>;
}

impl WebQuestionResponder for wukong_gateway::backend::AgentBackend {
    fn reply_web_question<'a>(
        &'a self,
        session_id: &'a str,
        request_id: &'a str,
        answers: Vec<Vec<String>>,
    ) -> WebQuestionFuture<'a> {
        Box::pin(async move {
            match self {
                wukong_gateway::backend::AgentBackend::Server(server) => {
                    server.reply_question(session_id, request_id, answers).await
                }
                wukong_gateway::backend::AgentBackend::Cli(_) => {
                    Err(wukong_gateway::GatewayError::AgentFailed {
                        code: None,
                        stderr: "目前只有 opencode server backend 支援 question 回答。".to_string(),
                    })
                }
            }
        })
    }

    fn reject_web_question<'a>(
        &'a self,
        session_id: &'a str,
        request_id: &'a str,
    ) -> WebQuestionFuture<'a> {
        Box::pin(async move {
            match self {
                wukong_gateway::backend::AgentBackend::Server(server) => {
                    server.reject_question(session_id, request_id).await
                }
                wukong_gateway::backend::AgentBackend::Cli(_) => {
                    Err(wukong_gateway::GatewayError::AgentFailed {
                        code: None,
                        stderr: "目前只有 opencode server backend 支援 question 取消。".to_string(),
                    })
                }
            }
        })
    }
}

#[derive(serde::Deserialize)]
struct QuestionReplyRequest {
    session_id: String,
    answers: Vec<Vec<String>>,
}

#[derive(serde::Deserialize)]
struct QuestionRejectRequest {
    session_id: String,
}

#[derive(serde::Deserialize)]
pub(crate) struct SettingsQuery {
    pub(crate) token: Option<String>,
}

#[derive(serde::Serialize)]
struct SettingsResponse {
    telegram: TelegramSettingsResponse,
}

#[derive(serde::Serialize)]
struct TelegramSettingsResponse {
    configured: bool,
    token: String,
    allowed: String,
}

#[derive(serde::Deserialize)]
struct SaveSettingsRequest {
    telegram: TelegramSettings,
}

#[derive(serde::Serialize)]
struct ModelSettingsResponse {
    model: Option<String>,
    source: String,
    editable: bool,
}

#[derive(serde::Deserialize)]
struct SaveModelSettingsRequest {
    model: String,
}

pub(crate) fn authorized(expected: &Option<String>, provided: Option<&str>) -> bool {
    match expected {
        Some(t) => provided
            .map(|p| ct_eq(p.as_bytes(), t.as_bytes()))
            .unwrap_or(false),
        None => true,
    }
}

/// Constant-time byte comparison so token checks don't leak length-independent
/// timing. (Length itself is not hidden, which is standard for bearer tokens.)
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

/// Pull the request's bearer token from `Authorization: Bearer <t>`, falling
/// back to the `?token=` query parameter.
fn request_token(request: &axum::extract::Request) -> Option<String> {
    if let Some(bearer) = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    {
        let t = bearer.trim();
        if !t.is_empty() {
            return Some(t.to_string());
        }
    }
    axum::extract::Query::<std::collections::HashMap<String, String>>::try_from_uri(request.uri())
        .ok()
        .and_then(|axum::extract::Query(m)| m.get("token").cloned())
}

/// Auth gate for every protected route: rejects requests whose token does not
/// match the configured one. This is the single choke point — a new protected
/// route is covered automatically, without relying on a per-handler check. When
/// the token arrives via header, it is copied into the query so the existing
/// per-handler checks (defense in depth) also see it.
async fn require_token<B>(
    axum::extract::State(state): axum::extract::State<AppState<B>>,
    request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response
where
    B: AiBackend + WebQuestionResponder + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    let Some(expected) = state.token.as_deref() else {
        return next.run(request).await;
    };
    let provided = request_token(&request);
    if !provided
        .as_deref()
        .map(|p| ct_eq(p.as_bytes(), expected.as_bytes()))
        .unwrap_or(false)
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let request = ensure_query_token(request, expected);
    next.run(request).await
}

/// If the request has no `?token=`, append the validated token so query-based
/// per-handler checks succeed for header-authenticated clients.
fn ensure_query_token(request: axum::extract::Request, token: &str) -> axum::extract::Request {
    let has_query_token =
        axum::extract::Query::<std::collections::HashMap<String, String>>::try_from_uri(
            request.uri(),
        )
        .ok()
        .map(|axum::extract::Query(m)| m.contains_key("token"))
        .unwrap_or(false);
    if has_query_token {
        return request;
    }
    let (mut parts, body) = request.into_parts();
    let path = parts.uri.path();
    let new_pq = match parts.uri.query() {
        Some(q) if !q.is_empty() => format!("{path}?{q}&token={token}"),
        _ => format!("{path}?token={token}"),
    };
    if let Ok(uri) = new_pq.parse::<axum::http::Uri>() {
        parts.uri = uri;
    }
    axum::extract::Request::from_parts(parts, body)
}

/// Whether the console must refuse to start: no token configured while bound to
/// a non-loopback interface, unless the operator explicitly opted into an
/// insecure bind. Keeps the "public + no auth" footgun from happening silently.
pub fn should_refuse_insecure_start(token: Option<&str>, host: &str, allow_insecure: bool) -> bool {
    let is_loopback = matches!(host, "127.0.0.1" | "localhost" | "::1");
    token.is_none() && !is_loopback && !allow_insecure
}

pub(crate) fn capped_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(10).clamp(1, 50)
}

pub(crate) fn selected_scope(default_scope: &str, requested: Option<String>) -> String {
    requested
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_scope.to_string())
}

pub(crate) fn date_bounds_utc(date: &str) -> Result<(i64, i64), String> {
    let day = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let start = day
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "invalid date".to_string())?;
    let end = day
        .succ_opt()
        .ok_or_else(|| "invalid date".to_string())?
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| "invalid date".to_string())?;
    Ok((
        Utc.from_utc_datetime(&start).timestamp(),
        Utc.from_utc_datetime(&end).timestamp(),
    ))
}

async fn get_settings<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    match wukong_settings::load_settings(&state.settings_path) {
        Ok(settings) => {
            let telegram = settings.telegram;
            Json(SettingsResponse {
                telegram: TelegramSettingsResponse {
                    configured: !telegram.token.trim().is_empty(),
                    token: wukong_settings::redact_token(&telegram.token),
                    allowed: telegram.allowed,
                },
            })
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn post_settings<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<SaveSettingsRequest>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let mut settings = wukong_settings::load_settings(&state.settings_path).unwrap_or_default();
    settings.telegram = req.telegram;
    match wukong_settings::save_settings(&state.settings_path, &settings) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

fn env_model_override() -> Option<String> {
    std::env::var("WUKONG_MODEL")
        .ok()
        .map(|m| m.trim().to_string())
        .filter(|m| !m.is_empty())
}

async fn get_model_settings<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if let Some(model) = env_model_override() {
        return Json(ModelSettingsResponse {
            model: Some(model),
            source: "env".to_string(),
            editable: false,
        })
        .into_response();
    }
    match wukong_settings::load_settings(&state.settings_path) {
        Ok(settings) => Json(ModelSettingsResponse {
            model: settings.agent.default_model,
            source: "persisted".to_string(),
            editable: true,
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn put_model_settings<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<SaveModelSettingsRequest>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if env_model_override().is_some() {
        return (StatusCode::CONFLICT, "model is controlled by environment").into_response();
    }
    let model = req.model.trim();
    if model.is_empty() {
        return (StatusCode::BAD_REQUEST, "model must not be empty").into_response();
    }
    let mut settings = wukong_settings::load_settings(&state.settings_path).unwrap_or_default();
    settings.agent.default_model = Some(model.to_string());
    match wukong_settings::save_settings(&state.settings_path, &settings) {
        Ok(()) => Json(ModelSettingsResponse {
            model: Some(model.to_string()),
            source: "persisted".to_string(),
            editable: true,
        })
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn list_schedules<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let store = match SchedulerStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.list_jobs().await {
        Ok(jobs) => Json(
            jobs.into_iter()
                .map(schedule_api::job_response)
                .collect::<Vec<_>>(),
        )
        .into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn set_schedule_enabled<B>(
    State(state): State<AppState<B>>,
    Path((id, action)): Path<(String, String)>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let enabled = match action.as_str() {
        "enable" => true,
        "disable" => false,
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    let store = match SchedulerStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.set_enabled(&id, enabled).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn delete_schedule<B>(
    State(state): State<AppState<B>>,
    Path(id): Path<String>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let store = match SchedulerStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.remove_job(&id).await {
        Ok(true) => StatusCode::OK.into_response(),
        Ok(false) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

const SYSTEM_DIAGNOSTIC_TIMEOUT_SECS: u64 = 5;

async fn run_opencode_diagnostic(args: &[&str]) -> Result<String, String> {
    let util = wukong_gateway::backend::OpencodeUtility::from_agent_command(
        &[],
        wukong_gateway::workspace_dir(),
    );
    match tokio::time::timeout(
        std::time::Duration::from_secs(SYSTEM_DIAGNOSTIC_TIMEOUT_SECS),
        util.run_fixed(args),
    )
    .await
    {
        Ok(Ok(output)) => Ok(output),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("command timed out".to_string()),
    }
}

async fn run_github_auth_status() -> Result<String, String> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(SYSTEM_DIAGNOSTIC_TIMEOUT_SECS),
        tokio::process::Command::new("gh")
            .args(["auth", "status"])
            .output(),
    )
    .await
    {
        Ok(Ok(output)) if output.status.success() => {
            Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
        }
        Ok(Ok(output)) => {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            Err(if stderr.is_empty() {
                "gh auth status failed".to_string()
            } else {
                stderr
            })
        }
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err("command timed out".to_string()),
    }
}

async fn system_extra_groups() -> Vec<system_api::DiagnosticGroup> {
    let (providers_result, models_result, github_result) = tokio::join!(
        run_opencode_diagnostic(&["providers", "list"]),
        run_opencode_diagnostic(&["models"]),
        run_github_auth_status(),
    );

    let providers = system_api::command_diagnostic_item("providers", "Providers", providers_result);
    let models = system_api::command_diagnostic_item("models", "Models", models_result);
    let github = system_api::command_diagnostic_item("github_cli", "GitHub CLI", github_result);

    vec![
        system_api::DiagnosticGroup {
            id: "providers".to_string(),
            title: "Providers".to_string(),
            items: vec![providers],
        },
        system_api::DiagnosticGroup {
            id: "models".to_string(),
            title: "Models".to_string(),
            items: vec![models],
        },
        system_api::DiagnosticGroup {
            id: "tools".to_string(),
            title: "Tools".to_string(),
            items: vec![github],
        },
    ]
}

async fn get_system<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let store = match SchedulerStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.list_jobs().await {
        Ok(jobs) => {
            let groups = system_extra_groups().await;
            Json(system_api::system_response(
                &state.scope,
                state.token.is_some(),
                &state.db_url,
                &jobs,
                groups,
            ))
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_memory_summary<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<memory_api::MemorySummaryQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match state.memory.snapshot(params.scope.as_deref()).await {
        Ok(snapshot) => Json(snapshot).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_memory_records<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<memory_api::MemoryRecordsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let kind = match memory_api::parse_kind(params.kind.as_deref()) {
        Ok(kind) => kind,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    match state
        .memory
        .records(
            params.scope.as_deref(),
            kind,
            memory_api::capped_records_limit(params.limit),
        )
        .await
    {
        Ok(page) => Json(page).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn post_memory_recall_preview<B>(
    State(state): State<AppState<B>>,
    Query(query): Query<SettingsQuery>,
    Json(req): Json<memory_api::RecallPreviewRequest>,
) -> Result<Json<memory_api::RecallPreviewResponse>, (StatusCode, String)>
where
    B: AiBackend + Send + Sync + 'static,
{
    if !authorized(&state.token, query.token.as_deref()) {
        return Err((StatusCode::UNAUTHORIZED, "unauthorized".to_string()));
    }
    let normalized = memory_api::normalize_recall_request(req, &state.scope)
        .map_err(|err| (StatusCode::BAD_REQUEST, err))?;
    let result = state
        .memory
        .recall(RecallQuery {
            query: normalized.query.clone(),
            top_k: normalized.top_k,
            scope: Some(normalized.scope.clone()),
            mode: RecallMode::Hybrid,
        })
        .await
        .map_err(|err| (StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))?;

    Ok(Json(memory_api::RecallPreviewResponse {
        query: normalized.query,
        scope: normalized.scope,
        mode: normalized.mode,
        hits: result.data,
        evidence: result.evidence,
        confidence: result.confidence,
        latency_ms: result.latency_ms,
    }))
}

async fn get_skills_catalog<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    Json(skills_api::catalog_response()).into_response()
}

async fn get_skills_preferences<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match wukong_settings::load_settings(&state.settings_path) {
        Ok(settings) => {
            let prefs = wukong_settings::effective_planner_preferences(&settings);
            Json(skills_api::preferences_response(&prefs)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn put_skills_preferences<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<skills_api::SaveSkillPreferencesRequest>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let prefs = match skills_api::validate_preferences(req) {
        Ok(prefs) => prefs,
        Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
    };
    let mut settings = wukong_settings::load_settings(&state.settings_path).unwrap_or_default();
    settings.planner_preferences = prefs;
    match wukong_settings::save_settings(&state.settings_path, &settings) {
        Ok(()) => {
            let effective = wukong_settings::effective_planner_preferences(&settings);
            Json(skills_api::preferences_response(&effective)).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn post_question_reply<B>(
    State(state): State<AppState<B>>,
    Path(request_id): Path<String>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<QuestionReplyRequest>,
) -> axum::response::Response
where
    B: AiBackend + WebQuestionResponder + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    match state
        .backend
        .reply_web_question(&req.session_id, &request_id, req.answers)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

async fn post_question_reject<B>(
    State(state): State<AppState<B>>,
    Path(request_id): Path<String>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<QuestionRejectRequest>,
) -> axum::response::Response
where
    B: AiBackend + WebQuestionResponder + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    match state
        .backend
        .reject_web_question(&req.session_id, &request_id)
        .await
    {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::BAD_GATEWAY, e.to_string()).into_response(),
    }
}

/// Build the application router from shared state.
/// Unauthenticated liveness probe for container healthchecks. Returns 200 with
/// an empty body so it leaks nothing about state or auth. Deliberately part of
/// the public router group (no token required).
async fn healthz() -> StatusCode {
    StatusCode::OK
}

pub fn build_router<B>(state: AppState<B>) -> axum::Router
where
    B: AiBackend + WebQuestionResponder + Send + Sync + 'static,
{
    use axum::routing::{get, post};
    // Public: the SPA shell and its static assets, served without a token so the
    // UI can load and prompt for one.
    let public = axum::Router::new()
        .route("/", get(index::<B>))
        .route("/healthz", get(healthz))
        .route("/app.js", get(static_assets::app_js))
        .route("/lib/html.js", get(static_assets::html_js))
        .route("/lib/chat-layout.mjs", get(static_assets::chat_layout_js))
        .route(
            "/lib/unread-marker.mjs",
            get(static_assets::unread_marker_js),
        )
        .route("/components/wukong-chat.js", get(static_assets::chat_js))
        .route(
            "/components/chat-thread-header.js",
            get(static_assets::chat_thread_header_js),
        )
        .route(
            "/components/chat-message.js",
            get(static_assets::chat_message_js),
        )
        .route(
            "/components/chat-activity.js",
            get(static_assets::chat_activity_js),
        )
        .route(
            "/components/chat-question-card.js",
            get(static_assets::chat_question_card_js),
        )
        .route(
            "/components/wukong-memory.js",
            get(static_assets::memory_js),
        )
        .route(
            "/components/wukong-skills.js",
            get(static_assets::skills_js),
        )
        .route(
            "/components/wukong-settings.js",
            get(static_assets::settings_js),
        )
        .route(
            "/components/wukong-schedules.js",
            get(static_assets::schedules_js),
        )
        .route(
            "/components/wukong-system.js",
            get(static_assets::system_js),
        )
        .route("/styles.css", get(static_assets::styles_css))
        .route("/settings", get(index::<B>));

    // Protected: everything that touches memory/agent/settings. The auth
    // middleware is the single choke point covering all of these.
    let protected = axum::Router::new()
        .route("/chat", get(chat_api::chat::<B>))
        .route("/api/chat/scopes", get(chat_api::get_chat_scopes::<B>))
        .route("/api/chat/stream", get(chat_api::stream_chat_events::<B>))
        .route("/api/chat/messages", get(chat_api::get_chat_messages::<B>))
        .route(
            "/api/chat/messages/:id/steps",
            get(chat_api::get_chat_steps::<B>),
        )
        .route(
            "/api/chat/messages/:id/events",
            get(chat_api::get_chat_events::<B>),
        )
        .route(
            "/api/chat/attachments/:id",
            get(chat_api::get_attachment::<B>),
        )
        .route(
            "/api/chat/attachments/:id/preview",
            get(chat_api::get_attachment_preview::<B>),
        )
        .route(
            "/api/questions/:request_id/reply",
            post(post_question_reply::<B>),
        )
        .route(
            "/api/questions/:request_id/reject",
            post(post_question_reject::<B>),
        )
        .route(
            "/api/settings",
            get(get_settings::<B>).post(post_settings::<B>),
        )
        .route(
            "/api/settings/model",
            get(get_model_settings::<B>).put(put_model_settings::<B>),
        )
        .route("/api/memory/summary", get(get_memory_summary::<B>))
        .route("/api/memory/records", get(get_memory_records::<B>))
        .route(
            "/api/memory/recall-preview",
            post(post_memory_recall_preview::<B>),
        )
        .route("/api/skills/catalog", get(get_skills_catalog::<B>))
        .route(
            "/api/skills/preferences",
            get(get_skills_preferences::<B>).put(put_skills_preferences::<B>),
        )
        .route("/api/schedules", get(list_schedules::<B>))
        .route(
            "/api/schedules/:id/:action",
            post(set_schedule_enabled::<B>),
        )
        .route(
            "/api/schedules/:id",
            axum::routing::delete(delete_schedule::<B>),
        )
        .route("/api/system", get(get_system::<B>))
        .route_layer(axum::middleware::from_fn_with_state(
            state.clone(),
            require_token::<B>,
        ));

    public.merge(protected).with_state(state)
}

/// Static 503 page served by [`build_misconfigured_router`]. Explains the two
/// ways to fix an unsafe bind. Self-contained (inline CSS, no external assets)
/// and theme-aware via `color-scheme`.
const MISCONFIGURED_HTML: &str = r#"<!doctype html>
<html lang="zh-Hant">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Wukong Web Console — 設定錯誤</title>
<style>
  :root { color-scheme: light dark; }
  body { font-family: system-ui, -apple-system, "Noto Sans TC", sans-serif;
         max-width: 40rem; margin: 4rem auto; padding: 0 1.5rem; line-height: 1.7; }
  h1 { font-size: 1.4rem; }
  code { background: rgba(127,127,127,.18); padding: .1em .4em; border-radius: .3em; }
  .fix { border-left: 3px solid #cc3388; padding-left: 1rem; margin: 1.2rem 0; }
  a { color: #3377aa; }
</style>
</head>
<body>
<h1>🐵 Wukong Web Console 拒絕啟動</h1>
<p>偵測到不安全的設定：Console 綁定在非 loopback 位址，但未設定存取密鑰
   <code>WUKONG_WEB_TOKEN</code>。為避免無認證對外開放，服務暫不提供功能。</p>
<p>請擇一修正後重啟：</p>
<div class="fix">
  <p><strong>方式一（建議）</strong>：設定存取密鑰</p>
  <p><code>WUKONG_WEB_TOKEN=&lt;你的密鑰&gt;</code></p>
</div>
<div class="fix">
  <p><strong>方式二</strong>：明確允許無認證綁定（僅限可信內網）</p>
  <p><code>WUKONG_WEB_ALLOW_INSECURE=1</code></p>
</div>
<p>Docker 部署請於 <code>.env</code> 設定後執行 <code>docker compose up -d</code>；
   詳見 <a href="https://github.com/raybird/Wukong/blob/main/docs/docker.md">docs/docker.md</a>。</p>
</body>
</html>"#;

/// Router used when [`should_refuse_insecure_start`] would otherwise abort the
/// process. Rather than exiting — which, under `restart: unless-stopped`,
/// degrades into an invisible crash loop (users just see connection refused) —
/// bind the same address and answer every request, including `/healthz`, with
/// `503` and a page explaining the fix. Fail-closed: it carries no state and
/// mounts no functional routes, so nothing touches memory/backend/settings.
pub fn build_misconfigured_router() -> axum::Router {
    axum::Router::new().fallback(serve_misconfigured_page)
}

async fn serve_misconfigured_page() -> (StatusCode, axum::response::Html<&'static str>) {
    (
        StatusCode::SERVICE_UNAVAILABLE,
        axum::response::Html(MISCONFIGURED_HTML),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::static_assets::{CHAT_ACTIVITY_JS, CHAT_JS, STYLES_CSS};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use tower::ServiceExt;
    use wukong_chat_history::ChatHistoryStore;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<String>>,
        prompts: Mutex<Vec<String>>,
    }
    impl MockBackend {
        fn new(r: &[&str]) -> Self {
            Self {
                replies: Mutex::new(r.iter().map(|s| s.to_string()).collect()),
                prompts: Mutex::new(Vec::new()),
            }
        }
    }
    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.prompts.lock().unwrap().push(req.prompt);
            Ok(AgentResponse {
                text: self.replies.lock().unwrap().pop_front().unwrap_or_default(),
                session_id: None,
            })
        }
    }

    macro_rules! unsupported_question_responder {
        ($backend:ty) => {
            impl WebQuestionResponder for $backend {
                fn reply_web_question<'a>(
                    &'a self,
                    _session_id: &'a str,
                    _request_id: &'a str,
                    _answers: Vec<Vec<String>>,
                ) -> WebQuestionFuture<'a> {
                    Box::pin(async move {
                        Err(GatewayError::AgentFailed {
                            code: None,
                            stderr: "question responder is not configured for this test backend"
                                .to_string(),
                        })
                    })
                }

                fn reject_web_question<'a>(
                    &'a self,
                    _session_id: &'a str,
                    _request_id: &'a str,
                ) -> WebQuestionFuture<'a> {
                    Box::pin(async move {
                        Err(GatewayError::AgentFailed {
                            code: None,
                            stderr: "question responder is not configured for this test backend"
                                .to_string(),
                        })
                    })
                }
            }
        };
    }

    unsupported_question_responder!(MockBackend);

    async fn state(token: Option<&str>, replies: &[&str]) -> AppState<MockBackend> {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        AppState {
            memory: Arc::new(Memory::open(&url).await.unwrap()),
            backend: Arc::new(MockBackend::new(replies)),
            scope: "global".to_string(),
            db_url: url,
            token: token.map(|s| s.to_string()),
            settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
        }
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    async fn content_type(app: axum::Router, uri: &str) -> String {
        let resp = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{uri} not 200");
        resp.headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn serves_static_assets_with_content_types() {
        assert!(
            content_type(build_router(state(None, &[]).await), "/app.js")
                .await
                .contains("javascript")
        );
        assert!(
            content_type(build_router(state(None, &[]).await), "/lib/html.js")
                .await
                .contains("javascript")
        );
        assert!(
            content_type(build_router(state(None, &[]).await), "/lib/chat-layout.mjs")
                .await
                .contains("javascript")
        );
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/lib/unread-marker.mjs"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-chat.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/chat-thread-header.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/chat-message.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/chat-activity.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/chat-question-card.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-settings.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-schedules.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-system.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-memory.js"
        )
        .await
        .contains("javascript"));
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-skills.js"
        )
        .await
        .contains("javascript"));
        assert!(
            content_type(build_router(state(None, &[]).await), "/styles.css")
                .await
                .contains("css")
        );
    }

    #[tokio::test]
    async fn chat_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &["oracle", "ans"]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn chat_accepts_matching_token() {
        let app = build_router(state(Some("sekret"), &["oracle", "ans"]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi&token=sekret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_accepts_bearer_header_token() {
        let app = build_router(state(Some("sekret"), &["oracle", "ans"]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .header("authorization", "Bearer sekret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn chat_rejects_wrong_bearer_header() {
        let app = build_router(state(Some("sekret"), &["oracle", "ans"]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .header("authorization", "Bearer nope")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[test]
    fn refuses_insecure_public_bind_only_when_unsafe() {
        assert!(should_refuse_insecure_start(None, "0.0.0.0", false));
        assert!(!should_refuse_insecure_start(None, "127.0.0.1", false));
        assert!(!should_refuse_insecure_start(None, "localhost", false));
        assert!(!should_refuse_insecure_start(None, "0.0.0.0", true));
        assert!(!should_refuse_insecure_start(Some("t"), "0.0.0.0", false));
    }

    #[tokio::test]
    async fn misconfigured_router_returns_503_with_guidance_on_all_paths() {
        // Every path — including /healthz, so the container healthcheck reads
        // unhealthy — must answer 503 with the fix instructions in the body.
        for path in ["/", "/healthz", "/api/chat/messages", "/whatever"] {
            let resp = build_misconfigured_router()
                .oneshot(Request::builder().uri(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(
                resp.status(),
                StatusCode::SERVICE_UNAVAILABLE,
                "path {path} should return 503"
            );
            let text = body_string(resp).await;
            assert!(
                text.contains("WUKONG_WEB_TOKEN"),
                "path {path} body missing token hint"
            );
            assert!(
                text.contains("WUKONG_WEB_ALLOW_INSECURE"),
                "path {path} body missing allow-insecure hint"
            );
        }
    }

    #[tokio::test]
    async fn index_injects_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_string(resp).await;
        assert!(
            body.contains(r#"window.WUKONG_TOKEN = "sekret""#),
            "token not injected:\n{body}"
        );
    }

    #[tokio::test]
    async fn chat_new_command_clears_session() {
        let app_state = state(None, &[]).await;
        app_state
            .memory
            .set_agent_session("global", "ses_1")
            .await
            .unwrap();
        let app = build_router(app_state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=/new")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("event: answer"), "missing answer:\n{body}");
        assert!(body.contains("已開新"), "missing reply:\n{body}");
        assert!(
            !body.contains("event: role"),
            "should not run a turn:\n{body}"
        );
        assert!(body.contains("event: done"));
        assert_eq!(
            app_state.memory.agent_session("global").await.unwrap(),
            None
        );
    }

    #[tokio::test]
    async fn chat_streams_helper_step_before_answer() {
        // planner -> [explorer, fixer]; explorer (helper) emits e1, fixer (final) f2.
        let app =
            build_router(state(None, &["explorer|systematic-debugging\nfixer", "e1", "f2"]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=build%20and%20fix")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;

        let step_at = body.find("event: step").expect("missing step event");
        let answer_at = body.find("event: answer").expect("missing answer event");
        // Helper baton card streams before the final answer.
        assert!(step_at < answer_at, "step must precede answer:\n{body}");
        // The helper step carries its role and rendered output; the final (fixer)
        // output goes through the answer event, not a step.
        assert!(body.contains(r#""role":"explorer""#), "step role:\n{body}");
        assert!(
            body.contains(r#""skill":"systematic-debugging""#),
            "step skill:\n{body}"
        );
        assert!(
            !body.contains(r#""role":"fixer""#),
            "final must not be a step:\n{body}"
        );
    }

    #[tokio::test]
    async fn turn_persists_helper_steps_and_serves_them() {
        let app_state = state(None, &["explorer, fixer", "e1", "f2"]).await;

        // Reading the full SSE body waits for the turn thread (and its DB writes).
        let body = body_string(
            build_router(app_state.clone())
                .oneshot(
                    Request::builder()
                        .uri("/chat?q=build%20and%20fix")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        assert!(
            body.contains("event: answer"),
            "turn did not finish:\n{body}"
        );

        // The messages payload carries step_count; the assistant message has one.
        let msgs = body_string(
            build_router(app_state.clone())
                .oneshot(
                    Request::builder()
                        .uri("/api/chat/messages?limit=10")
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        let parsed: serde_json::Value = serde_json::from_str(&msgs).unwrap();
        let assistant = parsed["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["role"] == "assistant")
            .expect("no assistant message");
        assert_eq!(assistant["step_count"].as_i64().unwrap(), 1);
        let mid = assistant["id"].as_i64().unwrap();

        // The lazy steps endpoint returns the helper (explorer) baton, not the final.
        let steps_body = body_string(
            build_router(app_state.clone())
                .oneshot(
                    Request::builder()
                        .uri(format!("/api/chat/messages/{mid}/steps"))
                        .body(Body::empty())
                        .unwrap(),
                )
                .await
                .unwrap(),
        )
        .await;
        let steps: serde_json::Value = serde_json::from_str(&steps_body).unwrap();
        let steps = steps.as_array().unwrap();
        assert_eq!(steps.len(), 1);
        assert_eq!(steps[0]["role"].as_str().unwrap(), "explorer");
        assert_eq!(steps[0]["content"].as_str().unwrap(), "e1");
    }

    #[tokio::test]
    async fn chat_set_models_command_persists_model_and_records_history() {
        let app_state = state(None, &[]).await;
        let settings_path = app_state.settings_path.clone();
        let db_url = app_state.db_url.clone();
        let app = build_router(app_state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=/set_models%20opencode/deepseek-v4-flash-free")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("已設定預設模型"), "body: {body}");

        let saved = wukong_settings::load_settings(&settings_path).unwrap();
        assert_eq!(
            saved.agent.default_model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );

        let store = ChatHistoryStore::open(&db_url).await.unwrap();
        let thread = store.default_thread("global").await.unwrap();
        let messages = store.latest_messages(&thread, 10).await.unwrap();
        assert!(messages
            .iter()
            .any(|m| m.role == "user" && m.content.contains("/set_models")));
        assert!(messages
            .iter()
            .any(|m| m.role == "assistant" && m.content.contains("已設定預設模型")));
    }

    struct ReasoningBackend {
        reasoning: &'static str,
    }
    impl AiBackend for ReasoningBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "答案".to_string(),
                session_id: None,
            })
        }
        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::Reasoning(
                self.reasoning.to_string(),
            ));
            self.run(req).await
        }
    }
    unsupported_question_responder!(ReasoningBackend);

    async fn reasoning_state(reasoning: &'static str) -> AppState<ReasoningBackend> {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        AppState {
            memory: Arc::new(Memory::open(&url).await.unwrap()),
            backend: Arc::new(ReasoningBackend { reasoning }),
            scope: "global".to_string(),
            db_url: url,
            token: None,
            settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
        }
    }

    struct QuestionState;

    impl AiBackend for QuestionState {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "done".to_string(),
                session_id: Some("ses_1".to_string()),
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::QuestionRequest(
                wukong_gateway::stream::QuestionRequest {
                    request_id: "que_1".to_string(),
                    session_id: "ses_1".to_string(),
                    questions: vec![wukong_gateway::stream::QuestionInfo {
                        question: "選一個".to_string(),
                        header: "偏好".to_string(),
                        options: vec![wukong_gateway::stream::QuestionOption {
                            label: "A".to_string(),
                            description: "第一個".to_string(),
                        }],
                        multiple: false,
                        custom: true,
                    }],
                },
            ));
            self.run(req).await
        }
    }
    unsupported_question_responder!(QuestionState);

    async fn question_state() -> AppState<QuestionState> {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        AppState {
            memory: Arc::new(Memory::open(&url).await.unwrap()),
            backend: Arc::new(QuestionState),
            scope: "global".to_string(),
            db_url: url,
            token: None,
            settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
        }
    }

    type QuestionReplyLog = Mutex<Vec<(String, String, Vec<Vec<String>>)>>;

    #[derive(Default)]
    struct RecordingQuestionBackend {
        replies: QuestionReplyLog,
        rejects: Mutex<Vec<(String, String)>>,
    }

    impl AiBackend for RecordingQuestionBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "ok".to_string(),
                session_id: None,
            })
        }
    }

    impl WebQuestionResponder for RecordingQuestionBackend {
        fn reply_web_question<'a>(
            &'a self,
            session_id: &'a str,
            request_id: &'a str,
            answers: Vec<Vec<String>>,
        ) -> WebQuestionFuture<'a> {
            Box::pin(async move {
                self.replies.lock().unwrap().push((
                    session_id.to_string(),
                    request_id.to_string(),
                    answers,
                ));
                Ok(())
            })
        }

        fn reject_web_question<'a>(
            &'a self,
            session_id: &'a str,
            request_id: &'a str,
        ) -> WebQuestionFuture<'a> {
            Box::pin(async move {
                self.rejects
                    .lock()
                    .unwrap()
                    .push((session_id.to_string(), request_id.to_string()));
                Ok(())
            })
        }
    }

    #[tokio::test]
    async fn question_reply_api_records_answers() {
        let backend = Arc::new(RecordingQuestionBackend::default());
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        let app = build_router(AppState {
            memory: Arc::new(Memory::open(&url).await.unwrap()),
            backend: backend.clone(),
            scope: "global".to_string(),
            db_url: url,
            token: None,
            settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/questions/que_1/reply")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_id":"ses_1","answers":[["A"]]}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            backend.replies.lock().unwrap().as_slice(),
            &[(
                "ses_1".to_string(),
                "que_1".to_string(),
                vec![vec!["A".to_string()]],
            )]
        );
    }

    #[tokio::test]
    async fn question_reject_api_records_reject() {
        let backend = Arc::new(RecordingQuestionBackend::default());
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        let app = build_router(AppState {
            memory: Arc::new(Memory::open(&url).await.unwrap()),
            backend: backend.clone(),
            scope: "global".to_string(),
            db_url: url,
            token: None,
            settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
        });
        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/questions/que_1/reject")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"session_id":"ses_1"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            backend.rejects.lock().unwrap().as_slice(),
            &[("ses_1".to_string(), "que_1".to_string())]
        );
    }

    struct ReasoningToolBackend;
    impl AiBackend for ReasoningToolBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "答案".to_string(),
                session_id: None,
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::Reasoning("想一下".to_string()));
            on_event(wukong_gateway::StreamEvent::ToolUse("read".to_string()));
            self.run(req).await
        }
    }
    unsupported_question_responder!(ReasoningToolBackend);

    async fn reasoning_tool_state() -> AppState<ReasoningToolBackend> {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        AppState {
            memory: Arc::new(Memory::open(&url).await.unwrap()),
            backend: Arc::new(ReasoningToolBackend),
            scope: "global".to_string(),
            db_url: url,
            token: None,
            settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
        }
    }

    #[tokio::test]
    async fn chat_streams_tool_event() {
        let app = build_router(reasoning_tool_state().await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_string(resp).await;
        assert!(body.contains("event: reasoning"), "body: {body}");
        assert!(body.contains("event: tool"), "body: {body}");
        assert!(body.contains("read"), "body: {body}");
    }

    #[tokio::test]
    async fn chat_persists_turn_events_and_serves_them() {
        let app_state = reasoning_tool_state().await;
        let db_url = app_state.db_url.clone();
        let app = build_router(app_state);
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert!(body_string(resp).await.contains("event: answer"));

        let store = ChatHistoryStore::open(&db_url).await.unwrap();
        let thread = store.default_thread("global").await.unwrap();
        let messages = store.latest_messages(&thread, 10).await.unwrap();
        let assistant = messages.iter().find(|m| m.role == "assistant").unwrap();
        assert!(assistant.event_count > 0);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/chat/messages/{}/events", assistant.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("reasoning"), "body: {body}");
        assert!(body.contains("tool_use"), "body: {body}");
    }

    #[tokio::test]
    async fn settings_get_returns_default_state() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains(r#""configured":false"#), "body: {body}");
        assert!(body.contains(r#""allowed":"""#), "body: {body}");
    }

    #[tokio::test]
    async fn settings_post_writes_telegram_settings() {
        let app_state = state(None, &[]).await;
        let settings_path = app_state.settings_path.clone();
        let app = build_router(app_state);
        let body = r#"{"telegram":{"token":"123:abc","allowed":"42 99"}}"#;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let saved = wukong_settings::load_settings(&settings_path).unwrap();
        assert_eq!(saved.telegram.token, "123:abc");
        assert_eq!(saved.telegram.allowed, "42 99");
    }

    #[tokio::test]
    async fn settings_post_preserves_agent_settings() {
        let app_state = state(None, &[]).await;
        let settings_path = app_state.settings_path.clone();
        wukong_settings::save_settings(
            &settings_path,
            &wukong_settings::Settings {
                telegram: TelegramSettings::default(),
                agent: wukong_settings::AgentSettings {
                    default_model: Some("opencode/deepseek-v4-flash-free".to_string()),
                },
                planner_preferences: wukong_settings::PlannerPreferences::default(),
            },
        )
        .unwrap();
        let app = build_router(app_state);
        let body = r#"{"telegram":{"token":"123:abc","allowed":"42 99"}}"#;

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/settings")
                    .header(axum::http::header::CONTENT_TYPE, "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let saved = wukong_settings::load_settings(&settings_path).unwrap();
        assert_eq!(saved.telegram.token, "123:abc");
        assert_eq!(
            saved.agent.default_model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
    }

    #[tokio::test]
    async fn settings_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn model_settings_round_trip() {
        let app = build_router(state(None, &[]).await);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/settings/model")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"model":"opencode/deepseek-v4-flash-free"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/settings/model")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("opencode/deepseek-v4-flash-free"));
        assert!(body.contains("persisted"));
    }

    #[tokio::test]
    async fn model_settings_reject_empty_model() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/settings/model")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"model":"   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn memory_summary_returns_snapshot() {
        let app_state = state(None, &[]).await;
        app_state
            .memory
            .remember(wukong_memory::RememberInput {
                scope: "project:Wukong".to_string(),
                session_id: None,
                items: vec![wukong_memory::MemoryItem {
                    kind: wukong_memory::MemoryKind::Decision,
                    text: "Use Web Console as control center".to_string(),
                    importance: Some(1.0),
                    dedupe_key: None,
                }],
            })
            .await
            .unwrap();
        let app = build_router(app_state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/memory/summary?scope=project:Wukong")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("project:Wukong"));
        assert!(body.contains("consolidation_candidates"));
    }

    #[tokio::test]
    async fn memory_records_returns_recent_rows() {
        let app_state = state(None, &[]).await;
        app_state
            .memory
            .remember(wukong_memory::RememberInput {
                scope: "project:Wukong".to_string(),
                session_id: None,
                items: vec![wukong_memory::MemoryItem {
                    kind: wukong_memory::MemoryKind::Note,
                    text: "Memory panel can read records".to_string(),
                    importance: Some(0.8),
                    dedupe_key: None,
                }],
            })
            .await
            .unwrap();
        let app = build_router(app_state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/memory/records?scope=project:Wukong&limit=5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("Memory panel can read records"));
        assert!(body.contains("has_more"));
    }

    #[tokio::test]
    async fn memory_recall_preview_returns_hits() {
        let app_state = state(None, &[]).await;
        app_state
            .memory
            .remember(wukong_memory::RememberInput {
                scope: "project:Wukong".to_string(),
                session_id: None,
                items: vec![wukong_memory::MemoryItem {
                    kind: wukong_memory::MemoryKind::Note,
                    text: "scheduler memory query note".to_string(),
                    importance: Some(0.8),
                    dedupe_key: None,
                }],
            })
            .await
            .unwrap();
        let app = build_router(app_state);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/memory/recall-preview")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"query":"scheduler memory","scope":"project:Wukong","top_k":5,"mode":"hybrid"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("scheduler memory query note"), "body: {body}");
        assert!(body.contains(r#""mode":"hybrid""#), "body: {body}");
        assert!(body.contains(r#""explanation""#), "body: {body}");
        assert!(body.contains(r#""lexical""#), "body: {body}");
        assert!(body.contains(r#""source_signals""#), "body: {body}");
    }

    #[tokio::test]
    async fn memory_recall_preview_rejects_blank_query() {
        let app = build_router(state(None, &[]).await);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/memory/recall-preview")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"   "}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(body_string(resp).await.contains("query is required"));
    }

    #[tokio::test]
    async fn memory_recall_preview_rejects_unknown_mode() {
        let app = build_router(state(None, &[]).await);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/memory/recall-preview")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"memory","mode":"keyword"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(body_string(resp).await.contains("unsupported recall mode"));
    }

    #[tokio::test]
    async fn memory_recall_preview_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/api/memory/recall-preview")
                    .header("content-type", "application/json")
                    .body(Body::from(r#"{"query":"memory"}"#))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn skills_catalog_returns_roles_and_skills() {
        let app = build_router(state(None, &[]).await);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/skills/catalog")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("Explorer"));
        assert!(body.contains("systematic-debugging"));
    }

    #[tokio::test]
    async fn skills_preferences_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/skills/preferences")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn get_skills_preferences_returns_defaults() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/skills/preferences")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains(r#""enabled":false"#));
        assert!(body.contains(r#""roles":[]"#));
        assert!(body.contains(r#""skills":[]"#));
        assert!(body.contains(r#""warnings":[]"#));
    }

    #[tokio::test]
    async fn put_skills_preferences_persists_normalized_values() {
        let state = state(None, &[]).await;
        let settings_path = state.settings_path.clone();
        let app = build_router(state);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/skills/preferences")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"enabled":true,"roles":["fixer","fixer","oracle"],"skills":["systematic-debugging"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains(r#""enabled":true"#));
        assert!(body.contains(r#""roles":["fixer","oracle"]"#));
        assert!(body.contains(r#""skills":["systematic-debugging"]"#));
        let saved = wukong_settings::load_settings(&settings_path).unwrap();
        assert!(saved.planner_preferences.enabled);
        assert_eq!(saved.planner_preferences.roles, vec!["fixer", "oracle"]);
        assert_eq!(
            saved.planner_preferences.skills,
            vec!["systematic-debugging"]
        );
    }

    #[tokio::test]
    async fn put_skills_preferences_rejects_unknown_role() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/skills/preferences")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"enabled":true,"roles":["not-a-role"],"skills":[]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(body_string(resp).await.contains("unknown role"));
    }

    #[tokio::test]
    async fn put_skills_preferences_rejects_unknown_skill() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/api/skills/preferences")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"enabled":true,"roles":[],"skills":["not-a-skill"]}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
        assert!(body_string(resp).await.contains("unknown skill"));
    }

    #[tokio::test]
    async fn chat_applies_saved_planner_preferences_to_turn_config() {
        let state = state(None, &["fixer|systematic-debugging", "answer"]).await;
        let backend = state.backend.clone();
        let settings = wukong_settings::Settings {
            telegram: wukong_settings::TelegramSettings::default(),
            agent: wukong_settings::AgentSettings::default(),
            planner_preferences: wukong_settings::PlannerPreferences {
                enabled: true,
                roles: vec!["fixer".to_string()],
                skills: vec!["systematic-debugging".to_string()],
            },
        };
        wukong_settings::save_settings(&state.settings_path, &settings).unwrap();
        let app = build_router(state);

        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=fix%20it")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let _ = body_string(resp).await;

        let prompts = backend.prompts.lock().unwrap();
        assert!(prompts[0].contains("[User Preferences]"));
        assert!(prompts[0].contains("Preferred roles: fixer"));
        assert!(prompts[0].contains("Preferred skills: systematic-debugging"));
    }

    #[tokio::test]
    async fn chat_messages_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/messages")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn chat_messages_include_attachments() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let thread = store.default_thread(&app_state.scope).await.unwrap();
        let message_id = store
            .insert_message(&thread, "user", "請看附件", None, "complete", 100)
            .await
            .unwrap();
        store
            .insert_attachment(&wukong_chat_history::NewChatAttachment {
                message_id,
                scope: app_state.scope.clone(),
                source: "telegram".to_string(),
                original_name: "report.pdf".to_string(),
                stored_name: "report.pdf".to_string(),
                relative_path: "project_Wukong/1/report.pdf".to_string(),
                mime_type: Some("application/pdf".to_string()),
                size_bytes: 12,
                sha256: None,
                telegram_file_id: Some("file_1".to_string()),
                created_at: 100,
            })
            .await
            .unwrap();
        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/messages")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("attachments"), "{body}");
        assert!(body.contains("report.pdf"), "{body}");
    }

    #[tokio::test]
    async fn attachment_download_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/attachments/1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn chat_stream_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/stream?scope=user%3Atg-12")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn chat_stream_replays_events_after_cursor_for_scope() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let first = store
            .insert_live_event("user:tg-12", "user", None, "old", Some(1), 100)
            .await
            .unwrap();
        store
            .insert_live_event("user:tg-99", "user", None, "wrong", Some(2), 101)
            .await
            .unwrap();
        store
            .insert_live_event("user:tg-12", "tool", Some("read"), "read", None, 102)
            .await
            .unwrap();

        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/chat/stream?scope=user%3Atg-12&after={first}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("event: tool"), "body: {body}");
        assert!(body.contains("read"), "body: {body}");
        assert!(!body.contains("old"), "body should respect cursor: {body}");
        assert!(!body.contains("wrong"), "body should filter scope: {body}");
    }

    #[tokio::test]
    async fn chat_messages_returns_latest_ten() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let thread = store.default_thread(&app_state.scope).await.unwrap();
        for i in 0..12 {
            store
                .insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i)
                .await
                .unwrap();
        }
        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/messages")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(!body.contains("m0"), "body should omit oldest rows: {body}");
        assert!(
            body.contains("m2"),
            "body should include first returned row: {body}"
        );
        assert!(
            body.contains("m11"),
            "body should include newest row: {body}"
        );
    }

    #[tokio::test]
    async fn chat_messages_before_returns_older_ten() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let thread = store.default_thread(&app_state.scope).await.unwrap();
        let mut ids = Vec::new();
        for i in 0..12 {
            ids.push(
                store
                    .insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i)
                    .await
                    .unwrap(),
            );
        }
        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/chat/messages?before={}", ids[10]))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("m0"), "body: {body}");
        assert!(body.contains("m9"), "body: {body}");
        assert!(
            !body.contains("m10"),
            "body should omit boundary row: {body}"
        );
    }

    #[tokio::test]
    async fn chat_messages_after_returns_next_five() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let thread = store.default_thread(&app_state.scope).await.unwrap();
        let mut ids = Vec::new();
        for i in 0..12 {
            ids.push(
                store
                    .insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i)
                    .await
                    .unwrap(),
            );
        }
        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/chat/messages?after={}&limit=5", ids[4]))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(
            !body.contains("m4"),
            "body should omit boundary row: {body}"
        );
        assert!(body.contains("m5"), "body: {body}");
        assert!(body.contains("m9"), "body: {body}");
        assert!(
            !body.contains("m10"),
            "body should stop after five rows: {body}"
        );
        assert!(body.contains(r#""has_more":true"#), "body: {body}");
    }

    #[tokio::test]
    async fn chat_messages_includes_latest_live_event_id() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let scope = app_state.scope.clone();

        let event_id = store
            .insert_live_event(&scope, "user", None, "test event", None, 100)
            .await
            .unwrap();

        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/messages?limit=5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(
            body.contains(&format!(r#""latest_live_event_id":{event_id}"#)),
            "body should include the latest live event id: {body}"
        );
    }

    #[tokio::test]
    async fn chat_turn_records_user_and_assistant_messages() {
        let app_state = state(None, &["oracle", "**ans**"]).await;
        let db_url = app_state.db_url.clone();
        let app = build_router(app_state.clone());
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let _ = body_string(resp).await;

        let store = ChatHistoryStore::open(&db_url).await.unwrap();
        let thread = store.default_thread("global").await.unwrap();
        let messages = store.latest_messages(&thread, 10).await.unwrap();
        assert!(messages
            .iter()
            .any(|m| m.role == "user" && m.content == "hi"));
        assert!(messages.iter().any(|m| {
            m.role == "assistant"
                && m.content == "**ans**"
                && m.content_html.as_deref() == Some("<p><strong>ans</strong></p>")
        }));
    }

    #[tokio::test]
    async fn chat_scopes_lists_default_and_telegram_scope() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let tg_thread = store.default_thread("user:tg-915354960").await.unwrap();
        store
            .insert_message(&tg_thread, "user", "from tg", None, "complete", 123)
            .await
            .unwrap();

        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/scopes")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("user:tg-915354960"), "body: {body}");
        assert!(body.contains("Telegram 915354960"), "body: {body}");
        assert!(body.contains("global"), "body: {body}");
    }

    #[tokio::test]
    async fn chat_messages_reads_requested_scope() {
        let app_state = state(None, &[]).await;
        let store = ChatHistoryStore::open(&app_state.db_url).await.unwrap();
        let default_thread = store.default_thread(&app_state.scope).await.unwrap();
        let tg_thread = store.default_thread("user:tg-915354960").await.unwrap();
        store
            .insert_message(&default_thread, "user", "from web", None, "complete", 100)
            .await
            .unwrap();
        store
            .insert_message(&tg_thread, "user", "from telegram", None, "complete", 101)
            .await
            .unwrap();

        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/chat/messages?scope=user%3Atg-915354960")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("from telegram"), "body: {body}");
        assert!(!body.contains("from web"), "body: {body}");
    }

    #[tokio::test]
    async fn chat_turn_records_into_requested_scope() {
        let app_state = state(None, &["oracle", "scoped answer"]).await;
        let db_url = app_state.db_url.clone();
        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi&scope=user%3Atg-915354960")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let _ = body_string(resp).await;

        let store = ChatHistoryStore::open(&db_url).await.unwrap();
        let tg_thread = store.default_thread("user:tg-915354960").await.unwrap();
        let messages = store.latest_messages(&tg_thread, 10).await.unwrap();
        assert!(messages
            .iter()
            .any(|m| m.role == "user" && m.content == "hi"));
        assert!(messages
            .iter()
            .any(|m| m.role == "assistant" && m.content == "scoped answer"));
    }

    #[tokio::test]
    async fn schedules_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/schedules")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn schedules_list_enable_disable_delete() {
        let app_state = state(None, &[]).await;
        let store = wukong_scheduler::SchedulerStore::open(&app_state.db_url)
            .await
            .unwrap();
        let job = store
            .add_job(wukong_scheduler::NewJob {
                name: "morning".to_string(),
                kind: wukong_scheduler::JobKind::Turn {
                    scope: "global".to_string(),
                    prompt: "hi".to_string(),
                },
                cron: "0 9 * * *".to_string(),
            })
            .await
            .unwrap();
        let app = build_router(app_state);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/schedules")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("morning"), "body: {body}");
        assert!(body.contains("turn"), "body: {body}");

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/schedules/{}/disable", job.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(!store.get_job(&job.id).await.unwrap().unwrap().enabled);

        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri(format!("/api/schedules/{}/enable", job.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(store.get_job(&job.id).await.unwrap().unwrap().enabled);

        let resp = app
            .oneshot(
                Request::builder()
                    .method("DELETE")
                    .uri(format!("/api/schedules/{}", job.id))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(store.get_job(&job.id).await.unwrap().is_none());
    }

    #[tokio::test]
    async fn system_returns_summary() {
        let app_state = state(Some("sekret"), &[]).await;
        let store = wukong_scheduler::SchedulerStore::open(&app_state.db_url)
            .await
            .unwrap();
        store
            .add_job(wukong_scheduler::NewJob {
                name: "prune".to_string(),
                kind: wukong_scheduler::JobKind::Maintenance {
                    scope: Some("global".to_string()),
                    task: wukong_scheduler::MaintenanceTask::Prune,
                },
                cron: "0 3 * * *".to_string(),
            })
            .await
            .unwrap();
        let app = build_router(app_state);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/system?token=sekret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains(r#""scope":"global""#), "body: {body}");
        assert!(body.contains(r#""token_enabled":true"#), "body: {body}");
        assert!(body.contains(r#""schedule_total":1"#), "body: {body}");
    }

    #[tokio::test]
    async fn system_returns_diagnostic_groups() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/system")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains(r#""groups""#), "body: {body}");
        assert!(body.contains(r#""id":"runtime""#), "body: {body}");
        assert!(body.contains(r#""id":"environment""#), "body: {body}");
        assert!(body.contains(r#""id":"schedules""#), "body: {body}");
    }

    #[tokio::test]
    async fn chat_streams_reasoning_event() {
        let app = build_router(reasoning_state("想一下").await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_string(resp).await;
        assert!(
            body.contains("event: reasoning"),
            "missing reasoning event:\n{body}"
        );
        assert!(body.contains("想一下"), "missing reasoning text:\n{body}");
    }

    #[tokio::test]
    async fn chat_skips_empty_reasoning() {
        let app = build_router(reasoning_state("").await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = body_string(resp).await;
        assert!(
            !body.contains("event: reasoning"),
            "empty reasoning should be skipped:\n{body}"
        );
    }

    #[test]
    fn chat_component_opens_live_thinking_details() {
        assert!(
            CHAT_JS.contains("liveThinkingNode"),
            "chat should use the shared live thinking renderer"
        );
        assert!(
            CHAT_ACTIVITY_JS.contains("details.open = true"),
            "live reasoning details should be open while streaming"
        );
        assert!(
            CHAT_ACTIVITY_JS.contains("className = 'activity-card thinking'"),
            "live thinking should render as an activity card"
        );
    }

    #[test]
    fn chat_component_uses_workbench_modules() {
        assert!(CHAT_JS.contains("/components/chat-thread-header.js"));
        assert!(CHAT_JS.contains("/components/chat-message.js"));
        assert!(CHAT_JS.contains("/components/chat-activity.js"));
        assert!(CHAT_JS.contains("/components/chat-question-card.js"));
        assert!(CHAT_JS.contains("chat-workbench"));
        assert!(CHAT_JS.contains("conversation-rail"));
    }

    #[test]
    fn chat_component_scrolls_to_bottom_after_layout() {
        assert!(
            CHAT_JS.contains("scrollToBottomAfterRender"),
            "initial message load should scroll after layout has completed"
        );
    }

    #[test]
    fn chat_component_scrolls_after_render_and_layout() {
        assert!(
            CHAT_JS.contains("scrollToBottomAfterRender"),
            "chat should centralize post-render bottom scrolling"
        );
        assert!(
            CHAT_JS.contains("requestAnimationFrame") && CHAT_JS.contains("decode()"),
            "bottom scroll should wait for animation frames and image decode/layout"
        );
    }

    #[test]
    fn chat_component_preserves_position_when_loading_older() {
        assert!(
            CHAT_JS.contains("preserveScrollPosition"),
            "loadOlder should use a named preserve-position helper"
        );
        assert!(
            CHAT_JS.contains("this.renderMessages(data.messages, 'prepend')"),
            "older messages should still render in prepend mode"
        );
    }

    #[test]
    fn chat_component_loads_from_last_seen_marker_first() {
        assert!(
            CHAT_JS.contains("readLastSeenMessageId") && CHAT_JS.contains("after="),
            "initial chat load should fetch from the localStorage cursor instead of latest history"
        );
        assert!(
            CHAT_JS.contains("limit=5"),
            "initial cursor load should fetch only a small window after the marker"
        );
    }

    #[test]
    fn chat_component_handles_question_events() {
        assert!(
            CHAT_JS.contains("addEventListener('question'")
                || CHAT_JS.contains("addEventListener(\"question\""),
            "direct chat SSE should listen for question events"
        );
        assert!(
            CHAT_JS.contains("data.kind === 'question'"),
            "live stream should handle question live events"
        );
        assert!(
            CHAT_JS.contains("renderQuestionCard"),
            "question UI should use a shared renderer"
        );
        assert!(
            CHAT_JS.contains("/api/questions/")
                && CHAT_JS.contains("/reply")
                && CHAT_JS.contains("/reject"),
            "question UI should call reply and reject APIs"
        );
    }

    #[test]
    fn chat_styles_include_question_card() {
        assert!(STYLES_CSS.contains(".question-card"));
        assert!(STYLES_CSS.contains(".question-option"));
        assert!(STYLES_CSS.contains(".question-footer"));
        assert!(STYLES_CSS.contains(".chat-workbench"));
        assert!(STYLES_CSS.contains(".chat-thread-header"));
        assert!(STYLES_CSS.contains(".conversation-rail"));
        assert!(STYLES_CSS.contains(".message-frame"));
        assert!(STYLES_CSS.contains(".activity-rail"));
        assert!(STYLES_CSS.contains("@media (max-width: 720px)"));
    }

    #[tokio::test]
    async fn chat_streams_role_answer_done() {
        // [0] planner -> "oracle" => single Oracle step; [1] execute -> markdown.
        let app = build_router(state(None, &["oracle", "**ans**"]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("event: role"), "missing role event:\n{body}");
        assert!(
            body.contains("event: answer"),
            "missing answer event:\n{body}"
        );
        assert!(
            body.contains("<strong>ans</strong>"),
            "answer not rendered:\n{body}"
        );
        assert!(body.contains("event: done"), "missing done event:\n{body}");
    }

    #[tokio::test]
    async fn chat_streams_question_event() {
        let app = build_router(question_state().await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(
            body.contains("event: question"),
            "missing question event:\n{body}"
        );
        assert!(
            body.contains(r#""request_id":"que_1""#),
            "missing request id:\n{body}"
        );
        assert!(
            body.contains(r#""session_id":"ses_1""#),
            "missing session id:\n{body}"
        );
        assert!(body.contains("選一個"), "missing question text:\n{body}");
    }

    #[tokio::test]
    async fn index_serves_the_shell() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains(r#"id="app""#));
        assert!(body.contains(r##"href="#/chat""##));
    }

    #[tokio::test]
    async fn healthz_is_public_and_returns_200_even_with_token_set() {
        // A token is configured, yet the liveness probe must pass without one.
        let app = build_router(state(Some("secret"), &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(body_string(resp).await.is_empty());
    }

    #[tokio::test]
    async fn settings_route_serves_the_shell() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/settings")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains(r#"id="app""#));
    }
}
