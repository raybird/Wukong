//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod consolidate;
pub mod embed;
pub mod error;
pub mod markdown;
pub mod model;
pub mod prune;
pub mod recall;
pub mod scope;
pub mod scoring;
pub mod store;

pub use consolidate::{
    plan_batches, ConcatSummarizer, ConsolidatePlan, ConsolidatePolicy, MockSummarizer, Summarizer,
};
#[cfg(feature = "embed")]
pub use embed::FastembedBackend;
pub use embed::{cosine_similarity, Embedder, MockEmbedder};
pub use error::{MemoryError, Result};
pub use markdown::MarkdownSink;
pub use model::{
    AgeBuckets, AgentSessionState, EmbeddingCoverage, Evidence, KindCount, MemoryItem, MemoryKind,
    MemoryRecord, MemoryRecordsPage, RecallExplanation, RecallHit, RecallMode, RecallQuery,
    RecallTelemetryInput, RecallTelemetrySummary, RememberInput, ScopeCount, Snapshot, Stats,
    WukongResult,
};
pub use prune::PrunePolicy;
pub use scope::Scope;
pub use scoring::Weights;

use embed::embedding_to_blob;
use recall::{
    apply_vector_sims, build_vector_candidates, contains_cjk, filter_by_scope, fts_match_string,
    is_trivial, merge_candidates, rank, sources_for_mode,
};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};
use store::{Candidate, Store};

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

fn query_hash(query: &str) -> String {
    let mut hash = 0xcbf29ce484222325_u64;
    for byte in query.trim().as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("{hash:016x}")
}

/// The public memory facade. Wraps the store, ranking weights, and an optional
/// embedder. With no embedder, recall and remember behave exactly like v1.
pub struct Memory {
    store: Store,
    weights: Weights,
    embedder: Option<Arc<dyn Embedder>>,
    md_sink: Option<MarkdownSink>,
}

impl Memory {
    /// Open (creating if missing) the memory database. No semantic layer.
    pub async fn open(db_url: &str) -> Result<Memory> {
        Ok(Memory {
            store: Store::open(db_url).await?,
            weights: Weights::default(),
            embedder: None,
            md_sink: None,
        })
    }

    /// Attach a markdown mirror directory. Every `remember` then also appends
    /// to the per-scope markdown file (best-effort).
    pub fn with_markdown(mut self, dir: impl Into<std::path::PathBuf>) -> Self {
        self.md_sink = Some(MarkdownSink::new(dir));
        self
    }

    /// Attach an embedder, enabling the semantic layer. Spawns a background task
    /// to backfill embeddings for any rows that lack them.
    pub fn with_embedder(mut self, embedder: Arc<dyn Embedder>) -> Self {
        self.embedder = Some(embedder.clone());
        let store = self.store.clone();
        tokio::spawn(async move {
            if let Err(e) = backfill(&store, &embedder).await {
                eprintln!("wukong-memory: backfill failed: {e}");
            }
        });
        self
    }

    /// Embed and store vectors for every memory still missing one. Awaitable
    /// directly (used by tests and by the spawned background task).
    pub async fn backfill_embeddings(&self) -> Result<()> {
        match &self.embedder {
            Some(emb) => backfill(&self.store, emb).await,
            None => Ok(()),
        }
    }

    /// Stored opencode session id for a scope (None if none).
    pub async fn agent_session(&self, scope: &str) -> Result<Option<String>> {
        self.store.agent_session(scope).await
    }

    /// Complete OpenCode session lifecycle state for a scope.
    pub async fn agent_session_state(&self, scope: &str) -> Result<Option<AgentSessionState>> {
        self.store.agent_session_state(scope).await
    }

    /// Set/overwrite the opencode session id for a scope.
    pub async fn set_agent_session(&self, scope: &str, session_id: &str) -> Result<()> {
        self.store
            .set_agent_session(scope, session_id, now_unix())
            .await
    }

    pub async fn acquire_agent_session_lease(
        &self,
        scope: &str,
        owner: &str,
        lease_secs: i64,
    ) -> Result<Option<AgentSessionState>> {
        self.store
            .acquire_agent_session_lease(scope, owner, now_unix(), lease_secs)
            .await
    }

    pub async fn renew_agent_session_lease(
        &self,
        scope: &str,
        owner: &str,
        lease_secs: i64,
    ) -> Result<bool> {
        self.store
            .renew_agent_session_lease(scope, owner, now_unix(), lease_secs)
            .await
    }

