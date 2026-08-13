//! Chat, attachment, and SSE handlers plus their direct helpers.
//!
//! These handlers drive a turn (`run_turn_traced`), serve chat history and
//! attachments, and stream live events. `build_router` in `lib.rs` routes to the
//! `pub(crate)` handlers; shared auth/query helpers live in `crate`.

use axum::extract::{Path, Query, State};
use axum::http::{header, StatusCode};
use axum::response::sse::{Event, Sse};
use axum::Json;
use std::convert::Infallible;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use wukong_chat_history::{ChatAttachment, ChatMessage};
use wukong_cli::run_turn_traced;
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_runtime::util::{now_unix, upload_root};

use crate::{authorized, capped_limit, date_bounds_utc, selected_scope, AppState, SettingsQuery};

/// Messages pushed from the turn task to the SSE stream.
enum SseMsg {
    Role(String),
    Reasoning(String),
    ToolUse(String),
    Question(WebQuestionRequest),
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

#[derive(serde::Serialize)]
struct WebQuestionOption {
    label: String,
    description: String,
}

#[derive(serde::Serialize)]
struct WebQuestionInfo {
    question: String,
    header: String,
    options: Vec<WebQuestionOption>,
    multiple: bool,
    custom: bool,
}

#[derive(serde::Serialize)]
struct WebQuestionRequest {
    request_id: String,
    session_id: String,
    questions: Vec<WebQuestionInfo>,
}

fn web_question_request(req: wukong_gateway::stream::QuestionRequest) -> WebQuestionRequest {
    WebQuestionRequest {
        request_id: req.request_id,
        session_id: req.session_id,
        questions: req
            .questions
            .into_iter()
            .map(|q| WebQuestionInfo {
                question: q.question,
                header: q.header,
                options: q
                    .options
                    .into_iter()
                    .map(|o| WebQuestionOption {
                        label: o.label,
                        description: o.description,
                    })
                    .collect(),
                multiple: q.multiple,
                custom: q.custom,
            })
            .collect(),
    }
}

impl SseMsg {
    fn into_event(self) -> Event {
        match self {
            SseMsg::Role(r) => Event::default().event("role").data(r),
            SseMsg::Reasoning(t) => Event::default().event("reasoning").data(t),
            SseMsg::ToolUse(name) => Event::default().event("tool").data(name),
            SseMsg::Question(request) => Event::default()
                .event("question")
                .data(serde_json::to_string(&request).unwrap_or_else(|_| "{}".to_string())),
            SseMsg::Step { role, skill, html } => Event::default().event("step").data(
                serde_json::json!({ "role": role, "skill": skill, "html": html }).to_string(),
            ),
            SseMsg::Answer(h) => Event::default().event("answer").data(h),
            SseMsg::Error(e) => Event::default().event("error").data(e),
            SseMsg::Done => Event::default().event("done").data("ok"),
        }
    }
}

fn live_event_to_sse(event: wukong_chat_history::ChatLiveEvent) -> Event {
    let mut payload = serde_json::json!({
        "id": event.id,
        "scope": event.scope,
        "kind": event.kind,
        "content": event.content,
        "message_id": event.message_id,
        "created_at": event.created_at,
    });
    if let Some(label) = event.label {
        payload["label"] = serde_json::Value::String(label);
    }
    let name = payload["kind"].as_str().unwrap_or("message").to_string();
    Event::default().event(name).data(payload.to_string())
}

#[derive(serde::Deserialize)]
pub(crate) struct ChatQuery {
    q: Option<String>,
    token: Option<String>,
    scope: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ChatMessagesQuery {
    token: Option<String>,
    after: Option<i64>,
    before: Option<i64>,
    date: Option<String>,
    limit: Option<i64>,
    scope: Option<String>,
}

#[derive(serde::Deserialize)]
pub(crate) struct ChatStreamQuery {
    token: Option<String>,
    scope: Option<String>,
    after: Option<i64>,
}

#[derive(serde::Deserialize)]
pub(crate) struct AttachmentQuery {
    token: Option<String>,
    scope: Option<String>,
}

#[derive(serde::Serialize)]
struct ChatAttachmentResponse {
    id: i64,
    original_name: String,
    mime_type: Option<String>,
    size_bytes: i64,
    download_url: String,
    preview_url: Option<String>,
}

#[derive(serde::Serialize)]
struct ChatMessageResponse {
    #[serde(flatten)]
    message: ChatMessage,
    attachments: Vec<ChatAttachmentResponse>,
}

#[derive(serde::Serialize)]
struct ChatMessagesResponse {
    messages: Vec<ChatMessageResponse>,
    has_more: bool,
    latest_live_event_id: Option<i64>,
}

fn attachment_response(a: ChatAttachment) -> ChatAttachmentResponse {
    let is_image = a.mime_type.as_deref().unwrap_or("").starts_with("image/");
    ChatAttachmentResponse {
        id: a.id,
        original_name: a.original_name,
        mime_type: a.mime_type,
        size_bytes: a.size_bytes,
        download_url: format!("/api/chat/attachments/{}", a.id),
        preview_url: is_image.then(|| format!("/api/chat/attachments/{}/preview", a.id)),
    }
}

fn content_disposition_name(name: &str) -> String {
    let safe = wukong_chat_history::sanitize_filename(name);
    format!("attachment; filename=\"{}\"", safe.replace('"', "_"))
}

/// `GET /chat?q=` — run a turn, streaming role progress then the rendered answer.
pub(crate) async fn chat<B>(
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
        let store = state.history.clone();
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
                    {
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
                let mut events_buf: Vec<(i64, String, Option<String>, String, i64)> = Vec::new();
                let mut event_seq: i64 = 0;
                let result = run_turn_traced(
                    mem.as_ref(),
                    backend.as_ref(),
                    &cfg,
                    &q,
                    &mut |ev| match ev {
                        wukong_gateway::StreamEvent::Reasoning(t) => {
                            if !t.trim().is_empty() {
                                let now = now_unix();
                                events_buf.push((
                                    event_seq,
                                    "reasoning".to_string(),
                                    None,
                                    t.clone(),
                                    now,
                                ));
                                event_seq += 1;
                                let _ = ev_tx.send(SseMsg::Reasoning(t));
                            }
                        }
                        wukong_gateway::StreamEvent::ToolUse(name) => {
                            let now = now_unix();
                            events_buf.push((
                                event_seq,
                                "tool_use".to_string(),
                                Some(name.clone()),
                                format!("使用工具 {name}"),
                                now,
                            ));
                            event_seq += 1;
                            let _ = ev_tx.send(SseMsg::ToolUse(name));
                        }
                        wukong_gateway::StreamEvent::StepStart => {
                            let now = now_unix();
                            events_buf.push((
                                event_seq,
                                "step_start".to_string(),
                                None,
                                "step_start".to_string(),
                                now,
                            ));
                            event_seq += 1;
                        }
                        wukong_gateway::StreamEvent::StepFinish => {
                            let now = now_unix();
                            events_buf.push((
                                event_seq,
                                "step_finish".to_string(),
                                None,
                                "step_finish".to_string(),
                                now,
                            ));
                            event_seq += 1;
                        }
                        wukong_gateway::StreamEvent::QuestionRequest(request) => {
                            let _ = ev_tx.send(SseMsg::Question(web_question_request(request)));
                        }
                        wukong_gateway::StreamEvent::Text(_) => {}
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
                        {
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
                                for (seq, kind, label, content, created_at) in &events_buf {
                                    let _ = store
                                        .insert_event(
                                            message_id,
                                            *seq,
                                            kind,
                                            label.as_deref(),
                                            content,
                                            *created_at,
                                        )
                                        .await;
                                }
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
                        {
                            if let Ok(message_id) = store
                                .insert_message(
                                    &thread,
                                    "assistant",
                                    &msg,
                                    None,
                                    "error",
                                    now_unix(),
                                )
                                .await
                            {
                                for (seq, kind, label, content, created_at) in &events_buf {
                                    let _ = store
                                        .insert_event(
                                            message_id,
                                            *seq,
                                            kind,
                                            label.as_deref(),
                                            content,
                                            *created_at,
                                        )
                                        .await;
                                }
                            }
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

pub(crate) async fn get_chat_messages<B>(
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
    let store = &state.history;
    let scope = selected_scope(&state.scope, params.scope.clone());
    let thread = match store.default_thread(&scope).await {
        Ok(thread) => thread,
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };

    let trimming_from_front = params.after.is_none();
    let result = if let Some(date) = params.date.as_deref() {
        match date_bounds_utc(date) {
            Ok((start, end)) => {
                store
                    .messages_for_date(&thread, start, end, limit + 1)
                    .await
            }
            Err(e) => return (StatusCode::BAD_REQUEST, e).into_response(),
        }
    } else if let Some(after) = params.after {
        store.messages_after(&thread, after, limit + 1).await
    } else if let Some(before) = params.before {
        store.messages_before(&thread, before, limit + 1).await
    } else {
        store.latest_messages(&thread, limit + 1).await
    };

    match result {
        Ok(mut messages) => {
            let has_more = messages.len() as i64 > limit;
            if has_more {
                if trimming_from_front {
                    messages.remove(0);
                } else {
                    messages.pop();
                }
            }
            let message_ids = messages.iter().map(|m| m.id).collect::<Vec<_>>();
            let attachments = match store.attachments_for_messages(&message_ids).await {
                Ok(attachments) => attachments,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            };
            let mut by_message: std::collections::HashMap<i64, Vec<ChatAttachmentResponse>> =
                std::collections::HashMap::new();
            for attachment in attachments {
                by_message
                    .entry(attachment.message_id)
                    .or_default()
                    .push(attachment_response(attachment));
            }
            let latest_live_event_id = match store.latest_live_event_id(&scope).await {
                Ok(val) => val,
                Err(e) => {
                    return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
                }
            };
            let messages = messages
                .into_iter()
                .map(|message| ChatMessageResponse {
                    attachments: by_message.remove(&message.id).unwrap_or_default(),
                    message,
                })
                .collect();
            Json(ChatMessagesResponse {
                messages,
                has_more,
                latest_live_event_id,
            })
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub(crate) async fn get_attachment<B>(
    State(state): State<AppState<B>>,
    Path(id): Path<i64>,
    Query(params): Query<AttachmentQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    attachment_file_response(state, id, params, false).await
}

pub(crate) async fn get_attachment_preview<B>(
    State(state): State<AppState<B>>,
    Path(id): Path<i64>,
    Query(params): Query<AttachmentQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    attachment_file_response(state, id, params, true).await
}

async fn attachment_file_response<B>(
    state: AppState<B>,
    id: i64,
    params: AttachmentQuery,
    preview: bool,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::body::Body;
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let store = &state.history;
    let attachment = match store.attachment(id).await {
        Ok(Some(attachment)) => attachment,
        Ok(None) => return StatusCode::NOT_FOUND.into_response(),
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    };
    if params
        .scope
        .as_deref()
        .is_some_and(|scope| attachment.scope != scope)
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let mime = attachment
        .mime_type
        .clone()
        .unwrap_or_else(|| "application/octet-stream".to_string());
    if preview && !mime.starts_with("image/") {
        return StatusCode::NOT_FOUND.into_response();
    }
    let path = match wukong_chat_history::resolve_under_upload_root(
        &upload_root(),
        &attachment.relative_path,
    ) {
        Some(path) => path,
        None => return StatusCode::NOT_FOUND.into_response(),
    };
    let bytes = match tokio::fs::read(path).await {
        Ok(bytes) => bytes,
        Err(_) => return StatusCode::NOT_FOUND.into_response(),
    };

    let mut builder = axum::response::Response::builder().header(header::CONTENT_TYPE, mime);
    if !preview {
        builder = builder.header(
            header::CONTENT_DISPOSITION,
            content_disposition_name(&attachment.original_name),
        );
    }
    builder
        .body(Body::from(bytes))
        .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
}

/// `GET /api/chat/messages/:id/steps` — the helper-baton steps for one assistant
/// message, lazily fetched when the user expands the collapsible card.
pub(crate) async fn get_chat_steps<B>(
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
    let store = &state.history;
    match store.list_steps(message_id).await {
        Ok(steps) => Json(steps).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// `GET /api/chat/messages/:id/events` — raw turn stream events for one
/// assistant message, lazily fetched for the reasoning/tool history expander.
pub(crate) async fn get_chat_events<B>(
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
    let store = &state.history;
    match store.list_events(message_id).await {
        Ok(events) => Json(events).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub(crate) async fn get_chat_scopes<B>(
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

    let store = &state.history;
    match store.list_scopes(&state.scope).await {
        Ok(scopes) => Json(scopes).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub(crate) async fn stream_chat_events<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<ChatStreamQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }

    let scope = match params
        .scope
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
    {
        Some(scope) => scope,
        None => return (StatusCode::BAD_REQUEST, "missing scope").into_response(),
    };
    let store = state.history.clone();
    let mut cursor = params.after.unwrap_or(0).max(0);
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Event>();

    tokio::spawn(async move {
        let mut idle_ticks = 0;
        loop {
            match store.live_events_after(&scope, cursor, 50).await {
                Ok(events) => {
                    if events.is_empty() {
                        idle_ticks += 1;
                    } else {
                        idle_ticks = 0;
                    }
                    for event in events {
                        cursor = event.id;
                        if tx.send(live_event_to_sse(event)).is_err() {
                            return;
                        }
                    }
                }
                Err(e) => {
                    let _ = tx.send(Event::default().event("error").data(e.to_string()));
                    return;
                }
            }

            if idle_ticks >= 2 && cfg!(test) {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });

    let stream = UnboundedReceiverStream::new(rx).map(Ok::<Event, Infallible>);
    Sse::new(stream).into_response()
}
