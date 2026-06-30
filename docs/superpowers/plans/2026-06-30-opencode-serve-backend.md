# Opencode Serve Backend Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Docker-first `opencode serve` backend that reduces repeated opencode startup latency while keeping the current `opencode run` CLI backend as the default fallback.

**Architecture:** Keep `AiBackend` as the boundary. Add `OpencodeServerBackend` for HTTP calls and an `AgentBackend` enum that delegates to either the existing `AgentCliBackend` or the new server backend, so callers can use one concrete backend type without large generic refactors. Docker services opt in through `WUKONG_AGENT_SERVER_URL`; binary installs remain on `opencode run` unless users manually set that URL.

**Tech Stack:** Rust 2021, Tokio, reqwest with rustls, serde/serde_json, existing Wukong `AiBackend`, Docker Compose.

---

## File Structure

- Modify `crates/wukong-gateway/Cargo.toml`: add `reqwest` and `serde` dependencies from the workspace.
- Modify `crates/wukong-gateway/src/backend.rs`: keep existing CLI backend, add `AgentBackend` enum, add `build_backend_from_env`, make timeout helper usable by both backends, and add delegation tests.
- Create `crates/wukong-gateway/src/opencode_server.rs`: implement HTTP client, session creation, message send, basic auth, response text extraction, timeout handling, and tests.
- Modify `crates/wukong-gateway/src/lib.rs`: export the new module.
- Modify `crates/wukong-cli/src/main.rs`: construct `AgentBackend` and change helper function parameters from `AgentCliBackend` to `AgentBackend`.
- Modify `crates/wukong-web/src/main.rs`: construct `AgentBackend`; existing generic `AppState<B>` can remain unchanged.
- Modify `crates/wukong-telegram/src/main.rs`: construct `AgentBackend`.
- Modify `crates/wukong-schedulerd/src/main.rs`: construct `AgentBackend` and change helper function parameters from `AgentCliBackend` to `AgentBackend`.
- Modify `docker-compose.yml`: add `opencode-server` and wire `WUKONG_AGENT_SERVER_URL` into Web, Telegram, and Scheduler services only.
- Modify `README.md`: document Docker low-latency mode and binary behavior.

---

### Task 1: Add Backend Selection Without HTTP Behavior

**Files:**
- Modify: `crates/wukong-gateway/src/backend.rs`
- Modify: `crates/wukong-gateway/Cargo.toml`
- Test: `crates/wukong-gateway/src/backend.rs`

- [ ] **Step 1: Add a failing test for CLI fallback selection**

Add this test inside `crates/wukong-gateway/src/backend.rs` `#[cfg(test)] mod tests`:

```rust
#[test]
fn backend_from_env_uses_cli_when_server_url_missing() {
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    let _lock = ENV_LOCK.lock().unwrap();
    let previous = std::env::var("WUKONG_AGENT_SERVER_URL").ok();
    std::env::remove_var("WUKONG_AGENT_SERVER_URL");

    let backend = build_backend_from_env(
        vec!["opencode".to_string(), "run".to_string()],
        None,
    );

    match previous {
        Some(value) => std::env::set_var("WUKONG_AGENT_SERVER_URL", value),
        None => std::env::remove_var("WUKONG_AGENT_SERVER_URL"),
    }

    assert!(matches!(backend, AgentBackend::Cli(_)));
}
```

- [ ] **Step 2: Run the failing gateway backend test**

Run: `cargo test -p wukong-gateway backend_from_env_uses_cli_when_server_url_missing`

Expected: FAIL because `build_backend_from_env` and `AgentBackend` do not exist.

- [ ] **Step 3: Add the backend enum and selector**

In `crates/wukong-gateway/src/backend.rs`, create the enum, selector, and delegation implementation. This compiles against the `OpencodeServerBackend` skeleton created later in this same step:

```rust
pub enum AgentBackend {
    Cli(AgentCliBackend),
    Server(crate::opencode_server::OpencodeServerBackend),
}

pub fn build_backend_from_env(command: Vec<String>, workspace: Option<PathBuf>) -> AgentBackend {
    match std::env::var("WUKONG_AGENT_SERVER_URL") {
        Ok(url) if !url.trim().is_empty() => AgentBackend::Server(
            crate::opencode_server::OpencodeServerBackend::from_env(url, workspace),
        ),
        _ => AgentBackend::Cli(AgentCliBackend { command, workspace }),
    }
}

impl AiBackend for AgentBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        match self {
            AgentBackend::Cli(backend) => backend.run(req).await,
            AgentBackend::Server(backend) => backend.run(req).await,
        }
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        match self {
            AgentBackend::Cli(backend) => backend.run_streaming(req, on_event).await,
            AgentBackend::Server(backend) => backend.run_streaming(req, on_event).await,
        }
    }
}
```

