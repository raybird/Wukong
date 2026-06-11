//! wukong-web: a zero-build browser console for Wukong. Reuses run_turn and
//! streams role progress + the rendered answer over SSE.

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, Sse};
use axum::Json;
use std::convert::Infallible;
use std::sync::Arc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use wukong_cli::run_turn;
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_memory::Memory;
use wukong_settings::{Settings, TelegramSettings};

/// Shared router state. Generic over the backend so tests inject a mock.
pub struct AppState<B: AiBackend> {
    pub memory: Arc<Memory>,
    pub backend: Arc<B>,
    pub scope: String,
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
            token: self.token.clone(),
            settings_path: self.settings_path.clone(),
        }
    }
}

const INDEX_HTML: &str = include_str!("../static/index.html");

const APP_JS: &str = include_str!("../static/app.js");
const HTML_JS: &str = include_str!("../static/lib/html.js");
const CHAT_JS: &str = include_str!("../static/components/wukong-chat.js");
const SETTINGS_JS: &str = include_str!("../static/components/wukong-settings.js");
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

async fn app_js() -> axum::response::Response { asset(JS, APP_JS) }
async fn html_js() -> axum::response::Response { asset(JS, HTML_JS) }
async fn chat_js() -> axum::response::Response { asset(JS, CHAT_JS) }
async fn settings_js() -> axum::response::Response { asset(JS, SETTINGS_JS) }
async fn styles_css() -> axum::response::Response { asset(CSS, STYLES_CSS) }

/// Messages pushed from the turn task to the SSE stream.
enum SseMsg {
    Role(String),
    Reasoning(String),
    Answer(String),
    Error(String),
    Done,
}

