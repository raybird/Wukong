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
