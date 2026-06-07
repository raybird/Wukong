# Web Console (wukong-web) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a browser entry point to Wukong: a zero-build single-page chat served by a new `wukong-web` axum crate that reuses `run_turn`, streams role progress over SSE, and renders the answer as safe HTML.

**Architecture:** New workspace crate `wukong-web` (lib + bin). The lib exposes `AppState<B>` + `build_router<B>` (generic over `AiBackend` for testability). `GET /chat?q=` spawns `run_turn` and pipes its `on_role` callback and final answer through an unbounded channel into an axum SSE stream (`role` → `answer` → `done`/`error`). Markdown → HTML uses a new `wukong_render::to_web_html` (pulldown-cmark `push_html`, raw HTML mapped to text for XSS safety). The frontend is static ES-module files embedded via `include_str!`, following the `raybird/plainvanillaweb` core conventions (SafeHTML, a `<wukong-chat>` custom element).

**Tech Stack:** Rust, axum 0.7, tokio, tokio-stream, pulldown-cmark 0.12, vanilla ES modules + EventSource.

---

### Task 1: `wukong_render::to_web_html`

Render GFM markdown to complete, browser-native, XSS-safe HTML. Raw HTML in the LLM output is mapped to text so `push_html` escapes it.

**Files:**
- Modify: `crates/wukong-render/Cargo.toml`
- Modify: `crates/wukong-render/src/lib.rs`

- [ ] **Step 1: Enable the pulldown-cmark `html` feature for this crate**

The workspace dep is `default-features = false`, so the `html` module (which provides `push_html`) is not compiled. Add the feature for wukong-render only (Cargo features are additive; this does not affect other crates).

In `crates/wukong-render/Cargo.toml` change the `[dependencies]` line:

```toml
[dependencies]
pulldown-cmark = { workspace = true, features = ["html"] }
```

- [ ] **Step 2: Write the failing tests**

Append to the `#[cfg(test)] mod tests` block in `crates/wukong-render/src/lib.rs`:

```rust
    #[test]
    fn web_renders_bold_and_table() {
        let out = to_web_html("**ans**\n\n| a | b |\n| - | - |\n| 1 | 2 |");
        assert!(out.contains("<strong>ans</strong>"));
        assert!(out.contains("<table>"));
        assert!(out.contains("<td>1</td>"));
    }

    #[test]
    fn web_renders_code_block() {
        let out = to_web_html("```\nlet x = 1;\n```");
        assert!(out.contains("<pre><code"));
        assert!(out.contains("let x = 1;"));
    }

    #[test]
    fn web_escapes_raw_html() {
        let out = to_web_html("a <script>alert(1)</script> tag");
        assert!(out.contains("&lt;script&gt;"));
        assert!(!out.contains("<script>"));
    }

    #[test]
    fn web_empty_input_is_empty_string() {
        assert_eq!(to_web_html(""), "");
    }
```

- [ ] **Step 3: Run the tests to verify they fail**

Run: `~/.cargo/bin/cargo test -p wukong-render web_ 2>&1 | tail -20`
Expected: FAIL — `cannot find function to_web_html in this scope`.

- [ ] **Step 4: Implement `to_web_html`**

Add this public function to `crates/wukong-render/src/lib.rs`, directly below `to_telegram_html` (before `render_html`):

```rust
/// Render GFM markdown into complete, browser-native HTML (real `<table>`,
/// `<pre><code>`, lists). Raw HTML in the source is mapped to text so it is
/// escaped by the renderer — this prevents an LLM from injecting `<script>`.
/// Empty input yields an empty string.
pub fn to_web_html(markdown: &str) -> String {
    if markdown.trim().is_empty() {
        return String::new();
    }
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let events = Parser::new_ext(markdown, opts).map(|ev| match ev {
        // Treat any raw HTML as literal text → push_html will escape it.
        Event::Html(t) => Event::Text(t),
        Event::InlineHtml(t) => Event::Text(t),
        other => other,
    });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html.trim().to_string()
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `~/.cargo/bin/cargo test -p wukong-render 2>&1 | tail -20`
Expected: PASS — all wukong-render tests (existing Telegram tests plus the 4 new web tests).

- [ ] **Step 6: Lint**

Run: `~/.cargo/bin/cargo clippy -p wukong-render -- -D warnings 2>&1 | tail -20`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/wukong-render/Cargo.toml crates/wukong-render/src/lib.rs
git commit -m "feat(render): add to_web_html for browser-native safe HTML"
```

---

### Task 2: Scaffold `wukong-web` crate serving `GET /`

