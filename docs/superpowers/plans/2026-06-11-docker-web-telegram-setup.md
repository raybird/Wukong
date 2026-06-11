# Docker Web + Telegram First-Run Setup Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `docker compose up -d` start Web and Telegram by default, with Telegram waiting for Web-provided settings and Docker installing current `opencode-ai` via npm.

**Architecture:** Add a small shared `wukong-settings` crate for `/data/settings.json` parsing and persistence. Web exposes `/settings` plus `/api/settings`; Telegram reads the same settings file and waits without exiting until a token is configured. Docker Compose keeps `wukong` as a passive run-only service via the `cli` profile.

**Tech Stack:** Rust 2021, axum 0.7, tokio, serde/serde_json, Docker Compose, npm `opencode-ai@latest`, vanilla Web Components.

---

## File Structure

- Create `crates/wukong-settings/Cargo.toml`: shared settings crate metadata.
- Create `crates/wukong-settings/src/lib.rs`: settings structs, JSON load/save, env override helpers, default `/data/settings.json` path.
- Modify `Cargo.toml`: add `crates/wukong-settings` to workspace members.
- Modify `crates/wukong-web/Cargo.toml`: depend on `wukong-settings` and `serde_json`.
- Modify `crates/wukong-web/src/main.rs`: pass settings path into `AppState`.
- Modify `crates/wukong-web/src/lib.rs`: add settings API routes and token-gated handlers.
- Create `crates/wukong-web/static/components/wukong-settings.js`: Telegram setup form.
- Modify `crates/wukong-web/static/app.js`: register the settings custom element.
- Modify `crates/wukong-web/static/index.html`: add nav and settings element.
- Modify `crates/wukong-web/static/styles.css`: style nav and settings form.
- Modify `crates/wukong-telegram/Cargo.toml`: depend on `wukong-settings`.
- Modify `crates/wukong-telegram/src/main.rs`: replace immediate token exit with settings wait/reload loop.
- Modify `Dockerfile`: install Node/npm and `opencode-ai@latest`, then verify `opencode --version`.
- Modify `docker-compose.yml`: make Web and Telegram default; put CLI service under profile `cli`.
- Modify `.env.example` and `README.md`: document first-run Web setup and passive CLI usage.

---

### Task 1: Add Shared Settings Crate

**Files:**
- Modify: `Cargo.toml`
- Create: `crates/wukong-settings/Cargo.toml`
- Create: `crates/wukong-settings/src/lib.rs`

- [ ] **Step 1: Write the failing settings tests**

Create `crates/wukong-settings/Cargo.toml`:

```toml
[package]
name = "wukong-settings"
edition.workspace = true
version.workspace = true

[lib]
name = "wukong_settings"
path = "src/lib.rs"

[dependencies]
serde = { workspace = true }
serde_json = { workspace = true }
thiserror = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
```

Create `crates/wukong-settings/src/lib.rs` with tests first:

