use tempfile::NamedTempFile;
use wukong_memory::{Memory, MemoryItem, MemoryKind, RecallMode, RecallQuery, RememberInput};

async fn open_memory() -> Memory {
    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);
    Memory::open(&url).await.unwrap()
}

fn item(text: &str) -> MemoryItem {
    MemoryItem {
        kind: MemoryKind::Note,
        text: text.to_string(),
        importance: None,
    }
}

#[tokio::test]
async fn remember_then_recall_finds_match() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("we migrated the database to SQLite")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "sqlite migration".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert_eq!(res.data.len(), 1);
    assert!(res.data[0].text.contains("SQLite"));
    assert_eq!(res.evidence.len(), 1);
    assert!(res.confidence > 0.0);
}

#[tokio::test]
async fn scope_isolation_excludes_other_scopes() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "project:Alpha".to_string(),
        session_id: None,
        items: vec![item("alpha secret token")],
    })
    .await
    .unwrap();
    mem.remember(RememberInput {
        scope: "project:Beta".to_string(),
        session_id: None,
        items: vec![item("beta secret token")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "secret token".to_string(),
            top_k: 5,
            scope: Some("project:Alpha".to_string()),
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(res.data.iter().all(|h| h.scope == "project:Alpha"));
    assert!(res.data.iter().any(|h| h.text.contains("alpha")));
}

#[tokio::test]
async fn trivial_query_returns_empty() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("something memorable")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "of".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(res.data.is_empty());
    assert_eq!(res.confidence, 0.0);
}

#[tokio::test]
async fn short_cjk_query_is_not_treated_as_trivial() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("連線池設定使用 SQLite WAL")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "連線".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(
        res.data.iter().any(|hit| hit.text.contains("連線池設定")),
        "expected CJK query to recall the Chinese memory, got: {:?}",
        res.data.iter().map(|hit| &hit.text).collect::<Vec<_>>()
    );
}

#[tokio::test]
async fn low_information_cjk_query_is_skipped() {
    let mem = open_memory().await;
    mem.remember(RememberInput {
        scope: "global".to_string(),
        session_id: None,
        items: vec![item("可以部署的資料庫設定")],
    })
    .await
    .unwrap();

    let res = mem
        .recall(RecallQuery {
            query: "可以".to_string(),
            top_k: 5,
            scope: None,
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert!(res.data.is_empty());
    assert_eq!(res.confidence, 0.0);
}

#[tokio::test]
async fn invalid_scope_is_rejected() {
    let mem = open_memory().await;
    let err = mem
        .remember(RememberInput {
            scope: "bogus".to_string(),
            session_id: None,
            items: vec![item("x")],
        })
        .await;
    assert!(err.is_err());
}

#[tokio::test]
async fn remember_writes_embedding_and_backfill_fills_old_rows() {
    use std::sync::Arc;
    use wukong_memory::MockEmbedder;

    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);

    // v1-style write: no embedder yet -> row has no embedding.
    let plain = Memory::open(&url).await.unwrap();
    plain
        .remember(RememberInput {
            scope: "global".into(),
            session_id: None,
            items: vec![item("old row without vector")],
        })
        .await
        .unwrap();
    drop(plain);

    // Reopen with embedder, run backfill directly (deterministic).
    let mem = Memory::open(&url)
        .await
        .unwrap()
        .with_embedder(Arc::new(MockEmbedder::new(16)));
    mem.backfill_embeddings().await.unwrap();

    // New write now also gets an embedding inline.
    mem.remember(RememberInput {
        scope: "global".into(),
        session_id: None,
        items: vec![item("new row with vector")],
    })
    .await
    .unwrap();

    let hits = mem
        .recall(RecallQuery {
            query: "row".into(),
            top_k: 5,
            scope: Some("global".into()),
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();
    assert!(!hits.data.is_empty());
}

#[tokio::test]
async fn semantic_similarity_boosts_ranking() {
    use std::sync::Arc;
    use wukong_memory::{Embedder, Result};

    struct StubEmbedder;
    impl Embedder for StubEmbedder {
        fn embed(&self, text: &str) -> Result<Vec<f32>> {
            // "alpha"-bearing text -> [1,0]; everything else -> [0,1] (orthogonal).
            if text.contains("alpha") {
                Ok(vec![1.0, 0.0])
            } else {
                Ok(vec![0.0, 1.0])
            }
        }
        fn dim(&self) -> usize {
            2
        }
        fn model_id(&self) -> &str {
            "stub"
        }
    }

    let file = NamedTempFile::new().unwrap();
    let url = format!("sqlite://{}", file.path().display());
    std::mem::forget(file);

    let mem = Memory::open(&url)
        .await
        .unwrap()
        .with_embedder(Arc::new(StubEmbedder));

    mem.remember(RememberInput {
        scope: "global".into(),
        session_id: None,
        items: vec![item("zzz alpha zzz"), item("yyy beta yyy")],
    })
    .await
    .unwrap();
    mem.backfill_embeddings().await.unwrap();

    // Query embeds to [1,0] (contains "alpha"); semantically matches row 1.
    // Query token "query" is absent from both rows so the keyword source is empty;
    // both rows still arrive via the recency source, so ranking decides order.
    let hits = mem
        .recall(RecallQuery {
            query: "alpha query".into(),
            top_k: 2,
            scope: Some("global".into()),
            mode: RecallMode::Hybrid,
        })
        .await
        .unwrap();

    assert_eq!(hits.data.len(), 2);
    assert!(
        hits.data[0].text.contains("alpha"),
        "semantic match should rank first, got: {:?}",
        hits.data.iter().map(|h| &h.text).collect::<Vec<_>>()
    );
}
