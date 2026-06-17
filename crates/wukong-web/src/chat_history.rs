use serde::Serialize;
use sqlx::{Row, SqlitePool};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatMessage {
    pub id: i64,
    pub thread_id: String,
    pub role: String,
    pub content: String,
    pub content_html: Option<String>,
    pub status: String,
    pub created_at: i64,
}

pub struct ChatHistoryStore {
    pool: SqlitePool,
}

impl ChatHistoryStore {
    pub async fn open(db_url: &str) -> Result<Self, sqlx::Error> {
        let pool = SqlitePool::connect(db_url).await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_threads (
                id TEXT PRIMARY KEY,
                scope TEXT NOT NULL,
                title TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_messages (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                thread_id TEXT NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                content_html TEXT,
                status TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(thread_id) REFERENCES chat_threads(id) ON DELETE CASCADE
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS chat_messages_thread_id_id_idx
             ON chat_messages(thread_id, id)",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS chat_messages_thread_id_created_at_idx
             ON chat_messages(thread_id, created_at)",
        )
        .execute(&pool)
        .await?;
        Ok(Self { pool })
    }

    pub async fn default_thread(&self, scope: &str) -> Result<String, sqlx::Error> {
        let id = format!("scope:{scope}");
        let now = now_unix();
        sqlx::query(
            "INSERT INTO chat_threads (id, scope, title, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4)
             ON CONFLICT(id) DO UPDATE SET updated_at = excluded.updated_at",
        )
        .bind(&id)
        .bind(scope)
        .bind("Default")
        .bind(now)
        .execute(&self.pool)
        .await?;
        Ok(id)
    }

    pub async fn insert_message(
        &self,
        thread_id: &str,
        role: &str,
        content: &str,
        content_html: Option<&str>,
        status: &str,
        created_at: i64,
    ) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO chat_messages (thread_id, role, content, content_html, status, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
        )
        .bind(thread_id)
        .bind(role)
        .bind(content)
        .bind(content_html)
        .bind(status)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;
        sqlx::query("UPDATE chat_threads SET updated_at = ?2 WHERE id = ?1")
            .bind(thread_id)
            .bind(created_at)
            .execute(&self.pool)
            .await?;
        Ok(row.get("id"))
    }

    pub async fn latest_messages(
        &self,
        thread_id: &str,
        limit: i64,
    ) -> Result<Vec<ChatMessage>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT * FROM (
                 SELECT id, thread_id, role, content, content_html, status, created_at
                 FROM chat_messages
                 WHERE thread_id = ?1
                 ORDER BY id DESC
                 LIMIT ?2
             ) ORDER BY id ASC",
        )
        .bind(thread_id)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_message).collect())
    }

    pub async fn messages_before(
        &self,
        thread_id: &str,
        before: i64,
        limit: i64,
    ) -> Result<Vec<ChatMessage>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT * FROM (
                 SELECT id, thread_id, role, content, content_html, status, created_at
                 FROM chat_messages
                 WHERE thread_id = ?1 AND id < ?2
                 ORDER BY id DESC
                 LIMIT ?3
             ) ORDER BY id ASC",
        )
        .bind(thread_id)
        .bind(before)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_message).collect())
    }

    pub async fn messages_for_date(
        &self,
        thread_id: &str,
        start: i64,
        end: i64,
        limit: i64,
    ) -> Result<Vec<ChatMessage>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, thread_id, role, content, content_html, status, created_at
             FROM chat_messages
             WHERE thread_id = ?1 AND created_at >= ?2 AND created_at < ?3
             ORDER BY created_at ASC, id ASC
             LIMIT ?4",
        )
        .bind(thread_id)
        .bind(start)
        .bind(end)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_message).collect())
    }
}

fn row_to_message(row: sqlx::sqlite::SqliteRow) -> ChatMessage {
    ChatMessage {
        id: row.get("id"),
        thread_id: row.get("thread_id"),
        role: row.get("role"),
        content: row.get("content"),
        content_html: row.get("content_html"),
        status: row.get("status"),
        created_at: row.get("created_at"),
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    async fn store() -> ChatHistoryStore {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        ChatHistoryStore::open(&url).await.unwrap()
    }

    #[tokio::test]
    async fn default_thread_is_stable_per_scope() {
        let store = store().await;
        let a = store.default_thread("global").await.unwrap();
        let b = store.default_thread("global").await.unwrap();
        assert_eq!(a, b);
        assert_eq!(a, "scope:global");
    }

    #[tokio::test]
    async fn latest_messages_returns_newest_window_in_ascending_order() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        for i in 0..12 {
            store
                .insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i)
                .await
                .unwrap();
        }

        let messages = store.latest_messages(&thread, 10).await.unwrap();
        assert_eq!(messages.len(), 10);
        assert_eq!(messages.first().unwrap().content, "m2");
        assert_eq!(messages.last().unwrap().content, "m11");
    }

    #[tokio::test]
    async fn messages_before_returns_older_window() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        let mut ids = Vec::new();
        for i in 0..12 {
            let id = store
                .insert_message(&thread, "user", &format!("m{i}"), None, "complete", 100 + i)
                .await
                .unwrap();
            ids.push(id);
        }

        let messages = store.messages_before(&thread, ids[10], 10).await.unwrap();
        assert_eq!(messages.len(), 10);
        assert_eq!(messages.first().unwrap().content, "m0");
        assert_eq!(messages.last().unwrap().content, "m9");
    }

    #[tokio::test]
    async fn messages_for_date_filters_by_time_range() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        store.insert_message(&thread, "user", "old", None, "complete", 9).await.unwrap();
        store.insert_message(&thread, "user", "in", None, "complete", 10).await.unwrap();
        store
            .insert_message(
                &thread,
                "assistant",
                "also in",
                Some("<p>also in</p>"),
                "complete",
                19,
            )
            .await
            .unwrap();
        store.insert_message(&thread, "user", "new", None, "complete", 20).await.unwrap();

        let messages = store.messages_for_date(&thread, 10, 20, 10).await.unwrap();
        assert_eq!(
            messages.iter().map(|m| m.content.as_str()).collect::<Vec<_>>(),
            vec!["in", "also in"]
        );
        assert_eq!(messages[1].content_html.as_deref(), Some("<p>also in</p>"));
    }
}
