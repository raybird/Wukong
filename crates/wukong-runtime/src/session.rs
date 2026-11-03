use crate::turn::WukongError;
use std::sync::atomic::{AtomicU64, Ordering};
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::GatewayError;
use wukong_memory::Memory;

const DEFAULT_COMPACT_EVERY_TURNS: i64 = 20;
const DEFAULT_LEASE_SECS: i64 = 15 * 60;
/// session 建立超過這麼久，下一個壓縮時點就輪替。7 天是暫定值，等正式主機的量測
/// 定案（issue-0028 TBD-1）。
const ROTATE_AFTER_MS: i64 = 7 * 24 * 60 * 60 * 1000;
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
    // 2026-11-03（issue-0028）：壓縮只縮短 context，opencode 的 event／part 一筆也不
    // 少，長期續接的 session 會一直變大。到壓縮時點時先看年齡，太舊就改為輪替，舊的
    // 由 `run_turn` 取得新 session 後刪除。只在這個時點查，因為本機模式每查一次就要
    // 多啟動一個 opencode serve。
    let age = hold_session_lease(
        memory,
        &cfg.scope,
        &owner,
        policy.lease_secs,
        backend.session_created_ms(&session_id),
    )
    .await;
    match age {
        Ok(created_ms) if crate::util::now_unix() * 1000 - created_ms > ROTATE_AFTER_MS => {
            eprintln!(
                "session_rotation_due scope={} session_id={} created_ms={created_ms}",
                cfg.scope, session_id
            );
            return Ok(SessionPreparation {
                owner,
                lease_secs: policy.lease_secs,
                session_id: None,
                rotated_from: Some(session_id),
            });
        }
        Ok(_) => {}
        Err(error) => eprintln!(
            "warning: session_age_unknown scope={} session_id={}: {error}",
            cfg.scope, session_id
        ),
    }
    let compaction = backend.compact_session(&session_id, cfg.default_model.as_deref());
    match hold_session_lease(memory, &cfg.scope, &owner, policy.lease_secs, compaction).await {
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

/// 2026-10-21（issue-0013）：在 `work` 執行期間持續持有 `scope` 的 session lease。
/// 單次 backend 呼叫可以跑得比 lease 久，不續租的話，別的回合會在中途拿走 lease。
pub async fn hold_session_lease<F: std::future::Future>(
    memory: &Memory,
    scope: &str,
    owner: &str,
    lease_secs: i64,
    work: F,
) -> F::Output {
    // 每隔 lease 的三分之一續租一次；續租和工作在同一個 task，工作結束或被丟掉，
    // 續租就跟著停，lease 照原本的時間到期。
    let every = std::time::Duration::from_millis(lease_secs.max(1) as u64 * 1000 / 3);
    let mut ticker = tokio::time::interval_at(tokio::time::Instant::now() + every, every);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    tokio::pin!(work);
    loop {
        tokio::select! {
            output = &mut work => return output,
            _ = ticker.tick() => {
                match memory.renew_agent_session_lease(scope, owner, lease_secs).await {
                    Ok(true) => {}
                    Ok(false) => eprintln!("session_lease_renew_lost scope={scope} owner={owner}"),
                    Err(err) => eprintln!("session_lease_renew_failed scope={scope}: {err}"),
                }
            }
        }
    }
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
    use std::time::Duration;

    async fn open_memory() -> Memory {
        let file = tempfile::NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    // SCN-003：工作跑得比 lease 久，別的 owner 在中途拿不到 lease。
    #[tokio::test]
    async fn lease_stays_held_while_work_outlives_it() {
        let memory = open_memory().await;
        memory
            .acquire_agent_session_lease("agent:schedule-j1", "turn-a", 2)
            .await
            .unwrap()
            .unwrap();

        let work = hold_session_lease(&memory, "agent:schedule-j1", "turn-a", 2, async {
            tokio::time::sleep(Duration::from_millis(3500)).await;
        });
        let intruder = async {
            tokio::time::sleep(Duration::from_millis(3000)).await;
            memory
                .acquire_agent_session_lease("agent:schedule-j1", "turn-b", 2)
                .await
                .unwrap()
        };
        let ((), stolen) = tokio::join!(work, intruder);

        assert!(
            stolen.is_none(),
            "lease 在工作途中被 turn-b 拿走：{stolen:?}"
        );
    }

    /// 壓縮要跑 3.5 秒，比 2 秒的 lease 久。
    struct SlowCompaction;

    impl AiBackend for SlowCompaction {
        async fn run(
            &self,
            _req: wukong_gateway::backend::AgentRequest,
        ) -> Result<wukong_gateway::backend::AgentResponse, GatewayError> {
            unreachable!("這支測試只會壓縮")
        }

        async fn compact_session(
            &self,
            session_id: &str,
            _model: Option<&str>,
        ) -> Result<wukong_gateway::backend::AgentResponse, GatewayError> {
            tokio::time::sleep(Duration::from_millis(3500)).await;
            Ok(wukong_gateway::backend::AgentResponse {
                text: "ok".to_string(),
                session_id: Some(session_id.to_string()),
            })
        }
    }

    // SCN-003：壓縮呼叫跑得比 lease 久時，也持續持有 lease，壓縮後照常取得 session。
    #[tokio::test]
    async fn lease_stays_held_during_a_long_compaction() {
        let memory = open_memory().await;
        // 先完成一個回合，turn_count 變成 1，達到下面的壓縮門檻。
        memory
            .acquire_agent_session_lease("project:T", "seed", 300)
            .await
            .unwrap()
            .unwrap();
        memory
            .finish_agent_session_turn("project:T", "seed", Some("ses_1"))
            .await
            .unwrap()
            .unwrap();
        let cfg = GatewayConfig {
            scope: "project:T".to_string(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: true,
        };
        let prepare = prepare_final_session_with_policy(
            &memory,
            &SlowCompaction,
            &cfg,
            SessionPolicy {
                compact_every_turns: 1,
                lease_secs: 2,
            },
        );
        let intruder = async {
            tokio::time::sleep(Duration::from_millis(3000)).await;
            memory
                .acquire_agent_session_lease("project:T", "other", 2)
                .await
                .unwrap()
        };
        let (prepared, stolen) = tokio::join!(prepare, intruder);

        assert!(stolen.is_none(), "壓縮期間 lease 被拿走：{stolen:?}");
        assert_eq!(prepared.unwrap().session_id.as_deref(), Some("ses_1"));
    }

    // SCN-005：工作被中止（future 被丟掉）後就不再續租，lease 照原本的時間到期。
    #[tokio::test]
    async fn lease_expires_once_the_work_is_dropped() {
        let memory = open_memory().await;
        memory
            .acquire_agent_session_lease("agent:schedule-j1", "turn-a", 2)
            .await
            .unwrap()
            .unwrap();

        let held = hold_session_lease(
            &memory,
            "agent:schedule-j1",
            "turn-a",
            2,
            std::future::pending::<()>(),
        );
        assert!(tokio::time::timeout(Duration::from_millis(1500), held)
            .await
            .is_err());
        tokio::time::sleep(Duration::from_millis(3000)).await;

        assert!(memory
            .acquire_agent_session_lease("agent:schedule-j1", "turn-b", 2)
            .await
            .unwrap()
            .is_some());
    }

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

    const DAY_MS: i64 = 24 * 60 * 60 * 1000;

    fn now_ms() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64
    }

    /// 回報固定建立時間的 backend，記下查了幾次年齡、做了幾次壓縮。
    struct AgedSession {
        created_ms: Option<i64>,
        age_queries: std::sync::Mutex<Vec<String>>,
        compactions: std::sync::Mutex<Vec<String>>,
    }

    impl AgedSession {
        fn created(created_ms: Option<i64>) -> Self {
            Self {
                created_ms,
                age_queries: Default::default(),
                compactions: Default::default(),
            }
        }
    }

    impl AiBackend for AgedSession {
        async fn run(
            &self,
            _req: wukong_gateway::backend::AgentRequest,
        ) -> Result<wukong_gateway::backend::AgentResponse, GatewayError> {
            unreachable!("準備 session 不會執行回合")
        }

        async fn session_created_ms(&self, session_id: &str) -> Result<i64, GatewayError> {
            self.age_queries
                .lock()
                .unwrap()
                .push(session_id.to_string());
            self.created_ms.ok_or_else(|| GatewayError::AgentFailed {
                code: None,
                stderr: "no time.created".to_string(),
            })
        }

        async fn compact_session(
            &self,
            session_id: &str,
            _model: Option<&str>,
        ) -> Result<wukong_gateway::backend::AgentResponse, GatewayError> {
            self.compactions
                .lock()
                .unwrap()
                .push(session_id.to_string());
            Ok(wukong_gateway::backend::AgentResponse {
                text: "ok".to_string(),
                session_id: Some(session_id.to_string()),
            })
        }
    }

    fn cfg(scope: &str) -> GatewayConfig {
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

    /// 讓 scope 指向 `ses_1`，並累積 `turns` 個回合。
    async fn seeded(scope: &str, turns: i64) -> Memory {
        let memory = open_memory().await;
        for i in 0..turns {
            let owner = format!("seed-{i}");
            memory
                .acquire_agent_session_lease(scope, &owner, 300)
                .await
                .unwrap()
                .unwrap();
            memory
                .finish_agent_session_turn(scope, &owner, Some("ses_1"))
                .await
                .unwrap()
                .unwrap();
        }
        memory
    }

    const EVERY_TURN: SessionPolicy = SessionPolicy {
        compact_every_turns: 1,
        lease_secs: 300,
    };

    // issue-0028 SCN-004：到壓縮時點、session 建立超過 7 天，就改為輪替、不壓縮。
    #[tokio::test]
    async fn an_old_session_rotates_instead_of_compacting() {
        let memory = seeded("user:tg-1", 1).await;
        let backend = AgedSession::created(Some(now_ms() - 8 * DAY_MS));
        let prepared =
            prepare_final_session_with_policy(&memory, &backend, &cfg("user:tg-1"), EVERY_TURN)
                .await
                .unwrap();
        assert_eq!(prepared.session_id, None);
        assert_eq!(prepared.rotated_from.as_deref(), Some("ses_1"));
        assert_eq!(backend.age_queries.lock().unwrap().clone(), ["ses_1"]);
        assert!(backend.compactions.lock().unwrap().is_empty());
    }

    // issue-0028 SCN-005：未滿 7 天照原本壓縮、續接同一個 session。
    #[tokio::test]
    async fn a_young_session_is_compacted_as_before() {
        let memory = seeded("user:tg-1", 1).await;
        let backend = AgedSession::created(Some(now_ms() - 6 * DAY_MS));
        let prepared =
            prepare_final_session_with_policy(&memory, &backend, &cfg("user:tg-1"), EVERY_TURN)
                .await
                .unwrap();
        assert_eq!(prepared.session_id.as_deref(), Some("ses_1"));
        assert_eq!(prepared.rotated_from, None);
        assert_eq!(backend.compactions.lock().unwrap().clone(), ["ses_1"]);
    }

    // issue-0028 SCN-010：還沒到壓縮時點就不查年齡，即使 session 已經很舊。
    #[tokio::test]
    async fn the_age_is_not_queried_before_the_compaction_point() {
        let memory = seeded("user:tg-1", 1).await;
        let backend = AgedSession::created(Some(now_ms() - 8 * DAY_MS));
        let policy = SessionPolicy {
            compact_every_turns: 2,
            lease_secs: 300,
        };
        let prepared =
            prepare_final_session_with_policy(&memory, &backend, &cfg("user:tg-1"), policy)
                .await
                .unwrap();
        assert_eq!(prepared.session_id.as_deref(), Some("ses_1"));
        assert_eq!(prepared.rotated_from, None);
        assert!(backend.age_queries.lock().unwrap().is_empty());
        assert!(backend.compactions.lock().unwrap().is_empty());
    }

    // issue-0028 SCN-006：查不到建立時間時不輪替，照原本壓縮。
    #[tokio::test]
    async fn an_unknown_age_falls_back_to_compaction() {
        let memory = seeded("user:tg-1", 1).await;
        let backend = AgedSession::created(None);
        let prepared =
            prepare_final_session_with_policy(&memory, &backend, &cfg("user:tg-1"), EVERY_TURN)
                .await
                .unwrap();
        assert_eq!(prepared.session_id.as_deref(), Some("ses_1"));
        assert_eq!(prepared.rotated_from, None);
        assert_eq!(backend.age_queries.lock().unwrap().clone(), ["ses_1"]);
        assert_eq!(backend.compactions.lock().unwrap().clone(), ["ses_1"]);
    }

    // issue-0028 SCN-007（資料層）：決定輪替不會動到 scope 的指向；回合在取得新
    // session 前失敗、放掉 lease 之後，下一回合還是指向舊的、也還會再輪替一次。
    #[tokio::test]
    async fn deciding_to_rotate_leaves_the_pointer_for_the_next_turn() {
        let memory = seeded("user:tg-1", 1).await;
        let backend = AgedSession::created(Some(now_ms() - 8 * DAY_MS));
        let first =
            prepare_final_session_with_policy(&memory, &backend, &cfg("user:tg-1"), EVERY_TURN)
                .await
                .unwrap();
        assert_eq!(first.rotated_from.as_deref(), Some("ses_1"));
        assert!(memory
            .release_agent_session_lease("user:tg-1", &first.owner)
            .await
            .unwrap());

        let state = memory
            .agent_session_state("user:tg-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.session_id.as_deref(), Some("ses_1"));
        let second =
            prepare_final_session_with_policy(&memory, &backend, &cfg("user:tg-1"), EVERY_TURN)
                .await
                .unwrap();
        assert_eq!(second.rotated_from.as_deref(), Some("ses_1"));
    }
}