Create `crates/wukong-gateway/src/opencode_server.rs` with this minimal server backend so the enum compiles before the HTTP client is added:

```rust
use crate::backend::{AgentRequest, AgentResponse, AiBackend};
use crate::error::GatewayError;
use crate::stream::StreamEvent;
use std::path::PathBuf;

pub struct OpencodeServerBackend {
    pub base_url: String,
    pub workspace: Option<PathBuf>,
}

impl OpencodeServerBackend {
    pub fn from_env(base_url: String, workspace: Option<PathBuf>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            workspace,
        }
    }
}

impl AiBackend for OpencodeServerBackend {
    async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "opencode server backend requires the HTTP client task before use".to_string(),
        })
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        let resp = self.run(req).await?;
        on_event(StreamEvent::Text(resp.text.clone()));
        Ok(resp)
    }
}
```

Update `crates/wukong-gateway/src/lib.rs`:

```rust
pub mod opencode_server;
```

- [ ] **Step 4: Run the gateway backend test**

Run: `cargo test -p wukong-gateway backend_from_env_uses_cli_when_server_url_missing`

Expected: PASS.

- [ ] **Step 5: Commit Task 1**

```bash
git add crates/wukong-gateway/src/backend.rs crates/wukong-gateway/src/opencode_server.rs crates/wukong-gateway/src/lib.rs crates/wukong-gateway/Cargo.toml
git commit -m "feat: add selectable agent backend"
```

---

### Task 2: Implement the Opencode Server HTTP Backend

**Files:**
- Modify: `crates/wukong-gateway/Cargo.toml`
- Modify: `crates/wukong-gateway/src/backend.rs`
- Modify: `crates/wukong-gateway/src/opencode_server.rs`
- Test: `crates/wukong-gateway/src/opencode_server.rs`

- [ ] **Step 1: Add dependencies**

Update `crates/wukong-gateway/Cargo.toml` dependencies:

```toml
[dependencies]
wukong-memory = { path = "../wukong-memory" }
tokio = { workspace = true }
clap = { workspace = true }
thiserror = { workspace = true }
serde = { workspace = true }
serde_json = { workspace = true }
reqwest = { workspace = true }
```

- [ ] **Step 2: Add focused tests for response text extraction**

Replace the skeleton `crates/wukong-gateway/src/opencode_server.rs` test module with:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn extracts_text_from_nested_message_parts() {
        let value = json!({
            "info": { "id": "msg_1" },
            "parts": [
                { "type": "reasoning", "text": "hidden" },
                { "type": "text", "text": "hello" },
                { "type": "text", "text": "world" }
            ]
        });

        assert_eq!(extract_text(&value), "hello\nworld");
    }

    #[test]
    fn extracts_session_id_from_session_response() {
        let value = json!({ "id": "ses_123", "title": "New Session" });
        assert_eq!(extract_session_id(&value).as_deref(), Some("ses_123"));
    }

    #[test]
    fn trims_base_url_once() {
        let backend = OpencodeServerBackend::from_env(
            "http://opencode-server:4096///".to_string(),
            None,
        );

        assert_eq!(backend.base_url, "http://opencode-server:4096");
    }
}
```

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p wukong-gateway opencode_server`

Expected: FAIL because `extract_text` and `extract_session_id` are absent before Step 4.

- [ ] **Step 4: Implement whole-response HTTP behavior**

Replace `crates/wukong-gateway/src/opencode_server.rs` with:

