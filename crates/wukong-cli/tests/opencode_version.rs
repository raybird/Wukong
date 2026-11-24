//! issue-0044 Task 2.7（SCN-011）：PATH 上的 opencode 是 V1 時，回合立即失敗並指出版本。
#![cfg(unix)]

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::time::{Duration, Instant};

/// 假 opencode：`--version` 印出 `$FAKE_OPENCODE_VERSION`；其他呼叫記到 `calls`，
/// `run --format json` 回一個帶 session 的最小 V2 回合。
const FAKE_OPENCODE: &str = r#"#!/bin/sh
if [ "$1" = --version ]; then
    echo "$FAKE_OPENCODE_VERSION"
    exit 0
fi
echo "$*" >> "$(dirname "$0")/calls"
for arg in "$@"; do
    if [ "$arg" = json ]; then
        printf '{"type":"step_start","sessionID":"ses_v2"}\n'
        printf '{"type":"text","sessionID":"ses_v2","part":{"type":"text","text":"V2_OK"}}\n'
        printf '{"type":"step_finish","sessionID":"ses_v2"}\n'
        exit 0
    fi
done
printf 'fixer|none\n'
"#;

fn write_fake_opencode(dir: &Path) {
    let path = dir.join("opencode");
    std::fs::write(&path, FAKE_OPENCODE).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

fn run_wukong(dir: &Path, agent_cmd: &str, version: &str) -> (std::process::Output, Duration) {
    let started = Instant::now();
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wukong"))
        .args([
            "--scope",
            "project:issue44-version",
            "--no-thinking",
            "--no-stream",
        ])
        .arg("Reply with exactly the word OK.")
        .env(
            "WUKONG_MEMORY_DB",
            format!("sqlite://{}", dir.join("memory.db").display()),
        )
        .env("WUKONG_WORKSPACE", dir)
        .env("WUKONG_AGENT_CMD", agent_cmd)
        .env("FAKE_OPENCODE_VERSION", version)
        .env("WUKONG_AGENT_TIMEOUT_SECS", "60")
        .env_remove("WUKONG_AGENT_SERVER_URL")
        .current_dir(dir)
        .output()
        .unwrap();
    (output, started.elapsed())
}

// 同一個測試裡依序跑，避免平行測試 fork 時撞上剛寫好的腳本（ETXTBSY）。
#[test]
fn v1_opencode_fails_the_turn_at_once_and_v2_passes() {
    let dir = tempfile::tempdir().unwrap();
    write_fake_opencode(dir.path());
    let opencode = dir.path().join("opencode").display().to_string();

    // `opencode run` 走本機按需模式；多一個旗標走 CLI backend。兩條都要擋下 V1。
    for agent_cmd in [
        format!("{opencode} run"),
        format!("{opencode} run --dangerously-skip-permissions"),
    ] {
        let (output, elapsed) = run_wukong(dir.path(), &agent_cmd, "1.19.6");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "{agent_cmd}: {output:?}");
        assert!(stderr.contains("需要 OpenCode V2"), "{agent_cmd}: {stderr}");
        assert!(stderr.contains("1.19.6"), "{agent_cmd}: {stderr}");
        assert!(
            elapsed < Duration::from_secs(10),
            "{agent_cmd}: {elapsed:?}"
        );
        assert!(
            !dir.path().join("calls").exists(),
            "{agent_cmd}: V1 opencode must not be run: {}",
            std::fs::read_to_string(dir.path().join("calls")).unwrap_or_default()
        );
    }

    // V2 照常完成。
    let (output, _) = run_wukong(
        dir.path(),
        &format!("{opencode} run --dangerously-skip-permissions"),
        // V2 2.3.2 的實際輸出。
        "opencode v2.3.2",
    );
    assert!(output.status.success(), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("V2_OK"),
        "{output:?}"
    );
}
