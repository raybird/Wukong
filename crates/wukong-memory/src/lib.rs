//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod consolidate;
pub mod embed;
pub mod error;
pub mod model;
pub mod prune;
pub mod recall;
pub mod scope;
pub mod scoring;
pub mod store;

pub use error::{MemoryError, Result};
pub use model::{
    Evidence, MemoryItem, MemoryKind, RecallHit, RecallMode, RecallQuery, RememberInput,
    ScopeCount, Stats, WukongResult,
};
pub use consolidate::{
    plan_batches, ConcatSummarizer, ConsolidatePlan, ConsolidatePolicy, MockSummarizer, Summarizer,
};
pub use embed::{cosine_similarity, Embedder, MockEmbedder};
#[cfg(feature = "embed")]
pub use embed::FastembedBackend;
pub use prune::PrunePolicy;
pub use scope::Scope;
pub use scoring::Weights;

use embed::embedding_to_blob;
use recall::{
    apply_vector_sims, build_vector_candidates, filter_by_scope, fts_match_string, is_trivial,
    merge_candidates, rank, sources_for_mode,
};
use store::{Candidate, Store};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

/// Internal fetch fan-out before ranking.
fn fetch_limit(top_k: usize) -> i64 {
    (top_k.max(5) * 10).max(50) as i64
}

fn now_unix() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The public memory facade. Wraps the store, ranking weights, and an optional
/// embedder. With no embedder, recall and remember behave exactly like v1.
pub struct Memory {
    store: Store,
    weights: Weights,
    embedder: Option<Arc<dyn Embedder>>,
}

impl Memory {
    /// Open (creating if missing) the memory database. No semantic layer.
    pub async fn open(db_url: &str) -> Result<Memory> {
        Ok(Memory {
            store: Store::open(db_url).await?,
            weights: Weights::default(),
            embedder: None,
        })
    }

