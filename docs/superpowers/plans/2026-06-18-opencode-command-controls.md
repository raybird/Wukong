# Opencode Command Controls Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add allowlisted opencode control commands (`/compact`, `/providers`, `/models`, `/set_models <model>`) that work from CLI/REPL, Web, Telegram, and persist the default model for future Wukong turns.

**Architecture:** Keep slash commands allowlisted in `wukong-cli`, add a small opencode utility runner in `wukong-gateway`, persist the default model in `wukong-settings`, and apply that model when constructing opencode run argv. Web and Telegram reuse the shared command executor so behavior and tests stay consistent.

**Tech Stack:** Rust workspace, Tokio async, existing JSON settings file (`wukong-settings`), opencode CLI subprocess execution, existing Axum Web and Telegram dispatch paths.

---

## File Structure

- Modify `crates/wukong-settings/src/lib.rs`: add persisted agent settings with `default_model`, loader helper, and tests.
- Modify `crates/wukong-gateway/src/backend.rs`: add model-aware argv assembly and opencode utility runner abstraction/functions.
- Modify `crates/wukong-gateway/src/config.rs`: add shared command/model config helpers and tests.
- Modify `crates/wukong-runtime/src/turn.rs`: make session passthrough accept an explicit allowlisted command string.
- Modify `crates/wukong-cli/Cargo.toml`: add `wukong-settings` dependency for `/set_models` persistence.
- Modify `crates/wukong-cli/src/command.rs`: expand `SessionCommand`, parse args, execute commands through settings and utility runner.
- Modify `crates/wukong-cli/src/repl.rs`: classify slash commands with arguments and preserve `/set_models <model>`.
- Modify `crates/wukong-cli/src/main.rs`: wire settings path into REPL command execution and support single-shot slash commands.
- Modify `crates/wukong-web/src/lib.rs`: wire command execution through settings path and preserve chat history behavior.
- Modify `crates/wukong-telegram/src/dispatch.rs`: wire command execution through settings path while preserving allowlist and history.
- Modify `crates/wukong-telegram/src/main.rs`: pass settings path into dispatch.
- Modify `crates/wukong-schedulerd/src/main.rs`: apply persisted default model when building scheduler backend config.
- Add or update tests in each touched crate, using existing mock backend patterns.

## Task 1: Persist Agent Default Model

**Files:**
- Modify: `crates/wukong-settings/src/lib.rs`

- [ ] **Step 1: Write failing settings tests**

Add these tests inside the existing `#[cfg(test)] mod tests` in `crates/wukong-settings/src/lib.rs`:

```rust
#[test]
fn saves_and_loads_agent_default_model() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    let settings = Settings {
        telegram: TelegramSettings::default(),
        agent: AgentSettings {
            default_model: Some("opencode/deepseek-v4-flash-free".to_string()),
        },
    };

    save_settings(&path, &settings).unwrap();
    let loaded = load_settings(&path).unwrap();

    assert_eq!(loaded.agent.default_model.as_deref(), Some("opencode/deepseek-v4-flash-free"));
}

#[test]
fn missing_agent_settings_defaults_cleanly() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("settings.json");
    std::fs::write(&path, r#"{"telegram":{"token":"123:abc","allowed":"42"}}"#).unwrap();

    let loaded = load_settings(&path).unwrap();

    assert_eq!(loaded.telegram.token, "123:abc");
    assert_eq!(loaded.agent.default_model, None);
}

#[test]
fn effective_agent_settings_uses_file_model() {
    let settings = Settings {
        telegram: TelegramSettings::default(),
        agent: AgentSettings {
            default_model: Some("opencode/deepseek-v4-flash-free".to_string()),
        },
    };

    let effective = effective_agent_settings(&settings);

    assert_eq!(effective.default_model.as_deref(), Some("opencode/deepseek-v4-flash-free"));
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wukong-settings`

Expected: FAIL because `AgentSettings`, `Settings.agent`, and `effective_agent_settings` do not exist.

- [ ] **Step 3: Implement agent settings**

