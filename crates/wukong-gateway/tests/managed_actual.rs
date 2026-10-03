use std::collections::BTreeMap;
use std::path::PathBuf;
use wukong_gateway::backend::{build_backend_from_env, AgentRequest, AiBackend};
use wukong_gateway::StreamEvent;

// SCN-010／007：必須由 probe-managed.py 的隔離真實 OpenCode 環境執行。
#[tokio::test]
#[ignore = "requires isolated OpenCode and local model fixture"]
async fn managed_actual_question_reply_and_session_resume() {
    let root = PathBuf::from(std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap());
    let backend = build_backend_from_env(vec!["opencode".into(), "run".into()], Some(root));
    let mut session = None;
    for answer in [Some("A"), None, Some("自己的答案")] {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut observed = 0;
        let mut sink = |event| {
            if let StreamEvent::QuestionRequest(question) = event {
                observed += 1;
                tx.send(question).unwrap();
            }
        };
        let run = backend.run_streaming(
            AgentRequest {
                prompt: "問我問題".into(),
                session_id: session.clone(),
                thinking: false,
                model: None,
                agent: None,
                tool_overrides: BTreeMap::new(),
                attachments: vec![],
            },
            &mut sink,
        );
        let reply = async {
            let question = rx.recv().await.unwrap();
            assert_eq!(question.questions[0].question, "選哪個？");
            assert!(backend
                .answer_question(
                    "wrong-session",
                    &question.request_id,
                    vec![vec!["B".into()]]
                )
                .await
                .is_err());
            if let Some(answer) = answer {
                backend
                    .answer_question(
                        &question.session_id,
                        &question.request_id,
                        vec![vec![answer.into()]],
                    )
                    .await
                    .unwrap();
            } else {
                backend
                    .cancel_question(&question.session_id, &question.request_id)
                    .await
                    .unwrap();
            }
        };
        let response = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            tokio::pin!(run);
            tokio::select! {
                result = &mut run => {
                    assert!(result.is_ok(), "run failed: {result:?}");
                    panic!("回合未等待使用者回覆便結束");
                }
                _ = reply => {}
            }
            run.await.unwrap()
        })
        .await;
        let response = response.expect("問答未完成");
        assert_eq!(observed, 1);
        assert!(session.is_none() || session == response.session_id);
        if let Some(answer) = answer {
            assert!(response.text.contains(answer), "{}", response.text);
            assert_eq!(
                response.text.contains("HISTORY_PRESENT"),
                session.is_some(),
                "provider must observe stored conversation history: {}",
                response.text
            );
        }
        session = response.session_id;
    }
}

#[tokio::test]
#[ignore = "requires isolated OpenCode and local model fixture"]
async fn managed_native_compact_delete_and_unattended_permission() {
    let root = PathBuf::from(std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap());
    let backend = build_backend_from_env(vec!["opencode".into(), "run".into()], Some(root.clone()));
    let marker = root.join("permission-marker");
    let _ = std::fs::remove_file(&marker);
    let response = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        backend.run(AgentRequest {
            prompt: "PERMISSION_PROBE".into(),
            session_id: None,
            thinking: false,
            model: None,
            agent: None,
            tool_overrides: BTreeMap::new(),
            attachments: vec![],
        }),
    )
    .await
    .unwrap()
    .unwrap();
    assert!(!marker.exists(), "unattended run must reject permission");
    let id = response.session_id.unwrap();
    let compact = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        backend.compact_session(&id, Some("probe/probe")),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(compact.session_id.as_deref(), Some(id.as_str()));
    assert!(
        compact.text.is_empty(),
        "native compact should not submit /compact as user text"
    );
    backend.delete_session(&id).await.unwrap();
    let result = backend
        .run(AgentRequest {
            prompt: "probe".into(),
            session_id: Some(id.clone()),
            thinking: false,
            model: None,
            agent: None,
            tool_overrides: BTreeMap::new(),
            attachments: vec![],
        })
        .await;
    assert_ne!(
        result.unwrap().session_id.as_deref(),
        Some(id.as_str()),
        "deleted session must not resume"
    );
}

