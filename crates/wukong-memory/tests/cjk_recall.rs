//! Chinese queries must reach the keyword index, not just the substring fallback.
//!
//! FTS5's default unicode61 tokenizer treats a run of CJK as ONE token, so a
//! query only matched when it was character-for-character identical to the whole
//! run in the document. Everything else fell through to the `LIKE '%query%'`
//! fallback, which papers over the miss but carries `bm25: None` — and that
//! makes `lexical_norm` 0, so ranking degrades to recency and the reported
//! confidence is 0.000 for every Chinese recall.
//!
//! These tests therefore assert on WHO answered and WHETHER it was ranked, not
//! merely on the hit count. Asserting `!hits.is_empty()` would pass on the
//! broken behaviour, because the fallback does return rows.

use tempfile::NamedTempFile;
use wukong_memory::{Memory, MemoryItem, MemoryKind, RecallMode, RecallQuery, RememberInput};

async fn store_with(texts: &[&str]) -> Memory {
    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);
    let mem = Memory::open(&url).await.unwrap();
    for text in texts {
        mem.remember(RememberInput {
            scope: "global".to_string(),
            session_id: None,
            items: vec![MemoryItem {
                kind: MemoryKind::Event,
                text: (*text).to_string(),
                importance: None,
                dedupe_key: None,
            }],
        })
        .await
        .unwrap();
    }
    mem
}

async fn recall(
    mem: &Memory,
    query: &str,
) -> wukong_memory::WukongResult<Vec<wukong_memory::RecallHit>> {
    mem.recall(RecallQuery {
        query: query.to_string(),
        top_k: 5,
        scope: Some("global".to_string()),
        mode: RecallMode::Keyword,
    })
    .await
    .unwrap()
}

const CORPUS: &[&str] = &[
    "User: 幫我看一下排程設定有沒有問題",
    "Assistant: 排程目前有三個任務，設定看起來正常",
    "User: 記憶庫現在多大",
    "User: scheduler settings look wrong",
];

/// The natural way to type a multi-keyword Chinese query. Before the fix this
/// returned zero rows: FTS could not match (the document is one big token) and
/// the fallback searched for the literal string *including the space*.
#[tokio::test]
async fn spaced_chinese_keywords_match() {
    let mem = store_with(CORPUS).await;
    let res = recall(&mem, "排程 設定").await;
    assert!(
        !res.data.is_empty(),
        "spaced Chinese keywords returned nothing"
    );
    assert!(
        res.data
            .iter()
            .any(|h| h.explanation.source_signals.iter().any(|s| s == "keyword")),
        "answered by the substring fallback, not the keyword index: {:?}",
        res.data
            .iter()
            .map(|h| &h.explanation.source_signals)
            .collect::<Vec<_>>()
    );
}

/// A Chinese phrase that appears inside a longer sentence must be found through
/// the index, so bm25 can rank it.
#[tokio::test]
async fn chinese_substring_of_a_longer_sentence_is_ranked() {
    let mem = store_with(CORPUS).await;
    let res = recall(&mem, "排程設定").await;
    assert!(!res.data.is_empty(), "no hits for 排程設定");
    assert!(
        res.data.iter().any(|h| h.explanation.lexical > 0.0),
        "every hit scored lexical=0, so ranking is recency-only"
    );
}

/// The regression that hides all the others: a Chinese recall reporting the same
/// confidence as a total miss. This is what made the failure invisible — the
/// aggregate `avg_top_relevance` reads 0 for a Chinese user whether recall is
/// working or not.
#[tokio::test]
async fn chinese_and_english_report_comparable_confidence() {
    let mem = store_with(CORPUS).await;
    let zh = recall(&mem, "排程設定").await;
    let en = recall(&mem, "scheduler settings").await;

    assert!(
        !zh.data.is_empty() && !en.data.is_empty(),
        "expected hits in both languages"
    );
    assert!(
        en.confidence > 0.0,
        "English baseline is itself broken: {}",
        en.confidence
    );
    assert!(
        zh.confidence > 0.0,
        "Chinese recall found {} hits but reported confidence {:.3}, \
         indistinguishable from finding nothing",
        zh.data.len(),
        zh.confidence
    );
}

/// Single characters and short runs must keep working — the bigram expansion
/// must not drop runs shorter than two characters.
#[tokio::test]
async fn short_chinese_queries_still_match() {
    let mem = store_with(&["User: 記憶庫現在多大", "User: 排程壞了"]).await;
    for query in ["排程", "記憶庫"] {
        let res = recall(&mem, query).await;
        assert!(!res.data.is_empty(), "no hits for {query}");
    }
}

/// English must not regress: it already worked through the index and must keep
/// its bm25 ranking.
#[tokio::test]
async fn english_recall_is_unaffected() {
    let mem = store_with(CORPUS).await;
    let res = recall(&mem, "scheduler settings").await;
    assert!(!res.data.is_empty(), "no hits for the English query");
    assert!(
        res.data
            .iter()
            .any(|h| h.explanation.source_signals.iter().any(|s| s == "keyword")),
        "English stopped using the keyword index"
    );
    assert!(res.confidence > 0.0, "English confidence collapsed");
}

/// Upgrading a database written before the bigram index must make its existing
/// Chinese memories searchable — the migration has to backfill `search_text` and
/// rebuild the index, not merely add a column. This builds the pre-fix schema by
/// hand (FTS on `text`, no `search_text`) so it exercises the real conversion.
#[tokio::test]
async fn existing_chinese_memories_become_searchable_after_upgrade() {
    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);

    {
        let pool = sqlx::SqlitePool::connect(&url).await.unwrap();
        sqlx::raw_sql(
            r#"
            CREATE TABLE memories (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                session_id TEXT, scope TEXT NOT NULL, kind TEXT NOT NULL,
                text TEXT NOT NULL, created_at INTEGER NOT NULL,
                last_recalled_at INTEGER, recall_count INTEGER NOT NULL DEFAULT 0,
                importance REAL NOT NULL DEFAULT 1.0
            );
            CREATE VIRTUAL TABLE memories_fts USING fts5(text, content='memories', content_rowid='id');
            CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
                INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
            END;
            INSERT INTO memories (scope, kind, text, created_at, importance)
            VALUES ('global', 'event', 'User: 幫我看一下排程設定有沒有問題', 100, 1.0),
                   ('global', 'event', 'User: legacy english memory', 101, 1.0);
            "#,
        )
        .execute(&pool)
        .await
        .unwrap();
        pool.close().await;
    }

    let mem = Memory::open(&url).await.unwrap();

    // Nothing may be lost by the rebuild.
    assert_eq!(mem.stats().await.unwrap().total, 2, "migration lost rows");

    let zh = recall(&mem, "排程 設定").await;
    assert!(
        zh.data
            .iter()
            .any(|h| h.explanation.source_signals.iter().any(|s| s == "keyword")),
        "pre-existing Chinese memory is still not in the keyword index: {:?}",
        zh.data
            .iter()
            .map(|h| &h.explanation.source_signals)
            .collect::<Vec<_>>()
    );
    assert!(
        zh.confidence > 0.0,
        "migrated Chinese recall still reports confidence 0"
    );

    let en = recall(&mem, "legacy english").await;
    assert!(!en.data.is_empty(), "migration broke English recall");
}
