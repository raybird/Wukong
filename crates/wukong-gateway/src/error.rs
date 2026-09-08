use crate::upstream_error::UpstreamFailure;
use thiserror::Error;

/// All errors produced by the gateway.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("memory error: {0}")]
    Memory(#[from] wukong_memory::MemoryError),
    #[error("agent command failed (code {code:?}): {stderr}")]
    AgentFailed { code: Option<i32>, stderr: String },
    /// 上游 provider 拒絕了這次請求（模型下架、限流……），或回合被中止。
    ///
    /// 與 [`GatewayError::AgentFailed`] 分開是為了讓告警說得出原因：agent 本身跑得
    /// 好好的、exit code 是 0、HTTP 也是 200，錯的是它背後的模型。
    #[error("上游模型錯誤（{kind}）：{detail}")]
    UpstreamFailed {
        kind: UpstreamFailure,
        status_code: Option<u16>,
        detail: String,
    },
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn agent_failed_message_includes_stderr() {
        let err = GatewayError::AgentFailed {
            code: Some(2),
            stderr: "boom".to_string(),
        };
        assert!(err.to_string().contains("boom"));
        assert!(err.to_string().contains("2"));
    }

    #[test]
    fn upstream_failed_message_names_the_classification() {
        let err = GatewayError::UpstreamFailed {
            kind: UpstreamFailure::ModelEol,
            status_code: Some(410),
            detail: "statusCode=410".to_string(),
        };
        let rendered = err.to_string();
        assert!(rendered.contains(UpstreamFailure::ModelEol.label()));
        assert!(rendered.contains("statusCode=410"));
        // 不得跟一般 agent 失敗混為一談。
        assert!(!rendered.contains("agent command failed"));
    }
}
