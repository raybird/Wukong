//! wukong-memory: persistent memory core for the Wukong assistant.

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
pub use scope::Scope;
pub use scoring::Weights;

use recall::{
    filter_by_scope, fts_match_string, is_trivial, merge_candidates, rank, sources_for_mode,
};
use store::{Candidate, Store};
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

/// The public memory facade. Wraps the store and ranking weights.
pub struct Memory {
    store: Store,
    weights: Weights,
}

impl Memory {
    /// Open (creating if missing) the memory database.
    pub async fn open(db_url: &str) -> Result<Memory> {
        Ok(Memory {
            store: Store::open(db_url).await?,
            weights: Weights::default(),
        })
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
        let (use_keyword, use_recent) = sources_for_mode(query.mode);
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
