//! Session control commands shared by every surface (REPL, Telegram, Web, CLI).

use crate::{run_turn_session_passthrough, WukongError};
use std::path::Path;
use wukong_gateway::backend::{AiBackend, OpencodeUtility};
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::GatewayError;
use wukong_memory::Memory;

/// A session control command parsed from a leading-slash input.
#[derive(Debug, Clone, PartialEq)]
pub enum SessionCommand {
    /// Start a fresh opencode session for this scope.
    New,
    /// Passthrough `/compact` to the current opencode session.
    Compact,
    /// List opencode providers.
    Providers,
    /// List opencode models.
    Models,
    /// Persist the system-wide default opencode model.
    SetModels(String),
}

/// Map a command name (without the leading '/') to a SessionCommand.
pub fn parse_session_command(name: &str, args: &str) -> Option<SessionCommand> {
    match name {
        "new" => Some(SessionCommand::New),
        "compact" => Some(SessionCommand::Compact),
        "providers" => Some(SessionCommand::Providers),
        "models" => Some(SessionCommand::Models),
        "set_models" => Some(SessionCommand::SetModels(args.trim().to_string())),
        _ => None,
    }
}

/// Execute a session command, returning user-facing reply text.
pub async fn run_session_command(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    settings_path: &Path,
    cmd: SessionCommand,
) -> Result<String, WukongError> {
    match cmd {
        SessionCommand::New => {
            memory.clear_agent_session(&cfg.scope).await?;
            Ok("🐵 已開新 context".to_string())
        }
        SessionCommand::Compact => match memory.agent_session(&cfg.scope).await? {
            None => Ok("🐵 尚無對話可壓縮".to_string()),
            Some(id) => {
                let text = run_turn_session_passthrough(backend, &id, "/compact").await?;
                Ok(format!("🐵 已送出壓縮指令：\n{text}"))
            }
        },
        SessionCommand::Providers => {
            let util = OpencodeUtility::from_agent_command(
                &cfg.agent_command,
                wukong_gateway::workspace_dir(),
            );
            Ok(util.run_fixed(&["providers", "list"]).await?)
        }
        SessionCommand::Models => {
            let util = OpencodeUtility::from_agent_command(
                &cfg.agent_command,
                wukong_gateway::workspace_dir(),
            );
            Ok(util.run_fixed(&["models"]).await?)
        }
        SessionCommand::SetModels(model) => {
            let model = model.trim();
            if model.is_empty() {
                return Ok("用法：/set_models opencode/deepseek-v4-flash-free".to_string());
            }
            let mut settings =
                wukong_settings::load_settings(settings_path).map_err(settings_error)?;
            settings.agent.default_model = Some(model.to_string());
            wukong_settings::save_settings(settings_path, &settings).map_err(settings_error)?;
            Ok(format!("🐵 已設定預設模型：{model}"))
        }
    }
}

fn settings_error(e: wukong_settings::SettingsError) -> WukongError {
    WukongError::Backend(GatewayError::AgentFailed {
        code: None,
        stderr: e.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<String>>,
        prompts: Mutex<Vec<String>>,
        sessions: Mutex<Vec<Option<String>>>,
    }
    impl MockBackend {
        fn new(r: &[&str]) -> Self {
            Self {
                replies: Mutex::new(r.iter().map(|s| s.to_string()).collect()),
                prompts: Mutex::new(Vec::new()),
                sessions: Mutex::new(Vec::new()),
            }
        }
    }
    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.prompts.lock().unwrap().push(req.prompt);
            self.sessions.lock().unwrap().push(req.session_id);
            let text = self.replies.lock().unwrap().pop_front().unwrap_or_default();
            Ok(AgentResponse {
                text,
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

    fn cfg() -> GatewayConfig {
        GatewayConfig {
            scope: "global".to_string(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        }
    }

    #[test]
    fn parses_known_commands() {
        assert_eq!(parse_session_command("new", ""), Some(SessionCommand::New));
        assert_eq!(
            parse_session_command("compact", ""),
            Some(SessionCommand::Compact)
        );
        assert_eq!(
            parse_session_command("providers", ""),
            Some(SessionCommand::Providers)
        );
        assert_eq!(
            parse_session_command("models", ""),
            Some(SessionCommand::Models)
        );
        assert_eq!(
            parse_session_command("set_models", "opencode/deepseek-v4-flash-free"),
            Some(SessionCommand::SetModels(
                "opencode/deepseek-v4-flash-free".to_string()
            ))
        );
        assert_eq!(parse_session_command("model", "gpt"), None);
    }

    #[tokio::test]
    async fn set_models_persists_default_model() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");

        let reply = run_session_command(
            &mem,
            &backend,
            &cfg(),
            &settings_path,
            SessionCommand::SetModels("opencode/deepseek-v4-flash-free".to_string()),
        )
        .await
        .unwrap();

        assert!(reply.contains("opencode/deepseek-v4-flash-free"));
        let saved = wukong_settings::load_settings(&settings_path).unwrap();
        assert_eq!(
            saved.agent.default_model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
    }

    #[tokio::test]
    async fn set_models_without_model_returns_usage() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");

        let reply = run_session_command(
            &mem,
            &backend,
            &cfg(),
            &settings_path,
            SessionCommand::SetModels(String::new()),
        )
        .await
        .unwrap();

        assert!(reply.contains("用法：/set_models"));
        assert_eq!(
            wukong_settings::load_settings(&settings_path)
                .unwrap()
                .agent
                .default_model,
            None
        );
    }

    #[tokio::test]
    async fn new_clears_session() {
        let mem = open_memory().await;
        mem.set_agent_session("global", "ses_1").await.unwrap();
        let backend = MockBackend::new(&[]);
        let settings_path = tempfile::NamedTempFile::new().unwrap().path().to_path_buf();
        let reply =
            run_session_command(&mem, &backend, &cfg(), &settings_path, SessionCommand::New)
                .await
                .unwrap();
        assert!(reply.contains("已開新"));
        assert_eq!(mem.agent_session("global").await.unwrap(), None);
        assert!(backend.prompts.lock().unwrap().is_empty()); // no model call
    }

    #[tokio::test]
    async fn compact_without_session_does_not_call_backend() {
        let mem = open_memory().await;
        let backend = MockBackend::new(&["ignored"]);
        let settings_path = tempfile::NamedTempFile::new().unwrap().path().to_path_buf();
        let reply = run_session_command(
            &mem,
            &backend,
            &cfg(),
            &settings_path,
            SessionCommand::Compact,
        )
        .await
        .unwrap();
        assert!(reply.contains("尚無對話"));
        assert!(backend.prompts.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn compact_passes_slash_compact_to_stored_session() {
        let mem = open_memory().await;
        mem.set_agent_session("global", "ses_42").await.unwrap();
        let backend = MockBackend::new(&["compacted ok"]);
        let settings_path = tempfile::NamedTempFile::new().unwrap().path().to_path_buf();
        let reply = run_session_command(
            &mem,
            &backend,
            &cfg(),
            &settings_path,
            SessionCommand::Compact,
        )
        .await
        .unwrap();
        assert!(reply.contains("compacted ok"));
        {
            let prompts = backend.prompts.lock().unwrap();
            assert_eq!(prompts.len(), 1);
            assert_eq!(prompts[0], "/compact");
        }
        let sessions = backend.sessions.lock().unwrap();
        assert_eq!(sessions[0], Some("ses_42".to_string()));
    }
}
