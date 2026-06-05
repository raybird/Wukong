use thiserror::Error;

/// All errors produced by the orchestrator.
#[derive(Debug, Error)]
pub enum OrchestratorError {
    #[error("backend error: {0}")]
    Backend(#[from] wukong_gateway::GatewayError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_error_displays() {
        let inner = wukong_gateway::GatewayError::AgentFailed {
            code: Some(1),
            stderr: "nope".to_string(),
        };
        let err = OrchestratorError::Backend(inner);
        assert!(err.to_string().contains("backend error"));
        assert!(err.to_string().contains("nope"));
    }
}
