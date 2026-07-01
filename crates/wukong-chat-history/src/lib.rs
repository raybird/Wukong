mod attachments;

pub use attachments::{
    relative_attachment_path, resolve_under_upload_root, sanitize_filename, sanitize_scope,
    ChatAttachment, NewChatAttachment,
};

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
    /// Number of helper-baton steps linked to this message (0 for most turns).
    /// Lets the UI show a "reasoning" collapsible only when there's something in it.
    #[serde(default)]
    pub step_count: i64,
    /// Number of stream events linked to this message (reasoning/tool/status).
    #[serde(default)]
    pub event_count: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatScope {
    pub scope: String,
    pub label: String,
    pub message_count: i64,
    pub updated_at: i64,
}

/// A non-final (helper) baton's output captured for one turn, linked to the
/// final assistant message. Kept out of the main `chat_messages` timeline so it
/// neither pollutes pagination nor memory recall.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnStep {
    pub id: i64,
    pub message_id: i64,
    pub seq: i64,
    pub role: String,
    pub content: String,
    pub content_html: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TurnEvent {
    pub id: i64,
    pub message_id: i64,
    pub seq: i64,
    pub kind: String,
    pub label: Option<String>,
    pub content: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ChatLiveEvent {
    pub id: i64,
    pub scope: String,
    pub kind: String,
    pub label: Option<String>,
    pub content: String,
    pub message_id: Option<i64>,
    pub created_at: i64,
}

#[derive(Clone)]
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
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_attachments (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id INTEGER NOT NULL,
                scope TEXT NOT NULL,
                source TEXT NOT NULL,
                original_name TEXT NOT NULL,
                stored_name TEXT NOT NULL,
                relative_path TEXT NOT NULL,
                mime_type TEXT,
                size_bytes INTEGER NOT NULL,
                sha256 TEXT,
                telegram_file_id TEXT,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(message_id) REFERENCES chat_messages(id) ON DELETE CASCADE
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS chat_attachments_message_id_idx
             ON chat_attachments(message_id)",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS chat_attachments_scope_id_idx
             ON chat_attachments(scope, id)",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS turn_steps (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id INTEGER NOT NULL,
                seq INTEGER NOT NULL,
                role TEXT NOT NULL,
                content TEXT NOT NULL,
                content_html TEXT,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(message_id) REFERENCES chat_messages(id) ON DELETE CASCADE
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS turn_steps_message_id_idx
             ON turn_steps(message_id)",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS turn_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                message_id INTEGER NOT NULL,
                seq INTEGER NOT NULL,
                kind TEXT NOT NULL,
                label TEXT,
                content TEXT NOT NULL,
                created_at INTEGER NOT NULL,
                FOREIGN KEY(message_id) REFERENCES chat_messages(id) ON DELETE CASCADE
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS turn_events_message_id_idx
             ON turn_events(message_id)",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE TABLE IF NOT EXISTS chat_live_events (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                scope TEXT NOT NULL,
                kind TEXT NOT NULL,
                label TEXT,
                content TEXT NOT NULL,
                message_id INTEGER,
                created_at INTEGER NOT NULL
            )",
        )
        .execute(&pool)
        .await?;
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS chat_live_events_scope_id_idx
             ON chat_live_events(scope, id)",
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

    pub async fn insert_attachment(&self, a: &NewChatAttachment) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO chat_attachments
             (message_id, scope, source, original_name, stored_name, relative_path, mime_type, size_bytes, sha256, telegram_file_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             RETURNING id",
        )
        .bind(a.message_id)
        .bind(&a.scope)
        .bind(&a.source)
        .bind(&a.original_name)
        .bind(&a.stored_name)
        .bind(&a.relative_path)
        .bind(&a.mime_type)
        .bind(a.size_bytes)
        .bind(&a.sha256)
        .bind(&a.telegram_file_id)
        .bind(a.created_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("id"))
    }

    pub async fn attachments_for_messages(
        &self,
        message_ids: &[i64],
    ) -> Result<Vec<ChatAttachment>, sqlx::Error> {
        if message_ids.is_empty() {
            return Ok(Vec::new());
        }
        let placeholders = std::iter::repeat_n("?", message_ids.len())
            .collect::<Vec<_>>()
            .join(",");
        let sql = format!(
            "SELECT id, message_id, scope, source, original_name, stored_name, relative_path, mime_type, size_bytes, sha256, telegram_file_id, created_at
             FROM chat_attachments WHERE message_id IN ({placeholders}) ORDER BY id ASC"
        );
        let mut query = sqlx::query(&sql);
        for id in message_ids {
            query = query.bind(id);
        }
        let rows = query.fetch_all(&self.pool).await?;
        Ok(rows.into_iter().map(row_to_attachment).collect())
    }

    pub async fn attachment(&self, id: i64) -> Result<Option<ChatAttachment>, sqlx::Error> {
        let row = sqlx::query(
            "SELECT id, message_id, scope, source, original_name, stored_name, relative_path, mime_type, size_bytes, sha256, telegram_file_id, created_at
             FROM chat_attachments WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(row.map(row_to_attachment))
    }

    /// Persist one helper-baton step linked to the final assistant message.
    pub async fn insert_step(
        &self,
        message_id: i64,
        seq: i64,
        role: &str,
        content: &str,
        content_html: Option<&str>,
        created_at: i64,
    ) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO turn_steps (message_id, seq, role, content, content_html, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
        )
        .bind(message_id)
        .bind(seq)
        .bind(role)
        .bind(content)
        .bind(content_html)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("id"))
    }

    /// All helper-baton steps for a final assistant message, in chain order.
    pub async fn list_steps(&self, message_id: i64) -> Result<Vec<TurnStep>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, message_id, seq, role, content, content_html, created_at
             FROM turn_steps
             WHERE message_id = ?1
             ORDER BY seq ASC, id ASC",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_step).collect())
    }

    pub async fn insert_event(
        &self,
        message_id: i64,
        seq: i64,
        kind: &str,
        label: Option<&str>,
        content: &str,
        created_at: i64,
    ) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO turn_events (message_id, seq, kind, label, content, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
        )
        .bind(message_id)
        .bind(seq)
        .bind(kind)
        .bind(label)
        .bind(content)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("id"))
    }

    pub async fn list_events(&self, message_id: i64) -> Result<Vec<TurnEvent>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, message_id, seq, kind, label, content, created_at
             FROM turn_events
             WHERE message_id = ?1
             ORDER BY seq ASC, id ASC",
        )
        .bind(message_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_event).collect())
    }

    pub async fn insert_live_event(
        &self,
        scope: &str,
        kind: &str,
        label: Option<&str>,
        content: &str,
        message_id: Option<i64>,
        created_at: i64,
    ) -> Result<i64, sqlx::Error> {
        let row = sqlx::query(
            "INSERT INTO chat_live_events (scope, kind, label, content, message_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             RETURNING id",
        )
        .bind(scope)
        .bind(kind)
        .bind(label)
        .bind(content)
        .bind(message_id)
        .bind(created_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(row.get("id"))
    }

    pub async fn live_events_after(
        &self,
        scope: &str,
        after: i64,
        limit: i64,
    ) -> Result<Vec<ChatLiveEvent>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT id, scope, kind, label, content, message_id, created_at
             FROM chat_live_events
             WHERE scope = ?1 AND id > ?2
             ORDER BY id ASC
             LIMIT ?3",
        )
        .bind(scope)
        .bind(after)
        .bind(limit.clamp(1, 100))
        .fetch_all(&self.pool)
        .await?;
        Ok(rows.into_iter().map(row_to_live_event).collect())
    }

    pub async fn prune_live_events_before(&self, created_before: i64) -> Result<u64, sqlx::Error> {
        let result = sqlx::query("DELETE FROM chat_live_events WHERE created_at < ?1")
            .bind(created_before)
            .execute(&self.pool)
            .await?;
        Ok(result.rows_affected())
    }

    pub async fn latest_messages(
        &self,
        thread_id: &str,
        limit: i64,
    ) -> Result<Vec<ChatMessage>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT * FROM (
                 SELECT id, thread_id, role, content, content_html, status, created_at,
                         (SELECT COUNT(*) FROM turn_steps ts WHERE ts.message_id = chat_messages.id) AS step_count,
                         (SELECT COUNT(*) FROM turn_events te WHERE te.message_id = chat_messages.id) AS event_count
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
                 SELECT id, thread_id, role, content, content_html, status, created_at,
                         (SELECT COUNT(*) FROM turn_steps ts WHERE ts.message_id = chat_messages.id) AS step_count,
                         (SELECT COUNT(*) FROM turn_events te WHERE te.message_id = chat_messages.id) AS event_count
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
            "SELECT id, thread_id, role, content, content_html, status, created_at,
                    (SELECT COUNT(*) FROM turn_steps ts WHERE ts.message_id = chat_messages.id) AS step_count,
                    (SELECT COUNT(*) FROM turn_events te WHERE te.message_id = chat_messages.id) AS event_count
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

    pub async fn list_scopes(&self, default_scope: &str) -> Result<Vec<ChatScope>, sqlx::Error> {
        let rows = sqlx::query(
            "SELECT t.scope AS scope,
                    COALESCE(MAX(t.updated_at), 0) AS updated_at,
                    COUNT(m.id) AS message_count
             FROM chat_threads t
             LEFT JOIN chat_messages m ON m.thread_id = t.id
             GROUP BY t.scope
             ORDER BY updated_at DESC, scope ASC",
        )
        .fetch_all(&self.pool)
        .await?;

        let mut scopes: Vec<ChatScope> = rows
            .into_iter()
            .map(|row| {
                let scope: String = row.get("scope");
                ChatScope {
                    label: scope_label(&scope),
                    scope,
                    message_count: row.get("message_count"),
                    updated_at: row.get("updated_at"),
                }
            })
            .collect();

        if !scopes.iter().any(|s| s.scope == default_scope) {
            scopes.push(ChatScope {
                scope: default_scope.to_string(),
                label: scope_label(default_scope),
                message_count: 0,
                updated_at: 0,
            });
        }

        Ok(scopes)
    }
}

