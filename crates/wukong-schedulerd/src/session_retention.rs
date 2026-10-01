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
    let report = prune_opencode_sessions(memory, backend, policy, now_unix() * 1000, false)
        .await
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
