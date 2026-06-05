use thiserror::Error;

/// All errors produced by the gateway.
#[derive(Debug, Error)]
pub enum GatewayError {
    #[error("memory error: {0}")]
    Memory(#[from] wukong_memory::MemoryError),
    #[error("agent command failed (code {code:?}): {stderr}")]
    AgentFailed { code: Option<i32>, stderr: String },
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
}
