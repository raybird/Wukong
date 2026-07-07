use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::sync::Arc;
use tempfile::NamedTempFile;
use tower::ServiceExt; // for `oneshot`
use wukong_memory::Memory;
use wukong_memoryd::build_router;

async fn test_app() -> axum::Router {
    build_app(None).await
}

async fn build_app(token: Option<String>) -> axum::Router {
    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);
    let memory = Memory::open(&url).await.unwrap();
    build_router(Arc::new(memory), token)
}

async fn body_json(resp: axum::response::Response) -> serde_json::Value {
    let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap()
}

#[tokio::test]
async fn health_returns_ok() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["status"], "ok");
}

#[tokio::test]
async fn protected_route_rejects_missing_token() {
    let app = build_app(Some("s3cret".to_string())).await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_rejects_wrong_token() {
    let app = build_app(Some("s3cret".to_string())).await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/stats")
                .header("authorization", "Bearer nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn protected_route_accepts_correct_token() {
    let app = build_app(Some("s3cret".to_string())).await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/stats")
                .header("authorization", "Bearer s3cret")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn health_stays_open_with_token_configured() {
    let app = build_app(Some("s3cret".to_string())).await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
}

#[tokio::test]
async fn remember_then_recall_over_http() {
    let app = test_app().await;

    let remember_req = Request::builder()
        .method("POST")
        .uri("/v1/remember")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"scope":"global","items":[{"kind":"note","text":"axum powers the http layer"}]}"#,
        ))
        .unwrap();
    let resp = app.clone().oneshot(remember_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["data"].as_array().unwrap().len(), 1);

    let recall_req = Request::builder()
        .method("POST")
        .uri("/v1/recall")
        .header("content-type", "application/json")
        .body(Body::from(r#"{"query":"http layer"}"#))
        .unwrap();
    let resp = app.oneshot(recall_req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(!json["data"].as_array().unwrap().is_empty());
    assert!(json["latency_ms"].is_number());
}

#[tokio::test]
async fn malformed_remember_payload_returns_400() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remember")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"scope":"global","items":"bad"}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn unknown_memory_kind_returns_400() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remember")
                .header("content-type", "application/json")
                .body(Body::from(
                    r#"{"scope":"global","items":[{"kind":"unknown","text":"x"}]}"#,
                ))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn empty_items_returns_400() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/remember")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"scope":"global","items":[]}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn blank_recall_query_returns_400() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/v1/recall")
                .header("content-type", "application/json")
                .body(Body::from(r#"{"query":"   "}"#))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn invalid_scope_returns_400() {
    let app = test_app().await;
    let req = Request::builder()
        .method("POST")
        .uri("/v1/remember")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"scope":"bogus","items":[{"kind":"note","text":"x"}]}"#,
        ))
        .unwrap();
    let resp = app.oneshot(req).await.unwrap();
    assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn snapshot_endpoint_returns_json() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/snapshot")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert!(json.get("total").is_some());
    assert!(json.get("by_kind").is_some());
}

#[tokio::test]
async fn stats_returns_totals() {
    let app = test_app().await;
    let resp = app
        .oneshot(
            Request::builder()
                .uri("/v1/stats")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let json = body_json(resp).await;
    assert_eq!(json["total"], 0);
}
