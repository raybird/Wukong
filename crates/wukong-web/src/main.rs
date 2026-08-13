use std::sync::Arc;
use wukong_gateway::backend::build_backend_from_env;
use wukong_gateway::workspace_dir;
use wukong_web::{build_router, AppState};

#[tokio::main]
async fn main() {
    let host = std::env::var("WUKONG_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("WUKONG_WEB_PORT").unwrap_or_else(|_| "8787".to_string());
    let scope = std::env::var("WUKONG_WEB_SCOPE").unwrap_or_else(|_| "global".to_string());
    let token = std::env::var("WUKONG_WEB_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());

    // Fail closed: refuse to expose an unauthenticated console on a non-loopback
    // interface. The console can trigger agent turns, read memory, and edit
    // settings, so a public bind with no token is a footgun.
    let allow_insecure = std::env::var("WUKONG_WEB_ALLOW_INSECURE").as_deref() == Ok("1");
    if wukong_web::should_refuse_insecure_start(token.as_deref(), &host, allow_insecure) {
        eprintln!(
            "error: refusing to start — WUKONG_WEB_TOKEN is empty while binding to a \
             non-loopback host ({host})."
        );
        eprintln!(
            "       Set WUKONG_WEB_TOKEN=<secret> to require auth, or \
             WUKONG_WEB_ALLOW_INSECURE=1 to override."
        );
        // Don't exit(1): under `restart: unless-stopped` that becomes an
        // invisible crash loop (users just see connection refused). Bind the
        // same address and serve a 503 page that explains the fix; /healthz
        // returns 503 too, so the container healthcheck reports unhealthy.
        serve_misconfigured(&host, &port).await;
        return;
    }

    let db_url = wukong_runtime::util::db_url_from_env();
    let memory = match wukong_runtime::bootstrap::open_memory_from_env(&db_url).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: failed to open memory: {e}");
            std::process::exit(1);
        }
    };

    // Opened once and shared by every handler: `open` applies the full schema,
    // which is far too much work to repeat per request.
    let history = match wukong_chat_history::ChatHistoryStore::open(&db_url).await {
        Ok(h) => h,
        Err(e) => {
            eprintln!("error: failed to open chat history: {e}");
            std::process::exit(1);
        }
    };

    let agent_command = wukong_runtime::util::agent_command_from_env();
    let backend = build_backend_from_env(agent_command, workspace_dir());

    let state = AppState {
        memory: Arc::new(memory),
        backend: Arc::new(backend),
        scope,
        db_url,
        history,
        token,
        settings_path: wukong_settings::default_settings_path(),
    };
    let app = build_router(state);

    let addr = format!("{host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("🐵 wukong-web 上線 http://{addr}/");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}

/// Bind `host:port` and serve the fail-closed 503 page instead of exiting, so a
/// misconfigured deployment stays visible (browser shows the fix, healthcheck
/// reports unhealthy) rather than crash-looping under `restart: unless-stopped`.
async fn serve_misconfigured(host: &str, port: &str) {
    let addr = format!("{host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("🐵 wukong-web 以「設定錯誤」降級模式上線 http://{addr}/（所有請求回 503）");
    let app = wukong_web::build_misconfigured_router();
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}
