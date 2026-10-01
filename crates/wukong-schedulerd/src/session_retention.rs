use std::time::Duration;
use wukong_gateway::backend::AgentBackend;
use wukong_memory::Memory;
use wukong_runtime::session_retention::{
    prune_opencode_sessions, RetentionPolicy, RetentionReport,
};
use wukong_runtime::util::now_unix;

/// 清理間隔。刪除很快（實測 484 個 session 共 4.43 秒），間隔只需要比保留期的
/// 單位「天」細得多，沒有人要求它可調，所以不做成環境變數。
pub const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// 一輪清理的時間上限。正常情況幾秒內結束；上限是為了 server 掛住的時候。
const RUN_BUDGET: Duration = Duration::from_secs(5 * 60);

/// 第一輪在啟動後隔一個完整間隔才跑，不在啟動當下。升級後有這段時間可以先用
/// `wukong opencode prune --dry-run` 看會刪哪些；也避免每次重啟都立刻刪一輪。
pub fn ticker() -> tokio::time::Interval {
    tokio::time::interval_at(tokio::time::Instant::now() + INTERVAL, INTERVAL)
}

/// 這個 schedulerd 該不該跑清理；不該跑時回傳 `None`。
pub fn active_policy(backend: &AgentBackend, policy: RetentionPolicy) -> Option<RetentionPolicy> {
    // CLI 模式的 opencode.db 與使用者自己的 opencode 使用共用，Wukong 分不出哪些
    // session 是自己建的，所以只有 server backend 才自動清掃。
    (matches!(backend, AgentBackend::Server(_)) && policy.enabled()).then_some(policy)
}

/// 跑一輪清理並記錄結果。錯誤交給呼叫端記錄——清理是背景維護，不能讓排程迴圈停下來。
pub async fn run_once(
    memory: &Memory,
    backend: &AgentBackend,
    policy: RetentionPolicy,
) -> Result<RetentionReport, String> {
    run_within(memory, backend, policy, RUN_BUDGET).await
}

async fn run_within(
    memory: &Memory,
    backend: &AgentBackend,
    policy: RetentionPolicy,
    budget: Duration,
) -> Result<RetentionReport, String> {
    // 列表與每一個刪除都沿用 agent 的逾時（預設 20 分鐘），而這裡是在排程迴圈裡
    // 同步等待的。server 掛住時放著不管，排程掃描會跟著停擺，所以整輪設上限；被
    // 中斷的那一輪什麼都不必收拾，下一輪從頭再來。
    let run = prune_opencode_sessions(memory, backend, policy, now_unix() * 1000, false);
    let report = tokio::time::timeout(budget, run)
        .await
        .map_err(|_| {
            format!(
                "timed out after {}s waiting for the opencode server",
                budget.as_secs()
            )
        })?
        .map_err(|error| error.to_string())?;
    eprintln!("{}", report.summary_line());
    for (session_id, reason) in &report.failed {
        eprintln!(
            "opencode_session_retention_delete_failed session_id={session_id} error={reason}"
        );
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wukong_gateway::backend::AgentCliBackend;
    use wukong_gateway::opencode_server::OpencodeServerBackend;

    const THIRTY_DAYS: RetentionPolicy = RetentionPolicy { retention_days: 30 };
    const DISABLED: RetentionPolicy = RetentionPolicy { retention_days: 0 };

    fn cli_backend() -> AgentBackend {
        AgentBackend::Cli(AgentCliBackend {
            command: vec!["command-that-must-not-run".to_string()],
            workspace: None,
        })
    }

    fn server_backend() -> AgentBackend {
        // 沒有人在聽的位址：任何請求都會失敗。
        AgentBackend::Server(OpencodeServerBackend::from_env(
            "http://127.0.0.1:9".to_string(),
            None,
        ))
    }

    #[test]
    fn only_a_server_backend_with_retention_enabled_is_active() {
        assert_eq!(
            active_policy(&server_backend(), THIRTY_DAYS),
            Some(THIRTY_DAYS)
        );
        assert_eq!(active_policy(&server_backend(), DISABLED), None);
        // CLI 模式的 opencode.db 與使用者自己的 opencode 使用共用，不自動清掃。
        assert_eq!(active_policy(&cli_backend(), THIRTY_DAYS), None);
    }

    #[tokio::test]
    async fn a_server_that_never_answers_gives_the_loop_back() {
        // 列表與刪除沿用 agent 的 20 分鐘逾時，而清理是在排程迴圈裡同步跑的：server
        // 掛住時不設上限，整個排程會跟著停擺。
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        std::thread::spawn(move || {
            let held: Vec<_> = listener.incoming().collect();
            drop(held);
        });
        let file = tempfile::NamedTempFile::new().unwrap();
        let memory = Memory::open(&format!("sqlite://{}", file.path().display()))
            .await
            .unwrap();
        let backend = AgentBackend::Server(OpencodeServerBackend::from_env(url, None));

        let started = std::time::Instant::now();
        let error = run_within(&memory, &backend, THIRTY_DAYS, Duration::from_millis(300))
            .await
            .unwrap_err();

        assert!(error.contains("timed out"), "{error}");
        assert!(started.elapsed() < Duration::from_secs(5));
    }

    #[tokio::test]
    async fn an_unreachable_server_is_an_error_the_loop_can_log() {
        let file = tempfile::NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        let memory = Memory::open(&url).await.unwrap();

        let error = run_once(&memory, &server_backend(), THIRTY_DAYS)
            .await
            .unwrap_err();

        assert!(error.contains("list_sessions"), "{error}");
    }
}
