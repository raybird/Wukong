//! Telegram Bot API client. A trait so callers are testable without network;
//! ReqwestTgClient is the real long-poll/send implementation.

use crate::error::TgError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgFileInfo {
    pub file_id: String,
    pub file_unique_id: Option<String>,
    pub file_size: Option<i64>,
    pub file_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlineKeyboardButton {
    pub text: String,
    pub callback_data: String,
}

pub type InlineKeyboard = Vec<Vec<InlineKeyboardButton>>;

/// The slice of the Telegram Bot API the bot and daemon need.
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
    /// Send a plain text message with an inline keyboard; returns message_id.
    fn send_message_with_inline_keyboard(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: InlineKeyboard,
    ) -> impl std::future::Future<Output = Result<i64, TgError>> + Send;
    /// Upload a document and return the new message_id.
    fn send_document(
        &self,
        chat_id: i64,
        filename: &str,
        bytes: Vec<u8>,
        caption: Option<&str>,
    ) -> impl std::future::Future<Output = Result<i64, TgError>> + Send;
    /// Stream an ephemeral plain-text draft in a private chat.
    fn send_message_draft(
        &self,
        chat_id: i64,
        draft_id: i64,
        text: &str,
    ) -> impl std::future::Future<Output = Result<(), TgError>> + Send;
    /// Edit an existing message's text (plain).
    fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
    ) -> impl std::future::Future<Output = Result<(), TgError>> + Send;
    /// Edit an existing message's text and inline keyboard.
    fn edit_message_text_with_inline_keyboard(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: InlineKeyboard,
    ) -> impl std::future::Future<Output = Result<(), TgError>> + Send;
    /// Answer a callback query so Telegram removes the client's loading state.
    fn answer_callback_query(
        &self,
        callback_query_id: &str,
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
    /// Resolve Telegram file metadata before downloading bytes.
    fn get_file(
        &self,
        file_id: &str,
    ) -> impl std::future::Future<Output = Result<TgFileInfo, TgError>> + Send;
    /// Download file bytes from a Telegram file path returned by `get_file`.
    fn download_file(
        &self,
        file_path: &str,
    ) -> impl std::future::Future<Output = Result<Vec<u8>, TgError>> + Send;
}

/// Real client over `https://api.telegram.org/bot<token>/`.
#[derive(Clone)]
pub struct ReqwestTgClient {
    http: reqwest::Client,
    base: String,
    file_base: String,
}

impl ReqwestTgClient {
    pub fn new(token: &str) -> Self {
        let http = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()
            .expect("reqwest client");
        Self {
            http,
            base: format!("https://api.telegram.org/bot{token}"),
            file_base: format!("https://api.telegram.org/file/bot{token}"),
        }
    }

    /// POST a JSON body and return the parsed response value.
    async fn post(
        &self,
        method: &str,
        body: serde_json::Value,
    ) -> Result<serde_json::Value, TgError> {
        let url = format!("{}/{method}", self.base);
        let resp = self.http.post(&url).json(&body).send().await?;
        check_ok(resp.json::<serde_json::Value>().await?)
    }
}

/// Reject a Telegram response whose `ok` field is not `true`, surfacing the
/// API's `error_code`/`description` as an error instead of silently treating a
/// failure (e.g. 401 on an invalid token, 400 on malformed HTML) as success.
fn check_ok(v: serde_json::Value) -> Result<serde_json::Value, TgError> {
    if v.get("ok").and_then(|o| o.as_bool()).unwrap_or(false) {
        return Ok(v);
    }
    let code = v.get("error_code").and_then(|c| c.as_i64());
    let desc = v
        .get("description")
        .and_then(|d| d.as_str())
        .unwrap_or("unknown error");
    Err(TgError::Api(match code {
        Some(code) => format!("telegram api error {code}: {desc}"),
        None => format!("telegram api error: {desc}"),
    }))
}