Update the top of `crates/wukong-settings/src/lib.rs`:

```rust
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct Settings {
    #[serde(default)]
    pub telegram: TelegramSettings,
    #[serde(default)]
    pub agent: AgentSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct TelegramSettings {
    pub token: String,
    pub allowed: String,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct AgentSettings {
    pub default_model: Option<String>,
}
```

Add this helper after `effective_telegram_settings`:

```rust
pub fn effective_agent_settings(file: &Settings) -> AgentSettings {
    AgentSettings {
        default_model: file
            .agent
            .default_model
            .as_ref()
            .map(|m| m.trim())
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string()),
    }
}
```

Update existing test struct literals in this file to include `agent: AgentSettings::default()`.

- [ ] **Step 4: Run settings tests**

Run: `cargo test -p wukong-settings`

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add crates/wukong-settings/src/lib.rs
git commit -m "feat(settings): persist agent default model"
```

## Task 2: Add Model-Aware Backend Args and Utility Runner

**Files:**
- Modify: `crates/wukong-gateway/src/backend.rs`

- [ ] **Step 1: Write failing backend tests**

Add these tests to `crates/wukong-gateway/src/backend.rs` inside the existing test module:

```rust
#[test]
fn assemble_argv_adds_model_before_prompt() {
    let argv = assemble_argv(
        &["opencode".to_string(), "run".to_string()],
        None,
        false,
        Some("opencode/deepseek-v4-flash-free"),
        "hi",
    );
    assert_eq!(argv, vec!["opencode", "run", "--model", "opencode/deepseek-v4-flash-free", "hi"]);
}

#[test]
fn assemble_argv_replaces_existing_model_flag() {
    let argv = assemble_argv(
        &[
            "opencode".to_string(),
            "run".to_string(),
            "--model".to_string(),
            "old/model".to_string(),
        ],
        Some("ses_x"),
        true,
        Some("new/model"),
        "hi",
    );
    assert_eq!(argv, vec!["opencode", "run", "-s", "ses_x", "--thinking", "--model", "new/model", "hi"]);
}

#[test]
fn opencode_binary_uses_first_base_command_arg() {
    assert_eq!(opencode_binary(&["opencode".to_string(), "run".to_string()]), "opencode");
    assert_eq!(opencode_binary(&["/usr/local/bin/opencode".to_string(), "run".to_string()]), "/usr/local/bin/opencode");
}
```

Update the existing `assemble_argv_plain` and `assemble_argv_with_session_and_thinking` tests to pass `None` for the new model parameter.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wukong-gateway backend::tests::assemble_argv_adds_model_before_prompt backend::tests::assemble_argv_replaces_existing_model_flag backend::tests::opencode_binary_uses_first_base_command_arg`

Expected: FAIL because the function signatures and helpers are not implemented.

- [ ] **Step 3: Implement model argv assembly**

Change `AgentRequest` in `crates/wukong-gateway/src/backend.rs`:

```rust
pub struct AgentRequest {
    pub prompt: String,
    pub session_id: Option<String>,
    pub thinking: bool,
    pub model: Option<String>,
}
```

Replace `assemble_argv` with:

```rust
pub fn assemble_argv(
    command: &[String],
    session_id: Option<&str>,
    thinking: bool,
    model: Option<&str>,
    prompt: &str,
) -> Vec<String> {
    let mut argv: Vec<String> = strip_model_args(command);
    if let Some(id) = session_id {
        argv.push("-s".to_string());
        argv.push(id.to_string());
    }
    if thinking {
        argv.push("--thinking".to_string());
    }
    if let Some(model) = model.map(str::trim).filter(|m| !m.is_empty()) {
        argv.push("--model".to_string());
        argv.push(model.to_string());
    }
    argv.push(prompt.to_string());
    argv
}

fn strip_model_args(command: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut iter = command.iter();
    while let Some(arg) = iter.next() {
        if arg == "--model" || arg == "-m" {
            let _ = iter.next();
            continue;
        }
        if arg.starts_with("--model=") {
            continue;
        }
        out.push(arg.clone());
    }
    out
}

pub fn opencode_binary(command: &[String]) -> &str {
    command.first().map(String::as_str).unwrap_or("opencode")
}
```

