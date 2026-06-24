use std::sync::Arc;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::workspace_dir;
use wukong_memory::Memory;
use wukong_web::{build_router, AppState};

#[tokio::main]
async fn main() {
    let host = std::env::var("WUKONG_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("WUKONG_WEB_PORT").unwrap_or_else(|_| "8787".to_string());
    let scope = std::env::var("WUKONG_WEB_SCOPE").unwrap_or_else(|_| "global".to_string());
    let token = std::env::var("WUKONG_WEB_TOKEN")
        .ok()
        .filter(|t| !t.is_empty());

    let db_url = std::env::var("WUKONG_MEMORY_DB").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let dir = format!("{home}/.wukong");
        let _ = std::fs::create_dir_all(&dir);
        format!("sqlite://{dir}/memory.db")
    });
    let memory = match Memory::open(&db_url).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: failed to open memory: {e}");
            std::process::exit(1);
        }
    };

    #[cfg(feature = "embed")]
    let memory = if std::env::var("WUKONG_EMBED").as_deref() == Ok("1") {
        match wukong_memory::FastembedBackend::new() {
            Ok(b) => memory.with_embedder(Arc::new(b)),
            Err(e) => {
                eprintln!("🐵 語意層停用（模型載入失敗）：{e}");
                memory
            }
        }
    } else {
        memory
    };

    let memory = match std::env::var("WUKONG_MD_DIR") {
        Ok(dir) if !dir.is_empty() => memory.with_markdown(dir),
        _ => memory,
    };

    let agent_command = std::env::var("WUKONG_AGENT_CMD")
        .ok()
        .map(|s| {
            s.split_whitespace()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]);
    let backend = AgentCliBackend {
        command: agent_command,
        workspace: workspace_dir(),
    };

    let state = AppState {
        memory: Arc::new(memory),
        backend: Arc::new(backend),
        scope,
        db_url,
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
