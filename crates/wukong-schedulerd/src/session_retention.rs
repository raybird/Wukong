use std::time::Duration;
use wukong_gateway::backend::AgentBackend;
use wukong_memory::Memory;
use wukong_runtime::session_retention::{
    prune_opencode_sessions, RetentionPolicy, RetentionReport,
};
use wukong_runtime::util::now_unix;

/// 清理間隔。刪除很快（實測 484 個 session 共 4.43 秒），間隔只需要比保留期的
/// 單位「天」細得多，沒有人要求它可調，所以不做成設定。唯一的例外是下方只存在
/// 於 debug 建置的測試鉤子。
pub const INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// 一輪清理的時間上限。正常情況幾秒內結束；上限是為了 server 掛住的時候。
const RUN_BUDGET: Duration = Duration::from_secs(5 * 60);

/// 這個行程實際使用的間隔：正式建置一律是 [`INTERVAL`]。
pub fn interval() -> Duration {
    #[cfg(debug_assertions)]
    if let Some(shortened) = test_interval(
        std::env::var("WUKONG_TEST_OPENCODE_RETENTION_INTERVAL_SECS")
            .ok()
            .as_deref(),
    ) {
        return shortened;
    }
    INTERVAL
}

/// `tests/retention_daemon.rs` 要看到真正的 daemon 跑完一輪，不能等六小時，所以
/// debug 建置認得一個縮短間隔的環境變數。它不是設定：release 建置根本不編譯這段，
/// 映像檔裡的 binary 不會讀它，文件也不提它。只接受比正式間隔短的正整數——放大
/// 沒有測試用途，而過大的值會讓計時器在啟動時溢位。
#[cfg(debug_assertions)]
fn test_interval(raw: Option<&str>) -> Option<Duration> {
    let secs = raw?.parse::<u64>().ok()?;
    (1..INTERVAL.as_secs())
        .contains(&secs)
        .then(|| Duration::from_secs(secs))
}

/// 第一輪在啟動後隔一個完整間隔才跑，不在啟動當下。升級後有這段時間可以先用
/// `wukong opencode prune --dry-run` 看會刪哪些；也避免每次重啟都立刻刪一輪。
pub fn ticker(every: Duration) -> tokio::time::Interval {
    tokio::time::interval_at(tokio::time::Instant::now() + every, every)
}

/// 這個 schedulerd 該不該跑清理；不該跑時回傳 `None`。
pub fn active_policy(backend: &AgentBackend, policy: RetentionPolicy) -> Option<RetentionPolicy> {
    // CLI 模式的 opencode.db 與使用者自己的 opencode 使用共用，Wukong 分不出哪些
    // session 是自己建的，所以只有 server backend 才自動清掃。
    (matches!(backend, AgentBackend::Server(_)) && policy.enabled()).then_some(policy)
}

/// 跑一輪清理並記錄結果。錯誤交給呼叫端記錄——清理是背景維護，不能讓排程迴圈停下來。
/// `db_url` 只用於日誌：記憶庫與 server 對不上時，要說得出用的是哪一份。
pub async fn run_once(
    memory: &Memory,
    backend: &AgentBackend,
    policy: RetentionPolicy,
    db_url: &str,
) -> Result<RetentionReport, String> {
    run_within(memory, backend, policy, db_url, RUN_BUDGET).await
}

/// 一輪清理要寫進日誌的每一行。
fn log_lines(report: &RetentionReport, db_url: &str) -> Vec<String> {
    let mut lines = vec![report.summary_line()];
    if !report.anchored {
        lines.push(format!(
            "warning: opencode session retention skipped: 記憶庫 {db_url} 指向的 session 沒有任何一個出現在 opencode server 的清單裡（共列出 {} 個），視為記憶庫與 server 對不上，整輪不刪。請確認這個服務的 WUKONG_MEMORY_DB 與 WUKONG_AGENT_SERVER_URL 屬於同一套部署。",
            report.listed
        ));
    }
    lines.extend(report.failed.iter().map(|(session_id, reason)| {
        format!("opencode_session_retention_delete_failed session_id={session_id} error={reason}")
    }));
    lines
}

async fn run_within(
    memory: &Memory,
    backend: &AgentBackend,
    policy: RetentionPolicy,
    db_url: &str,
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
    for line in log_lines(&report, db_url) {
        eprintln!("{line}");
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

    #[test]
    fn a_memory_that_does_not_match_the_server_is_explained_in_the_log() {
        // 定期清理沒有人在看輸出。記憶庫接錯時它每一輪都不刪，如果日誌只有一個
        // `anchored=false`，看的人不會知道原因，也不知道該去檢查哪一份記憶庫。
        let unmatched = RetentionReport {
            listed: 12,
            anchored: false,
            ..RetentionReport::default()
        };

        let lines = log_lines(&unmatched, "sqlite:///data/memory.db");

        assert_eq!(lines[0], unmatched.summary_line());
        let warning = lines[1..].join("\n");
        assert!(warning.starts_with("warning: "), "{lines:?}");
        assert!(warning.contains("sqlite:///data/memory.db"), "{lines:?}");
        assert!(warning.contains("沒有任何一個出現在"), "{lines:?}");
    }

    #[test]
    fn a_normal_run_logs_its_summary_and_each_failed_delete() {
        let report = RetentionReport {
            listed: 3,
            anchored: true,
            failed: vec![("ses_stuck".to_string(), "boom".to_string())],
            ..RetentionReport::default()
        };

        let lines = log_lines(&report, "sqlite:///data/memory.db");

        assert_eq!(
            lines,
            [
                report.summary_line(),
                "opencode_session_retention_delete_failed session_id=ses_stuck error=boom"
                    .to_string(),
            ]
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn the_test_hook_only_ever_shortens_the_interval() {
        assert_eq!(test_interval(Some("1")), Some(Duration::from_secs(1)));
        assert_eq!(
            test_interval(Some("21599")),
            Some(Duration::from_secs(21599))
        );
        // 其餘一律當作沒設定：0 會讓計時器 panic，過大的值會在啟動時溢位。
        for ignored in [
            None,
            Some(""),
            Some("0"),
            Some("-5"),
            Some("abc"),
            Some(" 1 "),
            Some("21600"),
            Some("9223372036854775807"),
            Some("18446744073709551615"),
            Some("99999999999999999999999"),
        ] {
            assert_eq!(test_interval(ignored), None, "{ignored:?}");
        }
    }

    #[tokio::test(start_paused = true)]
    async fn the_first_run_waits_a_full_interval() {
        // 啟動當下就清理的話，升級後根本來不及先預覽。
        let mut ticks = ticker(INTERVAL);

        let early = tokio::time::timeout(INTERVAL - Duration::from_secs(1), ticks.tick()).await;
        assert!(
            early.is_err(),
            "a run fired before the first interval elapsed"
        );
        let due = tokio::time::timeout(Duration::from_secs(2), ticks.tick()).await;
        assert!(due.is_ok(), "no run fired once the interval had elapsed");
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
        let error = run_within(
            &memory,
            &backend,
            THIRTY_DAYS,
            "sqlite://x",
            Duration::from_millis(300),
        )
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

        let error = run_once(&memory, &server_backend(), THIRTY_DAYS, "sqlite://x")
            .await
            .unwrap_err();

        assert!(error.contains("list_sessions"), "{error}");
    }
}
