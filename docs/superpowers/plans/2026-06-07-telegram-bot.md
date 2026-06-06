# Telegram bot 進入點(F1)實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `wukong-telegram` bin crate,以原生 long-poll 收 Telegram 訊息、白名單過濾、每 chat 一個 scope,呼叫既有 `run_turn` 處理並回覆,並預留可擴充的 slash 指令分派接縫。

**Architecture:** 純函式(parse/allowlist/scope/classify)+ `TgClient` trait(reqwest 真實 impl + mock 測試)+ `handle_message` dispatch(重用 `wukong_cli::run_turn`)+ `main` long-poll 迴圈。核心對話邏輯零改動。

**Tech Stack:** Rust 2021、tokio、reqwest(rustls-tls)、serde_json、`wukong-cli`/`wukong-memory`/`wukong-gateway`。

**慣例提醒:** cargo 不在 PATH,指令前綴 `. "$HOME/.cargo/env" &&`;測試+commit 串接 `set -o pipefail`;`cargo test` TESTNAME 一次一個。**git commit 訊息只寫功能描述,絕不含任何 AI 署名。**

---

## 檔案結構

- `Cargo.toml`(根,改):`members` 加 `crates/wukong-telegram`;`[workspace.dependencies]` 加 `reqwest`。
- `crates/wukong-telegram/Cargo.toml`(新):crate 清單。
- `crates/wukong-telegram/src/error.rs`(新):`TgError`。
- `crates/wukong-telegram/src/parse.rs`(新):`TgMessage`、`parse_updates`、`highest_update_id`、`parse_allowlist`、`is_allowed`、`scope_for_chat`。
- `crates/wukong-telegram/src/command.rs`(新):`MessageAction`、`classify_message`(slash 指令接縫)。
- `crates/wukong-telegram/src/client.rs`(新):`TgClient` trait、`ReqwestTgClient`、(test)`MockTgClient`。
- `crates/wukong-telegram/src/dispatch.rs`(新):`handle_message`。
- `crates/wukong-telegram/src/lib.rs`(新):模組宣告 + re-export。
- `crates/wukong-telegram/src/main.rs`(新):env 設定 + long-poll 迴圈。
- `crates/wukong-telegram/README.md`(新)、`README.md`(根,改)。

---

## Task 1: crate scaffold + workspace 接線

**Files:**
- Modify: `Cargo.toml`(根)
- Create: `crates/wukong-telegram/Cargo.toml`
- Create: `crates/wukong-telegram/src/error.rs`
- Create: `crates/wukong-telegram/src/lib.rs`
- Create: `crates/wukong-telegram/src/main.rs`

- [ ] **Step 1: 根 Cargo.toml 加 member 與 reqwest 依賴**

把 `members = [...]` 那行的清單尾端加入 `"crates/wukong-telegram"`。在 `[workspace.dependencies]` 區塊加一行:

```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "rustls-tls"] }
```

- [ ] **Step 2: 建 crate Cargo.toml**

建立 `crates/wukong-telegram/Cargo.toml`:

```toml
[package]
name = "wukong-telegram"
edition.workspace = true
version.workspace = true

[[bin]]
name = "wukong-telegram"
path = "src/main.rs"

[lib]
name = "wukong_telegram"
path = "src/lib.rs"

[dependencies]
wukong-memory = { path = "../wukong-memory" }
wukong-gateway = { path = "../wukong-gateway" }
wukong-cli = { path = "../wukong-cli" }
wukong-orchestrator = { path = "../wukong-orchestrator" }
tokio = { workspace = true }
serde_json = { workspace = true }
reqwest = { workspace = true }
thiserror = { workspace = true }

[features]
embed = ["wukong-memory/embed", "wukong-cli/embed"]
```

- [ ] **Step 3: 建 error.rs**

建立 `crates/wukong-telegram/src/error.rs`:

```rust
use thiserror::Error;

/// Errors from the Telegram transport layer.
#[derive(Debug, Error)]
pub enum TgError {
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),
    #[error("telegram api error: {0}")]
    Api(String),
}
```

- [ ] **Step 4: 建 lib.rs(先掛 error)**

建立 `crates/wukong-telegram/src/lib.rs`:

