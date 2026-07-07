use std::sync::Arc;
use std::sync::Mutex;
use wukong_chat_history::ChatHistoryStore;
use wukong_gateway::backend::build_backend_from_env;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::workspace_dir;
use wukong_memory::Memory;
use wukong_telegram::client::{ReqwestTgClient, TgClient};
use wukong_telegram::dispatch::{
    cleanup_expired_questions, handle_callback_query, handle_message_with_responder,
    PendingQuestions,
};
use wukong_telegram::parse::{
    highest_update_id, parse_allowlist, parse_update_events, TgUpdateEvent,
};

fn load_effective_telegram_settings() -> wukong_settings::TelegramSettings {
    let path = wukong_settings::default_settings_path();
    let file = wukong_settings::load_settings(&path).unwrap_or_default();
    wukong_settings::effective_telegram_settings(&file)
}

fn has_token(settings: &wukong_settings::TelegramSettings) -> bool {
    !settings.token.trim().is_empty()
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    tokio::task::LocalSet::new().run_until(run_bot()).await;
}

async fn run_bot() {
    let mut tg_settings = load_effective_telegram_settings();
    while !has_token(&tg_settings) {
        eprintln!("🐵 wukong-telegram 等待設定：請在 Web /settings 填入 Telegram bot token。或設定 WUKONG_TG_TOKEN。每 5 秒重新檢查。");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        tg_settings = load_effective_telegram_settings();
    }
    let mut token = tg_settings.token.clone();
    let allow = parse_allowlist(&tg_settings.allowed);
    if allow.is_empty() {
        eprintln!(
            "warning: WUKONG_TG_ALLOWED/shared allowed is empty — all messages will be ignored"
        );
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

    let history = match ChatHistoryStore::open(&db_url).await {
        Ok(store) => Some(Arc::new(store)),
        Err(e) => {
            eprintln!("warning: chat history disabled for telegram: {e}");
            None
        }
    };

    let agent_command = std::env::var("WUKONG_AGENT_CMD")
        .ok()
        .map(|s| {
            s.split_whitespace()
                .map(|t| t.to_string())
                .collect::<Vec<_>>()
        })
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| vec!["opencode".to_string(), "run".to_string()]);
    let backend = Arc::new(build_backend_from_env(agent_command, workspace_dir()));

    let base_cfg = GatewayConfig {
        scope: String::new(),
        db_url,
        agent_command: vec![],
        default_model: None,
        planner_preferences: None,
        thinking: true,
        recall_top_k: 5,
        stream: false,
    };

    let mut client = ReqwestTgClient::new(&token);
    eprintln!(
        "🐵 wukong-telegram 上線（long-poll）。允許 {} 個 chat。",
        allow.len()
    );

    let mut offset: i64 = 0;
    let base_cfg = Arc::new(base_cfg);
    let mut allow = Arc::new(allow);
    let pending_questions = Arc::new(Mutex::new(PendingQuestions::new()));
    loop {
        let latest = load_effective_telegram_settings();
        if has_token(&latest) && (latest.token != token || latest.allowed != tg_settings.allowed) {
            eprintln!("🐵 wukong-telegram 偵測到設定更新，套用新的 token/allowlist。");
            token = latest.token.clone();
            allow = Arc::new(parse_allowlist(&latest.allowed));
            tg_settings = latest;
            client = ReqwestTgClient::new(&token);
            offset = 0;
        }
        match client.get_updates(offset).await {
            Ok(json) => {
                cleanup_expired_questions(&client, &*backend, pending_questions.clone()).await;
                if let Some(max) = highest_update_id(&json) {
                    offset = max + 1;
                }
                for event in parse_update_events(&json) {
                    dispatch_update_event(
                        client.clone(),
                        memory.clone(),
                        base_cfg.clone(),
                        backend.clone(),
                        history.clone(),
                        allow.clone(),
                        pending_questions.clone(),
                        event,
                    )
                    .await;
                }
            }
            Err(e) => {
                eprintln!("get_updates error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn dispatch_update_event<C, B>(
    client: C,
    memory: Arc<Memory>,
    base_cfg: Arc<GatewayConfig>,
    backend: Arc<B>,
    history: Option<Arc<ChatHistoryStore>>,
    allow: Arc<Vec<i64>>,
    pending_questions: Arc<Mutex<PendingQuestions>>,
    event: TgUpdateEvent,
) where
    C: TgClient + Clone + Send + Sync + 'static,
    B: wukong_gateway::backend::AiBackend
        + wukong_telegram::dispatch::QuestionResponder
        + Sync
        + 'static,
{
    match event {
        TgUpdateEvent::Message(msg) => {
            tokio::task::spawn_local(async move {
                handle_message_with_responder(
                    &client,
                    &memory,
                    &base_cfg,
                    &*backend,
                    &*backend,
                    history.as_deref(),
                    &allow,
                    pending_questions,
                    &msg,
                )
                .await;
            });
        }
        TgUpdateEvent::CallbackQuery(callback) => {
            handle_callback_query(&client, &*backend, &allow, pending_questions, &callback).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use wukong_gateway::backend::{AgentRequest, AgentResponse, AiBackend};
    use wukong_gateway::stream::{QuestionInfo, QuestionRequest};
    use wukong_gateway::{GatewayError, StreamEvent};
    use wukong_telegram::dispatch::{PendingQuestion, QuestionResponder};
    use wukong_telegram::parse::{TgCallbackQuery, TgMessage};
    use wukong_tg_client::client::mock::MockTgClient;

    struct WaitingBackend;

    impl AiBackend for WaitingBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            std::future::pending().await
        }

        async fn run_streaming(
            &self,
            _req: AgentRequest,
            on_event: &mut dyn FnMut(StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(StreamEvent::QuestionRequest(QuestionRequest {
                request_id: "que_1".to_string(),
                session_id: "ses_1".to_string(),
                questions: vec![QuestionInfo {
                    question: "選一個".to_string(),
                    header: "".to_string(),
                    multiple: false,
                    custom: false,
                    options: vec![wukong_gateway::stream::QuestionOption {
                        label: "A".to_string(),
                        description: "".to_string(),
                    }],
                }],
            }));
            std::future::pending().await
        }
    }

    impl QuestionResponder for WaitingBackend {
        async fn reply_question(
            &self,
            _session_id: &str,
            _request_id: &str,
            _answers: Vec<Vec<String>>,
        ) -> Result<(), GatewayError> {
            Ok(())
        }

        async fn reject_question(
            &self,
            _session_id: &str,
            _request_id: &str,
        ) -> Result<(), GatewayError> {
            Ok(())
        }
    }

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

    fn test_cfg() -> Arc<GatewayConfig> {
        Arc::new(GatewayConfig {
            scope: String::new(),
            db_url: "sqlite::memory:".to_string(),
            agent_command: Vec::new(),
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        })
    }

    #[tokio::test]
    async fn update_dispatch_does_not_block_callbacks_behind_waiting_turns() {
        let client = MockTgClient::default();
        let memory = Arc::new(Memory::open("sqlite::memory:").await.unwrap());
        let backend = Arc::new(WaitingBackend);
        let history = None;
        let allow = Arc::new(vec![12]);
        let pending_questions = Arc::new(Mutex::new(PendingQuestions::new()));
        pending_questions.lock().unwrap().insert(
            12,
            PendingQuestion {
                chat_id: 12,
                session_id: "ses_1".to_string(),
                request_id: "que_1".to_string(),
                questions: vec![QuestionInfo {
                    question: "選一個".to_string(),
                    header: "".to_string(),
                    multiple: false,
                    custom: false,
                    options: vec![wukong_gateway::stream::QuestionOption {
                        label: "A".to_string(),
                        description: "".to_string(),
                    }],
                }],
                current_question_index: 0,
                answers: vec![Vec::new()],
                waiting_custom_question_index: None,
                deadline: std::time::Instant::now() + std::time::Duration::from_secs(60),
                message_id: Some(99),
            },
        );

        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };
        tokio::task::LocalSet::new()
            .run_until(async {
                tokio::time::timeout(
                    std::time::Duration::from_millis(50),
                    dispatch_update_event(
                        client.clone(),
                        memory.clone(),
                        test_cfg(),
                        backend.clone(),
                        history.clone(),
                        allow.clone(),
                        pending_questions.clone(),
                        TgUpdateEvent::Message(msg),
                    ),
                )
                .await
                .expect("message dispatch should return immediately");

                dispatch_update_event(
                    client.clone(),
                    memory,
                    test_cfg(),
                    backend,
                    history,
                    allow,
                    pending_questions,
                    TgUpdateEvent::CallbackQuery(TgCallbackQuery {
                        update_id: 2,
                        callback_query_id: "cb_1".to_string(),
                        chat_id: 12,
                        message_id: 99,
                        data: "q:que_1:pick:0:0".to_string(),
                    }),
                )
                .await;
            })
            .await;

        assert_eq!(client.callback_answers.lock().unwrap().len(), 1);
    }
}
