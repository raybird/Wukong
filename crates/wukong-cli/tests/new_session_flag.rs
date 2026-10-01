//! `wukong --new` 的黑箱測試：執行真正的 binary，對一個會記錄請求的假 opencode server。
//!
//! `--new` 的處理寫在 `main` 裡，單元測試碰不到；而它漏掉的正是一個對外的請求
//! （刪除舊 session），所以直接看 server 收到了什麼。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use wukong_memory::Memory;

/// 只認得刪除 session，其餘一律 404——`--new` 之後的回合因此在第一個請求就失敗
/// 收場，測試不需要模擬完整的一回合。
fn recording_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let hits = Arc::new(Mutex::new(Vec::new()));
    let seen = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut socket) = stream else { break };
            let mut buf = Vec::new();
            let mut tmp = [0u8; 1024];
            while !buf.windows(4).any(|w| w == b"\r\n\r\n") {
                match socket.read(&mut tmp) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => buf.extend_from_slice(&tmp[..n]),
                }
            }
            let head = String::from_utf8_lossy(&buf);
            let mut words = head.split_whitespace();
            let request = format!(
                "{} {}",
                words.next().unwrap_or_default(),
                words.next().unwrap_or_default()
            );
            let response = if request.starts_with("DELETE /session/") {
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: 4\r\nConnection: close\r\n\r\ntrue"
            } else {
                "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
            };
            seen.lock().unwrap().push(request);
            let _ = socket.write_all(response.as_bytes());
        }
    });
    (url, hits)
}

#[tokio::test(flavor = "multi_thread")]
async fn new_flag_deletes_the_session_the_scope_pointed_at() {
    let dir = tempfile::tempdir().unwrap();
    let db_url = format!("sqlite://{}", dir.path().join("memory.db").display());
    Memory::open(&db_url)
        .await
        .unwrap()
        .set_agent_session("project:T", "ses_old")
        .await
        .unwrap();
    let (server_url, hits) = recording_server();

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wukong"))
        .args(["--new", "--scope", "project:T", "--db", &db_url, "hi"])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("WUKONG_AGENT_SERVER_URL", &server_url)
        .env("WUKONG_SETTINGS_FILE", dir.path().join("settings.json"))
        .env_remove("WUKONG_AGENT_SERVER_USERNAME")
        .env_remove("WUKONG_AGENT_SERVER_PASSWORD")
        .env_remove("WUKONG_EMBED")
        .env_remove("WUKONG_MD_DIR")
        .output()
        .unwrap();

    let seen = hits.lock().unwrap().clone();
    assert!(
        seen.iter()
            .any(|request| request == "DELETE /session/ses_old"),
        "the old session was never deleted; server saw {seen:?}\nstderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let memory = Memory::open(&db_url).await.unwrap();
    assert_eq!(memory.agent_session("project:T").await.unwrap(), None);
}