    /// Attach an embedder, enabling the semantic layer. Spawns a background task
    /// to backfill embeddings for any rows that lack them.
    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder.clone());
        let store = self.store.clone();
        tokio::spawn(async move {
            if let Err(e) = backfill(&store, embedder.as_ref()).await {
                eprintln!("wukong-memory: backfill failed: {e}");
            }
        });
        self
    }

    /// Embed and store vectors for every memory still missing one. Awaitable
    /// directly (used by tests and by the spawned background task).
    pub async fn backfill_embeddings(&self) -> Result<()> {
        match &self.embedder {
            Some(emb) => backfill(&self.store, emb.as_ref()).await,
            None => Ok(()),
        }
    }

    /// Persist a batch of memories. Returns the new row ids.
    pub async fn remember(&self, input: RememberInput) -> Result<WukongResult<Vec<i64>>> {
        let start = Instant::now();
        let scope = Scope::parse(&input.scope)?;
        let scope_str = scope.to_string();
        let now = now_unix();

        if let Some(session_id) = &input.session_id {
            self.store.upsert_session(session_id, &scope_str, now).await?;
        }

        let mut ids = Vec::with_capacity(input.items.len());
        for item in &input.items {
            let importance = item.importance.unwrap_or(1.0);
            let id = self
                .store
                .insert_memory(
                    input.session_id.as_deref(),
                    &scope_str,
                    item.kind,
                    &item.text,
                    importance,
                    now,
                )
                .await?;
            ids.push(id);
            if let Some(emb) = &self.embedder {
                match emb.embed(&item.text) {
                    Ok(v) => {
                        let _ = self
                            .store
                            .update_embedding(id, &embedding_to_blob(&v), emb.model_id())
                            .await;
                    }
                    Err(e) => eprintln!("wukong-memory: embed on remember failed: {e}"),
                }
            }
        }

        Ok(WukongResult {
            data: ids,
            evidence: Vec::new(),
            confidence: 1.0,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Recall memories relevant to a query.
    pub async fn recall(&self, query: RecallQuery) -> Result<WukongResult<Vec<RecallHit>>> {
        let start = Instant::now();

        // Adaptive gate: skip trivial queries.
        if is_trivial(&query.query) {
            return Ok(WukongResult {
                data: Vec::new(),
                evidence: Vec::new(),
                confidence: 0.0,
                latency_ms: start.elapsed().as_millis() as u64,
            });
        }

        let scope_filter = match &query.scope {
            Some(s) => Some(Scope::parse(s)?),
            None => None,
        };
        let (use_keyword, use_recent, use_vector) = sources_for_mode(query.mode);
        let limit = fetch_limit(query.top_k);
        let now = now_unix();

        let keyword = if use_keyword {
            match fts_match_string(&query.query) {
                Some(expr) => self.store.keyword_candidates(&expr, limit).await?,
                None => Vec::new(),
            }
        } else {
            Vec::new()
        };
        let recent = if use_recent {
            self.store.recent_candidates(limit).await?
        } else {
            Vec::new()
        };

        let merged: Vec<Candidate> = match query.mode {
            RecallMode::Keyword => keyword,
            RecallMode::Tree => recent,
            RecallMode::Hybrid => merge_candidates(keyword, recent),
        };

        // Vector source: only when enabled by mode AND an embedder is attached.
        let merged = if use_vector {
            match &self.embedder {
                Some(emb) => {
                    let qvec = emb.embed(&query.query)?;
                    let embedded = self.store.embedded_candidates(limit).await?;
                    let vector_cands =
                        build_vector_candidates(&qvec, embedded, query.top_k.max(5) * 4);
                    apply_vector_sims(merged, vector_cands)
                }
                None => merged,
            }
        } else {
            merged
        };

        let filtered = filter_by_scope(merged, &scope_filter);
        let scored = rank(filtered, now, query.top_k, &self.weights);

        let ids: Vec<i64> = scored.iter().map(|s| s.id).collect();
        if !ids.is_empty() {
            self.store.touch_recalled(&ids, now).await?;
        }

        let evidence: Vec<Evidence> = scored
            .iter()
            .map(|s| Evidence {
                id: s.id,
                scope: s.scope.clone(),
                score: s.score,
            })
            .collect();
        let confidence = scored.first().map(|s| s.score.clamp(0.0, 1.0)).unwrap_or(0.0);
        let hits: Vec<RecallHit> = scored
            .into_iter()
            .map(|s| RecallHit {
                id: s.id,
                scope: s.scope,
                kind: s.kind,
                text: s.text,
                score: s.score,
            })
            .collect();

        Ok(WukongResult {
            data: hits,
            evidence,
            confidence,
            latency_ms: start.elapsed().as_millis() as u64,
        })
    }

    /// Plan (without executing) which source ids would fold into each summary.
    pub async fn plan_consolidation(
        &self,
        scope: &str,
        policy: &consolidate::ConsolidatePolicy,
    ) -> Result<consolidate::ConsolidatePlan> {
        let rows = self.store.consolidation_candidates(scope).await?;
        let batches = consolidate::plan_batches(rows, policy.batch_size)
            .into_iter()
            .map(|b| b.iter().map(|r| r.id).collect())
            .collect();
        Ok(consolidate::ConsolidatePlan { batches })
    }

    /// Execute consolidation: for each batch, summarize the texts, insert a
    /// Summary memory, and mark the sources as folded into it. Returns the new
    /// summary ids. Each new summary is embedded (if an embedder is attached)
    /// and mirrored to markdown (if a sink is attached).
    pub async fn consolidate(
        &self,
        scope: &str,
        policy: &consolidate::ConsolidatePolicy,
        summarizer: &dyn consolidate::Summarizer,
    ) -> Result<Vec<i64>> {
        let rows = self.store.consolidation_candidates(scope).await?;
        let batches = consolidate::plan_batches(rows, policy.batch_size);
        let now = now_unix();
        let mut summary_ids = Vec::with_capacity(batches.len());
        for batch in batches {
            if batch.is_empty() {
                continue;
            }
            let texts: Vec<String> = batch.iter().map(|r| r.text.clone()).collect();
            let importance = batch.iter().map(|r| r.importance).fold(0.0_f64, f64::max);
            let summary_text = summarizer.summarize(&texts)?;
            let summary_id = self
                .store
                .insert_memory(None, scope, MemoryKind::Summary, &summary_text, importance, now)
                .await?;
            let src_ids: Vec<i64> = batch.iter().map(|r| r.id).collect();
            self.store.mark_consolidated(&src_ids, summary_id).await?;
            if let Some(emb) = &self.embedder {
                if let Ok(v) = emb.embed(&summary_text) {
                    let _ = self
                        .store
                        .update_embedding(summary_id, &embedding_to_blob(&v), emb.model_id())
                        .await;
                }
            }
            summary_ids.push(summary_id);
        }
        Ok(summary_ids)
    }

    /// Ids that `prune` would delete (dry-run).
    pub async fn plan_prune(
        &self,
        scope: Option<&str>,
        policy: &prune::PrunePolicy,
    ) -> Result<Vec<i64>> {
        self.store
            .prune_candidates(scope, policy.max_age_secs, policy.importance_floor, now_unix())
            .await
    }

    /// Delete prunable rows. Returns the number deleted.
    pub async fn prune(&self, scope: Option<&str>, policy: &prune::PrunePolicy) -> Result<u64> {
        let ids = self.plan_prune(scope, policy).await?;
        if ids.is_empty() {
            return Ok(0);
        }
        self.store.delete_memories(&ids).await
    }

    /// Aggregate statistics.
    pub async fn stats(&self) -> Result<Stats> {
        self.store.stats().await
    }
}

/// Embed and persist vectors for memories lacking them, in batches.
async fn backfill(store: &Store, embedder: &dyn Embedder) -> Result<()> {
    loop {
        let batch = store.rows_missing_embedding(32).await?;
        if batch.is_empty() {
            break;
        }
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let vecs = embedder.embed_batch(&texts)?;
        for ((id, _), v) in batch.iter().zip(vecs.iter()) {
            store
                .update_embedding(*id, &embedding_to_blob(v), embedder.model_id())
                .await?;
        }
        tokio::task::yield_now().await;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::consolidate::MockSummarizer;
    use crate::model::{MemoryItem, RememberInput};
    use tempfile::NamedTempFile;

    async fn open_mem() -> Memory {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        Memory::open(&url).await.unwrap()
    }

    async fn remember_event(mem: &Memory, scope: &str, text: &str) {
        mem.remember(RememberInput {
            scope: scope.to_string(),
            session_id: None,
            items: vec![MemoryItem { kind: MemoryKind::Event, text: text.to_string(), importance: None }],
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn consolidate_creates_summary_and_marks_sources() {
        let mem = open_mem().await;
        remember_event(&mem, "project:X", "did A").await;
        remember_event(&mem, "project:X", "did B").await;

        let plan = mem
            .plan_consolidation("project:X", &ConsolidatePolicy { batch_size: 20 })
            .await
            .unwrap();
        assert_eq!(plan.batches.len(), 1);
        assert_eq!(plan.batches[0].len(), 2);

        let summary_ids = mem
            .consolidate("project:X", &ConsolidatePolicy { batch_size: 20 }, &MockSummarizer)
            .await
            .unwrap();
        assert_eq!(summary_ids.len(), 1);

        // Sources are now consolidated => no longer candidates.
        let after = mem
            .plan_consolidation("project:X", &ConsolidatePolicy { batch_size: 20 })
            .await
            .unwrap();
        assert!(after.batches.is_empty());

        // The summary text came from the summarizer.
        let recent = mem.store.recent_candidates(10).await.unwrap();
        assert!(recent.iter().any(|c| c.kind == MemoryKind::Summary && c.text == "SUMMARY(2)"));
    }

    #[tokio::test]
    async fn prune_deletes_only_low_value() {
        let mem = open_mem().await;
        // Two events, then consolidate so they become prunable.
        remember_event(&mem, "project:X", "did A").await;
        remember_event(&mem, "project:X", "did B").await;
        mem.consolidate("project:X", &ConsolidatePolicy::default(), &MockSummarizer)
            .await
            .unwrap();

        // dry-run plan lists the two consolidated source events.
        let plan = mem.plan_prune(Some("project:X"), &PrunePolicy::default()).await.unwrap();
        assert_eq!(plan.len(), 2);

        let deleted = mem.prune(Some("project:X"), &PrunePolicy::default()).await.unwrap();
        assert_eq!(deleted, 2);

        // The summary survives (never prunable).
        let recent = mem.store.recent_candidates(10).await.unwrap();
        assert!(recent.iter().all(|c| c.kind == MemoryKind::Summary));
    }
}
