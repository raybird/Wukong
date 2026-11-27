#[path = "../../../scripts/test-support/managed_actual.rs"]
mod support;
use std::sync::{Arc, Mutex};
use wukong_memory::Memory;
use wukong_telegram::dispatch::{
    handle_callback_query, handle_message_with_responder, question_callback_data, PendingQuestions,
    QuestionAction,
};
use wukong_telegram::parse::{TgCallbackQuery, TgMessage};
use wukong_tg_client::client::mock::MockTgClient;

// SCN-010：訊息 dispatch → 問答按鈕 → 真實 OpenCode；只替換外部 Telegram transport。
#[tokio::test]
#[ignore = "requires isolated OpenCode and local model fixture"]
async fn telegram_managed_question_can_be_answered_by_callback() {
    let (_dir, backend, cfg) = support::fixture("unused");
    let memory = Memory::open(&cfg.db_url).await.unwrap();
    let client = MockTgClient::default();
    let pending = Arc::new(Mutex::new(PendingQuestions::new()));
    let message = TgMessage {
        update_id: 1,
        chat_id: 7,
        text: "QUESTION_PROBE".into(),
        attachments: vec![],
    };
    let turn = handle_message_with_responder(
        &client,
        &memory,
        &cfg,
        &backend,
        &backend,
        None,
        &[7],
        pending.clone(),
        &message,
    );
    let reply = async {
        let question = loop {
            if let Some(question) = pending.lock().unwrap().get(&7).cloned() {
                break question;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        };
        let callback = TgCallbackQuery {
            update_id: 2,
            callback_query_id: "probe".into(),
            chat_id: 7,
            message_id: question.message_id.unwrap(),
            data: question_callback_data(
                &question.request_id,
                QuestionAction::Pick {
                    question: 0,
                    option: 0,
                },
            ),
        };
        handle_callback_query(&client, &backend, &[7], pending.clone(), &callback).await;
    };
    tokio::time::timeout(std::time::Duration::from_secs(20), async {
        tokio::join!(turn, reply);
    })
    .await
    .unwrap();
    let delivered = client
        .sent
        .lock()
        .unwrap()
        .iter()
        .any(|m| m.text.contains("User has answered your questions"))
        || client
            .edits
            .lock()
            .unwrap()
            .iter()
            .any(|m| m.2.contains("User has answered your questions"));
    assert!(delivered);
    assert!(!pending.lock().unwrap().contains_key(&7));
}

// #45 SCN-002：真實的 V2 server 派工給 fixer 時，狀態泡泡寫出「悟空·fixer」，最後送出回覆。
#[tokio::test]
#[ignore = "requires a resident OpenCode V2 server and a real model"]
async fn telegram_names_the_dispatched_subagent() {
    let (_dir, backend, cfg) = support::server_fixture("user:tg-45");
    let workspace = std::env::var("WUKONG_WORKSPACE").expect("set WUKONG_WORKSPACE");
    wukong_runtime::agent_assets::materialize_agent_definitions(std::path::Path::new(&workspace))
        .unwrap();
    let memory = Memory::open(&cfg.db_url).await.unwrap();
    let client = MockTgClient::default();
    let message = TgMessage {
        update_id: 1,
        chat_id: 45,
        text: support::DISPATCH_PROMPT.into(),
        attachments: vec![],
    };
    tokio::time::timeout(
        std::time::Duration::from_secs(300),
        handle_message_with_responder(
            &client,
            &memory,
            &cfg,
            &backend,
            &backend,
            None,
            &[45],
            Arc::new(Mutex::new(PendingQuestions::new())),
            &message,
        ),
    )
    .await
    .unwrap();
    let edits = client.edits.lock().unwrap().clone();
    let sent = client.sent.lock().unwrap().clone();
    println!(
        "edits: {:?}",
        edits.iter().map(|e| &e.2).collect::<Vec<_>>()
    );
    println!(
        "sent: {:?}",
        sent.iter().map(|m| &m.text).collect::<Vec<_>>()
    );
    assert!(
        edits.iter().any(|e| e.2.contains("悟空·fixer")),
        "{edits:?}"
    );
    assert!(
        sent.iter().any(|m| m.text.contains("hello from wukong")),
        "{sent:?}"
    );
}
