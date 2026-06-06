//! Consolidation: fold scattered event/note memories into Summary memories.
//! The memory layer stays LLM-agnostic via the `Summarizer` trait, mirroring
//! the `Embedder` pattern. A real LLM-backed summarizer is injected from the
//! cli/gateway layer; `ConcatSummarizer` is the dependency-free default.

use crate::error::Result;
use crate::store::ConsolidationRow;

/// Tuning for a consolidation pass.
#[derive(Debug, Clone)]
pub struct ConsolidatePolicy {
    /// Max source rows folded into one summary.
    pub batch_size: usize,
}

impl Default for ConsolidatePolicy {
    fn default() -> Self {
        Self { batch_size: 20 }
    }
}

/// A dry-run plan: the source ids that would form each summary.
#[derive(Debug, Clone)]
pub struct ConsolidatePlan {
    pub batches: Vec<Vec<i64>>,
}

/// Group foldable rows into batches: rows sharing a session_id stay together
/// (first-seen order), null-session rows are chunked by `batch_size`. Every
/// group is itself capped at `batch_size`.
pub fn plan_batches(rows: Vec<ConsolidationRow>, batch_size: usize) -> Vec<Vec<ConsolidationRow>> {
    let bs = batch_size.max(1);
    let mut sessions: Vec<(String, Vec<ConsolidationRow>)> = Vec::new();
    let mut null_rows: Vec<ConsolidationRow> = Vec::new();
    for r in rows {
        match &r.session_id {
            Some(sid) => {
                if let Some(slot) = sessions.iter_mut().find(|(k, _)| k == sid) {
                    slot.1.push(r);
                } else {
                    sessions.push((sid.clone(), vec![r]));
                }
            }
            None => null_rows.push(r),
        }
    }
    let mut out: Vec<Vec<ConsolidationRow>> = Vec::new();
    for (_, group) in sessions {
        for chunk in group.chunks(bs) {
            out.push(chunk.to_vec());
        }
    }
    for chunk in null_rows.chunks(bs) {
        out.push(chunk.to_vec());
    }
    out
}

/// Condenses a group of memory texts into a single summary string.
/// Object-safe (sync) so callers can hold `&dyn Summarizer`.
pub trait Summarizer: Send + Sync {
    fn summarize(&self, texts: &[String]) -> Result<String>;
}

/// Mechanical default: joins texts in order under a header. No LLM.
pub struct ConcatSummarizer;

impl Summarizer for ConcatSummarizer {
    fn summarize(&self, texts: &[String]) -> Result<String> {
        Ok(format!("[摘要] {}", texts.join(" / ")))
    }
}

/// Deterministic summarizer for tests. NOT semantic.
pub struct MockSummarizer;

impl Summarizer for MockSummarizer {
    fn summarize(&self, texts: &[String]) -> Result<String> {
        Ok(format!("SUMMARY({})", texts.len()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn concat_summarizer_joins_texts() {
        let s = ConcatSummarizer;
        let out = s.summarize(&["a".to_string(), "b".to_string()]).unwrap();
        assert!(out.contains("a / b"));
    }

    #[test]
    fn mock_summarizer_reports_count() {
        let s = MockSummarizer;
        assert_eq!(s.summarize(&["x".to_string(), "y".to_string()]).unwrap(), "SUMMARY(2)");
    }

    use crate::store::ConsolidationRow;

    fn row(id: i64, session: Option<&str>) -> ConsolidationRow {
        ConsolidationRow { id, session_id: session.map(|s| s.to_string()), text: format!("t{id}"), importance: 1.0 }
    }

    #[test]
    fn plan_batches_groups_session_then_chunks_null() {
        let rows = vec![
            row(1, Some("a")),
            row(2, Some("a")),
            row(3, None),
            row(4, None),
            row(5, None),
        ];
        let batches = plan_batches(rows, 2);
        // Session "a" batched together first, then null rows chunked by 2.
        let ids: Vec<Vec<i64>> = batches
            .iter()
            .map(|b| b.iter().map(|r| r.id).collect())
            .collect();
        assert_eq!(ids, vec![vec![1, 2], vec![3, 4], vec![5]]);
    }
}
