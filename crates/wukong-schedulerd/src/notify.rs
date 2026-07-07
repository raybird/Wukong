//! Deliver scheduled Turn results back to the originating Telegram chat.
//!
//! A Telegram-created schedule carries a scope of `user:tg-<chat_id>` (see
//! `wukong_tg_client::parse::scope_for_chat`), so the chat id is recovered from
//! the job scope — no schema change is needed. Jobs with non-Telegram scopes
//! (e.g. `project:X` created from the CLI) are never delivered to a chat.

use wukong_chat_history::ChatHistoryStore;
use wukong_runtime::util::now_unix;
use wukong_scheduler::{ExecutionOutput, Job, JobKind};
use wukong_tg_client::client::TgClient;
use wukong_tg_client::error::TgError;
use wukong_tg_client::parse::chat_id_from_scope;

/// One-line, length-bounded summary of a failure message (the user opted for a
/// brief failure notice rather than the full error dump).
fn failure_summary(message: &str) -> String {
    let line = message
        .lines()
        .find(|l| !l.trim().is_empty())
        .unwrap_or("")
        .trim();
    line.chars().take(300).collect()
}

async fn record_history(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    content: &str,
    status: &str,
) {
    let Some(history) = history else {
        return;
    };
    match history.default_thread(scope).await {
        Ok(thread) => {
            let html = wukong_render::to_web_html(content);
            if let Err(e) = history
                .insert_message(
                    &thread,
                    "assistant",
                    content,
                    Some(&html),
                    status,
                    now_unix(),
                )
                .await
            {
                eprintln!("warning: scheduler chat history insert failed: {e}");
            }
        }
        Err(e) => eprintln!("warning: scheduler chat history thread failed: {e}"),
    }
}

/// If `job` is a Telegram-originated Turn job, deliver its result to the chat.
///
/// Returns `Ok(true)` when a message was sent, `Ok(false)` when delivery does
/// not apply (maintenance job, or a non-Telegram scope). Network/API errors are
/// surfaced to the caller, which should log them without failing the job —
/// the job already ran and its outcome is recorded in `scheduler_runs`.
pub async fn notify_turn_result<C: TgClient + Sync>(
    client: &C,
    job: &Job,
    output: &ExecutionOutput,
) -> Result<bool, TgError> {
    let JobKind::Turn { scope, .. } = &job.kind else {
        return Ok(false);
    };
    let Some(chat_id) = chat_id_from_scope(scope) else {
        return Ok(false);
    };

    if output.success {
        let chunks = if output.message.trim().is_empty() {
            Vec::new()
        } else {
            wukong_render::to_telegram_html(&format!("⏰ {}\n\n{}", job.name, output.message))
        };
        if chunks.is_empty() {
            client
                .send_message(chat_id, &format!("⏰ {}（無內容）", job.name))
                .await?;
        } else {
            for c in &chunks {
                client.send_message_html(chat_id, c).await?;
            }
        }
    } else {
        // Plain text so an arbitrary error string can't break HTML parsing.
        let msg = format!(
            "⏰ {} 執行失敗：{}",
            job.name,
            failure_summary(&output.message)
        );
        client.send_message(chat_id, &msg).await?;
    }
    Ok(true)
}

pub async fn notify_turn_result_with_history<C: TgClient + Sync>(
    client: &C,
    history: Option<&ChatHistoryStore>,
    job: &Job,
    output: &ExecutionOutput,
) -> Result<bool, TgError> {
    let JobKind::Turn { scope, .. } = &job.kind else {
        return Ok(false);
    };
    let Some(_chat_id) = chat_id_from_scope(scope) else {
        return Ok(false);
    };

    let content = if output.success {
        if output.message.trim().is_empty() {
            format!("⏰ {}（無內容）", job.name)
        } else {
            format!("⏰ {}\n\n{}", job.name, output.message)
        }
    } else {
        format!(
            "⏰ {} 執行失敗：{}",
            job.name,
            failure_summary(&output.message)
        )
    };
    let status = if output.success { "complete" } else { "error" };

    let sent = notify_turn_result(client, job, output).await?;
    if sent {
        record_history(history, scope, &content, status).await;
    }
    Ok(sent)
}

