use crate::turn::WukongError;
use std::sync::atomic::{AtomicU64, Ordering};
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::GatewayError;
use wukong_memory::Memory;

const DEFAULT_COMPACT_EVERY_TURNS: i64 = 20;
const DEFAULT_LEASE_SECS: i64 = 15 * 60;
static NEXT_OWNER: AtomicU64 = AtomicU64::new(1);

pub fn new_owner(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        NEXT_OWNER.fetch_add(1, Ordering::Relaxed)
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionPolicy {
    pub compact_every_turns: i64,
    pub lease_secs: i64,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            compact_every_turns: DEFAULT_COMPACT_EVERY_TURNS,
            lease_secs: DEFAULT_LEASE_SECS,
        }
    }
}

impl SessionPolicy {
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
            compact_every_turns: parse_compact_threshold(
                std::env::var("WUKONG_SESSION_COMPACT_EVERY_TURNS")
                    .ok()
                    .as_deref(),
            ),
            lease_secs: positive_env_i64("WUKONG_SESSION_LEASE_SECS", defaults.lease_secs),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPreparation {
    pub owner: String,
    pub lease_secs: i64,
    pub session_id: Option<String>,
    pub rotated_from: Option<String>,
}

pub async fn prepare_final_session(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
) -> Result<SessionPreparation, WukongError> {
    prepare_final_session_with_policy(memory, backend, cfg, SessionPolicy::from_env()).await
}

pub async fn prepare_final_session_with_policy(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    policy: SessionPolicy,
) -> Result<SessionPreparation, WukongError> {
    let owner = new_owner("turn");
    let state = match memory
        .acquire_agent_session_lease(&cfg.scope, &owner, policy.lease_secs)
        .await
    {
        Ok(Some(state)) => state,
        Ok(None) => return Err(stale_lease_error(&cfg.scope)),
        Err(error) => {
            let _ = memory.release_agent_session_lease(&cfg.scope, &owner).await;
            return Err(error.into());
        }
    };

    if policy.compact_every_turns <= 0
        || state.session_id.is_none()
        || state.turn_count < policy.compact_every_turns
    {
        return Ok(SessionPreparation {
            owner,
            lease_secs: policy.lease_secs,
            session_id: state.session_id,
            rotated_from: None,
        });
    }

    let session_id = state.session_id.expect("checked above");
    match backend
        .compact_session(&session_id, cfg.default_model.as_deref())
        .await
    {
        Ok(_) => {
            match memory
                .mark_agent_session_compacted(&cfg.scope, &owner, &session_id)
                .await
            {
                Ok(true) => {}
                Ok(false) => {
                    let _ = memory.release_agent_session_lease(&cfg.scope, &owner).await;
                    return Err(stale_lease_error(&cfg.scope));
                }
                Err(error) => {
                    let _ = memory.release_agent_session_lease(&cfg.scope, &owner).await;
                    return Err(error.into());
                }
            }
            let reacquired = memory
                .acquire_agent_session_lease(&cfg.scope, &owner, policy.lease_secs)
                .await?
                .ok_or_else(|| stale_lease_error(&cfg.scope))?;
            Ok(SessionPreparation {
                owner,
                lease_secs: policy.lease_secs,
                session_id: reacquired.session_id,
                rotated_from: None,
            })
        }
        Err(error) if is_unsupported_compaction(&error) => {
            eprintln!(
                "session_compaction_unsupported scope={} session_id={}",
                cfg.scope, session_id
            );
            Ok(SessionPreparation {
                owner,
                lease_secs: policy.lease_secs,
                session_id: None,
                rotated_from: Some(session_id),
            })
        }
        Err(error) => {
            eprintln!(
                "session_compaction_failed scope={} session_id={}: {error}",
                cfg.scope, session_id
            );
            // Consume the compaction budget even though the attempt failed.
            // Without this the turn count stays above the threshold, so a
            // persistently failing summarize retries on *every* subsequent turn
            // — each retry being a full-session LLM call on the agent backend.
            // Spacing retries by `compact_every_turns` keeps the session usable
            // while bounding the cost of a backend that cannot compact.
            if let Err(reset_error) = memory
                .defer_agent_session_compaction(&cfg.scope, &owner)
                .await
            {
                eprintln!(
                    "session_compaction_backoff_failed scope={}: {reset_error}",
                    cfg.scope
                );
            }
            Ok(SessionPreparation {
                owner,
                lease_secs: policy.lease_secs,
                session_id: Some(session_id),
                rotated_from: None,
            })
        }
    }
}

pub fn parse_compact_threshold(value: Option<&str>) -> i64 {
    value
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value >= 0)
        .unwrap_or(DEFAULT_COMPACT_EVERY_TURNS)
}

pub fn is_unsupported_compaction(error: &GatewayError) -> bool {
    matches!(
        error,
        GatewayError::AgentFailed {
            code: Some(404 | 405),
            ..
        }
    )
}

fn stale_lease_error(scope: &str) -> WukongError {
    WukongError::Backend(GatewayError::AgentFailed {
        code: None,
        stderr: format!("session lease unavailable for scope {scope}"),
    })
}

fn positive_env_i64(name: &str, default: i64) -> i64 {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compact_threshold_parser_supports_disable() {
        assert_eq!(parse_compact_threshold(None), 20);
        assert_eq!(parse_compact_threshold(Some("0")), 0);
        assert_eq!(parse_compact_threshold(Some("bad")), 20);
        assert_eq!(parse_compact_threshold(Some("-1")), 20);
    }

    #[test]
    fn only_404_and_405_are_unsupported() {
        assert!(is_unsupported_compaction(&GatewayError::AgentFailed {
            code: Some(404),
            stderr: String::new(),
        }));
        assert!(is_unsupported_compaction(&GatewayError::AgentFailed {
            code: Some(405),
            stderr: String::new(),
        }));
        assert!(!is_unsupported_compaction(&GatewayError::AgentFailed {
            code: Some(500),
            stderr: String::new(),
        }));
    }
}
