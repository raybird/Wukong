//! 執行真正的 `wukong-schedulerd`，看它的定期清理實際做了什麼、寫了什麼。
//!
//! 清理的內容由 `wukong-runtime` 的測試釘住；這裡釘的是 daemon 這一段接線——迴圈
//! 真的呼叫了清理、結果真的寫進日誌、傳進去的真的是它在用的那份記憶庫。這三件事
//! 拿掉任何一件，函式層級的測試都還是綠的。

use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use wukong_memory::Memory;

const DAY_MS: u128 = 24 * 60 * 60 * 1000;

/// 一個 40 天前的無主 session 與一個 40 天前、名為 `ses_kept` 的 session。
fn fake_server() -> (String, Arc<Mutex<Vec<String>>>) {
    let old = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis()
        - 40 * DAY_MS;
    let sessions = format!(
        r#"[{{"id":"ses_orphan","time":{{"updated":{old}}}}},{{"id":"ses_kept","time":{{"updated":{old}}}}}]"#
    );
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
                ("200 OK", "true".to_string())
            } else if request == "GET /global/health" {
                ("200 OK", r#"{"healthy":true}"#.to_string())
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

struct Daemon {
    child: Child,
    stderr: Arc<Mutex<String>>,
    hits: Arc<Mutex<Vec<String>>>,
    db_url: String,
    _dir: tempfile::TempDir,
}

impl Daemon {
    /// 啟動 schedulerd，清理間隔縮成 1 秒；記憶庫裡有一個 scope 指向 `pointed_at`。
    async fn start(pointed_at: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let db_url = format!("sqlite://{}", dir.path().join("memory.db").display());
        Memory::open(&db_url)
            .await
            .unwrap()
            .set_agent_session("user:tg-1", pointed_at)
            .await
            .unwrap();
        let (server_url, hits) = fake_server();

        let mut child = Command::new(env!("CARGO_BIN_EXE_wukong-schedulerd"))
            .current_dir(dir.path())
            .env("HOME", dir.path())
            .env("WUKONG_MEMORY_DB", &db_url)
            .env("WUKONG_AGENT_SERVER_URL", &server_url)
            .env("WUKONG_SETTINGS_FILE", dir.path().join("settings.json"))
            .env("WUKONG_OPENCODE_SESSION_RETENTION_DAYS", "30")
            .env("WUKONG_TEST_OPENCODE_RETENTION_INTERVAL_SECS", "1")
            .env("WUKONG_SCHED_NOTIFY", "0")
            .env_remove("WUKONG_TG_TOKEN")
            .env_remove("WUKONG_AGENT_SERVER_USERNAME")
            .env_remove("WUKONG_AGENT_SERVER_PASSWORD")
            .env_remove("WUKONG_EMBED")
            .env_remove("WUKONG_MD_DIR")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let stderr = Arc::new(Mutex::new(String::new()));
        let sink = stderr.clone();
        let pipe = child.stderr.take().unwrap();
        std::thread::spawn(move || {
            for line in BufReader::new(pipe).lines().map_while(Result::ok) {
                let mut log = sink.lock().unwrap();
                log.push_str(&line);
                log.push('\n');
            }
        });
        Self {
            child,
            stderr,
            hits,
            db_url,
            _dir: dir,
        }
    }

    fn log(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }

    fn deletes(&self) -> Vec<String> {
        self.hits
            .lock()
            .unwrap()
            .iter()
            .filter(|request| request.starts_with("DELETE "))
            .cloned()
            .collect()
    }

    /// 等到日誌出現 `needle`；等不到就帶著目前的日誌失敗。
    fn wait_for_log(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(30);
        while !self.log().contains(needle) {
            assert!(
                Instant::now() < deadline,
                "the daemon never logged {needle:?}; its log so far:\n{}",
                self.log()
            );
            std::thread::sleep(Duration::from_millis(50));
        }
    }
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_runs_the_cleanup_and_logs_what_it_did() {
    let daemon = Daemon::start("ses_kept").await;

    daemon.wait_for_log("opencode_session_retention ");

    let log = daemon.log();
    assert!(log.contains("anchored=true"), "{log}");
    assert!(log.contains("deleted=1"), "{log}");
    let deletes = daemon.deletes();
    assert!(
        deletes.contains(&"DELETE /session/ses_orphan".to_string()),
        "{deletes:?}"
    );
    assert!(
        !deletes.contains(&"DELETE /session/ses_kept".to_string()),
        "{deletes:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn the_daemon_explains_a_memory_database_that_does_not_match() {
    let daemon = Daemon::start("ses_from_another_deployment").await;

    daemon.wait_for_log("warning: opencode session retention skipped");

    let log = daemon.log();
    assert!(log.contains("anchored=false"), "{log}");
    assert!(
        log.contains(&daemon.db_url),
        "the warning does not name the memory database {}:\n{log}",
        daemon.db_url
    );
    assert_eq!(daemon.deletes(), Vec::<String>::new());
}
