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

// #45 SCN-002：真實的 V2 server 派工給 fixer 時，SSE 依序送出 role fixer、fixer 的產出卡片與回覆。
#[tokio::test]
#[ignore = "requires a resident OpenCode V2 server and a real model"]
async fn web_streams_the_dispatched_subagent() {
    let (dir, backend, cfg) = support::server_fixture("project:issue45-web");
    let workspace = std::env::var("WUKONG_WORKSPACE").expect("set WUKONG_WORKSPACE");
    wukong_runtime::agent_assets::materialize_agent_definitions(std::path::Path::new(&workspace))
        .unwrap();
    // Web 每回合從 AppState 的設定檔取模型
    let settings_path = dir.path().join("settings.json");
    let model = cfg.default_model.clone().unwrap();
    std::fs::write(
        &settings_path,
        serde_json::json!({"agent": {"default_model": model}}).to_string(),
    )
    .unwrap();
    let memory = Arc::new(Memory::open(&cfg.db_url).await.unwrap());
    let app = build_router(AppState {
        memory,
        backend: Arc::new(backend),
        scope: cfg.scope.clone(),
        history: ChatHistoryStore::open(&cfg.db_url).await.unwrap(),
        db_url: cfg.db_url,
        token: None,
        settings_path,
    });
    let query: String = url_escape(support::DISPATCH_PROMPT);
    let response = app
        .oneshot(
            Request::builder()
                .uri(format!("/chat?q={query}"))
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let mut body = response.into_body().into_data_stream();
    let mut received = String::new();
    while let Some(frame) = tokio::time::timeout(std::time::Duration::from_secs(300), body.next())
        .await
        .unwrap()
    {
        received.push_str(std::str::from_utf8(&frame.unwrap()).unwrap());
    }
    println!("{received}");
    let role_at = received
        .find("event: role\ndata: fixer")
        .expect("role event");
    let step_at = received.find("event: step").expect("step event");
    let answer_at = received.find("event: answer").expect("answer event");
    assert!(role_at < step_at && step_at < answer_at, "{received}");
    assert!(received.contains(r#""role":"fixer""#), "{received}");
}

fn url_escape(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            _ => format!("%{b:02X}"),
        })
        .collect()
}
