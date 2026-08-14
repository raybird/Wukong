use crate::embed::cosine_similarity_blob;
use crate::model::{RecallExplanation, RecallMode};
use crate::scope::Scope;
use crate::scoring::{combined_score, time_decay, Weights, HALF_LIFE_DAYS};
use crate::store::Candidate;

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "of", "to", "and", "it", "in", "on", "for",
];

const LOW_INFORMATION_CJK: &[&str] = &["好", "嗯", "可以", "謝謝", "谢谢", "了解", "收到"];

fn is_cjk_char(ch: char) -> bool {
    matches!(
        ch as u32,
        0x3400..=0x4DBF
            | 0x4E00..=0x9FFF
            | 0xF900..=0xFAFF
            | 0x3040..=0x30FF
            | 0xAC00..=0xD7AF
    )
}

pub fn contains_cjk(query: &str) -> bool {
    query.chars().any(is_cjk_char)
}

pub fn is_low_information_cjk(query: &str) -> bool {
    let normalized = query.trim();
    LOW_INFORMATION_CJK.contains(&normalized)
}

pub fn cjk_weighted_len(query: &str) -> usize {
    query
        .chars()
        .filter(|ch| !ch.is_whitespace())
        .map(|ch| if is_cjk_char(ch) { 2 } else { 1 })
        .sum()
}

/// Trivial queries (too short or only stopwords) skip recall entirely.
pub fn is_trivial(query: &str) -> bool {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return true;
    }
    if contains_cjk(trimmed) {
        return is_low_information_cjk(trimmed) || cjk_weighted_len(trimmed) < 4;
    }
    if trimmed.chars().count() < 3 {
        return true;
    }
    let tokens = tokenize(trimmed);
    tokens.is_empty() || tokens.iter().all(|t| STOPWORDS.contains(&t.as_str()))
}

/// Rewrite runs of CJK as overlapping bigrams so a tokenizer with no notion of
/// Chinese word boundaries still produces useful terms.
///
/// FTS5's unicode61 tokenizer emits exactly one token per contiguous CJK run, so
/// a stored 「幫我看一下排程設定有沒有問題」 could only ever be matched by a query
/// that repeated that whole run character for character. Everything else missed
/// the index entirely and fell through to the `LIKE '%…%'` fallback, which
/// returns rows but carries no bm25 — leaving Chinese recall unranked and its
/// reported confidence pinned at 0.
///
/// Applying this to BOTH the indexed text and the query gives both sides the
/// same boundaries: 「排程設定」 becomes 「排程 程設 設定」, which the tokenizer can
/// split, so the terms line up and bm25 works again. Rewriting only one side
/// would not help — FTS5 compares tokens for equality, and the document's single
/// large token matches nothing.
///
/// A run of one character is emitted as-is so single-character queries keep
/// working, and non-CJK text passes through untouched so English tokenisation
/// and its existing ranking are unaffected.
pub fn cjk_expand(text: &str) -> String {
    let mut out = String::with_capacity(text.len() * 2);
    let mut run: Vec<char> = Vec::new();
    for ch in text.chars() {
        if is_cjk_char(ch) {
            run.push(ch);
        } else {
            flush_cjk_run(&mut out, &mut run);
            out.push(ch);
        }
    }
    flush_cjk_run(&mut out, &mut run);
    out
}

/// Emit one accumulated CJK run, space-separated from whatever surrounds it.
/// The padding matters: without it `a排程b` would still be one token, because
/// Latin letters and CJK are both alphanumeric to the tokenizer.
fn flush_cjk_run(out: &mut String, run: &mut Vec<char>) {
    if run.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with(' ') {
        out.push(' ');
    }
    if run.len() == 1 {
        out.push(run[0]);
    } else {
        for (i, pair) in run.windows(2).enumerate() {
            if i > 0 {
                out.push(' ');
            }
            out.push(pair[0]);
            out.push(pair[1]);
        }
    }
    out.push(' ');
    run.clear();
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
///
/// The query goes through `cjk_expand` first, mirroring what was done to the
/// indexed text. Skipping it here would leave Chinese queries as one token
/// again, matching nothing.
pub fn fts_match_string(query: &str) -> Option<String> {
    let tokens = tokenize(&cjk_expand(query));
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
        if let Some(existing) = out.iter_mut().find(|k| k.id == c.id) {
            merge_source_signals(&mut existing.source_signals, c.source_signals);
        } else {
            out.push(c);
        }
    }
    out
}

fn merge_source_signals(existing: &mut Vec<String>, incoming: Vec<String>) {
    for signal in incoming {
        if !existing.contains(&signal) {
            existing.push(signal);
        }
    }
}

