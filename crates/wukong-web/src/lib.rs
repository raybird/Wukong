//! wukong-web: a zero-build browser console for Wukong. Reuses run_turn and
//! streams role progress + the rendered answer over SSE.

use std::sync::Arc;
use wukong_gateway::backend::AiBackend;
use wukong_memory::Memory;

/// Shared router state. Generic over the backend so tests inject a mock.
pub struct AppState<B: AiBackend> {
    pub memory: Arc<Memory>,
    pub backend: Arc<B>,
    pub scope: String,
    pub token: Option<String>,
}

// Manual Clone: Arc fields clone cheaply and B need not be Clone.
impl<B: AiBackend> Clone for AppState<B> {
    fn clone(&self) -> Self {
        Self {
            memory: self.memory.clone(),
            backend: self.backend.clone(),
            scope: self.scope.clone(),
            token: self.token.clone(),
        }
    }
}

const INDEX_HTML: &str = include_str!("../static/index.html");

const APP_JS: &str = include_str!("../static/app.js");
const HTML_JS: &str = include_str!("../static/lib/html.js");
const CHAT_JS: &str = include_str!("../static/components/wukong-chat.js");
const STYLES_CSS: &str = include_str!("../static/styles.css");

const JS: &str = "application/javascript";
const CSS: &str = "text/css";

/// Serve the SPA shell at `/`.
async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(INDEX_HTML)
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
async fn styles_css() -> axum::response::Response { asset(CSS, STYLES_CSS) }

/// Build the application router from shared state.
pub fn build_router<B>(state: AppState<B>) -> axum::Router
where
    B: AiBackend + Send + Sync + 'static,
{
    axum::Router::new()
        .route("/", axum::routing::get(index))
        .route("/app.js", axum::routing::get(app_js))
        .route("/lib/html.js", axum::routing::get(html_js))
        .route("/components/wukong-chat.js", axum::routing::get(chat_js))
        .route("/styles.css", axum::routing::get(styles_css))
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
            Ok(AgentResponse { text: self.replies.lock().unwrap().pop_front().unwrap_or_default() })
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
        assert!(content_type(build_router(state(None, &[]).await), "/styles.css")
            .await
            .contains("css"));
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
}