```rust
use crate::backend::{AgentRequest, AgentResponse, AiBackend};
use crate::error::GatewayError;
use crate::stream::StreamEvent;
use reqwest::StatusCode;
use serde::Serialize;
use serde_json::Value;
use std::path::PathBuf;
use std::time::Duration;

const DEFAULT_AGENT_TIMEOUT_SECS: u64 = 20 * 60;

pub struct OpencodeServerBackend {
    pub base_url: String,
    pub workspace: Option<PathBuf>,
    client: reqwest::Client,
    username: Option<String>,
    password: Option<String>,
}

#[derive(Debug, Serialize)]
struct CreateSessionBody {
    title: String,
}

#[derive(Debug, Serialize)]
struct MessageBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    parts: Vec<MessagePart>,
}

#[derive(Debug, Serialize)]
struct MessagePart {
    #[serde(rename = "type")]
    kind: &'static str,
    text: String,
}

impl OpencodeServerBackend {
    pub fn from_env(base_url: String, workspace: Option<PathBuf>) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            workspace,
            client: reqwest::Client::builder()
                .timeout(agent_timeout())
                .build()
                .expect("reqwest client builder should not fail"),
            username: std::env::var("WUKONG_AGENT_SERVER_USERNAME").ok().filter(|s| !s.is_empty()),
            password: std::env::var("WUKONG_AGENT_SERVER_PASSWORD").ok().filter(|s| !s.is_empty()),
        }
    }

    async fn create_session(&self) -> Result<String, GatewayError> {
        let url = format!("{}/session", self.base_url);
        let value = self
            .send_json(
                self.client.post(url).json(&CreateSessionBody {
                    title: "Wukong".to_string(),
                }),
            )
            .await?;
        extract_session_id(&value).ok_or_else(|| GatewayError::AgentFailed {
            code: None,
            stderr: format!("opencode server did not return a session id: {value}"),
        })
    }

    async fn health_check(&self) -> Result<(), GatewayError> {
        let url = format!("{}/global/health", self.base_url);
        self.send_json(self.client.get(url)).await.map(|_| ())
    }

    async fn send_message(&self, session_id: &str, req: &AgentRequest) -> Result<Value, GatewayError> {
        let url = format!("{}/session/{}/message", self.base_url, session_id);
        let body = MessageBody {
            model: req.model.clone(),
            parts: vec![MessagePart {
                kind: "text",
                text: req.prompt.clone(),
            }],
        };
        self.send_json(self.client.post(url).json(&body)).await
    }

    async fn send_json(&self, request: reqwest::RequestBuilder) -> Result<Value, GatewayError> {
        let request = match self.password.as_deref() {
            Some(password) => request.basic_auth(
                self.username.as_deref().unwrap_or("opencode"),
                Some(password),
            ),
            None => request,
        };
        let response = request.send().await.map_err(http_error)?;
        let status = response.status();
        let text = response.text().await.map_err(http_error)?;
        if !status.is_success() {
            return Err(GatewayError::AgentFailed {
                code: Some(status.as_u16() as i32),
                stderr: format!("opencode server returned {status}: {text}"),
            });
        }
        serde_json::from_str(&text).map_err(|err| GatewayError::AgentFailed {
            code: None,
            stderr: format!("opencode server returned invalid JSON: {err}; body: {text}"),
        })
    }
}

impl AiBackend for OpencodeServerBackend {
    async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
        self.health_check().await?;

        let mut session_id = match req.session_id.clone() {
            Some(id) => id,
            None => self.create_session().await?,
        };

        let value = match self.send_message(&session_id, &req).await {
            Ok(value) => value,
            Err(GatewayError::AgentFailed { code: Some(code), .. }) if code == StatusCode::NOT_FOUND.as_u16() as i32 => {
                session_id = self.create_session().await?;
                self.send_message(&session_id, &req).await?
            }
            Err(err) => return Err(err),
        };
        Ok(AgentResponse {
            text: extract_text(&value).trim().to_string(),
            session_id: Some(session_id),
        })
    }

    async fn run_streaming(
        &self,
        req: AgentRequest,
        on_event: &mut dyn FnMut(StreamEvent),
    ) -> Result<AgentResponse, GatewayError> {
        let resp = self.run(req).await?;
        if !resp.text.is_empty() {
            on_event(StreamEvent::Text(resp.text.clone()));
        }
        Ok(resp)
    }
}

fn extract_session_id(value: &Value) -> Option<String> {
    value.get("id").and_then(Value::as_str).map(str::to_string)
}

fn extract_text(value: &Value) -> String {
    let mut out = Vec::new();
    collect_text(value, &mut out);
    out.join("\n")
}

fn collect_text(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Object(map) => {
            let is_text_part = map
                .get("type")
                .and_then(Value::as_str)
                .map(|kind| kind == "text" || kind == "assistant_text")
                .unwrap_or(false);
            if is_text_part {
                if let Some(text) = map.get("text").and_then(Value::as_str) {
                    if !text.trim().is_empty() {
                        out.push(text.to_string());
                    }
                }
            }
            for child in map.values() {
                collect_text(child, out);
            }
        }
        Value::Array(items) => {
            for item in items {
                collect_text(item, out);
            }
        }
        _ => {}
    }
}

fn agent_timeout() -> Duration {
    std::env::var("WUKONG_AGENT_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_AGENT_TIMEOUT_SECS))
}

fn http_error(err: reqwest::Error) -> GatewayError {
    GatewayError::AgentFailed {
        code: None,
        stderr: format!("opencode server request failed: {err}"),
    }
}
```