Create the crate, register it in the workspace, add the `tokio-stream` workspace dep, and serve the embedded `index.html` at `/`. `AppState`/`build_router` are generic over the backend so tests can inject a mock.

**Files:**
- Modify: `Cargo.toml` (workspace members + `tokio-stream` dep)
- Create: `crates/wukong-web/Cargo.toml`
- Create: `crates/wukong-web/src/lib.rs`
- Create: `crates/wukong-web/static/index.html`

- [ ] **Step 1: Register the crate and add the tokio-stream workspace dependency**

In the root `Cargo.toml`, add `"crates/wukong-web"` to `members`:

```toml
members = ["crates/wukong-memory", "crates/wukong-memoryd", "crates/wukong-gateway", "crates/wukong-orchestrator", "crates/wukong-cli", "crates/wukong-telegram", "crates/wukong-render", "crates/wukong-web"]
```

And add to `[workspace.dependencies]` (after the `pulldown-cmark` line):

```toml
tokio-stream = "0.1"
```

- [ ] **Step 2: Create the crate manifest**

Create `crates/wukong-web/Cargo.toml`:

```toml
[package]
name = "wukong-web"
edition.workspace = true
version.workspace = true

[[bin]]
name = "wukong-web"
path = "src/main.rs"

[lib]
name = "wukong_web"
path = "src/lib.rs"

[dependencies]
wukong-memory = { path = "../wukong-memory" }
wukong-gateway = { path = "../wukong-gateway" }
wukong-cli = { path = "../wukong-cli" }
wukong-orchestrator = { path = "../wukong-orchestrator" }
wukong-render = { path = "../wukong-render" }
axum = { workspace = true }
tokio = { workspace = true }
tokio-stream = { workspace = true }

[dev-dependencies]
tempfile = { workspace = true }
tower = { workspace = true }

[features]
embed = ["wukong-memory/embed", "wukong-cli/embed"]
```

- [ ] **Step 3: Create the embedded index.html**

Create `crates/wukong-web/static/index.html`:

```html
<!DOCTYPE html>
<html lang="zh-Hant">
<head>
  <meta charset="utf-8" />
  <meta name="viewport" content="width=device-width, initial-scale=1" />
  <title>悟空 · Wukong</title>
  <link rel="stylesheet" href="/styles.css" />
  <script>window.WUKONG_TOKEN = null;</script>
  <script type="module" src="/app.js"></script>
</head>
<body>
  <header><h1>🐵 悟空</h1></header>
  <wukong-chat></wukong-chat>
</body>
</html>
```

- [ ] **Step 4: Write the failing test (in lib.rs)**

Create `crates/wukong-web/src/lib.rs` with the lib skeleton plus the first test:

```rust
//! wukong-web: a zero-build browser console for Wukong. Reuses run_turn and
//! streams role progress + the rendered answer over SSE.

use std::sync::Arc;
use wukong_gateway::backend::AiBackend;
use wukong_memory::Memory;

/// Shared router state. Generic over the backend so tests inject a mock.
pub struct AppState<B: AiBackend> {
    pub memory: Arc<Memory>,
    pub backend: Arc<B>,
    pub scope: String,
    pub token: Option<String>,
}

// Manual Clone: Arc fields clone cheaply and B need not be Clone.
impl<B: AiBackend> Clone for AppState<B> {
    fn clone(&self) -> Self {
        Self {
            memory: self.memory.clone(),
            backend: self.backend.clone(),
            scope: self.scope.clone(),
            token: self.token.clone(),
        }
    }
}

const INDEX_HTML: &str = include_str!("../static/index.html");

/// Serve the SPA shell at `/`.
async fn index() -> axum::response::Html<&'static str> {
    axum::response::Html(INDEX_HTML)
}

/// Build the application router from shared state.
pub fn build_router<B>(state: AppState<B>) -> axum::Router
where
    B: AiBackend + Send + Sync + 'static,
{
    axum::Router::new()
        .route("/", axum::routing::get(index::<B>))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use tower::ServiceExt;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<String>>,
    }
    impl MockBackend {
        fn new(r: &[&str]) -> Self {
            Self { replies: Mutex::new(r.iter().map(|s| s.to_string()).collect()) }
        }
    }
    impl AiBackend for MockBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse { text: self.replies.lock().unwrap().pop_front().unwrap_or_default() })
        }
    }

    async fn state(token: Option<&str>, replies: &[&str]) -> AppState<MockBackend> {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        AppState {
            memory: Arc::new(Memory::open(&url).await.unwrap()),
            backend: Arc::new(MockBackend::new(replies)),
            scope: "global".to_string(),
            token: token.map(|s| s.to_string()),
        }
    }

    async fn body_string(resp: axum::response::Response) -> String {
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX).await.unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn index_serves_the_shell() {
        let app = build_router(state(None, &[]).await);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("<wukong-chat>"));
    }
}
```

