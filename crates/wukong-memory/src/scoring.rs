/// Relative weights for the combined recall score. The four weights are
/// expected to sum to ~1.0 so the base score stays within [0, 1].
#[derive(Debug, Clone, Copy)]
pub struct Weights {
    pub lexical: f64,
    pub semantic: f64,
    pub decay: f64,
    pub importance: f64,
}

impl Default for Weights {
    fn default() -> Self {
        Self {
            lexical: 0.4,
            semantic: 0.2,
            decay: 0.25,
            importance: 0.15,
        }
    }
}

const HALF_LIFE_DAYS: f64 = 90.0;

/// Exponential time decay with a 90-day half-life. Returns 1.0 at age 0,
/// 0.5 at 90 days. Negative ages are clamped to 0.
pub fn time_decay(age_seconds: i64, half_life_days: f64) -> f64 {
    let age_days = age_seconds.max(0) as f64 / 86_400.0;
    0.5_f64.powf(age_days / half_life_days)
}

/// Combined recall score. `lexical_norm`, `semantic_norm`, and `importance` are
/// expected to be in [0, 1]. Frequently recalled memories get a small
/// logarithmic bonus.
pub fn combined_score(
    lexical_norm: f64,
    semantic_norm: f64,
    age_seconds: i64,
    importance: f64,
    recall_count: i64,
    w: &Weights,
) -> f64 {
    let decay = time_decay(age_seconds, HALF_LIFE_DAYS);
    let base = w.lexical * lexical_norm
        + w.semantic * semantic_norm
        + w.decay * decay
        + w.importance * importance;
    base + 0.02 * (1.0 + recall_count.max(0) as f64).ln()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decay_is_one_at_zero_age() {
        assert!((time_decay(0, HALF_LIFE_DAYS) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn decay_is_half_at_half_life() {
        let ninety_days = 90 * 86_400;
        assert!((time_decay(ninety_days, HALF_LIFE_DAYS) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn negative_age_clamped() {
        assert!((time_decay(-100, HALF_LIFE_DAYS) - 1.0).abs() < 1e-9);
    }

    #[test]
    fn newer_memory_outranks_older_when_all_else_equal() {
        let w = Weights::default();
        let newer = combined_score(0.5, 0.0, 0, 1.0, 0, &w);
        let older = combined_score(0.5, 0.0, 200 * 86_400, 1.0, 0, &w);
        assert!(newer > older);
    }

    #[test]
    fn higher_lexical_match_outranks_lower() {
        let w = Weights::default();
        let strong = combined_score(1.0, 0.0, 0, 1.0, 0, &w);
        let weak = combined_score(0.1, 0.0, 0, 1.0, 0, &w);
        assert!(strong > weak);
    }

    #[test]
    fn recall_count_provides_small_bonus() {
        let w = Weights::default();
        let hot = combined_score(0.5, 0.0, 0, 1.0, 10, &w);
        let cold = combined_score(0.5, 0.0, 0, 1.0, 0, &w);
        assert!(hot > cold);
    }

    #[test]
    fn higher_semantic_match_outranks_lower() {
        let w = Weights::default();
        let strong = combined_score(0.0, 1.0, 0, 1.0, 0, &w);
        let weak = combined_score(0.0, 0.1, 0, 1.0, 0, &w);
        assert!(strong > weak);
    }
}
