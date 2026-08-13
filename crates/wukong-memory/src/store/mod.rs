use crate::error::Result;
use crate::model::{
    AgeBuckets, AgentSessionState, EmbeddingCoverage, KindCount, MemoryKind, MemoryRecord,
    RecallTelemetryInput, RecallTelemetrySummary, ScopeCount, Snapshot, Stats,
};
use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::{Row, SqlitePool};
use std::str::FromStr;

/// Idempotent schema. Applied on every open. External-content FTS5 table is
/// kept in sync by an AFTER INSERT trigger (v1 only inserts).
const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS sessions (
    id         TEXT PRIMARY KEY,
    scope      TEXT NOT NULL,
    project    TEXT,
    created_at INTEGER NOT NULL,
    summary    TEXT
);
CREATE TABLE IF NOT EXISTS memories (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    session_id       TEXT,
    scope            TEXT NOT NULL,
    kind             TEXT NOT NULL,
    text             TEXT NOT NULL,
    created_at       INTEGER NOT NULL,
    last_recalled_at INTEGER,
    recall_count     INTEGER NOT NULL DEFAULT 0,
    importance       REAL NOT NULL DEFAULT 1.0
);
CREATE VIRTUAL TABLE IF NOT EXISTS memories_fts USING fts5(
    text,
    content='memories',
    content_rowid='id'
);
CREATE TRIGGER IF NOT EXISTS memories_ai AFTER INSERT ON memories BEGIN
    INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
END;
CREATE TRIGGER IF NOT EXISTS memories_ad AFTER DELETE ON memories BEGIN
    INSERT INTO memories_fts(memories_fts, rowid, text) VALUES('delete', old.id, old.text);
