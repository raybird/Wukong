//! `opencode.db` 的空間回收。
//!
//! 刪除 session 只會把頁面放回 freelist，檔案不會變小；要 `VACUUM` 才縮得回來。
//! 由 `opencode-server` 容器在啟動 `opencode serve` 之前呼叫，那時還沒有人開著這個
//! 資料庫。量測與門檻的由來見 `docs/issues/issue-0003/implementation-plan.md`。

use sqlx::sqlite::SqliteConnectOptions;
use sqlx::{ConnectOptions, Connection};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// freelist 佔比達到這個比例才回收。
const MIN_FREE_RATIO: f64 = 0.25;
/// 剩餘磁碟空間至少要是資料庫大小的幾倍。實測 WAL 峰值約 1 倍，暫存檔未量測，
/// 取保守值。
const DISK_HEADROOM_FACTOR: u64 = 2;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq)]
enum VacuumOutcome {
    /// 資料庫檔案不存在（全新部署）。
    Missing,
    BelowThreshold {
        free_pages: u64,
        total_pages: u64,
    },
    InsufficientDisk {
        needed_bytes: u64,
        available_bytes: u64,
    },
    Reclaimed {
        before_bytes: u64,
        after_bytes: u64,
    },
}

/// `WUKONG_OPENCODE_DB`，否則 opencode 的預設位置。與 `opencode-idle-restart.sh`
/// 用同一個變數。
fn opencode_db_path() -> PathBuf {
    if let Some(path) = std::env::var_os("WUKONG_OPENCODE_DB").filter(|v| !v.is_empty()) {
        return PathBuf::from(path);
    }
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/share")
        });
    data_home.join("opencode").join("opencode.db")
}

/// 資料庫所在檔案系統的剩餘空間。
fn available_bytes(path: &Path) -> std::io::Result<u64> {
    let dir = path.parent().unwrap_or(Path::new("."));
    let stat = rustix::fs::statvfs(dir)?;
    Ok(stat.f_bavail.saturating_mul(stat.f_frsize))
}

/// 對預設位置的 `opencode.db` 執行一次，回傳給日誌用的一行結果。
pub async fn run() -> Result<String, String> {
    let path = opencode_db_path();
    let shown = path.display();
    let outcome = vacuum_if_worthwhile(&path)
        .await
        .map_err(|error| format!("opencode_db_vacuum path={shown} outcome=failed error={error}"))?;
    let detail = match outcome {
        VacuumOutcome::Missing => "outcome=missing".to_string(),
        VacuumOutcome::BelowThreshold {
            free_pages,
            total_pages,
        } => format!(
            "outcome=skipped reason=below_threshold free_pages={free_pages} total_pages={total_pages}"
        ),
        VacuumOutcome::InsufficientDisk {
            needed_bytes,
            available_bytes,
        } => format!(
            "outcome=skipped reason=insufficient_disk needed_bytes={needed_bytes} available_bytes={available_bytes}"
        ),
        VacuumOutcome::Reclaimed {
            before_bytes,
            after_bytes,
        } => format!("outcome=reclaimed before_bytes={before_bytes} after_bytes={after_bytes}"),
    };
    Ok(format!("opencode_db_vacuum path={shown} {detail}"))
}

/// 可回收空間夠多、磁碟也放得下時才 `VACUUM`。
async fn vacuum_if_worthwhile(path: &Path) -> Result<VacuumOutcome, sqlx::Error> {
    if !path.exists() {
        return Ok(VacuumOutcome::Missing);
    }
    vacuum(path, available_bytes(path)?, BUSY_TIMEOUT).await
}

