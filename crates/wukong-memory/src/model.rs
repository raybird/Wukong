use serde::{Deserialize, Serialize};

/// Category of a stored memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Decision,
    Event,
    Skill,
    Note,
    Summary,
}

impl MemoryKind {
    /// Stable lowercase string used for DB storage.
    pub fn as_str(&self) -> &'static str {
        match self {
            MemoryKind::Decision => "decision",
            MemoryKind::Event => "event",
            MemoryKind::Skill => "skill",
            MemoryKind::Note => "note",
            MemoryKind::Summary => "summary",
        }
    }

    /// Parse from a DB string; unknown values fall back to `Note`.
    pub fn from_db_str(s: &str) -> MemoryKind {
        match s {
            "decision" => MemoryKind::Decision,
            "event" => MemoryKind::Event,
            "skill" => MemoryKind::Skill,
            "summary" => MemoryKind::Summary,
            _ => MemoryKind::Note,
        }
    }
}

/// A single memory to persist.
#[derive(Debug, Clone, Deserialize)]
pub struct MemoryItem {
    pub kind: MemoryKind,
    pub text: String,
    /// Defaults to 1.0 when omitted (see remember()).
    #[serde(default)]
    pub importance: Option<f64>,
    /// Optional caller-provided idempotency key. When present, repeated writes
    /// return the existing row id instead of inserting a duplicate.
    #[serde(default)]
    pub dedupe_key: Option<String>,
}

/// Input to `remember`.
#[derive(Debug, Clone, Deserialize)]
pub struct RememberInput {
    /// Scope string, e.g. "project:Wukong".
    pub scope: String,
    pub session_id: Option<String>,
    pub items: Vec<MemoryItem>,
}

/// Recall strategy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RecallMode {
    Keyword,
    Tree,
    #[default]
    Hybrid,
}

fn default_top_k() -> usize {
    5
}

/// Input to `recall`.
#[derive(Debug, Clone, Deserialize)]
pub struct RecallQuery {
    pub query: String,
    #[serde(default = "default_top_k")]
    pub top_k: usize,
    /// Optional scope filter; when set, only this scope + ancestors match.
    pub scope: Option<String>,
    #[serde(default)]
    pub mode: RecallMode,
}

/// A ranked recall result.
#[derive(Debug, Clone, Serialize)]
pub struct RecallHit {
    pub id: i64,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    pub score: f64,
    pub explanation: RecallExplanation,
}

/// Explainable score breakdown for a recall result.
#[derive(Debug, Clone, Serialize)]
pub struct RecallExplanation {
    pub lexical: f64,
    pub semantic: f64,
    pub decay: f64,
    pub importance: f64,
    pub recall_bonus: f64,
    pub age_seconds: i64,
    pub recall_count: i64,
    pub source_signals: Vec<String>,
}

/// Provenance entry attached to every result envelope.
#[derive(Debug, Clone, Serialize)]
pub struct Evidence {
    pub id: i64,
    pub scope: String,
    pub score: f64,
}

/// Standard response envelope (mirrors Memoria's MemoriaResult).
#[derive(Debug, Clone, Serialize)]
pub struct WukongResult<T> {
    pub data: T,
    pub evidence: Vec<Evidence>,
    pub confidence: f64,
    pub latency_ms: u64,
}

/// Count of memories within one scope.
#[derive(Debug, Clone, Serialize)]
pub struct ScopeCount {
    pub scope: String,
    pub count: i64,
}

/// Aggregate statistics.
#[derive(Debug, Clone, Serialize)]
pub struct Stats {
    pub total: i64,
    pub by_scope: Vec<ScopeCount>,
}

/// Count of memories of one kind.
#[derive(Debug, Clone, Serialize)]
pub struct KindCount {
    pub kind: MemoryKind,
    pub count: i64,
}

/// Memory counts bucketed by age relative to "now".
#[derive(Debug, Clone, Serialize)]
pub struct AgeBuckets {
    pub last_day: i64,
    pub last_week: i64,
    pub last_month: i64,
    pub older: i64,
}

/// How many memories carry an embedding.
#[derive(Debug, Clone, Serialize)]
pub struct EmbeddingCoverage {
    pub embedded: i64,
    pub total: i64,
}

/// Rich health snapshot for observability.
#[derive(Debug, Clone, Serialize)]
pub struct Snapshot {
    pub total: i64,
    pub by_scope: Vec<ScopeCount>,
    pub by_kind: Vec<KindCount>,
    pub age: AgeBuckets,
    pub embedding: EmbeddingCoverage,
    pub consolidation_candidates: i64,
    pub prune_candidates: i64,
}

/// A single memory row for Web observability views.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecord {
    pub id: i64,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    pub importance: f64,
    pub created_at: i64,
    pub last_recalled_at: Option<i64>,
    pub recall_count: i64,
    pub has_embedding: bool,
    pub consolidated_into: Option<i64>,
}

/// A bounded page of memory records.
#[derive(Debug, Clone, Serialize)]
pub struct MemoryRecordsPage {
    pub records: Vec<MemoryRecord>,
    pub has_more: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_mode_defaults_to_hybrid() {
        assert_eq!(RecallMode::default(), RecallMode::Hybrid);
    }

    #[test]
    fn recall_query_applies_defaults() {
        let q: RecallQuery = serde_json::from_str(r#"{"query":"sqlite"}"#).unwrap();
        assert_eq!(q.top_k, 5);
        assert_eq!(q.mode, RecallMode::Hybrid);
        assert!(q.scope.is_none());
    }

    #[test]
    fn memory_kind_serde_is_snake_case() {
        let json = serde_json::to_string(&MemoryKind::Decision).unwrap();
        assert_eq!(json, "\"decision\"");
        let parsed: MemoryKind = serde_json::from_str("\"skill\"").unwrap();
        assert_eq!(parsed, MemoryKind::Skill);
    }

    #[test]
    fn memory_kind_db_roundtrip() {
        for k in [
            MemoryKind::Decision,
            MemoryKind::Event,
            MemoryKind::Skill,
            MemoryKind::Note,
            MemoryKind::Summary,
        ] {
            assert_eq!(MemoryKind::from_db_str(k.as_str()), k);
        }
        assert_eq!(MemoryKind::from_db_str("garbage"), MemoryKind::Note);
    }
}
