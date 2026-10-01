//! opencode session 的保留期清理：刪除超過保留期、且沒有任何 scope 指向的 session。
//!
//! Wukong 自己另存對話，不讀取舊的 opencode session；只有 scope 目前指向的那一個
//! 會被續接。背景與量測見 `docs/issues/issue-0003/`。

use crate::WukongError;
use std::collections::{HashMap, HashSet};
use wukong_gateway::backend::{AiBackend, SessionSummary};
use wukong_memory::Memory;

pub const DEFAULT_RETENTION_DAYS: u32 = 30;
const RETENTION_DAYS_ENV: &str = "WUKONG_OPENCODE_SESSION_RETENTION_DAYS";
/// 每輪最多刪除數。實測連續刪除 484 個 session 共 4.43 秒，這個上限只是不讓首次
/// 套用時的一輪拖得太久，剩下的留給下一輪。
pub const MAX_DELETES_PER_RUN: usize = 500;
const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// 一份 session 清單經過挑選後的結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetentionSelection {
    /// 要刪除的 session，由舊到新，已套用每輪上限。
    pub expired: Vec<String>,
    /// 因為被 scope 指向（它自己，或它底下的任何子 session）而不論多舊都保留的
    /// 根 session。
    pub protected: Vec<String>,
    /// `protected` 之中已超過保留期的數量：被棄置 scope 的規模訊號。
    pub stale_protected: usize,
    /// 已過期但超出本輪上限、留待下一輪的數量。
    pub deferred: usize,
}

/// 挑出該刪的 session。`retention_days` 為 0 表示停用，不挑任何 session。
pub fn select_expired(
    sessions: &[SessionSummary],
    protected: &HashSet<String>,
    now_ms: i64,
    retention_days: u32,
    cap: usize,
) -> RetentionSelection {
    let mut selection = RetentionSelection::default();
    if retention_days == 0 {
        return selection;
    }
    let cutoff_ms = now_ms - i64::from(retention_days) * DAY_MS;

    // 刪除根 session 會連整棵樹一起帶走，所以判定單位是樹：只要樹裡有任何一個被
    // scope 指向就整棵保留；樹的「最後活動」取所有成員中最新的。
    let parent_of: HashMap<&str, &str> = sessions
        .iter()
        .filter_map(|session| Some((session.id.as_str(), session.parent_id.as_deref()?)))
        .collect();
    let root_of = |id: &str| {
        let mut current = id;
        // 步數上限只是防呆：父子關係成環時不要卡死。
        for _ in 0..sessions.len() {
            match parent_of.get(current) {
                Some(parent) => current = parent,
                None => break,
            }
        }
        current.to_string()
    };
    let mut last_activity: HashMap<String, i64> = HashMap::new();
    let mut protected_roots: HashSet<String> = HashSet::new();
    for session in sessions {
        let root = root_of(&session.id);
        if protected.contains(&session.id) {
            protected_roots.insert(root.clone());
        }
        let activity = last_activity.entry(root).or_insert(session.updated_ms);
        *activity = (*activity).max(session.updated_ms);
    }

    let mut expired: Vec<(i64, &str)> = Vec::new();
    for root in sessions
        .iter()
        .filter(|session| session.parent_id.is_none())
    {
        let activity = last_activity[&root.id];
        let is_stale = activity < cutoff_ms;
        if protected_roots.contains(&root.id) {
            selection.protected.push(root.id.clone());
            selection.stale_protected += usize::from(is_stale);
        } else if is_stale {
            expired.push((activity, &root.id));
        }
    }
    expired.sort();
    selection.deferred = expired.len().saturating_sub(cap);
    selection.expired = expired
        .into_iter()
        .take(cap)
        .map(|(_, id)| id.to_string())
        .collect();
    selection
}

/// 保留期設定。天數為 0 表示停用清理。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetentionPolicy {
    pub retention_days: u32,
}

impl RetentionPolicy {
    pub fn from_env() -> Self {
        let raw = std::env::var(RETENTION_DAYS_ENV).ok();
        let policy = Self::from_value(raw.as_deref());
        if let Some(raw) = raw.filter(|raw| raw.trim().parse::<u32>().is_err()) {
            eprintln!(
                "warning: {RETENTION_DAYS_ENV}={raw:?} 不是非負整數，opencode session 保留期清理停用"
            );
        }
        policy
    }