The handler is registered as `index::<B>` so the function must be generic. Update the `index` fn signature accordingly:

```rust
/// Serve the SPA shell at `/`.
async fn index<B: AiBackend>() -> axum::response::Html<&'static str> {
    axum::response::Html(INDEX_HTML)
}
```

(The `<B>` is unused inside but lets axum infer the state type uniformly; it will be replaced by a stateful version in Task 5. Keep the type parameter.)

- [ ] **Step 5: Create a placeholder bin so the crate builds**

Create `crates/wukong-web/src/main.rs` (real wiring lands in Task 6):

```rust
fn main() {
    eprintln!("wukong-web: not yet wired up (see Task 6)");
    std::process::exit(1);
}
```

- [ ] **Step 6: Run the test to verify it passes**

Run: `~/.cargo/bin/cargo test -p wukong-web index_serves_the_shell 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 7: Lint**

Run: `~/.cargo/bin/cargo clippy -p wukong-web --all-targets -- -D warnings 2>&1 | tail -20`
Expected: no warnings (the unused `B` in `index` is a generic param, not an unused variable, so no warning).

- [ ] **Step 8: Commit**

```bash
git add Cargo.toml crates/wukong-web
git commit -m "feat(web): scaffold wukong-web crate serving the SPA shell"
```

---

### Task 3: Static asset routes + frontend files

Embed and serve `app.js`, `styles.css`, `lib/html.js`, `components/wukong-chat.js` with correct content types, following plainvanillaweb core conventions.

**Files:**
- Create: `crates/wukong-web/static/lib/html.js`
- Create: `crates/wukong-web/static/components/wukong-chat.js`
- Create: `crates/wukong-web/static/app.js`
- Create: `crates/wukong-web/static/styles.css`
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Create `static/lib/html.js` (SafeHTML, adopted from plainvanillaweb)**

```js
// SafeHTML tagged template. Adopted from raybird/plainvanillaweb (lib/html.js):
// values are HTML-escaped unless explicitly marked safe via unsafe().
export class SafeHTML {
  constructor(value) {
    this.value = value;
    this.__isSafe = true;
  }
  toString() {
    return this.value;
  }
}

export function escapeHTML(value) {
  return String(value)
    .replaceAll('&', '&amp;')
    .replaceAll('<', '&lt;')
    .replaceAll('>', '&gt;')
    .replaceAll('"', '&quot;')
    .replaceAll("'", '&#39;');
}

export function unsafe(value) {
  return new SafeHTML(String(value));
}

export function html(strings, ...values) {
  let out = '';
  strings.forEach((str, i) => {
    out += str;
    if (i < values.length) {
      const v = values[i];
      if (v && v.__isSafe) out += v.toString();
      else if (Array.isArray(v)) {
        out += v.map((x) => (x && x.__isSafe ? x.toString() : escapeHTML(x))).join('');
      } else out += escapeHTML(v);
    }
  });
  return new SafeHTML(out);
}
```

- [ ] **Step 2: Create `static/components/wukong-chat.js` (custom element)**

```js
import { html, unsafe, escapeHTML } from '/lib/html.js';

// <wukong-chat>: message log + composer + SSE wiring. Self-contained custom
// element (no router/services), per plainvanillaweb core conventions.
export class WukongChat extends HTMLElement {
  connectedCallback() {
    this.innerHTML = html`
      <div class="log" id="log"></div>
      <form id="form" class="composer">
        <input id="q" type="text" autocomplete="off" placeholder="問悟空…" />
        <button type="submit">送出</button>
      </form>
    `.toString();
    this.log = this.querySelector('#log');
    this.input = this.querySelector('#q');
    this.querySelector('#form').addEventListener('submit', (e) => {
      e.preventDefault();
      this.send();
    });
  }

  bubble(cls, innerHTML) {
    const div = document.createElement('div');
    div.className = 'bubble ' + cls;
    div.innerHTML = innerHTML;
    this.log.appendChild(div);
    this.log.scrollTop = this.log.scrollHeight;
    return div;
  }