END;
CREATE TABLE IF NOT EXISTS agent_sessions (
    scope       TEXT PRIMARY KEY,
    session_id  TEXT NOT NULL,
    updated_at  INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS agent_session_state (
    scope              TEXT PRIMARY KEY,
    session_id         TEXT,
    turn_count         INTEGER NOT NULL DEFAULT 0,
    lease_owner        TEXT,
    lease_until        INTEGER,
    last_compacted_at  INTEGER,
    updated_at         INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS recall_telemetry (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    query_hash        TEXT NOT NULL,
    mode              TEXT NOT NULL,
    top_k             INTEGER NOT NULL,
    scope             TEXT,
    hit_count         INTEGER NOT NULL,
    top_relevance     REAL NOT NULL,
    skipped           INTEGER NOT NULL,
    latency_ms        INTEGER NOT NULL,
    created_at        INTEGER NOT NULL
);
"#;

/// A raw row pulled during recall, before scoring.
#[derive(Debug, Clone)]
pub struct Candidate {
    pub id: i64,
    pub scope: String,
    pub kind: MemoryKind,
    pub text: String,
    pub created_at: i64,
    pub recall_count: i64,
    pub importance: f64,
    /// FTS5 bm25 rank (lower = better match); None for non-keyword sources.
    pub bm25: Option<f64>,
    /// Cosine similarity to the query (higher = better); None for non-vector sources.
    pub vector_sim: Option<f64>,
    /// Candidate source names observed before ranking.
    pub source_signals: Vec<String>,
}

/// A row eligible for consolidation (event/note, not yet consolidated).
#[derive(Debug, Clone)]
pub struct ConsolidationRow {
    pub id: i64,
    pub session_id: Option<String>,
    pub text: String,
    pub importance: f64,
}

/// Owns the SQLite connection pool and all SQL.
#[derive(Clone)]
pub struct Store {
    pool: SqlitePool,
}

impl Store {
    /// Open (creating if missing) a SQLite database at `db_url`
    /// (e.g. "sqlite://data/memory.db" or "sqlite::memory:") and apply schema.
    pub async fn open(db_url: &str) -> Result<Store> {
        let opts = SqliteConnectOptions::from_str(db_url)?
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            // WAL already survives a process crash at synchronous=NORMAL; FULL
            // only adds durability against an OS/power loss, at the cost of an
            // fsync per commit. Every turn writes several rows, so that fsync
            // dominates remember() latency.
            .synchronous(SqliteSynchronous::Normal)
            // Let a writer wait for an in-flight write instead of failing with
            // SQLITE_BUSY (background backfill can overlap foreground writes).
            .busy_timeout(std::time::Duration::from_secs(5));
        let pool = SqlitePoolOptions::new().connect_with(opts).await?;
        sqlx::raw_sql(SCHEMA).execute(&pool).await?;
        migrate(&pool).await?;
        Ok(Store { pool })
    }

    /// Insert a session row if it does not already exist.
    pub async fn upsert_session(&self, id: &str, scope: &str, now: i64) -> Result<()> {
        sqlx::query(
            "INSERT INTO sessions (id, scope, created_at) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO NOTHING",
        )
        .bind(id)
        .bind(scope)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Insert one memory and return its row id.
    // Low-level insert helper: each column is an explicit parameter by design.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert_memory(
        &self,
        session_id: Option<&str>,
        scope: &str,
        kind: MemoryKind,
        text: &str,
        importance: f64,
        now: i64,
        dedupe_key: Option<&str>,
    ) -> Result<(i64, bool)> {
        if let Some(key) = dedupe_key {
            if let Some(row) = sqlx::query("SELECT id FROM memories WHERE dedupe_key = ?1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await?
            {
                return Ok((row.get::<i64, _>("id"), false));
            }
        }

        let row = sqlx::query(
            "INSERT INTO memories (session_id, scope, kind, text, created_at, importance, dedupe_key)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             RETURNING id",
        )
        .bind(session_id)
        .bind(scope)
        .bind(kind.as_str())
        .bind(text)
        .bind(now)
        .bind(importance)
        .bind(dedupe_key)
        .fetch_one(&self.pool)
        .await?;
        Ok((row.get::<i64, _>("id"), true))
    }

    /// Keyword candidates ranked by FTS5 bm25 (best first).
    ///
    /// `scopes`, when given, restricts the scan to those scopes (a recall
    /// scope's ancestry). Filtering in SQL rather than after the fact means
    /// `limit` is spent entirely on rows the caller can actually use.
    pub async fn keyword_candidates(
        &self,
        match_expr: &str,
        limit: i64,
        scopes: Option<&[String]>,
    ) -> Result<Vec<Candidate>> {
        let sql = format!(
            "SELECT m.id, m.scope, m.kind, m.text, m.created_at, m.recall_count, m.importance,
                    bm25(memories_fts) AS bm25
             FROM memories_fts
             JOIN memories m ON m.id = memories_fts.rowid
             WHERE memories_fts MATCH ?{}
             ORDER BY bm25 ASC
             LIMIT ?",
            scope_in_clause("m.scope", scopes)
        );
        let mut q = sqlx::query(&sql).bind(match_expr);
        q = bind_scopes(q, scopes);
        let rows = q.bind(limit).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let mut c = row_to_candidate(r);
                c.source_signals.push("keyword".to_string());
                c
            })
            .collect())
    }

    /// Bounded fallback for short CJK queries when FTS5 has no keyword hits.
    pub async fn cjk_fallback_candidates(
        &self,
        query: &str,
        limit: i64,
        scopes: Option<&[String]>,
    ) -> Result<Vec<Candidate>> {
        let pattern = format!("%{}%", query.trim());
        let sql = format!(
            "SELECT id, scope, kind, text, created_at, recall_count, importance,
                    NULL AS bm25
             FROM memories
             WHERE text LIKE ?{}
             ORDER BY created_at DESC
             LIMIT ?",
            scope_in_clause("scope", scopes)
        );
        let mut q = sqlx::query(&sql).bind(pattern);
        q = bind_scopes(q, scopes);
        let rows = q.bind(limit).fetch_all(&self.pool).await?;

        Ok(rows
            .into_iter()
            .map(|r| {
                let mut c = row_to_candidate(r);
                c.source_signals.push("cjk_fallback".to_string());
                c
            })
            .collect())
    }

    /// Most recent memories (tree/recency source). bm25 is None.
    pub async fn recent_candidates(
        &self,
        limit: i64,
        scopes: Option<&[String]>,
    ) -> Result<Vec<Candidate>> {
        let sql = format!(
            "SELECT id, scope, kind, text, created_at, recall_count, importance,
                    NULL AS bm25
             FROM memories
             {}
             ORDER BY created_at DESC
             LIMIT ?",
            scope_where_clause(scopes)
        );
        let mut q = sqlx::query(&sql);
        q = bind_scopes(q, scopes);
        let rows = q.bind(limit).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let mut c = row_to_candidate(r);
                c.source_signals.push("recent".to_string());
                c
            })
            .collect())
    }

    /// Bump recall_count and last_recalled_at for the given ids.
    pub async fn touch_recalled(&self, ids: &[i64], now: i64) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        // Single atomic statement instead of one round-trip per id.
        let placeholders = sql_placeholders(ids.len());
        let sql = format!(
            "UPDATE memories
             SET recall_count = recall_count + 1, last_recalled_at = ?
             WHERE id IN ({placeholders})"
        );
        let mut q = sqlx::query(&sql).bind(now);
        for id in ids {
            q = q.bind(id);
        }
        q.execute(&self.pool).await?;
        Ok(())
    }

    /// Store an embedding blob and its model id for one memory.
    pub async fn update_embedding(&self, id: i64, blob: &[u8], model: &str) -> Result<()> {
        sqlx::query("UPDATE memories SET embedding = ?2, embedding_model = ?3 WHERE id = ?1")
            .bind(id)
            .bind(blob)
            .bind(model)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    /// All memories that have an embedding, paired with the raw little-endian
    /// f32 blob. bm25/vector_sim on the Candidate are None (filled later during
    /// ranking). The blob is handed back undecoded so the caller can score it
    /// with `cosine_similarity_blob` without allocating a vector per row.
    pub async fn embedded_candidates(
        &self,
        limit: i64,
        scopes: Option<&[String]>,
    ) -> Result<Vec<(Candidate, Vec<u8>)>> {
        let sql = format!(
            "SELECT id, scope, kind, text, created_at, recall_count, importance,
                    NULL AS bm25, embedding
             FROM memories
             WHERE embedding IS NOT NULL{}
             ORDER BY created_at DESC
             LIMIT ?",
            scope_in_clause("scope", scopes)
        );
        let mut q = sqlx::query(&sql);
        q = bind_scopes(q, scopes);
        let rows = q.bind(limit).fetch_all(&self.pool).await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let blob: Vec<u8> = r.get::<Vec<u8>, _>("embedding");
                let cand = row_to_candidate(r);
                (cand, blob)
            })
            .collect())
    }

    /// (id, text) for memories still lacking an embedding (backfill source).
    pub async fn rows_missing_embedding(&self, limit: i64) -> Result<Vec<(i64, String)>> {
        let rows = sqlx::query(
            "SELECT id, text FROM memories WHERE embedding IS NULL ORDER BY id ASC LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| (r.get::<i64, _>("id"), r.get::<String, _>("text")))
            .collect())
    }

    /// Foldable rows in one scope: event/note kinds not yet consolidated,
    /// oldest first.
    pub async fn consolidation_candidates(&self, scope: &str) -> Result<Vec<ConsolidationRow>> {
        let rows = sqlx::query(
            "SELECT id, session_id, text, importance
             FROM memories
             WHERE scope = ?1
               AND kind IN ('event','note')
               AND consolidated_into IS NULL
             ORDER BY created_at ASC, id ASC",
        )
        .bind(scope)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| ConsolidationRow {
                id: r.get::<i64, _>("id"),
                session_id: r.get::<Option<String>, _>("session_id"),
                text: r.get::<String, _>("text"),
                importance: r.get::<f64, _>("importance"),
            })
            .collect())
    }

    /// Mark the given rows as folded into `summary_id`.
    pub async fn mark_consolidated(&self, ids: &[i64], summary_id: i64) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let placeholders = sql_placeholders(ids.len());
        let sql = format!("UPDATE memories SET consolidated_into = ? WHERE id IN ({placeholders})");
        let mut q = sqlx::query(&sql).bind(summary_id);
        for id in ids {
            q = q.bind(id);
        }
        q.execute(&self.pool).await?;
        Ok(())
    }

    /// Return distinct memory scopes in stable lexical order.
    pub async fn list_scopes(&self) -> Result<Vec<String>> {
        let rows = sqlx::query("SELECT DISTINCT scope FROM memories ORDER BY scope ASC")
            .fetch_all(&self.pool)
            .await?;
        Ok(rows
            .into_iter()
            .map(|row| row.get::<String, _>("scope"))
            .collect())
    }

    /// Delete only consolidated event/note source rows.
    pub async fn delete_consolidated(&self, scope: Option<&str>) -> Result<u64> {
        let mut sql = String::from(
            "DELETE FROM memories
             WHERE kind IN ('event','note') AND consolidated_into IS NOT NULL",
        );
        if scope.is_some() {
            sql.push_str(" AND scope = ?1");
        }
        let mut query = sqlx::query(&sql);
        if let Some(scope) = scope {
            query = query.bind(scope);
        }
        Ok(query.execute(&self.pool).await?.rows_affected())
    }

    /// Ids eligible for pruning: consolidated rows, OR old + never-recalled +
    /// low-importance event/note rows. Never decision/skill/summary.
    pub async fn prune_candidates(
        &self,
        scope: Option<&str>,
        max_age_secs: i64,
        importance_floor: f64,
        now: i64,
    ) -> Result<Vec<i64>> {
        let sql = format!("SELECT id FROM memories {}", prune_where_clause(scope));
        let rows = self
            .bind_prune_args(&sql, scope, max_age_secs, importance_floor, now)
            .fetch_all(&self.pool)
            .await?;
        Ok(rows.into_iter().map(|r| r.get::<i64, _>("id")).collect())
    }

    /// How many rows `prune_candidates` would return, without materializing them.
    pub async fn count_prune_candidates(
        &self,
        scope: Option<&str>,
        max_age_secs: i64,
        importance_floor: f64,
        now: i64,
    ) -> Result<i64> {
        let sql = format!(
            "SELECT COUNT(*) AS c FROM memories {}",
            prune_where_clause(scope)
        );
        let row = self
            .bind_prune_args(&sql, scope, max_age_secs, importance_floor, now)
            .fetch_one(&self.pool)
            .await?;
        Ok(row.get::<i64, _>("c"))
    }

    fn bind_prune_args<'q>(
        &self,
        sql: &'q str,
        scope: Option<&'q str>,
        max_age_secs: i64,
        importance_floor: f64,
        now: i64,
    ) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
        let mut q = sqlx::query(sql)
            .bind(now - max_age_secs)
            .bind(importance_floor);
        if let Some(s) = scope {
            q = q.bind(s);
        }
        q
    }

    /// Read the stored opencode session id for a scope.
    pub async fn agent_session(&self, scope: &str) -> Result<Option<String>> {
        let row = sqlx::query("SELECT session_id FROM agent_session_state WHERE scope = ?1")
            .bind(scope)
            .fetch_optional(&self.pool)
            .await?;
        Ok(row.and_then(|r| r.get::<Option<String>, _>("session_id")))
    }

    /// Read complete lifecycle state for a scope.
    pub async fn agent_session_state(&self, scope: &str) -> Result<Option<AgentSessionState>> {
        let row = sqlx::query(
            "SELECT scope, session_id, turn_count, lease_owner, lease_until,
                    last_compacted_at, updated_at
             FROM agent_session_state WHERE scope = ?1",
        )
        .bind(scope)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_agent_session_state))
    }

    /// Upsert the opencode session id for a scope.
    pub async fn set_agent_session(&self, scope: &str, session_id: &str, now: i64) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query(
            "INSERT INTO agent_sessions(scope, session_id, updated_at) VALUES (?1, ?2, ?3) \
             ON CONFLICT(scope) DO UPDATE SET session_id = ?2, updated_at = ?3",
        )
        .bind(scope)
        .bind(session_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        sqlx::query(
            "INSERT INTO agent_session_state(scope, session_id, updated_at)
             VALUES (?1, ?2, ?3)
             ON CONFLICT(scope) DO UPDATE SET
                 session_id = excluded.session_id,
                 turn_count = CASE
                     WHEN agent_session_state.session_id = excluded.session_id
                     THEN agent_session_state.turn_count ELSE 0 END,
                 last_compacted_at = CASE
                     WHEN agent_session_state.session_id = excluded.session_id
                     THEN agent_session_state.last_compacted_at ELSE NULL END,
                 lease_owner = NULL,
                 lease_until = NULL,
                 updated_at = excluded.updated_at",
        )
        .bind(scope)
        .bind(session_id)
        .bind(now)
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Acquire an exclusive per-scope session lease.
    pub async fn acquire_agent_session_lease(
        &self,
        scope: &str,
        owner: &str,
        now: i64,
        lease_secs: i64,
    ) -> Result<Option<AgentSessionState>> {
        sqlx::query(
            "INSERT OR IGNORE INTO agent_session_state(scope, session_id, updated_at)
             SELECT scope, session_id, updated_at FROM agent_sessions WHERE scope = ?1",
        )
        .bind(scope)
        .execute(&self.pool)
        .await?;
        sqlx::query(
            "INSERT INTO agent_session_state(scope, updated_at) VALUES (?1, ?2)
             ON CONFLICT(scope) DO NOTHING",
        )
        .bind(scope)
        .bind(now)
        .execute(&self.pool)
        .await?;

        let lease_until = now + lease_secs.max(1);
        let row = sqlx::query(
            "UPDATE agent_session_state
             SET lease_owner = ?2, lease_until = ?3, updated_at = ?4
             WHERE scope = ?1
               AND (lease_until IS NULL OR lease_until <= ?4 OR lease_owner = ?2)
             RETURNING scope, session_id, turn_count, lease_owner, lease_until,
                       last_compacted_at, updated_at",
        )
        .bind(scope)
        .bind(owner)
        .bind(lease_until)
        .bind(now)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_agent_session_state))
    }

    /// Renew an active session lease owned by `owner`.
    pub async fn renew_agent_session_lease(
        &self,
        scope: &str,
        owner: &str,
        now: i64,
        lease_secs: i64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE agent_session_state
             SET lease_until = ?3, updated_at = ?4
             WHERE scope = ?1 AND lease_owner = ?2 AND lease_until > ?4",
        )
        .bind(scope)
        .bind(owner)
        .bind(now + lease_secs.max(1))
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Finish one successful logical turn and release its lease.
    pub async fn finish_agent_session_turn(
        &self,
        scope: &str,
        owner: &str,
        session_id: Option<&str>,
        now: i64,
    ) -> Result<Option<AgentSessionState>> {
        let mut tx = self.pool.begin().await?;
        let row = sqlx::query(
            "UPDATE agent_session_state
             SET session_id = COALESCE(?3, session_id),
                 turn_count = turn_count + 1,
                 lease_owner = NULL,
                 lease_until = NULL,
                 updated_at = ?4
             WHERE scope = ?1 AND lease_owner = ?2 AND lease_until > ?4
             RETURNING scope, session_id, turn_count, lease_owner, lease_until,
                       last_compacted_at, updated_at",
        )
        .bind(scope)
        .bind(owner)
        .bind(session_id)
        .bind(now)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(row) = row else {
            tx.rollback().await?;
            return Ok(None);
        };
        let state = row_to_agent_session_state(row);
        if let Some(session_id) = state.session_id.as_deref() {
            sqlx::query(
                "INSERT INTO agent_sessions(scope, session_id, updated_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(scope) DO UPDATE SET session_id = ?2, updated_at = ?3",
            )
            .bind(scope)
            .bind(session_id)
            .bind(now)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await?;
        Ok(Some(state))
    }

    /// Release an active session lease owned by `owner`.
    pub async fn release_agent_session_lease(
        &self,
        scope: &str,
        owner: &str,
        now: i64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE agent_session_state
             SET lease_owner = NULL, lease_until = NULL, updated_at = ?3
             WHERE scope = ?1 AND lease_owner = ?2",
        )
        .bind(scope)
        .bind(owner)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Mark a successful compaction and reset the session turn count.
    pub async fn mark_agent_session_compacted(
        &self,
        scope: &str,
        owner: &str,
        session_id: &str,
        now: i64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE agent_session_state
             SET turn_count = 0, last_compacted_at = ?4,
                 lease_owner = NULL, lease_until = NULL, updated_at = ?4
             WHERE scope = ?1 AND lease_owner = ?2 AND session_id = ?3 AND lease_until > ?4",
        )
        .bind(scope)
        .bind(owner)
        .bind(session_id)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Consume the compaction budget after a *failed* compaction attempt.
    ///
    /// Resets the turn count so the next attempt is a full `compact_every_turns`
    /// away instead of firing on every subsequent turn. Unlike
    /// `mark_agent_session_compacted` this leaves `last_compacted_at` untouched
    /// (no compaction actually happened) and keeps the caller's lease held,
    /// because the turn continues under the same owner.
    pub async fn defer_agent_session_compaction(
        &self,
        scope: &str,
        owner: &str,
        now: i64,
    ) -> Result<bool> {
        let result = sqlx::query(
            "UPDATE agent_session_state
             SET turn_count = 0, updated_at = ?3
             WHERE scope = ?1 AND lease_owner = ?2 AND lease_until > ?3",
        )
        .bind(scope)
        .bind(owner)
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(result.rows_affected() == 1)
    }

    /// Remove any stored session id for a scope (no-op if absent).
    pub async fn clear_agent_session(&self, scope: &str) -> Result<()> {
        let mut tx = self.pool.begin().await?;
        sqlx::query("DELETE FROM agent_sessions WHERE scope = ?1")
            .bind(scope)
            .execute(&mut *tx)
            .await?;
        sqlx::query("DELETE FROM agent_session_state WHERE scope = ?1")
            .bind(scope)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
        Ok(())
    }

    /// Delete the given memory rows. The AFTER DELETE trigger keeps FTS in sync.
    /// Returns the number of rows removed.
    pub async fn delete_memories(&self, ids: &[i64]) -> Result<u64> {
        if ids.is_empty() {
            return Ok(0);
        }
        let placeholders = sql_placeholders(ids.len());
        let sql = format!("DELETE FROM memories WHERE id IN ({placeholders})");
        let mut q = sqlx::query(&sql);
        for id in ids {
            q = q.bind(id);
        }
        Ok(q.execute(&self.pool).await?.rows_affected())
    }

    /// Compose a full health snapshot. `scope` filters by-kind/age/coverage and
    /// candidate counts to one scope; by_scope/total are always global.
    pub async fn snapshot(
        &self,
        scope: Option<&str>,
        now: i64,
        max_age_secs: i64,
        importance_floor: f64,
    ) -> Result<Snapshot> {
        let base = self.stats().await?; // total + by_scope (global)

        // by_kind (optionally scoped)
        let mut kind_sql = String::from("SELECT kind, COUNT(*) AS c FROM memories");
        if scope.is_some() {
            kind_sql.push_str(" WHERE scope = ?1");
        }
        kind_sql.push_str(" GROUP BY kind ORDER BY c DESC");
        let mut kq = sqlx::query(&kind_sql);
        if let Some(s) = scope {
            kq = kq.bind(s);
        }
        let by_kind = kq
            .fetch_all(&self.pool)
            .await?
            .into_iter()
            .map(|r| KindCount {
                kind: MemoryKind::from_db_str(&r.get::<String, _>("kind")),
                count: r.get::<i64, _>("c"),
            })
            .collect();

        // age buckets
        let day = now - 86_400;
        let week = now - 7 * 86_400;
        let month = now - 30 * 86_400;
        let mut age_sql = String::from(
            "SELECT
                 SUM(CASE WHEN created_at >= ?1 THEN 1 ELSE 0 END) AS d,
                 SUM(CASE WHEN created_at >= ?2 AND created_at < ?1 THEN 1 ELSE 0 END) AS w,
                 SUM(CASE WHEN created_at >= ?3 AND created_at < ?2 THEN 1 ELSE 0 END) AS m,
                 SUM(CASE WHEN created_at < ?3 THEN 1 ELSE 0 END) AS o
             FROM memories",
        );
        if scope.is_some() {
            age_sql.push_str(" WHERE scope = ?4");
        }
        let mut aq = sqlx::query(&age_sql).bind(day).bind(week).bind(month);
        if let Some(s) = scope {
            aq = aq.bind(s);
        }
        let ar = aq.fetch_one(&self.pool).await?;
        let age = AgeBuckets {
            last_day: ar.get::<Option<i64>, _>("d").unwrap_or(0),
            last_week: ar.get::<Option<i64>, _>("w").unwrap_or(0),
            last_month: ar.get::<Option<i64>, _>("m").unwrap_or(0),
            older: ar.get::<Option<i64>, _>("o").unwrap_or(0),
        };

        // embedding coverage
        let mut cov_sql = String::from(
            "SELECT COUNT(*) AS total, SUM(CASE WHEN embedding IS NOT NULL THEN 1 ELSE 0 END) AS emb
             FROM memories",
        );
        if scope.is_some() {
            cov_sql.push_str(" WHERE scope = ?1");
        }
        let mut cq = sqlx::query(&cov_sql);
        if let Some(s) = scope {
            cq = cq.bind(s);
        }
        let cr = cq.fetch_one(&self.pool).await?;
        let embedding = EmbeddingCoverage {
            embedded: cr.get::<Option<i64>, _>("emb").unwrap_or(0),
            total: cr.get::<i64, _>("total"),
        };

        // candidate counts
        let mut cons_sql = String::from(
            "SELECT COUNT(*) AS c FROM memories
             WHERE kind IN ('event','note') AND consolidated_into IS NULL",
        );
        if scope.is_some() {
            cons_sql.push_str(" AND scope = ?1");
        }
        let mut consq = sqlx::query(&cons_sql);
        if let Some(s) = scope {
            consq = consq.bind(s);
        }
        let consolidation_candidates = consq.fetch_one(&self.pool).await?.get::<i64, _>("c");

        let prune_candidates = self
            .count_prune_candidates(scope, max_age_secs, importance_floor, now)
            .await?;

        Ok(Snapshot {
            total: base.total,
            by_scope: base.by_scope,
            by_kind,
            age,
            embedding,
            consolidation_candidates,
            prune_candidates,
        })
    }

    /// Recent memory rows for observability. Ordered newest-first.
    pub async fn list_records(
        &self,
        scope: Option<&str>,
        kind: Option<MemoryKind>,
        limit: i64,
    ) -> Result<Vec<MemoryRecord>> {
        let limit = limit.clamp(1, 101);
        let rows = match (scope, kind) {
            (Some(scope), Some(kind)) => {
                sqlx::query(
                    "SELECT id, scope, kind, text, importance, created_at, last_recalled_at,
                            session_id, recall_count,
                            embedding IS NOT NULL AS has_embedding, consolidated_into
                     FROM memories
                     WHERE scope = ?1 AND kind = ?2
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?3",
                )
                .bind(scope)
                .bind(kind.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (Some(scope), None) => {
                sqlx::query(
                    "SELECT id, scope, kind, text, importance, created_at, last_recalled_at,
                            session_id, recall_count,
                            embedding IS NOT NULL AS has_embedding, consolidated_into
                     FROM memories
                     WHERE scope = ?1
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?2",
                )
                .bind(scope)
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (None, Some(kind)) => {
                sqlx::query(
                    "SELECT id, scope, kind, text, importance, created_at, last_recalled_at,
                            session_id, recall_count,
                            embedding IS NOT NULL AS has_embedding, consolidated_into
                     FROM memories
                     WHERE kind = ?1
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?2",
                )
                .bind(kind.as_str())
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
            (None, None) => {
                sqlx::query(
                    "SELECT id, scope, kind, text, importance, created_at, last_recalled_at,
                            session_id, recall_count,
                            embedding IS NOT NULL AS has_embedding, consolidated_into
                     FROM memories
                     ORDER BY created_at DESC, id DESC
                     LIMIT ?1",
                )
                .bind(limit)
                .fetch_all(&self.pool)
                .await?
            }
        };

        Ok(rows.into_iter().map(row_to_memory_record).collect())
    }

    /// All memories ordered oldest-first, for full markdown export.
    /// Returns (scope, created_at, kind, text).
    pub async fn all_for_export(&self) -> Result<Vec<(String, i64, MemoryKind, String)>> {
        let rows = sqlx::query(
            "SELECT scope, created_at, kind, text FROM memories ORDER BY created_at ASC, id ASC",
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                (
                    r.get::<String, _>("scope"),
                    r.get::<i64, _>("created_at"),
                    MemoryKind::from_db_str(&r.get::<String, _>("kind")),
                    r.get::<String, _>("text"),
                )
            })
            .collect())
    }

    /// Record one recall query for aggregate diagnostics. Query text is hashed by caller.
    pub async fn insert_recall_telemetry(&self, input: &RecallTelemetryInput) -> Result<()> {
        sqlx::query(
            "INSERT INTO recall_telemetry
             (query_hash, mode, top_k, scope, hit_count, top_relevance, skipped, latency_ms, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        )
        .bind(&input.query_hash)
        .bind(input.mode.as_str())
        .bind(input.top_k as i64)
        .bind(input.scope.as_deref())
        .bind(input.hit_count as i64)
        .bind(input.top_relevance)
        .bind(if input.skipped { 1_i64 } else { 0_i64 })
        .bind(input.latency_ms as i64)
        .bind(input.created_at)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Aggregate recall telemetry for diagnostics.
    pub async fn recall_telemetry_summary(&self) -> Result<RecallTelemetrySummary> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS total_queries,
                    COALESCE(SUM(skipped), 0) AS skipped_queries,
                    COALESCE(AVG(CASE WHEN skipped = 0 THEN top_relevance END), 0.0) AS avg_top_relevance
             FROM recall_telemetry",
        )
        .fetch_one(&self.pool)
        .await?;
        Ok(RecallTelemetrySummary {
            total_queries: row.get::<i64, _>("total_queries"),
            skipped_queries: row.get::<i64, _>("skipped_queries"),
            avg_top_relevance: row.get::<f64, _>("avg_top_relevance"),
        })
    }

    /// Total memory count and per-scope breakdown.
    pub async fn stats(&self) -> Result<Stats> {
        let total: i64 = sqlx::query("SELECT COUNT(*) AS c FROM memories")
            .fetch_one(&self.pool)
            .await?
            .get::<i64, _>("c");
        let rows =
            sqlx::query("SELECT scope, COUNT(*) AS c FROM memories GROUP BY scope ORDER BY c DESC")
                .fetch_all(&self.pool)
                .await?;
        let by_scope = rows
            .into_iter()
            .map(|r| ScopeCount {
                scope: r.get::<String, _>("scope"),
                count: r.get::<i64, _>("c"),
            })
            .collect();
        Ok(Stats { total, by_scope })
    }
}

/// Idempotently add v2 embedding columns to an existing `memories` table.
/// Safe to run on every open: checks PRAGMA table_info before ALTER.
async fn migrate(pool: &SqlitePool) -> Result<()> {
    let cols = sqlx::query("PRAGMA table_info(memories)")
        .fetch_all(pool)
        .await?;
    let names: Vec<String> = cols.iter().map(|r| r.get::<String, _>("name")).collect();
    if !names.iter().any(|n| n == "embedding") {
        sqlx::query("ALTER TABLE memories ADD COLUMN embedding BLOB")
            .execute(pool)
            .await?;
    }
    if !names.iter().any(|n| n == "embedding_model") {
        sqlx::query("ALTER TABLE memories ADD COLUMN embedding_model TEXT")
            .execute(pool)
            .await?;
    }
    if !names.iter().any(|n| n == "consolidated_into") {
        sqlx::query("ALTER TABLE memories ADD COLUMN consolidated_into INTEGER")
            .execute(pool)
            .await?;
    }
    if !names.iter().any(|n| n == "dedupe_key") {
        sqlx::query("ALTER TABLE memories ADD COLUMN dedupe_key TEXT")
            .execute(pool)
            .await?;
    }
    sqlx::query(
        "CREATE UNIQUE INDEX IF NOT EXISTS memories_dedupe_key_idx
         ON memories(dedupe_key)
         WHERE dedupe_key IS NOT NULL",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "INSERT OR IGNORE INTO agent_session_state(scope, session_id, updated_at)
         SELECT scope, session_id, updated_at FROM agent_sessions",
    )
    .execute(pool)
    .await?;
    create_indexes(pool).await?;
    Ok(())
}

/// Indexes for the recall/backfill/observability hot paths. Created after
/// `migrate` has added the v2 columns, because two of them are partial indexes
/// over `embedding`.
async fn create_indexes(pool: &SqlitePool) -> Result<()> {
    // `recent_candidates`: ORDER BY created_at DESC LIMIT n.
    sqlx::query("CREATE INDEX IF NOT EXISTS memories_created_at_idx ON memories(created_at DESC)")
        .execute(pool)
        .await?;
    // `list_records` / `consolidation_candidates` / `stats`: filter or group by scope.
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS memories_scope_created_at_idx
         ON memories(scope, created_at DESC, id DESC)",
    )
    .execute(pool)
    .await?;
    // `embedded_candidates`: the vector-recall scan, newest first.
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS memories_embedded_idx
         ON memories(created_at DESC)
         WHERE embedding IS NOT NULL",
    )
    .execute(pool)
    .await?;
    // `rows_missing_embedding`: without this the backfill loop re-scans an
    // ever-longer prefix of already-embedded rows to find each next batch.
    sqlx::query(
        "CREATE INDEX IF NOT EXISTS memories_missing_embedding_idx
         ON memories(id)
         WHERE embedding IS NULL",
    )
    .execute(pool)
    .await?;
    Ok(())
}

