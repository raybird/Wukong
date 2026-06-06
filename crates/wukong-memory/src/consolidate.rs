//! Consolidation: fold scattered event/note memories into Summary memories.
//! The memory layer stays LLM-agnostic via the `Summarizer` trait, mirroring
//! the `Embedder` pattern. A real LLM-backed summarizer is injected from the
//! cli/gateway layer; `ConcatSummarizer` is the dependency-free default.

use crate::error::Result;

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
}
