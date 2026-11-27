//! 測試用的假 OpenCode V2 server，只實作 Wukong 用到的 `/api` 端點。
//!
//! 行為照 2026-11-24 對 @opencode/cli 2.3.2 的實測（`docs/issues/issue-0044/probe-*.md`）：
//! `/api` 以外的路徑一律回網頁（200，text/html），`/api` 要帶 basic auth 且使用者名稱
//! 固定為 opencode，列表由新到舊、照 cursor 分頁，最後一頁之後還要再取一次空頁才沒有
//! `cursor.next`。`/api/event` 一連上就送出預先排好的事件（多半是真實樣本），送完即斷線。

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub(super) const PASSWORD: &str = "fake-v2-secret";
/// 真的 V2 不設上限；假 server 每頁最多回這麼多，確認呼叫端照 cursor 走，不是信任 limit。
pub(super) const PAGE_CAP: usize = 50;
/// `handle` 回這個狀態時，連線保持開著、一直不回應。
const HANG: &str = "hang";

#[derive(Default)]
struct State {
    /// 由舊到新。
    sessions: Vec<Value>,
    requests: Vec<String>,
    /// 帶 JSON body 的請求：`METHOD path` 與 body。
    bodies: Vec<(String, Value)>,
    events: Vec<String>,
    /// `GET /api/experimental/job` 依序回的背景工作清單；用完之後回空清單。
    jobs: Vec<Value>,
    /// session 指令層：session → (key, value)，依寫入順序。
    entries: Vec<(String, String, Value)>,
    /// `GET /api/agent` 依序回的 agent 清單；剩最後一份時之後都回它。
    agents: Vec<Value>,
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
                    let Some((method, path, auth, body)) = read_request(&mut socket).await else {
                        return;
                    };
                    if method == "GET" && path == "/api/event" && authorized(auth.as_deref()) {
                        let events = {
                            let mut state = state.lock().unwrap();
                            state.requests.push("GET /api/event".to_string());
                            state.events.clone()
                        };
                        stream_events(&mut socket, events).await;
                        return;
                    }
                    let (status, content_type, body) = handle(&state, &method, &path, auth, &body);
                    if status == HANG {
                        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;
                        return;
                    }
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

    /// 之後每次連上 `/api/event` 依序送出的事件（每項一個 JSON）。`pause:<毫秒>` 先停一段
    /// 時間再送下一項，`hold` 讓串流停在那裡不再前進也不斷線；其他情況送完即斷線。
    pub fn script_events(&self, events: Vec<String>) {
        self.state.lock().unwrap().events = events;
    }

    /// 之後每次 `GET /api/experimental/job` 依序回的 `data`（背景工作清單）；用完回空清單。
    /// 項目是字串 `"fail"` 時，那一次回 503；是 `"hang"` 時，那一次一直不回應。
    pub fn script_jobs(&self, jobs: Vec<Value>) {
        self.state.lock().unwrap().jobs = jobs;
    }

    /// 之後每次 `GET /api/agent` 依序回的 `data`；剩最後一份時一直回它，沒有時回空清單。
    pub fn script_agents(&self, agents: Vec<Value>) {
        self.state.lock().unwrap().agents = agents;
    }

    /// 預先放一筆 session 指令層的 entry。
    pub fn set_entry(&self, session: &str, key: &str, value: Value) {
        let mut state = self.state.lock().unwrap();
        state
            .entries
            .retain(|(s, k, _)| !(s == session && k == key));
        state
            .entries
            .push((session.to_string(), key.to_string(), value));
    }

    /// session 指令層目前的 entry（key → value）。
    pub fn entries_of(&self, session: &str) -> Vec<(String, Value)> {
        let state = self.state.lock().unwrap();
        state
            .entries
            .iter()
            .filter(|(s, _, _)| s == session)
            .map(|(_, k, v)| (k.clone(), v.clone()))
            .collect()
    }

