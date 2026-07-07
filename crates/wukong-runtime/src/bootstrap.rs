//! Environment-driven Memory construction shared by every entrypoint.
//!
//! Each surface (CLI, Web, Telegram, scheduler daemon) previously inlined the
//! same three-step chain: open the store, optionally attach the embedder (when
//! built with the `embed` feature and `WUKONG_EMBED=1`), and optionally mirror
//! to markdown (`WUKONG_MD_DIR`). This centralizes it so the semantic layer and
//! markdown wiring stay in lockstep across all of them.

use wukong_memory::Memory;

/// Open [`Memory`] from a resolved `db_url`, attaching the embedder and markdown
/// mirror per environment.
///
/// - When compiled with the `embed` feature and `WUKONG_EMBED=1`, a
///   `FastembedBackend` is attached; a load failure downgrades to keyword-only
///   recall with a warning rather than aborting.
/// - When `WUKONG_MD_DIR` is a non-empty path, memory is mirrored to markdown.
pub async fn open_memory_from_env(db_url: &str) -> Result<Memory, String> {
    let memory = Memory::open(db_url).await.map_err(|e| e.to_string())?;

    #[cfg(feature = "embed")]
    let memory = if std::env::var("WUKONG_EMBED").as_deref() == Ok("1") {
        match wukong_memory::FastembedBackend::new() {
            Ok(backend) => memory.with_embedder(std::sync::Arc::new(backend)),
            Err(e) => {
                eprintln!("🐵 語意層停用（模型載入失敗）：{e}");
                memory
            }
        }
    } else {
        memory
    };

    let memory = match std::env::var("WUKONG_MD_DIR") {
        Ok(dir) if !dir.is_empty() => memory.with_markdown(dir),
        _ => memory,
    };
    Ok(memory)
}
