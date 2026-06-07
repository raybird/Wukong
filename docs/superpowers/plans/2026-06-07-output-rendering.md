# 輸出渲染(wukong-render)+ Telegram 訊息整併實作計畫

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 新增 `wukong-render` crate 把 GFM markdown 轉成 Telegram HTML(切段、表格降級、跳脫),並把 `wukong-telegram` 的多泡泡訊息整併為「單一狀態泡泡原地更新 → 刪除 → 發渲染答案」。

**Architecture:** 純函式渲染 crate(pulldown-cmark)+ `TgClient` 擴充(message_id/edit/delete/HTML)+ dispatch 訊息流改寫。離線全可測。

**Tech Stack:** Rust 2021、pulldown-cmark、tokio、reqwest、`wukong-cli`/`wukong-memory`/`wukong-gateway`。

**慣例提醒:** cargo 不在 PATH,前綴 `. "$HOME/.cargo/env" &&`;測試+commit `set -o pipefail`;`cargo test` TESTNAME 一次一個;測試持 MutexGuard 跨 await 用 block scope(非 drop)。**git commit 訊息只寫功能描述,絕不含 AI 署名。** 本計畫接在 `feat/telegram-progress` 分支(已含即時 ack + 持續 typing)。

---

## 檔案結構

- `Cargo.toml`(根,改):`members` 加 `crates/wukong-render`;`[workspace.dependencies]` 加 `pulldown-cmark`。
- `crates/wukong-render/Cargo.toml`(新)、`crates/wukong-render/src/lib.rs`(新):`to_telegram_html` + escape/表格/切段。
- `crates/wukong-telegram/Cargo.toml`(改):依賴 `wukong-render`。
- `crates/wukong-telegram/src/client.rs`(改):`TgClient` 擴充 + reqwest + mock。
- `crates/wukong-telegram/src/dispatch.rs`(改):訊息流整併。
- `crates/wukong-render/README.md`(新)、`crates/wukong-telegram/README.md`(改)、`README.md`(根,改)。

---

## Task 1: `wukong-render` crate scaffold + escape

**Files:**
- Modify: `Cargo.toml`(根)
- Create: `crates/wukong-render/Cargo.toml`
- Create: `crates/wukong-render/src/lib.rs`

- [ ] **Step 1: 根 Cargo.toml 加 member 與 pulldown-cmark**

把 `members = [...]` 清單尾端加入 `"crates/wukong-render"`。在 `[workspace.dependencies]` 加:

```toml
pulldown-cmark = { version = "0.12", default-features = false }
```

- [ ] **Step 2: 建 crate Cargo.toml**

建立 `crates/wukong-render/Cargo.toml`:

```toml
[package]
name = "wukong-render"
edition.workspace = true
version.workspace = true

[lib]
name = "wukong_render"
path = "src/lib.rs"

[dependencies]
pulldown-cmark = { workspace = true }
```

- [ ] **Step 3: 建 lib.rs 與 escape 失敗測試**

建立 `crates/wukong-render/src/lib.rs`:

```rust
//! wukong-render: render LLM markdown into transport-specific formats.
//! Telegram now (HTML subset); web (to_web_html) reserved for later.

/// Escape the three characters Telegram's HTML parse_mode is sensitive to.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_replaces_specials() {
        assert_eq!(escape_html("a < b & c > d"), "a &lt; b &amp; c &gt; d");
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
    }
}
```