    /// 最後一次 `request`（`METHOD path`）的 JSON body。
    pub fn body_of(&self, request: &str) -> Option<Value> {
        let state = self.state.lock().unwrap();
        state
            .bodies
            .iter()
            .rev()
            .find(|(seen, _)| seen == request)
            .map(|(_, body)| body.clone())
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

/// 讀一份 `tests/fixtures/opencode-v2/` 的真實樣本，把樣本裡（回合本身）的 session 換成
/// `session_id`。回合的 session 是沒有 parentID 的 `session.created`；樣本沒有它時（session 在
/// 連上事件串流前就建立了），取第一個帶 sessionID 的事件。
pub(super) fn fixture_events(name: &str, session_id: &str) -> Vec<String> {
    let path = format!(
        "{}/tests/fixtures/opencode-v2/{name}.ndjson",
        env!("CARGO_MANIFEST_DIR")
    );
    let raw = std::fs::read_to_string(path).unwrap();
    let events: Vec<Value> = raw
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .collect();
    let original = events
        .iter()
        .find(|event| event["type"] == "session.created" && event["data"].get("parentID").is_none())
        .or_else(|| {
            events
                .iter()
                .find(|event| event["data"]["sessionID"].is_string())
        })
        .and_then(|event| event["data"]["sessionID"].as_str().map(str::to_string))
        .unwrap();
    raw.lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.replace(&original, session_id))
        .collect()
}

fn authorized(auth: Option<&str>) -> bool {
    let expected = format!(
        "Basic {}",
        BASE64_STANDARD.encode(format!("opencode:{PASSWORD}"))
    );
    auth == Some(expected.as_str())
}

async fn stream_events(socket: &mut tokio::net::TcpStream, events: Vec<String>) {
    let head = "HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: close\r\n\r\n";
    if socket.write_all(head.as_bytes()).await.is_err() {
        return;
    }
    for event in events {
        if event == "hold" {
            std::future::pending::<()>().await;
        }
        if let Some(ms) = event.strip_prefix("pause:") {
            tokio::time::sleep(std::time::Duration::from_millis(ms.parse().unwrap())).await;
            continue;
        }
        if socket
            .write_all(format!("data: {event}\n\n").as_bytes())
            .await
            .is_err()
        {
            return;
        }
    }
}

async fn read_request(
    socket: &mut tokio::net::TcpStream,
) -> Option<(String, String, Option<String>, String)> {
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
    let body = String::from_utf8_lossy(&buf[header_end..]).into_owned();
    Some((method, path, header("authorization"), body))
}

fn handle(
    state: &Mutex<State>,
    method: &str,
    path: &str,
    auth: Option<String>,
    body: &str,
) -> (&'static str, &'static str, String) {
    const JSON: &str = "application/json";
    let mut state = state.lock().unwrap();
    state.requests.push(format!("{method} {path}"));
    if let Ok(json) = serde_json::from_str::<Value>(body) {
        state.bodies.push((format!("{method} {path}"), json));
    }
    if !path.starts_with("/api/") {
        return (
            "200 OK",
            "text/html",
            "<!doctype html><html></html>".to_string(),
        );
    }
    if !authorized(auth.as_deref()) {
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
        ("GET", "/api/agent") => {
            let agents = match state.agents.len() {
                0 => json!([]),
                1 => state.agents[0].clone(),
                _ => state.agents.remove(0),
            };
            ("200 OK", JSON, json!({ "location": {"directory": "/workspace"}, "data": agents }).to_string())
        }
        ("GET", "/api/experimental/job") => {
            let jobs = if state.jobs.is_empty() {
                json!([])
            } else {
                state.jobs.remove(0)
            };
            if jobs == HANG {
                return (HANG, JSON, String::new());
            }
            if jobs == "fail" {
                return (
                    "503 Service Unavailable",
                    JSON,
                    r#"{"_tag":"ServiceUnavailable","message":"job store busy"}"#.to_string(),
                );
            }
            ("200 OK", JSON, json!({ "data": jobs }).to_string())
        }
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
        (method, route) if route.starts_with("/api/experimental/session/") => {
            let rest = &route["/api/experimental/session/".len()..];
            let (id, action) = rest.split_once('/').unwrap_or((rest, ""));
            if !state.sessions.iter().any(|s| s["id"] == id) {
                return not_found(id);
            }
            let id = id.to_string();
            match (method, action.strip_prefix("instructions/entries")) {
                ("GET", Some("")) => {
                    let data: Vec<Value> = state
                        .entries
                        .iter()
                        .filter(|(s, _, _)| *s == id)
                        .map(|(_, k, v)| json!({"key": k, "value": v}))
                        .collect();
                    ("200 OK", JSON, json!({ "data": data }).to_string())
                }
                ("PUT", Some(key)) if key.starts_with('/') => {
                    let key = key[1..].to_string();
                    let value = serde_json::from_str::<Value>(body)
                        .ok()
                        .and_then(|b| b.get("value").cloned())
                        .unwrap_or(Value::Null);
                    state.entries.retain(|(s, k, _)| !(*s == id && *k == key));
                    state.entries.push((id, key, value));
                    ("204 No Content", JSON, String::new())
                }
                _ => not_found(&id),
            }
        }
        (method, route) if route.starts_with("/api/session/") => {
            let rest = &route["/api/session/".len()..];
            let (id, action) = rest.split_once('/').unwrap_or((rest, ""));
            let index = state.sessions.iter().position(|s| s["id"] == id);
            match (method, action, index) {
                (_, _, None) => not_found(id),
                ("POST", "agent" | "model", Some(_)) => ("204 No Content", JSON, String::new()),
                // 提問與權限的回覆：只記錄請求，事件由腳本決定。
                ("POST", action, Some(_))
                    if (action.starts_with("form/") || action.starts_with("permission/"))
                        && action.ends_with("/reply") =>
                {
                    ("204 No Content", JSON, String::new())
                }
                ("DELETE", action, Some(_)) if action.starts_with("form/") => {
                    ("204 No Content", JSON, String::new())
                }
                ("POST", "prompt", Some(_)) => (
                    "200 OK",
                    JSON,
                    json!({"data": {"id": "msg_fake", "sessionID": id, "type": "user"}}).to_string(),
                ),
                ("POST", "compact", Some(_)) => (
                    "200 OK",
                    JSON,
                    json!({"data": {"id": "msg_fake", "sessionID": id, "type": "compaction"}})
                        .to_string(),
                ),
                ("POST", "interrupt", Some(_)) => (
                    "200 OK",
                    JSON,
                    r#"{"data":{"interrupted":true}}"#.to_string(),
                ),
                (_, "", Some(index)) => match method {
                    "GET" => (
                        "200 OK",
                        JSON,
                        json!({ "data": state.sessions[index] }).to_string(),
                    ),
                    "DELETE" => {
                        state.sessions.remove(index);
                        ("204 No Content", JSON, String::new())
                    }
                    _ => not_found(id),
                },
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