impl SseMsg {
    fn into_event(self) -> Event {
        match self {
            SseMsg::Role(r) => Event::default().event("role").data(r),
            SseMsg::Reasoning(t) => Event::default().event("reasoning").data(t),
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

fn authorized(expected: &Option<String>, provided: Option<&str>) -> bool {
    match expected {
        Some(t) => provided == Some(t.as_str()),
        None => true,
    }
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
        let mem = state.memory.clone();
        let backend = state.backend.clone();
        let scope = state.scope.clone();
        // run_turn's future is not Send (AiBackend uses async_fn_in_trait and the
        // callbacks are dyn FnMut), so it can't ride tokio::spawn or the axum
        // handler future. Drive it on a dedicated thread with its own
        // current-thread runtime; only the Send channel crosses back.
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
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
                    thinking: true,
                    recall_top_k: 5,
                    stream: false,
                };
                // Leading-slash inputs are session commands, not turns.
                let trimmed = q.trim();
                if let Some(rest) = trimmed.strip_prefix('/') {
                    let name = rest.split_whitespace().next().unwrap_or("").to_string();
                    let reply = match wukong_cli::parse_session_command(&name) {
                        Some(cmd) => match wukong_cli::run_session_command(mem.as_ref(), backend.as_ref(), &cfg, cmd).await {
                            Ok(t) => t,
                            Err(e) => format!("⚠️ 失敗：{e}"),
                        },
                        None => format!("指令 /{name} 尚未支援"),
                    };
                    let _ = tx.send(SseMsg::Answer(wukong_render::to_web_html(&reply)));
                    let _ = tx.send(SseMsg::Done);
                    return;
                }

                let role_tx = tx.clone();
                let ev_tx = tx.clone();
                let result = run_turn(
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
                )
                .await;
                match result {
                    Ok(out) => {
                        let _ = tx.send(SseMsg::Answer(wukong_render::to_web_html(&out.text)));
                    }
                    Err(e) => {
                        let _ = tx.send(SseMsg::Error(e.to_string()));
                    }
                }
                let _ = tx.send(SseMsg::Done);
            });
        });
    }

    let stream = UnboundedReceiverStream::new(rx)
        .map(|m| Ok::<Event, Infallible>(m.into_event()));
    Sse::new(stream).into_response()
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

    let settings = Settings { telegram: req.telegram };
    match wukong_settings::save_settings(&state.settings_path, &settings) {
        Ok(()) => StatusCode::OK.into_response(),
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
        .route("/components/wukong-settings.js", axum::routing::get(settings_js))
        .route("/styles.css", axum::routing::get(styles_css))
        .route("/settings", axum::routing::get(index::<B>))
        .route("/chat", axum::routing::get(chat::<B>))
        .route("/api/settings", axum::routing::get(get_settings::<B>).post(post_settings::<B>))
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
    }
    impl MockBackend {
        fn new(r: &[&str]) -> Self {
            Self { replies: Mutex::new(r.iter().map(|s| s.to_string()).collect()) }
        }
    }
    impl AiBackend for MockBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
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
            token: token.map(|s| s.to_string()),
            settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
        }
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
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
        assert!(content_type(build_router(state(None, &[]).await), "/app.js")
            .await
            .contains("javascript"));
        assert!(content_type(build_router(state(None, &[]).await), "/lib/html.js")
            .await
            .contains("javascript"));
        assert!(content_type(build_router(state(None, &[]).await), "/components/wukong-chat.js")
            .await
            .contains("javascript"));
        assert!(content_type(build_router(state(None, &[]).await), "/components/wukong-settings.js")
            .await
            .contains("javascript"));
        assert!(content_type(build_router(state(None, &[]).await), "/styles.css")
            .await
            .contains("css"));
    }

    #[tokio::test]
    async fn chat_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &["oracle", "ans"]).await);
        let resp = app
            .oneshot(Request::builder().uri("/chat?q=hi").body(Body::empty()).unwrap())
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
        assert!(body.contains(r#"window.WUKONG_TOKEN = "sekret""#), "token not injected:\n{body}");
    }

    #[tokio::test]
    async fn chat_new_command_clears_session() {
        let app_state = state(None, &[]).await;
        app_state.memory.set_agent_session("global", "ses_1").await.unwrap();
        let app = build_router(app_state.clone());
        let resp = app
            .oneshot(Request::builder().uri("/chat?q=/new").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("event: answer"), "missing answer:\n{body}");
        assert!(body.contains("已開新"), "missing reply:\n{body}");
        assert!(!body.contains("event: role"), "should not run a turn:\n{body}");
        assert!(body.contains("event: done"));
        assert_eq!(app_state.memory.agent_session("global").await.unwrap(), None);
    }

    struct ReasoningBackend {
        reasoning: &'static str,
    }
    impl AiBackend for ReasoningBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse { text: "答案".to_string(), session_id: None })
        }
        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::Reasoning(self.reasoning.to_string()));
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
            token: None,
            settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
        }
    }

    #[tokio::test]
    async fn settings_get_returns_default_state() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(Request::builder().uri("/api/settings").body(Body::empty()).unwrap())
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
    async fn settings_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);

        let resp = app
            .oneshot(Request::builder().uri("/api/settings").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn chat_streams_reasoning_event() {
        let app = build_router(reasoning_state("想一下").await);
        let resp = app
            .oneshot(Request::builder().uri("/chat?q=hi").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_string(resp).await;
        assert!(body.contains("event: reasoning"), "missing reasoning event:\n{body}");
        assert!(body.contains("想一下"), "missing reasoning text:\n{body}");
    }

    #[tokio::test]
    async fn chat_skips_empty_reasoning() {
        let app = build_router(reasoning_state("").await);
        let resp = app
            .oneshot(Request::builder().uri("/chat?q=hi").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_string(resp).await;
        assert!(!body.contains("event: reasoning"), "empty reasoning should be skipped:\n{body}");
    }

    #[tokio::test]
    async fn chat_streams_role_answer_done() {
        // [0] planner -> "oracle" => single Oracle step; [1] execute -> markdown.
        let app = build_router(state(None, &["oracle", "**ans**"]).await);
        let resp = app
            .oneshot(Request::builder().uri("/chat?q=hi").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("event: role"), "missing role event:\n{body}");
        assert!(body.contains("event: answer"), "missing answer event:\n{body}");
        assert!(body.contains("<strong>ans</strong>"), "answer not rendered:\n{body}");
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
        assert!(body.contains("<wukong-chat>"));
    }

    #[tokio::test]
    async fn settings_route_serves_the_shell() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(Request::builder().uri("/settings").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("<wukong-settings>"));
    }
}