- [ ] **Step 5: Run gateway tests**

Run: `cargo test -p wukong-gateway`

Expected: PASS.

- [ ] **Step 6: Commit Task 2**

```bash
git add crates/wukong-gateway/Cargo.toml crates/wukong-gateway/src/backend.rs crates/wukong-gateway/src/opencode_server.rs
git commit -m "feat: call opencode serve backend"
```

---

### Task 3: Wire the Selectable Backend Into Runtime Entrypoints

**Files:**
- Modify: `crates/wukong-cli/src/main.rs`
- Modify: `crates/wukong-web/src/main.rs`
- Modify: `crates/wukong-telegram/src/main.rs`
- Modify: `crates/wukong-schedulerd/src/main.rs`

- [ ] **Step 1: Run compile check to capture current concrete backend coupling**

Run: `cargo check --workspace`

Expected: PASS before edits.

- [ ] **Step 2: Update CLI imports and construction**

In `crates/wukong-cli/src/main.rs`, replace:

```rust
use wukong_gateway::backend::AgentCliBackend;
```

with:

```rust
use wukong_gateway::backend::{build_backend_from_env, AgentBackend};
```

Replace backend construction:

```rust
let backend = AgentCliBackend {
    command: cfg.agent_command.clone(),
    workspace: workspace_dir(),
};
```

with:

```rust
let backend = build_backend_from_env(cfg.agent_command.clone(), workspace_dir());
```

Update these private helper signatures that currently accept `&AgentCliBackend` to accept `&AgentBackend`:

```rust
async fn run_schedule_op(
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    op: &ScheduleOp,
) -> Result<(), wukong_cli::WukongError> {
```

```rust
async fn trigger_job(
    store: &SchedulerStore,
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    job: &Job,
    worker_id: &str,
) -> Result<(), wukong_cli::WukongError> {
```

```rust
async fn run_one(
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    input: &str,
) -> Result<(), wukong_cli::WukongError> {
```

```rust
async fn run_memory_op(
    memory: &Memory,
    backend: &AgentBackend,
    cfg: &GatewayConfig,
    op: &MemoryOp,
) -> Result<(), wukong_cli::WukongError> {
```

- [ ] **Step 3: Update Web backend construction**

In `crates/wukong-web/src/main.rs`, replace:

```rust
use wukong_gateway::backend::AgentCliBackend;
```

with:

```rust
use wukong_gateway::backend::build_backend_from_env;
```

Replace:

```rust
let backend = AgentCliBackend {
    command: agent_command,
    workspace: workspace_dir(),
};
```

with:

```rust
let backend = build_backend_from_env(agent_command, workspace_dir());
```

- [ ] **Step 4: Update Telegram backend construction**

In `crates/wukong-telegram/src/main.rs`, replace:

```rust
use wukong_gateway::backend::AgentCliBackend;
```

with:

```rust
use wukong_gateway::backend::build_backend_from_env;
```

Replace:

```rust
let backend = AgentCliBackend {
    command: agent_command,
    workspace: workspace_dir(),
};
```

with:

```rust
let backend = build_backend_from_env(agent_command, workspace_dir());
```

- [ ] **Step 5: Update Scheduler backend construction and helper signatures**

In `crates/wukong-schedulerd/src/main.rs`, replace:

```rust
use wukong_gateway::backend::AgentCliBackend;
```

with:

```rust
use wukong_gateway::backend::{build_backend_from_env, AgentBackend};
```

Replace:

```rust
let backend = AgentCliBackend {
    command: cfg.agent_command.clone(),
    workspace: workspace_dir(),
};
```

with:

```rust
let backend = build_backend_from_env(cfg.agent_command.clone(), workspace_dir());
```

Change scheduler helper signatures that take `&AgentCliBackend` to `&AgentBackend`:

```rust
backend: &AgentBackend,
```

- [ ] **Step 6: Run compile and targeted tests**

Run: `cargo check --workspace`

Expected: PASS.

Run: `cargo test -p wukong-cli -p wukong-web -p wukong-telegram -p wukong-schedulerd`

Expected: PASS.

- [ ] **Step 7: Commit Task 3**

```bash
git add crates/wukong-cli/src/main.rs crates/wukong-web/src/main.rs crates/wukong-telegram/src/main.rs crates/wukong-schedulerd/src/main.rs
git commit -m "feat: wire selectable agent backend"
```

---

### Task 4: Add Docker Opencode Server Service

**Files:**
- Modify: `docker-compose.yml`

