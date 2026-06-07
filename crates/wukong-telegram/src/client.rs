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