```rust
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct Settings {
    pub telegram: TelegramSettings,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, Default)]
pub struct TelegramSettings {
    pub token: String,
    pub allowed: String,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("settings io: {0}")]
    Io(#[from] std::io::Error),
    #[error("settings json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, SettingsError>;

pub fn default_settings_path() -> PathBuf {
    std::env::var("WUKONG_SETTINGS_FILE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("/data/settings.json"))
}

pub fn load_settings(_path: &Path) -> Result<Settings> {
    unimplemented!("settings loading is not available yet")
}

pub fn save_settings(_path: &Path, _settings: &Settings) -> Result<()> {
    unimplemented!("settings saving is not available yet")
}

pub fn effective_telegram_settings(file: &Settings) -> TelegramSettings {
    let token = std::env::var("WUKONG_TG_TOKEN").unwrap_or_else(|_| file.telegram.token.clone());
    let allowed = std::env::var("WUKONG_TG_ALLOWED").unwrap_or_else(|_| file.telegram.allowed.clone());
    TelegramSettings { token, allowed }
}

pub fn redact_token(token: &str) -> String {
    if token.is_empty() {
        String::new()
    } else if token.len() <= 8 {
        "********".to_string()
    } else {
        format!("{}…{}", &token[..4], &token[token.len() - 4..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_file_loads_default_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("settings.json");

        let settings = load_settings(&path).unwrap();

        assert_eq!(settings, Settings::default());
    }

    #[test]
    fn saves_and_loads_telegram_settings() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested/settings.json");
        let settings = Settings {
            telegram: TelegramSettings {
                token: "123:abc".to_string(),
                allowed: "42 99".to_string(),
            },
        };

        save_settings(&path, &settings).unwrap();
        let loaded = load_settings(&path).unwrap();

        assert_eq!(loaded, settings);
    }

    #[test]
    fn env_overrides_file_settings() {
        std::env::set_var("WUKONG_TG_TOKEN", "env-token");
        std::env::set_var("WUKONG_TG_ALLOWED", "7");
        let file = Settings {
            telegram: TelegramSettings {
                token: "file-token".to_string(),
                allowed: "42".to_string(),
            },
        };

        let effective = effective_telegram_settings(&file);

        std::env::remove_var("WUKONG_TG_TOKEN");
        std::env::remove_var("WUKONG_TG_ALLOWED");
        assert_eq!(effective.token, "env-token");
        assert_eq!(effective.allowed, "7");
    }

    #[test]
    fn redacts_saved_token_for_api_responses() {
        assert_eq!(redact_token(""), "");
        assert_eq!(redact_token("short"), "********");
        assert_eq!(redact_token("1234567890"), "1234…7890");
    }
}
```

Modify workspace `Cargo.toml` members to include the new crate:

```toml
members = ["crates/wukong-memory", "crates/wukong-memoryd", "crates/wukong-gateway", "crates/wukong-orchestrator", "crates/wukong-skills", "crates/wukong-cli", "crates/wukong-telegram", "crates/wukong-render", "crates/wukong-web", "crates/wukong-settings"]
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wukong-settings`

Expected: FAIL because `load_settings` and `save_settings` hit `not implemented`.

- [ ] **Step 3: Implement settings load/save**

Replace the two unimplemented functions in `crates/wukong-settings/src/lib.rs`:

```rust
pub fn load_settings(path: &Path) -> Result<Settings> {
    match std::fs::read_to_string(path) {
        Ok(raw) => Ok(serde_json::from_str(&raw)?),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Settings::default()),
        Err(e) => Err(e.into()),
    }
}

pub fn save_settings(path: &Path, settings: &Settings) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let raw = serde_json::to_string_pretty(settings)?;
    std::fs::write(path, raw)?;
    Ok(())
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p wukong-settings`

Expected: PASS.

- [ ] **Step 5: Commit this task**

Run:

```bash
git add Cargo.toml crates/wukong-settings
git commit -m "feat: add shared docker settings"
```

---

### Task 2: Add Web Settings API

**Files:**
- Modify: `crates/wukong-web/Cargo.toml`
- Modify: `crates/wukong-web/src/main.rs`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run GitNexus impact before editing Web symbols**

Run impact checks before changing `AppState`, `build_router`, and `main`:

```text
gitnexus_impact({target: "AppState", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
gitnexus_impact({target: "build_router", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
gitnexus_impact({target: "main", direction: "upstream", file_path: "crates/wukong-web/src/main.rs", repo: "Wukong"})
```

Expected: Review risk. If HIGH or CRITICAL, stop and report before editing.

- [ ] **Step 2: Add dependencies**

Modify `crates/wukong-web/Cargo.toml` dependencies:

```toml
wukong-settings = { path = "../wukong-settings" }
serde_json = { workspace = true }
```

- [ ] **Step 3: Write failing API tests**

In `crates/wukong-web/src/lib.rs`, update `AppState` in tests after adding a new `settings_path` field to the struct:

```rust
pub struct AppState<B: AiBackend> {
    pub memory: Arc<Memory>,
    pub backend: Arc<B>,
    pub scope: String,
    pub token: Option<String>,
    pub settings_path: std::path::PathBuf,
}
```

