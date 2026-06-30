# Opencode Serve Streaming Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `OpencodeServerBackend::run_streaming` emit reasoning, tool/function-use, and step progress events from `opencode serve` while keeping final answer text as a whole response.

**Architecture:** Keep `AiBackend` and existing `StreamEvent` unchanged. Add focused helpers inside `crates/wukong-gateway/src/opencode_server.rs` for SSE parsing, opencode event mapping, async prompt send, and final message fetch; then change only the server backend `run_streaming` path to use `POST /session/:id/prompt_async` plus `GET /event`.

**Tech Stack:** Rust 2021, Tokio, reqwest, serde_json, existing Wukong `AiBackend` / `StreamEvent`, opencode server HTTP/SSE API.

---

## File Structure

- Modify `crates/wukong-gateway/src/opencode_server.rs`: add SSE/event parsing helpers, async prompt helper, final message fetch helper, and streaming loop.
- No Cargo dependency change expected. The plan uses `reqwest::Response::chunk` and unit-tests parser/event helpers directly.
- No Web/Telegram/Scheduler UI changes expected. They already consume `StreamEvent::Reasoning`, `StreamEvent::ToolUse`, `StreamEvent::StepStart`, and `StreamEvent::StepFinish`.
- No Docker Compose change expected. `v0.16.18-rc.1` already starts `opencode-server` and wires `WUKONG_AGENT_SERVER_URL` into long-lived services.

---

### Task 1: Add SSE Frame Parsing and Opencode Event Mapping

**Files:**
- Modify: `crates/wukong-gateway/src/opencode_server.rs`
- Test: `crates/wukong-gateway/src/opencode_server.rs`

- [ ] **Step 1: Run GitNexus impact before editing the server backend**

Run:

```text
gitnexus_impact({"target":"OpencodeServerBackend","direction":"upstream","file_path":"crates/wukong-gateway/src/opencode_server.rs","repo":"Wukong"})
```

Expected: reports upstream runtime entrypoints through `AgentBackend`. If risk is HIGH or CRITICAL, report it before editing. This change is expected to affect the server backend path only.

- [ ] **Step 2: Add failing tests for SSE frame parsing**

Add these tests inside the existing `#[cfg(test)] mod tests` in `crates/wukong-gateway/src/opencode_server.rs`:

```rust
    #[test]
    fn sse_parser_collects_single_data_event() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line("data: {\"hello\":true}"), None);
        assert_eq!(
            parser.feed_line(""),
            Some("{\"hello\":true}".to_string())
        );
    }

    #[test]
    fn sse_parser_joins_multiline_data_and_ignores_comments() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line(": keep-alive"), None);
        assert_eq!(parser.feed_line("event: message"), None);
        assert_eq!(parser.feed_line("data: {\"a\":"), None);
        assert_eq!(parser.feed_line("data: 1}"), None);

        assert_eq!(parser.feed_line(""), Some("{\"a\":\n1}".to_string()));
    }

    #[test]
    fn sse_parser_ignores_blank_events() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line("event: ping"), None);
        assert_eq!(parser.feed_line(""), None);
    }
```

- [ ] **Step 3: Run tests to verify failure**

Run:

```bash
cargo test -p wukong-gateway opencode_server::tests::sse_parser
```

Expected: FAIL because `SseParser` does not exist.

- [ ] **Step 4: Implement the minimal SSE parser**

Add this helper near the other private helpers in `crates/wukong-gateway/src/opencode_server.rs`:

```rust
#[derive(Default)]
struct SseParser {
    data_lines: Vec<String>,
}

impl SseParser {
    fn feed_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if self.data_lines.is_empty() {
                return None;
            }
            return Some(std::mem::take(&mut self.data_lines).join("\n"));
        }
        if line.starts_with(':') {
            return None;
        }
        if let Some(data) = line.strip_prefix("data:") {
            self.data_lines.push(data.strip_prefix(' ').unwrap_or(data).to_string());
        }
        None
    }
}
```

- [ ] **Step 5: Run parser tests to verify pass**

Run:

```bash
cargo test -p wukong-gateway opencode_server::tests::sse_parser
```

Expected: PASS.