    pub async fn finish_agent_session_turn(
        &self,
        scope: &str,
        owner: &str,
        session_id: Option<&str>,
    ) -> Result<Option<AgentSessionState>> {
        self.store
            .finish_agent_session_turn(scope, owner, session_id, now_unix())
            .await
    }

    pub async fn release_agent_session_lease(&self, scope: &str, owner: &str) -> Result<bool> {
        self.store
            .release_agent_session_lease(scope, owner, now_unix())
            .await
    }

    pub async fn mark_agent_session_compacted(
        &self,
        scope: &str,
        owner: &str,
        session_id: &str,
    ) -> Result<bool> {
        self.store
            .mark_agent_session_compacted(scope, owner, session_id, now_unix())
            .await
    }

    /// Clear the stored opencode session id for a scope.
    pub async fn clear_agent_session(&self, scope: &str) -> Result<()> {
        self.store.clear_agent_session(scope).await
    }

    /// Persist a batch of memories. Returns the new row ids.
    pub async fn remember(&self, input: RememberInput) -> Result<WukongResult<Vec<i64>>> {
        let start = Instant::now();
        if input.items.is_empty() {
            return Err(MemoryError::InvalidQuery(
                "remember requires at least one item".to_string(),
            ));
        }
        if input.items.iter().any(|item| item.text.trim().is_empty()) {
            return Err(MemoryError::InvalidQuery(
                "memory item text is required".to_string(),
            ));
        }

        let scope = Scope::parse(&input.scope)?;
        let scope_str = scope.to_string();
        let now = now_unix();

        if let Some(session_id) = &input.session_id {
            self.store
                .upsert_session(session_id, &scope_str, now)
                .await?;
        }

        let mut ids = Vec::with_capacity(input.items.len());
        for item in &input.items {
            let importance = item.importance.unwrap_or(1.0);
            let (id, inserted) = self
                .store
                .insert_memory(
                    input.session_id.as_deref(),
                    &scope_str,
                    item.kind,
                    &item.text,
                    importance,
                    now,
                    item.dedupe_key.as_deref(),
                )
                .await?;
            ids.push(id);
            if inserted {
                if let Some(emb) = &self.embedder {
                    match embed_blocking(emb, &item.text).await {
                        Ok(v) => {
                            let _ = self
                                .store
                                .update_embedding(id, &embedding_to_blob(&v), emb.model_id())
                                .await;
                        }
                        Err(e) => eprintln!("wukong-memory: embed on remember failed: {e}"),
                    }
                }
                if let Some(sink) = &self.md_sink {
                    if let Err(e) = sink.append(&scope_str, now, item.kind, &item.text) {
                        eprintln!("wukong-memory: markdown append failed: {e}");
                    }
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
        if query.query.trim().is_empty() {
            return Err(MemoryError::InvalidQuery(
                "recall query is required".to_string(),
            ));
        }

        // Adaptive gate: skip trivial queries.
        if is_trivial(&query.query) {
            let latency_ms = start.elapsed().as_millis() as u64;
            let _ = self
                .store
                .insert_recall_telemetry(&RecallTelemetryInput {
                    query_hash: query_hash(&query.query),
                    mode: query.mode,
                    top_k: query.top_k,
                    scope: query.scope.clone(),
                    hit_count: 0,
                    top_relevance: 0.0,
                    skipped: true,
                    latency_ms,
                    created_at: now_unix(),
                })
                .await;
            return Ok(WukongResult {
                data: Vec::new(),
                evidence: Vec::new(),
                confidence: 0.0,
                latency_ms,
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
                Some(expr) => {
                    let hits = self.store.keyword_candidates(&expr, limit).await?;
                    if hits.is_empty() && contains_cjk(&query.query) {
                        self.store
                            .cjk_fallback_candidates(&query.query, limit)
                            .await?
                    } else {
                        hits
                    }
                }
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
                    let qvec = embed_blocking(emb, &query.query).await?;
                    // Score ALL embedded rows by similarity, not just the most
                    // recent — otherwise an older but semantically closer memory
                    // is truncated before ranking. Capped to bound work on very
                    // large stores (ANN indexing is future work).
                    let embedded = self.store.embedded_candidates(MAX_VECTOR_SCAN).await?;
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
        let confidence = scored
            .first()
            .map(|s| s.explanation.relevance.clamp(0.0, 1.0))
            .unwrap_or(0.0);
        let top_relevance = confidence;
        let hit_count = scored.len();
        let hits: Vec<RecallHit> = scored
            .into_iter()
            .map(|s| RecallHit {
                id: s.id,
                scope: s.scope,
                kind: s.kind,
                text: s.text,
                score: s.score,
                explanation: s.explanation,
            })
            .collect();
        let latency_ms = start.elapsed().as_millis() as u64;
        let _ = self
            .store
            .insert_recall_telemetry(&RecallTelemetryInput {
                query_hash: query_hash(&query.query),
                mode: query.mode,
                top_k: query.top_k,
                scope: query.scope.clone(),
                hit_count,
                top_relevance,
                skipped: false,
                latency_ms,
                created_at: now,
            })
            .await;

        Ok(WukongResult {
            data: hits,
            evidence,
            confidence,
            latency_ms,
        })
    }

    /// Aggregate recall telemetry for diagnostics.
    pub async fn recall_telemetry_summary(&self) -> Result<RecallTelemetrySummary> {
        self.store.recall_telemetry_summary().await
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
            let (summary_id, _) = self
                .store
                .insert_memory(
                    None,
                    scope,
                    MemoryKind::Summary,
                    &summary_text,
                    importance,
                    now,
                    None,
                )
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
            if let Some(sink) = &self.md_sink {
                if let Err(e) = sink.append(scope, now, MemoryKind::Summary, &summary_text) {
                    eprintln!("wukong-memory: markdown append failed: {e}");
                }
            }
            summary_ids.push(summary_id);
        }
        Ok(summary_ids)
    }

    /// List distinct scopes containing memory rows.
    pub async fn scopes(&self) -> Result<Vec<String>> {
        self.store.list_scopes().await
    }

    /// Delete only consolidated event/note source rows.
    pub async fn prune_consolidated(&self, scope: Option<&str>) -> Result<u64> {
        self.store.delete_consolidated(scope).await
    }

    /// Ids that `prune` would delete (dry-run).
    pub async fn plan_prune(
        &self,
        scope: Option<&str>,
        policy: &prune::PrunePolicy,
    ) -> Result<Vec<i64>> {
        self.store
            .prune_candidates(
                scope,
                policy.max_age_secs,
                policy.importance_floor,
                now_unix(),
            )
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

    /// Rebuild markdown for every scope from the DB (full overwrite). Ignores
    /// any attached live sink; writes a fresh mirror under `dir`.
    pub async fn export(&self, dir: impl Into<std::path::PathBuf>) -> Result<()> {
        let dir = dir.into();
        std::fs::create_dir_all(&dir)?;
        let rows = self.store.all_for_export().await?;
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        let sink = markdown::MarkdownSink::new(&dir);
        for (scope, created_at, kind, text) in rows {
            if seen.insert(scope.clone()) {
                // First time this scope appears in the rebuild: truncate its file.
                let path = dir.join(markdown::scope_to_filename(&scope));
                let _ = std::fs::write(&path, "");
            }
            sink.append(&scope, created_at, kind, &text)?;
        }
        Ok(())
    }

    /// Compose a health snapshot using default prune thresholds.
    pub async fn snapshot(&self, scope: Option<&str>) -> Result<model::Snapshot> {
        let p = prune::PrunePolicy::default();
        self.store
            .snapshot(scope, now_unix(), p.max_age_secs, p.importance_floor)
            .await
    }

    /// Recent memory rows for Web observability.
    pub async fn records(
        &self,
        scope: Option<&str>,
        kind: Option<MemoryKind>,
        limit: i64,
    ) -> Result<MemoryRecordsPage> {
        let requested = limit.clamp(1, 100);
        let mut records = self.store.list_records(scope, kind, requested + 1).await?;
        let has_more = records.len() as i64 > requested;
        if has_more {
            records.pop();
        }
        Ok(MemoryRecordsPage { records, has_more })
    }

    /// Aggregate statistics.
    pub async fn stats(&self) -> Result<Stats> {
        self.store.stats().await
    }
}

/// Embed and persist vectors for memories lacking them, in batches.
/// Upper bound on embedded rows scored per vector recall. Beyond this the oldest
/// rows are skipped; sized well above a typical personal store so it rarely bites.
const MAX_VECTOR_SCAN: i64 = 10_000;

/// Run a synchronous, CPU-bound embedding off the async executor so it never
/// blocks a tokio worker (ONNX inference can take tens of ms).
async fn embed_blocking(embedder: &Arc<dyn Embedder>, text: &str) -> Result<Vec<f32>> {
    let embedder = embedder.clone();
    let text = text.to_string();
    tokio::task::spawn_blocking(move || embedder.embed(&text))
        .await
        .map_err(|e| MemoryError::Embed(format!("embed task failed: {e}")))?
}

async fn backfill(store: &Store, embedder: &Arc<dyn Embedder>) -> Result<()> {
    loop {
        let batch = store.rows_missing_embedding(32).await?;
        if batch.is_empty() {
            break;
        }
        let texts: Vec<String> = batch.iter().map(|(_, t)| t.clone()).collect();
        let emb = embedder.clone();
        let vecs = tokio::task::spawn_blocking(move || emb.embed_batch(&texts))
            .await
            .map_err(|e| MemoryError::Embed(format!("backfill embed task failed: {e}")))??;
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

    #[tokio::test]
    async fn agent_session_round_trip() {
        let mem = open_mem().await;
        assert_eq!(mem.agent_session("global").await.unwrap(), None);
        mem.set_agent_session("global", "ses_1").await.unwrap();
        assert_eq!(
            mem.agent_session("global").await.unwrap(),
            Some("ses_1".to_string())
        );
        // UPSERT overwrites.
        mem.set_agent_session("global", "ses_2").await.unwrap();
        assert_eq!(
            mem.agent_session("global").await.unwrap(),
            Some("ses_2".to_string())
        );
        // Clear, including a no-op clear of an unknown scope.
        mem.clear_agent_session("global").await.unwrap();
        assert_eq!(mem.agent_session("global").await.unwrap(), None);
        mem.clear_agent_session("nope").await.unwrap();
    }

    #[tokio::test]
    async fn agent_session_state_tracks_turns_and_provenance() {
        let mem = open_mem().await;
        mem.set_agent_session("global", "ses_1").await.unwrap();
        let lease = mem
            .acquire_agent_session_lease("global", "owner-1", 300)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(lease.session_id.as_deref(), Some("ses_1"));
        assert_eq!(lease.turn_count, 0);

        let finished = mem
            .finish_agent_session_turn("global", "owner-1", Some("ses_1"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(finished.turn_count, 1);
        assert!(finished.lease_owner.is_none());

        let state = mem.agent_session_state("global").await.unwrap().unwrap();
        assert_eq!(state.session_id.as_deref(), Some("ses_1"));
        assert_eq!(state.turn_count, 1);
    }

    #[tokio::test]
    async fn memory_records_expose_session_provenance() {
        let mem = open_mem().await;
        mem.remember(RememberInput {
            scope: "project:X".to_string(),
            session_id: Some("ses_1".to_string()),
            items: vec![MemoryItem {
                kind: MemoryKind::Event,
                text: "session event".to_string(),
                importance: None,
                dedupe_key: None,
            }],
        })
        .await
        .unwrap();

        let records = mem
            .store
            .list_records(Some("project:X"), None, 10)
            .await
            .unwrap();
        assert_eq!(records[0].session_id.as_deref(), Some("ses_1"));
    }

    async fn remember_event(mem: &Memory, scope: &str, text: &str) {
        mem.remember(RememberInput {
            scope: scope.to_string(),
            session_id: None,
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
            .consolidate(
                "project:X",
                &ConsolidatePolicy { batch_size: 20 },
                &MockSummarizer,
            )
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
        assert!(recent
            .iter()
            .any(|c| c.kind == MemoryKind::Summary && c.text == "SUMMARY(2)"));
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
        let plan = mem
            .plan_prune(Some("project:X"), &PrunePolicy::default())
            .await
            .unwrap();
        assert_eq!(plan.len(), 2);

        let deleted = mem
            .prune(Some("project:X"), &PrunePolicy::default())
            .await
            .unwrap();
        assert_eq!(deleted, 2);

        // The summary survives (never prunable).
        let recent = mem.store.recent_candidates(10).await.unwrap();
        assert!(recent.iter().all(|c| c.kind == MemoryKind::Summary));
    }

    #[tokio::test]
    async fn remember_mirrors_to_markdown_when_sink_set() {
        let dir = tempfile::tempdir().unwrap();
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        let mem = Memory::open(&url).await.unwrap().with_markdown(dir.path());

        remember_event(&mem, "project:X", "mirrored note").await;

        let body = std::fs::read_to_string(dir.path().join("project_X.md")).unwrap();
        assert!(body.contains("mirrored note"));
    }

    #[tokio::test]
    async fn export_rebuilds_all_scope_files() {
        let dir = tempfile::tempdir().unwrap();
        let mem = open_mem().await; // no sink attached
        remember_event(&mem, "project:X", "x-note").await;
        remember_event(&mem, "global", "g-note").await;

        mem.export(dir.path()).await.unwrap();

        assert!(std::fs::read_to_string(dir.path().join("project_X.md"))
            .unwrap()
            .contains("x-note"));
        assert!(std::fs::read_to_string(dir.path().join("global.md"))
            .unwrap()
            .contains("g-note"));
    }
}
