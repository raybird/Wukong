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
            Ok(AgentResponse {
                text: self.replies.lock().unwrap().pop_front().unwrap_or_default(),
                session_id: None,
            })
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
            thinking: true,
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
    async fn turn_renders_answer_and_consolidates_messages() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        // planner -> single role; then execute answer with markdown.
        let backend = MockBackend::new(&["oracle", "**重點** 答案"]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "什麼是 BM25".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;

        // Status bubble edited per role, then deleted.
        assert!(!client.edits.lock().unwrap().is_empty());
        assert!(!client.deletes.lock().unwrap().is_empty());

        // Final answer sent as rendered HTML.
        {
            let sent = client.sent.lock().unwrap();
            assert!(sent.iter().any(|s| s.html && s.text.contains("<b>重點</b>")));
        }

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
    async fn turn_sends_status_bubble_and_typing() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle", "答案"]);
        let msg = TgMessage { update_id: 1, chat_id: 12, text: "hi".to_string() };
        handle_message(&client, &mem, &base_cfg(), &backend, &[12], &msg).await;

        // The first send is the plain status bubble.
        {
            let sent = client.sent.lock().unwrap();
            assert!(sent.iter().any(|s| !s.html && s.text.contains("思考中")));
        }
        assert!(!client.actions.lock().unwrap().is_empty()); // typing emitted
    }

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
}