fn row_to_agent_session_state(r: sqlx::sqlite::SqliteRow) -> AgentSessionState {
    AgentSessionState {
        scope: r.get::<String, _>("scope"),
        session_id: r.get::<Option<String>, _>("session_id"),
        turn_count: r.get::<i64, _>("turn_count"),
        lease_owner: r.get::<Option<String>, _>("lease_owner"),
        lease_until: r.get::<Option<i64>, _>("lease_until"),
        last_compacted_at: r.get::<Option<i64>, _>("last_compacted_at"),
        updated_at: r.get::<i64, _>("updated_at"),
    }
}

/// Comma-separated `?` placeholders for an `IN (…)` clause with `n` binds.
fn sql_placeholders(n: usize) -> String {
    vec!["?"; n].join(",")
}

/// Shared `WHERE` for the prune id-list and its count. Binds, in order:
/// created-at cutoff, importance floor, and the scope when one is given.
fn prune_where_clause(scope: Option<&str>) -> String {
    let mut sql = String::from(
        "WHERE (
             consolidated_into IS NOT NULL
             OR (kind IN ('event','note') AND created_at < ? AND recall_count = 0 AND importance < ?)
         )",
    );
    if scope.is_some() {
        sql.push_str(" AND scope = ?");
    }
    sql
}

