//! wukong-web: a zero-build browser console for Wukong. Reuses run_turn and
//! streams role progress + the rendered answer over SSE.

pub mod memory_api;
pub mod schedule_api;
pub mod skills_api;
pub mod system_api;

use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::Json;
use chrono::{NaiveDate, TimeZone, Utc};
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use wukong_chat_history::{ChatHistoryStore, ChatMessage};
use wukong_cli::run_turn_traced;
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_memory::Memory;
use wukong_scheduler::SchedulerStore;
use wukong_settings::TelegramSettings;

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

const INDEX_HTML: &str = include_str!("../static/index.html");

const APP_JS: &str = include_str!("../static/app.js");
const HTML_JS: &str = include_str!("../static/lib/html.js");
const CHAT_JS: &str = include_str!("../static/components/wukong-chat.js");
const MEMORY_JS: &str = include_str!("../static/components/wukong-memory.js");
const SKILLS_JS: &str = include_str!("../static/components/wukong-skills.js");
const SETTINGS_JS: &str = include_str!("../static/components/wukong-settings.js");
const SCHEDULES_JS: &str = include_str!("../static/components/wukong-schedules.js");
const SYSTEM_JS: &str = include_str!("../static/components/wukong-system.js");
const STYLES_CSS: &str = include_str!("../static/styles.css");

const JS: &str = "application/javascript";
const CSS: &str = "text/css";

/// Serve the SPA shell at `/`, injecting the token (if configured) so the
/// bundled UI can authenticate.
async fn index<B>(State(state): State<AppState<B>>) -> axum::response::Html<String>
where
    B: AiBackend + Send + Sync + 'static,
{
    let html = match &state.token {
        Some(t) => {
            // Tokens are short opaque strings; escape the two chars that could
            // break out of the JS string literal.
            let safe = t.replace('\\', "\\\\").replace('"', "\\\"");
            INDEX_HTML.replace(
                "window.WUKONG_TOKEN = null;",
                &format!(r#"window.WUKONG_TOKEN = "{safe}";"#),
            )
        }
        None => INDEX_HTML.to_string(),
    };
    axum::response::Html(html)
}

/// Build a static-asset response with an explicit content type.
fn asset(content_type: &'static str, body: &'static str) -> axum::response::Response {
    use axum::http::header::CONTENT_TYPE;
    use axum::response::IntoResponse;
    ([(CONTENT_TYPE, content_type)], body).into_response()
}

async fn app_js() -> axum::response::Response {
    asset(JS, APP_JS)
}
async fn html_js() -> axum::response::Response {
    asset(JS, HTML_JS)
}
async fn chat_js() -> axum::response::Response {
    asset(JS, CHAT_JS)
}
async fn memory_js() -> axum::response::Response {
    asset(JS, MEMORY_JS)
}
async fn skills_js() -> axum::response::Response {
    asset(JS, SKILLS_JS)
}
async fn settings_js() -> axum::response::Response {
    asset(JS, SETTINGS_JS)
}
async fn schedules_js() -> axum::response::Response {
    asset(JS, SCHEDULES_JS)
}
async fn system_js() -> axum::response::Response {
    asset(JS, SYSTEM_JS)
}
async fn styles_css() -> axum::response::Response {
    asset(CSS, STYLES_CSS)
}

/// Messages pushed from the turn task to the SSE stream.
enum SseMsg {
    Role(String),
    Reasoning(String),
    /// A non-final (helper) baton's rendered output, surfaced as a collapsible card.
    Step {
        role: String,
        skill: Option<String>,
        html: String,
    },
    Answer(String),
    Error(String),
    Done,
}

impl SseMsg {
    fn into_event(self) -> Event {
        match self {
            SseMsg::Role(r) => Event::default().event("role").data(r),
            SseMsg::Reasoning(t) => Event::default().event("reasoning").data(t),
            SseMsg::Step { role, skill, html } => Event::default().event("step").data(
                serde_json::json!({ "role": role, "skill": skill, "html": html }).to_string(),
            ),
            SseMsg::Answer(h) => Event::default().event("answer").data(h),
            SseMsg::Error(e) => Event::default().event("error").data(e),
            SseMsg::Done => Event::default().event("done").data("ok"),
        }
    }
}

#[derive(serde::Deserialize)]
struct ChatQuery {
    q: Option<String>,
    token: Option<String>,
    scope: Option<String>,
}

#[derive(serde::Deserialize)]
struct ChatMessagesQuery {
    token: Option<String>,
    before: Option<i64>,
    date: Option<String>,
    limit: Option<i64>,
    scope: Option<String>,
}

#[derive(serde::Serialize)]
struct ChatMessagesResponse {
    messages: Vec<ChatMessage>,
    has_more: bool,
}

#[derive(serde::Deserialize)]
struct SettingsQuery {
    token: Option<String>,
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

fn authorized(expected: &Option<String>, provided: Option<&str>) -> bool {
    match expected {
        Some(t) => provided == Some(t.as_str()),
        None => true,
    }
}

fn capped_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(10).clamp(1, 50)
}

fn selected_scope(default_scope: &str, requested: Option<String>) -> String {
    requested
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_scope.to_string())
}