Update both `AgentCliBackend` call sites:

```rust
let argv = assemble_argv(
    &self.command,
    req.session_id.as_deref(),
    req.thinking,
    req.model.as_deref(),
    &req.prompt,
);
```

- [ ] **Step 4: Update test mock AgentRequest construction**

Search for `AgentRequest {` in the workspace and add `model: None` to existing mock requests and tests that do not care about model.

Example replacement in tests:

```rust
AgentRequest { prompt, session_id, thinking, model: None }
```

- [ ] **Step 5: Add utility runner helper**

Add this struct and impl near `AgentCliBackend` in `crates/wukong-gateway/src/backend.rs`:

```rust
pub struct OpencodeUtility {
    pub binary: String,
    pub workspace: Option<PathBuf>,
}

impl OpencodeUtility {
    pub fn from_agent_command(command: &[String], workspace: Option<PathBuf>) -> Self {
        Self { binary: opencode_binary(command).to_string(), workspace }
    }

    pub async fn run_fixed(&self, args: &[&str]) -> Result<String, GatewayError> {
        let mut cmd = Command::new(&self.binary);
        cmd.args(args).stdin(Stdio::null());
        if let Some(ws) = &self.workspace {
            cmd.current_dir(ws);
        }
        let output = cmd.output().await?;
        if !output.status.success() {
            return Err(GatewayError::AgentFailed {
                code: output.status.code(),
                stderr: String::from_utf8_lossy(&output.stderr).trim().to_string(),
            });
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }
}
```

- [ ] **Step 6: Run gateway and workspace tests**

Run: `cargo test -p wukong-gateway`

Expected: PASS.

Run: `cargo test`

Expected: PASS after all `AgentRequest` initializers are updated.

- [ ] **Step 7: Commit**

Run:

```bash
git add crates/wukong-gateway/src/backend.rs crates/wukong-*/src/*.rs
git commit -m "feat(gateway): support opencode model override"
```

## Task 3: Make Session Passthrough Explicit

**Files:**
- Modify: `crates/wukong-runtime/src/turn.rs`
- Modify callers in `crates/wukong-cli/src/command.rs`

- [ ] **Step 1: Write failing runtime test**

Add this test in `crates/wukong-runtime/src/turn.rs` test module:

```rust
#[tokio::test]
async fn passthrough_sends_requested_command_to_session() {
    let backend = MockBackend::new(&["ok"]);

    let text = run_turn_session_passthrough(&backend, "ses_42", "/compact").await.unwrap();

    assert_eq!(text, "ok");
    assert_eq!(backend.prompts.lock().unwrap()[0], "/compact");
    assert_eq!(backend.session_ids.lock().unwrap()[0], Some("ses_42".to_string()));
}
```

Update the mock `AgentResponse` creation if needed to include `model: None` in request assertions from Task 2.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p wukong-runtime passthrough_sends_requested_command_to_session`

Expected: FAIL because `run_turn_session_passthrough` still takes only backend and session ID.

- [ ] **Step 3: Implement explicit passthrough command**

Change the helper signature in `crates/wukong-runtime/src/turn.rs`:

```rust
pub async fn run_turn_session_passthrough(
    backend: &impl AiBackend,
    session_id: &str,
    command: &str,
) -> Result<String, WukongError> {
    let resp = backend
        .run_streaming(
            AgentRequest {
                prompt: command.to_string(),
                session_id: Some(session_id.to_string()),
                thinking: false,
                model: None,
            },
            &mut |_| {},
        )
        .await?;
    Ok(resp.text)
}
```

Update the existing `/compact` caller in `crates/wukong-cli/src/command.rs` to pass `"/compact"`.

- [ ] **Step 4: Run runtime and CLI tests**

Run: `cargo test -p wukong-runtime -p wukong-cli`

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add crates/wukong-runtime/src/turn.rs crates/wukong-cli/src/command.rs
git commit -m "refactor(runtime): parameterize session passthrough"
```