- [ ] **Step 6: Add failing tests for opencode event mapping**

Add these tests to the same test module:

```rust
    #[test]
    fn maps_reasoning_delta_for_matching_session() {
        let value = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "delta": "thinking",
                    "part": {
                        "id": "part_1",
                        "sessionID": "ses_1",
                        "messageID": "msg_1",
                        "type": "reasoning",
                        "text": "thinking total"
                    }
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::Reasoning("thinking".to_string()))
        );
    }

    #[test]
    fn maps_reasoning_text_when_delta_missing() {
        let value = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": {
                        "id": "part_1",
                        "sessionID": "ses_1",
                        "messageID": "msg_1",
                        "type": "reasoning",
                        "text": "thinking total"
                    }
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::Reasoning("thinking total".to_string()))
        );
    }

    #[test]
    fn maps_tool_use_once_per_call_id() {
        let value = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": {
                        "id": "part_tool",
                        "sessionID": "ses_1",
                        "messageID": "msg_1",
                        "type": "tool",
                        "callID": "call_1",
                        "tool": "bash"
                    }
                }
            }
        });
        let mut seen_tools = std::collections::HashSet::new();

        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::ToolUse("bash".to_string()))
        );
        assert_eq!(
            map_server_event(&value, "ses_1", &mut seen_tools),
            ServerEventAction::Ignore
        );
    }

    #[test]
    fn maps_step_boundaries_and_idle() {
        let mut seen_tools = std::collections::HashSet::new();
        let step_start = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": { "id": "s1", "sessionID": "ses_1", "type": "step-start" }
                }
            }
        });
        let step_finish = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "part": { "id": "s2", "sessionID": "ses_1", "type": "step-finish" }
                }
            }
        });
        let idle = json!({
            "payload": {
                "type": "session.idle",
                "properties": { "sessionID": "ses_1" }
            }
        });

        assert_eq!(
            map_server_event(&step_start, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::StepStart)
        );
        assert_eq!(
            map_server_event(&step_finish, "ses_1", &mut seen_tools),
            ServerEventAction::Emit(StreamEvent::StepFinish)
        );
        assert_eq!(
            map_server_event(&idle, "ses_1", &mut seen_tools),
            ServerEventAction::Idle
        );
    }

    #[test]
    fn ignores_events_for_other_sessions_and_text_parts() {
        let mut seen_tools = std::collections::HashSet::new();
        let other = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "delta": "hidden",
                    "part": { "id": "p", "sessionID": "ses_2", "type": "reasoning" }
                }
            }
        });
        let text = json!({
            "payload": {
                "type": "message.part.updated",
                "properties": {
                    "delta": "answer",
                    "part": { "id": "p", "sessionID": "ses_1", "type": "text", "text": "answer" }
                }
            }
        });

        assert_eq!(
            map_server_event(&other, "ses_1", &mut seen_tools),
            ServerEventAction::Ignore
        );
        assert_eq!(
            map_server_event(&text, "ses_1", &mut seen_tools),
            ServerEventAction::Ignore
        );
    }
```

- [ ] **Step 7: Run mapping tests to verify failure**

Run:

```bash
cargo test -p wukong-gateway opencode_server::tests::maps_
```

Expected: FAIL because `ServerEventAction` and `map_server_event` do not exist.

- [ ] **Step 8: Implement event mapping helpers**

Add these helpers below `SseParser`:

