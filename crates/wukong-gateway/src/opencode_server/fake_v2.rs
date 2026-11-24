//! 測試用的假 OpenCode V2 server，只實作 Wukong 用到的 `/api` 端點。
//!
//! 行為照 2026-11-24 對 @opencode/cli 2.3.2 的實測（`docs/issues/issue-0044/probe-*.md`）：
//! `/api` 以外的路徑一律回網頁（200，text/html），`/api` 要帶 basic auth 且使用者名稱
//! 固定為 opencode，列表由新到舊、照 cursor 分頁，最後一頁之後還要再取一次空頁才沒有
//! `cursor.next`。

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(super) const PASSWORD: &str = "fake-v2-secret";
/// 真的 V2 不設上限；假 server 每頁最多回這麼多，確認呼叫端照 cursor 走，不是信任 limit。
pub(super) const PAGE_CAP: usize = 50;

#[derive(Default)]
struct State {
    /// 由舊到新。
    sessions: Vec<Value>,
    requests: Vec<String>,
    created: usize,
}

pub(super) struct FakeV2 {
    pub url: String,
    state: Arc<Mutex<State>>,
}

impl FakeV2 {
    pub async fn start() -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let state = Arc::new(Mutex::new(State::default()));
        let shared = state.clone();
        tokio::spawn(async move {
            while let Ok((mut socket, _)) = listener.accept().await {
                let state = shared.clone();
                tokio::spawn(async move {
                    let Some((method, path, auth)) = read_request(&mut socket).await else {
                        return;
                    };
                    let (status, content_type, body) = handle(&state, &method, &path, auth);
                    let response = format!(
                        "HTTP/1.1 {status}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                    let _ = socket.write_all(response.as_bytes()).await;
                });
            }
        });
        Self { url, state }
    }

    /// 收到的請求，格式為 `METHOD path?query`。
    pub fn requests(&self) -> Vec<String> {
        self.state.lock().unwrap().requests.clone()
    }

    /// 加在最新的位置；`parent` 為子 session 的上層。
    pub fn add_session(&self, id: &str, created: i64, updated: i64, parent: Option<&str>) {
        let mut session = json!({
            "id": id,
            "projectID": "global",
            "title": format!("title of {id}"),
            "cost": 0,
            "tokens": {},
            "location": {"directory": "/workspace"},
            "time": {"created": created, "updated": updated},
        });
        if let Some(parent) = parent {
            session["parentID"] = json!(parent);
        }
        self.state.lock().unwrap().sessions.push(session);
    }

    pub fn session_ids(&self) -> Vec<String> {
        let state = self.state.lock().unwrap();
        state
            .sessions
            .iter()
            .map(|session| session["id"].as_str().unwrap().to_string())
            .collect()
    }
}

async fn read_request(
    socket: &mut tokio::net::TcpStream,
) -> Option<(String, String, Option<String>)> {
    let mut buf = Vec::new();
    let mut tmp = [0u8; 4096];
    let header_end = loop {
        let n = socket.read(&mut tmp).await.ok().filter(|n| *n > 0)?;
        buf.extend_from_slice(&tmp[..n]);
        if let Some(pos) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break pos + 4;
        }
    };
    let head = String::from_utf8_lossy(&buf[..header_end]).into_owned();
    let header = |name: &str| {
        head.lines().find_map(|line| {
            let (key, value) = line.split_once(':')?;
            key.eq_ignore_ascii_case(name)
                .then(|| value.trim().to_string())
        })
    };
    let content_length: usize = header("content-length")
        .and_then(|value| value.parse().ok())
        .unwrap_or(0);
    while buf.len() < header_end + content_length {
        match socket.read(&mut tmp).await {
            Ok(0) | Err(_) => break,
            Ok(n) => buf.extend_from_slice(&tmp[..n]),
        }
    }
    let mut words = head.split_whitespace();
    let method = words.next()?.to_string();
    let path = words.next()?.to_string();
    Some((method, path, header("authorization")))
}

fn handle(
    state: &Mutex<State>,
    method: &str,
    path: &str,
    auth: Option<String>,
) -> (&'static str, &'static str, String) {
    const JSON: &str = "application/json";
    let mut state = state.lock().unwrap();
    state.requests.push(format!("{method} {path}"));
    if !path.starts_with("/api/") {
        return (
            "200 OK",
            "text/html",
            "<!doctype html><html></html>".to_string(),
        );
    }
    let expected = format!(
        "Basic {}",
        BASE64_STANDARD.encode(format!("opencode:{PASSWORD}"))
    );
    if auth.as_deref() != Some(expected.as_str()) {
        return (
            "401 Unauthorized",
            JSON,
            r#"{"_tag":"UnauthorizedError","message":"Authentication required"}"#.to_string(),
        );
    }
    let (route, query) = path.split_once('?').unwrap_or((path, ""));
    let not_found = |id: &str| {
        (
            "404 Not Found",
            JSON,
            json!({"_tag":"SessionNotFoundError","sessionID":id,"message":format!("Session not found: {id}")})
                .to_string(),
        )
    };
    match (method, route) {
        ("GET", "/api/info") => (
            "200 OK",
            JSON,
            r#"{"version":"2.3.2","pid":1,"urls":["http://127.0.0.1:4096"],"paths":{"tmp":"/tmp/opencode"}}"#
                .to_string(),
        ),
        ("POST", "/api/session") => {
            state.created += 1;
            let id = format!("ses_fake{}", state.created);
            let session = json!({"id": id, "projectID": "global", "time": {"created": 1, "updated": 1}});
            state.sessions.push(session.clone());
            ("200 OK", JSON, json!({ "data": session }).to_string())
        }
        ("GET", "/api/session") => {
            let param = |name: &str| {
                query.split('&').find_map(|pair| {
                    let (key, value) = pair.split_once('=')?;
                    (key == name).then(|| value.to_string())
                })
            };
            let limit: usize = param("limit").and_then(|v| v.parse().ok()).unwrap_or(50);
            let offset: usize = match param("cursor") {
                None => 0,
                Some(cursor) => match cursor.strip_prefix('o').and_then(|n| n.parse().ok()) {
                    Some(offset) => offset,
                    None => {
                        return (
                            "400 Bad Request",
                            JSON,
                            r#"{"_tag":"InvalidCursorError","message":"Invalid cursor"}"#
                                .to_string(),
                        )
                    }
                },
            };
            let newest_first: Vec<&Value> = state.sessions.iter().rev().collect();
            let page: Vec<&Value> = newest_first
                .iter()
                .skip(offset)
                .take(limit.min(PAGE_CAP))
                .copied()
                .collect();
            let next = (!page.is_empty()).then(|| format!("o{}", offset + page.len()));
            (
                "200 OK",
                JSON,
                json!({"data": page, "cursor": {"previous": null, "next": next}}).to_string(),
            )
        }
        (method, route) if route.starts_with("/api/session/") => {
            let id = &route["/api/session/".len()..];
            let index = state.sessions.iter().position(|s| s["id"] == id);
            match (method, index) {
                ("GET", Some(index)) => (
                    "200 OK",
                    JSON,
                    json!({ "data": state.sessions[index] }).to_string(),
                ),
                ("DELETE", Some(index)) => {
                    state.sessions.remove(index);
                    ("204 No Content", JSON, String::new())
                }
                _ => not_found(id),
            }
        }
        _ => (
            "404 Not Found",
            JSON,
            r#"{"_tag":"NotFound","message":"Not found"}"#.to_string(),
        ),
    }
}