Add `settings_path` to the manual `Clone` implementation:

```rust
settings_path: self.settings_path.clone(),
```

Update test helper `state` and `reasoning_state` with a temp settings path:

```rust
settings_path: tempfile::NamedTempFile::new().unwrap().path().to_path_buf(),
```

Add these tests in the existing `#[cfg(test)] mod tests`:

```rust
#[tokio::test]
async fn settings_get_returns_default_state() {
    let app = build_router(state(None, &[]).await);
    let resp = app
        .oneshot(Request::builder().uri("/api/settings").body(Body::empty()).unwrap())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = body_string(resp).await;
    assert!(body.contains(r#""configured":false"#), "body: {body}");
    assert!(body.contains(r#""allowed":"""#), "body: {body}");
}

#[tokio::test]
async fn settings_post_writes_telegram_settings() {
    let app_state = state(None, &[]).await;
    let settings_path = app_state.settings_path.clone();
    let app = build_router(app_state);
    let body = r#"{"telegram":{"token":"123:abc","allowed":"42 99"}}"#;

    let resp = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/settings")
                .header(axum::http::header::CONTENT_TYPE, "application/json")
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::OK);
    let saved = wukong_settings::load_settings(&settings_path).unwrap();
    assert_eq!(saved.telegram.token, "123:abc");
    assert_eq!(saved.telegram.allowed, "42 99");
}

#[tokio::test]
async fn settings_requires_token_when_set() {
    let app = build_router(state(Some("sekret"), &[]).await);

    let resp = app
        .oneshot(Request::builder().uri("/api/settings").body(Body::empty()).unwrap())
        .await
        .unwrap();

    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
}
```

- [ ] **Step 4: Run tests to verify they fail**

Run: `cargo test -p wukong-web settings_ -- --nocapture`

Expected: FAIL because `/api/settings` route and handlers do not exist.

- [ ] **Step 5: Implement settings API**

Add imports near the top of `crates/wukong-web/src/lib.rs`:

```rust
use axum::Json;
use axum::http::StatusCode;
use wukong_settings::{Settings, TelegramSettings};
```

Add request/response types after `ChatQuery`:

```rust
#[derive(serde::Deserialize)]
struct SettingsQuery {
    token: Option<String>,
}

#[derive(serde::Serialize)]
struct SettingsResponse {
    telegram: TelegramSettingsResponse,
}

#[derive(serde::Serialize)]
struct TelegramSettingsResponse {
    configured: bool,
    token: String,
    allowed: String,
}

#[derive(serde::Deserialize)]
struct SaveSettingsRequest {
    telegram: TelegramSettings,
}
```

Add a shared auth helper:

```rust
fn authorized(expected: &Option<String>, provided: Option<&str>) -> bool {
    match expected {
        Some(t) => provided == Some(t.as_str()),
        None => true,
    }
}
```

Use the helper in `chat` by replacing the existing token check with:

```rust
if !authorized(&state.token, params.token.as_deref()) {
    return axum::http::StatusCode::UNAUTHORIZED.into_response();
}
```

Add handlers:

```rust
async fn get_settings<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    match wukong_settings::load_settings(&state.settings_path) {
        Ok(settings) => {
            let telegram = settings.telegram;
            Json(SettingsResponse {
                telegram: TelegramSettingsResponse {
                    configured: !telegram.token.trim().is_empty(),
                    token: wukong_settings::redact_token(&telegram.token),
                    allowed: telegram.allowed,
                },
            })
            .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

async fn post_settings<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<SettingsQuery>,
    Json(req): Json<SaveSettingsRequest>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;
    if !authorized(&state.token, params.token.as_deref()) {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let settings = Settings { telegram: req.telegram };
    match wukong_settings::save_settings(&state.settings_path, &settings) {
        Ok(()) => StatusCode::OK.into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
```

Add routes in `build_router` before `.with_state(state)`:

```rust
.route("/api/settings", axum::routing::get(get_settings::<B>).post(post_settings::<B>))
```

In `crates/wukong-web/src/main.rs`, set `settings_path` in `AppState`:

```rust
settings_path: wukong_settings::default_settings_path(),
```

- [ ] **Step 6: Run tests to verify they pass**

Run: `cargo test -p wukong-web settings_ chat_requires_token_when_set chat_accepts_matching_token`

Expected: PASS.

- [ ] **Step 7: Commit this task**

Run:

```bash
git add crates/wukong-web/Cargo.toml crates/wukong-web/src/main.rs crates/wukong-web/src/lib.rs
git commit -m "feat: add web settings api"
```

---

### Task 3: Add Web Settings UI

**Files:**
- Create: `crates/wukong-web/static/components/wukong-settings.js`
- Modify: `crates/wukong-web/static/app.js`
- Modify: `crates/wukong-web/static/index.html`
- Modify: `crates/wukong-web/static/styles.css`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Run GitNexus impact before editing static-serving route symbols**

Run:

```text
gitnexus_impact({target: "build_router", direction: "upstream", file_path: "crates/wukong-web/src/lib.rs", repo: "Wukong"})
```

Expected: Review risk. If HIGH or CRITICAL, stop and report before editing.

- [ ] **Step 2: Write failing asset test**

Add a new static include in `crates/wukong-web/src/lib.rs` near other static constants:

```rust
const SETTINGS_JS: &str = include_str!("../static/components/wukong-settings.js");
```

Add a handler near `chat_js`:

```rust
async fn settings_js() -> axum::response::Response { asset(JS, SETTINGS_JS) }
```

Add a test assertion to `serves_static_assets_with_content_types`:

```rust
assert!(content_type(build_router(state(None, &[]).await), "/components/wukong-settings.js")
    .await
    .contains("javascript"));
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test -p wukong-web serves_static_assets_with_content_types`

Expected: FAIL because the file and route are missing.

- [ ] **Step 4: Implement settings asset route**

Create `crates/wukong-web/static/components/wukong-settings.js`:

```javascript
import { html } from '/lib/html.js';

export class WukongSettings extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <section class="settings-card">
        <h2>Telegram 設定</h2>
        <p class="settings-help">輸入 Bot token 與允許的 chat/user ID。儲存後 Telegram 服務會自動開始等待訊息。</p>
        <form id="settings-form" class="settings-form">
          <label>Bot token<input id="tg-token" type="password" autocomplete="off" placeholder="123456:ABC..." /></label>
          <label>Allowed IDs<textarea id="tg-allowed" rows="3" placeholder="例如：123456789 或多個 ID 以空白分隔"></textarea></label>
          <button type="submit">儲存設定</button>
        </form>
        <p id="settings-status" class="settings-status">載入中…</p>
      </section>
    `.toString();
    this.status = this.querySelector('#settings-status');
    this.tokenInput = this.querySelector('#tg-token');
    this.allowedInput = this.querySelector('#tg-allowed');
    this.querySelector('#settings-form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.save();
    });
    this.load();
  }

  tokenParam() {
    return window.WUKONG_TOKEN ? '?token=' + encodeURIComponent(window.WUKONG_TOKEN) : '';
  }

  async load() {
    const resp = await fetch('/api/settings' + this.tokenParam());
    if (!resp.ok) {
      this.status.textContent = '無法讀取設定：HTTP ' + resp.status;
      return;
    }
    const data = await resp.json();
    this.allowedInput.value = data.telegram.allowed || '';
    this.status.textContent = data.telegram.configured
      ? '已設定 token：' + data.telegram.token
      : '尚未設定 Telegram token';
  }

  async save() {
    const body = {
      telegram: {
        token: this.tokenInput.value.trim(),
        allowed: this.allowedInput.value.trim(),
      },
    };
    const resp = await fetch('/api/settings' + this.tokenParam(), {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      body: JSON.stringify(body),
    });
    if (!resp.ok) {
      this.status.textContent = '儲存失敗：HTTP ' + resp.status;
      return;
    }
    this.tokenInput.value = '';
    this.status.textContent = '已儲存。Telegram 服務會自動套用設定。';
    await this.load();
  }
}
```

Modify `crates/wukong-web/src/lib.rs` `build_router`:

```rust
.route("/components/wukong-settings.js", axum::routing::get(settings_js))
```

Modify `crates/wukong-web/static/app.js`:

```javascript
import { WukongChat } from '/components/wukong-chat.js';
import { WukongSettings } from '/components/wukong-settings.js';