  send() {
    const text = this.input.value.trim();
    if (!text) return;
    this.input.value = '';
    // User bubble: input is escaped via the html`` template.
    this.bubble('user', html`${text}`.toString());
    // Single progress bubble, updated in place by role events.
    const progress = this.bubble('status', '🐵 收到，思考中…');

    const tokenParam = window.WUKONG_TOKEN
      ? '&token=' + encodeURIComponent(window.WUKONG_TOKEN)
      : '';
    const es = new EventSource('/chat?q=' + encodeURIComponent(text) + tokenParam);

    es.addEventListener('role', (ev) => {
      progress.innerHTML = '🐵 悟空·' + escapeHTML(ev.data) + ' 思考中…';
    });
    es.addEventListener('answer', (ev) => {
      progress.remove();
      // Server already produced safe HTML; mark it trusted.
      this.bubble('assistant', unsafe(ev.data).toString());
    });
    es.addEventListener('error', (ev) => {
      // EventSource also fires a data-less 'error' on connection close; ignore
      // those and only surface server-sent error events (which carry data).
      if (!ev.data) return;
      progress.remove();
      this.bubble('assistant', '⚠️ ' + escapeHTML(ev.data));
      es.close();
    });
    es.addEventListener('done', () => {
      progress.remove();
      es.close();
    });
  }
}
```

- [ ] **Step 3: Create `static/app.js` (entry point)**

```js
import { WukongChat } from '/components/wukong-chat.js';

