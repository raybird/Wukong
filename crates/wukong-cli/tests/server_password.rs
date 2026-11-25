//! issue-0044 Task 3.2（SCN-017）：server 模式沒有密碼時，入口啟動就報錯並指出變數名稱。

use std::time::{Duration, Instant};

#[test]
fn server_mode_without_a_password_fails_at_once() {
    let dir = tempfile::tempdir().unwrap();
    let started = Instant::now();
    // URL 指向不會有人聽的位址：如果沒在啟動時擋下，回合會一路去連線。
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wukong"))
        .args(["--scope", "project:issue44-password", "--no-stream"])
        .arg("Reply with exactly the word OK.")
        .env(
            "WUKONG_MEMORY_DB",
            format!("sqlite://{}", dir.path().join("memory.db").display()),
        )
        .env("WUKONG_WORKSPACE", dir.path())
        .env("WUKONG_AGENT_SERVER_URL", "http://127.0.0.1:9")
        .env_remove("WUKONG_AGENT_SERVER_PASSWORD")
        .env("WUKONG_AGENT_TIMEOUT_SECS", "60")
        .current_dir(dir.path())
        .output()
        .unwrap();
    let elapsed = started.elapsed();
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{output:?}");
    assert!(stderr.contains("WUKONG_AGENT_SERVER_PASSWORD"), "{stderr}");
    assert!(elapsed < Duration::from_secs(10), "{elapsed:?}");
}