#[cfg(test)]
mod tests {
    use super::*;
    use wukong_scheduler::{Job, JobKind, MaintenanceTask};
    use wukong_tg_client::client::mock::MockTgClient;

    fn turn_job(scope: &str) -> Job {
        Job {
            id: "job-1".to_string(),
            name: "晨間報告".to_string(),
            kind: JobKind::Turn {
                scope: scope.to_string(),
                prompt: "go".to_string(),
            },
            cron: "0 9 * * *".to_string(),
            enabled: true,
            next_run_at: Some(1),
            last_run_at: None,
        }
    }

    fn ok(message: &str) -> ExecutionOutput {
        ExecutionOutput {
            success: true,
            message: message.to_string(),
        }
    }

    async fn history() -> (wukong_chat_history::ChatHistoryStore, String) {
        let f = tempfile::NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        (
            wukong_chat_history::ChatHistoryStore::open(&url)
                .await
                .unwrap(),
            url,
        )
    }

    #[tokio::test]
    async fn delivers_success_result_as_html_with_header() {
        let client = MockTgClient::default();
        let sent = notify_turn_result(&client, &turn_job("user:tg-555"), &ok("今天一切正常"))
            .await
            .unwrap();
        assert!(sent);
        let log = client.sent.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert_eq!(log[0].chat_id, 555);
        assert!(log[0].html);
        assert!(log[0].text.contains("⏰ 晨間報告"));
        assert!(log[0].text.contains("今天一切正常"));
    }

    #[tokio::test]
    async fn records_scheduled_telegram_result_in_chat_history() {
        let client = MockTgClient::default();
        let (history, _url) = history().await;
        let job = turn_job("user:tg-555");

        let sent =
            notify_turn_result_with_history(&client, Some(&history), &job, &ok("今天一切正常"))
                .await
                .unwrap();

        assert!(sent);
        let thread = history.default_thread("user:tg-555").await.unwrap();
        let messages = history.latest_messages(&thread, 10).await.unwrap();
        assert!(messages.iter().any(|m| {
            m.role == "assistant"
                && m.content.contains("⏰ 晨間報告")
                && m.content.contains("今天一切正常")
        }));
    }

    #[tokio::test]
    async fn delivers_brief_plain_text_on_failure() {
        let client = MockTgClient::default();
        let out = ExecutionOutput {
            success: false,
            message: "boom\nstack line 2\nline 3".to_string(),
        };
        let sent = notify_turn_result(&client, &turn_job("user:tg-7"), &out)
            .await
            .unwrap();
        assert!(sent);
        let log = client.sent.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert!(!log[0].html, "failure notice should be plain text");
        assert!(log[0].text.contains("執行失敗"));
        assert!(log[0].text.contains("boom"));
        assert!(
            !log[0].text.contains("line 3"),
            "only the first line is included"
        );
    }

    #[tokio::test]
    async fn sends_placeholder_when_success_body_is_empty() {
        let client = MockTgClient::default();
        let sent = notify_turn_result(&client, &turn_job("user:tg-9"), &ok("   "))
            .await
            .unwrap();
        assert!(sent);
        let log = client.sent.lock().unwrap();
        assert_eq!(log.len(), 1);
        assert!(log[0].text.contains("（無內容）"));
    }

    #[tokio::test]
    async fn skips_non_telegram_scope() {
        let client = MockTgClient::default();
        let sent = notify_turn_result(&client, &turn_job("project:Wukong"), &ok("hi"))
            .await
            .unwrap();
        assert!(!sent);
        assert!(client.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn skips_maintenance_job() {
        let client = MockTgClient::default();
        let job = Job {
            id: "m".to_string(),
            name: "consolidate".to_string(),
            kind: JobKind::Maintenance {
                scope: Some("user:tg-1".to_string()),
                task: MaintenanceTask::Consolidate,
            },
            cron: "0 2 * * *".to_string(),
            enabled: true,
            next_run_at: Some(1),
            last_run_at: None,
        };
        let sent = notify_turn_result(&client, &job, &ok("done"))
            .await
            .unwrap();
        assert!(!sent);
        assert!(client.sent.lock().unwrap().is_empty());
    }
}
