//! 多個服務同時開同一個記憶庫時，schema 遷移不能互相踩死。
//!
//! 容器部署裡 `wukong-web`、`wukong-telegram`、`wukong-schedulerd` 由 compose 同時
//! 拉起，三者都指向 `/data/memory.db`。遷移是 check-then-act（PRAGMA table_info 看
//! 欄位在不在，不在就 ALTER），三者可以同時讀到「不在」，然後同時 ALTER——後到的
//! 拿到 `duplicate column name` 而整個開檔失敗。
//!
//! v0.21.6 升級到既有部署時真的發生了：`wukong-schedulerd` 崩了一次，靠
//! `restart: unless-stopped` 才活過來。單行程的測試永遠看不到這個——`Store::open`
//! 自己是冪等的，壞的是併發。

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode};
use sqlx::{ConnectOptions, Executor};
use std::str::FromStr;
use tempfile::TempDir;
use wukong_memory::Memory;

/// 造一個「舊版」記憶庫：有資料、但沒有 v0.21.5 才加的 `search_text`，也沒有其他
/// 後加的欄位。這樣每個併發的 open 都會真的想去跑 ALTER，而不是一開始就短路。
/// 注意 fixture 必須是 **WAL**。`PRAGMA journal_mode=WAL` 需要獨佔鎖，而且那一步
/// 不吃 `busy_timeout`；rollback-journal 的 fixture 會讓三個 open 在「轉成 WAL」就
/// 互撞，測到的是 fixture 的假象而不是遷移競態。真實部署的 memory.db 從第一次
/// `Store::open` 起就是 WAL，這裡要對齊。
async fn seed_pre_migration_db(path: &str) {
    let mut conn = SqliteConnectOptions::from_str(&format!("sqlite://{path}"))
        .unwrap()
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .connect()
        .await
        .unwrap();

    conn.execute(
        "CREATE TABLE memories (
             id          INTEGER PRIMARY KEY AUTOINCREMENT,
             session_id  TEXT,
             scope       TEXT NOT NULL,
             kind        TEXT NOT NULL,
             text        TEXT NOT NULL,
             created_at  INTEGER NOT NULL,
             importance  REAL NOT NULL DEFAULT 0.5,
             recall_count INTEGER NOT NULL DEFAULT 0,
             last_recalled_at INTEGER
         );
         CREATE VIRTUAL TABLE memories_fts USING fts5(
             text, content='memories', content_rowid='id');
         CREATE TRIGGER memories_ai AFTER INSERT ON memories BEGIN
             INSERT INTO memories_fts(rowid, text) VALUES (new.id, new.text);
         END;
         INSERT INTO memories(scope, kind, text, created_at)
         VALUES ('global', 'event', '排程設定要怎麼改', 1),
                ('global', 'event', '記憶庫現在多大', 2);",
    )
    .await
    .unwrap();
}

/// 每個 opener 跑在自己的 OS 執行緒與自己的 current_thread runtime 上。
///
/// 不能用 `tokio::spawn`：`Memory::open` 的 future 不是 `Send`。而且獨立執行緒 +
/// 獨立 runtime 本來就更貼近要模擬的東西——三個各自持有 pool 的獨立行程。
fn spawn_opener(url: &str) -> std::thread::JoinHandle<Result<(), String>> {
    let url = url.to_string();
    std::thread::spawn(move || {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                Memory::open(&url)
                    .await
                    .map(|_| ())
                    .map_err(|e| e.to_string())
            })
    })
}

/// 三個服務同時開同一個舊版資料庫，全部都要成功。
///
/// 斷言的是「每一個都 Ok」，不是「至少一個 Ok」——壞掉的行為下也會有贏家，只斷言
/// 有人成功的話這個測試不會紅燈。
#[tokio::test]
async fn concurrent_opens_of_a_pre_migration_database_all_succeed() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("memory.db");
    let path = path.to_str().unwrap().to_string();
    seed_pre_migration_db(&path).await;

    let url = format!("sqlite://{path}");
    let handles: Vec<_> = (0..3).map(|i| (i, spawn_opener(&url))).collect();

    let mut failures = Vec::new();
    for (i, h) in handles {
        if let Err(e) = h.join().unwrap() {
            failures.push(format!("opener {i}: {e}"));
        }
    }
    assert!(
        failures.is_empty(),
        "併發開檔有失敗（生產環境等同一個服務起不來）：{failures:?}"
    );
}

/// 併發跑完之後，schema 必須是遷移後的樣子，而且資料還在。
///
/// 上一個測試只證明沒有人失敗；如果遷移在競爭中被跳過或做了一半，它仍會通過。
#[tokio::test]
async fn schema_is_fully_migrated_after_a_concurrent_race() {
    let dir = TempDir::new().unwrap();
    let path = dir.path().join("memory.db");
    let path = path.to_str().unwrap().to_string();
    seed_pre_migration_db(&path).await;

    let url = format!("sqlite://{path}");
    let handles: Vec<_> = (0..3).map(|_| spawn_opener(&url)).collect();
    for h in handles {
        h.join().unwrap().expect("concurrent open failed");
    }

    let mut conn = SqliteConnectOptions::from_str(&url)
        .unwrap()
        .connect()
        .await
        .unwrap();

    let cols: Vec<String> = sqlx::query_scalar("SELECT name FROM pragma_table_info('memories')")
        .fetch_all(&mut conn)
        .await
        .unwrap();
    for expected in ["search_text", "embedding", "embedding_model", "dedupe_key"] {
        assert!(cols.contains(&expected.to_string()), "少了欄位 {expected}");
    }

    let fts_sql: String =
        sqlx::query_scalar("SELECT sql FROM sqlite_master WHERE name = 'memories_fts'")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert!(
        fts_sql.contains("search_text"),
        "FTS 仍索引舊欄位：{fts_sql}"
    );

    // 恰好兩個 trigger，不是零個（被砸掉）也不是重複建立的殘骸。
    let triggers: Vec<String> =
        sqlx::query_scalar("SELECT name FROM sqlite_master WHERE type = 'trigger' ORDER BY name")
            .fetch_all(&mut conn)
            .await
            .unwrap();
    assert_eq!(
        triggers,
        vec!["memories_ad", "memories_ai"],
        "trigger 不完整"
    );

    let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM memories")
        .fetch_one(&mut conn)
        .await
        .unwrap();
    assert_eq!(rows, 2, "遷移過程掉了資料");

    let backfilled: i64 =
        sqlx::query_scalar("SELECT count(*) FROM memories WHERE search_text IS NOT NULL")
            .fetch_one(&mut conn)
            .await
            .unwrap();
    assert_eq!(backfilled, 2, "search_text 沒有回填完");
}