## Task 4: Expand Shared Command Parser and Executor

**Files:**
- Modify: `crates/wukong-cli/Cargo.toml`
- Modify: `crates/wukong-cli/src/command.rs`
- Modify: `crates/wukong-cli/src/repl.rs`

- [ ] **Step 1: Write failing parser tests**

Replace or extend `parses_known_commands` in `crates/wukong-cli/src/command.rs`:

```rust
#[test]
fn parses_known_commands() {
    assert_eq!(parse_session_command("new", ""), Some(SessionCommand::New));
    assert_eq!(parse_session_command("compact", ""), Some(SessionCommand::Compact));
    assert_eq!(parse_session_command("providers", ""), Some(SessionCommand::Providers));
    assert_eq!(parse_session_command("models", ""), Some(SessionCommand::Models));
    assert_eq!(
        parse_session_command("set_models", "opencode/deepseek-v4-flash-free"),
        Some(SessionCommand::SetModels("opencode/deepseek-v4-flash-free".to_string()))
    );
    assert_eq!(parse_session_command("model", "gpt"), None);
}
```

Update `classify_line_recognizes_session_commands` in `crates/wukong-cli/src/repl.rs`:

```rust
#[test]
fn classify_line_recognizes_session_commands() {
    assert_eq!(classify_line("/new"), LineAction::Command(SessionCommand::New));
    assert_eq!(classify_line("/compact"), LineAction::Command(SessionCommand::Compact));
    assert_eq!(classify_line("/providers"), LineAction::Command(SessionCommand::Providers));
    assert_eq!(classify_line("/models"), LineAction::Command(SessionCommand::Models));
    assert_eq!(
        classify_line("/set_models opencode/deepseek-v4-flash-free"),
        LineAction::Command(SessionCommand::SetModels("opencode/deepseek-v4-flash-free".to_string()))
    );
    assert_eq!(classify_line("/model gpt"), LineAction::Skip);
}
```

- [ ] **Step 2: Write failing executor tests**

Add these tests to `crates/wukong-cli/src/command.rs`:

```rust
#[tokio::test]
async fn set_models_persists_default_model() {
    let mem = open_memory().await;
    let backend = MockBackend::new(&[]);
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");

    let reply = run_session_command(
        &mem,
        &backend,
        &cfg(),
        &settings_path,
        SessionCommand::SetModels("opencode/deepseek-v4-flash-free".to_string()),
    )
    .await
    .unwrap();

    assert!(reply.contains("opencode/deepseek-v4-flash-free"));
    let saved = wukong_settings::load_settings(&settings_path).unwrap();
    assert_eq!(saved.agent.default_model.as_deref(), Some("opencode/deepseek-v4-flash-free"));
}

#[tokio::test]
async fn set_models_without_model_returns_usage() {
    let mem = open_memory().await;
    let backend = MockBackend::new(&[]);
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");

    let reply = run_session_command(
        &mem,
        &backend,
        &cfg(),
        &settings_path,
        SessionCommand::SetModels(String::new()),
    )
    .await
    .unwrap();

    assert!(reply.contains("用法：/set_models"));
    assert_eq!(wukong_settings::load_settings(&settings_path).unwrap().agent.default_model, None);
}
```

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p wukong-cli parses_known_commands classify_line_recognizes_session_commands set_models_persists_default_model set_models_without_model_returns_usage`

Expected: FAIL because command variants and signatures do not exist.

- [ ] **Step 4: Implement parser changes**

Update `SessionCommand` and parser in `crates/wukong-cli/src/command.rs`:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    New,
    Compact,
    Providers,
    Models,
    SetModels(String),
}

pub fn parse_session_command(name: &str, args: &str) -> Option<SessionCommand> {
    match name {
        "new" => Some(SessionCommand::New),
        "compact" => Some(SessionCommand::Compact),
        "providers" => Some(SessionCommand::Providers),
        "models" => Some(SessionCommand::Models),
        "set_models" => Some(SessionCommand::SetModels(args.trim().to_string())),
        _ => None,
    }
}
```

