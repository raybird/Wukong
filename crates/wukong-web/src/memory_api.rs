use serde::Deserialize;
use wukong_memory::{MemoryKind, MemoryRecordsPage, Snapshot};

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

pub type MemorySummaryResponse = Snapshot;
pub type MemoryRecordsResponse = MemoryRecordsPage;