pub fn scope_label(scope: &str) -> String {
    if let Some(id) = scope.strip_prefix("user:tg-") {
        format!("Telegram {id}")
    } else if let Some(project) = scope.strip_prefix("project:") {
        format!("Project {project}")
    } else if scope == "global" {
        "Global".to_string()
    } else {
        scope.to_string()
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
        // Tolerant: SELECTs that don't compute the count still map cleanly to 0.
        step_count: row.try_get("step_count").unwrap_or(0),
        event_count: row.try_get("event_count").unwrap_or(0),
    }
}

fn row_to_step(row: sqlx::sqlite::SqliteRow) -> TurnStep {
    TurnStep {
        id: row.get("id"),
        message_id: row.get("message_id"),
        seq: row.get("seq"),
        role: row.get("role"),
        content: row.get("content"),
        content_html: row.get("content_html"),
        created_at: row.get("created_at"),
    }
}

fn row_to_event(row: sqlx::sqlite::SqliteRow) -> TurnEvent {
    TurnEvent {
        id: row.get("id"),
        message_id: row.get("message_id"),
        seq: row.get("seq"),
        kind: row.get("kind"),
        label: row.get("label"),
        content: row.get("content"),
        created_at: row.get("created_at"),
    }
}

fn row_to_attachment(row: sqlx::sqlite::SqliteRow) -> ChatAttachment {
    ChatAttachment {
        id: row.get("id"),
        message_id: row.get("message_id"),
        scope: row.get("scope"),
        source: row.get("source"),
        original_name: row.get("original_name"),
        stored_name: row.get("stored_name"),
        relative_path: row.get("relative_path"),
        mime_type: row.get("mime_type"),
        size_bytes: row.get("size_bytes"),
        sha256: row.get("sha256"),
        telegram_file_id: row.get("telegram_file_id"),
        created_at: row.get("created_at"),
    }
}

