//! Per-message dispatch: allowlist → classify → run_turn → reply.

use crate::client::TgClient;
use crate::command::{classify_message, MessageAction};
use crate::parse::{is_allowed, scope_for_chat, TgMessage};
use wukong_cli::run_turn;
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_memory::Memory;
use wukong_orchestrator::Role;

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
