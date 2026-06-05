use crate::embed::cosine_similarity;
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

/// Normalize bm25 and vector_sim across candidates, compute combined scores,
/// sort descending, and take top_k. bm25: lower = better. vector_sim: higher =
/// better. Either signal absent on a candidate contributes 0 for that term.
pub fn rank(
    candidates: Vec<Candidate>,
    now: i64,
    top_k: usize,
    weights: &Weights,
) -> Vec<Scored> {
    // bm25: more negative = better match.
    let bm25_vals: Vec<f64> = candidates.iter().filter_map(|c| c.bm25).collect();
    let (bmin, bmax) = match (
        bm25_vals.iter().cloned().fold(f64::INFINITY, f64::min),
        bm25_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    ) {
        (mn, mx) if mn.is_finite() && mx.is_finite() => (mn, mx),
        _ => (0.0, 0.0),
    };

    // vector_sim: higher = better match.
    let vec_vals: Vec<f64> = candidates.iter().filter_map(|c| c.vector_sim).collect();
    let (vmin, vmax) = match (
        vec_vals.iter().cloned().fold(f64::INFINITY, f64::min),
        vec_vals.iter().cloned().fold(f64::NEG_INFINITY, f64::max),
    ) {
        (mn, mx) if mn.is_finite() && mx.is_finite() => (mn, mx),
        _ => (0.0, 0.0),
    };

    let mut scored: Vec<Scored> = candidates
        .into_iter()
        .map(|c| {
            // lexical: invert bm25 (lower better) then min-max to [0,1].
            let lexical_norm = match c.bm25 {
                None => 0.0,
                Some(_) if (bmax - bmin).abs() < 1e-9 => 1.0,
                Some(b) => (bmax - b) / (bmax - bmin),
            };
            // semantic: min-max vector_sim (higher better) to [0,1].
            let semantic_norm = match c.vector_sim {
                None => 0.0,
                Some(_) if (vmax - vmin).abs() < 1e-9 => 1.0,
                Some(s) => (s - vmin) / (vmax - vmin),
            };
            let age = (now - c.created_at).max(0);
            let score = combined_score(
                lexical_norm,
                semantic_norm,
                age,
                c.importance,
                c.recall_count,
                weights,
            );
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
/// Returns (use_keyword, use_recent, use_vector).
pub fn sources_for_mode(mode: RecallMode) -> (bool, bool, bool) {
    match mode {
        RecallMode::Keyword => (true, false, false),
        RecallMode::Tree => (false, true, false),
        RecallMode::Hybrid => (true, true, true),
    }
}

/// Build vector candidates from embedded rows: compute cosine to the query,
/// set vector_sim, sort by similarity (best first), and keep `limit`.
pub fn build_vector_candidates(
    query_vec: &[f32],
    embedded: Vec<(Candidate, Vec<f32>)>,
    limit: usize,
) -> Vec<Candidate> {
    let mut cands: Vec<Candidate> = embedded
        .into_iter()
        .map(|(mut c, v)| {
            c.vector_sim = Some(cosine_similarity(query_vec, &v));
            c
        })
        .collect();
    cands.sort_by(|a, b| {
        b.vector_sim
            .partial_cmp(&a.vector_sim)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    cands.truncate(limit);
    cands
}

/// Fold vector candidates into a base set: for ids already present, copy their
/// vector_sim onto the existing candidate (preserving bm25); append vector-only ids.
pub fn apply_vector_sims(mut base: Vec<Candidate>, vector: Vec<Candidate>) -> Vec<Candidate> {
    for v in vector {
        if let Some(existing) = base.iter_mut().find(|c| c.id == v.id) {
            existing.vector_sim = v.vector_sim;
        } else {
            base.push(v);
        }
    }
    base
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
            vector_sim: None,
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

    #[test]
    fn higher_vector_sim_outranks_when_equal_age() {
        let mut a = cand(1, "global", 0, None);
        let mut b = cand(2, "global", 0, None);
        a.vector_sim = Some(0.1);
        b.vector_sim = Some(0.9);
        let ranked = rank(vec![a, b], 0, 2, &Weights::default());
        assert_eq!(ranked[0].id, 2); // stronger semantic match wins
    }

    #[test]
    fn build_vector_candidates_sorts_and_truncates() {
        let q = vec![1.0f32, 0.0];
        let embedded = vec![
            (cand(1, "global", 0, None), vec![0.0f32, 1.0]), // cosine 0
            (cand(2, "global", 0, None), vec![1.0f32, 0.0]), // cosine 1
        ];
        let out = build_vector_candidates(&q, embedded, 1);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, 2);
        assert!(out[0].vector_sim.unwrap() > 0.99);
    }

    #[test]
    fn apply_vector_sims_merges_signals_and_appends() {
        let base = vec![cand(1, "global", 0, Some(-2.0))]; // keyword hit
        let mut v1 = cand(1, "global", 0, None);
        v1.vector_sim = Some(0.8);
        let mut v2 = cand(2, "global", 0, None);
        v2.vector_sim = Some(0.5);
        let merged = apply_vector_sims(base, vec![v1, v2]);
        assert_eq!(merged.len(), 2);
        let one = merged.iter().find(|c| c.id == 1).unwrap();
        assert!(one.bm25.is_some() && one.vector_sim == Some(0.8)); // both signals
    }
}
