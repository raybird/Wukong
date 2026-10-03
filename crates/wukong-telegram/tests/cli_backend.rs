#[path = "../../../scripts/test-support/cli_backend.rs"]
mod support;

use std::sync::{Arc, Mutex};
use wukong_memory::Memory;
use wukong_telegram::dispatch::{handle_message_with_responder, PendingQuestions};
use wukong_telegram::parse::TgMessage;
use wukong_tg_client::client::mock::MockTgClient;

// SCN-003：真實訊息 dispatch → run_turn → CLI subprocess；只替換 Telegram 網路。
#[tokio::test]
async fn telegram_dispatch_delivers_cli_final_output_and_resumes() {
    let (dir, backend, cfg) = support::fixture("unused");
    let memory = Memory::open(&cfg.db_url).await.unwrap();
    let client = MockTgClient::default();
    let pending = Arc::new(Mutex::new(PendingQuestions::new()));
    let mut session = None;
    for update_id in [1, 2] {
        handle_message_with_responder(
            &client,
            &memory,
            &cfg,
            &backend,
            &backend,
            None,
            &[7],
            pending.clone(),
            &TgMessage {
                update_id,
                chat_id: 7,
                text: "integration probe".into(),
                attachments: vec![],
            },
        )
        .await;
        let captured = memory.agent_session("user:tg-7").await.unwrap().unwrap();
        if let Some(id) = &session {
            assert_eq!(&captured, id);
        }
        session = Some(captured);
    }
    let sent = client.sent.lock().unwrap();
    let edits = client.edits.lock().unwrap();
    let delivered = sent
        .iter()
        .filter(|m| m.text.contains("CLI_INTEGRATION_OK"))
        .count()
        + edits
            .iter()
            .filter(|m| m.2.contains("CLI_INTEGRATION_OK"))
            .count();
    assert_eq!(delivered, 2);
    assert!(dir.path().join("finished").exists());
}
