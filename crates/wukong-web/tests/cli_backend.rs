#[path = "../../../scripts/test-support/cli_backend.rs"]
mod support;

use axum::body::Body;
use axum::http::Request;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tower::ServiceExt;
use wukong_chat_history::ChatHistoryStore;
use wukong_memory::Memory;
use wukong_web::{build_router, AppState};

// SCN-003：真實 Axum HTTP／SSE → run_turn → CLI subprocess。
#[tokio::test]
async fn web_sse_delivers_cli_tool_progress_before_process_finishes() {
    let (dir, backend, cfg) = support::fixture("project:issue7-web");
    let memory = Arc::new(Memory::open(&cfg.db_url).await.unwrap());
    let history = ChatHistoryStore::open(&cfg.db_url).await.unwrap();
    let app = build_router(AppState {
        memory: memory.clone(),
        backend: Arc::new(backend),
        scope: cfg.scope.clone(),
        db_url: cfg.db_url,
        history,
        token: None,
        settings_path: dir.path().join("settings.toml"),
    });
    let response = app
        .oneshot(
            Request::builder()
                .uri("/chat?q=integration%20probe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), 200);
    let mut body = response.into_body().into_data_stream();
    let mut received = String::new();
    let mut saw_live_tool = false;
    while let Some(frame) = tokio::time::timeout(std::time::Duration::from_secs(5), body.next())
        .await
        .unwrap()
    {
        let data = frame.unwrap();
        received.push_str(&String::from_utf8_lossy(&data));
        if received.contains("event: tool") && !received.contains("event: answer") {
            assert!(
                !dir.path().join("finished").exists(),
                "progress arrived after child finished"
            );
            saw_live_tool = true;
        }
    }
    assert!(saw_live_tool, "{received}");
    assert!(received.contains("CLI_INTEGRATION_OK"), "{received}");
    assert!(received.contains("event: done"), "{received}");
    assert!(!received.contains("event: error"), "{received}");
    assert!(memory.agent_session(&cfg.scope).await.unwrap().is_some());
}
