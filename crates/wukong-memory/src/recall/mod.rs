use crate::model::RecallMode;
use crate::scope::Scope;
use crate::scoring::{combined_score, Weights};
use crate::store::Candidate;

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "of", "to", "and", "it", "in", "on", "for",
];

/// Trivial queries (too short or only stopwords) skip recall entirely.
pub fn is_trivial(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.chars().count() < 3 {
        return true;
    }
    let tokens = tokenize(trimmed);
    tokens.is_empty() || tokens.iter().all(|t| STOPWORDS.contains(&t.as_str()))
}

/// Lowercase alphanumeric tokens.
pub fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_lowercase())
        .collect()
}

/// Build an FTS5 MATCH expression: each token quoted, OR-joined.
/// Returns None when there are no usable tokens.
pub fn fts_match_string(query: &str) -> Option<String> {
    let tokens = tokenize(query);
    if tokens.is_empty() {
        return None;
    }
    Some(
        tokens
            .iter()
            .map(|t| format!("\"{t}\""))
            .collect::<Vec<_>>()
            .join(" OR "),
    )
}

/// Merge keyword + recency candidates by id, preferring the keyword row's bm25.
pub fn merge_candidates(keyword: Vec<Candidate>, recent: Vec<Candidate>) -> Vec<Candidate> {
    let mut out: Vec<Candidate> = keyword;
    for c in recent {
        if !out.iter().any(|k| k.id == c.id) {
            out.push(c);
        }
    }
    out
}

/// Keep only candidates whose scope is within the filter's ancestry.
pub fn filter_by_scope(candidates: Vec<Candidate>, filter: &Option<Scope>) -> Vec<Candidate> {
    match filter {
        None => candidates,
        Some(scope) => {
            let allowed: Vec<String> =
                scope.ancestry().iter().map(|s| s.to_string()).collect();
            candidates
                .into_iter()
                .filter(|c| allowed.contains(&c.scope))
                .collect()
        }
    }
}

/// A scored hit (id, scope, kind, text, score), produced by `rank`.
#[derive(Debug, Clone)]
pub struct Scored {
    pub id: i64,
    pub scope: String,
    pub kind: crate::model::MemoryKind,
    pub text: String,
    pub score: f64,
}

/// Normalize bm25 across candidates (lower bm25 = better => higher norm),
/// compute combined scores, sort descending, and take top_k.
pub fn rank(
    candidates: Vec<Candidate>,
    now: i64,
    top_k: usize,
    weights: &Weights,
) -> Vec<Scored> {
    // Collect bm25 values (more negative = better match).
    let bm25_vals: Vec<f64> = candidates.iter().filter_map(|c| c.bm25).collect();
    let (min, max) = match (
        bm25_vals.iter().cloned().fold(f64::INFINITY, f64::min),
        bm25_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    ) {
        (mn, mx) if mn.is_finite() && mx.is_finite() => (mn, mx),
        _ => (0.0, 0.0),
    };

    let mut scored: Vec<Scored> = candidates
        .into_iter()
        .map(|c| {
            // relevance: invert bm25 (lower is better) then min-max to [0,1].
            let lexical_norm = match c.bm25 {
                None => 0.0,
                Some(_) if (max - min).abs() < 1e-9 => 1.0,
                Some(b) => (max - b) / (max - min),
            };
            let age = (now - c.created_at).max(0);
            let score =
                combined_score(lexical_norm, age, c.importance, c.recall_count, weights);
            Scored {
                id: c.id,
                scope: c.scope,
                kind: c.kind,
                text: c.text,
                score,
            }
        })
        .collect();

    scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
    scored.truncate(top_k);
    scored
}

/// Decide which candidate sources to combine for the given mode.
pub fn sources_for_mode(mode: RecallMode) -> (bool, bool) {
    // returns (use_keyword, use_recent)
    match mode {
        RecallMode::Keyword => (true, false),
        RecallMode::Tree => (false, true),
        RecallMode::Hybrid => (true, true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemoryKind;

    fn cand(id: i64, scope: &str, created_at: i64, bm25: Option<f64>) -> Candidate {
        Candidate {
            id,
            scope: scope.to_string(),
            kind: MemoryKind::Note,
            text: format!("memory {id}"),
            created_at,
            recall_count: 0,
            importance: 1.0,
            bm25,
        }
    }

    #[test]
    fn trivial_queries_detected() {
        assert!(is_trivial("of"));
        assert!(is_trivial("a the"));
        assert!(is_trivial("  "));
        assert!(!is_trivial("sqlite migration"));
    }

    #[test]
    fn fts_match_quotes_and_or_joins() {
        assert_eq!(
            fts_match_string("SQLite, migration!").unwrap(),
            "\"sqlite\" OR \"migration\""
        );
        assert!(fts_match_string("  ").is_none());
    }

    #[test]
    fn merge_prefers_keyword_rows() {
        let kw = vec![cand(1, "global", 100, Some(-2.0))];
        let recent = vec![cand(1, "global", 100, None), cand(2, "global", 100, None)];
        let merged = merge_candidates(kw, recent);
        assert_eq!(merged.len(), 2);
        let one = merged.iter().find(|c| c.id == 1).unwrap();
        assert!(one.bm25.is_some()); // kept keyword row
    }

    #[test]
    fn scope_filter_includes_ancestry() {
        let cands = vec![
            cand(1, "agent:main", 100, None),
            cand(2, "global", 100, None),
            cand(3, "project:X", 100, None),
        ];
        let filtered =
            filter_by_scope(cands, &Some(Scope::Agent("main".to_string())));
        let ids: Vec<i64> = filtered.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2]); // agent:main + global, not project:X
    }

    #[test]
    fn rank_orders_by_score_and_truncates() {
        let cands = vec![
            cand(1, "global", 0, Some(-1.0)),   // weaker match (less negative bm25)
            cand(2, "global", 0, Some(-5.0)),   // stronger match (more negative bm25)
        ];
        let ranked = rank(cands, 0, 1, &Weights::default());
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].id, 2); // more-negative bm25 = better match => higher lexical_norm
    }
}
