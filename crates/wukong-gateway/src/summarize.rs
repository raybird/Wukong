//! Opencode-backed Summarizer: bridges the async AiBackend to the synchronous
//! `wukong_memory::Summarizer` trait. Runs on the current tokio runtime via
//! block_in_place (requires the multi-thread runtime, which #[tokio::main] uses
//! by default).

use crate::backend::{AgentRequest, AiBackend};
use wukong_memory::error::Result as MemResult;
use wukong_memory::{MemoryError, Summarizer};

/// Wraps an AiBackend so the memory layer can request summaries.
pub struct OpencodeSummarizer<'a, B: AiBackend> {
    pub backend: &'a B,
    pub handle: tokio::runtime::Handle,
}

impl<'a, B: AiBackend> OpencodeSummarizer<'a, B> {
    pub fn new(backend: &'a B) -> Self {
        Self {
            backend,
            handle: tokio::runtime::Handle::current(),
        }
    }
}

impl<B: AiBackend + Sync> Summarizer for OpencodeSummarizer<'_, B> {
    fn summarize(&self, texts: &[String]) -> MemResult<String> {
        let prompt = format!(
            "請把以下記憶濃縮成一段精簡摘要,保留關鍵決策與事實,只輸出摘要本身:\n\n{}",
            texts.join("\n")
        );
        let fut = self.backend.run_ephemeral(AgentRequest {
            prompt,
            session_id: None,
            thinking: false,
            model: None,
            agent: None,
            tool_overrides: std::collections::BTreeMap::new(),
            attachments: Vec::new(),
        });
        let resp = tokio::task::block_in_place(|| self.handle.block_on(fut))
            .map_err(|e| MemoryError::Other(format!("summarizer backend failed: {e}")))?;
        Ok(resp.text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backend::AgentResponse;
    use crate::error::GatewayError;

    struct Echo;
    impl AiBackend for Echo {
        async fn run(&self, req: AgentRequest) -> std::result::Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: format!("SUM[{}]", req.prompt.len()),
                session_id: None,
            })
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn summarizer_calls_backend() {
        let b = Echo;
        let s = OpencodeSummarizer::new(&b);
        let out = s.summarize(&["a".to_string(), "b".to_string()]).unwrap();
        assert!(out.starts_with("SUM["));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn summarizer_cleans_up_ephemeral_session() {
        struct CleanupEcho {
            deleted: std::sync::Mutex<Vec<String>>,
        }

        impl AiBackend for CleanupEcho {
            async fn run(
                &self,
                _req: AgentRequest,
            ) -> std::result::Result<AgentResponse, GatewayError> {
                Ok(AgentResponse {
                    text: "summary".to_string(),
                    session_id: Some("ses_helper".to_string()),
                })
            }

            async fn delete_session(&self, session_id: &str) -> Result<(), GatewayError> {
                self.deleted.lock().unwrap().push(session_id.to_string());
                Ok(())
            }
        }

        let backend = CleanupEcho {
            deleted: std::sync::Mutex::new(Vec::new()),
        };
        let summarizer = OpencodeSummarizer::new(&backend);
        summarizer.summarize(&["a".to_string()]).unwrap();
        assert_eq!(
            backend.deleted.lock().unwrap().as_slice(),
            &["ses_helper".to_string()]
        );
    }
}
