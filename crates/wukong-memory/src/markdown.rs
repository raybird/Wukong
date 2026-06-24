//! Markdown mirror of memory. DB is the source of truth; markdown is a
//! one-way, human-readable, git-friendly derived view. Opt-in via a directory.

use crate::error::Result;
use crate::model::MemoryKind;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::PathBuf;

/// Map a scope string to a safe filename, e.g. "project:Wukong" ->
/// "project_Wukong.md". Replaces ':' and '/' with '_'.
pub fn scope_to_filename(scope: &str) -> String {
    let safe: String = scope
        .chars()
        .map(|c| {
            if c == ':' || c == '/' || c == '\\' {
                '_'
            } else {
                c
            }
        })
        .collect();
    format!("{safe}.md")
}

/// Render one memory as a markdown block. `created_at` is unix seconds.
pub fn render_markdown_entry(created_at: i64, kind: MemoryKind, text: &str) -> String {
    format!(
        "## {} · {}\n{}\n\n",
        iso8601(created_at),
        kind.as_str(),
        text
    )
}

/// Minimal UTC ISO-8601 formatter (no external date dep).
fn iso8601(unix_secs: i64) -> String {
    let days = unix_secs.div_euclid(86_400);
    let secs = unix_secs.rem_euclid(86_400);
    let (h, mi, s) = (secs / 3600, (secs % 3600) / 60, secs % 60);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// Howard Hinnant's civil_from_days.
fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// Appends memory entries to per-scope markdown files under `dir`.
#[derive(Debug, Clone)]
pub struct MarkdownSink {
    dir: PathBuf,
}

impl MarkdownSink {
    /// Create a sink writing under `dir` (created on first append).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    /// Append one entry to the scope's file. Creates dir/file as needed.
    pub fn append(&self, scope: &str, created_at: i64, kind: MemoryKind, text: &str) -> Result<()> {
        fs::create_dir_all(&self.dir)?;
        let path = self.dir.join(scope_to_filename(scope));
        let mut f = OpenOptions::new().create(true).append(true).open(path)?;
        f.write_all(render_markdown_entry(created_at, kind, text).as_bytes())?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn scope_to_filename_sanitizes() {
        assert_eq!(scope_to_filename("project:Wukong"), "project_Wukong.md");
        assert_eq!(scope_to_filename("global"), "global.md");
    }

    #[test]
    fn render_entry_has_kind_and_text() {
        let s = render_markdown_entry(0, MemoryKind::Event, "hello");
        assert!(s.contains("1970-01-01T00:00:00Z"));
        assert!(s.contains("· event"));
        assert!(s.contains("hello"));
    }

    #[test]
    fn sink_appends_to_scope_file() {
        let dir = tempdir().unwrap();
        let sink = MarkdownSink::new(dir.path());
        sink.append("project:X", 100, MemoryKind::Note, "first")
            .unwrap();
        sink.append("project:X", 200, MemoryKind::Note, "second")
            .unwrap();
        let body = std::fs::read_to_string(dir.path().join("project_X.md")).unwrap();
        assert!(body.contains("first"));
        assert!(body.contains("second"));
        // Two entries => two headers.
        assert_eq!(body.matches("## ").count(), 2);
    }
}