- [ ] **Step 1: Add Compose service and environment wiring**

Modify `docker-compose.yml` so Web, Telegram, and Scheduler opt in to the server backend, while the `wukong` CLI profile remains on `WUKONG_AGENT_CMD`.

Add this service after the `wukong` CLI service:

```yaml
  # ── Opencode Server Backend ──
  opencode-server:
    build:
      context: .
      dockerfile: Dockerfile
    image: wukong:latest
    container_name: wukong-opencode-server
    environment:
      - USER_ID=${USER_ID:-1000}
      - GROUP_ID=${GROUP_ID:-1000}
      - WUKONG_WORKSPACE=/workspace
    volumes:
      - ${WUKONG_HOST_WORKSPACE:-./workspace}:/workspace
      - opencode-config:/home/wukong/.config/opencode
      - opencode-state:/home/wukong/.local/share/opencode
      - agent-reach-state:/home/wukong/.agent-reach
      - gh-config:/home/wukong/.config/gh
    command: ["opencode", "serve", "--hostname", "0.0.0.0", "--port", "4096"]
    restart: unless-stopped
```

Add this environment line to `wukong-telegram`, `wukong-web`, and `wukong-schedulerd`:

```yaml
      - WUKONG_AGENT_SERVER_URL=${WUKONG_AGENT_SERVER_URL:-http://opencode-server:4096}
```

Add this dependency to the same three services:

```yaml
    depends_on:
      - opencode-server
```

- [ ] **Step 2: Validate Compose config**

Run: `docker compose config`

Expected: PASS and rendered config contains `opencode-server` plus `WUKONG_AGENT_SERVER_URL=http://opencode-server:4096` for Web, Telegram, and Scheduler.

- [ ] **Step 3: Commit Task 4**

```bash
git add docker-compose.yml
git commit -m "feat: add opencode server docker service"
```

---

### Task 5: Document the Docker-First Serve Mode

**Files:**
- Modify: `README.md`

- [ ] **Step 1: Add README section**

Add this section near the Docker runtime documentation in `README.md`:

```markdown
### Docker 低延遲 opencode serve 模式

Docker 常駐服務預設會啟動 `opencode-server`，並讓 `wukong-web`、`wukong-telegram`、`wukong-schedulerd` 透過 `WUKONG_AGENT_SERVER_URL=http://opencode-server:4096` 呼叫同一個長壽命 `opencode serve` process。

這個模式保留 Wukong 的 scope-level session 管理，但避免每次回合都重新啟動 `opencode run`，可降低 Web、Telegram、Scheduler 等常駐入口的延遲感。

Binary 模式第一版不自動啟動或管理 `opencode serve`。在一般本機 CLI 使用情境，Wukong 仍預設透過 `opencode run` 執行，以避免背景 daemon、port、跨專案工作目錄與清理策略帶來額外複雜度。進階使用者若自行啟動 `opencode serve`，可手動設定 `WUKONG_AGENT_SERVER_URL` 使用同一 backend。

若要回到舊的 Docker CLI subprocess 模式，移除服務環境中的 `WUKONG_AGENT_SERVER_URL`，Wukong 會使用 `WUKONG_AGENT_CMD`，預設為 `opencode run --dangerously-skip-permissions`。
```

- [ ] **Step 2: Run docs grep to verify key terms**

Run: `rg "opencode serve|WUKONG_AGENT_SERVER_URL|Binary 模式" README.md`

Expected: output shows the new section and all three terms.

- [ ] **Step 3: Commit Task 5**

```bash
git add README.md docs/superpowers/specs/2026-06-30-opencode-serve-backend-design.md docs/superpowers/plans/2026-06-30-opencode-serve-backend.md
git commit -m "docs: plan opencode serve backend"
```

---

### Task 6: Full Verification

**Files:**
- No source edits expected.

- [ ] **Step 1: Run Rust test suite**

Run: `cargo test --workspace`

Expected: PASS.

- [ ] **Step 2: Run workspace check**

Run: `cargo check --workspace`

Expected: PASS.

- [ ] **Step 3: Verify Docker config**

Run: `docker compose config`

Expected: PASS.

- [ ] **Step 4: Verify affected execution flows before final commit or PR**

Run GitNexus change detection:

```text
gitnexus_detect_changes({"scope":"all","repo":"Wukong"})
```

Expected: changed symbols are limited to gateway backend selection, opencode server backend, entrypoint construction, Docker wiring, and docs.

- [ ] **Step 5: Final status check**

Run: `git status --short`

Expected: no unintended files are staged or modified beyond this feature.