- [ ] **Step 4: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-render escape_html`
Expected: PASS（本步即實作 escape 並測試）。

- [ ] **Step 5: commit**

```bash
set -o pipefail
git add Cargo.toml crates/wukong-render
git commit -m "feat(render): scaffold wukong-render crate with html escape"
```

---

## Task 2: 行內與區塊映射(`to_telegram_html`,未切段)

**Files:**
- Modify: `crates/wukong-render/src/lib.rs`

- [ ] **Step 1: 寫失敗測試**

在 `crates/wukong-render/src/lib.rs` 的 `mod tests` 內新增:

```rust
    #[test]
    fn renders_bold_italic_inline_code() {
        let out = to_telegram_html("**bold** and *it* and `co`").join("");
        assert!(out.contains("<b>bold</b>"));
        assert!(out.contains("<i>it</i>"));
        assert!(out.contains("<code>co</code>"));
    }

    #[test]
    fn renders_code_block_as_pre() {
        let out = to_telegram_html("```\nlet x = 1;\n```").join("");
        assert!(out.contains("<pre>"));
        assert!(out.contains("let x = 1;"));
        assert!(out.contains("</pre>"));
    }

    #[test]
    fn renders_heading_as_bold() {
        let out = to_telegram_html("# Title").join("");
        assert!(out.contains("<b>Title</b>"));
    }

    #[test]
    fn renders_link() {
        let out = to_telegram_html("[docs](https://x.io)").join("");
        assert!(out.contains(r#"<a href="https://x.io">docs</a>"#));
    }

    #[test]
    fn renders_list_items_with_bullets() {
        let out = to_telegram_html("- one\n- two").join("");
        assert!(out.contains("• one"));
        assert!(out.contains("• two"));
    }

    #[test]
    fn escapes_text_content() {
        let out = to_telegram_html("a <script> tag").join("");
        assert!(out.contains("&lt;script&gt;"));
        assert!(!out.contains("<script>"));
    }

    #[test]
    fn empty_input_yields_empty_vec() {
        assert!(to_telegram_html("").is_empty());
    }
```

- [ ] **Step 2: 跑測試確認失敗**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-render renders_bold_italic_inline_code`
Expected: 編譯失敗（`to_telegram_html` 未定義）。

- [ ] **Step 3: 實作 `to_telegram_html`（先不切段，回單元素 Vec）**

在 `crates/wukong-render/src/lib.rs` 的 `escape_html` 之後加:

```rust
use pulldown_cmark::{CodeBlockKind, Event, Options, Parser, Tag, TagEnd};

/// Render GFM markdown into Telegram-supported HTML, split into chunks of at
/// most 4096 chars. Empty input yields an empty Vec.
pub fn to_telegram_html(markdown: &str) -> Vec<String> {
    if markdown.trim().is_empty() {
        return Vec::new();
    }
    let html = render_html(markdown);
    let trimmed = html.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    split_chunks(trimmed, 4096)
}

/// Walk markdown events and emit a Telegram HTML-subset string.
fn render_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, opts);

    let mut out = String::new();
    let mut link_url = String::new();
    // Table accumulation state.
    let mut in_table = false;
    let mut table: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();

    for ev in parser {
        // While inside a table, capture cell text and render at table end.
        if in_table {
            match ev {
                Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => row = Vec::new(),
                Event::Start(Tag::TableCell) => cell = String::new(),
                Event::End(TagEnd::TableCell) => row.push(cell.trim().to_string()),
                Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                    table.push(std::mem::take(&mut row))
                }
                Event::End(TagEnd::Table) => {
                    out.push_str(&render_table(&table));
                    in_table = false;
                    table.clear();
                }
                Event::Text(t) | Event::Code(t) => cell.push_str(&t),
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::Strong) => out.push_str("<b>"),
            Event::End(TagEnd::Strong) => out.push_str("</b>"),
            Event::Start(Tag::Emphasis) => out.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => out.push_str("</i>"),
            Event::Start(Tag::Strikethrough) => out.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => out.push_str("</s>"),
            Event::Start(Tag::Heading { .. }) => out.push_str("<b>"),
            Event::End(TagEnd::Heading(_)) => out.push_str("</b>\n"),
            Event::Start(Tag::BlockQuote(_)) => out.push_str("<blockquote>"),
            Event::End(TagEnd::BlockQuote(_)) => out.push_str("</blockquote>\n"),
            Event::Start(Tag::CodeBlock(_)) => out.push_str("<pre>"),
            Event::End(TagEnd::CodeBlock) => out.push_str("</pre>\n"),
            Event::Start(Tag::Item) => out.push_str("• "),
            Event::End(TagEnd::Item) => out.push('\n'),
            Event::Start(Tag::Link { dest_url, .. }) => {
                link_url = dest_url.to_string();
                out.push_str(&format!(r#"<a href="{}">"#, escape_html(&link_url)));
            }
            Event::End(TagEnd::Link) => out.push_str("</a>"),
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                table.clear();
            }
            Event::End(TagEnd::Paragraph) => out.push_str("\n\n"),
            Event::Code(t) => out.push_str(&format!("<code>{}</code>", escape_html(&t))),
            Event::Text(t) => out.push_str(&escape_html(&t)),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Rule => out.push_str("——————\n"),
            _ => {}
        }
    }
    out
}

/// Render an accumulated table as an aligned monospace <pre> block.
fn render_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; cols];
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let mut s = String::from("<pre>");
    for r in rows {
        let mut line = String::new();
        for i in 0..cols {
            let cellv = r.get(i).map(|x| x.as_str()).unwrap_or("");
            let pad = widths[i].saturating_sub(cellv.chars().count());
            line.push_str(cellv);
            line.push_str(&" ".repeat(pad));
            if i + 1 < cols {
                line.push_str("  ");
            }
        }
        s.push_str(&escape_html(line.trim_end()));
        s.push('\n');
    }
    s.push_str("</pre>\n");
    s
}

/// Split rendered HTML into chunks ≤ max chars, breaking on newline boundaries
/// so HTML tags are never split mid-tag.
fn split_chunks(html: &str, max: usize) -> Vec<String> {
    if html.len() <= max {
        return vec![html.to_string()];
    }
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for line in html.split_inclusive('\n') {
        if cur.len() + line.len() > max && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur).trim_end().to_string());
        }
        // A single over-long line: hard-split by chars.
        if line.len() > max {
            let mut rest = line;
            while rest.len() > max {
                let (a, b) = rest.split_at(max);
                chunks.push(a.to_string());
                rest = b;
            }
            cur.push_str(rest);
        } else {
            cur.push_str(line);
        }
    }
    if !cur.trim().is_empty() {
        chunks.push(cur.trim_end().to_string());
    }
    chunks
}
```

注意:`split_at(max)` 以 byte 切,對含多位元組字元的超長單行可能切在字元中間。本專案答案以中英文段落為主、極少出現無換行的 >4096 單行;若日後需要可改字元邊界切。本版接受此限制(已在非目標註明渲染近似)。

- [ ] **Step 4: 跑測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-render`
Expected: 全綠(Task 1+2 測試)。逐一驗:`renders_bold_italic_inline_code`、`renders_code_block_as_pre`、`renders_heading_as_bold`、`renders_link`、`renders_list_items_with_bullets`、`escapes_text_content`、`empty_input_yields_empty_vec`。

- [ ] **Step 5: commit**

```bash
set -o pipefail
git add crates/wukong-render/src/lib.rs
git commit -m "feat(render): GFM to Telegram HTML mapping"
```

---

## Task 3: 表格降級與切段測試

**Files:**
- Modify: `crates/wukong-render/src/lib.rs`

- [ ] **Step 1: 寫失敗測試**

在 `crates/wukong-render/src/lib.rs` 的 `mod tests` 內新增:

```rust
    #[test]
    fn renders_table_as_pre_block() {
        let md = "| a | b |\n| - | - |\n| 1 | 2 |";
        let out = to_telegram_html(md).join("");
        assert!(out.contains("<pre>"));
        assert!(out.contains('a') && out.contains('b'));
        assert!(out.contains('1') && out.contains('2'));
        assert!(out.contains("</pre>"));
    }

    #[test]
    fn long_output_splits_into_multiple_chunks() {
        // 200 lines of ~50 chars => well over 4096.
        let md = (0..200).map(|i| format!("line number {i} with some words")).collect::<Vec<_>>().join("\n\n");
        let chunks = to_telegram_html(&md);
        assert!(chunks.len() > 1, "expected multiple chunks, got {}", chunks.len());
        assert!(chunks.iter().all(|c| c.len() <= 4096));
    }
```

- [ ] **Step 2: 跑測試**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-render renders_table_as_pre_block`
然後:`cargo test -p wukong-render long_output_splits_into_multiple_chunks`
Expected: 兩者 PASS(表格與切段在 Task 2 已實作;本任務補測試固化行為)。若失敗,檢查 Task 2 的 `render_table`/`split_chunks`。

- [ ] **Step 3: commit**

```bash
set -o pipefail
git add crates/wukong-render/src/lib.rs
git commit -m "test(render): cover table downgrade and chunk splitting"
```

---

## Task 4: `wukong-render` README + 接進 telegram 依賴

**Files:**
- Create: `crates/wukong-render/README.md`
- Modify: `crates/wukong-telegram/Cargo.toml`

- [ ] **Step 1: 建 README**

建立 `crates/wukong-render/README.md`,涵蓋:用途(LLM markdown → 傳輸格式)、`to_telegram_html`(GFM→Telegram HTML 子集、表格降級 `<pre>`、跳脫、4096 切段)、未來 `to_web_html` 預留、依 pulldown-cmark。

- [ ] **Step 2: telegram 依賴 wukong-render**

在 `crates/wukong-telegram/Cargo.toml` 的 `[dependencies]` 加:

```toml
wukong-render = { path = "../wukong-render" }
```

- [ ] **Step 3: 編譯確認**

Run: `. "$HOME/.cargo/env" && cargo build -p wukong-telegram`
Expected: 成功編譯。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-render/README.md crates/wukong-telegram/Cargo.toml
git commit -m "docs(render): readme; wire render into telegram crate"
```

---

## Task 5: `TgClient` 擴充(message_id / html / edit / delete)

**Files:**
- Modify: `crates/wukong-telegram/src/client.rs`

- [ ] **Step 1: 改寫 trait、reqwest impl、mock**

把 `crates/wukong-telegram/src/client.rs` 整檔替換為:

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
    /// Send a plain text message; returns the new message_id.
    fn send_message(
        &self,
        chat_id: i64,
        text: &str,
    ) -> impl std::future::Future<Output = Result<i64, TgError>> + Send;
    /// Send an HTML (parse_mode=HTML) message; returns the new message_id.
    fn send_message_html(
        &self,
        chat_id: i64,
        html: &str,
    ) -> impl std::future::Future<Output = Result<i64, TgError>> + Send;
    /// Edit an existing message's text (plain).
    fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
    ) -> impl std::future::Future<Output = Result<(), TgError>> + Send;
    /// Delete a message.
    fn delete_message(
        &self,
        chat_id: i64,
        message_id: i64,
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

    /// POST a JSON body and return the parsed response value.
    async fn post(&self, method: &str, body: serde_json::Value) -> Result<serde_json::Value, TgError> {
        let url = format!("{}/{method}", self.base);
        let resp = self.http.post(&url).json(&body).send().await?;
        Ok(resp.json::<serde_json::Value>().await?)
    }
}

/// Pull `result.message_id` out of a sendMessage response.
fn message_id_of(v: &serde_json::Value) -> Result<i64, TgError> {
    v.get("result")
        .and_then(|r| r.get("message_id"))
        .and_then(|m| m.as_i64())
        .ok_or_else(|| TgError::Api(format!("no message_id in response: {v}")))
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

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<i64, TgError> {
        let v = self
            .post("sendMessage", serde_json::json!({ "chat_id": chat_id, "text": text }))
            .await?;
        message_id_of(&v)
    }

    async fn send_message_html(&self, chat_id: i64, html: &str) -> Result<i64, TgError> {
        let v = self
            .post(
                "sendMessage",
                serde_json::json!({ "chat_id": chat_id, "text": html, "parse_mode": "HTML" }),
            )
            .await?;
        message_id_of(&v)
    }

    async fn edit_message_text(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), TgError> {
        self.post(
            "editMessageText",
            serde_json::json!({ "chat_id": chat_id, "message_id": message_id, "text": text }),
        )
        .await?;
        Ok(())
    }

    async fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<(), TgError> {
        self.post(
            "deleteMessage",
            serde_json::json!({ "chat_id": chat_id, "message_id": message_id }),
        )
        .await?;
        Ok(())
    }

    async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<(), TgError> {
        self.post("sendChatAction", serde_json::json!({ "chat_id": chat_id, "action": action }))
            .await?;
        Ok(())
    }
}

#[cfg(test)]
pub mod mock {
    use super::*;
    use std::sync::{Arc, Mutex};

    /// One recorded outbound message.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Sent {
        pub chat_id: i64,
        pub text: String,
        pub html: bool,
    }

    /// In-memory client: scripts no updates, records all calls. Returns
    /// monotonically increasing message_ids starting at 1.
    #[derive(Clone, Default)]
    pub struct MockTgClient {
        pub sent: Arc<Mutex<Vec<Sent>>>,
        pub edits: Arc<Mutex<Vec<(i64, i64, String)>>>,
        pub deletes: Arc<Mutex<Vec<(i64, i64)>>>,
        pub actions: Arc<Mutex<Vec<(i64, String)>>>,
        next_id: Arc<Mutex<i64>>,
    }

    impl MockTgClient {
        fn alloc_id(&self) -> i64 {
            let mut g = self.next_id.lock().unwrap();
            *g += 1;
            *g
        }
    }

    impl TgClient for MockTgClient {
        async fn get_updates(&self, _offset: i64) -> Result<serde_json::Value, TgError> {
            Ok(serde_json::json!({ "result": [] }))
        }
        async fn send_message(&self, chat_id: i64, text: &str) -> Result<i64, TgError> {
            self.sent.lock().unwrap().push(Sent { chat_id, text: text.to_string(), html: false });
            Ok(self.alloc_id())
        }
        async fn send_message_html(&self, chat_id: i64, html: &str) -> Result<i64, TgError> {
            self.sent.lock().unwrap().push(Sent { chat_id, text: html.to_string(), html: true });
            Ok(self.alloc_id())
        }
        async fn edit_message_text(&self, chat_id: i64, message_id: i64, text: &str) -> Result<(), TgError> {
            self.edits.lock().unwrap().push((chat_id, message_id, text.to_string()));
            Ok(())
        }
        async fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<(), TgError> {
            self.deletes.lock().unwrap().push((chat_id, message_id));
            Ok(())
        }
        async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<(), TgError> {
            self.actions.lock().unwrap().push((chat_id, action.to_string()));
            Ok(())
        }
    }

    #[tokio::test]
    async fn mock_allocates_ids_and_records() {
        let c = MockTgClient::default();
        let id1 = c.send_message(7, "a").await.unwrap();
        let id2 = c.send_message_html(7, "<b>b</b>").await.unwrap();
        assert_eq!(id1, 1);
        assert_eq!(id2, 2);
        c.edit_message_text(7, id1, "edited").await.unwrap();
        c.delete_message(7, id1).await.unwrap();
        assert_eq!(c.sent.lock().unwrap().len(), 2);
        assert!(c.sent.lock().unwrap()[1].html);
        assert_eq!(c.edits.lock().unwrap()[0], (7, 1, "edited".to_string()));
        assert_eq!(c.deletes.lock().unwrap()[0], (7, 1));
    }
}
```

- [ ] **Step 2: 跑測試確認失敗→通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram mock_allocates_ids_and_records`
Expected: 本檔測試 PASS,但 **dispatch.rs 既有測試會編譯失敗**(send_message 簽章變了、MockTgClient 欄位變了)——Task 6 修正。本步先確認 client 測試本身綠:
Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram --lib client::mock 2>&1 | tail -5`
若整體編譯因 dispatch 失敗,屬預期(下一任務修)。

- [ ] **Step 3: commit**

```bash
set -o pipefail
git add crates/wukong-telegram/src/client.rs
git commit -m "feat(telegram): extend TgClient with message ids, html, edit, delete"
```

---

## Task 6: dispatch 訊息流整併

**Files:**
- Modify: `crates/wukong-telegram/src/dispatch.rs`

- [ ] **Step 1: 改寫 `Turn` 分支**

把 `crates/wukong-telegram/src/dispatch.rs` 的 `MessageAction::Turn(input)` 整段替換為:

```rust
        MessageAction::Turn(input) => {
            let mut cfg = base_cfg.clone();
            cfg.scope = scope_for_chat(chat_id);

            // Single status bubble, edited in place as the turn progresses.
            let mid = match client.send_message(chat_id, "🐵 收到，思考中…").await {
                Ok(id) => id,
                Err(_) => return, // can't even post a status bubble; give up quietly
            };

            // Sustained "typing…": opencode runs for tens of seconds with no
            // token streaming; Telegram's typing indicator lasts only ~5s.
            let typing = {
                let c = client.clone();
                tokio::spawn(async move {
                    loop {
                        let _ = c.send_chat_action(chat_id, "typing").await;
                        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    }
                })
            };

            // Per-role progress edits the single status bubble (no new bubbles).
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Role>();
            let progress = {
                let c = client.clone();
                tokio::spawn(async move {
                    while let Some(role) = rx.recv().await {
                        let _ = c
                            .edit_message_text(chat_id, mid, &format!("🐵 悟空·{} 思考中…", role.name()))
                            .await;
                    }
                })
            };

            let result = run_turn(mem, backend, &cfg, &input, &mut |_| {}, &mut |r| {
                let _ = tx.send(r);
            })
            .await;
            drop(tx);
            let _ = progress.await;
            typing.abort();

            match result {
                Ok(out) => {
                    let chunks = wukong_render::to_telegram_html(&out.text);
                    let _ = client.delete_message(chat_id, mid).await;
                    if chunks.is_empty() {
                        let _ = client.send_message(chat_id, "(無內容)").await;
                    } else {
                        for c in &chunks {
                            let _ = client.send_message_html(chat_id, c).await;
                        }
                    }
                }
                Err(e) => {
                    let _ = client
                        .edit_message_text(chat_id, mid, &format!("⚠️ 處理失敗：{e}"))
                        .await;
                }
            }
        }
```

並在檔頭 `use` 區確認有 `use wukong_orchestrator::Role;`(既有)。`wukong_render` 以完整路徑 `wukong_render::to_telegram_html` 呼叫,免加 use。

- [ ] **Step 2: 更新既有 dispatch 測試為新簽章 + 新斷言**

把 `crates/wukong-telegram/src/dispatch.rs` 的 `mod tests` 內三個受影響測試改寫(`MockTgClient` 現在記錄 `Sent{chat_id,text,html}`、`edits`、`deletes`):

替換 `turn_runs_and_replies_in_chat_scope`:

```rust
    #[tokio::test]
    async fn turn_renders_answer_and_consolidates_messages() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        // planner -> single role; then execute answer with markdown.
        let backend = MockBackend::new(&["oracle", "**重點** 答案"]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "什麼是 BM25".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;

        // Status bubble created, edited per role, then deleted.
        assert!(!client.edits.lock().unwrap().is_empty());
        assert!(!client.deletes.lock().unwrap().is_empty());

        // Final answer sent as rendered HTML.
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| s.html && s.text.contains("<b>重點</b>")));
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
```

替換 `turn_sends_immediate_ack_and_typing`(ack 現在是狀態泡泡的第一則純文字 + typing 仍在):

```rust
    #[tokio::test]
    async fn turn_sends_status_bubble_and_typing() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle", "答案"]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "hi".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;

        // The first send is the plain status bubble.
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| !s.html && s.text.contains("思考中")));
        drop(sent);
        assert!(!client.actions.lock().unwrap().is_empty()); // typing emitted
    }
```

`slash_command_replies_unsupported` 改 `sent` 斷言為新結構:

```rust
    #[tokio::test]
    async fn slash_command_replies_unsupported() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "/reset".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| s.chat_id == 12 && s.text.contains("尚未支援")));
    }
```

`ignores_messages_outside_allowlist` 的斷言 `client.sent.lock().unwrap().is_empty()` 不受結構變更影響(仍是空 Vec),保留不動。

- [ ] **Step 3: 跑全 crate 測試確認通過**

Run: `. "$HOME/.cargo/env" && cargo test -p wukong-telegram`
Expected: 全綠(render 已在另 crate;此處 dispatch + client + parse + command 測試)。逐項確認新測試 `turn_renders_answer_and_consolidates_messages`、`turn_sends_status_bubble_and_typing` 通過。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add crates/wukong-telegram/src/dispatch.rs
git commit -m "feat(telegram): consolidate into one status bubble and render answer html"
```

---

## Task 7: clippy、文件、手動煙霧

**Files:**
- Modify: `README.md`(根)
- Modify: `crates/wukong-telegram/README.md`

- [ ] **Step 1: clippy 全綠**

Run: `. "$HOME/.cargo/env" && cargo clippy --all-targets -- -D warnings`
Expected:零警告。

- [ ] **Step 2: 全 workspace 測試**

Run: `. "$HOME/.cargo/env" && cargo test`
Expected: 全綠。

- [ ] **Step 3: 更新文件**

- `crates/wukong-telegram/README.md`:把「進度呈現」段更新為「單一狀態泡泡原地更新 → 完成後刪除 → 發 HTML 渲染答案(經 `wukong-render`)」;markdown 渲染說明(粗體/code/表格降級)。
- `README.md`(根):
  - 「專案結構」加 `wukong-render`(渲染層 lib)。
  - 「Telegram bot」段補:答案以 HTML 渲染(粗體/code block/表格降級)、進度為單泡泡。

- [ ] **Step 4: commit**

```bash
set -o pipefail
git add README.md crates/wukong-telegram/README.md
git commit -m "docs: document output rendering and consolidated telegram messages"
```

- [ ] **Step 5: 手動真實煙霧(需 token,非 CI)**

```bash
. "$HOME/.cargo/env"
export WUKONG_TG_TOKEN="<token>"
export WUKONG_TG_ALLOWED="<chat id>"
export WUKONG_MEMORY_DB="sqlite:///tmp/tg-render.db"
cargo run -p wukong-telegram
```
傳一個會產生粗體/清單/code block/表格的問題(例:「用表格比較 BM25 與向量檢索,並給一段 Rust code」)。確認:單一狀態泡泡隨角色變化 → 完成後消失 → 答案以粗體/等寬 code/對齊表格正確顯示,不再是多個純文字泡泡。

---

## 完成後

依 `superpowers:finishing-a-development-branch`:跑全測試 → 呈現 4 選項。本分支同時含「即時 ack + 持續 typing(已完成)」與本次「渲染 + 訊息整併」,合併後比照慣例詢問是否開 **v0.6.1 / v0.7.0 release**。

## 自我複查紀錄

- **Spec 覆蓋:** `wukong-render` crate + escape(T1)、行內/區塊映射(T2)、表格降級 + 切段(T2 實作/T3 固化)、README + 接線(T4)、TgClient 擴充(T5)、dispatch 整併(T6)、clippy/docs/手動煙霧(T7)。spec 各節皆有對應 task。
- **型別一致:** `to_telegram_html(&str)->Vec<String>`(T1/T2 定義,T6 使用)、`TgClient`(send_message→i64、send_message_html、edit_message_text、delete_message;T5 定義,T6 使用)、`MockTgClient` 的 `Sent{chat_id,text,html}`/`edits`/`deletes`(T5 定義,T6 測試使用)。`Role`/`run_turn`/`GatewayConfig` 與既有相符。
- **前向相依:** T5 改 TgClient 簽章會讓 T6 前的 dispatch 暫時編譯失敗(已於 T5 Step 2 註明屬預期,T6 修正)。
- **離線可測:** render 純函式;dispatch 用 MockTgClient + 假 backend + 暫存 sqlite;真實 Telegram 為手動煙霧。
- **接續分支:** 本計畫在 `feat/telegram-progress`,T6 將該分支既有的 ack/typing/per-role 多訊息演進為單泡泡整併,無遺留重複邏輯。