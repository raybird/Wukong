//! 第一次呼叫 opencode 前確認它是 V2（issue-0044 Task 2.7，SCN-011）。
//!
//! Wukong 只支援 OpenCode V2。PATH 上留著 V1 時，本機模式會等不到 V2 的啟動字串，CLI 模式會被
//! V2 才有的旗標弄壞，錯誤訊息都看不出原因，所以在第一次呼叫前以 `--version` 確認。
//! 只有明確回報 1.x 才擋：執行不了、逾時或輸出看不懂時照常呼叫，讓原本的錯誤浮出來。

use std::collections::HashSet;
use std::sync::Mutex;
use std::time::Duration;

use crate::error::GatewayError;

/// 已確認是 V2 的執行檔。只記成功的結果，換版後不必重啟就能通過。
static CONFIRMED: Mutex<Option<HashSet<String>>> = Mutex::new(None);

const VERSION_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) async fn ensure_v2(binary: &str) -> Result<(), GatewayError> {
    if CONFIRMED
        .lock()
        .unwrap()
        .as_ref()
        .is_some_and(|confirmed| confirmed.contains(binary))
    {
        return Ok(());
    }
    let output = tokio::time::timeout(
        VERSION_TIMEOUT,
        tokio::process::Command::new(binary)
            .arg("--version")
            .stdin(std::process::Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await;
    let Ok(Ok(output)) = output else {
        return Ok(());
    };
    if !output.status.success() {
        return Ok(());
    }
    let reported = String::from_utf8_lossy(&output.stdout).trim().to_string();
    match major_version(&reported) {
        Some(major) if major < 2 => Err(GatewayError::AgentFailed {
            code: None,
            stderr: format!(
                "需要 OpenCode V2（2.x），{binary} 回報的版本是 {reported}。請改裝 V2，例如 `npm install -g @opencode/cli@2.3.2`。"
            ),
        }),
        Some(_) => {
            CONFIRMED
                .lock()
                .unwrap()
                .get_or_insert_with(HashSet::new)
                .insert(binary.to_string());
            Ok(())
        }
        None => Ok(()),
    }
}

/// `--version` 輸出的主版號：取最後一行的最後一段（V1 印 `1.19.6`，V2 印 `opencode v2.3.2`）。
fn major_version(reported: &str) -> Option<u64> {
    let token = reported.lines().last()?.split_whitespace().last()?;
    let version = token.strip_prefix('v').unwrap_or(token);
    let (major, rest) = version.split_once('.')?;
    if rest.is_empty() || !rest.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    major.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_major_version_of_real_outputs() {
        // V1 1.19.6 與 V2 2.3.2 的 `opencode --version` 實際輸出（2026-11-24）。
        assert_eq!(major_version("1.19.6"), Some(1));
        assert_eq!(major_version("opencode v2.3.2"), Some(2));
        assert_eq!(major_version("2.3.2"), Some(2));
        assert_eq!(major_version("v2.3.2"), Some(2));
        assert_eq!(major_version("sh: 0: Illegal option --"), None);
        assert_eq!(major_version(""), None);
    }
}