Update `classify_line` in `crates/wukong-cli/src/repl.rs` so slash commands preserve args:

```rust
} else if let Some(rest) = t.strip_prefix('/') {
    let mut parts = rest.splitn(2, char::is_whitespace);
    let name = parts.next().unwrap_or("");
    let args = parts.next().unwrap_or("").trim();
    match parse_session_command(name, args) {
        Some(cmd) => LineAction::Command(cmd),
        None => LineAction::Skip,
    }
} else {
```

- [ ] **Step 5: Add settings dependency**

Add this dependency in `crates/wukong-cli/Cargo.toml`:

```toml
wukong-settings = { path = "../wukong-settings" }
```

- [ ] **Step 6: Implement command execution signature**

Change `run_session_command` signature in `crates/wukong-cli/src/command.rs`:

```rust
pub async fn run_session_command(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    settings_path: &std::path::Path,
    cmd: SessionCommand,
) -> Result<String, WukongError> {
```

Implement the new match arms:

```rust
SessionCommand::Providers => {
    let util = wukong_gateway::backend::OpencodeUtility::from_agent_command(&cfg.agent_command, wukong_gateway::workspace_dir());
    Ok(util.run_fixed(&["providers", "list"]).await?)
}
SessionCommand::Models => {
    let util = wukong_gateway::backend::OpencodeUtility::from_agent_command(&cfg.agent_command, wukong_gateway::workspace_dir());
    Ok(util.run_fixed(&["models"]).await?)
}
SessionCommand::SetModels(model) => {
    let model = model.trim();
    if model.is_empty() {
        return Ok("用法：/set_models opencode/deepseek-v4-flash-free".to_string());
    }
    let mut settings = wukong_settings::load_settings(settings_path).map_err(|e| WukongError::Backend(wukong_gateway::GatewayError::AgentFailed { code: None, stderr: e.to_string() }))?;
    settings.agent.default_model = Some(model.to_string());
    wukong_settings::save_settings(settings_path, &settings).map_err(|e| WukongError::Backend(wukong_gateway::GatewayError::AgentFailed { code: None, stderr: e.to_string() }))?;
    Ok(format!("🐵 已設定預設模型：{model}"))
}
```

Keep `/new` and `/compact` behavior unchanged except passing `settings_path` through the signature and calling `run_turn_session_passthrough(backend, &id, "/compact")`.

- [ ] **Step 7: Update existing call sites**

Update every `run_session_command(...)` call to pass a settings path:

```rust
let settings_path = wukong_settings::default_settings_path();
let reply = run_session_command(memory, backend, &cfg, &settings_path, cmd).await?;
```

In tests, pass a temp path:

```rust
let settings_path = tempfile::NamedTempFile::new().unwrap().path().to_path_buf();
let reply = run_session_command(&mem, &backend, &cfg(), &settings_path, SessionCommand::Compact).await.unwrap();
```

- [ ] **Step 8: Run CLI tests**

Run: `cargo test -p wukong-cli`

Expected: PASS.

- [ ] **Step 9: Commit**

Run:

```bash
git add crates/wukong-cli/Cargo.toml crates/wukong-cli/src/command.rs crates/wukong-cli/src/repl.rs
git commit -m "feat(cli): add opencode control commands"
```

## Task 5: Apply Persisted Model in Runtime Config

**Files:**
- Modify: `crates/wukong-gateway/src/config.rs`
- Modify: `crates/wukong-runtime/src/turn.rs`
- Modify: `crates/wukong-web/src/main.rs`
- Modify: `crates/wukong-telegram/src/main.rs`
- Modify: `crates/wukong-schedulerd/src/main.rs`
- Modify: `crates/wukong-cli/src/main.rs`

- [ ] **Step 1: Write failing config tests**

Add to `crates/wukong-gateway/src/config.rs` tests:

```rust
#[test]
fn apply_default_model_sets_config_model() {
    let mut cfg = GatewayConfig {
        scope: "global".to_string(),
        db_url: "sqlite://x.db".to_string(),
        agent_command: vec!["opencode".to_string(), "run".to_string()],
        default_model: None,
        thinking: true,
        recall_top_k: 5,
        stream: false,
    };

    cfg.apply_default_model(Some("opencode/deepseek-v4-flash-free"));

    assert_eq!(cfg.default_model.as_deref(), Some("opencode/deepseek-v4-flash-free"));
}
```

- [ ] **Step 2: Write failing runtime model propagation test**

Add to `crates/wukong-runtime/src/turn.rs` tests:

```rust
#[tokio::test]
async fn run_turn_passes_default_model_to_final_step() {
    let mem = open_memory().await;
    let backend = MockBackend::new(&["oracle", "answer"]);
    let mut cfg = cfg();
    cfg.default_model = Some("opencode/deepseek-v4-flash-free".to_string());

    run_turn(&mem, &backend, &cfg, "hello", &mut |_| {}, &mut |_| {}).await.unwrap();

    let models = backend.models.lock().unwrap();
    assert_eq!(models.last().and_then(Clone::clone).as_deref(), Some("opencode/deepseek-v4-flash-free"));
}
```

Update the runtime `MockBackend` to store `req.model` in `models: Mutex<Vec<Option<String>>>`.

- [ ] **Step 3: Run tests to verify failure**

Run: `cargo test -p wukong-gateway apply_default_model_sets_config_model && cargo test -p wukong-runtime run_turn_passes_default_model_to_final_step`

Expected: FAIL because `GatewayConfig.default_model` does not exist and `run_turn` does not pass it.

- [ ] **Step 4: Add model to GatewayConfig**

Update `GatewayConfig` in `crates/wukong-gateway/src/config.rs`:

```rust
pub struct GatewayConfig {
    pub scope: String,
    pub db_url: String,
    pub agent_command: Vec<String>,
    pub default_model: Option<String>,
    pub thinking: bool,
    pub recall_top_k: usize,
    pub stream: bool,
}
```

Set `default_model: None` in `GatewayConfig::resolve` and all struct literals across the workspace.

Add this method:

```rust
impl GatewayConfig {
    pub fn apply_default_model(&mut self, model: Option<&str>) {
        self.default_model = model
            .map(str::trim)
            .filter(|m| !m.is_empty())
            .map(|m| m.to_string());
    }
}
```

- [ ] **Step 5: Pass model into run_turn backend calls**

In `crates/wukong-runtime/src/turn.rs`, update the final-step `AgentRequest` construction:

```rust
AgentRequest {
    prompt,
    session_id,
    thinking: cfg.thinking,
    model: cfg.default_model.clone(),
}
```

For non-turn passthrough calls, keep `model: None`.

- [ ] **Step 6: Load settings model in binaries**

In each binary that creates a `GatewayConfig`, load settings and apply the effective model before constructing `AgentCliBackend` or executing turns:

```rust
let settings_path = wukong_settings::default_settings_path();
let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
let agent_settings = wukong_settings::effective_agent_settings(&settings);
cfg.apply_default_model(agent_settings.default_model.as_deref());
```

Apply this in:

- `crates/wukong-cli/src/main.rs` after `let mut cfg = GatewayConfig::resolve(&cli);`.
- `crates/wukong-web/src/main.rs` when building state/backend config source.
- `crates/wukong-telegram/src/main.rs` when building `base_cfg`.
- `crates/wukong-schedulerd/src/main.rs` in `resolve_config` or immediately after it in `run`.

- [ ] **Step 7: Run tests**

Run: `cargo test -p wukong-gateway -p wukong-runtime -p wukong-schedulerd`

Expected: PASS.

- [ ] **Step 8: Commit**

Run:

```bash
git add crates/wukong-gateway/src/config.rs crates/wukong-runtime/src/turn.rs crates/wukong-cli/src/main.rs crates/wukong-web/src/main.rs crates/wukong-telegram/src/main.rs crates/wukong-schedulerd/src/main.rs
git commit -m "feat(runtime): apply persisted opencode model"
```

## Task 6: Wire Web Command Execution

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Write failing Web tests**