customElements.define('wukong-chat', WukongChat);
```

- [ ] **Step 4: Create `static/styles.css`**

```css
:root { color-scheme: light dark; }
body {
  font-family: system-ui, sans-serif;
  margin: 0;
  display: flex;
  flex-direction: column;
  height: 100vh;
}
header { padding: 0.5rem 1rem; border-bottom: 1px solid #8884; }
header h1 { margin: 0; font-size: 1.2rem; }
wukong-chat { display: flex; flex-direction: column; flex: 1; min-height: 0; }
.log { flex: 1; overflow-y: auto; padding: 1rem; display: flex; flex-direction: column; gap: 0.5rem; }
.bubble { max-width: 80%; padding: 0.5rem 0.75rem; border-radius: 0.75rem; white-space: normal; }
.bubble.user { align-self: flex-end; background: #2f6fed; color: #fff; }
.bubble.assistant { align-self: flex-start; background: #8882; }
.bubble.status { align-self: flex-start; opacity: 0.7; font-style: italic; }
.bubble pre { overflow-x: auto; background: #0001; padding: 0.5rem; border-radius: 0.5rem; }
.bubble table { border-collapse: collapse; }
.bubble td, .bubble th { border: 1px solid #8886; padding: 0.2rem 0.5rem; }
.composer { display: flex; gap: 0.5rem; padding: 0.75rem; border-top: 1px solid #8884; }
.composer input { flex: 1; padding: 0.5rem; font-size: 1rem; }
.composer button { padding: 0.5rem 1rem; font-size: 1rem; }
```

- [ ] **Step 5: Write the failing tests for the asset routes**

Add to the `tests` module in `crates/wukong-web/src/lib.rs`:

```rust
    async fn content_type(app: axum::Router, uri: &str) -> String {
        let resp = app
            .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK, "{uri} not 200");
        resp.headers()
            .get(axum::http::header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap()
            .to_string()
    }

    #[tokio::test]
    async fn serves_static_assets_with_content_types() {
        assert!(content_type(build_router(state(None, &[]).await), "/app.js")
            .await
            .contains("javascript"));
        assert!(content_type(build_router(state(None, &[]).await), "/lib/html.js")
            .await
            .contains("javascript"));
        assert!(content_type(build_router(state(None, &[]).await), "/components/wukong-chat.js")
            .await
            .contains("javascript"));
        assert!(content_type(build_router(state(None, &[]).await), "/styles.css")
            .await
            .contains("css"));
    }
```

- [ ] **Step 6: Run the tests to verify they fail**

Run: `~/.cargo/bin/cargo test -p wukong-web serves_static_assets 2>&1 | tail -20`
Expected: FAIL — routes return 404.

- [ ] **Step 7: Implement the asset constants and routes**

In `crates/wukong-web/src/lib.rs`, add the embedded constants below `INDEX_HTML`:

```rust
const APP_JS: &str = include_str!("../static/app.js");
const HTML_JS: &str = include_str!("../static/lib/html.js");
const CHAT_JS: &str = include_str!("../static/components/wukong-chat.js");
const STYLES_CSS: &str = include_str!("../static/styles.css");

/// Build a static-asset response with an explicit content type.
fn asset(content_type: &'static str, body: &'static str) -> axum::response::Response {
    use axum::http::header::CONTENT_TYPE;
    use axum::response::IntoResponse;
    ([(CONTENT_TYPE, content_type)], body).into_response()
}

const JS: &str = "application/javascript";
const CSS: &str = "text/css";

async fn app_js<B: AiBackend>() -> axum::response::Response { asset(JS, APP_JS) }
async fn html_js<B: AiBackend>() -> axum::response::Response { asset(JS, HTML_JS) }
async fn chat_js<B: AiBackend>() -> axum::response::Response { asset(JS, CHAT_JS) }
async fn styles_css<B: AiBackend>() -> axum::response::Response { asset(CSS, STYLES_CSS) }
```

Then extend `build_router` to register them:

```rust
pub fn build_router<B>(state: AppState<B>) -> axum::Router
where
    B: AiBackend + Send + Sync + 'static,
{
    axum::Router::new()
        .route("/", axum::routing::get(index::<B>))
        .route("/app.js", axum::routing::get(app_js::<B>))
        .route("/lib/html.js", axum::routing::get(html_js::<B>))
        .route("/components/wukong-chat.js", axum::routing::get(chat_js::<B>))
        .route("/styles.css", axum::routing::get(styles_css::<B>))
        .with_state(state)
}
```

- [ ] **Step 8: Run the tests to verify they pass**

Run: `~/.cargo/bin/cargo test -p wukong-web 2>&1 | tail -20`
Expected: PASS — `index_serves_the_shell` and `serves_static_assets_with_content_types`.

- [ ] **Step 9: Commit**

```bash
git add crates/wukong-web
git commit -m "feat(web): embed and serve vanilla SPA assets"
```

---

### Task 4: SSE `/chat` handler

`GET /chat?q=` spawns `run_turn`, forwarding role progress and the rendered answer through an unbounded channel into an axum SSE stream.

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Write the failing test**

Add to the `tests` module in `crates/wukong-web/src/lib.rs`:

```rust
    #[tokio::test]
    async fn chat_streams_role_answer_done() {
        // [0] planner -> "oracle" => single Oracle step; [1] execute -> markdown.
        let app = build_router(state(None, &["oracle", "**ans**"]).await);
        let resp = app
            .oneshot(Request::builder().uri("/chat?q=hi").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("event: role"), "missing role event:\n{body}");
        assert!(body.contains("event: answer"), "missing answer event:\n{body}");
        assert!(body.contains("<strong>ans</strong>"), "answer not rendered:\n{body}");
        assert!(body.contains("event: done"), "missing done event:\n{body}");
    }
```

- [ ] **Step 2: Run the test to verify it fails**

Run: `~/.cargo/bin/cargo test -p wukong-web chat_streams 2>&1 | tail -20`
Expected: FAIL — `/chat` route returns 404.

- [ ] **Step 3: Implement the SSE message type and handler**

In `crates/wukong-web/src/lib.rs`, add imports near the top (with the existing `use` lines):

```rust
use axum::extract::{Query, State};
use axum::response::sse::{Event, Sse};
use std::convert::Infallible;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt;
use wukong_gateway::config::GatewayConfig;
use wukong_cli::run_turn;
```

Add the message enum and query type below the asset helpers:

```rust
/// Messages pushed from the turn task to the SSE stream.
enum SseMsg {
    Role(String),
    Answer(String),
    Error(String),
    Done,
}

impl SseMsg {
    fn into_event(self) -> Event {
        match self {
            SseMsg::Role(r) => Event::default().event("role").data(r),
            SseMsg::Answer(h) => Event::default().event("answer").data(h),
            SseMsg::Error(e) => Event::default().event("error").data(e),
            SseMsg::Done => Event::default().event("done").data("ok"),
        }
    }
}

#[derive(serde::Deserialize)]
struct ChatQuery {
    q: Option<String>,
    #[allow(dead_code)]
    token: Option<String>,
}
```

`serde` is needed for `Query` deserialization. Add it to `[dependencies]` in `crates/wukong-web/Cargo.toml`:

```toml
serde = { workspace = true }
```

Add the handler:

```rust
async fn chat<B>(
    State(state): State<AppState<B>>,
    Query(params): Query<ChatQuery>,
) -> axum::response::Response
where
    B: AiBackend + Send + Sync + 'static,
{
    use axum::response::IntoResponse;

    let q = params.q.unwrap_or_default();
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SseMsg>();

    if q.trim().is_empty() {
        let _ = tx.send(SseMsg::Error("空白訊息".to_string()));
        let _ = tx.send(SseMsg::Done);
    } else {
        let mem = state.memory.clone();
        let backend = state.backend.clone();
        let scope = state.scope.clone();
        tokio::spawn(async move {
            let cfg = GatewayConfig {
                scope,
                db_url: String::new(),
                agent_command: vec![],
                continue_args: vec![],
                continue_session: false,
                recall_top_k: 5,
                stream: false,
            };
            let role_tx = tx.clone();
            let result = run_turn(
                mem.as_ref(),
                backend.as_ref(),
                &cfg,
                &q,
                &mut |_| {},
                &mut |role| {
                    let _ = role_tx.send(SseMsg::Role(role.name().to_string()));
                },
            )
            .await;
            match result {
                Ok(out) => {
                    let _ = tx.send(SseMsg::Answer(wukong_render::to_web_html(&out.text)));
                }
                Err(e) => {
                    let _ = tx.send(SseMsg::Error(e.to_string()));
                }
            }
            let _ = tx.send(SseMsg::Done);
        });
    }

    let stream = UnboundedReceiverStream::new(rx)
        .map(|m| Ok::<Event, Infallible>(m.into_event()));
    Sse::new(stream).into_response()
}
```

Register the route in `build_router` (add before `.with_state(state)`):

```rust
        .route("/chat", axum::routing::get(chat::<B>))
```

- [ ] **Step 4: Run the test to verify it passes**

Run: `~/.cargo/bin/cargo test -p wukong-web chat_streams 2>&1 | tail -20`
Expected: PASS.

- [ ] **Step 5: Run the full crate test suite**

Run: `~/.cargo/bin/cargo test -p wukong-web 2>&1 | tail -20`
Expected: PASS — all four tests.

- [ ] **Step 6: Lint**

Run: `~/.cargo/bin/cargo clippy -p wukong-web --all-targets -- -D warnings 2>&1 | tail -20`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/wukong-web
git commit -m "feat(web): stream role progress and rendered answer over SSE"
```

---

### Task 5: Token gate + injection

When `token` is set: reject `/chat` requests without a matching token (401), and inject the token into the served `index.html` so the bundled UI can pass it.

**Files:**
- Modify: `crates/wukong-web/src/lib.rs`

- [ ] **Step 1: Write the failing tests**

Add to the `tests` module:

```rust
    #[tokio::test]
    async fn chat_requires_token_when_set() {
        let app = build_router(state(Some("sekret"), &["oracle", "ans"]).await);
        let resp = app
            .oneshot(Request::builder().uri("/chat?q=hi").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn chat_accepts_matching_token() {
        let app = build_router(state(Some("sekret"), &["oracle", "ans"]).await);
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/chat?q=hi&token=sekret")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn index_injects_token_when_set() {
        let app = build_router(state(Some("sekret"), &[]).await);
        let resp = app
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = body_string(resp).await;
        assert!(body.contains(r#"window.WUKONG_TOKEN = "sekret""#), "token not injected:\n{body}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `~/.cargo/bin/cargo test -p wukong-web token 2>&1 | tail -20`
Expected: FAIL — `chat_requires_token_when_set` gets 200 (no gate), and the injection test fails (index still serves the static `null`).

- [ ] **Step 3: Enforce the token in the `chat` handler**

In `chat`, immediately after `use axum::response::IntoResponse;`, add the gate:

```rust
    if let Some(expected) = &state.token {
        if params.token.as_deref() != Some(expected.as_str()) {
            return axum::http::StatusCode::UNAUTHORIZED.into_response();
        }
    }
```

Remove the `#[allow(dead_code)]` on `ChatQuery::token` (it is now read):

```rust
#[derive(serde::Deserialize)]
struct ChatQuery {
    q: Option<String>,
    token: Option<String>,
}
```

- [ ] **Step 4: Inject the token in the `index` handler**

Replace the `index` handler so it takes state and rewrites the placeholder. Change its signature and body:

```rust
/// Serve the SPA shell at `/`, injecting the token (if configured) so the
/// bundled UI can authenticate.
async fn index<B>(State(state): State<AppState<B>>) -> axum::response::Html<String>
where
    B: AiBackend + Send + Sync + 'static,
{
    let html = match &state.token {
        Some(t) => {
            // Tokens are short opaque strings; escape the two chars that could
            // break out of the JS string literal.
            let safe = t.replace('\\', "\\\\").replace('"', "\\\"");
            INDEX_HTML.replace(
                "window.WUKONG_TOKEN = null;",
                &format!(r#"window.WUKONG_TOKEN = "{safe}";"#),
            )
        }
        None => INDEX_HTML.to_string(),
    };
    axum::response::Html(html)
}
```

`index` is already registered as `index::<B>`; with the `State` extractor it now needs the `B: ... + 'static` bound, which `build_router` already provides.

- [ ] **Step 5: Run the tests to verify they pass**

Run: `~/.cargo/bin/cargo test -p wukong-web 2>&1 | tail -20`
Expected: PASS — all seven tests (`index_serves_the_shell`, `serves_static_assets_with_content_types`, `chat_streams_role_answer_done`, `chat_requires_token_when_set`, `chat_accepts_matching_token`, `index_injects_token_when_set`).

- [ ] **Step 6: Lint**

Run: `~/.cargo/bin/cargo clippy -p wukong-web --all-targets -- -D warnings 2>&1 | tail -20`
Expected: no warnings.

- [ ] **Step 7: Commit**

```bash
git add crates/wukong-web
git commit -m "feat(web): gate /chat with optional token and inject it into the shell"
```

---

### Task 6: `main.rs` env wiring + serve

Wire env config (host/port/token/scope + reused memory/backend env), build the router, and serve. Mirrors `wukong-telegram/src/main.rs` construction.

**Files:**
- Modify: `crates/wukong-web/src/main.rs`

- [ ] **Step 1: Implement `main`**

Replace the placeholder `crates/wukong-web/src/main.rs` with:

```rust
use std::sync::Arc;
use wukong_gateway::backend::AgentCliBackend;
use wukong_memory::Memory;
use wukong_web::{build_router, AppState};

#[tokio::main]
async fn main() {
    let host = std::env::var("WUKONG_WEB_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("WUKONG_WEB_PORT").unwrap_or_else(|_| "8787".to_string());
    let scope = std::env::var("WUKONG_WEB_SCOPE").unwrap_or_else(|_| "global".to_string());
    let token = std::env::var("WUKONG_WEB_TOKEN").ok().filter(|t| !t.is_empty());

    let db_url = std::env::var("WUKONG_MEMORY_DB").unwrap_or_else(|_| {
        let home = std::env::var("HOME").unwrap_or_else(|_| ".".to_string());
        let dir = format!("{home}/.wukong");
        let _ = std::fs::create_dir_all(&dir);
        format!("sqlite://{dir}/memory.db")
    });
    let memory = match Memory::open(&db_url).await {
        Ok(m) => m,
        Err(e) => {
            eprintln!("error: failed to open memory: {e}");
            std::process::exit(1);
        }
    };

    #[cfg(feature = "embed")]
    let memory = if std::env::var("WUKONG_EMBED").as_deref() == Ok("1") {
        match wukong_memory::FastembedBackend::new() {
            Ok(b) => memory.with_embedder(Arc::new(b)),
            Err(e) => {
                eprintln!("🐵 語意層停用（模型載入失敗）：{e}");
                memory
            }
        }
    } else {
        memory
    };

    let memory = match std::env::var("WUKONG_MD_DIR") {
        Ok(dir) if !dir.is_empty() => memory.with_markdown(dir),
        _ => memory,
    };

    let agent_command = std::env::var("WUKONG_AGENT_CMD")
        .ok()
        .map(|s| s.split_whitespace().map(|t| t.to_string()).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]);
    let backend = AgentCliBackend { command: agent_command, continue_args: vec![] };

    let state = AppState {
        memory: Arc::new(memory),
        backend: Arc::new(backend),
        scope,
        token,
    };
    let app = build_router(state);

    let addr = format!("{host}:{port}");
    let listener = match tokio::net::TcpListener::bind(&addr).await {
        Ok(l) => l,
        Err(e) => {
            eprintln!("error: failed to bind {addr}: {e}");
            std::process::exit(1);
        }
    };
    eprintln!("🐵 wukong-web 上線 http://{addr}/");
    if let Err(e) = axum::serve(listener, app).await {
        eprintln!("server error: {e}");
        std::process::exit(1);
    }
}
```

- [ ] **Step 2: Build the binary**

Run: `~/.cargo/bin/cargo build -p wukong-web 2>&1 | tail -20`
Expected: compiles cleanly.

- [ ] **Step 3: Lint the whole crate (incl. bin)**

Run: `~/.cargo/bin/cargo clippy -p wukong-web --all-targets -- -D warnings 2>&1 | tail -20`
Expected: no warnings.

- [ ] **Step 4: Manual browser smoke test**

Run (background): `WUKONG_AGENT_CMD="opencode run" ~/.cargo/bin/cargo run -p wukong-web`
Then open `http://127.0.0.1:8787/` in a browser, send a question, and confirm: the progress bubble updates with the role, then a rendered answer bubble appears (bold/table/code visible). Stop the server afterward (Ctrl-C).

If `opencode` is not available in this environment, note that the manual smoke test is deferred and rely on the automated SSE test (`chat_streams_role_answer_done`) for verification.

- [ ] **Step 5: Commit**

```bash
git add crates/wukong-web/src/main.rs
git commit -m "feat(web): wire env config and serve the console"
```

---

### Task 7: Documentation

Document the web console in the project README(s), consistent with how Telegram is documented.

**Files:**
- Modify: `README.md` (and `README.zh-Hant.md` if it exists — check first)

- [ ] **Step 1: Check which READMEs exist**

Run: `ls README*.md crates/wukong-web/ 2>&1`
Expected: lists the top-level README file(s).

- [ ] **Step 2: Add a Web Console section**

Add a section to the top-level README (match the existing language/format used for the Telegram bot). Use this content, adapting headings to the file's style:

```markdown
### Web Console (wukong-web)

A zero-build browser console. Reuses the same turn engine and memory as the CLI;
streams role progress and the rendered answer over SSE.

Run:

    WUKONG_AGENT_CMD="opencode run" cargo run -p wukong-web

Then open http://127.0.0.1:8787/.

Environment:

- `WUKONG_WEB_HOST` (default `127.0.0.1`)
- `WUKONG_WEB_PORT` (default `8787`)
- `WUKONG_WEB_TOKEN` (optional; when set, the UI and `/chat` require it)
- `WUKONG_WEB_SCOPE` (default `global`)
- Reused: `WUKONG_MEMORY_DB`, `WUKONG_AGENT_CMD`, `WUKONG_MD_DIR`, (feature `embed`) `WUKONG_EMBED`
```

- [ ] **Step 3: Commit**

```bash
git add README*.md
git commit -m "docs: document the wukong-web console"
```

---

### Task 8: Full verification + finish

**Files:** none (verification + branch completion).

- [ ] **Step 1: Run the entire workspace test suite**

Run: `~/.cargo/bin/cargo test --workspace 2>&1 | tail -30`
Expected: all tests pass (existing suite plus the new wukong-render and wukong-web tests).

- [ ] **Step 2: Workspace-wide clippy**

Run: `~/.cargo/bin/cargo clippy --workspace --all-targets -- -D warnings 2>&1 | tail -30`
Expected: no warnings.

- [ ] **Step 3: Finish the branch**

Announce: "I'm using the finishing-a-development-branch skill to complete this work." Then follow superpowers:finishing-a-development-branch (verify tests → present the 4 options → execute the user's choice). Per established Wukong cadence, expect: merge to `main` → push → GitHub release with the next 孫悟空-themed version name.

---

## Self-Review

**Spec coverage:**
- `to_web_html` (pulldown-cmark `push_html`, Html/InlineHtml→Text, empty→"") → Task 1. ✔
- `wukong-web` crate, `AppState<B>` + `build_router<B>` generic, `AgentCliBackend` prod / `MockBackend` test → Tasks 2, 4. ✔
- Routes `GET /`, static assets with content types, `GET /chat?q=` SSE (role/answer/done/error) → Tasks 2, 3, 4. ✔
- `/chat` flow: token check → q → channel → spawn run_turn (scope set, stream=false) → role events → answer via to_web_html → done; error path → Tasks 4, 5. ✔
- Env (`WUKONG_WEB_HOST/PORT/TOKEN/SCOPE` + reused memory/backend env) → Task 6. ✔
- Frontend `static/` per plainvanillaweb core conventions (lib/html.js SafeHTML with attribution, `<wukong-chat>` custom element, app.js entry, styles.css), `include_str!` embedding → Tasks 2, 3. ✔
- Security: localhost default bind, optional token (gate + injection), server-side HTML escaping of raw HTML, frontend input escaping via SafeHTML → Tasks 1, 5, 6 + frontend. ✔
- Error handling: token mismatch → 401, empty q → error event, run_turn failure → error event → Tasks 4, 5. ✔
- Tests: to_web_html unit tests; axum oneshot + MockBackend for `/`, assets, SSE, token → Tasks 1–5. ✔
- Non-goals (no router/PWA/IDB/i18n, multi-session, login, websocket, token streaming) honored — none implemented. ✔

**Placeholder scan:** No TBD/TODO; every code step shows complete code; the only intentional stub (Task 2 `main.rs`) is replaced with full content in Task 6.

**Type consistency:** `AppState<B>` fields (`memory: Arc<Memory>`, `backend: Arc<B>`, `scope: String`, `token: Option<String>`) are consistent across Tasks 2/4/5/6. `build_router<B>` bound `AiBackend + Send + Sync + 'static` is uniform. `SseMsg` variants and `into_event` mapping match the frontend's `role`/`answer`/`error`/`done` listeners. `ChatQuery { q, token }` matches the query string built by the frontend (`?q=…&token=…`). `run_turn` is called with the exact signature from `wukong-cli` (`&Memory`, `&impl AiBackend`, `&GatewayConfig`, `&str`, `on_event`, `on_role`). `to_web_html` signature matches its use in the handler.
