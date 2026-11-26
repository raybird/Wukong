//! issue-0044 Task 2.4（SCN-002）與 Task 2.5（SCN-006）：本機按需模式接真實的 OpenCode V2 2.3.2。
//!
//! 由 `docs/issues/issue-0044/scripts/local-v2-actual.sh` 在隔離的 HOME 與 workspace 執行：
//! `WUKONG_V2_OPENCODE` 為 V2 的 opencode 執行檔，`WUKONG_V2_ROOT` 為空的 workspace，
//! `WUKONG_V2_MODEL` 為要指定的模型（`provider/model`）。只送固定的合成提示詞。
#![cfg(target_os = "linux")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use wukong_gateway::backend::{
    AgentAttachment, AgentBackend, AgentRequest, AgentResponse, AiBackend,
};
use wukong_gateway::local_process::LocalProcessBackend;
use wukong_gateway::stream::PERMISSION_ALLOW_ONCE_LABEL;
use wukong_gateway::{GatewayError, StreamEvent};

fn request(prompt: &str, session_id: Option<String>) -> AgentRequest {
    AgentRequest {
        prompt: prompt.into(),
        session_id,
        thinking: true,
        model: None,
        agent: None,
        tool_overrides: BTreeMap::new(),
        attachments: vec![],
        session_instructions: Default::default(),
    }
}

/// 命令列含 V2 執行檔路徑的程序：(pid, 命令列)。
fn opencode_processes(binary: &Path) -> Vec<(String, String)> {
    let needle = binary.display().to_string();
    let mut found = Vec::new();
    for entry in std::fs::read_dir("/proc").unwrap().flatten() {
        let pid = entry.file_name().to_string_lossy().into_owned();
        if !pid.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(raw) = std::fs::read(entry.path().join("cmdline")) else {
            continue;
        };
        let cmdline = String::from_utf8_lossy(&raw).replace('\0', " ");
        if cmdline.contains(&needle) {
            found.push((pid, cmdline));
        }
    }
    found
}