```rust
#[derive(Debug, PartialEq)]
enum ServerEventAction {
    Emit(StreamEvent),
    Idle,
    Ignore,
}

fn map_server_event(
    value: &Value,
    session_id: &str,
    seen_tools: &mut std::collections::HashSet<String>,
) -> ServerEventAction {
    let payload = match value.get("payload") {
        Some(payload) => payload,
        None => value,
    };
    let event_type = payload.get("type").and_then(Value::as_str).unwrap_or_default();
    let properties = payload.get("properties").unwrap_or(payload);

    if event_type == "session.idle" {
        return match event_session_id(properties).as_deref() {
            Some(id) if id == session_id => ServerEventAction::Idle,
            _ => ServerEventAction::Ignore,
        };
    }
    if event_type == "session.status" {
        let is_idle = properties
            .get("status")
            .and_then(|status| status.get("type"))
            .and_then(Value::as_str)
            .map(|kind| kind == "idle")
            .unwrap_or(false);
        return match (event_session_id(properties).as_deref(), is_idle) {
            (Some(id), true) if id == session_id => ServerEventAction::Idle,
            _ => ServerEventAction::Ignore,
        };
    }
    if event_type != "message.part.updated" {
        return ServerEventAction::Ignore;
    }

    let part = match properties.get("part") {
        Some(part) => part,
        None => return ServerEventAction::Ignore,
    };
    if part.get("sessionID").and_then(Value::as_str) != Some(session_id) {
        return ServerEventAction::Ignore;
    }

    match part.get("type").and_then(Value::as_str).unwrap_or_default() {
        "reasoning" => {
            let text = properties
                .get("delta")
                .and_then(Value::as_str)
                .or_else(|| part.get("text").and_then(Value::as_str))
                .unwrap_or_default();
            if text.trim().is_empty() {
                ServerEventAction::Ignore
            } else {
                ServerEventAction::Emit(StreamEvent::Reasoning(text.to_string()))
            }
        }
        "tool" => {
            let dedupe_key = part
                .get("callID")
                .and_then(Value::as_str)
                .or_else(|| part.get("id").and_then(Value::as_str))
                .unwrap_or("tool")
                .to_string();
            if !seen_tools.insert(dedupe_key) {
                return ServerEventAction::Ignore;
            }
            let name = part.get("tool").and_then(Value::as_str).unwrap_or("tool");
            ServerEventAction::Emit(StreamEvent::ToolUse(name.to_string()))
        }
        "step-start" => ServerEventAction::Emit(StreamEvent::StepStart),
        "step-finish" => ServerEventAction::Emit(StreamEvent::StepFinish),
        _ => ServerEventAction::Ignore,
    }
}

fn event_session_id(properties: &Value) -> Option<String> {
    properties
        .get("sessionID")
        .and_then(Value::as_str)
        .or_else(|| {
            properties
                .get("session")
                .and_then(|session| session.get("id"))
                .and_then(Value::as_str)
        })
        .or_else(|| {
            properties
                .get("info")
                .and_then(|info| info.get("id"))
                .and_then(Value::as_str)
        })
        .map(str::to_string)
}
```

- [ ] **Step 9: Run mapping tests to verify pass**

Run:

```bash
cargo test -p wukong-gateway opencode_server::tests::maps_
```

Expected: PASS.

- [ ] **Step 10: Run all opencode_server unit tests**

Run:

```bash
cargo test -p wukong-gateway opencode_server
```

Expected: PASS.

- [ ] **Step 11: Stage, inspect, and commit Task 1**

Stage the intended file:

```bash
git add crates/wukong-gateway/src/opencode_server.rs
```

Then run:

```text
gitnexus_detect_changes({"scope":"staged","repo":"Wukong"})
```

Then commit only the intended file:

```bash
git commit -m "feat: parse opencode server stream events"
```

---

### Task 2: Add Async Prompt and Final Message Helpers

**Files:**
- Modify: `crates/wukong-gateway/src/opencode_server.rs`
- Test: `crates/wukong-gateway/src/opencode_server.rs`

- [ ] **Step 1: Add failing tests for final assistant text extraction**

Add these tests inside the same test module:

