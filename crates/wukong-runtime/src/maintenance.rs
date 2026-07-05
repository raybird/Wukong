use crate::WukongError;
use wukong_gateway::backend::AiBackend;
use wukong_gateway::summarize::OpencodeSummarizer;
use wukong_memory::{ConsolidatePolicy, Memory, PrunePolicy};

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
    use tempfile::NamedTempFile;
    use wukong_memory::{MemoryItem, MemoryKind, RememberInput};

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
}
