#[path = "../../../scripts/test-support/managed_actual.rs"]
mod support;
use axum::body::Body;
use axum::http::Request;
use std::sync::Arc;
use tokio_stream::StreamExt;
use tower::ServiceExt;
use wukong_chat_history::ChatHistoryStore;
use wukong_memory::Memory;
use wukong_web::{build_router, AppState};

// SCN-010：真正 HTTP／SSE question → HTTP reply → 真實 OpenCode 繼續。
#[tokio::test]
#[ignore = "requires isolated OpenCode and local model fixture"]
async fn web_managed_question_can_be_answered_over_http() {
    let (dir, backend, cfg) = support::fixture("project:issue7-managed-web");
    let memory = Arc::new(Memory::open(&cfg.db_url).await.unwrap());
    let app = build_router(AppState {
        memory: memory.clone(),
        backend: Arc::new(backend),
        scope: cfg.scope.clone(),
        history: ChatHistoryStore::open(&cfg.db_url).await.unwrap(),
        db_url: cfg.db_url,
        token: None,
        settings_path: dir.path().join("settings.toml"),
    });
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/chat?q=QUESTION_PROBE")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body().into_data_stream();
    let mut received = String::new();
    let mut answered = false;
    while let Some(frame) = tokio::time::timeout(std::time::Duration::from_secs(20), body.next())
        .await
        .unwrap()
    {
        let data = String::from_utf8(frame.unwrap().to_vec()).unwrap();
        if data.contains("event: question") && !answered {
            let json = data
                .lines()
                .find_map(|line| line.strip_prefix("data: "))
                .unwrap();
            let question: serde_json::Value = serde_json::from_str(json).unwrap();
            let request_id = question["request_id"].as_str().unwrap();
            let reply =
                serde_json::json!({"session_id": question["session_id"], "answers": [["B"]]});
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("POST")
                        .uri(format!("/api/questions/{request_id}/reply"))
                        .header("Content-Type", "application/json")
                        .body(Body::from(reply.to_string()))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(response.status(), 200);
            answered = true;
        }
        received.push_str(&data);
    }
    assert!(answered, "{received}");
    assert!(
        received.contains("User has answered your questions"),
        "{received}"
    );
    assert!(received.contains("event: done"), "{received}");
    assert!(!received.contains("event: error"), "{received}");
    assert!(memory.agent_session(&cfg.scope).await.unwrap().is_some());
}