```rust
//! wukong-telegram: Telegram bot entry point over the Wukong turn engine.

pub mod error;

pub use error::TgError;
```

- [ ] **Step 5: 建最小 main.rs(暫時 stub,確保 bin 可編譯)**

建立 `crates/wukong-telegram/src/main.rs`:

```rust
#[tokio::main]
async fn main() {
    eprintln!("wukong-telegram: not yet wired");
}
```

- [ ] **Step 6: 編譯確認**

Run: `. "$HOME/.cargo/env" && cargo build -p wukong-telegram`
Expected: 成功編譯(會抓 reqwest,首次較久)。

- [ ] **Step 7: commit**

```bash
set -o pipefail
git add Cargo.toml crates/wukong-telegram
git commit -m "feat(telegram): scaffold wukong-telegram crate"
```

---

## Task 2: 白名單與 scope 純函式

**Files:**
- Create: `crates/wukong-telegram/src/parse.rs`
- Modify: `crates/wukong-telegram/src/lib.rs`

- [ ] **Step 1: 建 parse.rs 並寫失敗測試(allowlist/scope 部分)**

建立 `crates/wukong-telegram/src/parse.rs`,先放這幾個函式的測試骨架(實作下一步補):

```rust
//! Pure parsing & policy helpers for the Telegram transport (no network).

/// Parse a comma-separated allowlist of chat ids. Whitespace tolerant; empty
/// entries skipped.
pub fn parse_allowlist(s: &str) -> Vec<i64> {
    s.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<i64>().ok())
        .collect()
}

/// Whether a chat id is in the allowlist.
pub fn is_allowed(chat_id: i64, allow: &[i64]) -> bool {
    allow.contains(&chat_id)
}

/// The memory scope for a given chat.
pub fn scope_for_chat(chat_id: i64) -> String {
    format!("user:tg-{chat_id}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_allowlist_handles_spaces_and_empties() {
        assert_eq!(parse_allowlist("12, 34 ,,56"), vec![12, 34, 56]);
        assert!(parse_allowlist("").is_empty());
        assert!(parse_allowlist("  ").is_empty());
    }

    #[test]
    fn is_allowed_checks_membership() {
        assert!(is_allowed(12, &[12, 34]));
        assert!(!is_allowed(99, &[12, 34]));
        assert!(!is_allowed(12, &[]));
    }

    #[test]
    fn scope_for_chat_formats_id() {
        assert_eq!(scope_for_chat(42), "user:tg-42");
        assert_eq!(scope_for_chat(-100), "user:tg--100");
    }
}
```

- [ ] **Step 2: 掛上 lib.rs**

在 `crates/wukong-telegram/src/lib.rs` 加:

```rust
pub mod parse;
```

- [ ] **Step 3: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram parse_allowlist`
Expected: PASS(本步實作即測試,函式已寫;確認三個測試綠)。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-telegram/src/parse.rs crates/wukong-telegram/src/lib.rs
git commit -m "feat(telegram): allowlist and scope helpers"
```

---

## Task 3: 解析 getUpdates 回應

**Files:**
- Modify: `crates/wukong-telegram/src/parse.rs`

- [ ] **Step 1: 寫失敗測試**

在 `crates/wukong-telegram/src/parse.rs` 的 `mod tests` 內新增:

```rust
    #[test]
    fn parse_updates_extracts_text_messages() {
        let json = serde_json::json!({
            "ok": true,
            "result": [
                {"update_id": 10, "message": {"chat": {"id": 12}, "text": "hello"}},
                {"update_id": 11, "message": {"chat": {"id": 34}, "text": "world"}}
            ]
        });
        let msgs = parse_updates(&json);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].update_id, 10);
        assert_eq!(msgs[0].chat_id, 12);
        assert_eq!(msgs[0].text, "hello");
        assert_eq!(msgs[1].chat_id, 34);
    }

    #[test]
    fn parse_updates_skips_non_text_updates() {
        let json = serde_json::json!({
            "result": [
                {"update_id": 1, "message": {"chat": {"id": 5}}},   // no text
                {"update_id": 2, "edited_message": {"chat": {"id": 5}, "text": "x"}}, // not "message"
                {"update_id": 3, "message": {"chat": {"id": 5}, "text": "ok"}}
            ]
        });
        let msgs = parse_updates(&json);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].update_id, 3);
        assert_eq!(msgs[0].text, "ok");
    }

    #[test]
    fn highest_update_id_scans_all_updates() {
        let json = serde_json::json!({
            "result": [
                {"update_id": 7, "message": {"chat": {"id": 5}}},
                {"update_id": 9, "edited_message": {}},
                {"update_id": 8, "message": {"chat": {"id": 5}, "text": "ok"}}
            ]
        });
        // Must advance past ALL updates, even non-text ones, or they re-deliver.
        assert_eq!(highest_update_id(&json), Some(9));
    }

    #[test]
    fn highest_update_id_none_for_empty() {
        let json = serde_json::json!({ "result": [] });
        assert_eq!(highest_update_id(&json), None);
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram parse_updates`
Expected: 編譯失敗(`TgMessage`、`parse_updates`、`highest_update_id` 未定義)。