fn row_to_live_event(row: sqlx::sqlite::SqliteRow) -> ChatLiveEvent {
    ChatLiveEvent {
        id: row.get("id"),
        scope: row.get("scope"),
        kind: row.get("kind"),
        label: row.get("label"),
        content: row.get("content"),
        message_id: row.get("message_id"),
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
    async fn attachments_round_trip_for_message() {
        let store = store().await;
        let thread = store.default_thread("user:tg-42").await.unwrap();
        let message_id = store
            .insert_message(&thread, "user", "請看附件", None, "complete", 100)
            .await
            .unwrap();

        let new = NewChatAttachment {
            message_id,
            scope: "user:tg-42".to_string(),
            source: "telegram".to_string(),
            original_name: "report.pdf".to_string(),
            stored_name: "report.pdf".to_string(),
            relative_path: "user_tg-42/1/report.pdf".to_string(),
            mime_type: Some("application/pdf".to_string()),
            size_bytes: 1234,
            sha256: Some("abc123".to_string()),
            telegram_file_id: Some("file_1".to_string()),
            created_at: 101,
        };

        let id = store.insert_attachment(&new).await.unwrap();
        let attachments = store.attachments_for_messages(&[message_id]).await.unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].id, id);
        assert_eq!(attachments[0].message_id, message_id);
        assert_eq!(attachments[0].original_name, "report.pdf");
        assert_eq!(attachments[0].mime_type.as_deref(), Some("application/pdf"));

        let loaded = store.attachment(id).await.unwrap().unwrap();
        assert_eq!(loaded.relative_path, "user_tg-42/1/report.pdf");
    }

    #[test]
    fn attachment_path_parts_are_sanitized() {
        assert_eq!(sanitize_scope("user:tg-42"), "user_tg-42");
        assert_eq!(sanitize_filename("../report final.pdf"), "report final.pdf");
        assert_eq!(sanitize_filename(""), "attachment");
        assert_eq!(sanitize_filename(".."), "attachment");
    }

    #[tokio::test]
    async fn turn_steps_round_trip_in_chain_order() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        let mid = store
            .insert_message(
                &thread,
                "assistant",
                "final",
                Some("<p>final</p>"),
                "complete",
                100,
            )
            .await
            .unwrap();
        // Insert out of seq order to prove ORDER BY seq.
        store
            .insert_step(mid, 1, "oracle", "o1", Some("<p>o1</p>"), 100)
            .await
            .unwrap();
        store
            .insert_step(mid, 0, "explorer", "e1", None, 100)
            .await
            .unwrap();

        let steps = store.list_steps(mid).await.unwrap();
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0].role, "explorer");
        assert_eq!(steps[0].seq, 0);
        assert_eq!(steps[0].content, "e1");
        assert_eq!(steps[0].content_html, None);
        assert_eq!(steps[1].role, "oracle");
        assert_eq!(steps[1].content_html.as_deref(), Some("<p>o1</p>"));

        // Steps are scoped per message_id.
        let other = store
            .insert_message(&thread, "assistant", "other", None, "complete", 200)
            .await
            .unwrap();
        assert!(store.list_steps(other).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn turn_events_round_trip_in_stream_order() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        let mid = store
            .insert_message(
                &thread,
                "assistant",
                "final",
                Some("<p>final</p>"),
                "complete",
                100,
            )
            .await
            .unwrap();

        store
            .insert_event(mid, 1, "tool_use", Some("read"), "使用工具 read", 101)
            .await
            .unwrap();
        store
            .insert_event(mid, 0, "reasoning", None, "先想一下", 100)
            .await
            .unwrap();

        let events = store.list_events(mid).await.unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 0);
        assert_eq!(events[0].kind, "reasoning");
        assert_eq!(events[0].label, None);
        assert_eq!(events[0].content, "先想一下");
        assert_eq!(events[1].seq, 1);
        assert_eq!(events[1].kind, "tool_use");
        assert_eq!(events[1].label.as_deref(), Some("read"));
    }

    #[tokio::test]
    async fn latest_messages_include_event_count() {
        let store = store().await;
        let thread = store.default_thread("global").await.unwrap();
        let mid = store
            .insert_message(&thread, "assistant", "final", None, "complete", 100)
            .await
            .unwrap();
        store
            .insert_event(mid, 0, "reasoning", None, "想", 100)
            .await
            .unwrap();

        let messages = store.latest_messages(&thread, 10).await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].event_count, 1);
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
        store
            .insert_message(&thread, "user", "old", None, "complete", 9)
            .await
            .unwrap();
        store
            .insert_message(&thread, "user", "in", None, "complete", 10)
            .await
            .unwrap();
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
        store
            .insert_message(&thread, "user", "new", None, "complete", 20)
            .await
            .unwrap();

        let messages = store.messages_for_date(&thread, 10, 20, 10).await.unwrap();
        assert_eq!(
            messages
                .iter()
                .map(|m| m.content.as_str())
                .collect::<Vec<_>>(),
            vec!["in", "also in"]
        );
        assert_eq!(messages[1].content_html.as_deref(), Some("<p>also in</p>"));
    }

    #[tokio::test]
    async fn list_scopes_includes_existing_and_empty_default() {
        let store = store().await;
        let tg = store.default_thread("user:tg-915354960").await.unwrap();
        store
            .insert_message(&tg, "user", "hi", None, "complete", 10)
            .await
            .unwrap();

        let scopes = store.list_scopes("global").await.unwrap();

        assert!(scopes.iter().any(|s| {
            s.scope == "user:tg-915354960"
                && s.label == "Telegram 915354960"
                && s.message_count == 1
                && s.updated_at == 10
        }));
        assert!(scopes
            .iter()
            .any(|s| s.scope == "global" && s.label == "Global" && s.message_count == 0));
    }

    #[tokio::test]
    async fn live_events_round_trip_by_scope_after_cursor() {
        let store = store().await;
        let first = store
            .insert_live_event("user:tg-12", "user", None, "hello", Some(10), 100)
            .await
            .unwrap();
        let second = store
            .insert_live_event("user:tg-12", "reasoning", None, "想一下", None, 101)
            .await
            .unwrap();
        store
            .insert_live_event("user:tg-99", "user", None, "other", Some(99), 102)
            .await
            .unwrap();

        let events = store
            .live_events_after("user:tg-12", first, 10)
            .await
            .unwrap();

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, second);
        assert_eq!(events[0].scope, "user:tg-12");
        assert_eq!(events[0].kind, "reasoning");
        assert_eq!(events[0].content, "想一下");
        assert_eq!(events[0].message_id, None);
    }

    #[tokio::test]
    async fn live_events_prune_by_created_at() {
        let store = store().await;
        store
            .insert_live_event("user:tg-12", "user", None, "old", None, 100)
            .await
            .unwrap();
        let kept = store
            .insert_live_event("user:tg-12", "answer", None, "new", Some(2), 200)
            .await
            .unwrap();

        let deleted = store.prune_live_events_before(150).await.unwrap();
        let events = store.live_events_after("user:tg-12", 0, 10).await.unwrap();

        assert_eq!(deleted, 1);
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].id, kept);
        assert_eq!(events[0].message_id, Some(2));
    }

    #[test]
    fn labels_known_scope_prefixes() {
        assert_eq!(scope_label("user:tg-12"), "Telegram 12");
        assert_eq!(scope_label("project:Wukong"), "Project Wukong");
        assert_eq!(scope_label("global"), "Global");
        assert_eq!(scope_label("agent:fixer"), "agent:fixer");
    }
}
