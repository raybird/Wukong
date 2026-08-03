use crate::WukongError;
use wukong_gateway::backend::AiBackend;
use wukong_gateway::summarize::OpencodeSummarizer;
use wukong_memory::{ConsolidatePolicy, Memory, PrunePolicy};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AutoMaintenanceReport {
    pub scopes_checked: usize,
    pub scopes_consolidated: usize,
    pub summaries_created: usize,
    pub memories_pruned: u64,
}

/// Consolidate every scope above the threshold and delete only folded sources.
pub async fn memory_auto_maintenance<B: AiBackend + Sync>(
    memory: &Memory,
    backend: &B,
    candidate_threshold: i64,
) -> Result<AutoMaintenanceReport, WukongError> {
    let mut report = AutoMaintenanceReport::default();
    if candidate_threshold <= 0 {
        return Ok(report);
    }
    let policy = ConsolidatePolicy::default();
    for scope in memory.scopes().await? {
        report.scopes_checked += 1;
        let already_folded = memory.prune_consolidated(Some(&scope)).await?;
        report.memories_pruned += already_folded;
        let snapshot = memory.snapshot(Some(&scope)).await?;
        if snapshot.consolidation_candidates < candidate_threshold {
            continue;
        }

        let summarizer = OpencodeSummarizer::new(backend);
        let ids = memory.consolidate(&scope, &policy, &summarizer).await?;
        let deleted = memory.prune_consolidated(Some(&scope)).await?;
        report.scopes_consolidated += 1;
        report.summaries_created += ids.len();
        report.memories_pruned += deleted;
        eprintln!(
            "memory_consolidated scope={} summaries={} pruned={}",
            scope,
            ids.len(),
            deleted
        );
    }
    Ok(report)
}

pub async fn memory_snapshot(memory: &Memory, scope: Option<&str>) -> Result<String, WukongError> {
    let snap = memory.snapshot(scope).await?;
    let mut out = String::new();
    out.push_str(&format!("總計: {}\n", snap.total));
    out.push_str("依範圍:\n");
    for s in &snap.by_scope {
        out.push_str(&format!("  {} = {}\n", s.scope, s.count));
    }
    out.push_str("依類型:\n");
    for k in &snap.by_kind {
        out.push_str(&format!("  {} = {}\n", k.kind.as_str(), k.count));
    }
    out.push_str(&format!(
        "年齡: <1d={} <7d={} <30d={} older={}\n",
        snap.age.last_day, snap.age.last_week, snap.age.last_month, snap.age.older
    ));
    out.push_str(&format!(
        "embedding 覆蓋: {}/{}\n",
        snap.embedding.embedded, snap.embedding.total
    ));
    out.push_str(&format!(
        "consolidation 候選: {}\n",
        snap.consolidation_candidates
    ));
    out.push_str(&format!("prune 候選: {}", snap.prune_candidates));
    Ok(out)
}

pub async fn memory_consolidate<B: AiBackend + Sync>(
    memory: &Memory,
    backend: &B,
    scope: &str,
    dry_run: bool,
) -> Result<String, WukongError> {
    let policy = ConsolidatePolicy::default();
    if dry_run {
        let plan = memory.plan_consolidation(scope, &policy).await?;
        let mut out = format!("[dry-run] 將產生 {} 筆摘要:", plan.batches.len());
        for (i, b) in plan.batches.iter().enumerate() {
            out.push_str(&format!("\n  批 {}: {} 筆來源 {:?}", i + 1, b.len(), b));
        }
        Ok(out)
    } else {
        let summarizer = OpencodeSummarizer::new(backend);
        let ids = memory.consolidate(scope, &policy, &summarizer).await?;
        Ok(format!("已建立 {} 筆摘要: {:?}", ids.len(), ids))
    }
}

pub async fn memory_prune(
    memory: &Memory,
    scope: Option<&str>,
    dry_run: bool,
) -> Result<String, WukongError> {
    let policy = PrunePolicy::default();
    if dry_run {
        let ids = memory.plan_prune(scope, &policy).await?;
        Ok(format!("[dry-run] 將刪除 {} 筆: {:?}", ids.len(), ids))
    } else {
        let n = memory.prune(scope, &policy).await?;
        Ok(format!("已刪除 {n} 筆"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;
    use wukong_memory::{MemoryItem, MemoryKind, RememberInput};

    struct MockBackend {
        prompts: Mutex<Vec<String>>,
    }

    impl AiBackend for MockBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.prompts.lock().unwrap().push(req.prompt);
            Ok(AgentResponse {
                text: "summary".to_string(),
                session_id: Some("ses_helper".to_string()),
            })
        }
    }

    async fn open_memory() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    #[tokio::test]
    async fn snapshot_returns_human_readable_text() {
        let mem = open_memory().await;
        mem.remember(RememberInput {
            scope: "project:T".to_string(),
            session_id: None,
            items: vec![MemoryItem {
                kind: MemoryKind::Note,
                text: "note".to_string(),
                importance: None,
                dedupe_key: None,
            }],
        })
        .await
        .unwrap();

        let out = memory_snapshot(&mem, Some("project:T")).await.unwrap();

        assert!(out.contains("總計: 1"));
        assert!(out.contains("project:T"));
    }

    #[tokio::test]
    async fn prune_dry_run_returns_candidate_text() {
        let mem = open_memory().await;
        let out = memory_prune(&mem, Some("project:T"), true).await.unwrap();
        assert!(out.contains("[dry-run] 將刪除 0 筆"));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn auto_maintenance_consolidates_all_scopes_and_prunes_only_folded_sources() {
        let mem = open_memory().await;
        let backend = MockBackend {
            prompts: Mutex::new(Vec::new()),
        };
        for scope in ["project:A", "project:B"] {
            for text in ["a", "b"] {
                mem.remember(RememberInput {
                    scope: scope.to_string(),
                    session_id: Some("ses_1".to_string()),
                    items: vec![MemoryItem {
                        kind: MemoryKind::Event,
                        text: text.to_string(),
                        importance: None,
                        dedupe_key: None,
                    }],
                })
                .await
                .unwrap();
            }
        }
        mem.remember(RememberInput {
            scope: "project:C".to_string(),
            session_id: None,
            items: vec![MemoryItem {
                kind: MemoryKind::Decision,
                text: "keep decision".to_string(),
                importance: None,
                dedupe_key: None,
            }],
        })
        .await
        .unwrap();

        let report = memory_auto_maintenance(&mem, &backend, 2).await.unwrap();
        assert_eq!(report.scopes_checked, 3);
        assert_eq!(report.scopes_consolidated, 2);
        assert_eq!(report.summaries_created, 2);
        assert_eq!(report.memories_pruned, 4);
        let records = mem.records(None, None, 20).await.unwrap();
        assert!(records
            .records
            .iter()
            .any(|record| record.text == "keep decision"));
    }
}
