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