/// Telegram 拒收 HTML（`parse_mode=HTML` 解析失敗）時的錯誤：400，描述含
/// `can't parse entities`。字樣取自 issue-0032 的實機探測；同為 400 的
/// `chat not found`、`text must be non-empty` 不算。
fn is_html_parse_error(err: &TgError) -> bool {
    matches!(err, TgError::Api(msg)
        if msg.starts_with("telegram api error 400:") && msg.contains("can't parse entities"))
}

/// 把 `wukong-render` 輸出的 Telegram HTML 轉成純文字：拿掉標籤、還原跳脫字元，
/// 連結寫成「文字 (網址)」；空白與換行原樣保留。
fn html_to_plain(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut href: Option<&str> = None;
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        out.push_str(&unescape_html(&rest[..start]));
        let Some(len) = rest[start..].find('>') else {
            // 沒有 `>` 就不是標籤，剩下的當文字。
            rest = &rest[start..];
            break;
        };
        let tag = &rest[start..start + len + 1];
        if let Some(url) = tag
            .strip_prefix("<a href=\"")
            .and_then(|t| t.strip_suffix("\">"))
        {
            href = Some(url);
        } else if tag == "</a>" {
            if let Some(url) = href.take() {
                out.push_str(&format!(" ({})", unescape_html(url)));
            }
        }
        rest = &rest[start + len + 1..];
    }
    out.push_str(&unescape_html(rest));
    out
}

/// 送出一段 HTML；Telegram 判定 HTML 解析失敗時，同一段改以純文字重送，並在 stderr
/// 記一行警告（不含訊息內容）。其他錯誤原樣回傳，由呼叫端照原本的方式處理。
pub async fn send_html_with_plain_fallback<C: TgClient>(
    client: &C,
    chat_id: i64,
    html: &str,
    context: &str,
) -> Result<i64, TgError> {
    match client.send_message_html(chat_id, html).await {
        Err(err) if is_html_parse_error(&err) => {
            eprintln!("{}", fallback_warning(context, &err));
            client.send_message(chat_id, &html_to_plain(html)).await
        }
        result => result,
    }
}

fn fallback_warning(context: &str, err: &TgError) -> String {
    format!("🐵 Telegram 拒收 HTML（{context}），改送純文字：{err}")
}

/// `&amp;` 最後還原，`&amp;lt;` 才不會被還原兩次。
fn unescape_html(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&amp;", "&")
}

/// Pull `result.message_id` out of a sendMessage response.
fn message_id_of(v: &serde_json::Value) -> Result<i64, TgError> {
    v.get("result")
        .and_then(|r| r.get("message_id"))
        .and_then(|m| m.as_i64())
        .ok_or_else(|| TgError::Api(format!("no message_id in response: {v}")))
}