fn date_bounds_utc(date: &str) -> Result<(i64, i64), String> {
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

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// `GET /chat?q=` — run a turn, streaming role progress then the rendered answer.
async fn chat<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<ChatQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return axum::http::StatusCode::UNAUTHORIZED.into_response();
    }

    let q = params.q.unwrap_or_default();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SseMsg>();

    if q.trim().is_empty() {
        let _ = tx.send(SseMsg::Error("空白訊息".to_string()));
        let _ = tx.send(SseMsg::Done);
    } else {
        let store = match ChatHistoryStore::open(&state.db_url).await {
            Ok(store) => store,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        };
        let scope = selected_scope(&state.scope, params.scope.clone());
        let thread = match store.default_thread(&scope).await {
            Ok(thread) => thread,
            Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        };
        if let Err(e) = store
            .insert_message(&thread, "user", &q, None, "complete", now_unix())
            .await
        {
            return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        }

        let mem = state.memory.clone();
        let backend = state.backend.clone();
        let db_url = state.db_url.clone();
        let settings_path = state.settings_path.clone();
        // run_turn's future is not Send (AiBackend uses async_fn_in_trait and the
        // callbacks are dyn FnMut), so it can't ride tokio::spawn or the axum
        // handler future. Drive it on a dedicated thread with its own
        // current-thread runtime; only the Send channel crosses back.
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    let _ = tx.send(SseMsg::Error(format!("runtime: {e}")));
                    let _ = tx.send(SseMsg::Done);
                    return;
                }
            };
            rt.block_on(async move {
                let cfg = GatewayConfig {
                    scope,
                    db_url: String::new(),
                    agent_command: vec![],
                    default_model: None,
                    planner_preferences: None,
                    thinking: true,
                    recall_top_k: 5,
                    stream: false,
                };
                let mut cfg = cfg;
                let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
                let agent_settings = wukong_settings::effective_agent_settings(&settings);
                cfg.apply_default_model(agent_settings.default_model.as_deref());
                let planner_preferences = wukong_settings::effective_planner_preferences(&settings);
                cfg.apply_planner_preferences(
                    planner_preferences.enabled,
                    planner_preferences.roles,
                    planner_preferences.skills,
                );
                // Leading-slash inputs are session commands, not turns.
                let trimmed = q.trim();
                if let Some(rest) = trimmed.strip_prefix('/') {
                    let mut parts = rest.splitn(2, char::is_whitespace);
                    let name = parts.next().unwrap_or("").to_string();
                    let args = parts.next().unwrap_or("").trim().to_string();
                    let reply = match wukong_cli::parse_session_command(&name, &args) {
                        Some(cmd) => match wukong_cli::run_session_command(
                            mem.as_ref(),
                            backend.as_ref(),
                            &cfg,
                            &settings_path,
                            cmd,
                        )
                        .await
                        {
                            Ok(t) => t,
                            Err(e) => format!("⚠️ 失敗：{e}"),
                        },
                        None => format!("指令 /{name} 尚未支援"),
                    };
                    let html = wukong_render::to_web_html(&reply);
                    if let Ok(store) = ChatHistoryStore::open(&db_url).await {
                        let _ = store
                            .insert_message(
                                &thread,
                                "assistant",
                                &reply,
                                Some(&html),
                                "complete",
                                now_unix(),
                            )
                            .await;
                    }
                    let _ = tx.send(SseMsg::Answer(html));
                    let _ = tx.send(SseMsg::Done);
                    return;
                }

                let role_tx = tx.clone();
                let ev_tx = tx.clone();
                let step_tx = tx.clone();
                // Buffer helper-baton steps to persist them after the turn, linked
                // to the final assistant message. (role, raw content, rendered html)
                let mut steps_buf: Vec<(String, String, String)> = Vec::new();
                let result = run_turn_traced(
                    mem.as_ref(),
                    backend.as_ref(),
                    &cfg,
                    &q,
                    &mut |ev| {
                        if let wukong_gateway::StreamEvent::Reasoning(t) = ev {
                            if !t.trim().is_empty() {
                                let _ = ev_tx.send(SseMsg::Reasoning(t));
                            }
                        }
                    },
                    &mut |role| {
                        let _ = role_tx.send(SseMsg::Role(role.name().to_string()));
                    },
                    &mut |step| {
                        let html = wukong_render::to_web_html(step.output);
                        let _ = step_tx.send(SseMsg::Step {
                            role: step.role.name().to_string(),
                            skill: step.skill_name.map(str::to_string),
                            html: html.clone(),
                        });
                        steps_buf.push((
                            step.role.name().to_string(),
                            step.output.to_string(),
                            html,
                        ));
                    },
                )
                .await;
                match result {
                    Ok(out) => {
                        let html = wukong_render::to_web_html(&out.text);
                        if let Ok(store) = ChatHistoryStore::open(&db_url).await {
                            let now = now_unix();
                            if let Ok(message_id) = store
                                .insert_message(
                                    &thread,
                                    "assistant",
                                    &out.text,
                                    Some(&html),
                                    "complete",
                                    now,
                                )
                                .await
                            {
                                // best-effort: surface failures don't block the answer.
                                for (seq, (role, content, step_html)) in
                                    steps_buf.iter().enumerate()
                                {
                                    let _ = store
                                        .insert_step(
                                            message_id,
                                            seq as i64,
                                            role,
                                            content,
                                            Some(step_html),
                                            now,
                                        )
                                        .await;
                                }
                            }
                        }
                        let _ = tx.send(SseMsg::Answer(html));
                    }
                    Err(e) => {
                        let msg = e.to_string();
                        if let Ok(store) = ChatHistoryStore::open(&db_url).await {
                            let _ = store
                                .insert_message(
                                    &thread,
                                    "assistant",
                                    &msg,
                                    None,
                                    "error",
                                    now_unix(),
                                )
                                .await;
                        }
                        let _ = tx.send(SseMsg::Error(msg));
                    }
                }
                let _ = tx.send(SseMsg::Done);
            });
        });
    }

    let stream = UnboundedReceiverStream::new(rx).map(|m| Ok::<Event, Infallible>(m.into_event()));
    Sse::new(stream).into_response()
}