- [ ] **Step 3: 實作**

在 `crates/wukong-telegram/src/parse.rs` 頂部(`#[cfg(test)]` 之前)加:

```rust
/// A text message extracted from a Telegram update.
#[derive(Debug, Clone, PartialEq)]
pub struct TgMessage {
    pub update_id: i64,
    pub chat_id: i64,
    pub text: String,
}

/// Extract text messages from a getUpdates response. Updates without a
/// top-level `message.text` (edits, photos, etc.) are skipped.
pub fn parse_updates(json: &serde_json::Value) -> Vec<TgMessage> {
    let Some(arr) = json.get("result").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|u| {
            let update_id = u.get("update_id")?.as_i64()?;
            let msg = u.get("message")?;
            let chat_id = msg.get("chat")?.get("id")?.as_i64()?;
            let text = msg.get("text")?.as_str()?.to_string();
            Some(TgMessage { update_id, chat_id, text })
        })
        .collect()
}

/// The highest update_id across ALL updates (any type), used to advance the
/// long-poll offset so non-text updates are not re-delivered forever.
pub fn highest_update_id(json: &serde_json::Value) -> Option<i64> {
    json.get("result")?
        .as_array()?
        .iter()
        .filter_map(|u| u.get("update_id").and_then(|v| v.as_i64()))
        .max()
}
```

- [ ] **Step 4: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram parse_updates`
然後:`cargo test -p wukong-telegram highest_update_id`
Expected: 兩組皆 PASS。

- [ ] **Step 5: commit**

```bash
set -o pipefail
git add crates/wukong-telegram/src/parse.rs
git commit -m "feat(telegram): parse getUpdates responses"
```

---

## Task 4: slash 指令分派接縫

**Files:**
- Create: `crates/wukong-telegram/src/command.rs`
- Modify: `crates/wukong-telegram/src/lib.rs`

- [ ] **Step 1: 建 command.rs 與失敗測試**

建立 `crates/wukong-telegram/src/command.rs`:

```rust
//! Message classification: the seam where future slash commands plug in.
//! v1 only distinguishes "/cmd" (replied as unsupported) from plain turns.
//! Future commands (e.g. /reset, /compact, /model) add arms in the dispatcher
//! without touching this parser.

/// What a received message resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageAction {
    /// A normal conversational turn.
    Turn(String),
    /// A slash command: name (without '/') and the remaining argument string.
    Command { name: String, args: String },
}