```rust
    #[test]
    fn extracts_latest_assistant_text_from_message_list() {
        let value = json!([
            {
                "info": { "id": "msg_user", "role": "user", "sessionID": "ses_1" },
                "parts": [{ "type": "text", "text": "question" }]
            },
            {
                "info": { "id": "msg_old", "role": "assistant", "sessionID": "ses_1" },
                "parts": [{ "type": "text", "text": "old" }]
            },
            {
                "info": { "id": "msg_new", "role": "assistant", "sessionID": "ses_1" },
                "parts": [
                    { "type": "reasoning", "text": "hidden" },
                    { "type": "text", "text": "hello" },
                    { "type": "text", "text": "world" }
                ]
            }
        ]);

        assert_eq!(extract_latest_assistant_text(&value), "hello\nworld");
    }

    #[test]
    fn latest_assistant_text_is_empty_when_absent() {
        let value = json!([
            {
                "info": { "id": "msg_user", "role": "user", "sessionID": "ses_1" },
                "parts": [{ "type": "text", "text": "question" }]
            }
        ]);

        assert_eq!(extract_latest_assistant_text(&value), "");
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run:

```bash
cargo test -p wukong-gateway latest_assistant_text
```

Expected: FAIL because `extract_latest_assistant_text` does not exist.

- [ ] **Step 3: Implement final assistant text extraction**

Add this helper near `extract_text`:

```rust
fn extract_latest_assistant_text(value: &Value) -> String {
    let Some(messages) = value.as_array() else {
        return String::new();
    };
    messages
        .iter()
        .rev()
        .find(|message| {
            message
                .get("info")
                .and_then(|info| info.get("role"))
                .and_then(Value::as_str)
                == Some("assistant")
        })
        .map(extract_text)
        .unwrap_or_default()
        .trim()
        .to_string()
}
```

- [ ] **Step 4: Run final text tests to verify pass**

Run:

```bash
cargo test -p wukong-gateway latest_assistant_text
```

Expected: PASS.

- [ ] **Step 5: Add HTTP helpers for async prompt and message list**

In the `impl OpencodeServerBackend` block, add these methods:

```rust
    async fn send_message_async(
        &self,
        session_id: &str,
        req: &AgentRequest,
    ) -> Result<(), GatewayError> {
        let url = format!("{}/session/{}/prompt_async", self.base_url, session_id);
        let body = MessageBody {
            model: req.model.as_deref().and_then(parse_model_override),
            parts: vec![MessagePart {
                kind: "text",
                text: req.prompt.clone(),
            }],
        };
        self.send_empty(self.client.post(url).json(&body)).await
    }

    async fn list_messages(&self, session_id: &str) -> Result<Value, GatewayError> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        self.send_json(self.client.get(url)).await
    }

    async fn send_empty(&self, request: reqwest::RequestBuilder) -> Result<(), GatewayError> {
        let request = self.authorize(request);
        let response = request.send().await.map_err(http_error)?;
        let status = response.status();
        let text = response.text().await.map_err(http_error)?;
        if !status.is_success() {
            return Err(GatewayError::AgentFailed {
                code: Some(status.as_u16() as i32),
                stderr: format!("opencode server returned {status}: {text}"),
            });
        }
        Ok(())
    }

    fn authorize(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match self.password.as_deref() {
            Some(password) => request.basic_auth(
                self.username.as_deref().unwrap_or("opencode"),
                Some(password),
            ),
            None => request,
        }
    }
```

Then change the start of `send_json` from:

```rust
        let request = match self.password.as_deref() {
            Some(password) => request.basic_auth(
                self.username.as_deref().unwrap_or("opencode"),
                Some(password),
            ),
            None => request,
        };
```

to:

```rust
        let request = self.authorize(request);
```

- [ ] **Step 6: Run opencode_server tests**

Run:

```bash
cargo test -p wukong-gateway opencode_server
```

Expected: PASS.

- [ ] **Step 7: Stage, inspect, and commit Task 2**

Stage the intended file:

```bash
git add crates/wukong-gateway/src/opencode_server.rs
```

Then run:

```text
gitnexus_detect_changes({"scope":"staged","repo":"Wukong"})
```

Then commit:

```bash
git commit -m "feat: add opencode async prompt helpers"
```

---

### Task 3: Implement Server Backend Streaming Loop

**Files:**
- Modify: `crates/wukong-gateway/src/opencode_server.rs`
- Test: `crates/wukong-gateway/src/opencode_server.rs`

- [ ] **Step 1: Add failing test for streaming event line handling without HTTP**

Add this unit test to verify that a sequence of SSE data payloads emits events and stops on idle:

```rust
    #[test]
    fn maps_sse_payload_sequence_to_stream_events_until_idle() {
        let payloads = vec![
            json!({
                "payload": {
                    "type": "message.part.updated",
                    "properties": {
                        "delta": "think",
                        "part": { "id": "r1", "sessionID": "ses_1", "type": "reasoning" }
                    }
                }
            }),
            json!({
                "payload": {
                    "type": "message.part.updated",
                    "properties": {
                        "part": { "id": "t1", "sessionID": "ses_1", "type": "tool", "tool": "bash" }
                    }
                }
            }),
            json!({
                "payload": {
                    "type": "session.idle",
                    "properties": { "sessionID": "ses_1" }
                }
            }),
        ];
        let mut seen_tools = std::collections::HashSet::new();
        let mut events = Vec::new();
        let mut idle = false;

        for payload in payloads {
            match map_server_event(&payload, "ses_1", &mut seen_tools) {
                ServerEventAction::Emit(event) => events.push(event),
                ServerEventAction::Idle => {
                    idle = true;
                    break;
                }
                ServerEventAction::Ignore => {}
            }
        }

        assert!(idle);
        assert_eq!(
            events,
            vec![
                StreamEvent::Reasoning("think".to_string()),
                StreamEvent::ToolUse("bash".to_string()),
            ]
        );
    }
