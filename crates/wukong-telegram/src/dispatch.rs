//! Per-message dispatch: allowlist → classify → run_turn → reply.

use crate::client::TgClient;
use crate::command::{classify_message, MessageAction};
use crate::parse::{is_allowed, scope_for_chat, TgMessage};
use wukong_chat_history::ChatHistoryStore;
use wukong_cli::run_turn;
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::StreamEvent;
use wukong_memory::Memory;
use wukong_orchestrator::Role;

/// Progress updates fed to the single status bubble.
enum Progress {
    Role(Role),
    Reasoning(String),
    ToolUse(String),
}

/// Compose the status-bubble text from the current role and accumulated reasoning.
fn bubble_text(role: Option<&str>, reasoning: &str) -> String {
    // Full-width space (U+3000) after each emoji so the glyph doesn't visually
    // crowd / obscure the first CJK character on some Telegram clients.
    let base = match role {
        Some(r) => format!("🐵　悟空·{r} 思考中…"),
        None => "🐵　思考中…".to_string(),
    };
    let r = reasoning.trim();
    if r.is_empty() {
        return base;
    }
    format!("{base}\n💭　{r}")
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

async fn record_chat(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    role: &str,
    content: &str,
    content_html: Option<&str>,
    status: &str,
) {
    let Some(history) = history else {
        return;
    };
    match history.default_thread(scope).await {
        Ok(thread) => {
            if let Err(e) = history
                .insert_message(&thread, role, content, content_html, status, now_unix())
                .await
            {
                eprintln!("warning: telegram chat history insert failed: {e}");
            }
        }
        Err(e) => eprintln!("warning: telegram chat history thread failed: {e}"),
    }
}

async fn record_chat_with_events(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    role: &str,
    content: &str,
    content_html: Option<&str>,
    status: &str,
    events: &[(i64, String, Option<String>, String, i64)],
) {
    let Some(history) = history else {
        return;
    };
    match history.default_thread(scope).await {
        Ok(thread) => {
            match history
                .insert_message(&thread, role, content, content_html, status, now_unix())
                .await
            {
                Ok(message_id) => {
                    for (seq, kind, label, content, created_at) in events {
                        let _ = history
                            .insert_event(
                                message_id,
                                *seq,
                                kind,
                                label.as_deref(),
                                content,
                                *created_at,
                            )
                            .await;
                    }
                }
                Err(e) => eprintln!("warning: telegram chat history insert failed: {e}"),
            }
        }
        Err(e) => eprintln!("warning: telegram chat history thread failed: {e}"),
    }
}

/// Handle one incoming message: enforce the allowlist, classify, run the turn,
/// and reply. Errors are reported to the chat and swallowed (the loop goes on).
/// `C` must be Clone + Send + 'static so a side task can stream role progress.
pub async fn handle_message<C, B>(
    client: &C,
    mem: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    history: Option<&ChatHistoryStore>,
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
        MessageAction::Command { name, args } => {
            let mut cfg = base_cfg.clone();
            cfg.scope = scope_for_chat(chat_id);
            let settings_path = wukong_settings::default_settings_path();
            let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
            let agent_settings = wukong_settings::effective_agent_settings(&settings);
            cfg.apply_default_model(agent_settings.default_model.as_deref());
            let planner_preferences = wukong_settings::effective_planner_preferences(&settings);
            cfg.apply_planner_preferences(
                planner_preferences.enabled,
                planner_preferences.roles,
                planner_preferences.skills,
            );
            record_chat(history, &cfg.scope, "user", &msg.text, None, "complete").await;
            match wukong_cli::parse_session_command(&name, &args) {
                Some(cmd) => {
                    let reply = match wukong_cli::run_session_command(
                        mem,
                        backend,
                        &cfg,
                        &settings_path,
                        cmd,
                    )
                    .await
                    {
                        Ok(t) => t,
                        Err(e) => format!("⚠️ 失敗：{e}"),
                    };
                    record_chat(history, &cfg.scope, "assistant", &reply, None, "complete").await;
                    let _ = client.send_message(chat_id, &reply).await;
                }
                None => {
                    let reply = format!("指令 /{name} 尚未支援");
                    record_chat(history, &cfg.scope, "assistant", &reply, None, "complete").await;
                    let _ = client.send_message(chat_id, &reply).await;
                }
            }
        }
        MessageAction::Turn(input) => {
            let mut cfg = base_cfg.clone();
            cfg.scope = scope_for_chat(chat_id);
            let settings_path = wukong_settings::default_settings_path();
            let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
            let agent_settings = wukong_settings::effective_agent_settings(&settings);
            cfg.apply_default_model(agent_settings.default_model.as_deref());
            let planner_preferences = wukong_settings::effective_planner_preferences(&settings);
            cfg.apply_planner_preferences(
                planner_preferences.enabled,
                planner_preferences.roles,
                planner_preferences.skills,
            );
            record_chat(history, &cfg.scope, "user", &input, None, "complete").await;

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

            // Per-role + reasoning progress edits the single status bubble.
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();
            let progress = {
                let c = client.clone();
                tokio::spawn(async move {
                    let mut role: Option<String> = None;
                    let mut reasoning = String::new();
                    let mut last_reasoning_edit: Option<std::time::Instant> = None;
                    while let Some(msg) = rx.recv().await {
                        match msg {
                            Progress::Role(r) => {
                                role = Some(r.name().to_string());
                                let _ = c
                                    .edit_message_text(
                                        chat_id,
                                        mid,
                                        &bubble_text(role.as_deref(), &reasoning),
                                    )
                                    .await;
                            }
                            Progress::Reasoning(t) => {
                                reasoning.push_str(&t);
                                // Throttle reasoning edits (~1.5s) but never block
                                // the first one (so it always shows up promptly).
                                let due = last_reasoning_edit.is_none_or(|i| {
                                    i.elapsed() >= std::time::Duration::from_millis(1500)
                                });
                                if due {
                                    let _ = c
                                        .edit_message_text(
                                            chat_id,
                                            mid,
                                            &bubble_text(role.as_deref(), &reasoning),
                                        )
                                        .await;
                                    last_reasoning_edit = Some(std::time::Instant::now());
                                }
                            }
                            Progress::ToolUse(name) => {
                                reasoning.push_str("\n▸ 使用工具 ");
                                reasoning.push_str(&name);
                                reasoning.push('\n');
                                let _ = c
                                    .edit_message_text(
                                        chat_id,
                                        mid,
                                        &bubble_text(role.as_deref(), &reasoning),
                                    )
                                    .await;
                            }
                        }
                    }
                })
            };

            let tx_ev = tx.clone();
            let mut events_buf: Vec<(i64, String, Option<String>, String, i64)> = Vec::new();
            let mut event_seq: i64 = 0;
            let result = run_turn(
                mem,
                backend,
                &cfg,
                &input,
                &mut |ev| match ev {
                    StreamEvent::Reasoning(t) => {
                        if !t.trim().is_empty() {
                            let now = now_unix();
                            events_buf.push((
                                event_seq,
                                "reasoning".to_string(),
                                None,
                                t.clone(),
                                now,
                            ));
                            event_seq += 1;
                            let _ = tx_ev.send(Progress::Reasoning(t));
                        }
                    }
                    StreamEvent::ToolUse(name) => {
                        let now = now_unix();
                        events_buf.push((
                            event_seq,
                            "tool_use".to_string(),
                            Some(name.clone()),
                            format!("使用工具 {name}"),
                            now,
                        ));
                        event_seq += 1;
                        let _ = tx_ev.send(Progress::ToolUse(name));
                    }
                    StreamEvent::StepStart => {
                        let now = now_unix();
                        events_buf.push((
                            event_seq,
                            "step_start".to_string(),
                            None,
                            "step_start".to_string(),
                            now,
                        ));
                        event_seq += 1;
                    }
                    StreamEvent::StepFinish => {
                        let now = now_unix();
                        events_buf.push((
                            event_seq,
                            "step_finish".to_string(),
                            None,
                            "step_finish".to_string(),
                            now,
                        ));
                        event_seq += 1;
                    }
                    StreamEvent::Text(_) => {}
                },
                &mut |r| {
                    let _ = tx.send(Progress::Role(r));
                },
            )
            .await;
            drop(tx);
            drop(tx_ev);
            let _ = progress.await;
            typing.abort();

            match result {
                Ok(out) => {
                    let html = wukong_render::to_web_html(&out.text);
                    record_chat_with_events(
                        history,
                        &cfg.scope,
                        "assistant",
                        &out.text,
                        Some(&html),
                        "complete",
                        &events_buf,
                    )
                    .await;
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
                    let err = format!("⚠️ 處理失敗：{e}");
                    record_chat_with_events(
                        history,
                        &cfg.scope,
                        "assistant",
                        &err,
                        None,
                        "error",
                        &events_buf,
                    )
                    .await;
                    let _ = client.edit_message_text(chat_id, mid, &err).await;
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
            Self {
                replies: Mutex::new(r.iter().map(|s| s.to_string()).collect()),
            }
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

    async fn open_memory_with_url() -> (Memory, String) {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        (Memory::open(&url).await.unwrap(), url)
    }

    async fn open_memory() -> Memory {
        open_memory_with_url().await.0
    }

    fn base_cfg() -> GatewayConfig {
        GatewayConfig {
            scope: String::new(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            planner_preferences: None,
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
        let msg = TgMessage {
            update_id: 1,
            chat_id: 999,
            text: "hi".to_string(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        // No reply, no work.
        assert!(client.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn turn_renders_answer_and_consolidates_messages() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        // planner -> single role; then execute answer with markdown.
        let backend = MockBackend::new(&["oracle", "**重點** 答案"]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "什麼是 BM25".to_string(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;

        // Status bubble edited per role, then deleted.
        assert!(!client.edits.lock().unwrap().is_empty());
        assert!(!client.deletes.lock().unwrap().is_empty());

        // Final answer sent as rendered HTML.
        {
            let sent = client.sent.lock().unwrap();
            assert!(sent
                .iter()
                .any(|s| s.html && s.text.contains("<b>重點</b>")));
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
    async fn turn_records_telegram_user_and_assistant_messages_in_chat_history() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = MockBackend::new(&["oracle", "telegram answer"]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hello from tg".to_string(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
        let messages = history.latest_messages(&thread, 10).await.unwrap();
        assert!(messages
            .iter()
            .any(|m| m.role == "user" && m.content == "hello from tg"));
        assert!(messages
            .iter()
            .any(|m| m.role == "assistant" && m.content == "telegram answer"));
    }

    #[tokio::test]
    async fn command_records_telegram_user_and_reply_messages_in_chat_history() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = MockBackend::new(&[]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "/new".to_string(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
        let messages = history.latest_messages(&thread, 10).await.unwrap();
        assert!(messages
            .iter()
            .any(|m| m.role == "user" && m.content == "/new"));
        assert!(messages
            .iter()
            .any(|m| m.role == "assistant" && m.content.contains("已開新")));
    }

    #[tokio::test]
    async fn set_models_command_persists_and_replies() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::env::set_var("WUKONG_SETTINGS_FILE", &settings_path);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 42,
            text: "/set_models opencode/deepseek-v4-flash-free".to_string(),
        };

        handle_message(&client, &mem, &base_cfg(), &backend, None, &[42], &msg).await;
        std::env::remove_var("WUKONG_SETTINGS_FILE");

        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| s.text.contains("已設定預設模型")));
        drop(sent);

        let saved = wukong_settings::load_settings(&settings_path).unwrap();
        assert_eq!(
            saved.agent.default_model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
    }

    struct ReasoningBackend;
    impl AiBackend for ReasoningBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "答案".to_string(),
                session_id: None,
            })
        }
        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::Reasoning("想一下".to_string()));
            self.run(req).await
        }
    }

    #[tokio::test]
    async fn reasoning_appears_in_status_bubble() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = ReasoningBackend;
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        let edits = client.edits.lock().unwrap();
        assert!(
            edits
                .iter()
                .any(|(_, _, t)| t.contains("💭") && t.contains("想一下")),
            "no reasoning edit: {edits:?}"
        );
    }

    struct ToolBackend;
    impl AiBackend for ToolBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "done".to_string(),
                session_id: None,
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::ToolUse("read".to_string()));
            self.run(req).await
        }
    }

    #[tokio::test]
    async fn tool_use_appears_in_status_bubble() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = ToolBackend;
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        let edits = client.edits.lock().unwrap();
        assert!(
            edits
                .iter()
                .any(|(_, _, text)| text.contains("使用工具 read")),
            "tool edit missing: {edits:?}"
        );
    }

    #[tokio::test]
    async fn turn_records_telegram_events_in_chat_history() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = ReasoningBackend;
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
        let messages = history.latest_messages(&thread, 10).await.unwrap();
        let assistant = messages.iter().find(|m| m.role == "assistant").unwrap();
        assert!(assistant.event_count > 0);
        let events = history.list_events(assistant.id).await.unwrap();
        assert!(events.iter().any(|event| event.kind == "reasoning"));
    }

    #[test]
    fn bubble_text_keeps_full_reasoning() {
        let reasoning = format!("{}{}", "前段".repeat(80), "後段".repeat(80));
        let text = bubble_text(Some("explorer"), &reasoning);

        assert!(text.contains("💭　前段"), "reasoning head missing: {text}");
        assert!(text.contains("後段"), "reasoning tail missing: {text}");
        assert!(
            !text.contains("💭　…"),
            "reasoning should not be presented as a truncated tail: {text}"
        );
    }

    #[tokio::test]
    async fn turn_sends_status_bubble_and_typing() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle", "答案"]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;

        // The first send is the plain status bubble.
        {
            let sent = client.sent.lock().unwrap();
            assert!(sent.iter().any(|s| !s.html && s.text.contains("思考中")));
        }
        assert!(!client.actions.lock().unwrap().is_empty()); // typing emitted
    }

    #[tokio::test]
    async fn new_command_clears_session_and_replies() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        mem.set_agent_session(&scope_for_chat(12), "ses_1")
            .await
            .unwrap();
        let backend = MockBackend::new(&[]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "/new".to_string(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        {
            let sent = client.sent.lock().unwrap();
            assert!(sent.iter().any(|s| s.text.contains("已開新")));
        }
        assert_eq!(mem.agent_session(&scope_for_chat(12)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn unknown_command_still_unsupported() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "/model gpt".to_string(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| s.text.contains("尚未支援")));
    }

    #[tokio::test]
    async fn slash_command_replies_unsupported() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "/reset".to_string(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        let sent = client.sent.lock().unwrap();
        assert!(sent
            .iter()
            .any(|s| s.chat_id == 12 && s.text.contains("尚未支援")));
    }
}