fn inline_keyboard_markup(keyboard: InlineKeyboard) -> serde_json::Value {
    serde_json::json!({
        "inline_keyboard": keyboard
            .into_iter()
            .map(|row| {
                row.into_iter()
                    .map(|button| serde_json::json!({
                        "text": button.text,
                        "callback_data": button.callback_data,
                    }))
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
    })
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
        check_ok(resp.json::<serde_json::Value>().await?)
    }

    async fn send_message(&self, chat_id: i64, text: &str) -> Result<i64, TgError> {
        let v = self
            .post(
                "sendMessage",
                serde_json::json!({ "chat_id": chat_id, "text": text }),
            )
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

    async fn send_message_with_inline_keyboard(
        &self,
        chat_id: i64,
        text: &str,
        keyboard: InlineKeyboard,
    ) -> Result<i64, TgError> {
        let v = self
            .post(
                "sendMessage",
                serde_json::json!({
                    "chat_id": chat_id,
                    "text": text,
                    "reply_markup": inline_keyboard_markup(keyboard),
                }),
            )
            .await?;
        message_id_of(&v)
    }

    async fn send_document(
        &self,
        chat_id: i64,
        filename: &str,
        bytes: Vec<u8>,
        caption: Option<&str>,
    ) -> Result<i64, TgError> {
        let url = format!("{}/sendDocument", self.base);
        let document = reqwest::multipart::Part::bytes(bytes).file_name(filename.to_string());
        let mut form = reqwest::multipart::Form::new()
            .text("chat_id", chat_id.to_string())
            .part("document", document);
        if let Some(caption) = caption.filter(|caption| !caption.is_empty()) {
            form = form.text("caption", caption.to_string());
        }
        let response = self.http.post(url).multipart(form).send().await?;
        let value = check_ok(response.json::<serde_json::Value>().await?)?;
        message_id_of(&value)
    }

    async fn send_message_draft(
        &self,
        chat_id: i64,
        draft_id: i64,
        text: &str,
    ) -> Result<(), TgError> {
        self.post(
            "sendMessageDraft",
            serde_json::json!({ "chat_id": chat_id, "draft_id": draft_id, "text": text }),
        )
        .await?;
        Ok(())
    }

    async fn edit_message_text(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
    ) -> Result<(), TgError> {
        self.post(
            "editMessageText",
            serde_json::json!({ "chat_id": chat_id, "message_id": message_id, "text": text }),
        )
        .await?;
        Ok(())
    }

    async fn edit_message_text_with_inline_keyboard(
        &self,
        chat_id: i64,
        message_id: i64,
        text: &str,
        keyboard: InlineKeyboard,
    ) -> Result<(), TgError> {
        self.post(
            "editMessageText",
            serde_json::json!({
                "chat_id": chat_id,
                "message_id": message_id,
                "text": text,
                "reply_markup": inline_keyboard_markup(keyboard),
            }),
        )
        .await?;
        Ok(())
    }

    async fn answer_callback_query(
        &self,
        callback_query_id: &str,
        text: &str,
    ) -> Result<(), TgError> {
        self.post(
            "answerCallbackQuery",
            serde_json::json!({ "callback_query_id": callback_query_id, "text": text }),
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
        self.post(
            "sendChatAction",
            serde_json::json!({ "chat_id": chat_id, "action": action }),
        )
        .await?;
        Ok(())
    }

    async fn get_file(&self, file_id: &str) -> Result<TgFileInfo, TgError> {
        let v = self
            .post("getFile", serde_json::json!({ "file_id": file_id }))
            .await?;
        let result = v
            .get("result")
            .ok_or_else(|| TgError::Api(format!("no file result in response: {v}")))?;
        let file_path = result
            .get("file_path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| TgError::Api(format!("no file_path in response: {v}")))?;
        Ok(TgFileInfo {
            file_id: result
                .get("file_id")
                .and_then(|v| v.as_str())
                .unwrap_or(file_id)
                .to_string(),
            file_unique_id: result
                .get("file_unique_id")
                .and_then(|v| v.as_str())
                .map(str::to_string),
            file_size: result.get("file_size").and_then(|v| v.as_i64()),
            file_path: file_path.to_string(),
        })
    }

    async fn download_file(&self, file_path: &str) -> Result<Vec<u8>, TgError> {
        let url = format!("{}/{file_path}", self.file_base);
        let resp = self.http.get(&url).send().await?;
        Ok(resp.bytes().await?.to_vec())
    }
}

/// In-memory client for tests. Gated so dependent crates can opt in via the
/// `mock` feature (their dev-dependency) while it stays out of release builds.
#[cfg(any(test, feature = "mock"))]
pub mod mock {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// One recorded outbound message.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Sent {
        pub chat_id: i64,
        pub text: String,
        pub html: bool,
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    pub struct SentDocument {
        pub chat_id: i64,
        pub filename: String,
        pub bytes: Vec<u8>,
        pub caption: Option<String>,
    }

    type MockFiles = Arc<Mutex<HashMap<String, (TgFileInfo, Vec<u8>)>>>;
    type InlineEditLog = Arc<Mutex<Vec<(i64, i64, String, InlineKeyboard)>>>;
    /// (內容片段, Telegram 的 error_code, description)
    type Rejections = Arc<Mutex<Vec<(String, i64, String)>>>;

    /// 內容含 `needle` 的送出，以 Telegram 實際的錯誤回應格式失敗，不寫進 `sent`。
    fn rejection(rules: &Rejections, text: &str) -> Option<TgError> {
        rules
            .lock()
            .unwrap()
            .iter()
            .find(|(needle, _, _)| text.contains(needle.as_str()))
            .map(|(_, code, description)| {
                check_ok(serde_json::json!({
                    "ok": false,
                    "error_code": code,
                    "description": description,
                }))
                .unwrap_err()
            })
    }

    /// In-memory client: scripts no updates, records all calls. Returns
    /// monotonically increasing message_ids starting at 1.
    #[derive(Clone, Default)]
    pub struct MockTgClient {
        pub sent: Arc<Mutex<Vec<Sent>>>,
        pub edits: Arc<Mutex<Vec<(i64, i64, String)>>>,
        pub inline_messages: Arc<Mutex<Vec<(i64, String, InlineKeyboard)>>>,
        pub inline_edits: InlineEditLog,
        pub callback_answers: Arc<Mutex<Vec<(String, String)>>>,
        pub deletes: Arc<Mutex<Vec<(i64, i64)>>>,
        pub actions: Arc<Mutex<Vec<(i64, String)>>>,
        pub drafts: Arc<Mutex<Vec<(i64, i64, String)>>>,
        pub documents: Arc<Mutex<Vec<SentDocument>>>,
        files: MockFiles,
        next_id: Arc<Mutex<i64>>,
        drafts_enabled: Arc<Mutex<bool>>,
        html_rejections: Rejections,
        plain_rejections: Rejections,
    }

    impl MockTgClient {
        fn alloc_id(&self) -> i64 {
            let mut g = self.next_id.lock().unwrap();
            *g += 1;
            *g
        }

        pub fn with_file(self, file_id: &str, file_path: &str, bytes: Vec<u8>) -> Self {
            self.files.lock().unwrap().insert(
                file_id.to_string(),
                (
                    TgFileInfo {
                        file_id: file_id.to_string(),
                        file_unique_id: None,
                        file_size: Some(bytes.len() as i64),
                        file_path: file_path.to_string(),
                    },
                    bytes,
                ),
            );
            self
        }

        /// `send_message_html` 遇到含 `needle` 的內容時回這個錯誤。
        pub fn reject_html_containing(self, needle: &str, code: i64, description: &str) -> Self {
            self.html_rejections.lock().unwrap().push((
                needle.to_string(),
                code,
                description.to_string(),
            ));
            self
        }

        /// `send_message` 遇到含 `needle` 的內容時回這個錯誤。
        pub fn reject_plain_containing(self, needle: &str, code: i64, description: &str) -> Self {
            self.plain_rejections.lock().unwrap().push((
                needle.to_string(),
                code,
                description.to_string(),
            ));
            self
        }

        pub fn with_message_drafts(self) -> Self {
            *self.drafts_enabled.lock().unwrap() = true;
            self
        }
    }

    impl TgClient for MockTgClient {
        async fn get_updates(&self, _offset: i64) -> Result<serde_json::Value, TgError> {
            Ok(serde_json::json!({ "result": [] }))
        }
        async fn send_message(&self, chat_id: i64, text: &str) -> Result<i64, TgError> {
            if let Some(err) = rejection(&self.plain_rejections, text) {
                return Err(err);
            }
            self.sent.lock().unwrap().push(Sent {
                chat_id,
                text: text.to_string(),
                html: false,
            });
            Ok(self.alloc_id())
        }
        async fn send_message_html(&self, chat_id: i64, html: &str) -> Result<i64, TgError> {
            if let Some(err) = rejection(&self.html_rejections, html) {
                return Err(err);
            }
            self.sent.lock().unwrap().push(Sent {
                chat_id,
                text: html.to_string(),
                html: true,
            });
            Ok(self.alloc_id())
        }
        async fn send_message_with_inline_keyboard(
            &self,
            chat_id: i64,
            text: &str,
            keyboard: InlineKeyboard,
        ) -> Result<i64, TgError> {
            self.inline_messages
                .lock()
                .unwrap()
                .push((chat_id, text.to_string(), keyboard));
            Ok(self.alloc_id())
        }
        async fn send_document(
            &self,
            chat_id: i64,
            filename: &str,
            bytes: Vec<u8>,
            caption: Option<&str>,
        ) -> Result<i64, TgError> {
            self.documents.lock().unwrap().push(SentDocument {
                chat_id,
                filename: filename.to_string(),
                bytes,
                caption: caption.map(str::to_string),
            });
            Ok(self.alloc_id())
        }
        async fn send_message_draft(
            &self,
            chat_id: i64,
            draft_id: i64,
            text: &str,
        ) -> Result<(), TgError> {
            self.drafts
                .lock()
                .unwrap()
                .push((chat_id, draft_id, text.to_string()));
            if *self.drafts_enabled.lock().unwrap() {
                Ok(())
            } else {
                Err(TgError::Api(
                    "sendMessageDraft is disabled in this mock".to_string(),
                ))
            }
        }
        async fn edit_message_text(
            &self,
            chat_id: i64,
            message_id: i64,
            text: &str,
        ) -> Result<(), TgError> {
            self.edits
                .lock()
                .unwrap()
                .push((chat_id, message_id, text.to_string()));
            Ok(())
        }
        async fn edit_message_text_with_inline_keyboard(
            &self,
            chat_id: i64,
            message_id: i64,
            text: &str,
            keyboard: InlineKeyboard,
        ) -> Result<(), TgError> {
            self.inline_edits.lock().unwrap().push((
                chat_id,
                message_id,
                text.to_string(),
                keyboard,
            ));
            Ok(())
        }
        async fn answer_callback_query(
            &self,
            callback_query_id: &str,
            text: &str,
        ) -> Result<(), TgError> {
            self.callback_answers
                .lock()
                .unwrap()
                .push((callback_query_id.to_string(), text.to_string()));
            Ok(())
        }
        async fn delete_message(&self, chat_id: i64, message_id: i64) -> Result<(), TgError> {
            self.deletes.lock().unwrap().push((chat_id, message_id));
            Ok(())
        }
        async fn send_chat_action(&self, chat_id: i64, action: &str) -> Result<(), TgError> {
            self.actions
                .lock()
                .unwrap()
                .push((chat_id, action.to_string()));
            Ok(())
        }
        async fn get_file(&self, file_id: &str) -> Result<TgFileInfo, TgError> {
            self.files
                .lock()
                .unwrap()
                .get(file_id)
                .map(|(info, _)| info.clone())
                .ok_or_else(|| TgError::Api(format!("mock file not found: {file_id}")))
        }
        async fn download_file(&self, file_path: &str) -> Result<Vec<u8>, TgError> {
            self.files
                .lock()
                .unwrap()
                .values()
                .find(|(info, _)| info.file_path == file_path)
                .map(|(_, bytes)| bytes.clone())
                .ok_or_else(|| TgError::Api(format!("mock file path not found: {file_path}")))
        }
    }

    #[cfg(test)]
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

    #[cfg(test)]
    #[tokio::test]
    async fn mock_returns_file_metadata_and_bytes() {
        let c = MockTgClient::default().with_file("file_1", "docs/report.pdf", b"hello".to_vec());
        let info = c.get_file("file_1").await.unwrap();
        assert_eq!(info.file_path, "docs/report.pdf");
        let bytes = c.download_file(&info.file_path).await.unwrap();
        assert_eq!(bytes, b"hello");
    }

    #[cfg(test)]
    #[tokio::test]
    async fn mock_records_documents() {
        let client = MockTgClient::default();

        let message_id = client
            .send_document(7, "report.csv", b"a,b\n1,2\n".to_vec(), Some("完成"))
            .await
            .unwrap();

        assert_eq!(message_id, 1);
        let documents = client.documents.lock().unwrap();
        assert_eq!(documents[0].filename, "report.csv");
        assert_eq!(documents[0].bytes, b"a,b\n1,2\n");
        assert_eq!(documents[0].caption.as_deref(), Some("完成"));
    }

    #[cfg(test)]
    #[tokio::test]
    async fn mock_records_enabled_message_drafts() {
        let c = MockTgClient::default().with_message_drafts();

        c.send_message_draft(7, 42, "thinking").await.unwrap();

        assert_eq!(c.drafts.lock().unwrap()[0], (7, 42, "thinking".to_string()));
        assert!(c.sent.lock().unwrap().is_empty());
    }

    #[cfg(test)]
    #[tokio::test]
    async fn mock_records_inline_keyboard_and_callback_answers() {
        let c = MockTgClient::default();
        let keyboard = vec![vec![InlineKeyboardButton {
            text: "選項 A".to_string(),
            callback_data: "q:que_1:pick:0:0".to_string(),
        }]];

        let id = c
            .send_message_with_inline_keyboard(7, "請選", keyboard.clone())
            .await
            .unwrap();
        c.edit_message_text_with_inline_keyboard(7, id, "更新", keyboard.clone())
            .await
            .unwrap();
        c.answer_callback_query("cb_1", "已收到").await.unwrap();

        assert_eq!(
            c.inline_messages.lock().unwrap()[0],
            (7, "請選".to_string(), keyboard.clone())
        );
        assert_eq!(
            c.inline_edits.lock().unwrap()[0],
            (7, id, "更新".to_string(), keyboard)
        );
        assert_eq!(
            c.callback_answers.lock().unwrap()[0],
            ("cb_1".to_string(), "已收到".to_string())
        );
    }
}

#[cfg(test)]
mod check_ok_tests {
    use super::check_ok;
    use serde_json::json;

    #[test]
    fn accepts_ok_true() {
        let v = json!({"ok": true, "result": {"message_id": 5}});
        assert!(check_ok(v).is_ok());
    }

    #[test]
    fn rejects_ok_false_with_code_and_description() {
        let v = json!({"ok": false, "error_code": 401, "description": "Unauthorized"});
        let msg = check_ok(v).unwrap_err().to_string();
        assert!(msg.contains("401"), "missing code: {msg}");
        assert!(msg.contains("Unauthorized"), "missing description: {msg}");
    }

    #[test]
    fn rejects_missing_ok_field() {
        assert!(check_ok(json!({"result": {}})).is_err());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-11-16 實機回應（issue-0032 implementation-plan 步驟 1）。
    fn api_error(code: i64, description: &str) -> TgError {
        check_ok(serde_json::json!({
            "ok": false,
            "error_code": code,
            "description": description,
        }))
        .unwrap_err()
    }

    #[test]
    fn html_parse_failures_from_telegram_are_recognised() {
        for desc in [
            "Bad Request: can't parse entities: Unsupported start tag \"think\" at byte offset 13",
            "Bad Request: can't parse entities: Can't find end tag corresponding to start tag \"b\"",
            "Bad Request: can't parse entities: Unexpected end tag at byte offset 14",
            "Bad Request: can't parse entities: Unclosed start tag at byte offset 13",
        ] {
            assert!(is_html_parse_error(&api_error(400, desc)), "{desc}");
        }
    }

    #[test]
    fn other_errors_are_not_html_parse_failures() {
        assert!(!is_html_parse_error(&api_error(
            400,
            "Bad Request: chat not found"
        )));
        assert!(!is_html_parse_error(&api_error(
            400,
            "Bad Request: text must be non-empty"
        )));
        assert!(!is_html_parse_error(&api_error(
            403,
            "Forbidden: bot was blocked by the user"
        )));
        assert!(!is_html_parse_error(&TgError::Http(
            "error sending request: connection refused".to_string()
        )));
    }

    #[test]
    fn html_to_plain_strips_tags_unescapes_and_spells_out_links() {
        let cases = [
            ("<b>粗</b> <i>斜</i> <code>x</code>", "粗 斜 x"),
            ("a &lt; b &amp;&amp; c &gt; d", "a < b && c > d"),
            (
                r#"<a href="https://x.io/?a=1&amp;b=2">docs</a>"#,
                "docs (https://x.io/?a=1&b=2)",
            ),
            (
                "第一行\n  ◦ 子項\n<blockquote>引言</blockquote>",
                "第一行\n  ◦ 子項\n引言",
            ),
            ("&amp;lt; 只還原一次", "&lt; 只還原一次"),
            // wukong-render 對含雙引號的網址實際輸出這個；Telegram 以解析失敗拒收（#36）。
            (
                r#"<a href="https://x.io/a"b">docs</a>"#,
                r#"docs (https://x.io/a"b)"#,
            ),
        ];
        for (html, want) in cases {
            assert_eq!(html_to_plain(html), want, "{html}");
        }
    }

    const PARSE_ERROR: &str =
        "Bad Request: can't parse entities: Unsupported start tag \"think\" at byte offset 13";

    fn sent(client: &mock::MockTgClient) -> Vec<(bool, String)> {
        client
            .sent
            .lock()
            .unwrap()
            .iter()
            .map(|s| (s.html, s.text.clone()))
            .collect()
    }

    #[tokio::test]
    async fn rejected_html_is_resent_as_plain_text() {
        let client = mock::MockTgClient::default().reject_html_containing("<b>", 400, PARSE_ERROR);
        let id = send_html_with_plain_fallback(&client, 7, "<b>粗</b> a &lt; b", "answer")
            .await
            .unwrap();
        assert_eq!(id, 1);
        assert_eq!(sent(&client), vec![(false, "粗 a < b".to_string())]);
    }

    #[tokio::test]
    async fn accepted_html_is_sent_once_without_fallback() {
        let client = mock::MockTgClient::default();
        send_html_with_plain_fallback(&client, 7, "<b>粗</b>", "answer")
            .await
            .unwrap();
        assert_eq!(sent(&client), vec![(true, "<b>粗</b>".to_string())]);
    }

    #[tokio::test]
    async fn other_errors_are_returned_without_resending() {
        let client = mock::MockTgClient::default().reject_html_containing(
            "<b>",
            400,
            "Bad Request: chat not found",
        );
        let err = send_html_with_plain_fallback(&client, 7, "<b>粗</b>", "answer")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("chat not found"), "{err}");
        assert!(sent(&client).is_empty());
    }

    #[tokio::test]
    async fn plain_resend_failure_is_returned() {
        let client = mock::MockTgClient::default()
            .reject_html_containing("<b>", 400, PARSE_ERROR)
            .reject_plain_containing("粗", 429, "Too Many Requests: retry after 5");
        let err = send_html_with_plain_fallback(&client, 7, "<b>粗</b>", "answer")
            .await
            .unwrap_err();
        assert!(err.to_string().contains("Too Many Requests"), "{err}");
        assert!(sent(&client).is_empty());
    }

    #[test]
    fn fallback_warning_names_the_caller_and_reason_but_not_the_content() {
        let warning = fallback_warning("schedule", &api_error(400, PARSE_ERROR));
        assert!(warning.contains("schedule"), "{warning}");
        assert!(
            warning.contains("can't parse entities: Unsupported start tag"),
            "{warning}"
        );
        assert!(!warning.contains("<b>粗</b>"), "{warning}");
    }
}
