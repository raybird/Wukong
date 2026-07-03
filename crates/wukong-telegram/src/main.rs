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

#[tokio::main]
async fn main() {
    let mut tg_settings = load_effective_telegram_settings();
    while !has_token(&tg_settings) {
        eprintln!("🐵 wukong-telegram 等待設定：請在 Web /settings 填入 Telegram bot token。或設定 WUKONG_TG_TOKEN。每 5 秒重新檢查。");
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
        tg_settings = load_effective_telegram_settings();
    }
    let mut token = tg_settings.token.clone();
    let mut allow = parse_allowlist(&tg_settings.allowed);
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
        Ok(store) => Some(store),
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
    let backend = build_backend_from_env(agent_command, workspace_dir());

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
    let pending_questions = Arc::new(Mutex::new(PendingQuestions::new()));
    loop {
        let latest = load_effective_telegram_settings();
        if has_token(&latest) && (latest.token != token || latest.allowed != tg_settings.allowed) {
            eprintln!("🐵 wukong-telegram 偵測到設定更新，套用新的 token/allowlist。");
            token = latest.token.clone();
            allow = parse_allowlist(&latest.allowed);
            tg_settings = latest;
            client = ReqwestTgClient::new(&token);
            offset = 0;
        }
        match client.get_updates(offset).await {
            Ok(json) => {
                cleanup_expired_questions(&client, &backend, pending_questions.clone()).await;
                if let Some(max) = highest_update_id(&json) {
                    offset = max + 1;
                }
                for event in parse_update_events(&json) {
                    match event {
                        TgUpdateEvent::Message(msg) => {
                            handle_message_with_responder(
                                &client,
                                &memory,
                                &base_cfg,
                                &backend,
                                &backend,
                                history.as_ref(),
                                &allow,
                                pending_questions.clone(),
                                &msg,
                            )
                            .await;
                        }
                        TgUpdateEvent::CallbackQuery(callback) => {
                            handle_callback_query(
                                &client,
                                &backend,
                                pending_questions.clone(),
                                &callback,
                            )
                            .await;
                        }
                    }
                }
            }
            Err(e) => {
                eprintln!("get_updates error: {e}");
                tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