/// ` AND <column> IN (?,…)` for a scope allow-list, or empty when unfiltered.
/// Pair every call with `bind_scopes` on the same query, in the same order.
fn scope_in_clause(column: &str, scopes: Option<&[String]>) -> String {
    match scopes {
        Some(s) if !s.is_empty() => {
            format!(" AND {column} IN ({})", sql_placeholders(s.len()))
        }
        _ => String::new(),
    }
}

/// Same allow-list as `scope_in_clause` but as a standalone `WHERE`.
fn scope_where_clause(scopes: Option<&[String]>) -> String {
    match scopes {
        Some(s) if !s.is_empty() => format!("WHERE scope IN ({})", sql_placeholders(s.len())),
        _ => String::new(),
    }
}

fn bind_scopes<'q>(
    mut q: sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>>,
    scopes: Option<&'q [String]>,
) -> sqlx::query::Query<'q, sqlx::Sqlite, sqlx::sqlite::SqliteArguments<'q>> {
    if let Some(scopes) = scopes {
        for scope in scopes {
            q = q.bind(scope);
        }
    }
    q
}

fn row_to_candidate(r: sqlx::sqlite::SqliteRow) -> Candidate {
    Candidate {
        id: r.get::<i64, _>("id"),
        scope: r.get::<String, _>("scope"),
        kind: MemoryKind::from_db_str(&r.get::<String, _>("kind")),
        text: r.get::<String, _>("text"),
        created_at: r.get::<i64, _>("created_at"),
        recall_count: r.get::<i64, _>("recall_count"),
        importance: r.get::<f64, _>("importance"),
        bm25: r.get::<Option<f64>, _>("bm25"),
        vector_sim: None,
        source_signals: Vec::new(),
    }
}