customElements.define('wukong-chat', WukongChat);
customElements.define('wukong-settings', WukongSettings);
```

Modify `crates/wukong-web/static/index.html` body:

```html
<header>
  <h1>🐵 悟空</h1>
  <nav><a href="#chat">對話</a><a href="#settings">設定</a></nav>
</header>
<main>
  <section id="chat"><wukong-chat></wukong-chat></section>
  <section id="settings"><wukong-settings></wukong-settings></section>
</main>
```

Append to `crates/wukong-web/static/styles.css`:

```css
header { display: flex; align-items: center; justify-content: space-between; gap: 1rem; }
nav { display: flex; gap: 0.75rem; }
nav a { color: inherit; text-decoration: none; opacity: 0.8; }
nav a:hover { opacity: 1; text-decoration: underline; }
main { display: flex; flex: 1; min-height: 0; }
main > section { flex: 1; min-width: 0; display: flex; flex-direction: column; }
#settings { max-width: 28rem; border-left: 1px solid #8884; }
.settings-card { padding: 1rem; display: flex; flex-direction: column; gap: 0.75rem; }
.settings-card h2 { margin: 0; font-size: 1.1rem; }
.settings-help { margin: 0; opacity: 0.75; line-height: 1.4; }
.settings-form { display: flex; flex-direction: column; gap: 0.75rem; }
.settings-form label { display: flex; flex-direction: column; gap: 0.35rem; font-weight: 600; }
.settings-form input, .settings-form textarea { font: inherit; padding: 0.5rem; border: 1px solid #8886; border-radius: 0.5rem; background: transparent; color: inherit; }
.settings-form button { align-self: flex-start; padding: 0.5rem 1rem; font: inherit; }
.settings-status { margin: 0; opacity: 0.85; }
@media (max-width: 760px) {
  body { height: auto; min-height: 100vh; }
  main { flex-direction: column; }
  #settings { max-width: none; border-left: 0; border-top: 1px solid #8884; }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p wukong-web serves_static_assets_with_content_types index_serves_the_shell`

Expected: PASS.

- [ ] **Step 6: Commit this task**

Run:

```bash
git add crates/wukong-web/src/lib.rs crates/wukong-web/static
git commit -m "feat: add web telegram settings ui"
```

---

### Task 4: Make Telegram Wait for Shared Settings

**Files:**
- Modify: `crates/wukong-telegram/Cargo.toml`
- Modify: `crates/wukong-telegram/src/main.rs`

- [ ] **Step 1: Run GitNexus impact before editing Telegram main**

Run:

```text
gitnexus_impact({target: "main", direction: "upstream", file_path: "crates/wukong-telegram/src/main.rs", repo: "Wukong"})
```

Expected: Review risk. If HIGH or CRITICAL, stop and report before editing.

- [ ] **Step 2: Add dependency**

Modify `crates/wukong-telegram/Cargo.toml` dependencies:

```toml
wukong-settings = { path = "../wukong-settings" }
```

- [ ] **Step 3: Write failing tests for settings resolution**

In `crates/wukong-telegram/src/main.rs`, add helper functions above `main`:

```rust
fn load_effective_telegram_settings() -> wukong_settings::TelegramSettings {
    let path = wukong_settings::default_settings_path();
    let file = wukong_settings::load_settings(&path).unwrap_or_default();
    wukong_settings::effective_telegram_settings(&file)
}

fn has_token(settings: &wukong_settings::TelegramSettings) -> bool {
    !settings.token.trim().is_empty()
}
```

Add tests at the bottom of `crates/wukong-telegram/src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn has_token_rejects_empty_token() {
        let settings = wukong_settings::TelegramSettings {
            token: "   ".to_string(),
            allowed: String::new(),
        };

        assert!(!has_token(&settings));
    }

    #[test]
    fn has_token_accepts_non_empty_token() {
        let settings = wukong_settings::TelegramSettings {
            token: "123:abc".to_string(),
            allowed: String::new(),
        };

        assert!(has_token(&settings));
    }
}
```

- [ ] **Step 4: Run tests to verify they pass before behavior change**

Run: `cargo test -p wukong-telegram has_token`

Expected: PASS. These characterize the helper before rewriting `main` behavior.

- [ ] **Step 5: Replace immediate token exit with wait loop**

Change the top of `main` from immediate env read/exit to initial settings load:

```rust
let mut tg_settings = load_effective_telegram_settings();
while !has_token(&tg_settings) {
    eprintln!("🐵 wukong-telegram 等待設定：請在 Web /settings 填入 Telegram bot token。或設定 WUKONG_TG_TOKEN。每 5 秒重新檢查。");
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    tg_settings = load_effective_telegram_settings();
}
let mut token = tg_settings.token.clone();
let mut allow = parse_allowlist(&tg_settings.allowed);
```

Inside the polling loop, before `client.get_updates(offset).await`, reload settings and restart the client when token changes:

```rust
let latest = load_effective_telegram_settings();
if has_token(&latest) && (latest.token != token || latest.allowed != tg_settings.allowed) {
    eprintln!("🐵 wukong-telegram 偵測到設定更新，套用新的 token/allowlist。");
    token = latest.token.clone();
    allow = parse_allowlist(&latest.allowed);
    tg_settings = latest;
    offset = 0;
}
```

Because `ReqwestTgClient::new(&token)` currently creates an immutable client before the loop, change it to recreate per token update:

```rust
let mut client = ReqwestTgClient::new(&token);
```

And inside the settings-change block add:

```rust
client = ReqwestTgClient::new(&token);
```

Keep the existing allowlist warning after initial parse:

```rust
if allow.is_empty() {
    eprintln!("warning: WUKONG_TG_ALLOWED/shared allowed is empty — all messages will be ignored");
}
```

- [ ] **Step 6: Run Telegram tests**

Run: `cargo test -p wukong-telegram`

Expected: PASS.

- [ ] **Step 7: Commit this task**

Run:

```bash
git add crates/wukong-telegram/Cargo.toml crates/wukong-telegram/src/main.rs
git commit -m "feat: wait for telegram settings"
```

---

### Task 5: Update Dockerfile and Compose Defaults

**Files:**
- Modify: `Dockerfile`
- Modify: `docker-compose.yml`

- [ ] **Step 1: Write verification command before editing**

Run current verification to capture the bug:

```bash
docker compose run --rm wukong opencode --version
```

Expected before this task: may return old `0.0.55` or a non-current version depending on local cache.

- [ ] **Step 2: Install OpenCode via npm**

Modify the runtime dependency install block in `Dockerfile`:

```dockerfile
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates curl git gosu nodejs npm ripgrep fzf && \
    npm install -g opencode-ai@latest && \
    opencode --version && \
    rm -rf /var/lib/apt/lists/* /root/.npm
```

Remove the old `curl -fsSL https://raw.githubusercontent.com/opencode-ai/opencode/refs/heads/main/install | bash` block completely.

- [ ] **Step 3: Make CLI passive in Compose**

In `docker-compose.yml`, add a profile to `wukong` service:

```yaml
    profiles: ["cli"]
```

Keep `wukong-web` and `wukong-telegram` without profiles.

- [ ] **Step 4: Remove obsolete Compose version line**

Delete the top-level line:

```yaml
version: "3.8"
```

This removes the Docker Compose warning seen during verification.

- [ ] **Step 5: Rebuild and verify OpenCode**

Run:

```bash
docker compose build --no-cache wukong
docker compose run --rm wukong opencode --version
```

Expected: build succeeds and version is current npm `opencode-ai`, not `0.0.55`.

- [ ] **Step 6: Verify default Compose service selection**

Run:

```bash
docker compose config --services
```

Expected output includes `wukong-telegram` and `wukong-web`; `wukong` may appear in config output because profiles are config-level metadata. Then run:

```bash
docker compose up -d --build
docker compose ps
```

Expected: `wukong-web` is Up; `wukong-telegram` is Up waiting for settings; `wukong-cli` is not running as a default service.

- [ ] **Step 7: Commit this task**

Run:

```bash
git add Dockerfile docker-compose.yml
git commit -m "feat: simplify docker first run"
```

---

### Task 6: Documentation Updates

**Files:**
- Modify: `.env.example`
- Modify: `README.md`

- [ ] **Step 1: Update `.env.example`**

Change Telegram comments to describe optional env override:

```env
# ── Telegram Bot（選用）──
# 可留空後透過 Web /settings 設定；若填入，env 會優先於 Web 設定。
# WUKONG_TG_TOKEN=your_bot_token_here
# WUKONG_TG_ALLOWED=your_chat_id_here
```

- [ ] **Step 2: Update README Docker quickstart**

In the Docker section, update the quickstart commands to:

```markdown
docker compose up -d

# 開啟 Web Console
open http://localhost:8787/

# CLI / opencode 只在需要時被動執行
docker compose run --rm wukong opencode
docker compose run --rm wukong wukong
```

Add first-run setup text:

```markdown
第一次啟動時，`wukong-telegram` 會保持待命而不是因缺少 token 重啟。開啟 Web Console 的設定區，填入 Telegram bot token 與允許的 chat/user ID 後，Telegram 服務會自動套用設定並開始 long-poll。
```

- [ ] **Step 3: Verify docs mention npm package name**

Search docs for old opencode install expectations:

```bash
rg "0\.0\.55|raw.githubusercontent.com/opencode-ai|npm install -g opencode" README.md .env.example Dockerfile docs/superpowers/specs/2026-06-11-docker-web-telegram-setup-design.md
```

Expected: no stale instruction telling users to install `opencode` npm package or legacy raw script.

- [ ] **Step 4: Commit this task**

Run:

```bash
git add .env.example README.md
git commit -m "docs: update docker first run setup"
```

---

### Task 7: Final Verification and Change Detection

**Files:**
- No source edits unless verification reveals failures.

- [ ] **Step 1: Run Rust tests for affected crates**

Run:

```bash
cargo test -p wukong-settings
cargo test -p wukong-web
cargo test -p wukong-telegram
```

Expected: all PASS.

- [ ] **Step 2: Run Docker verification**

Run:

```bash
docker compose build --no-cache wukong
docker compose run --rm wukong opencode --version
docker compose up -d --build
docker compose ps
docker compose logs wukong-web
docker compose logs wukong-telegram
```

Expected:
- `opencode --version` reports current npm `opencode-ai` version.
- `wukong-web` is Up and logs the Web URL.
- `wukong-telegram` is Up and logs waiting-for-settings when no token is configured.
- `wukong-cli` is not started by default `docker compose up -d`.

- [ ] **Step 3: Verify Web settings writes shared config**

Run:

```bash
curl -sS -X POST http://localhost:8787/api/settings \
  -H 'content-type: application/json' \
  -d '{"telegram":{"token":"test-token","allowed":"123"}}'
docker compose exec wukong-web sh -lc 'test -s /data/settings.json && grep -q test-token /data/settings.json'
```

Expected: POST succeeds and `/data/settings.json` contains the saved token.

- [ ] **Step 4: Run GitNexus change detection before completion**

Run:

```text
gitnexus_detect_changes({scope: "all", repo: "Wukong"})
```

Expected: affected symbols match Docker/Web settings/Telegram startup changes.

- [ ] **Step 5: Inspect git state and diff**

Run:

```bash
git status --short
git diff --stat
```

Expected: only intended files are modified or committed. Do not revert unrelated existing changes such as pre-existing `AGENTS.md` or `CLAUDE.md` modifications.

- [ ] **Step 6: Stop test containers if needed**

Run:

```bash
docker compose stop wukong-web wukong-telegram
```

Expected: containers stop cleanly if the user does not want them running after verification.
