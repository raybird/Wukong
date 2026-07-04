use crate::backend::{AgentRequest, AiBackend};
use crate::config::GatewayConfig;
use crate::error::GatewayError;
use crate::prompt::compose_prompt;
use wukong_memory::{Memory, MemoryItem, MemoryKind, RecallMode, RecallQuery, RememberInput};

/// Run one assistant turn: recall relevant memory, compose the prompt, invoke
/// the backend, persist the turn, and return the response text.
pub async fn run_turn(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    input: &str,
) -> Result<String, GatewayError> {
    let recall = memory
        .recall(RecallQuery {
            query: input.to_string(),
            top_k: cfg.recall_top_k,
            scope: Some(cfg.scope.clone()),
            mode: RecallMode::Hybrid,
        })
        .await?;

    let prompt = compose_prompt(&recall.data, input);

    let resp = backend
        .run(AgentRequest {
            prompt,
            session_id: None,
            thinking: cfg.thinking,
            model: None,
            agent: None,
            tool_overrides: std::collections::BTreeMap::new(),
            attachments: Vec::new(),
        })
        .await?;

    memory
        .remember(RememberInput {
            scope: cfg.scope.clone(),
            session_id: None,
            items: vec![
                MemoryItem {
                    kind: MemoryKind::Event,
                    text: format!("User: {input}"),
                    importance: None,
                },
                MemoryItem {
                    kind: MemoryKind::Event,
                    text: format!("Assistant: {}", resp.text),
                    importance: None,
                },
            ],
        })
        .await?;

    Ok(resp.text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AgentResponse;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;

    /// Records the prompt it receives and returns a canned reply.
    struct MockBackend {
        captured: Mutex<Option<String>>,
        reply: String,
    }

    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            *self.captured.lock().unwrap() = Some(req.prompt);
            Ok(AgentResponse {
                text: self.reply.clone(),
                session_id: None,
            })
        }
    }

    async fn open_memory() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    fn test_cfg(scope: &str) -> GatewayConfig {
        GatewayConfig {
            scope: scope.to_string(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: true,
        }
    }

    #[tokio::test]
    async fn run_turn_returns_reply_and_persists_turn() {
        let mem = open_memory().await;
        let backend = MockBackend {
            captured: Mutex::new(None),
            reply: "pong".to_string(),
        };
        let out = run_turn(&mem, &backend, &test_cfg("project:T"), "ping")
            .await
            .unwrap();
        assert_eq!(out, "pong");

        // The turn was persisted and is recallable.
        let r = mem
            .recall(RecallQuery {
                query: "ping".to_string(),
                top_k: 10,
                scope: Some("project:T".to_string()),
                mode: RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("User: ping")));
        assert!(r.data.iter().any(|h| h.text.contains("Assistant: pong")));
    }

    #[tokio::test]
    async fn prior_memory_is_injected_into_prompt() {
        let mem = open_memory().await;
        mem.remember(RememberInput {
            scope: "project:T".to_string(),
            session_id: None,
            items: vec![MemoryItem {
                kind: MemoryKind::Event,
                text: "earlier decision about Rust".to_string(),
                importance: None,
            }],
        })
        .await
        .unwrap();

        let backend = MockBackend {
            captured: Mutex::new(None),
            reply: "ok".to_string(),
        };
        run_turn(&mem, &backend, &test_cfg("project:T"), "tell me about rust")
            .await
            .unwrap();

        let captured = backend.captured.lock().unwrap().clone().unwrap();
        assert!(captured.contains("[相關記憶]"));
        assert!(captured.contains("earlier decision about Rust"));
    }
}