fn row_to_memory_record(r: sqlx::sqlite::SqliteRow) -> MemoryRecord {
    MemoryRecord {
        id: r.get::<i64, _>("id"),
        scope: r.get::<String, _>("scope"),
        session_id: r.get::<Option<String>, _>("session_id"),
        kind: MemoryKind::from_db_str(&r.get::<String, _>("kind")),
        text: r.get::<String, _>("text"),
        importance: r.get::<f64, _>("importance"),
        created_at: r.get::<i64, _>("created_at"),
        last_recalled_at: r.get::<Option<i64>, _>("last_recalled_at"),
        recall_count: r.get::<i64, _>("recall_count"),
        has_embedding: r.get::<i64, _>("has_embedding") != 0,
        consolidated_into: r.get::<Option<i64>, _>("consolidated_into"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    async fn test_store() -> Store {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        // Leak the temp file handle so it lives for the whole test process.
        std::mem::forget(file);
        Store::open(&url).await.unwrap()
    }

    #[tokio::test]
    async fn fts5_is_available() {
        // Fails loudly if the bundled sqlite lacks FTS5.
        let store = test_store().await;
        store
            .insert_memory(
                None,
                "global",
                MemoryKind::Note,
                "hello world",
                1.0,
                100,
                None,
            )
            .await
            .unwrap();
        let hits = store.keyword_candidates("\"hello\"", 10, None).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].bm25.is_some());
    }

    #[tokio::test]
    async fn insert_and_recent() {
        let store = test_store().await;
        store
            .insert_memory(None, "global", MemoryKind::Note, "first", 1.0, 100, None)
            .await
            .unwrap();
        store
            .insert_memory(None, "global", MemoryKind::Note, "second", 1.0, 200, None)
            .await
            .unwrap();
        let recent = store.recent_candidates(10, None).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].text, "second"); // newest first
        assert!(recent[0].bm25.is_none());
    }

    #[tokio::test]
    async fn stats_counts_by_scope() {
        let store = test_store().await;
        store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100, None)
            .await
            .unwrap();
        store
            .insert_memory(None, "project:X", MemoryKind::Note, "b", 1.0, 100, None)
            .await
            .unwrap();
        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.by_scope.len(), 2);
    }

    #[tokio::test]
    async fn list_records_filters_scope_kind_and_limit() {
        let store = test_store().await;
        let now = 1_700_000_000;

        store
            .insert_memory(
                None,
                "global",
                MemoryKind::Note,
                "global note",
                0.7,
                now,
                None,
            )
            .await
            .unwrap();
        store
            .insert_memory(
                None,
                "project:Wukong",
                MemoryKind::Decision,
                "keep this",
                1.0,
                now + 1,
                None,
            )
            .await
            .unwrap();
        store
            .insert_memory(
                None,
                "project:Wukong",
                MemoryKind::Note,
                "skip kind",
                0.4,
                now + 2,
                None,
            )
            .await
            .unwrap();

        let rows = store
            .list_records(Some("project:Wukong"), Some(MemoryKind::Decision), 10)
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].scope, "project:Wukong");
        assert_eq!(rows[0].kind, MemoryKind::Decision);
        assert_eq!(rows[0].text, "keep this");
    }

    #[tokio::test]
    async fn touch_recalled_bumps_count() {
        let store = test_store().await;
        let id = store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100, None)
            .await
            .unwrap()
            .0;
        store.touch_recalled(&[id], 500).await.unwrap();
        let recent = store.recent_candidates(1, None).await.unwrap();
        assert_eq!(recent[0].recall_count, 1);
    }

    /// The scope allow-list is applied in SQL, so `limit` is spent inside the
    /// scope. With the filter applied after the fetch instead, the 60 newer
    /// out-of-scope rows below would consume the whole window and the caller
    /// would see nothing.
    #[tokio::test]
    async fn recent_candidates_spend_the_limit_inside_the_scope() {
        let store = test_store().await;
        store
            .insert_memory(None, "project:Alpha", MemoryKind::Note, "alpha", 1.0, 100, None)
            .await
            .unwrap();
        for i in 0..60 {
            store
                .insert_memory(
                    None,
                    "project:Beta",
                    MemoryKind::Note,
                    "beta",
                    1.0,
                    200 + i,
                    None,
                )
                .await
                .unwrap();
        }

        let scoped = vec!["project:Alpha".to_string()];
        let rows = store.recent_candidates(50, Some(&scoped)).await.unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "alpha");
        // Unfiltered, the same limit returns only the newer Beta rows.
        let all = store.recent_candidates(50, None).await.unwrap();
        assert_eq!(all.len(), 50);
        assert!(all.iter().all(|c| c.scope == "project:Beta"));
    }

    #[tokio::test]
    async fn keyword_candidates_honour_the_scope_filter() {
        let store = test_store().await;
        store
            .insert_memory(None, "project:Alpha", MemoryKind::Note, "shared term", 1.0, 100, None)
            .await
            .unwrap();
        store
            .insert_memory(None, "project:Beta", MemoryKind::Note, "shared term", 1.0, 200, None)
            .await
            .unwrap();

        let scoped = vec!["project:Alpha".to_string()];
        let rows = store
            .keyword_candidates("\"shared\"", 50, Some(&scoped))
            .await
            .unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].scope, "project:Alpha");
    }

    /// Vector recall scans every embedded row, so restricting that scan to the
    /// caller's scope is where the filter saves the most work.
    #[tokio::test]
    async fn embedded_candidates_honour_the_scope_filter() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let a = store
            .insert_memory(None, "project:Alpha", MemoryKind::Note, "a", 1.0, 100, None)
            .await
            .unwrap()
            .0;
        let b = store
            .insert_memory(None, "project:Beta", MemoryKind::Note, "b", 1.0, 200, None)
            .await
            .unwrap()
            .0;
        for id in [a, b] {
            store
                .update_embedding(id, &embedding_to_blob(&[1.0f32, 0.0]), "mock")
                .await
                .unwrap();
        }

        let scoped = vec!["project:Alpha".to_string()];
        let rows = store.embedded_candidates(50, Some(&scoped)).await.unwrap();

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].0.scope, "project:Alpha");
        assert_eq!(store.embedded_candidates(50, None).await.unwrap().len(), 2);
    }

    #[tokio::test]
    async fn count_prune_candidates_matches_the_id_list() {
        let store = test_store().await;
        let now = 1_000_000_000i64;
        let old = now - 40 * 86_400;
        let a = store
            .insert_memory(None, "project:X", MemoryKind::Event, "a", 1.0, old, None)
            .await
            .unwrap()
            .0;
        store.mark_consolidated(&[a], 999).await.unwrap();
        store
            .insert_memory(None, "project:X", MemoryKind::Note, "b", 0.2, old, None)
            .await
            .unwrap();
        store
            .insert_memory(None, "project:X", MemoryKind::Note, "c", 0.9, old, None)
            .await
            .unwrap();

        for scope in [Some("project:X"), None] {
            let ids = store
                .prune_candidates(scope, 30 * 86_400, 0.5, now)
                .await
                .unwrap();
            let count = store
                .count_prune_candidates(scope, 30 * 86_400, 0.5, now)
                .await
                .unwrap();
            assert_eq!(count, ids.len() as i64);
        }
    }

    #[tokio::test]
    async fn migrate_adds_embedding_columns() {
        let store = test_store().await;
        let cols = sqlx::query("PRAGMA table_info(memories)")
            .fetch_all(&store.pool)
            .await
            .unwrap();
        let names: Vec<String> = cols.iter().map(|r| r.get::<String, _>("name")).collect();
        assert!(names.iter().any(|n| n == "embedding"));
        assert!(names.iter().any(|n| n == "embedding_model"));
    }

    #[tokio::test]
    async fn migrate_is_idempotent() {
        let file = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", file.path().display());
        std::mem::forget(file);
        // Open twice: second open re-runs migrate over already-migrated table.
        let _first = Store::open(&url).await.unwrap();
        let _second = Store::open(&url).await.unwrap();
    }

    #[tokio::test]
    async fn session_lease_is_exclusive_and_expiry_is_reclaimable() {
        let store = test_store().await;
        store
            .set_agent_session("global", "ses_1", 100)
            .await
            .unwrap();

        assert!(store
            .acquire_agent_session_lease("global", "owner-1", 100, 10)
            .await
            .unwrap()
            .is_some());
        assert!(store
            .acquire_agent_session_lease("global", "owner-2", 105, 10)
            .await
            .unwrap()
            .is_none());
        assert!(!store
            .renew_agent_session_lease("global", "owner-2", 106, 10)
            .await
            .unwrap());
        assert!(store
            .renew_agent_session_lease("global", "owner-1", 106, 10)
            .await
            .unwrap());
        assert!(store
            .finish_agent_session_turn("global", "owner-2", Some("ses_2"), 107)
            .await
            .unwrap()
            .is_none());
        assert!(store
            .acquire_agent_session_lease("global", "owner-2", 117, 10)
            .await
            .unwrap()
            .is_some());
    }

    #[tokio::test]
    async fn update_and_read_embedding() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let id = store
            .insert_memory(None, "global", MemoryKind::Note, "vec me", 1.0, 100, None)
            .await
            .unwrap()
            .0;
        let v = vec![0.1f32, 0.2, 0.3];
        store
            .update_embedding(id, &embedding_to_blob(&v), "mock")
            .await
            .unwrap();
        let embedded = store.embedded_candidates(10, None).await.unwrap();
        assert_eq!(embedded.len(), 1);
        assert_eq!(embedded[0].0.id, id);
        // The blob comes back undecoded; it must round-trip to the original vector.
        assert_eq!(crate::embed::blob_to_embedding(&embedded[0].1), v);
    }

    #[tokio::test]
    async fn rows_missing_embedding_excludes_embedded() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let a = store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100, None)
            .await
            .unwrap()
            .0;
        let _b = store
            .insert_memory(None, "global", MemoryKind::Note, "b", 1.0, 100, None)
            .await
            .unwrap();
        store
            .update_embedding(a, &embedding_to_blob(&[1.0f32]), "mock")
            .await
            .unwrap();
        let missing = store.rows_missing_embedding(10).await.unwrap();
        assert_eq!(missing.len(), 1);
        assert_eq!(missing[0].1, "b");
    }

    #[tokio::test]
    async fn consolidation_candidates_excludes_consolidated_and_nonfoldable() {
        let store = test_store().await;
        let e1 = store
            .insert_memory(
                Some("s1"),
                "project:X",
                MemoryKind::Event,
                "e1",
                1.0,
                100,
                None,
            )
            .await
            .unwrap()
            .0;
        let _e2 = store
            .insert_memory(None, "project:X", MemoryKind::Note, "n1", 2.0, 110, None)
            .await
            .unwrap();
        // Decision is never foldable.
        let _d = store
            .insert_memory(
                None,
                "project:X",
                MemoryKind::Decision,
                "d1",
                1.0,
                120,
                None,
            )
            .await
            .unwrap();
        // Different scope must not appear.
        let _o = store
            .insert_memory(None, "global", MemoryKind::Event, "other", 1.0, 130, None)
            .await
            .unwrap();
        // Mark e1 as already consolidated.
        store.mark_consolidated(&[e1], 999).await.unwrap();

        let rows = store.consolidation_candidates("project:X").await.unwrap();
        // Only the unconsolidated Note (n1) remains.
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].text, "n1");
        assert_eq!(rows[0].importance, 2.0);
    }

    #[tokio::test]
    async fn snapshot_reports_counts() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let now = 1_000_000_000i64;
        let old = now - 40 * 86_400;
        let e1 = store
            .insert_memory(None, "project:X", MemoryKind::Event, "e1", 1.0, now, None)
            .await
            .unwrap()
            .0;
        let _n1 = store
            .insert_memory(None, "project:X", MemoryKind::Note, "n1", 0.2, old, None)
            .await
            .unwrap();
        let _d1 = store
            .insert_memory(
                None,
                "project:X",
                MemoryKind::Decision,
                "d1",
                1.0,
                now,
                None,
            )
            .await
            .unwrap();
        store
            .update_embedding(e1, &embedding_to_blob(&[0.1f32]), "mock")
            .await
            .unwrap();

        let snap = store.snapshot(None, now, 30 * 86_400, 0.5).await.unwrap();
        assert_eq!(snap.total, 3);
        // by_kind has event/note/decision.
        assert_eq!(snap.by_kind.iter().map(|k| k.count).sum::<i64>(), 3);
        // age: e1 and d1 are "today", n1 is "older".
        assert_eq!(snap.age.last_day, 2);
        assert_eq!(snap.age.older, 1);
        // 1 of 3 embedded.
        assert_eq!(snap.embedding.embedded, 1);
        assert_eq!(snap.embedding.total, 3);
        // consolidation candidates: e1 + n1 (event/note, unconsolidated) = 2.
        assert_eq!(snap.consolidation_candidates, 2);
        // prune candidates: old low-value n1 = 1.
        assert_eq!(snap.prune_candidates, 1);
    }

    #[tokio::test]
    async fn prune_candidates_matches_consolidated_or_low_value() {
        let store = test_store().await;
        let now = 1_000_000_000i64;
        let old = now - 40 * 86_400; // 40 days old
                                     // (a) consolidated event => prunable via main path.
        let a = store
            .insert_memory(None, "project:X", MemoryKind::Event, "a", 1.0, old, None)
            .await
            .unwrap()
            .0;
        store.mark_consolidated(&[a], 999).await.unwrap();
        // (b) old, never recalled, low importance note => prunable via fallback.
        let b = store
            .insert_memory(None, "project:X", MemoryKind::Note, "b", 0.2, old, None)
            .await
            .unwrap()
            .0;
        // (c) old but high importance => NOT prunable.
        let _c = store
            .insert_memory(None, "project:X", MemoryKind::Note, "c", 0.9, old, None)
            .await
            .unwrap();
        // (d) decision is never prunable, even if old/low.
        let _d = store
            .insert_memory(None, "project:X", MemoryKind::Decision, "d", 0.1, old, None)
            .await
            .unwrap();
        // (e) recent low-importance note => NOT prunable (too new).
        let _e = store
            .insert_memory(None, "project:X", MemoryKind::Note, "e", 0.1, now, None)
            .await
            .unwrap();

        let ids = store
            .prune_candidates(Some("project:X"), 30 * 86_400, 0.5, now)
            .await
            .unwrap();
        let mut ids = ids;
        ids.sort();
        assert_eq!(ids, vec![a, b]);
    }

    #[tokio::test]
    async fn delete_memories_removes_rows_and_fts() {
        let store = test_store().await;
        let id = store
            .insert_memory(None, "global", MemoryKind::Note, "gizmo", 1.0, 100, None)
            .await
            .unwrap()
            .0;
        let n = store.delete_memories(&[id]).await.unwrap();
        assert_eq!(n, 1);
        assert_eq!(store.recent_candidates(10, None).await.unwrap().len(), 0);
        assert_eq!(
            store
                .keyword_candidates("\"gizmo\"", 10, None)
                .await
                .unwrap()
                .len(),
            0
        );
    }

    #[tokio::test]
    async fn migrate_adds_consolidated_into_column() {
        let store = test_store().await;
        let cols = sqlx::query("PRAGMA table_info(memories)")
            .fetch_all(&store.pool)
            .await
            .unwrap();
        let names: Vec<String> = cols.iter().map(|r| r.get::<String, _>("name")).collect();
        assert!(names.iter().any(|n| n == "consolidated_into"));
    }

    #[tokio::test]
    async fn delete_keeps_fts_in_sync() {
        let store = test_store().await;
        let id = store
            .insert_memory(
                None,
                "global",
                MemoryKind::Note,
                "deletable widget",
                1.0,
                100,
                None,
            )
            .await
            .unwrap()
            .0;
        assert_eq!(
            store
                .keyword_candidates("\"widget\"", 10, None)
                .await
                .unwrap()
                .len(),
            1
        );
        sqlx::query("DELETE FROM memories WHERE id = ?1")
            .bind(id)
            .execute(&store.pool)
            .await
            .unwrap();
        // FTS index must no longer match the deleted row.
        assert_eq!(
            store
                .keyword_candidates("\"widget\"", 10, None)
                .await
                .unwrap()
                .len(),
            0
        );
    }
}