```

- [ ] **Step 2: Run the sequence test**

Run:

```bash
cargo test -p wukong-gateway maps_sse_payload_sequence
```

Expected: PASS if Task 1 helpers are correct. If it fails, fix the mapping helper before touching HTTP streaming.

- [ ] **Step 3: Implement event stream opening helper**

In `impl OpencodeServerBackend`, add:

```rust
    async fn open_event_stream(&self) -> Result<reqwest::Response, GatewayError> {
        let url = format!("{}/event", self.base_url);
        let response = self.authorize(self.client.get(url)).send().await.map_err(http_error)?;
        let status = response.status();
        if !status.is_success() {
            let text = response.text().await.map_err(http_error)?;
            return Err(GatewayError::AgentFailed {
                code: Some(status.as_u16() as i32),
                stderr: format!("opencode server returned {status}: {text}"),
            });
        }
        Ok(response)
    }
```

- [ ] **Step 4: Implement `run_streaming` using `prompt_async` and `/event`**

Replace the current `OpencodeServerBackend::run_streaming` implementation with:

```rust
    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        self.health_check().await?;

        let mut session_id = match req.session_id.clone() {
            Some(id) => id,
            None => self.create_session().await?,
        };

        let response = self.open_event_stream().await?;
        match self.send_message_async(&session_id, &req).await {
            Ok(()) => {}
            Err(GatewayError::AgentFailed { code: Some(code), .. })
                if code == StatusCode::NOT_FOUND.as_u16() as i32 =>
            {
                session_id = self.create_session().await?;
                self.send_message_async(&session_id, &req).await?;
            }
            Err(err) => return Err(err),
        }

        self.consume_event_stream(response, &session_id, on_event).await?;
        let messages = self.list_messages(&session_id).await?;
        Ok(AgentResponse {
            text: extract_latest_assistant_text(&messages),
            session_id: Some(session_id),
        })
    }
```

Then add this helper in the same `impl` block:

```rust
    async fn consume_event_stream(
        &self,
        mut response: reqwest::Response,
        session_id: &str,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<(), GatewayError> {
        let mut parser = SseParser::default();
        let mut buffer = String::new();
        let mut seen_tools = std::collections::HashSet::new();
        let deadline = tokio::time::sleep(agent_timeout());
        tokio::pin!(deadline);

        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk.map_err(http_error)?,
                _ = &mut deadline => {
                    return Err(GatewayError::AgentFailed {
                        code: None,
                        stderr: "opencode server stream timed out before session became idle".to_string(),
                    });
                }
            };
            let Some(chunk) = chunk else {
                return Err(GatewayError::AgentFailed {
                    code: None,
                    stderr: "opencode server event stream ended before session became idle".to_string(),
                });
            };
            buffer.push_str(&String::from_utf8_lossy(&chunk));
            while let Some(newline) = buffer.find('\n') {
                let mut line = buffer.drain(..=newline).collect::<String>();
                if line.ends_with('\n') {
                    line.pop();
                }
                if let Some(data) = parser.feed_line(&line) {
                    let value: Value = match serde_json::from_str(&data) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    match map_server_event(&value, session_id, &mut seen_tools) {
                        ServerEventAction::Emit(event) => on_event(event),
                        ServerEventAction::Idle => return Ok(()),
                        ServerEventAction::Ignore => {}
                    }
                }
            }
        }
    }
