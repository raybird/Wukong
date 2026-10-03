#[path = "../../../scripts/test-support/cli_backend.rs"]
mod support;

use wukong_memory::Memory;

// SCN-002／003：真正 CLI binary → run_turn → CLI subprocess。
#[tokio::test]
async fn cli_binary_resumes_only_the_matching_scope() {
    let (dir, _, cfg) = support::fixture("project:issue7-cli");
    let memory = Memory::open(&cfg.db_url).await.unwrap();
    let mut first = None;
    for scope in [&cfg.scope, &cfg.scope, "project:other"] {
        let output = std::process::Command::new(env!("CARGO_BIN_EXE_wukong"))
            .args(["--scope", scope, "--no-thinking", "integration probe"])
            .env("WUKONG_MEMORY_DB", &cfg.db_url)
            .env("WUKONG_WORKSPACE", dir.path())
            .env("WUKONG_AGENT_CMD", cfg.agent_command.join(" "))
            .env_remove("WUKONG_AGENT_SERVER_URL")
            .current_dir(dir.path())
            .output()
            .unwrap();
        assert!(output.status.success(), "{:?}", output);
        assert_eq!(
            String::from_utf8_lossy(&output.stdout)
                .matches("CLI_INTEGRATION_OK")
                .count(),
            1
        );
        let captured = memory.agent_session(scope).await.unwrap().unwrap();
        if scope == cfg.scope {
            match &first {
                Some(id) => assert_eq!(&captured, id),
                None => first = Some(captured),
            }
        } else {
            assert_ne!(Some(captured), first);
        }
    }
    let prompts = std::fs::read_to_string(dir.path().join("argv")).unwrap();
    assert!(
        prompts.contains("CLI_INTEGRATION_OK"),
        "previous answer must enter recalled prompt"
    );
    let calls = std::fs::read_to_string(dir.path().join("sessions")).unwrap();
    assert_eq!(calls.lines().count(), 3);
    assert_eq!(
        calls.lines().nth(1).unwrap(),
        format!("resumed={}", first.unwrap())
    );
    assert_eq!(calls.lines().nth(2).unwrap(), "resumed=");
}

// SCN-005：CLI 回合寫入記憶並召回；全空輸出仍有 sentinel。
#[tokio::test]
async fn cli_empty_output_fallback_is_remembered_and_recalled() {
    let (dir, _, cfg) = support::fixture("project:issue7-empty");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_wukong"))
        .args(["--scope", &cfg.scope, "--no-thinking", "EMPTY_OUTPUT_PROBE"])
        .env("WUKONG_MEMORY_DB", &cfg.db_url)
        .env("WUKONG_WORKSPACE", dir.path())
        .env("WUKONG_AGENT_CMD", cfg.agent_command.join(" "))
        .env_remove("WUKONG_AGENT_SERVER_URL")
        .current_dir(dir.path())
        .output()
        .unwrap();
    assert!(output.status.success(), "{:?}", output);
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("(本回合未產生文字輸出)"),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let memory = Memory::open(&cfg.db_url).await.unwrap();
    let recalled = memory
        .recall(wukong_memory::RecallQuery {
            query: "本回合未產生文字輸出".into(),
            top_k: 5,
            scope: Some(cfg.scope),
            mode: wukong_memory::RecallMode::Keyword,
        })
        .await
        .unwrap();
    assert!(recalled
        .data
        .iter()
        .any(|hit| hit.text.contains("(本回合未產生文字輸出)")));
    let prompts = std::fs::read_to_string(dir.path().join("argv")).unwrap();
    assert!(prompts.contains("[輸出要求]"));
    assert!(prompts.contains("EMPTY_OUTPUT_PROBE"));
}
