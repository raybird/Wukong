use wukong_gateway::backend::AgentBackend;
use wukong_memory::Memory;
use wukong_runtime::maintenance::{memory_auto_maintenance, AutoMaintenanceReport};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AutoMaintenanceConfig {
    pub enabled: bool,
    pub interval_secs: u64,
    pub threshold: i64,
}

impl AutoMaintenanceConfig {
    pub fn from_env() -> Self {
        Self::from_values(
            std::env::var("WUKONG_MEMORY_AUTO_MAINTENANCE")
                .ok()
                .as_deref(),
            std::env::var("WUKONG_MEMORY_MAINTENANCE_INTERVAL_SECS")
                .ok()
                .as_deref(),
            std::env::var("WUKONG_MEMORY_CONSOLIDATE_THRESHOLD")
                .ok()
                .as_deref(),
        )
    }

    pub fn from_values(
        enabled: Option<&str>,
        interval_secs: Option<&str>,
        threshold: Option<&str>,
    ) -> Self {
        Self {
            enabled: enabled != Some("0"),
            interval_secs: interval_secs
                .and_then(|value| value.parse::<u64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(900),
            threshold: threshold
                .and_then(|value| value.parse::<i64>().ok())
                .filter(|value| *value > 0)
                .unwrap_or(40),
        }
    }
}

pub async fn run_once(
    memory: &Memory,
    backend: &AgentBackend,
    policy: AutoMaintenanceConfig,
) -> Result<AutoMaintenanceReport, String> {
    if !policy.enabled {
        return Ok(AutoMaintenanceReport::default());
    }
    memory_auto_maintenance(memory, backend, policy.threshold)
        .await
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_enable_fifteen_minute_maintenance() {
        let cfg = AutoMaintenanceConfig::from_values(None, None, None);
        assert!(cfg.enabled);
        assert_eq!(cfg.interval_secs, 900);
        assert_eq!(cfg.threshold, 40);
    }

    #[test]
    fn zero_disables_maintenance_and_invalid_values_use_defaults() {
        let cfg = AutoMaintenanceConfig::from_values(Some("0"), Some("bad"), Some("0"));
        assert!(!cfg.enabled);
        assert_eq!(cfg.interval_secs, 900);
        assert_eq!(cfg.threshold, 40);
    }
}