/// Keep only candidates whose scope is within the filter's ancestry.
pub fn filter_by_scope(candidates: Vec<Candidate>, filter: &Option<Scope>) -> Vec<Candidate> {
    match filter {
        None => candidates,
        Some(scope) => {
            let allowed: Vec<String> = scope.ancestry().iter().map(|s| s.to_string()).collect();
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
    pub explanation: RecallExplanation,
}

/// Normalize bm25 and vector_sim across candidates, compute combined scores,
/// sort descending, and take top_k. bm25: lower = better. vector_sim: higher =
/// better. Either signal absent on a candidate contributes 0 for that term.
pub fn rank(candidates: Vec<Candidate>, now: i64, top_k: usize, weights: &Weights) -> Vec<Scored> {
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
            let decay = time_decay(age, HALF_LIFE_DAYS);
            let relevance = lexical_norm.max(semantic_norm);
            let recall_bonus = 0.02 * (1.0 + c.recall_count.max(0) as f64).ln();
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
                explanation: RecallExplanation {
                    lexical: lexical_norm,
                    semantic: semantic_norm,
                    relevance,
                    decay,
                    importance: c.importance,
                    recall_bonus,
                    age_seconds: age,
                    recall_count: c.recall_count,
                    source_signals: c.source_signals,
                },
            }
        })
        .collect();

    scored.sort_by(|a, b| {
        b.score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
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
///
/// Rows arrive as raw little-endian f32 blobs straight from SQLite and are
/// scored in place — decoding each into a `Vec<f32>` first would allocate once
/// per scanned row, and this scans the whole embedded set on every recall.
pub fn build_vector_candidates(
    query_vec: &[f32],
    embedded: Vec<(Candidate, Vec<u8>)>,
    limit: usize,
) -> Vec<Candidate> {
    let mut cands: Vec<Candidate> = embedded
        .into_iter()
        .map(|(mut c, blob)| {
            c.vector_sim = Some(cosine_similarity_blob(query_vec, &blob));
            c.source_signals.push("vector".to_string());
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
            merge_source_signals(&mut existing.source_signals, v.source_signals);
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

    fn blob(v: &[f32]) -> Vec<u8> {
        crate::embed::embedding_to_blob(v)
    }

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
            source_signals: Vec::new(),
        }
    }

    #[test]
    fn trivial_queries_detected() {
        assert!(is_trivial("of"));
        assert!(is_trivial("a the"));
        assert!(is_trivial("  "));
        assert!(!is_trivial("sqlite migration"));
        assert!(!is_trivial("連線池設定"));
        assert!(!is_trivial("連線"));
        assert!(is_trivial("可以"));
        assert!(contains_cjk("連線池設定"));
    }

    #[test]
    fn cjk_expand_produces_overlapping_bigrams() {
        assert_eq!(cjk_expand("排程設定"), "排程 程設 設定 ");
        // A single character has no pair; it must survive rather than vanish.
        assert_eq!(cjk_expand("排"), "排 ");
        // Latin is untouched, so English tokenisation and ranking are unchanged.
        assert_eq!(cjk_expand("scheduler settings"), "scheduler settings");
    }

    #[test]
    fn cjk_expand_separates_runs_from_adjacent_latin() {
        // Without the padding these would tokenise as one term: Latin letters and
        // CJK are both alphanumeric, so nothing would split `a排程b`.
        assert_eq!(cjk_expand("a排程b"), "a 排程 b");
        assert_eq!(cjk_expand("User: 排程壞了"), "User: 排程 程壞 壞了 ");
    }

    #[test]
    fn expanded_query_and_document_share_terms() {
        // The property the whole fix rests on: both sides must yield overlapping
        // tokens. Rewriting only one side leaves the other as a single term.
        let doc = tokenize(&cjk_expand("幫我看一下排程設定有沒有問題"));
        for query in ["排程設定", "排程 設定", "排程"] {
            let q = tokenize(&cjk_expand(query));
            assert!(
                q.iter().any(|t| doc.contains(t)),
                "query {query:?} shares no term with the document"
            );
        }
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
        let filtered = filter_by_scope(cands, &Some(Scope::Agent("main".to_string())));
        let ids: Vec<i64> = filtered.iter().map(|c| c.id).collect();
        assert_eq!(ids, vec![1, 2]); // agent:main + global, not project:X
    }

    #[test]
    fn rank_orders_by_score_and_truncates() {
        let cands = vec![
            cand(1, "global", 0, Some(-1.0)), // weaker match (less negative bm25)
            cand(2, "global", 0, Some(-5.0)), // stronger match (more negative bm25)
        ];
        let ranked = rank(cands, 0, 1, &Weights::default());
        assert_eq!(ranked.len(), 1);
        assert_eq!(ranked[0].id, 2); // more-negative bm25 = better match => higher lexical_norm
    }

    #[test]
    fn rank_explains_score_components_and_signals() {
        let now = 200;
        let mut c = cand(1, "global", 100, Some(-5.0));
        c.vector_sim = Some(0.9);
        c.importance = 0.8;
        c.recall_count = 3;
        c.source_signals = vec![
            "keyword".to_string(),
            "recent".to_string(),
            "vector".to_string(),
        ];

        let ranked = rank(vec![c], now, 1, &Weights::default());

        assert_eq!(ranked.len(), 1);
        let explanation = &ranked[0].explanation;
        assert_eq!(explanation.lexical, 1.0);
        assert_eq!(explanation.semantic, 1.0);
        assert!(explanation.decay > 0.99);
        assert_eq!(explanation.importance, 0.8);
        assert!(explanation.recall_bonus > 0.0);
        assert_eq!(explanation.age_seconds, 100);
        assert_eq!(explanation.recall_count, 3);
        assert_eq!(
            explanation.source_signals,
            vec![
                "keyword".to_string(),
                "recent".to_string(),
                "vector".to_string()
            ]
        );
    }

    #[test]
    fn merge_candidates_preserves_recent_signal_on_duplicate() {
        let mut keyword = cand(1, "global", 100, Some(-2.0));
        keyword.source_signals = vec!["keyword".to_string()];
        let mut recent = cand(1, "global", 100, None);
        recent.source_signals = vec!["recent".to_string()];

        let merged = merge_candidates(vec![keyword], vec![recent]);

        assert_eq!(merged.len(), 1);
        assert_eq!(
            merged[0].source_signals,
            vec!["keyword".to_string(), "recent".to_string()]
        );
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
            (cand(1, "global", 0, None), blob(&[0.0, 1.0])), // cosine 0
            (cand(2, "global", 0, None), blob(&[1.0, 0.0])), // cosine 1
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