async fn assert_no_leftover(binary: &Path, after: &str) {
    for _ in 0..50 {
        if opencode_processes(binary).is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
    panic!(
        "opencode left running after {after}: {:?}",
        opencode_processes(binary)
    );
}

/// 回合進行中：serve 程序存在，命令列不含它環境變數裡的密碼。
fn assert_password_not_in_argv(binary: &Path) -> usize {
    let processes = opencode_processes(binary);
    for (pid, cmdline) in &processes {
        let environ = std::fs::read(format!("/proc/{pid}/environ")).unwrap_or_default();
        let password = String::from_utf8_lossy(&environ)
            .split('\0')
            .find_map(|pair| pair.strip_prefix("OPENCODE_SERVER_PASSWORD="))
            .map(str::to_string);
        if let Some(password) = password {
            assert!(password.len() >= 32, "password too short");
            assert!(!cmdline.contains(&password), "password in argv of {pid}");
        }
    }
    processes.len()
}

#[tokio::test]
#[ignore = "requires OpenCode V2 2.3.2 in an isolated HOME (scripts/local-v2-actual.sh)"]
async fn local_on_demand_turns_against_real_v2() {
    let binary = PathBuf::from(std::env::var("WUKONG_V2_OPENCODE").unwrap());
    let root = PathBuf::from(std::env::var("WUKONG_V2_ROOT").unwrap());
    let model = std::env::var("WUKONG_V2_MODEL").unwrap();
    let backend = LocalProcessBackend::new(binary.display().to_string(), Some(root.clone()));
    assert_no_leftover(&binary, "start").await;

    // 1. 單棒回合，串流中看得到步驟、思考與工具；回合中檢查 serve 的命令列。
    let mut events = Vec::new();
    let mut seen_processes = 0;
    let first = backend
        .run_streaming(
            request(
                "Run the shell command `echo probe` once, then reply with exactly the word DONE.",
                None,
            ),
            &mut |event| {
                if seen_processes == 0 {
                    seen_processes = assert_password_not_in_argv(&binary);
                }
                events.push(event);
            },
        )
        .await
        .unwrap();
    println!("turn 1: {:?} events={}", first.text, events.len());
    assert!(first.text.contains("DONE"), "{:?}", first.text);
    let session = first.session_id.clone().expect("session recorded");
    assert!(seen_processes > 0, "no serve process seen during the turn");
    assert!(events.contains(&StreamEvent::StepStart));
    assert!(
        events.iter().any(
            |event| matches!(event, StreamEvent::ToolUse(tool) if tool.contains("echo probe"))
        ),
        "{events:?}"
    );
    assert_no_leftover(&binary, "turn 1").await;

    // 2. 下一回合接續同一個 session。
    let second = backend
        .run_streaming(
            request(
                "Which single word did you reply with in your previous answer? Answer with that word only.",
                Some(session.clone()),
            ),
            &mut |_| {},
        )
        .await
        .unwrap();
    println!("turn 2: {:?}", second.text);
    assert_eq!(second.session_id.as_deref(), Some(session.as_str()));
    assert!(second.text.contains("DONE"), "{:?}", second.text);
    assert_no_leftover(&binary, "turn 2").await;

    // 3. 帶附件的回合（本機模式走 file:// URI）。
    let upload = root.join(".wukong/uploads/probe");
    std::fs::create_dir_all(&upload).unwrap();
    let attachment = upload.join("secret.txt");
    std::fs::write(&attachment, "The secret word is PINEAPPLE42.\n").unwrap();
    let mut with_file = request(
        "Reply with only the secret word written in the attached file.",
        None,
    );
    with_file.attachments = vec![AgentAttachment {
        path: attachment,
        original_name: "secret.txt".into(),
        mime_type: Some("text/plain".into()),
    }];
    let third = backend.run_streaming(with_file, &mut |_| {}).await.unwrap();
    println!("turn 3: {:?}", third.text);
    assert!(third.text.contains("PINEAPPLE42"), "{:?}", third.text);
    assert_no_leftover(&binary, "turn 3").await;

    // 4. 指定模型的回合。
    let mut with_model = request("Reply with exactly the word MODEL.", None);
    with_model.model = Some(model.clone());
    let fourth = backend.run(with_model).await.unwrap();
    println!("turn 4 ({model}): {:?}", fourth.text);
    assert!(fourth.text.contains("MODEL"), "{:?}", fourth.text);
    assert_no_leftover(&binary, "turn 4").await;

    // 5. 規劃與路由用的暫時回合：不回傳 session，事後刪除。
    let before = backend.list_sessions().await.unwrap().sessions.len();
    let helper = backend
        .run_ephemeral(request("Reply with exactly the word HELPER.", None))
        .await
        .unwrap();
    assert!(helper.text.contains("HELPER"), "{:?}", helper.text);
    assert_eq!(helper.session_id, None);
    assert_eq!(
        backend.list_sessions().await.unwrap().sessions.len(),
        before
    );

    // 6. 壓縮與刪除，之後都沒有殘留程序。
    backend.compact_session(&session, None).await.unwrap();
    backend.delete_session(&session).await.unwrap();
    let remaining = backend.list_sessions().await.unwrap();
    assert!(!remaining.sessions.iter().any(|s| s.id == session));
    assert_no_leftover(&binary, "compact and delete").await;
}

enum Reply {
    Answer(&'static str),
    Cancel,
}

/// 跑一個互動回合：第一個提問出現時經 `AgentBackend`（Telegram、Web 與排程共用的分派）
/// 回覆。回傳回合結果，以及有沒有出現提問。
async fn turn_with_reply(
    backend: &AgentBackend,
    prompt: &str,
    reply: Reply,
) -> (Result<AgentResponse, GatewayError>, bool) {
    let mut req = request(prompt, None);
    req.model = Some(std::env::var("WUKONG_V2_MODEL").unwrap());
    let (questions, mut asked) = tokio::sync::mpsc::unbounded_channel();
    let mut sink = move |event| {
        if let StreamEvent::QuestionRequest(question) = event {
            let _ = questions.send(question);
        }
    };
    let turn = backend.run_streaming(req, &mut sink);
    tokio::pin!(turn);
    let mut replied = false;
    let result = loop {
        tokio::select! {
            result = &mut turn => break result,
            Some(question) = asked.recv(), if !replied => {
                println!("  asked: {:?} {:?}", question.request_id, question.questions);
                replied = true;
                let sent = match reply {
                    Reply::Answer(text) => {
                        backend
                            .answer_question(
                                &question.session_id,
                                &question.request_id,
                                vec![vec![text.to_string()]],
                            )
                            .await
                    }
                    Reply::Cancel => {
                        backend
                            .cancel_question(&question.session_id, &question.request_id)
                            .await
                    }
                };
                sent.unwrap();
            }
        }
    };
    (result, replied)
}

/// 本測試期間讓按需啟動的 serve 把 shell 設成詢問；結束（含失敗）時移除。
struct ShellAsks;

impl ShellAsks {
    fn on() -> Self {
        std::env::set_var(
            "OPENCODE_CONFIG_CONTENT",
            r#"{"permission":{"shell":"ask"}}"#,
        );
        Self
    }
}

impl Drop for ShellAsks {
    fn drop(&mut self) {
        std::env::remove_var("OPENCODE_CONFIG_CONTENT");
    }
}

// issue-0044 Task 2.5（SCN-006）：提問與權限的回答、自訂回答與取消，送到真實 V2 後回合繼續或結束。
#[tokio::test]
#[ignore = "requires OpenCode V2 2.3.2 in an isolated HOME (scripts/local-v2-actual.sh)"]
async fn local_question_and_permission_replies_against_real_v2() {
    let binary = PathBuf::from(std::env::var("WUKONG_V2_OPENCODE").unwrap());
    let root = PathBuf::from(std::env::var("WUKONG_V2_ROOT").unwrap());
    let backend = AgentBackend::Local(LocalProcessBackend::new(
        binary.display().to_string(),
        Some(root.clone()),
    ));
    let ask = "Use the question tool to ask me to choose between the options A and B. \
               After I answer, reply with exactly what I answered, or with the word NONE if I gave no answer.";

    // 1. 選一個選項（入口送的是選項標籤）。
    let (result, asked) = turn_with_reply(&backend, ask, Reply::Answer("B")).await;
    println!("answer: {result:?}");
    assert!(asked);
    assert!(result.unwrap().text.contains('B'));
    assert_no_leftover(&binary, "answer").await;

    // 2. 自訂回答。
    let (result, asked) = turn_with_reply(&backend, ask, Reply::Answer("CUSTOM-42")).await;
    println!("custom: {result:?}");
    assert!(asked);
    assert!(result.unwrap().text.contains("CUSTOM-42"));
    assert_no_leftover(&binary, "custom answer").await;

    // 3. 取消提問：回合結束，不回報成錯誤（與 V1 相同）。
    let (result, asked) = turn_with_reply(&backend, ask, Reply::Cancel).await;
    println!("cancel: {result:?}");
    assert!(asked);
    result.unwrap();
    assert_no_leftover(&binary, "cancel").await;

    // 4、5. shell 設成詢問：允許一次後完成；拒絕後回合結束、不回報成錯誤。
    let _asks = ShellAsks::on();
    let run =
        "Use the shell tool to run exactly: echo probe. Then reply with exactly the word DONE.";
    let (result, asked) =
        turn_with_reply(&backend, run, Reply::Answer(PERMISSION_ALLOW_ONCE_LABEL)).await;
    println!("permission once: {result:?}");
    assert!(asked);
    assert!(result.unwrap().text.contains("DONE"));
    assert_no_leftover(&binary, "permission once").await;

    let (result, asked) = turn_with_reply(&backend, run, Reply::Cancel).await;
    println!("permission reject: {result:?}");
    assert!(asked);
    result.unwrap();
    assert_no_leftover(&binary, "permission reject").await;
}