async fn get_chat_messages<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<ChatMessagesQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let limit = capped_limit(params.limit);
    let store = match ChatHistoryStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    let scope = selected_scope(&state.scope, params.scope.clone());
    let thread = match store.default_thread(&scope).await {
        Ok(thread) => thread,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let result = if let Some(date) = params.date.as_deref() {
        match date_bounds_utc(date) {
            Ok((start, end)) => {
                store
                    .messages_for_date(&thread, start, end, limit + 1)
                    .await
            }
            Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
        }
    } else if let Some(before) = params.before {
        store.messages_before(&thread, before, limit + 1).await
    } else {
        store.latest_messages(&thread, limit + 1).await
    };

    match result {
        Ok(mut messages) => {
            let has_more = messages.len() as i64 > limit;
            if has_more {
                messages.remove(0);
            }
            Json(ChatMessagesResponse { messages, has_more }).into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// `GET /api/chat/messages/:id/steps` — the helper-baton steps for one assistant
/// message, lazily fetched when the user expands the collapsible card.
async fn get_chat_steps<B>(
    State(state): State<AppState<B>>,
    Path(message_id): Path<i64>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let store = match ChatHistoryStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.list_steps(message_id).await {
        Ok(steps) => Json(steps).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn get_chat_scopes<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<ChatMessagesQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let store = match ChatHistoryStore::open(&state.db_url).await {
        Ok(store) => store,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    match store.list_scopes(&state.scope).await {
        Ok(scopes) => Json(scopes).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
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
        Ok(jobs) => Json(system_api::system_response(
            &state.scope,
            state.token.is_some(),
            &state.db_url,
            &jobs,
        ))
        .into_response(),
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

/// Build the application router from shared state.
pub fn build_router<B>(state: AppState<B>) -> axum::Router
where
    B: AiBackend + Send + Sync + 'static,
{
    axum::Router::new()
        .route("/", axum::routing::get(index::<B>))
        .route("/app.js", axum::routing::get(app_js))
        .route("/lib/html.js", axum::routing::get(html_js))
        .route("/components/wukong-chat.js", axum::routing::get(chat_js))
        .route(
            "/components/wukong-memory.js",
            axum::routing::get(memory_js),
        )
        .route(
            "/components/wukong-skills.js",
            axum::routing::get(skills_js),
        )
        .route(
            "/components/wukong-settings.js",
            axum::routing::get(settings_js),
        )
        .route(
            "/components/wukong-schedules.js",
            axum::routing::get(schedules_js),
        )
        .route(
            "/components/wukong-system.js",
            axum::routing::get(system_js),
        )
        .route("/styles.css", axum::routing::get(styles_css))
        .route("/settings", axum::routing::get(index::<B>))
        .route("/chat", axum::routing::get(chat::<B>))
        .route("/api/chat/scopes", axum::routing::get(get_chat_scopes::<B>))
        .route(
            "/api/chat/messages",
            axum::routing::get(get_chat_messages::<B>),
        )
        .route(
            "/api/chat/messages/:id/steps",
            axum::routing::get(get_chat_steps::<B>),
        )
        .route(
            "/api/settings",
            axum::routing::get(get_settings::<B>).post(post_settings::<B>),
        )
        .route(
            "/api/settings/model",
            axum::routing::get(get_model_settings::<B>).put(put_model_settings::<B>),
        )
        .route(
            "/api/memory/summary",
            axum::routing::get(get_memory_summary::<B>),
        )
        .route(
            "/api/memory/records",
            axum::routing::get(get_memory_records::<B>),
        )
        .route(
            "/api/skills/catalog",
            axum::routing::get(get_skills_catalog::<B>),
        )
        .route(
            "/api/skills/preferences",
            axum::routing::get(get_skills_preferences::<B>).put(put_skills_preferences::<B>),
        )
        .route("/api/schedules", axum::routing::get(list_schedules::<B>))
        .route(
            "/api/schedules/:id/:action",
            axum::routing::post(set_schedule_enabled::<B>),
        )
        .route(
            "/api/schedules/:id",
            axum::routing::delete(delete_schedule::<B>),
        )
        .route("/api/system", axum::routing::get(get_system::<B>))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use tower::ServiceExt;
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
        assert!(content_type(
            build_router(state(None, &[]).await),
            "/components/wukong-chat.js"
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