/// 剩餘空間與 busy timeout 由呼叫端給，測試才能指定。
async fn vacuum(
    path: &Path,
    available_bytes: u64,
    busy_timeout: Duration,
) -> Result<VacuumOutcome, sqlx::Error> {
    // 不指定 journal_mode：這是 opencode 的資料庫，它的設定不歸我們改。
    let mut conn = SqliteConnectOptions::new()
        .filename(path)
        .busy_timeout(busy_timeout)
        .connect()
        .await?;
    let total_pages: i64 = sqlx::query_scalar("PRAGMA page_count")
        .fetch_one(&mut conn)
        .await?;
    let free_pages: i64 = sqlx::query_scalar("PRAGMA freelist_count")
        .fetch_one(&mut conn)
        .await?;
    let (total_pages, free_pages) = (total_pages as u64, free_pages as u64);
    if total_pages == 0 || (free_pages as f64) < (total_pages as f64) * MIN_FREE_RATIO {
        conn.close().await?;
        return Ok(VacuumOutcome::BelowThreshold {
            free_pages,
            total_pages,
        });
    }
    let before_bytes = std::fs::metadata(path)?.len();
    let needed_bytes = before_bytes.saturating_mul(DISK_HEADROOM_FACTOR);
    if available_bytes < needed_bytes {
        conn.close().await?;
        return Ok(VacuumOutcome::InsufficientDisk {
            needed_bytes,
            available_bytes,
        });
    }
    sqlx::query("VACUUM").execute(&mut conn).await?;
    // WAL 模式下重寫的內容先進 WAL，checkpoint 之後主檔才真的變小。
    sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
        .execute(&mut conn)
        .await?;
    conn.close().await?;
    Ok(VacuumOutcome::Reclaimed {
        before_bytes,
        after_bytes: std::fs::metadata(path)?.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqliteJournalMode;
    use sqlx::SqliteConnection;

    async fn connect(path: &Path) -> SqliteConnection {
        SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true)
            .journal_mode(SqliteJournalMode::Wal)
            .connect()
            .await
            .unwrap()
    }

    /// 一個 WAL 模式的資料庫：一筆要留下的資料，外加 `filler_rows` 筆之後會被刪掉的。
    async fn database(dir: &Path, filler_rows: u32, delete_filler: bool) -> PathBuf {
        let path = dir.join("opencode.db");
        let mut conn = connect(&path).await;
        sqlx::raw_sql(
            "CREATE TABLE part (id INTEGER PRIMARY KEY, keep INTEGER NOT NULL, data BLOB NOT NULL);
             INSERT INTO part (keep, data) VALUES (1, randomblob(4096));",
        )
        .execute(&mut conn)
        .await
        .unwrap();
        sqlx::query(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < ?1)
             INSERT INTO part (keep, data) SELECT 0, randomblob(4096) FROM n",
        )
        .bind(filler_rows)
        .execute(&mut conn)
        .await
        .unwrap();
        if delete_filler {
            sqlx::query("DELETE FROM part WHERE keep = 0")
                .execute(&mut conn)
                .await
                .unwrap();
        }
        sqlx::query("PRAGMA wal_checkpoint(TRUNCATE)")
            .execute(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        path
    }

    fn size(path: &Path) -> u64 {
        std::fs::metadata(path).unwrap().len()
    }

    async fn kept_rows(path: &Path) -> i64 {
        let mut conn = connect(path).await;
        let rows: i64 = sqlx::query_scalar("SELECT count(*) FROM part WHERE keep = 1")
            .fetch_one(&mut conn)
            .await
            .unwrap();
        conn.close().await.unwrap();
        rows
    }

    #[tokio::test]
    async fn reclaims_space_once_enough_of_the_file_is_free() {
        let dir = tempfile::tempdir().unwrap();
        let path = database(dir.path(), 500, true).await;
        let before = size(&path);

        let outcome = vacuum(&path, u64::MAX, BUSY_TIMEOUT).await.unwrap();

        let after = size(&path);
        assert_eq!(
            outcome,
            VacuumOutcome::Reclaimed {
                before_bytes: before,
                after_bytes: after
            }
        );
        // 500 筆 4 KiB 的資料被刪掉後，檔案應該只剩下原本的一小部分。
        assert!(after * 10 < before, "before={before} after={after}");
        assert_eq!(kept_rows(&path).await, 1);
    }

    #[tokio::test]
    async fn leaves_the_file_alone_below_the_threshold() {
        let dir = tempfile::tempdir().unwrap();
        let path = database(dir.path(), 500, false).await;
        let before = size(&path);

        let outcome = vacuum(&path, u64::MAX, BUSY_TIMEOUT).await.unwrap();

        assert!(
            matches!(outcome, VacuumOutcome::BelowThreshold { free_pages: 0, total_pages } if total_pages > 0),
            "{outcome:?}"
        );
        assert_eq!(size(&path), before);
    }

    #[tokio::test]
    async fn refuses_when_the_disk_cannot_hold_the_rewrite() {
        let dir = tempfile::tempdir().unwrap();
        let path = database(dir.path(), 500, true).await;
        let before = size(&path);

        let outcome = vacuum(&path, before * 2 - 1, BUSY_TIMEOUT).await.unwrap();

        assert_eq!(
            outcome,
            VacuumOutcome::InsufficientDisk {
                needed_bytes: before * 2,
                available_bytes: before * 2 - 1
            }
        );
        assert_eq!(size(&path), before);
    }

    #[tokio::test]
    async fn fails_without_touching_the_file_while_another_writer_holds_it() {
        let dir = tempfile::tempdir().unwrap();
        let path = database(dir.path(), 500, true).await;
        let before = size(&path);
        let mut writer = connect(&path).await;
        sqlx::query("BEGIN IMMEDIATE")
            .execute(&mut writer)
            .await
            .unwrap();

        let result = vacuum(&path, u64::MAX, Duration::from_millis(200)).await;

        let error = result.unwrap_err().to_string();
        assert!(error.contains("locked"), "{error}");
        assert_eq!(size(&path), before);
        sqlx::query("ROLLBACK").execute(&mut writer).await.unwrap();
        writer.close().await.unwrap();
    }

    #[tokio::test]
    async fn a_missing_database_is_not_an_error_and_is_not_created() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("opencode.db");

        let outcome = vacuum_if_worthwhile(&path).await.unwrap();

        assert_eq!(outcome, VacuumOutcome::Missing);
        assert!(!path.exists());
    }

    #[test]
    fn reports_free_space_for_the_database_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert!(available_bytes(&dir.path().join("opencode.db")).unwrap() > 0);
    }
}