#[tokio::test]
#[ignore = "requires isolated OpenCode and local model fixture"]
async fn managed_pending_turn_drop_invalidates_reply() {
    let root = PathBuf::from(std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap());
    let backend = build_backend_from_env(vec!["opencode".into(), "run".into()], Some(root));
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    let mut sink = |event| {
        if let StreamEvent::QuestionRequest(q) = event {
            tx.send(q).unwrap();
        }
    };
    let run = backend.run_streaming(
        AgentRequest {
            prompt: "QUESTION_PROBE".into(),
            session_id: None,
            thinking: false,
            model: None,
            agent: None,
            tool_overrides: BTreeMap::new(),
            attachments: vec![],
        },
        &mut sink,
    );
    let question = {
        tokio::pin!(run);
        tokio::select! { _ = &mut run => panic!("turn ended before question"), question = rx.recv() => question.unwrap() }
    };
    assert!(backend
        .answer_question(
            &question.session_id,
            &question.request_id,
            vec![vec!["A".into()]]
        )
        .await
        .is_err());
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
}

#[tokio::test]
#[ignore = "requires isolated OpenCode and local model fixture"]
async fn managed_ephemeral_returns_no_resumable_session() {
    let root = PathBuf::from(std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap());
    let backend = build_backend_from_env(vec!["opencode".into(), "run".into()], Some(root));
    let request = AgentRequest {
        prompt: "SERVER_PROBE".into(),
        session_id: None,
        thinking: false,
        model: None,
        agent: None,
        tool_overrides: BTreeMap::new(),
        attachments: vec![],
    };
    let plain = backend.run_ephemeral(request.clone()).await.unwrap();
    assert!(
        plain.session_id.is_none(),
        "deleted helper session must not be exposed"
    );
    let streaming = backend
        .run_streaming_ephemeral(request, &mut |_| {})
        .await
        .unwrap();
    assert!(
        streaming.session_id.is_none(),
        "deleted helper session must not be exposed"
    );
}

#[tokio::test]
#[ignore = "requires isolated OpenCode and local model fixture"]
async fn managed_concurrent_questions_use_distinct_sessions_and_routes() {
    let root = PathBuf::from(std::env::var("WUKONG_ISSUE7_MANAGED_ROOT").unwrap());
    let backend = build_backend_from_env(vec!["opencode".into(), "run".into()], Some(root));
    let request = AgentRequest {
        prompt: "QUESTION_PROBE".into(),
        session_id: None,
        thinking: false,
        model: None,
        agent: None,
        tool_overrides: BTreeMap::new(),
        attachments: vec![],
    };
    let (tx_a, mut rx_a) = tokio::sync::mpsc::unbounded_channel();
    let (tx_b, mut rx_b) = tokio::sync::mpsc::unbounded_channel();
    let mut sink_a = |event| {
        if let StreamEvent::QuestionRequest(q) = event {
            tx_a.send(q).unwrap();
        }
    };
    let mut sink_b = |event| {
        if let StreamEvent::QuestionRequest(q) = event {
            tx_b.send(q).unwrap();
        }
    };
    let replies = async {
        let a = rx_a.recv().await.unwrap();
        let b = rx_b.recv().await.unwrap();
        assert_ne!(a.session_id, b.session_id);
        assert!(backend
            .answer_question(&a.session_id, &b.request_id, vec![vec!["B".into()]])
            .await
            .is_err());
        backend
            .answer_question(&a.session_id, &a.request_id, vec![vec!["A".into()]])
            .await
            .unwrap();
        backend
            .answer_question(&b.session_id, &b.request_id, vec![vec!["B".into()]])
            .await
            .unwrap();
    };
    let (a, b, ()) = tokio::time::timeout(std::time::Duration::from_secs(20), async {
        tokio::join!(
            backend.run_streaming(request.clone(), &mut sink_a),
            backend.run_streaming(request, &mut sink_b),
            replies
        )
    })
    .await
    .unwrap();
    let a = a.unwrap();
    let b = b.unwrap();
    assert_ne!(a.session_id, b.session_id);
    assert!(a.text.contains("\\\"A\\\""), "{}", a.text);
    assert!(b.text.contains("\\\"B\\\""), "{}", b.text);
}
