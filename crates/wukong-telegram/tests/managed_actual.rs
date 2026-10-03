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
