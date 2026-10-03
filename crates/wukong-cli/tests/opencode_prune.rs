//! `wukong opencode prune` 的黑箱測試：執行真正的 binary，對一個假 opencode server。
//!
//! 輸出的第一行與結束碼都寫在 `main` 裡，是腳本與操作者實際依賴的介面，單元測試
//! 碰不到。

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::{Arc, Mutex};
use wukong_memory::Memory;

const DAY_MS: u128 = 24 * 60 * 60 * 1000;

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
}

/// 列表回傳 `sessions`，刪除以 `delete_status` 回應，其餘 404。
fn fake_server(sessions: String, delete_status: &'static str) -> (String, Arc<Mutex<Vec<String>>>) {
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
            let (status, body) = if request.starts_with("GET /session?") {
                ("200 OK", sessions.clone())
            } else if request.starts_with("DELETE /session/") {
                (delete_status, "true".to_string())
            } else {
                ("404 Not Found", String::new())
            };
            seen.lock().unwrap().push(request);
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes());
        }
    });
    (url, hits)
}

struct Outcome {
    code: i32,
    stdout: String,
    deletes: Vec<String>,
    db_url: String,
}

/// 以 30 天保留期執行 `wukong opencode prune`。`pointed_at` 是記憶庫裡某個 scope 指向
/// 的 session；server 上有一個 40 天前的無主 session 與一個 40 天前的受保護 session。
async fn prune(
    pointed_at: Option<&str>,
    sessions: Option<String>,
    delete_status: &'static str,
) -> Outcome {
    let dir = tempfile::tempdir().unwrap();
    let db_url = format!("sqlite://{}", dir.path().join("memory.db").display());
    let memory = Memory::open(&db_url).await.unwrap();
    if let Some(session_id) = pointed_at {
        memory
            .set_agent_session("user:tg-1", session_id)
            .await
            .unwrap();
    }
    drop(memory);
    let old = now_ms() - 40 * DAY_MS;
    let sessions = sessions.unwrap_or_else(|| {
        format!(
            r#"[{{"id":"ses_orphan","time":{{"updated":{old}}}}},{{"id":"ses_kept","time":{{"updated":{old}}}}}]"#
        )
    });
    let (server_url, hits) = fake_server(sessions, delete_status);

    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wukong"))
        .args(["opencode", "prune"])
        .current_dir(dir.path())
        .env("HOME", dir.path())
        .env("WUKONG_MEMORY_DB", &db_url)
        .env("WUKONG_AGENT_SERVER_URL", &server_url)
        .env("WUKONG_SETTINGS_FILE", dir.path().join("settings.json"))
        .env("WUKONG_OPENCODE_SESSION_RETENTION_DAYS", "30")
        .env_remove("WUKONG_AGENT_SERVER_USERNAME")
        .env_remove("WUKONG_AGENT_SERVER_PASSWORD")
        .env_remove("WUKONG_EMBED")
        .env_remove("WUKONG_MD_DIR")
        .output()
        .unwrap();

    let deletes = hits
        .lock()
        .unwrap()
        .iter()
        .filter(|request| request.starts_with("DELETE "))
        .cloned()
        .collect();
    Outcome {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        deletes,
        db_url,
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn a_clean_run_names_its_memory_database_and_exits_zero() {
    let outcome = prune(Some("ses_kept"), None, "200 OK").await;

    assert_eq!(
        outcome.stdout.lines().next(),
        Some(format!("記憶庫：{}", outcome.db_url).as_str()),
        "{}",
        outcome.stdout
    );
    assert_eq!(outcome.deletes, ["DELETE /session/ses_orphan"]);
    assert_eq!(outcome.code, 0, "{}", outcome.stdout);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_failed_delete_exits_nonzero() {
    let outcome = prune(Some("ses_kept"), None, "500 Internal Server Error").await;

    assert_eq!(outcome.deletes, ["DELETE /session/ses_orphan"]);
    assert!(
        outcome.stdout.contains("刪除失敗 1 個"),
        "{}",
        outcome.stdout
    );
    assert_eq!(outcome.code, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_memory_database_that_matches_nothing_refuses_and_exits_nonzero() {
    let outcome = prune(Some("ses_from_another_deployment"), None, "200 OK").await;

    assert_eq!(outcome.deletes, Vec::<String>::new());
    assert!(
        outcome.stdout.contains(&outcome.db_url),
        "{}",
        outcome.stdout
    );
    assert!(outcome.stdout.contains("未清理"), "{}", outcome.stdout);
    assert_eq!(outcome.code, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn a_memory_with_pointers_against_an_empty_server_refuses() {
    let outcome = prune(
        Some("ses_that_used_to_exist"),
        Some("[]".to_string()),
        "200 OK",
    )
    .await;

    assert_eq!(outcome.deletes, Vec::<String>::new());
    assert!(outcome.stdout.contains("未清理"), "{}", outcome.stdout);
    assert_eq!(outcome.code, 1);
}

#[tokio::test(flavor = "multi_thread")]
async fn an_empty_server_is_nothing_to_do_not_a_failure() {
    let outcome = prune(None, Some("[]".to_string()), "200 OK").await;

    assert_eq!(outcome.deletes, Vec::<String>::new());
    assert!(outcome.stdout.contains("共列出 0 個"), "{}", outcome.stdout);
    assert_eq!(outcome.code, 0, "{}", outcome.stdout);
}
