use serde::{Deserialize, Serialize};
use wukong_memory::{Evidence, MemoryKind, MemoryRecordsPage, RecallHit, Snapshot};

#[derive(Debug, Deserialize)]
pub struct MemorySummaryQuery {
    pub token: Option<String>,
    pub scope: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MemoryRecordsQuery {
    pub token: Option<String>,
    pub scope: Option<String>,
    pub kind: Option<String>,
    pub limit: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct RecallPreviewRequest {
    pub query: String,
    pub scope: Option<String>,
    pub top_k: Option<usize>,
    pub mode: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormalizedRecallRequest {
    pub query: String,
    pub scope: String,
    pub top_k: usize,
    pub mode: String,
}

#[derive(Debug, Serialize)]
pub struct RecallPreviewResponse {
    pub query: String,
    pub scope: String,
    pub mode: String,
    pub hits: Vec<RecallHit>,
    pub evidence: Vec<Evidence>,
    pub confidence: f64,
    pub latency_ms: u64,
}

pub fn parse_kind(kind: Option<&str>) -> Result<Option<MemoryKind>, String> {
    match kind.map(str::trim).filter(|s| !s.is_empty()) {
        None => Ok(None),
        Some("decision") => Ok(Some(MemoryKind::Decision)),
        Some("event") => Ok(Some(MemoryKind::Event)),
        Some("skill") => Ok(Some(MemoryKind::Skill)),
        Some("note") => Ok(Some(MemoryKind::Note)),
        Some("summary") => Ok(Some(MemoryKind::Summary)),
        Some(other) => Err(format!("unknown memory kind: {other}")),
    }
}

pub fn capped_records_limit(limit: Option<i64>) -> i64 {
    limit.unwrap_or(20).clamp(1, 100)
}

pub fn normalize_recall_request(
    req: RecallPreviewRequest,
    default_scope: &str,
) -> Result<NormalizedRecallRequest, String> {
    let query = req.query.trim().to_string();
    if query.is_empty() {
        return Err("query is required".to_string());
    }

    let scope = req
        .scope
        .map(|scope| scope.trim().to_string())
        .filter(|scope| !scope.is_empty())
        .unwrap_or_else(|| default_scope.to_string());
    let top_k = req.top_k.unwrap_or(8).clamp(1, 20);
    let mode = req
        .mode
        .map(|mode| mode.trim().to_ascii_lowercase())
        .filter(|mode| !mode.is_empty())
        .unwrap_or_else(|| "hybrid".to_string());
    if mode != "hybrid" {
        return Err(format!("unsupported recall mode: {mode}"));
    }

    Ok(NormalizedRecallRequest {
        query,
        scope,
        top_k,
        mode,
    })
}

pub type MemorySummaryResponse = Snapshot;
pub type MemoryRecordsResponse = MemoryRecordsPage;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalize_recall_query_rejects_blank_query() {
        let req = RecallPreviewRequest {
            query: "  ".to_string(),
            scope: None,
            top_k: None,
            mode: None,
        };

        let err = normalize_recall_request(req, "project:Wukong").unwrap_err();

        assert!(err.contains("query is required"));
    }

    #[test]
    fn normalize_recall_query_defaults_scope_top_k_and_mode() {
        let req = RecallPreviewRequest {
            query: " schedule memory ".to_string(),
            scope: None,
            top_k: None,
            mode: None,
        };

        let normalized = normalize_recall_request(req, "project:Wukong").unwrap();

        assert_eq!(normalized.query, "schedule memory");
        assert_eq!(normalized.scope, "project:Wukong");
        assert_eq!(normalized.top_k, 8);
        assert_eq!(normalized.mode, "hybrid");
    }

    #[test]
    fn normalize_recall_query_clamps_top_k() {
        let req = RecallPreviewRequest {
            query: "memory".to_string(),
            scope: Some("global".to_string()),
            top_k: Some(100),
            mode: Some("hybrid".to_string()),
        };

        let normalized = normalize_recall_request(req, "project:Wukong").unwrap();

        assert_eq!(normalized.top_k, 20);
    }

    #[test]
    fn normalize_recall_query_rejects_unknown_mode() {
        let req = RecallPreviewRequest {
            query: "memory".to_string(),
            scope: None,
            top_k: None,
            mode: Some("keyword".to_string()),
        };

        let err = normalize_recall_request(req, "project:Wukong").unwrap_err();

        assert!(err.contains("unsupported recall mode"));
    }
}
