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