    /// 未設定時用預設天數。寫了卻無法解析的值視為停用：`off`、`false`、`-1` 最可能
    /// 的意思是「不要清」，把它當成 30 天照樣刪，方向正好相反。
    pub fn from_value(days: Option<&str>) -> Self {
        Self {
            retention_days: match days {
                None => DEFAULT_RETENTION_DAYS,
                Some(value) => value.trim().parse::<u32>().unwrap_or(0),
            },
        }
    }

    pub fn enabled(&self) -> bool {
        self.retention_days > 0
    }
}

/// 一輪清理（或預覽）的結果。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetentionReport {
    pub dry_run: bool,
    /// backend 列出的 session 數。
    pub listed: usize,
    /// 列表被截斷：最舊的 session 可能沒被看到。
    pub truncated: bool,
    /// 這份記憶庫指向的 session 至少有一個出現在 server 的清單裡。為 `false` 時
    /// 代表記憶庫與 server 對不上，整輪不挑也不刪。
    pub anchored: bool,
    pub selection: RetentionSelection,
    pub deleted: Vec<String>,
    /// 刪除失敗的 session 與原因；下一輪會再遇到它們。
    pub failed: Vec<(String, String)>,
}

impl RetentionReport {
    /// 給日誌用的一行摘要。
    pub fn summary_line(&self) -> String {
        format!(
            "opencode_session_retention dry_run={} listed={} truncated={} anchored={} protected={} stale_protected={} expired={} deferred={} deleted={} failed={}",
            self.dry_run,
            self.listed,
            self.truncated,
            self.anchored,
            self.selection.protected.len(),
            self.selection.stale_protected,
            self.selection.expired.len(),
            self.selection.deferred,
            self.deleted.len(),
            self.failed.len(),
        )
    }
}

/// 給人看的結果：逐一列出被刪（預覽時為將被刪）與受保護的 session。
pub fn render_report(report: &RetentionReport, policy: RetentionPolicy) -> String {
    if !policy.enabled() {
        return format!(
            "opencode session 保留期清理已停用（{RETENTION_DAYS_ENV} 為 0 或無法解析）"
        );
    }
    if !report.anchored {
        return format!(
            "未清理：這份記憶庫指向的 session 沒有任何一個出現在 opencode server 的清單裡（共列出 {} 個）。\n\
             記憶庫與 server 對不上時，server 上真正還有人接著的 session 會被誤判為無主，所以整輪不刪。\n\
             請確認 WUKONG_MEMORY_DB 與 WUKONG_AGENT_SERVER_URL 指向同一套部署；全新的記憶庫要先跑過一個回合。",
            report.listed
        );
    }
    let days = policy.retention_days;
    let mut lines = Vec::new();
    if report.dry_run {
        lines.push(format!(
            "[dry-run] 將刪除 {} 個 opencode session（保留期 {days} 天）",
            report.selection.expired.len()
        ));
        lines.extend(report.selection.expired.iter().map(|id| format!("  {id}")));
    } else {
        lines.push(format!(
            "已刪除 {} 個 opencode session（保留期 {days} 天）",
            report.deleted.len()
        ));
        lines.extend(report.deleted.iter().map(|id| format!("  {id}")));
        if !report.failed.is_empty() {
            lines.push(format!("刪除失敗 {} 個，下一輪會再試", report.failed.len()));
            lines.extend(
                report
                    .failed
                    .iter()
                    .map(|(id, reason)| format!("  {id}: {reason}")),
            );
        }
    }
    if report.selection.deferred > 0 {
        lines.push(format!(
            "另有 {} 個已過期，超出單輪上限 {MAX_DELETES_PER_RUN}，留待下一輪",
            report.selection.deferred
        ));
    }
    lines.push(format!(
        "受保護（仍被 scope 指向）{} 個，其中 {} 個已超過保留期",
        report.selection.protected.len(),
        report.selection.stale_protected
    ));
    lines.extend(
        report
            .selection
            .protected
            .iter()
            .map(|id| format!("  {id}")),
    );
    lines.push(format!("共列出 {} 個 session", report.listed));
    if report.truncated {
        lines.push("列表已達上限而被截斷：更舊的 session 可能沒被看到".to_string());
    }
    lines.join("\n")
}