```

- [ ] **Step 5: Run gateway tests**

Run:

```bash
cargo test -p wukong-gateway
```

Expected: PASS.

- [ ] **Step 6: Run affected package tests**

Run:

```bash
cargo test -p wukong-cli -p wukong-web -p wukong-telegram -p wukong-schedulerd
```

Expected: PASS. These packages should not need code changes because they already consume the existing `StreamEvent` variants.

- [ ] **Step 7: Stage, inspect, and commit Task 3**

Stage the intended file:

```bash
git add crates/wukong-gateway/src/opencode_server.rs
```

Then run:

```text
gitnexus_detect_changes({"scope":"staged","repo":"Wukong"})
```

Then commit:

```bash
git commit -m "feat: stream opencode serve progress events"
```

---

### Task 4: Verify Against a Real Docker Opencode Server

**Files:**
- No source edits expected.

- [ ] **Step 1: Build or update the Docker runtime locally**

From the repository root, run:

```bash
docker compose build opencode-server wukong-web wukong-telegram wukong-schedulerd
```

Expected: PASS.

- [ ] **Step 2: Start the server-backed runtime**

Run:

```bash
docker compose up -d opencode-server wukong-web
```

Expected: `opencode-server` and `wukong-web` are up.

- [ ] **Step 3: Confirm opencode server health from Wukong Web**

Run:

```bash
docker compose exec -T wukong-web curl -fsS http://opencode-server:4096/global/health
```

Expected: JSON containing `"healthy":true`.

- [ ] **Step 4: Manually verify Web progress events**

Open Web Console and submit a prompt that should require a tool call, for example:

```text
請用工具列出目前 workspace 的檔案，然後簡短說明看到什麼。
```

Expected:

- Progress shows reasoning when the model emits thinking.
- Progress shows tool/function use such as `bash`, `list`, or another opencode tool name.
- Final answer appears once at completion.
- No duplicate final answer text appears during progress.

- [ ] **Step 5: Stop test services if they are not needed**

Run:

```bash
docker compose stop wukong-web opencode-server
```

Expected: services stop cleanly.

---

### Task 5: Full Verification and Release Candidate Prep

**Files:**
- No source edits expected unless verification reveals a bug.

- [ ] **Step 1: Format code**

Run:

```bash
cargo fmt
```

Expected: no output, or only formatting changes in intended Rust files.

- [ ] **Step 2: Run full tests**

Run:

```bash
cargo test --workspace
```

Expected: PASS.

- [ ] **Step 3: Run workspace check**

Run:

```bash
cargo check --workspace
```

Expected: PASS.

- [ ] **Step 4: Validate Docker Compose**

Run:

```bash
docker compose config
```

Expected: PASS.

- [ ] **Step 5: Run GitNexus change detection**

Run:

```text
gitnexus_detect_changes({"scope":"all","repo":"Wukong"})
```

Expected: changed symbols are limited to `OpencodeServerBackend` streaming helpers and related tests. Risk may be MEDIUM/HIGH because a backend method is shared; report the risk and affected flows.

- [ ] **Step 6: Final status check**

Run:

```bash
git status --short --branch
```

Expected: only intended commits plus any known pre-existing dirty files such as `AGENTS.md` and `CLAUDE.md`.

- [ ] **Step 7: Prepare RC only after verification passes**

If the user asks for an RC, use the `wukong-release` skill. Do not tag or push automatically from this plan unless explicitly requested.

---

## Plan Self-Review

- Spec coverage: covers SSE parsing, event mapping, final answer extraction, error handling, testing, Docker verification, and non-goal of assistant text token streaming.
- Placeholder scan: no placeholder markers remain. Optional integration test is intentionally excluded from mandatory tasks to keep scope small; manual Docker verification covers real server behavior.
- Type consistency: all helpers use existing `Value`, `GatewayError`, `StreamEvent`, `AgentRequest`, and `AgentResponse` types from `opencode_server.rs` imports.
