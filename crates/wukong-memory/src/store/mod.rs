use crate::embed::blob_to_embedding;
use crate::error::Result;
use crate::model::{MemoryKind, ScopeCount, Stats};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions};
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
            .journal_mode(SqliteJournalMode::Wal);
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
    pub async fn insert_memory(
        &self,
        session_id: Option<&str>,
        scope: &str,
        kind: MemoryKind,
        text: &str,
        importance: f64,
        now: i64,
    ) -> Result<i64> {
        let row = sqlx::query(
            "INSERT INTO memories (session_id, scope, kind, text, created_at, importance)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
        )
        .bind(session_id)
        .bind(scope)
        .bind(kind.as_str())
        .bind(text)
        .bind(now)
        .bind(importance)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get::<i64, _>("id"))
    }

    /// Keyword candidates ranked by FTS5 bm25 (best first).
    pub async fn keyword_candidates(
        &self,
        match_expr: &str,
        limit: i64,
    ) -> Result<Vec<Candidate>> {
        let rows = sqlx::query(
            "SELECT m.id, m.scope, m.kind, m.text, m.created_at, m.recall_count, m.importance,
                    bm25(memories_fts) AS bm25
             FROM memories_fts
             JOIN memories m ON m.id = memories_fts.rowid
             WHERE memories_fts MATCH ?1
             ORDER BY bm25 ASC
             LIMIT ?2",
        )
        .bind(match_expr)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_candidate).collect())
    }

    /// Most recent memories (tree/recency source). bm25 is None.
    pub async fn recent_candidates(&self, limit: i64) -> Result<Vec<Candidate>> {
        let rows = sqlx::query(
            "SELECT id, scope, kind, text, created_at, recall_count, importance,
                    NULL AS bm25
             FROM memories
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_candidate).collect())
    }

    /// Bump recall_count and last_recalled_at for the given ids.
    pub async fn touch_recalled(&self, ids: &[i64], now: i64) -> Result<()> {
        for id in ids {
            sqlx::query(
                "UPDATE memories
                 SET recall_count = recall_count + 1, last_recalled_at = ?2
                 WHERE id = ?1",
            )
            .bind(id)
            .bind(now)
            .execute(&self.pool)
            .await?;
        }
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

    /// All memories that have an embedding, paired with the decoded vector.
    /// bm25/vector_sim on the Candidate are None (filled later during ranking).
    pub async fn embedded_candidates(&self, limit: i64) -> Result<Vec<(Candidate, Vec<f32>)>> {
        let rows = sqlx::query(
            "SELECT id, scope, kind, text, created_at, recall_count, importance,
                    NULL AS bm25, embedding
             FROM memories
             WHERE embedding IS NOT NULL
             ORDER BY created_at DESC
             LIMIT ?1",
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows
            .into_iter()
            .map(|r| {
                let blob: Vec<u8> = r.get::<Vec<u8>, _>("embedding");
                let cand = row_to_candidate(r);
                (cand, blob_to_embedding(&blob))
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

    /// Total memory count and per-scope breakdown.
    pub async fn stats(&self) -> Result<Stats> {
        let total: i64 = sqlx::query("SELECT COUNT(*) AS c FROM memories")
            .fetch_one(&self.pool)
            .await?
            .get::<i64, _>("c");
        let rows = sqlx::query(
            "SELECT scope, COUNT(*) AS c FROM memories GROUP BY scope ORDER BY c DESC",
        )
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
    Ok(())
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
            .insert_memory(None, "global", MemoryKind::Note, "hello world", 1.0, 100)
            .await
            .unwrap();
        let hits = store.keyword_candidates("\"hello\"", 10).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].bm25.is_some());
    }

    #[tokio::test]
    async fn insert_and_recent() {
        let store = test_store().await;
        store
            .insert_memory(None, "global", MemoryKind::Note, "first", 1.0, 100)
            .await
            .unwrap();
        store
            .insert_memory(None, "global", MemoryKind::Note, "second", 1.0, 200)
            .await
            .unwrap();
        let recent = store.recent_candidates(10).await.unwrap();
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].text, "second"); // newest first
        assert!(recent[0].bm25.is_none());
    }

    #[tokio::test]
    async fn stats_counts_by_scope() {
        let store = test_store().await;
        store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100)
            .await
            .unwrap();
        store
            .insert_memory(None, "project:X", MemoryKind::Note, "b", 1.0, 100)
            .await
            .unwrap();
        let stats = store.stats().await.unwrap();
        assert_eq!(stats.total, 2);
        assert_eq!(stats.by_scope.len(), 2);
    }

    #[tokio::test]
    async fn touch_recalled_bumps_count() {
        let store = test_store().await;
        let id = store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100)
            .await
            .unwrap();
        store.touch_recalled(&[id], 500).await.unwrap();
        let recent = store.recent_candidates(1).await.unwrap();
        assert_eq!(recent[0].recall_count, 1);
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
    async fn update_and_read_embedding() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let id = store
            .insert_memory(None, "global", MemoryKind::Note, "vec me", 1.0, 100)
            .await
            .unwrap();
        let v = vec![0.1f32, 0.2, 0.3];
        store
            .update_embedding(id, &embedding_to_blob(&v), "mock")
            .await
            .unwrap();
        let embedded = store.embedded_candidates(10).await.unwrap();
        assert_eq!(embedded.len(), 1);
        assert_eq!(embedded[0].0.id, id);
        assert_eq!(embedded[0].1, v);
    }

    #[tokio::test]
    async fn rows_missing_embedding_excludes_embedded() {
        use crate::embed::embedding_to_blob;
        let store = test_store().await;
        let a = store
            .insert_memory(None, "global", MemoryKind::Note, "a", 1.0, 100)
            .await
            .unwrap();
        let _b = store
            .insert_memory(None, "global", MemoryKind::Note, "b", 1.0, 100)
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
}