Add tests near existing chat command tests in `crates/wukong-web/src/lib.rs`:

```rust
#[tokio::test]
async fn chat_set_models_command_persists_model_and_records_history() {
    let app_state = state(None, &[]).await;
    let settings_path = app_state.settings_path.clone();
    let db_url = app_state.db_url.clone();
    let app = build_router(app_state);

    let resp = app
        .oneshot(Request::builder().uri("/chat?q=/set_models%20opencode/deepseek-v4-flash-free").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains("已設定預設模型"), "body: {body}");

    let saved = wukong_settings::load_settings(&settings_path).unwrap();
    assert_eq!(saved.agent.default_model.as_deref(), Some("opencode/deepseek-v4-flash-free"));

    let store = ChatHistoryStore::open(&db_url).await.unwrap();
    let messages = store.latest_messages("global", 10).await.unwrap();
    assert!(messages.iter().any(|m| m.role == "user" && m.content.contains("/set_models")));
    assert!(messages.iter().any(|m| m.role == "assistant" && m.content.contains("已設定預設模型")));
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p wukong-web chat_set_models_command_persists_model_and_records_history`

Expected: FAIL until Web passes settings path and the expanded command executor handles `/set_models`.

- [ ] **Step 3: Update Web command execution**

In the slash-command branch in `crates/wukong-web/src/lib.rs`, parse args with `splitn(2, char::is_whitespace)` and call:

```rust
let reply = match wukong_cli::parse_session_command(&name, args) {
    Some(cmd) => match wukong_cli::run_session_command(mem.as_ref(), backend.as_ref(), &cfg, &settings_path, cmd).await {
        Ok(t) => t,
        Err(e) => format!("⚠️ 失敗：{e}"),
    },
    None => format!("指令 /{name} 尚未支援"),
};
```

Move `let settings_path = state.settings_path.clone();` into the thread closure capture before `std::thread::spawn`.

- [ ] **Step 4: Run Web tests**

Run: `cargo test -p wukong-web`

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add crates/wukong-web/src/lib.rs
git commit -m "feat(web): run opencode control commands"
```

## Task 7: Wire Telegram Command Execution

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs`
- Modify: `crates/wukong-telegram/src/main.rs`

- [ ] **Step 1: Write failing Telegram tests**

Add a test in `crates/wukong-telegram/src/dispatch.rs` test module:

```rust
#[tokio::test]
async fn set_models_command_persists_and_replies() {
    let mem = open_memory().await;
    let backend = MockBackend::new(&[]);
    let client = MockClient::default();
    let dir = tempfile::tempdir().unwrap();
    let settings_path = dir.path().join("settings.json");
    let cfg = cfg();

    handle_message(
        &client,
        &mem,
        &cfg,
        &backend,
        None,
        &settings_path,
        &[42],
        &TgMessage { chat_id: 42, text: "/set_models opencode/deepseek-v4-flash-free".to_string() },
    )
    .await;

    let sent = client.sent.lock().unwrap();
    assert!(sent.iter().any(|(_, text)| text.contains("已設定預設模型")));
    drop(sent);

    let saved = wukong_settings::load_settings(&settings_path).unwrap();
    assert_eq!(saved.agent.default_model.as_deref(), Some("opencode/deepseek-v4-flash-free"));
}
```

