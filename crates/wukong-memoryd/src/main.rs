use std::sync::Arc;
use wukong_memory::Memory;
use wukong_memoryd::{build_router, Config};

#[tokio::main]
async fn main() {
    let config = Config::from_env();
    let memory = Memory::open(&config.db_url)
        .await
        .expect("failed to open memory database");
    let app = build_router(Arc::new(memory));

    let addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .expect("failed to bind");
    println!("wukong-memoryd listening on {addr}");
    axum::serve(listener, app).await.expect("server error");
}