/// 執行一輪清理。`dry_run` 時只挑選、不刪除。
///
/// 受保護集合或 session 清單任一取不到就回傳錯誤、什麼都不刪：寧可漏一輪，也不要
/// 在不知道哪些 session 還有人接的情況下動手。同理，記憶庫指向的 session 沒有任何
/// 一個在 server 上時（記憶庫接錯、或是空的），也整輪不刪。
pub async fn prune_opencode_sessions<B: AiBackend>(
    memory: &Memory,
    backend: &B,
    policy: RetentionPolicy,
    now_ms: i64,
    dry_run: bool,
) -> Result<RetentionReport, WukongError> {
    let mut report = RetentionReport {
        dry_run,
        ..RetentionReport::default()
    };
    if !policy.enabled() {
        return Ok(report);
    }
    let protected: HashSet<String> = memory.referenced_session_ids().await?.into_iter().collect();
    let listing = backend.list_sessions().await?;
    report.listed = listing.sessions.len();
    report.truncated = listing.truncated;
    report.anchored = listing
        .sessions
        .iter()
        .any(|session| protected.contains(&session.id));
    if !report.anchored {
        return Ok(report);
    }
    report.selection = select_expired(
        &listing.sessions,
        &protected,
        now_ms,
        policy.retention_days,
        MAX_DELETES_PER_RUN,
    );
    if dry_run {
        return Ok(report);
    }
    for id in &report.selection.expired {
        match backend.delete_session(id).await {
            Ok(()) => report.deleted.push(id.clone()),
            Err(error) => report.failed.push((id.clone(), error.to_string())),
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse, SessionListing};
    use wukong_gateway::GatewayError;

    const NOW_MS: i64 = 1_800_000_000_000;

    /// 回傳固定的 session 清單，並記錄被要求刪除的 id。
    struct ListingBackend {
        listing: Result<SessionListing, String>,
        undeletable: Vec<&'static str>,
        delete_calls: Mutex<Vec<String>>,
    }

    impl ListingBackend {
        fn with(sessions: Vec<SessionSummary>) -> Self {
            Self {
                listing: Ok(SessionListing {
                    sessions,
                    truncated: false,
                }),
                undeletable: Vec::new(),
                delete_calls: Mutex::new(Vec::new()),
            }
        }

        fn delete_calls(&self) -> Vec<String> {
            self.delete_calls.lock().unwrap().clone()
        }
    }

    impl AiBackend for ListingBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            unreachable!("retention never runs a turn")
        }

        async fn list_sessions(&self) -> Result<SessionListing, GatewayError> {
            self.listing
                .clone()
                .map_err(|stderr| GatewayError::AgentFailed { code: None, stderr })
        }

        async fn delete_session(&self, session_id: &str) -> Result<(), GatewayError> {
            self.delete_calls
                .lock()
                .unwrap()
                .push(session_id.to_string());
            if self.undeletable.contains(&session_id) {
                return Err(GatewayError::AgentFailed {
                    code: Some(500),
                    stderr: "boom".to_string(),
                });
            }
            Ok(())
        }
    }

    async fn open_memory() -> (Memory, String) {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        (Memory::open(&url).await.unwrap(), url)
    }

    const POLICY: RetentionPolicy = RetentionPolicy { retention_days: 30 };

    fn fixture() -> Vec<SessionSummary> {
        vec![
            session("fresh_orphan", DAY_MS),
            session("old_orphan_a", 50 * DAY_MS),
            session("old_protected", 60 * DAY_MS),
            session("old_orphan_b", 40 * DAY_MS),
        ]
    }

    #[tokio::test]
    async fn deletes_expired_orphans_and_keeps_what_a_scope_points_at() {
        let (memory, _) = open_memory().await;
        memory
            .set_agent_session("user:tg-1", "old_protected")
            .await
            .unwrap();
        let backend = ListingBackend::with(fixture());

        let report = prune_opencode_sessions(&memory, &backend, POLICY, NOW_MS, false)
            .await
            .unwrap();

        assert_eq!(backend.delete_calls(), ["old_orphan_a", "old_orphan_b"]);
        assert_eq!(report.deleted, ["old_orphan_a", "old_orphan_b"]);
        assert_eq!(report.selection.protected, ["old_protected"]);
        assert_eq!(report.selection.stale_protected, 1);
        assert_eq!(report.listed, 4);
        assert!(report.failed.is_empty());
        assert_eq!(
            memory.agent_session("user:tg-1").await.unwrap().as_deref(),
            Some("old_protected")
        );
    }

    #[tokio::test]
    async fn preview_deletes_nothing_and_names_what_a_real_run_deletes() {
        let (memory, _) = open_memory().await;
        memory
            .set_agent_session("user:tg-1", "old_protected")
            .await
            .unwrap();

        let previewed = ListingBackend::with(fixture());
        let preview = prune_opencode_sessions(&memory, &previewed, POLICY, NOW_MS, true)
            .await
            .unwrap();
        let executed = ListingBackend::with(fixture());
        prune_opencode_sessions(&memory, &executed, POLICY, NOW_MS, false)
            .await
            .unwrap();

        assert_eq!(previewed.delete_calls(), Vec::<String>::new());
        assert!(preview.dry_run);
        assert!(preview.deleted.is_empty());
        assert_eq!(preview.selection.expired, ["old_orphan_a", "old_orphan_b"]);
        assert_eq!(preview.selection.expired, executed.delete_calls());
    }

    #[tokio::test]
    async fn a_memory_that_matches_nothing_on_the_server_deletes_nothing() {
        // 記憶庫接錯（或是空的）時，server 上每個過期 session 看起來都是無主的，包括
        // 真正那份記憶庫還指著的。對不上就當成接錯，寧可不清。
        for pointed_at in [None, Some("ses_from_another_deployment")] {
            let (memory, _) = open_memory().await;
            if let Some(session_id) = pointed_at {
                memory
                    .set_agent_session("user:tg-1", session_id)
                    .await
                    .unwrap();
            }
            for dry_run in [true, false] {
                let backend = ListingBackend::with(fixture());

                let report = prune_opencode_sessions(&memory, &backend, POLICY, NOW_MS, dry_run)
                    .await
                    .unwrap();

                assert_eq!(backend.delete_calls(), Vec::<String>::new());
                assert!(!report.anchored);
                assert!(report.selection.expired.is_empty(), "{report:?}");
                assert_eq!(report.listed, 4);
                assert!(report.summary_line().contains("anchored=false"));
                let text = render_report(&report, POLICY);
                assert!(text.contains("沒有任何一個出現在"), "{text}");
                assert!(!text.contains("old_orphan_a"), "{text}");
            }
        }
    }

    #[tokio::test]
    async fn disabled_retention_deletes_nothing() {
        let (memory, _) = open_memory().await;
        let backend = ListingBackend::with(fixture());

        let report = prune_opencode_sessions(
            &memory,
            &backend,
            RetentionPolicy { retention_days: 0 },
            NOW_MS,
            false,
        )
        .await
        .unwrap();

        assert_eq!(backend.delete_calls(), Vec::<String>::new());
        assert!(report.deleted.is_empty());
    }

    #[tokio::test]
    async fn a_failed_listing_deletes_nothing() {
        let (memory, _) = open_memory().await;
        let backend = ListingBackend {
            listing: Err("opencode server list_sessions returned 500".to_string()),
            ..ListingBackend::with(Vec::new())
        };

        let err = prune_opencode_sessions(&memory, &backend, POLICY, NOW_MS, false)
            .await
            .unwrap_err();

        assert!(
            err.to_string().contains("list_sessions returned 500"),
            "{err}"
        );
        assert_eq!(backend.delete_calls(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn an_unreadable_scope_table_deletes_nothing() {
        // 讀不到哪些 session 還被 scope 指向時，清單上每一個過期 session 看起來都是
        // 無主的——這正是最不能動手的時候。
        let (memory, url) = open_memory().await;
        memory
            .set_agent_session("user:tg-1", "old_protected")
            .await
            .unwrap();
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::query("DROP TABLE agent_session_state")
            .execute(&pool)
            .await
            .unwrap();
        let backend = ListingBackend::with(fixture());

        let result = prune_opencode_sessions(&memory, &backend, POLICY, NOW_MS, false).await;

        assert!(matches!(result, Err(WukongError::Memory(_))), "{result:?}");
        assert_eq!(backend.delete_calls(), Vec::<String>::new());
    }

    #[tokio::test]
    async fn one_failed_delete_does_not_stop_the_rest() {
        let (memory, _) = open_memory().await;
        memory
            .set_agent_session("user:tg-1", "old_protected")
            .await
            .unwrap();
        let backend = ListingBackend {
            undeletable: vec!["old_orphan_a"],
            ..ListingBackend::with(fixture())
        };

        let report = prune_opencode_sessions(&memory, &backend, POLICY, NOW_MS, false)
            .await
            .unwrap();

        assert_eq!(backend.delete_calls(), ["old_orphan_a", "old_orphan_b"]);
        assert_eq!(report.deleted, ["old_orphan_b"]);
        assert_eq!(report.failed.len(), 1);
        assert_eq!(report.failed[0].0, "old_orphan_a");
        assert!(report.failed[0].1.contains("boom"), "{:?}", report.failed);
    }

    #[tokio::test]
    async fn truncation_is_carried_into_the_report_and_its_summary() {
        let (memory, _) = open_memory().await;
        memory
            .set_agent_session("user:tg-1", "old_protected")
            .await
            .unwrap();
        let backend = ListingBackend {
            listing: Ok(SessionListing {
                sessions: fixture(),
                truncated: true,
            }),
            ..ListingBackend::with(Vec::new())
        };

        let report = prune_opencode_sessions(&memory, &backend, POLICY, NOW_MS, false)
            .await
            .unwrap();

        assert!(report.truncated);
        assert!(report.summary_line().contains("truncated=true"));
    }

    #[tokio::test]
    async fn rendered_report_names_every_session_it_touches() {
        let (memory, _) = open_memory().await;
        memory
            .set_agent_session("user:tg-1", "old_protected")
            .await
            .unwrap();

        let preview = prune_opencode_sessions(
            &memory,
            &ListingBackend::with(fixture()),
            POLICY,
            NOW_MS,
            true,
        )
        .await
        .unwrap();
        let text = render_report(&preview, POLICY);
        assert!(text.starts_with("[dry-run] 將刪除 2 個"), "{text}");
        for id in ["old_orphan_a", "old_orphan_b", "old_protected"] {
            assert!(text.contains(id), "{id} missing from:\n{text}");
        }
        assert!(!text.contains("fresh_orphan"), "{text}");

        let failing = ListingBackend {
            undeletable: vec!["old_orphan_a"],
            ..ListingBackend::with(fixture())
        };
        let executed = prune_opencode_sessions(&memory, &failing, POLICY, NOW_MS, false)
            .await
            .unwrap();
        let text = render_report(&executed, POLICY);
        assert!(text.starts_with("已刪除 1 個"), "{text}");
        assert!(text.contains("刪除失敗 1 個"), "{text}");
        assert!(text.contains("old_orphan_a: "), "{text}");
        assert!(text.contains("boom"), "{text}");
    }

    #[test]
    fn rendered_report_says_when_retention_is_disabled() {
        let disabled = RetentionPolicy { retention_days: 0 };
        let text = render_report(&RetentionReport::default(), disabled);
        assert!(text.contains("已停用"), "{text}");
        assert!(
            text.contains("WUKONG_OPENCODE_SESSION_RETENTION_DAYS"),
            "{text}"
        );
    }

    #[test]
    fn policy_defaults_to_thirty_days_and_zero_disables() {
        assert_eq!(RetentionPolicy::from_value(None).retention_days, 30);
        assert_eq!(RetentionPolicy::from_value(Some("7")).retention_days, 7);
        assert_eq!(RetentionPolicy::from_value(Some(" 14 ")).retention_days, 14);
        assert!(!RetentionPolicy::from_value(Some("0")).enabled());
        assert!(RetentionPolicy::from_value(None).enabled());
        // 寫了卻看不懂的值（`off`、`false`、`-1`）最可能的意思是「不要清」，絕不能
        // 被當成 30 天照樣刪：不確定就不刪。
        for invalid in ["", "abc", "off", "false", "-5", "1.5"] {
            assert!(
                !RetentionPolicy::from_value(Some(invalid)).enabled(),
                "{invalid:?}"
            );
        }
    }

    fn session(id: &str, age_ms: i64) -> SessionSummary {
        SessionSummary {
            id: id.to_string(),
            updated_ms: NOW_MS - age_ms,
            parent_id: None,
        }
    }

    fn child(id: &str, parent: &str, age_ms: i64) -> SessionSummary {
        SessionSummary {
            parent_id: Some(parent.to_string()),
            ..session(id, age_ms)
        }
    }

    fn protect(ids: &[&str]) -> HashSet<String> {
        ids.iter().map(|id| id.to_string()).collect()
    }

    #[test]
    fn selects_only_expired_sessions_no_scope_points_at() {
        let sessions = [
            session("fresh_orphan", DAY_MS),
            session("at_cutoff", 30 * DAY_MS),
            session("just_past_cutoff", 30 * DAY_MS + 1),
            session("old_orphan", 40 * DAY_MS),
            session("old_protected", 40 * DAY_MS),
            session("fresh_protected", DAY_MS),
            child("old_child", "fresh_orphan", 40 * DAY_MS),
        ];

        let selection = select_expired(
            &sessions,
            &protect(&["old_protected", "fresh_protected", "not_listed"]),
            NOW_MS,
            30,
            MAX_DELETES_PER_RUN,
        );

        assert_eq!(selection.expired, ["old_orphan", "just_past_cutoff"]);
        assert_eq!(selection.protected, ["old_protected", "fresh_protected"]);
        assert_eq!(selection.stale_protected, 1);
        assert_eq!(selection.deferred, 0);
    }

    #[test]
    fn a_scope_pointing_at_a_child_protects_the_whole_tree() {
        // CLI backend 記下的是串流裡最後一個 session id，那可能是子 session。刪掉它的
        // 根 session 會連它一起帶走，所以根也必須受保護。
        let sessions = [
            session("root", 40 * DAY_MS),
            child("kid", "root", 40 * DAY_MS),
            child("grandkid", "kid", 40 * DAY_MS),
            session("unrelated_root", 40 * DAY_MS),
        ];

        let selection = select_expired(
            &sessions,
            &protect(&["grandkid"]),
            NOW_MS,
            30,
            MAX_DELETES_PER_RUN,
        );

        assert_eq!(selection.expired, ["unrelated_root"]);
        assert_eq!(selection.protected, ["root"]);
    }

    #[test]
    fn a_root_is_only_as_old_as_its_most_recent_descendant() {
        let sessions = [
            session("quiet_root_busy_child", 40 * DAY_MS),
            child("busy_child", "quiet_root_busy_child", DAY_MS),
            session("quiet_root_quiet_child", 50 * DAY_MS),
            child("quiet_child", "quiet_root_quiet_child", 45 * DAY_MS),
        ];

        let selection = select_expired(&sessions, &protect(&[]), NOW_MS, 30, MAX_DELETES_PER_RUN);

        assert_eq!(selection.expired, ["quiet_root_quiet_child"]);
    }

    #[test]
    fn zero_retention_days_selects_nothing() {
        let sessions = [session("ancient_orphan", 400 * DAY_MS)];

        let selection = select_expired(&sessions, &protect(&[]), NOW_MS, 0, MAX_DELETES_PER_RUN);

        assert_eq!(selection.expired, Vec::<String>::new());
    }

    #[test]
    fn a_run_is_capped_and_takes_the_oldest_first() {
        let sessions = [
            session("old", 40 * DAY_MS),
            session("oldest", 90 * DAY_MS),
            session("older", 60 * DAY_MS),
        ];

        let selection = select_expired(&sessions, &protect(&[]), NOW_MS, 30, 2);

        assert_eq!(selection.expired, ["oldest", "older"]);
        assert_eq!(selection.deferred, 1);
    }
}