Adjust names (`MockClient`, sent field) to match the existing Telegram test helpers in the file.

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test -p wukong-telegram set_models_command_persists_and_replies`

Expected: FAIL because `handle_message` does not accept a settings path and parser signature changed.

- [ ] **Step 3: Update Telegram dispatcher**

Change `handle_message` signature to include `settings_path: &std::path::Path` before `allow`:

```rust
pub async fn handle_message<C, B>(
    client: &C,
    mem: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    history: Option<&ChatHistoryStore>,
    settings_path: &std::path::Path,
    allow: &[i64],
    msg: &TgMessage,
) where
```

In the command branch, use `args` from `MessageAction::Command { name, args }`:

```rust
match wukong_cli::parse_session_command(&name, &args) {
    Some(cmd) => {
        let reply = match wukong_cli::run_session_command(mem, backend, &cfg, settings_path, cmd).await {
            Ok(t) => t,
            Err(e) => format!("⚠️ 失敗：{e}"),
        };
```

Update `crates/wukong-telegram/src/main.rs` to create `let settings_path = wukong_settings::default_settings_path();` and pass `&settings_path` to each `handle_message` call.

- [ ] **Step 4: Run Telegram tests**

Run: `cargo test -p wukong-telegram`

Expected: PASS.

- [ ] **Step 5: Commit**

Run:

```bash
git add crates/wukong-telegram/src/dispatch.rs crates/wukong-telegram/src/main.rs
git commit -m "feat(telegram): run opencode control commands"
```

## Task 8: Update Docs and Regression Checks

**Files:**
- Modify: `README.md`
- Add: `scripts/test-opencode-command-controls.sh`

- [ ] **Step 1: Add README command documentation**

Add a short bullet list in the Web Console or Telegram section:

```markdown
### Chat control commands

Across CLI/REPL, Web, and Telegram, Wukong recognizes a small allowlist of control commands:

- `/compact`: ask opencode to compact the current scope's stored session.
- `/providers`: run `opencode providers list` and return the output.
- `/models`: run `opencode models` and return the output.
- `/set_models <model>`: persist a system-wide default model for future Web, Telegram, Scheduler, and CLI turns.

Unknown slash commands are not passed through automatically.
```

- [ ] **Step 2: Add static regression script**

Create `scripts/test-opencode-command-controls.sh`:

```bash
#!/usr/bin/env bash
set -euo pipefail

grep -q 'SessionCommand::Providers' crates/wukong-cli/src/command.rs
grep -q 'SessionCommand::Models' crates/wukong-cli/src/command.rs
grep -q 'SessionCommand::SetModels' crates/wukong-cli/src/command.rs
grep -q 'providers", "list' crates/wukong-cli/src/command.rs
grep -q 'default_model' crates/wukong-settings/src/lib.rs
grep -q 'run_turn_session_passthrough(.*"/compact"' crates/wukong-cli/src/command.rs
grep -q '/set_models' README.md

echo "opencode command control checks passed"
```

Make it executable: `chmod +x scripts/test-opencode-command-controls.sh`.

- [ ] **Step 3: Run docs/regression checks**

Run: `bash scripts/test-opencode-command-controls.sh`

Expected: PASS with `opencode command control checks passed`.

- [ ] **Step 4: Commit**

Run:

```bash
git add README.md scripts/test-opencode-command-controls.sh
git commit -m "docs: describe opencode control commands"
```

## Task 9: Final Verification and Release Prep

**Files:**
- No planned source edits unless verification reveals a defect.

- [ ] **Step 1: Run focused tests**

Run:

```bash
cargo test -p wukong-settings
cargo test -p wukong-gateway
cargo test -p wukong-runtime
cargo test -p wukong-cli
cargo test -p wukong-web
cargo test -p wukong-telegram
cargo test -p wukong-schedulerd
bash scripts/test-opencode-command-controls.sh
```

Expected: all commands exit 0.

- [ ] **Step 2: Run full workspace tests**

Run: `cargo test`

Expected: PASS.

- [ ] **Step 3: Run GitNexus change detection**

Run: `gitnexus_detect_changes({ scope: "all", repo: "Wukong" })`

Expected: command/config/backend flows may be reported; inspect any HIGH/CRITICAL risk before proceeding.

- [ ] **Step 4: Inspect git status and diff**

Run:

```bash
git status --short
git diff --stat HEAD~8..HEAD
git log --oneline -10
```

Expected: clean worktree after task commits; recent commits correspond only to this feature.

- [ ] **Step 5: Decide release tag**

If verification passes, prepare the next patch release, likely `v0.16.6`, with a title in the existing style:

```text
🐵 v0.16.6 — 令牌調度：模型可換 × 指令直通
```

Only tag and push after user approval or if the current execution request explicitly includes release.