/// Classify a raw message body. Leading/trailing whitespace ignored. A body
/// starting with '/' becomes a Command (name = first token without '/',
/// args = the rest); everything else is a Turn.
pub fn classify_message(text: &str) -> MessageAction {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_string();
        let args = parts.next().unwrap_or("").trim().to_string();
        MessageAction::Command { name, args }
    } else {
        MessageAction::Turn(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_a_turn() {
        assert_eq!(classify_message("hello there"), MessageAction::Turn("hello there".to_string()));
    }

    #[test]
    fn slash_becomes_command_with_args() {
        assert_eq!(
            classify_message("/reset now please"),
            MessageAction::Command { name: "reset".to_string(), args: "now please".to_string() }
        );
    }

    #[test]
    fn slash_without_args() {
        assert_eq!(
            classify_message("  /compact  "),
            MessageAction::Command { name: "compact".to_string(), args: String::new() }
        );
    }
}
```

- [ ] **Step 2: 掛上 lib.rs**

在 `crates/wukong-telegram/src/lib.rs` 加:

```rust
pub mod command;
```

- [ ] **Step 3: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram classify`
Expected: 三個測試 PASS。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-telegram/src/command.rs crates/wukong-telegram/src/lib.rs
git commit -m "feat(telegram): message classification seam for slash commands"
```

---

## Task 5: `TgClient` trait + reqwest 實作 + mock

**Files:**
- Create: `crates/wukong-telegram/src/client.rs`
- Modify: `crates/wukong-telegram/src/lib.rs`

- [ ] **Step 1: 建 client.rs(trait + 真實 impl + mock + mock 測試)**

建立 `crates/wukong-telegram/src/client.rs`:

```rust
//! Telegram Bot API client. A trait so the dispatcher is testable without
//! network; ReqwestTgClient is the real long-poll implementation.

use crate::error::TgError;

/// The slice of the Telegram Bot API the bot needs.
pub trait TgClient {
    /// Long-poll for updates starting at `offset` (timeout baked in).
    fn get_updates(
        &self,
        offset: i64,
    ) -> impl std::future::Future<Output = Result<serde_json::Value, TgError>> + Send;
    /// Send a text message to a chat.
    fn send_message(
        &self,
        chat_id: i64,
        text: &str,
    ) -> impl std::future::Future<Output = Result<(), TgError>> + Send;
    /// Send a chat action (e.g. "typing").
    fn send_chat_action(
        &self,
        chat_id: i64,
        action: &str,
    ) -> impl std::future::Future<Output = Result<(), TgError>> + Send;
}

/// Real client over `https://api.telegram.org/bot<token>/`.
#[derive(Clone)]
pub struct ReqwestTgClient {
    http: reqwest::Client,
    base: String,
}

impl ReqwestTgClient {
    pub fn new(token: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        Self { http, base: format!("https://api.telegram.org/bot{token}") }
    }
}

impl TgClient for ReqwestTgClient {
    async fn get_updates(&self, offset: i64) -> Result<serde_json::Value, TgError> {
        let url = format!("{}/getUpdates", self.base);
        let resp = self
            .http
            .get(&url)
            .query(&[("timeout", "30"), ("offset", &offset.to_string())])
            .send()
            .await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), TgError> {
        let url = format!("{}/sendMessage", self.base);
        self.http
            .post(&url)
            .json(&serde_json::json!({ "chat_id": chat_id, "text": text }))
            .send()
            .await?;
        Ok(())
    }

    async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<(), TgError> {
        let url = format!("{}/sendChatAction", self.base);
        self.http
            .post(&url)
            .json(&serde_json::json!({ "chat_id": chat_id, "action": action }))
            .send()
            .await?;
        Ok(())
    }
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// In-memory client: scripts no updates, records every sent message.
    #[derive(Clone, Default)]
    pub struct MockTgClient {
        pub sent: Arc<Mutex<Vec<(i64, String)>>>,
        pub actions: Arc<Mutex<Vec<(i64, String)>>>,
    }

    impl TgClient for MockTgClient {
        async fn get_updates(&self, _offset: i64) -> Result<serde_json::Value, TgError> {
            Ok(serde_json::json!({ "result": [] }))
        }
        async fn send_message(&self, chat_id: i64, text: &str) -> Result<(), TgError> {
            self.sent.lock().unwrap().push((chat_id, text.to_string()));
            Ok(())
        }
        async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<(), TgError> {
            self.actions.lock().unwrap().push((chat_id, action.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn mock_records_sent_messages() {
        let c = MockTgClient::default();
        c.send_message(7, "hi").await.unwrap();
        c.send_chat_action(7, "typing").await.unwrap();
        assert_eq!(c.sent.lock().unwrap()[0], (7, "hi".to_string()));
        assert_eq!(c.actions.lock().unwrap()[0], (7, "typing".to_string()));
    }
}
```

注意:`TgClient` 用 RPITIT(`impl Future + Send`)而非 `async fn`,以便 `handle_message` 內 `tokio::spawn` 旁路任務時 future 為 `Send`。

- [ ] **Step 2: 掛上 lib.rs**

在 `crates/wukong-telegram/src/lib.rs` 加:

```rust
pub mod client;
```

- [ ] **Step 3: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram mock_records_sent_messages`
Expected: PASS。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-telegram/src/client.rs crates/wukong-telegram/src/lib.rs
git commit -m "feat(telegram): TgClient trait with reqwest and mock impls"
```

---

## Task 6: `handle_message` dispatch

**Files:**
- Create: `crates/wukong-telegram/src/dispatch.rs`
- Modify: `crates/wukong-telegram/src/lib.rs`

- [ ] **Step 1: 寫失敗測試**

建立 `crates/wukong-telegram/src/dispatch.rs`,先放測試(實作下一步補)。測試用 `MockTgClient` + 假 `AiBackend` + 暫存 sqlite Memory:

```rust
//! Per-message dispatch: allowlist → classify → run_turn → reply.

use crate::client::TgClient;
use crate::command::{classify_message, MessageAction};
use crate::parse::{is_allowed, scope_for_chat, TgMessage};
use wukong_cli::run_turn;
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_memory::Memory;
use wukong_orchestrator::Role;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::mock::MockTgClient;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
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

    async fn open_memory() -> Memory {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        Memory::open(&url).await.unwrap()
    }

    fn base_cfg() -> GatewayConfig {
        GatewayConfig {
            scope: String::new(),
            db_url: String::new(),
            agent_command: vec![],
            continue_args: vec![],
            continue_session: false,
            recall_top_k: 5,
            stream: false,
        }
    }

    #[tokio::test]
    async fn ignores_messages_outside_allowlist() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle", "answer"]);
        let msg = TgMessage { update_id: 1, chat_id: 999, text: "hi".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;
        // No reply, no work.
        assert!(client.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn turn_runs_and_replies_in_chat_scope() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        // planner -> single role; then execute answer.
        let backend = MockBackend::new(&["oracle", "答案來了"]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "什麼是 BM25".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;

        // Final answer was sent to the right chat.
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|(c, t)| *c == 12 && t == "答案來了"));
        drop(sent);

        // Stored under the per-chat scope.
        let r = mem
            .recall(wukong_memory::RecallQuery {
                query: "BM25".to_string(),
                top_k: 10,
                scope: Some(scope_for_chat(12)),
                mode: wukong_memory::RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("User: 什麼是 BM25")));
    }

    #[tokio::test]
    async fn slash_command_replies_unsupported() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "/reset".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|(c, t)| *c == 12 && t.contains("尚未支援")));
    }
}
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram ignores_messages_outside_allowlist`
Expected: 編譯失敗(`handle_message` 未定義)。

- [ ] **Step 3: 實作 `handle_message`**

在 `crates/wukong-telegram/src/dispatch.rs` 的 `use` 之後、`#[cfg(test)]` 之前加:

```rust
/// Handle one incoming message: enforce the allowlist, classify, run the turn,
/// and reply. Errors are reported to the chat and swallowed (the loop goes on).
/// `C` must be Clone + Send + 'static so a side task can stream role progress.
pub async fn handle_message<C, B>(
    client: &C,
    mem: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    allow: &[i64],
    msg: &TgMessage,
) where
    C: TgClient + Clone + Send + Sync + 'static,
    B: AiBackend,
{
    if !is_allowed(msg.chat_id, allow) {
        return; // silently ignore non-allowlisted chats
    }
    let chat_id = msg.chat_id;
    match classify_message(&msg.text) {
        MessageAction::Command { name, .. } => {
            let _ = client
                .send_message(chat_id, &format!("指令 /{name} 尚未支援"))
                .await;
        }
        MessageAction::Turn(input) => {
            let mut cfg = base_cfg.clone();
            cfg.scope = scope_for_chat(chat_id);

            // Stream role progress from a side task so the sync on_role callback
            // never blocks on network I/O.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Role>();
            let progress = {
                let c = client.clone();
                tokio::spawn(async move {
                    while let Some(role) = rx.recv().await {
                        let _ = c.send_chat_action(chat_id, "typing").await;
                        let _ = c.send_message(chat_id, &format!("🐵 悟空·{}", role.name())).await;
                    }
                })
            };

            let _ = client.send_chat_action(chat_id, "typing").await;
            let result = run_turn(mem, backend, &cfg, &input, &mut |_| {}, &mut |r| {
                let _ = tx.send(r);
            })
            .await;
            drop(tx);
            let _ = progress.await;

            match result {
                Ok(out) => {
                    let _ = client.send_message(chat_id, &out.text).await;
                }
                Err(e) => {
                    let _ = client.send_message(chat_id, &format!("⚠️ 處理失敗：{e}")).await;
                }
            }
        }
    }
}
```

- [ ] **Step 4: 掛上 lib.rs**

在 `crates/wukong-telegram/src/lib.rs` 加:

```rust
pub mod dispatch;
```

- [ ] **Step 5: 跑全 crate 測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram`
Expected: 全綠(三個 dispatch 測試 + 前面純函式/mock 測試)。

注意:`turn_runs_and_replies_in_chat_scope` 用 `MockBackend`(只實作 `run`);`run_turn` 內以 `run_streaming` 預設實作呼叫 `run`,故腳本 `["oracle","答案來了"]` = 1 plan + 1 execute。

- [ ] **Step 6: commit**

```bash
set -o pipefail
git add crates/wukong-telegram/src/dispatch.rs crates/wukong-telegram/src/lib.rs
git commit -m "feat(telegram): per-message dispatch over run_turn"
```

---

## Task 7: `main` long-poll 迴圈與設定

**Files:**
- Modify: `crates/wukong-telegram/src/main.rs`

- [ ] **Step 1: 改寫 main.rs**

把 `crates/wukong-telegram/src/main.rs` 整檔替換為:

```rust
use std::sync::Arc;
use wukong_gateway::backend::AgentCliBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_memory::Memory;
use wukong_telegram::client::{ReqwestTgClient, TgClient};
use wukong_telegram::dispatch::handle_message;
use wukong_telegram::parse::{highest_update_id, parse_allowlist, parse_updates};

#[tokio::main]
async fn main() {
    let token = match std::env::var("WUKONG_TG_TOKEN") {
        Ok(t) if !t.is_empty() => t,
        _ => {
            eprintln!("error: WUKONG_TG_TOKEN is required");
            std::process::exit(1);
        }
    };
    let allow = parse_allowlist(&std::env::var("WUKONG_TG_ALLOWED").unwrap_or_default());
    if allow.is_empty() {
        eprintln!("warning: WUKONG_TG_ALLOWED is empty — all messages will be ignored");
    }

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
    let memory = Arc::new(memory);

    let agent_command = std::env::var("WUKONG_AGENT_CMD")
        .ok()
        .map(|s| s.split_whitespace().map(|t| t.to_string()).collect::<Vec<_>>())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]);
    let backend = AgentCliBackend { command: agent_command, continue_args: vec![] };

    let base_cfg = GatewayConfig {
        scope: String::new(),
        db_url,
        agent_command: vec![],
        continue_args: vec![],
        continue_session: false,
        recall_top_k: 5,
        stream: false,
    };

    let client = ReqwestTgClient::new(&token);
    eprintln!("🐵 wukong-telegram 上線（long-poll）。允許 {} 個 chat。", allow.len());

    let mut offset: i64 = 0;
    loop {
        match client.get_updates(offset).await {
            Ok(json) => {
                if let Some(max) = highest_update_id(&json) {
                    offset = max + 1;
                }
                for msg in parse_updates(&json) {
                    handle_message(&client, &memory, &base_cfg, &backend, &allow, &msg).await;
                }
            }
            Err(e) => {
                eprintln!("get_updates error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
}
```

注意:`Arc<Memory>` deref 成 `&Memory` 傳給 `handle_message`(`&memory` 為 `&Arc<Memory>`,需 `&*memory` 或直接傳 `&memory` 靠 deref coercion;若型別不符改 `handle_message(&client, &memory, ...)` → `handle_message(&client, &*memory, ...)`)。

- [ ] **Step 2: 編譯確認**

Run: `. "$HOME/.cargo/env" && cargo build -p wukong-telegram`
Expected: 成功編譯。若 `&memory`(`&Arc<Memory>`)型別不符,改傳 `&*memory`。

- [ ] **Step 3: 全 workspace 測試**

Run: `. "$HOME/.cargo/env" && cargo test`
Expected: 全綠(既有 + 新 telegram 測試)。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-telegram/src/main.rs
git commit -m "feat(telegram): long-poll loop and env configuration"
```

---

## Task 8: clippy、文件、手動煙霧

**Files:**
- Create: `crates/wukong-telegram/README.md`
- Modify: `README.md`(根)

- [ ] **Step 1: clippy 全綠**

Run: `. "$HOME/.cargo/env" && cargo clippy --all-targets -- -D warnings`
Expected:零警告。若有,逐一修正後再跑。

- [ ] **Step 2: 建 crate README**

建立 `crates/wukong-telegram/README.md`,涵蓋:用途(turn engine 的 Telegram 進入點)、env(`WUKONG_TG_TOKEN`、`WUKONG_TG_ALLOWED`、重用 `WUKONG_MEMORY_DB`/`WUKONG_AGENT_CMD`/`WUKONG_MD_DIR`/`WUKONG_EMBED`)、啟動方式、每 chat scope = `user:tg-<id>`、slash 指令接縫(v1 回「尚未支援」,未來加 `/reset`/`/compact`/`/model`)、long-poll/白名單/僅純文字等非目標。

- [ ] **Step 3: 更新根 README**

- 「記憶服務（選用）」附近或新增「Telegram bot（選用）」段:啟動方式與 env。
- Roadmap:把 F 的 Telegram 部分標記完成(`✅`),註明 Web Console 仍待做。
- 架構圖/四柱列表處可點出新增 `wukong-telegram`(第 5 個 crate,進入點層)。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add README.md crates/wukong-telegram/README.md
git commit -m "docs: document wukong-telegram bot entry point"
```

- [ ] **Step 5: 手動真實煙霧(需 token,非 CI)**

在 BotFather 建 bot 取得 token;用 `@userinfobot` 取自己的 chat id。

```bash
. "$HOME/.cargo/env"
export WUKONG_TG_TOKEN="<bot token>"
export WUKONG_TG_ALLOWED="<你的 chat id>"
export WUKONG_MEMORY_DB="sqlite:///tmp/tg-smoke.db"
cargo run -p wukong-telegram
```
自手機傳「什麼是 BM25？」→ 應看到 typing + `🐵 悟空·<role>` 狀態 + 最終答案;傳「/reset」→ 回「指令 /reset 尚未支援」;用非白名單帳號傳訊 → 無回應。Ctrl-C 結束。

---

## 完成後

依 `superpowers:finishing-a-development-branch`:跑全測試 → 呈現 4 選項。合併後比照慣例詢問是否開 **v0.6.0 release**(Telegram bot)。

## 自我複查紀錄

- **Spec 覆蓋:** 純函式(T2/T3)、classify_message 指令接縫(T4)、TgClient trait + reqwest + mock(T5)、handle_message dispatch 含白名單/scope/指令/錯誤(T6)、main long-poll + env + 安全預設(T7)、文件 + 手動煙霧(T8)。spec 各節皆有對應 task。
- **型別一致:** `TgMessage{update_id,chat_id,text}`(T3 定義,T6 使用)、`MessageAction::{Turn,Command{name,args}}`(T4 定義,T6 使用)、`TgClient`(T5 定義,T6/T7 使用)、`handle_message<C,B>`(T6 定義,T7 呼叫)、`scope_for_chat`/`is_allowed`/`parse_allowlist`/`parse_updates`/`highest_update_id`(T2/T3 定義,T6/T7 使用)。`GatewayConfig` 欄位與既有定義一致(scope/db_url/agent_command/continue_args/continue_session/recall_top_k/stream)。
- **重用:** `wukong_cli::run_turn` 簽章 `(mem, backend, cfg, input, on_event, on_role)` 完全相符;`stream:false` 故 `on_event` 忽略。
- **安全/擴充:** 缺 token 退出、空白名單警告且忽略全部、白名單外靜默忽略;slash 指令第一版佔位,未來只在 T6 的 `Command` 分支加臂。
- **離線可測:** 全部單元/整合測試走 MockTgClient + MockBackend + 暫存 sqlite,不碰網路;真實 Telegram 為手動煙霧。
