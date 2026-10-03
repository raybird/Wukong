use std::io::Write;
use std::process::Stdio;

// SCN-010：真正 CLI binary 讀取 stdin 回答，再交由真實 OpenCode 繼續。
#[test]
#[ignore = "requires isolated OpenCode and local model fixture"]
fn cli_managed_question_accepts_answer_from_stdin() {
    let root = std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap();
    for (input, streaming, expected, prompt) in [
        ("1\n", true, "A", "QUESTION_PROBE"),
        ("自己的答案\n", false, "自己的答案", "QUESTION_PROBE"),
        ("/cancel\n", true, "QUESTION_CANCELLED", "QUESTION_PROBE"),
        ("", true, "QUESTION_CANCELLED", "QUESTION_PROBE"),
        ("\n1,2\n補充文字\n", true, "補充文字", "MULTI_PROBE"),
    ] {
        let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_wukong"));
        if !streaming {
            command.arg("--no-stream");
        }
        let mut child = command
            .args(["--scope", "project:issue7-managed", "--no-thinking", prompt])
            .env("WUKONG_MEMORY_DB", format!("sqlite://{root}/cli-memory.db"))
            .env("WUKONG_WORKSPACE", &root)
            .env("WUKONG_AGENT_CMD", "opencode run")
            .env("WUKONG_AGENT_TIMEOUT_SECS", "8")
            .env_remove("WUKONG_AGENT_SERVER_URL")
            .current_dir(&root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert!(
            String::from_utf8_lossy(&output.stdout).contains(expected),
            "{:?}",
            output
        );
        assert!(String::from_utf8_lossy(&output.stderr).contains("選哪個？"));
        if prompt == "MULTI_PROBE" {
            assert!(String::from_utf8_lossy(&output.stdout).contains("A, B"));
            assert!(String::from_utf8_lossy(&output.stderr).contains("請輸入有效選項"));
        }
    }
}
