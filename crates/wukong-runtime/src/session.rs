use crate::turn::WukongError;
use std::sync::atomic::{AtomicU64, Ordering};
use wukong_gateway::backend::AiBackend;
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::GatewayError;
use wukong_memory::Memory;

const DEFAULT_LEASE_SECS: i64 = 15 * 60;
/// session 開始超過這麼久，下一個回合開始時就輪替。7 天是暫定值，等正式主機的量測
/// 定案（issue-0028 TBD-1）。
const ROTATE_AFTER_SECS: i64 = 7 * 24 * 60 * 60;
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
    pub lease_secs: i64,
}

impl Default for SessionPolicy {
    fn default() -> Self {
        Self {
            lease_secs: DEFAULT_LEASE_SECS,
        }
    }
}

impl SessionPolicy {
    /// 2026-11-26（#45）：`WUKONG_SESSION_COMPACT_EVERY_TURNS` 不再讀取；壓縮交給 OpenCode
    /// 自己的自動壓縮。設了這個變數也照常運作。
    pub fn from_env() -> Self {
        let defaults = Self::default();
        Self {
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

    let prepared = |session_id, rotated_from| SessionPreparation {
        owner: owner.clone(),
        lease_secs: policy.lease_secs,
        session_id,
        rotated_from,
    };
    let Some(session_id) = state.session_id else {
        return Ok(prepared(None, None));
    };
    // 2026-11-26（#45）：Wukong 不再依回合數壓縮，壓縮交給 OpenCode 的自動壓縮。壓縮本來也
    // 不會減少 opencode.db 的資料（issue-0028），所以長期續接的 session 仍靠輪替：每個回合
    // 開始時看 session 開始了多久，超過 7 天就換新的，舊的由 `run_turn` 取得新 session 後
    // 刪除。年齡用 Wukong 自己記下的時間，不必每回合問 backend（本機模式每問一次就要多
    // 啟動一個 opencode serve）。
    let started_at = match state.session_started_at {
        Some(started_at) => started_at,
        None => {
            backfill_session_started(memory, backend, cfg, &owner, policy.lease_secs, &session_id)
                .await
        }
    };
    if crate::util::now_unix() - started_at > ROTATE_AFTER_SECS {
        eprintln!(
            "session_rotation_due scope={} session_id={} started_at={started_at}",
            cfg.scope, session_id
        );
        return Ok(prepared(None, Some(session_id)));
    }
    Ok(prepared(Some(session_id), None))
}

/// 升級前就在的 session 沒有記下開始時間：向 backend 查一次建立時間並記下。查不到時
/// （例如純 CLI backend 沒有這個 API）從現在起算，之後也不再查。
async fn backfill_session_started(
    memory: &Memory,
    backend: &impl AiBackend,
    cfg: &GatewayConfig,
    owner: &str,
    lease_secs: i64,
    session_id: &str,
) -> i64 {
    let created = hold_session_lease(
        memory,
        &cfg.scope,
        owner,
        lease_secs,
        backend.session_created_ms(session_id),
    )
    .await;
    let started_at = match created {
        Ok(created_ms) => created_ms / 1000,
        Err(error) => {
            eprintln!(
                "warning: session_age_unknown scope={} session_id={session_id}: {error}; counting from now",
                cfg.scope
            );
            crate::util::now_unix()
        }
    };
    if let Err(error) = memory
        .set_agent_session_started(&cfg.scope, session_id, started_at)
        .await
    {
        eprintln!(
            "warning: session_started_record_failed scope={}: {error}",
            cfg.scope
        );
    }
    started_at
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

    const DAY_SECS: i64 = 24 * 60 * 60;

    fn now_secs() -> i64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64
    }

    /// 回報固定建立時間（毫秒）的 backend，記下查了幾次年齡；壓縮一律算測試失敗。
    struct AgedSession {
        created_ms: Option<i64>,
        delay: Duration,
        age_queries: std::sync::Mutex<Vec<String>>,
    }

    impl AgedSession {
        fn created(created_ms: Option<i64>) -> Self {
            Self {
                created_ms,
                delay: Duration::ZERO,
                age_queries: Default::default(),
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
            tokio::time::sleep(self.delay).await;
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
            panic!("#45 SCN-010：Wukong 不再主動壓縮，卻壓縮了 {session_id}")
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

    /// 讓 scope 指向 `ses_1`，並累積 `turns` 個回合；回傳記憶庫與它的 URL。
    async fn seeded(scope: &str, turns: i64) -> (Memory, String) {
        let file = tempfile::NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        let memory = Memory::open(&url).await.unwrap();
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
        (memory, url)
    }

    /// 升級前就在的 session：資料庫裡沒有開始時間。
    async fn forget_started_at(url: &str) {
        let pool = sqlx::SqlitePool::connect(url).await.unwrap();
        sqlx::query("UPDATE agent_session_state SET session_started_at = NULL")
            .execute(&pool)
            .await
            .unwrap();
        pool.close().await;
    }

    fn policy() -> SessionPolicy {
        SessionPolicy { lease_secs: 300 }
    }

    async fn prepare(memory: &Memory, backend: &AgedSession) -> SessionPreparation {
        prepare_final_session_with_policy(memory, backend, &cfg("user:tg-1"), policy())
            .await
            .unwrap()
    }

    // #45 SCN-010：超過 20 回合也不壓縮，續接同一個 session；年齡用 Wukong 記下的時間，不問 backend。
    #[tokio::test]
    async fn a_scope_past_twenty_turns_is_not_compacted() {
        let (memory, _) = seeded("user:tg-1", 21).await;
        let backend = AgedSession::created(None);

        let prepared = prepare(&memory, &backend).await;

        assert_eq!(prepared.session_id.as_deref(), Some("ses_1"));
        assert_eq!(prepared.rotated_from, None);
        assert!(backend.age_queries.lock().unwrap().is_empty());
    }

    // #45 SCN-010：設了 WUKONG_SESSION_COMPACT_EVERY_TURNS 也照常運作（不讀這個變數）。
    #[tokio::test]
    async fn the_old_compaction_variable_has_no_effect() {
        std::env::set_var("WUKONG_SESSION_COMPACT_EVERY_TURNS", "1");
        let (memory, _) = seeded("user:tg-1", 3).await;
        let backend = AgedSession::created(None);

        let prepared = prepare_final_session(&memory, &backend, &cfg("user:tg-1"))
            .await
            .unwrap();

        assert_eq!(prepared.session_id.as_deref(), Some("ses_1"));
        assert_eq!(prepared.rotated_from, None);
    }

    // #45 SCN-010：開始超過 7 天的 session 在下一個回合開始時輪替，不必等到某個回合數。
    #[tokio::test]
    async fn an_old_session_rotates_at_the_start_of_the_next_turn() {
        let (memory, _) = seeded("user:tg-1", 1).await;
        memory
            .set_agent_session_started("user:tg-1", "ses_1", now_secs() - 8 * DAY_SECS)
            .await
            .unwrap();
        let backend = AgedSession::created(None);

        let prepared = prepare(&memory, &backend).await;

        assert_eq!(prepared.session_id, None);
        assert_eq!(prepared.rotated_from.as_deref(), Some("ses_1"));
        assert!(backend.age_queries.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn a_young_session_continues() {
        let (memory, _) = seeded("user:tg-1", 1).await;
        memory
            .set_agent_session_started("user:tg-1", "ses_1", now_secs() - 6 * DAY_SECS)
            .await
            .unwrap();
        let backend = AgedSession::created(None);

        let prepared = prepare(&memory, &backend).await;

        assert_eq!(prepared.session_id.as_deref(), Some("ses_1"));
        assert_eq!(prepared.rotated_from, None);
    }

    // 升級前就在的 session：第一個回合向 backend 查一次建立時間並記下，之後不再查。
    #[tokio::test]
    async fn a_session_from_before_the_upgrade_is_aged_by_the_backend_once() {
        let (memory, url) = seeded("user:tg-1", 1).await;
        forget_started_at(&url).await;
        let created_ms = (now_secs() - 8 * DAY_SECS) * 1000;
        let backend = AgedSession::created(Some(created_ms));

        let first = prepare(&memory, &backend).await;
        assert_eq!(first.rotated_from.as_deref(), Some("ses_1"));
        memory
            .release_agent_session_lease("user:tg-1", &first.owner)
            .await
            .unwrap();
        let state = memory
            .agent_session_state("user:tg-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(state.session_started_at, Some(created_ms / 1000));

        let second = prepare(&memory, &backend).await;
        assert_eq!(second.rotated_from.as_deref(), Some("ses_1"));
        assert_eq!(backend.age_queries.lock().unwrap().clone(), ["ses_1"]);
    }

    // 查不到建立時間（例如純 CLI backend）時不輪替，從現在起算，之後不再查。
    #[tokio::test]
    async fn an_unknown_age_counts_from_now_and_is_not_queried_again() {
        let (memory, url) = seeded("user:tg-1", 1).await;
        forget_started_at(&url).await;
        let backend = AgedSession::created(None);

        let first = prepare(&memory, &backend).await;
        assert_eq!(first.session_id.as_deref(), Some("ses_1"));
        assert_eq!(first.rotated_from, None);
        memory
            .release_agent_session_lease("user:tg-1", &first.owner)
            .await
            .unwrap();
        let started = memory
            .agent_session_state("user:tg-1")
            .await
            .unwrap()
            .unwrap()
            .session_started_at
            .unwrap();
        assert!((now_secs() - started).abs() < 60, "{started}");

        prepare(&memory, &backend).await;
        assert_eq!(backend.age_queries.lock().unwrap().len(), 1);
    }

    // SCN-003（issue-0013）：向 backend 查年齡比 lease 久時，也持續持有 lease。
    #[tokio::test]
    async fn lease_stays_held_during_a_slow_age_lookup() {
        let (memory, url) = seeded("project:T", 1).await;
        forget_started_at(&url).await;
        let backend = AgedSession {
            delay: Duration::from_millis(3500),
            ..AgedSession::created(Some(now_secs() * 1000))
        };
        let cfg = cfg("project:T");
        let prepare = prepare_final_session_with_policy(
            &memory,
            &backend,
            &cfg,
            SessionPolicy { lease_secs: 2 },
        );
        let intruder = async {
            tokio::time::sleep(Duration::from_millis(3000)).await;
            memory
                .acquire_agent_session_lease("project:T", "other", 2)
                .await
                .unwrap()
        };
        let (prepared, stolen) = tokio::join!(prepare, intruder);

        assert!(stolen.is_none(), "查年齡期間 lease 被拿走：{stolen:?}");
        assert_eq!(prepared.unwrap().session_id.as_deref(), Some("ses_1"));
    }

    // issue-0028 SCN-007（資料層）：決定輪替不會動到 scope 的指向；回合在取得新
    // session 前失敗、放掉 lease 之後，下一回合還是指向舊的、也還會再輪替一次。
    #[tokio::test]
    async fn deciding_to_rotate_leaves_the_pointer_for_the_next_turn() {
        let (memory, _) = seeded("user:tg-1", 1).await;
        memory
            .set_agent_session_started("user:tg-1", "ses_1", now_secs() - 8 * DAY_SECS)
            .await
            .unwrap();
        let backend = AgedSession::created(None);
        let first = prepare(&memory, &backend).await;
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
        let second = prepare(&memory, &backend).await;
        assert_eq!(second.rotated_from.as_deref(), Some("ses_1"));
    }
}
