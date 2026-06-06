//! Prune policy: which memories are safe to delete.

/// Thresholds for the low-value fallback path. The "already consolidated" path
/// needs no thresholds.
#[derive(Debug, Clone)]
pub struct PrunePolicy {
    /// Rows older than this many seconds may be pruned (fallback path).
    pub max_age_secs: i64,
    /// Rows with importance strictly below this may be pruned (fallback path).
    pub importance_floor: f64,
}

impl Default for PrunePolicy {
    fn default() -> Self {
        Self { max_age_secs: 30 * 86_400, importance_floor: 0.5 }
    }
}
