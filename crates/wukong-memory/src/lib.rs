//! wukong-memory: persistent memory core for the Wukong assistant.

pub mod embed;
pub mod error;
pub mod model;
pub mod recall;
pub mod scope;
pub mod scoring;
pub mod store;

pub use error::{MemoryError, Result};
pub use model::{
    Evidence, MemoryItem, MemoryKind, RecallHit, RecallMode, RecallQuery, RememberInput,
    ScopeCount, Stats, WukongResult,
};
pub use embed::{cosine_similarity, Embedder, MockEmbedder};
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
