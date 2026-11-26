//! Per-message dispatch: allowlist → classify → run_turn → reply.

use crate::client::{
    send_html_with_plain_fallback, InlineKeyboard, InlineKeyboardButton, TgClient,
};
use crate::command::{classify_message, MessageAction};
use crate::parse::{is_allowed, scope_for_chat, TgAttachment, TgCallbackQuery, TgMessage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use wukong_chat_history::{ChatHistoryStore, NewChatAttachment};
use wukong_cli::run_turn_observed_with_attachments;
use wukong_gateway::backend::{AgentAttachment, AgentBackend, AiBackend};
use wukong_gateway::config::GatewayConfig;
use wukong_gateway::stream::{QuestionInfo, QuestionRequest};
use wukong_gateway::{GatewayError, StreamEvent};
use wukong_memory::Memory;
use wukong_orchestrator::Role;
use wukong_runtime::util::{now_unix, upload_root};

/// Progress updates fed to the single status bubble.
enum Progress {
    Role(Role),
    Reasoning(String),
    ToolUse(String),
    QuestionRequest(QuestionRequest),
    /// The question's answer was sent; the turn goes on (#39).
    QuestionAnswered,
    /// The question was cancelled or timed out; the turn goes on (#39).
    QuestionClosed,
}

/// Lets a question's answer, cancel or timeout path tell the turn's progress
/// task to show progress again. Weak, so a leftover pending question never
/// keeps the progress task (and the turn's cleanup) waiting.
#[derive(Debug, Clone, Default)]
pub struct ProgressResume(Option<tokio::sync::mpsc::WeakUnboundedSender<Progress>>);

impl ProgressResume {
    fn notify(&self, progress: Progress) {
        if let Some(tx) = self.0.as_ref().and_then(|weak| weak.upgrade()) {
            let _ = tx.send(progress);
        }
    }
}

enum ProgressDisplay {
    Draft { draft_id: i64 },
    Message { message_id: Option<i64> },
}

async fn start_progress_display<C: TgClient>(
    client: &C,
    chat_id: i64,
    draft_id: i64,
    text: &str,
) -> Option<ProgressDisplay> {
    if client
        .send_message_draft(chat_id, draft_id, text)
        .await
        .is_ok()
    {
        return Some(ProgressDisplay::Draft { draft_id });
    }

    client
        .send_message(chat_id, text)
        .await
        .ok()
        .map(|message_id| ProgressDisplay::Message {
            message_id: Some(message_id),
        })
}

async fn update_progress_display<C: TgClient>(
    client: &C,
    chat_id: i64,
    display: &mut ProgressDisplay,
    text: &str,
) {
    match display {
        ProgressDisplay::Draft { draft_id } => {
            if client
                .send_message_draft(chat_id, *draft_id, text)
                .await
                .is_err()
            {
                if let Ok(message_id) = client.send_message(chat_id, text).await {
                    *display = ProgressDisplay::Message {
                        message_id: Some(message_id),
                    };
                }
            }
        }
        ProgressDisplay::Message { message_id } => {
            if let Some(message_id) = *message_id {
                let _ = client.edit_message_text(chat_id, message_id, text).await;
            } else if let Ok(new_message_id) = client.send_message(chat_id, text).await {
                *message_id = Some(new_message_id);
            }
        }
    }
}

async fn clear_progress_display<C: TgClient>(
    client: &C,
    chat_id: i64,
    display: &mut ProgressDisplay,
) {
    if let ProgressDisplay::Message { message_id } = display {
        if let Some(message_id) = message_id.take() {
            let _ = client.delete_message(chat_id, message_id).await;
        }
    }
}

/// Status shown again once a question is resolved and the turn goes on (#39).
fn resumed_text(role: Option<&str>, status: &str) -> String {
    match role {
        Some(r) => format!("🐵　悟空·{r} {status}"),
        None => format!("🐵　{status}"),
    }
}

/// A draft not refreshed for about 60 seconds disappears (seen by real send on
/// 2026-11-19, #39), so an idle draft is resent well before that.
const DRAFT_REFRESH: std::time::Duration = std::time::Duration::from_secs(20);

/// Drive the turn's single progress display (draft or status message) from
/// progress events until the turn drops its senders, then clear it. `status` is
/// what the display shows when this starts.
async fn run_progress<C: TgClient>(
    c: C,
    chat_id: i64,
    mut progress_display: ProgressDisplay,
    mut status: String,
    mut rx: tokio::sync::mpsc::UnboundedReceiver<Progress>,
    resume: tokio::sync::mpsc::WeakUnboundedSender<Progress>,
    pending_questions: Arc<Mutex<PendingQuestions>>,
) {
    let mut role: Option<String> = None;
    let mut reasoning = String::new();
    let mut last_reasoning_edit: Option<std::time::Instant> = None;
    let mut last_shown = tokio::time::Instant::now();
    // While a question waits, the question message stays at the bottom: events
    // (e.g. another tool call of the same step) only update `status`, which
    // shows once the question is resolved.
    let mut question_waiting = false;
    loop {
        let keep_alive =
            !question_waiting && matches!(progress_display, ProgressDisplay::Draft { .. });
        let msg = tokio::select! {
            msg = rx.recv() => match msg {
                Some(msg) => msg,
                None => break,
            },
            () = tokio::time::sleep_until(last_shown + DRAFT_REFRESH), if keep_alive => {
                update_progress_display(&c, chat_id, &mut progress_display, &status).await;
                last_shown = tokio::time::Instant::now();
                continue;
            }
        };
        let show = match msg {
            Progress::Role(r) => {
                role = Some(r.name().to_string());
                status = bubble_text(role.as_deref(), &reasoning);
                true
            }
            Progress::Reasoning(t) => {
                reasoning.push_str(&t);
                status = bubble_text(role.as_deref(), &reasoning);
                // Throttle reasoning edits (~1.5s) but never block
                // the first one (so it always shows up promptly).
                let due = last_reasoning_edit
                    .is_none_or(|i| i.elapsed() >= std::time::Duration::from_millis(1500));
                if due {
                    last_reasoning_edit = Some(std::time::Instant::now());
                }
                due
            }
            Progress::ToolUse(name) => {
                reasoning.push_str("\n▸ 使用工具 ");
                reasoning.push_str(&name);
                reasoning.push('\n');
                status = bubble_text(role.as_deref(), &reasoning);
                true
            }
            Progress::QuestionRequest(request) => {
                clear_progress_display(&c, chat_id, &mut progress_display).await;
                let pending = PendingQuestion {
                    chat_id,
                    session_id: request.session_id.clone(),
                    request_id: request.request_id.clone(),
                    answers: vec![Vec::new(); request.questions.len()],
                    questions: request.questions.clone(),
                    current_question_index: 0,
                    waiting_custom_question_index: None,
                    deadline: std::time::Instant::now() + QUESTION_TIMEOUT,
                    message_id: None,
                    resume: ProgressResume(Some(resume.clone())),
                };
                let (text, keyboard) = render_pending_question(&pending);
                if let Ok(message_id) = c
                    .send_message_with_inline_keyboard(chat_id, &text, keyboard)
                    .await
                {
                    let mut pending = pending;
                    pending.message_id = Some(message_id);
                    pending_questions.lock().unwrap().insert(chat_id, pending);
                    question_waiting = true;
                }
                false
            }
            Progress::QuestionAnswered => {
                question_waiting = false;
                status = resumed_text(role.as_deref(), "已收到回答，繼續處理…");
                true
            }
            Progress::QuestionClosed => {
                question_waiting = false;
                status = resumed_text(role.as_deref(), "繼續處理…");
                true
            }
        };
        if show && !question_waiting {
            update_progress_display(&c, chat_id, &mut progress_display, &status).await;
            last_shown = tokio::time::Instant::now();
        }
    }
    clear_progress_display(&c, chat_id, &mut progress_display).await;
}

const QUESTION_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionAction {
    Pick { question: usize, option: usize },
    Toggle { question: usize, option: usize },
    Custom { question: usize },
    Next,
    Cancel,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedQuestionCallback {
    pub request_id: String,
    pub action: QuestionAction,
}

#[derive(Debug, Clone)]
pub struct PendingQuestion {
    pub chat_id: i64,
    pub session_id: String,
    pub request_id: String,
    pub questions: Vec<QuestionInfo>,
    pub current_question_index: usize,
    pub answers: Vec<Vec<String>>,
    pub waiting_custom_question_index: Option<usize>,
    pub deadline: std::time::Instant,
    pub message_id: Option<i64>,
    pub resume: ProgressResume,
}

pub type PendingQuestions = HashMap<i64, PendingQuestion>;

pub trait QuestionResponder {
    fn reply_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> impl std::future::Future<Output = Result<(), GatewayError>> + Send;

    fn reject_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> impl std::future::Future<Output = Result<(), GatewayError>> + Send;
}

impl QuestionResponder for AgentBackend {
    async fn reply_question(
        &self,
        session_id: &str,
        request_id: &str,
        answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        self.answer_question(session_id, request_id, answers).await
    }

    async fn reject_question(
        &self,
        session_id: &str,
        request_id: &str,
    ) -> Result<(), GatewayError> {
        self.cancel_question(session_id, request_id).await
    }
}

struct NoopQuestionResponder;

impl QuestionResponder for NoopQuestionResponder {
    async fn reply_question(
        &self,
        _session_id: &str,
        _request_id: &str,
        _answers: Vec<Vec<String>>,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "question responder is not configured".to_string(),
        })
    }

    async fn reject_question(
        &self,
        _session_id: &str,
        _request_id: &str,
    ) -> Result<(), GatewayError> {
        Err(GatewayError::AgentFailed {
            code: None,
            stderr: "question responder is not configured".to_string(),
        })
    }
}

pub fn question_callback_data(request_id: &str, action: QuestionAction) -> String {
    match action {
        QuestionAction::Pick { question, option } => {
            format!("q:{request_id}:pick:{question}:{option}")
        }
        QuestionAction::Toggle { question, option } => {
            format!("q:{request_id}:toggle:{question}:{option}")
        }
        QuestionAction::Custom { question } => format!("q:{request_id}:custom:{question}"),
        QuestionAction::Next => format!("q:{request_id}:next"),
        QuestionAction::Cancel => format!("q:{request_id}:cancel"),
    }
}

pub fn parse_question_callback(data: &str) -> Option<ParsedQuestionCallback> {
    let mut parts = data.split(':');
    if parts.next()? != "q" {
        return None;
    }
    let request_id = parts.next()?.to_string();
    let action = match parts.next()? {
        "pick" => QuestionAction::Pick {
            question: parts.next()?.parse().ok()?,
            option: parts.next()?.parse().ok()?,
        },
        "toggle" => QuestionAction::Toggle {
            question: parts.next()?.parse().ok()?,
            option: parts.next()?.parse().ok()?,
        },
        "custom" => QuestionAction::Custom {
            question: parts.next()?.parse().ok()?,
        },
        "next" => QuestionAction::Next,
        "cancel" => QuestionAction::Cancel,
        _ => return None,
    };
    if parts.next().is_some() {
        return None;
    }
    Some(ParsedQuestionCallback { request_id, action })
}

pub fn render_pending_question(pending: &PendingQuestion) -> (String, InlineKeyboard) {
    let question = &pending.questions[pending.current_question_index];
    let total = pending.questions.len();
    let current = pending.current_question_index + 1;
    let mut text = format!("❓ 第 {current} / {total} 題");
    if !question.header.trim().is_empty() {
        text.push('\n');
        text.push_str(&question.header);
    }
    text.push_str("\n\n");
    text.push_str(&question.question);

    let selected = pending
        .answers
        .get(pending.current_question_index)
        .cloned()
        .unwrap_or_default();
    let mut keyboard = Vec::new();

    if question.multiple {
        for (row, chunk) in question.options.chunks(2).enumerate() {
            let buttons = chunk
                .iter()
                .enumerate()
                .map(|(offset, option)| {
                    let option_index = row * 2 + offset;
                    let mark = if selected.contains(&option.label) {
                        "[x]"
                    } else {
                        "[ ]"
                    };
                    InlineKeyboardButton {
                        text: format!("{mark} {}", option.label),
                        callback_data: question_callback_data(
                            &pending.request_id,
                            QuestionAction::Toggle {
                                question: pending.current_question_index,
                                option: option_index,
                            },
                        ),
                    }
                })
                .collect::<Vec<_>>();
            keyboard.push(buttons);
        }
        keyboard.push(vec![InlineKeyboardButton {
            text: if pending.current_question_index + 1 == total {
                "送出".to_string()
            } else {
                "下一題".to_string()
            },
            callback_data: question_callback_data(&pending.request_id, QuestionAction::Next),
        }]);
    } else {
        for (option_index, option) in question.options.iter().enumerate() {
            keyboard.push(vec![InlineKeyboardButton {
                text: option.label.clone(),
                callback_data: question_callback_data(
                    &pending.request_id,
                    QuestionAction::Pick {
                        question: pending.current_question_index,
                        option: option_index,
                    },
                ),
            }]);
        }
    }

    if question.custom {
        keyboard.push(vec![InlineKeyboardButton {
            text: "自訂回答".to_string(),
            callback_data: question_callback_data(
                &pending.request_id,
                QuestionAction::Custom {
                    question: pending.current_question_index,
                },
            ),
        }]);
    }
    keyboard.push(vec![InlineKeyboardButton {
        text: "取消".to_string(),
        callback_data: question_callback_data(&pending.request_id, QuestionAction::Cancel),
    }]);

    (text, keyboard)
}

fn toggle_answer(answers: &mut Vec<String>, label: &str) {
    if let Some(index) = answers.iter().position(|answer| answer == label) {
        answers.remove(index);
    } else {
        answers.push(label.to_string());
    }
}

async fn edit_pending_question<C: TgClient>(client: &C, pending: &PendingQuestion) {
    let Some(message_id) = pending.message_id else {
        return;
    };
    let (text, keyboard) = render_pending_question(pending);
    let _ = client
        .edit_message_text_with_inline_keyboard(pending.chat_id, message_id, &text, keyboard)
        .await;
}

async fn advance_or_reply<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    pending_questions: Arc<Mutex<PendingQuestions>>,
    chat_id: i64,
) {
    let mut reply_payload = None;
    let mut edit_pending = None;
    {
        let mut pending_questions = pending_questions.lock().unwrap();
        let Some(pending) = pending_questions.get_mut(&chat_id) else {
            return;
        };
        if pending.current_question_index + 1 < pending.questions.len() {
            pending.current_question_index += 1;
            pending.waiting_custom_question_index = None;
            edit_pending = Some(pending.clone());
        } else if let Some(pending) = pending_questions.remove(&chat_id) {
            reply_payload = Some((
                pending.session_id,
                pending.request_id,
                pending.answers,
                pending.message_id,
                pending.resume,
            ));
        }
    }

    if let Some(pending) = edit_pending {
        edit_pending_question(client, &pending).await;
    }
    if let Some((session_id, request_id, answers, message_id, resume)) = reply_payload {
        match responder
            .reply_question(&session_id, &request_id, answers)
            .await
        {
            Ok(()) => {
                if let Some(message_id) = message_id {
                    let _ = client
                        .edit_message_text(chat_id, message_id, "已送出回答。")
                        .await;
                }
                resume.notify(Progress::QuestionAnswered);
            }
            Err(err) => {
                let _ = client
                    .send_message(chat_id, &format!("⚠️ 回答送出失敗：{err}"))
                    .await;
            }
        }
    }
}

async fn reject_and_clear_question<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    pending_questions: Arc<Mutex<PendingQuestions>>,
    chat_id: i64,
    message: &str,
) {
    let pending = pending_questions.lock().unwrap().remove(&chat_id);
    let Some(pending) = pending else {
        return;
    };
    let _ = responder
        .reject_question(&pending.session_id, &pending.request_id)
        .await;
    if let Some(message_id) = pending.message_id {
        let _ = client.edit_message_text(chat_id, message_id, message).await;
    }
    pending.resume.notify(Progress::QuestionClosed);
}

async fn complete_custom_answer<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    pending_questions: Arc<Mutex<PendingQuestions>>,
    chat_id: i64,
    answer: String,
) {
    {
        let mut pending_questions = pending_questions.lock().unwrap();
        let Some(pending) = pending_questions.get_mut(&chat_id) else {
            return;
        };
        let Some(index) = pending.waiting_custom_question_index.take() else {
            return;
        };
        if pending.questions[index].multiple {
            if !pending.answers[index].contains(&answer) {
                pending.answers[index].push(answer);
            }
        } else {
            pending.answers[index] = vec![answer];
        }
    }
    advance_or_reply(client, responder, pending_questions, chat_id).await;
}

pub async fn cleanup_expired_questions<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    pending_questions: Arc<Mutex<PendingQuestions>>,
) {
    let now = std::time::Instant::now();
    let expired = pending_questions
        .lock()
        .unwrap()
        .iter()
        .filter_map(|(chat_id, pending)| (pending.deadline <= now).then_some(*chat_id))
        .collect::<Vec<_>>();

    for chat_id in expired {
        reject_and_clear_question(
            client,
            responder,
            pending_questions.clone(),
            chat_id,
            "問題已逾時，已取消。",
        )
        .await;
    }
}

pub async fn handle_callback_query<C: TgClient, R: QuestionResponder>(
    client: &C,
    responder: &R,
    allow: &[i64],
    pending_questions: Arc<Mutex<PendingQuestions>>,
    callback: &TgCallbackQuery,
) {
    // Enforce the allowlist on callbacks too, consistent with message handling.
    if !is_allowed(callback.chat_id, allow) {
        return;
    }
    let Some(parsed) = parse_question_callback(&callback.data) else {
        let _ = client
            .answer_callback_query(&callback.callback_query_id, "無法處理這個操作")
            .await;
        return;
    };
    let mut edit_pending = None;
    let mut reject = false;
    let mut advance = false;
    let mut custom_edit: Option<(i64, i64)> = None;
    let mut callback_message = "";

    // Decide everything under the lock without awaiting, then release the guard
    // before any network call — holding the std Mutex across an await risks
    // blocking the executor.
    let mut invalid = false;
    {
        let mut pending_questions_guard = pending_questions.lock().unwrap();
        match pending_questions_guard.get_mut(&callback.chat_id) {
            None => invalid = true,
            Some(pending) if pending.request_id != parsed.request_id => invalid = true,
            Some(pending) => match parsed.action {
                QuestionAction::Pick { question, option } => {
                    if question != pending.current_question_index {
                        callback_message = "題目已更新";
                    } else if let Some(label) = pending.questions[question]
                        .options
                        .get(option)
                        .map(|option| option.label.clone())
                    {
                        pending.answers[question] = vec![label];
                        advance = true;
                    }
                }
                QuestionAction::Toggle { question, option } => {
                    if question == pending.current_question_index {
                        if let Some(label) = pending.questions[question]
                            .options
                            .get(option)
                            .map(|option| option.label.clone())
                        {
                            toggle_answer(&mut pending.answers[question], &label);
                            edit_pending = Some(pending.clone());
                        }
                    }
                }
                QuestionAction::Custom { question } => {
                    pending.waiting_custom_question_index = Some(question);
                    if let Some(message_id) = pending.message_id {
                        custom_edit = Some((pending.chat_id, message_id));
                    }
                }
                QuestionAction::Next => advance = true,
                QuestionAction::Cancel => reject = true,
            },
        }
    }

    if invalid {
        let _ = client
            .answer_callback_query(&callback.callback_query_id, "這個問題已失效")
            .await;
        return;
    }

    if let Some((chat_id, message_id)) = custom_edit {
        let _ = client
            .edit_message_text(chat_id, message_id, "請直接傳下一則文字作為自訂回答。")
            .await;
    }
    if let Some(pending) = edit_pending {
        edit_pending_question(client, &pending).await;
    }
    if advance {
        advance_or_reply(
            client,
            responder,
            pending_questions.clone(),
            callback.chat_id,
        )
        .await;
    }
    if reject {
        reject_and_clear_question(
            client,
            responder,
            pending_questions.clone(),
            callback.chat_id,
            "已取消問題。",
        )
        .await;
    }
    let _ = client
        .answer_callback_query(&callback.callback_query_id, callback_message)
        .await;
}

/// Compose the status-bubble text from the current role and accumulated reasoning.
fn bubble_text(role: Option<&str>, reasoning: &str) -> String {
    // Full-width space (U+3000) after each emoji so the glyph doesn't visually
    // crowd / obscure the first CJK character on some Telegram clients.
    let base = match role {
        Some(r) => format!("🐵　悟空·{r} 思考中…"),
        None => "🐵　思考中…".to_string(),
    };
    let r = reasoning.trim();
    if r.is_empty() {
        return base;
    }
    let full = format!("{base}\n💭　{r}");
    if full.encode_utf16().count() <= PROGRESS_TEXT_LIMIT {
        return full;
    }
    // Too long for Telegram: keep the tail that fits, after an ellipsis
    // marking the dropped head.
    let head = format!("{base}\n💭　…");
    let mut room = PROGRESS_TEXT_LIMIT.saturating_sub(head.encode_utf16().count());
    let mut start = r.len();
    for (i, c) in r.char_indices().rev() {
        if c.len_utf16() > room {
            break;
        }
        room -= c.len_utf16();
        start = i;
    }
    format!("{head}{}", &r[start..])
}

/// Telegram rejects a draft or message over 4096 characters with
/// `MESSAGE_TOO_LONG` (real send, 2026-11-19, #39). The progress text is
/// measured in UTF-16 units, never fewer than characters, so staying a little
/// under 4096 of them keeps it under the limit however Telegram counts.
const PROGRESS_TEXT_LIMIT: usize = 4000;

/// Log a failed Telegram send instead of silently dropping it, so a rejected
/// message (e.g. a 400 on malformed HTML) is visible rather than lost.
fn log_send<T, E: std::fmt::Display>(result: Result<T, E>, context: &str) {
    if let Err(e) = result {
        eprintln!("🐵 Telegram 傳送失敗（{context}）：{e}");
    }
}

const MAX_ATTACHMENT_BYTES: i64 = 25 * 1024 * 1024;
const MAX_ATTACHMENTS_PER_MESSAGE: usize = 5;

async fn store_telegram_attachments<C: TgClient>(
    client: &C,
    history: &ChatHistoryStore,
    scope: &str,
    message_id: i64,
    attachments: &[TgAttachment],
) -> Result<Vec<AgentAttachment>, String> {
    if attachments.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err("⚠️ 附件數量超過目前支援上限。".to_string());
    }
    let root = upload_root();
    let mut out = Vec::new();
    for attachment in attachments {
        if attachment.size_bytes.unwrap_or(0) > MAX_ATTACHMENT_BYTES {
            return Err("⚠️ 檔案超過目前支援大小，請改用較小的檔案。".to_string());
        }
        let info = client
            .get_file(&attachment.file_id)
            .await
            .map_err(|_| "⚠️ 無法下載 Telegram 檔案，請稍後再試。".to_string())?;
        let bytes = client
            .download_file(&info.file_path)
            .await
            .map_err(|_| "⚠️ 無法下載 Telegram 檔案，請稍後再試。".to_string())?;
        if bytes.len() as i64 > MAX_ATTACHMENT_BYTES {
            return Err("⚠️ 檔案超過目前支援大小，請改用較小的檔案。".to_string());
        }
        let stored_name = wukong_chat_history::sanitize_filename(&attachment.original_name);
        let relative_path =
            wukong_chat_history::relative_attachment_path(scope, message_id, &stored_name);
        let path = wukong_chat_history::resolve_under_upload_root(&root, &relative_path)
            .ok_or_else(|| "⚠️ 附件路徑無效。".to_string())?;
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| e.to_string())?;
        }
        tokio::fs::write(&path, &bytes)
            .await
            .map_err(|e| e.to_string())?;
        history
            .insert_attachment(&NewChatAttachment {
                message_id,
                scope: scope.to_string(),
                source: "telegram".to_string(),
                original_name: attachment.original_name.clone(),
                stored_name: stored_name.clone(),
                relative_path,
                mime_type: attachment.mime_type.clone(),
                size_bytes: bytes.len() as i64,
                sha256: None,
                telegram_file_id: Some(attachment.file_id.clone()),
                created_at: now_unix(),
            })
            .await
            .map_err(|e| e.to_string())?;
        out.push(AgentAttachment {
            path,
            original_name: attachment.original_name.clone(),
            mime_type: attachment.mime_type.clone(),
        });
    }
    Ok(out)
}

fn turn_state_dir(kind: &str, scope: &str, turn_id: i64) -> PathBuf {
    upload_root()
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(upload_root)
        .join(kind)
        .join(wukong_chat_history::sanitize_scope(scope))
        .join(turn_id.to_string())
}

async fn prepare_working_attachments(
    scope: &str,
    turn_id: i64,
    attachments: Vec<AgentAttachment>,
) -> Result<Vec<AgentAttachment>, String> {
    if attachments.is_empty() {
        return Ok(attachments);
    }
    let upload_root = upload_root()
        .canonicalize()
        .map_err(|err| format!("無法解析附件目錄：{err}"))?;
    let work_dir = turn_state_dir("workfiles", scope, turn_id);
    tokio::fs::create_dir_all(&work_dir)
        .await
        .map_err(|err| format!("無法建立附件工作目錄：{err}"))?;

    let mut prepared = Vec::with_capacity(attachments.len());
    for (index, attachment) in attachments.into_iter().enumerate() {
        let metadata = tokio::fs::symlink_metadata(&attachment.path)
            .await
            .map_err(|err| format!("無法讀取附件資訊：{err}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            return Err("附件必須是一般檔案，不能是連結或目錄。".to_string());
        }
        let source = attachment
            .path
            .canonicalize()
            .map_err(|err| format!("無法解析附件路徑：{err}"))?;
        if !source.starts_with(&upload_root) {
            return Err("附件不在受控上傳目錄內。".to_string());
        }
        let filename = format!(
            "{:02}-{}",
            index + 1,
            wukong_chat_history::sanitize_filename(&attachment.original_name)
        );
        let destination = work_dir.join(filename);
        tokio::fs::copy(&source, &destination)
            .await
            .map_err(|err| format!("無法建立附件工作副本：{err}"))?;
        prepared.push(AgentAttachment {
            path: destination,
            original_name: attachment.original_name,
            mime_type: attachment.mime_type,
        });
    }
    Ok(prepared)
}

fn artifact_return_enabled() -> bool {
    if cfg!(test) {
        return false;
    }
    let server_enabled = std::env::var("WUKONG_AGENT_SERVER_URL")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    if !server_enabled {
        return true;
    }
    !matches!(
        std::env::var("WUKONG_AGENT_SERVER_FILE_MODE")
            .unwrap_or_else(|_| "shared".to_string())
            .trim()
            .to_ascii_lowercase()
            .as_str(),
        "inline" | "disabled"
    )
}

async fn prepare_artifact_dir(scope: &str, turn_id: i64) -> Result<PathBuf, String> {
    let dir = turn_state_dir("artifacts", scope, turn_id);
    tokio::fs::create_dir_all(&dir)
        .await
        .map_err(|err| format!("無法建立產出目錄：{err}"))?;
    Ok(dir)
}

fn agent_visible_path(path: &Path) -> Result<PathBuf, String> {
    let server_enabled = std::env::var("WUKONG_AGENT_SERVER_URL")
        .ok()
        .is_some_and(|value| !value.trim().is_empty());
    if !server_enabled {
        return Ok(path.to_path_buf());
    }
    let local_workspace = std::env::var("WUKONG_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .canonicalize()
        .map_err(|err| format!("無法解析 WUKONG_WORKSPACE：{err}"))?;
    let local_path = path
        .canonicalize()
        .map_err(|err| format!("無法解析產出目錄：{err}"))?;
    let relative = local_path
        .strip_prefix(&local_workspace)
        .map_err(|_| "產出目錄不在 WUKONG_WORKSPACE 內。".to_string())?;
    let server_workspace = std::env::var("WUKONG_AGENT_SERVER_WORKSPACE")
        .map(PathBuf::from)
        .unwrap_or(local_workspace);
    if !server_workspace.is_absolute() {
        return Err("WUKONG_AGENT_SERVER_WORKSPACE 必須是絕對路徑。".to_string());
    }
    Ok(server_workspace.join(relative))
}

fn prompt_with_artifact_instruction(input: &str, artifact_dir: &Path) -> Result<String, String> {
    let visible_dir = agent_visible_path(artifact_dir)?;
    // 標頭用 runtime 的常數：run_turn 靠它把這一行切出記憶與召回。規則本身在 agent 定義裡
    // （#45 SCN-006），CLI backend 由 run_turn 補上。
    Ok(format!(
        "{input}\n\n{} {}",
        wukong_runtime::persona::ARTIFACT_DIR_HEADER,
        visible_dir.display()
    ))
}

async fn collect_artifacts(dir: &Path) -> Result<Vec<(String, Vec<u8>)>, String> {
    let root = dir
        .canonicalize()
        .map_err(|err| format!("無法解析產出目錄：{err}"))?;
    let mut entries = tokio::fs::read_dir(&root)
        .await
        .map_err(|err| format!("無法讀取產出目錄：{err}"))?;
    let mut paths = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|err| format!("無法列出產出檔案：{err}"))?
    {
        let metadata = tokio::fs::symlink_metadata(entry.path())
            .await
            .map_err(|err| format!("無法讀取產出檔案資訊：{err}"))?;
        if metadata.file_type().is_symlink() || !metadata.is_file() {
            continue;
        }
        if metadata.len() > MAX_ATTACHMENT_BYTES as u64 {
            return Err(format!(
                "產出檔案 {} 超過 25 MiB，未回傳 Telegram。",
                entry.file_name().to_string_lossy()
            ));
        }
        let path = entry
            .path()
            .canonicalize()
            .map_err(|err| format!("無法解析產出檔案：{err}"))?;
        if !path.starts_with(&root) {
            return Err("產出檔案逃離受控目錄，已拒絕回傳。".to_string());
        }
        paths.push(path);
    }
    paths.sort();
    if paths.len() > MAX_ATTACHMENTS_PER_MESSAGE {
        return Err("產出檔案超過 5 份，請要求 OpenCode 減少回傳檔案。".to_string());
    }
    let mut artifacts = Vec::with_capacity(paths.len());
    for path in paths {
        let filename = path
            .file_name()
            .and_then(|value| value.to_str())
            .map(wukong_chat_history::sanitize_filename)
            .unwrap_or_else(|| "artifact".to_string());
        let bytes = tokio::fs::read(&path)
            .await
            .map_err(|err| format!("無法讀取產出檔案：{err}"))?;
        artifacts.push((filename, bytes));
    }
    Ok(artifacts)
}

async fn return_artifacts<C: TgClient>(
    client: &C,
    chat_id: i64,
    dir: &Path,
) -> Result<usize, String> {
    let artifacts = collect_artifacts(dir).await?;
    let count = artifacts.len();
    for (filename, bytes) in artifacts {
        client
            .send_document(chat_id, &filename, bytes, Some("OpenCode 產出"))
            .await
            .map_err(|err| format!("Telegram 回傳 {filename} 失敗：{err}"))?;
    }
    Ok(count)
}

async fn record_chat(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    role: &str,
    content: &str,
    content_html: Option<&str>,
    status: &str,
) -> Option<i64> {
    let history = history?;
    match history.default_thread(scope).await {
        Ok(thread) => match history
            .insert_message(&thread, role, content, content_html, status, now_unix())
            .await
        {
            Ok(id) => Some(id),
            Err(e) => {
                eprintln!("warning: telegram chat history insert failed: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("warning: telegram chat history thread failed: {e}");
            None
        }
    }
}

async fn record_chat_with_events(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    role: &str,
    content: &str,
    content_html: Option<&str>,
    status: &str,
    events: &[(i64, String, Option<String>, String, i64)],
) -> Option<i64> {
    let history = history?;
    match history.default_thread(scope).await {
        Ok(thread) => match history
            .insert_message(&thread, role, content, content_html, status, now_unix())
            .await
        {
            Ok(message_id) => {
                for (seq, kind, label, content, created_at) in events {
                    let _ = history
                        .insert_event(
                            message_id,
                            *seq,
                            kind,
                            label.as_deref(),
                            content,
                            *created_at,
                        )
                        .await;
                }
                Some(message_id)
            }
            Err(e) => {
                eprintln!("warning: telegram chat history insert failed: {e}");
                None
            }
        },
        Err(e) => {
            eprintln!("warning: telegram chat history thread failed: {e}");
            None
        }
    }
}

#[derive(Debug)]
struct LiveEventWrite {
    kind: String,
    label: Option<String>,
    content: String,
    message_id: Option<i64>,
    created_at: i64,
}

fn queue_live_event(
    tx: &Option<tokio::sync::mpsc::UnboundedSender<LiveEventWrite>>,
    kind: &str,
    label: Option<&str>,
    content: &str,
    message_id: Option<i64>,
) {
    let Some(tx) = tx else {
        return;
    };
    let _ = tx.send(LiveEventWrite {
        kind: kind.to_string(),
        label: label.map(str::to_string),
        content: content.to_string(),
        message_id,
        created_at: now_unix(),
    });
}

fn question_request_json(request: &QuestionRequest) -> String {
    serde_json::json!({
        "request_id": request.request_id,
        "session_id": request.session_id,
        "questions": request.questions.iter().map(|q| {
            serde_json::json!({
                "question": q.question,
                "header": q.header,
                "multiple": q.multiple,
                "custom": q.custom,
                "options": q.options.iter().map(|o| {
                    serde_json::json!({
                        "label": o.label,
                        "description": o.description,
                    })
                }).collect::<Vec<_>>()
            })
        }).collect::<Vec<_>>()
    })
    .to_string()
}

async fn record_live_event(
    history: Option<&ChatHistoryStore>,
    scope: &str,
    kind: &str,
    label: Option<&str>,
    content: &str,
    message_id: Option<i64>,
) {
    let Some(history) = history else {
        return;
    };
    if let Err(e) = history
        .insert_live_event(scope, kind, label, content, message_id, now_unix())
        .await
    {
        eprintln!("warning: telegram live event insert failed: {e}");
    }
}

/// Handle one incoming message: enforce the allowlist, classify, run the turn,
/// and reply. Errors are reported to the chat and swallowed (the loop goes on).
/// `C` must be Clone + Send + 'static so a side task can stream role progress.
pub async fn handle_message<C, B>(
    client: &C,
    mem: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    history: Option<&ChatHistoryStore>,
    allow: &[i64],
    msg: &TgMessage,
) where
    C: TgClient + Clone + Send + Sync + 'static,
    B: AiBackend,
{
    let pending_questions = Arc::new(Mutex::new(PendingQuestions::new()));
    handle_message_with_pending(
        client,
        mem,
        base_cfg,
        backend,
        history,
        allow,
        pending_questions,
        msg,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_message_with_pending<C, B>(
    client: &C,
    mem: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    history: Option<&ChatHistoryStore>,
    allow: &[i64],
    pending_questions: Arc<Mutex<PendingQuestions>>,
    msg: &TgMessage,
) where
    C: TgClient + Clone + Send + Sync + 'static,
    B: AiBackend,
{
    handle_message_with_responder(
        client,
        mem,
        base_cfg,
        backend,
        &NoopQuestionResponder,
        history,
        allow,
        pending_questions,
        msg,
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_message_with_responder<C, B, R>(
    client: &C,
    mem: &Memory,
    base_cfg: &GatewayConfig,
    backend: &B,
    responder: &R,
    history: Option<&ChatHistoryStore>,
    allow: &[i64],
    pending_questions: Arc<Mutex<PendingQuestions>>,
    msg: &TgMessage,
) where
    C: TgClient + Clone + Send + Sync + 'static,
    B: AiBackend,
    R: QuestionResponder,
{
    if !is_allowed(msg.chat_id, allow) {
        return; // silently ignore non-allowlisted chats
    }
    let chat_id = msg.chat_id;
    let waiting_custom = pending_questions
        .lock()
        .unwrap()
        .get(&chat_id)
        .and_then(|pending| pending.waiting_custom_question_index);
    if waiting_custom.is_some() {
        if msg.text.trim().is_empty() || !msg.attachments.is_empty() {
            let _ = client
                .send_message(chat_id, "請傳文字答案，不要傳附件。")
                .await;
            return;
        }
        complete_custom_answer(
            client,
            responder,
            pending_questions.clone(),
            chat_id,
            msg.text.trim().to_string(),
        )
        .await;
        return;
    }
    if pending_questions.lock().unwrap().contains_key(&chat_id) {
        let _ = client
            .send_message(chat_id, "請先回答目前問題，或按取消。")
            .await;
        return;
    }
    match classify_message(&msg.text) {
        MessageAction::Command { name, args } => {
            let mut cfg = base_cfg.clone();
            cfg.scope = scope_for_chat(chat_id);
            let settings_path = wukong_settings::default_settings_path();
            let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
            let agent_settings = wukong_settings::effective_agent_settings(&settings);
            cfg.apply_default_model(agent_settings.default_model.as_deref());
            let user_message_id =
                record_chat(history, &cfg.scope, "user", &msg.text, None, "complete").await;
            record_live_event(
                history,
                &cfg.scope,
                "user",
                None,
                &msg.text,
                user_message_id,
            )
            .await;
            match wukong_cli::parse_session_command(&name, &args) {
                Some(cmd) => {
                    let reply = match wukong_cli::run_session_command(
                        mem,
                        backend,
                        &cfg,
                        &settings_path,
                        cmd,
                    )
                    .await
                    {
                        Ok(t) => t,
                        Err(e) => format!("⚠️ 失敗：{e}"),
                    };
                    let reply_message_id =
                        record_chat(history, &cfg.scope, "assistant", &reply, None, "complete")
                            .await;
                    let reply_html = wukong_render::to_web_html(&reply);
                    record_live_event(
                        history,
                        &cfg.scope,
                        "answer",
                        None,
                        &reply_html,
                        reply_message_id,
                    )
                    .await;
                    log_send(client.send_message(chat_id, &reply).await, "command");
                }
                None => {
                    let reply = format!("指令 /{name} 尚未支援");
                    let reply_message_id =
                        record_chat(history, &cfg.scope, "assistant", &reply, None, "complete")
                            .await;
                    let reply_html = wukong_render::to_web_html(&reply);
                    record_live_event(
                        history,
                        &cfg.scope,
                        "answer",
                        None,
                        &reply_html,
                        reply_message_id,
                    )
                    .await;
                    log_send(client.send_message(chat_id, &reply).await, "command");
                }
            }
        }
        MessageAction::Turn(input) => {
            let mut cfg = base_cfg.clone();
            cfg.scope = scope_for_chat(chat_id);
            let settings_path = wukong_settings::default_settings_path();
            let settings = wukong_settings::load_settings(&settings_path).unwrap_or_default();
            let agent_settings = wukong_settings::effective_agent_settings(&settings);
            cfg.apply_default_model(agent_settings.default_model.as_deref());
            let (live_tx, live_writer) = if let Some(history) = history {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<LiveEventWrite>();
                let history = history.clone();
                let scope = cfg.scope.clone();
                let writer = tokio::spawn(async move {
                    while let Some(event) = rx.recv().await {
                        let _ = history
                            .insert_live_event(
                                &scope,
                                &event.kind,
                                event.label.as_deref(),
                                &event.content,
                                event.message_id,
                                event.created_at,
                            )
                            .await;
                    }
                });
                (Some(tx), Some(writer))
            } else {
                (None, None)
            };
            let user_message_id =
                record_chat(history, &cfg.scope, "user", &input, None, "complete").await;
            queue_live_event(&live_tx, "user", None, &input, user_message_id);

            let agent_attachments = if msg.attachments.is_empty() {
                Vec::new()
            } else {
                let Some(history) = history else {
                    let reply = "⚠️ 目前無法保存附件，請稍後再試。";
                    log_send(client.send_message(chat_id, reply).await, "command");
                    drop(live_tx);
                    if let Some(writer) = live_writer {
                        let _ = writer.await;
                    }
                    return;
                };
                let Some(message_id) = user_message_id else {
                    let reply = "⚠️ 目前無法保存附件，請稍後再試。";
                    log_send(client.send_message(chat_id, reply).await, "command");
                    drop(live_tx);
                    if let Some(writer) = live_writer {
                        let _ = writer.await;
                    }
                    return;
                };
                match store_telegram_attachments(
                    client,
                    history,
                    &cfg.scope,
                    message_id,
                    &msg.attachments,
                )
                .await
                {
                    Ok(attachments) => attachments,
                    Err(reply) => {
                        log_send(client.send_message(chat_id, &reply).await, "command");
                        drop(live_tx);
                        if let Some(writer) = live_writer {
                            let _ = writer.await;
                        }
                        return;
                    }
                }
            };

            let turn_id = user_message_id.unwrap_or_else(|| {
                if msg.update_id == 0 {
                    now_unix()
                } else {
                    msg.update_id
                }
            });
            let agent_attachments =
                match prepare_working_attachments(&cfg.scope, turn_id, agent_attachments).await {
                    Ok(attachments) => attachments,
                    Err(error) => {
                        let reply = format!("⚠️ 無法準備附件工作副本：{error}");
                        log_send(client.send_message(chat_id, &reply).await, "attachment");
                        drop(live_tx);
                        if let Some(writer) = live_writer {
                            let _ = writer.await;
                        }
                        return;
                    }
                };
            let (input, artifact_dir) = if artifact_return_enabled() {
                match prepare_artifact_dir(&cfg.scope, turn_id).await {
                    Ok(dir) => match prompt_with_artifact_instruction(&input, &dir) {
                        Ok(prompt) => (prompt, Some(dir)),
                        Err(error) => {
                            let reply = format!("⚠️ 無法準備檔案回傳：{error}");
                            log_send(client.send_message(chat_id, &reply).await, "artifact");
                            drop(live_tx);
                            if let Some(writer) = live_writer {
                                let _ = writer.await;
                            }
                            return;
                        }
                    },
                    Err(error) => {
                        let reply = format!("⚠️ 無法準備檔案回傳：{error}");
                        log_send(client.send_message(chat_id, &reply).await, "artifact");
                        drop(live_tx);
                        if let Some(writer) = live_writer {
                            let _ = writer.await;
                        }
                        return;
                    }
                }
            } else {
                (input, None)
            };

            // Prefer Telegram's native ephemeral draft. Chats that don't support
            // drafts (for example groups) fall back to one edited status message.
            let draft_id = if msg.update_id == 0 { 1 } else { msg.update_id };
            let initial_status = "🐵 收到，思考中…";
            let progress_display =
                match start_progress_display(client, chat_id, draft_id, initial_status).await {
                    Some(display) => display,
                    None => return, // can't post either a draft or fallback status
                };

            // Sustained "typing…": opencode runs for tens of seconds with no
            // token streaming; Telegram's typing indicator lasts only ~5s.
            let typing = {
                let c = client.clone();
                tokio::spawn(async move {
                    loop {
                        let _ = c.send_chat_action(chat_id, "typing").await;
                        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    }
                })
            };

            // Per-role + reasoning progress edits the single status bubble.
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();
            let progress = tokio::spawn(run_progress(
                client.clone(),
                chat_id,
                progress_display,
                initial_status.to_string(),
                rx,
                tx.downgrade(),
                pending_questions.clone(),
            ));

            let tx_ev = tx.clone();
            let mut events_buf: Vec<(i64, String, Option<String>, String, i64)> = Vec::new();
            let mut event_seq: i64 = 0;
            let result = run_turn_observed_with_attachments(
                mem,
                backend,
                &cfg,
                &input,
                agent_attachments,
                &mut |ev| match ev {
                    StreamEvent::Reasoning(t) => {
                        if !t.trim().is_empty() {
                            let now = now_unix();
                            events_buf.push((
                                event_seq,
                                "reasoning".to_string(),
                                None,
                                t.clone(),
                                now,
                            ));
                            event_seq += 1;
                            let _ = tx_ev.send(Progress::Reasoning(t));
                            queue_live_event(
                                &live_tx,
                                "reasoning",
                                None,
                                events_buf.last().map(|e| e.3.as_str()).unwrap_or(""),
                                None,
                            );
                        }
                    }
                    StreamEvent::ToolUse(name) => {
                        let now = now_unix();
                        events_buf.push((
                            event_seq,
                            "tool_use".to_string(),
                            Some(name.clone()),
                            format!("使用工具 {name}"),
                            now,
                        ));
                        event_seq += 1;
                        let _ = tx_ev.send(Progress::ToolUse(name));
                        queue_live_event(
                            &live_tx,
                            "tool",
                            events_buf.last().and_then(|e| e.2.as_deref()),
                            events_buf.last().map(|e| e.3.as_str()).unwrap_or(""),
                            None,
                        );
                    }
                    StreamEvent::QuestionRequest(request) => {
                        queue_live_event(
                            &live_tx,
                            "question",
                            Some(&request.request_id),
                            &question_request_json(&request),
                            None,
                        );
                        let _ = tx_ev.send(Progress::QuestionRequest(request));
                    }
                    StreamEvent::StepStart => {
                        let now = now_unix();
                        events_buf.push((
                            event_seq,
                            "step_start".to_string(),
                            None,
                            "step_start".to_string(),
                            now,
                        ));
                        event_seq += 1;
                    }
                    StreamEvent::StepFinish => {
                        let now = now_unix();
                        events_buf.push((
                            event_seq,
                            "step_finish".to_string(),
                            None,
                            "step_finish".to_string(),
                            now,
                        ));
                        event_seq += 1;
                    }
                    StreamEvent::Text(_) => {}
                    // #45 SCN-002：角色與產出卡片由 run_turn 的 on_role／on_step 處理。
                    StreamEvent::SubagentStarted { .. } | StreamEvent::SubagentReported { .. } => {}
                },
                &mut |r| {
                    let role_name = r.name().to_string();
                    queue_live_event(&live_tx, "role", None, &role_name, None);
                    let _ = tx.send(Progress::Role(r));
                },
                &mut |_, _| {},
            )
            .await;
            drop(tx);
            drop(tx_ev);
            let _ = progress.await;
            typing.abort();

            match result {
                Ok(out) => {
                    let html = wukong_render::to_web_html(&out.text);
                    let assistant_message_id = record_chat_with_events(
                        history,
                        &cfg.scope,
                        "assistant",
                        &out.text,
                        Some(&html),
                        "complete",
                        &events_buf,
                    )
                    .await;
                    queue_live_event(&live_tx, "answer", None, &html, assistant_message_id);
                    let chunks = wukong_render::to_telegram_html(&out.text);
                    if chunks.is_empty() {
                        log_send(client.send_message(chat_id, "(無內容)").await, "answer");
                    } else {
                        for c in &chunks {
                            log_send(
                                send_html_with_plain_fallback(client, chat_id, c, "answer").await,
                                "answer",
                            );
                        }
                    }
                    if let Some(dir) = artifact_dir.as_deref() {
                        if let Err(error) = return_artifacts(client, chat_id, dir).await {
                            let warning = format!("⚠️ 檔案產出完成，但回傳失敗：{error}");
                            log_send(client.send_message(chat_id, &warning).await, "artifact");
                        }
                    }
                }
                Err(e) => {
                    let err = format!("⚠️ 處理失敗：{e}");
                    let assistant_message_id = record_chat_with_events(
                        history,
                        &cfg.scope,
                        "assistant",
                        &err,
                        None,
                        "error",
                        &events_buf,
                    )
                    .await;
                    queue_live_event(&live_tx, "error", None, &err, assistant_message_id);
                    log_send(client.send_message(chat_id, &err).await, "error");
                }
            }
            drop(live_tx);
            if let Some(writer) = live_writer {
                let _ = writer.await;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::mock::MockTgClient;
    use crate::parse::{TgAttachment, TgAttachmentKind};
    use std::collections::VecDeque;
    use std::sync::{Arc, Mutex};
    use tempfile::NamedTempFile;
    use wukong_gateway::backend::{AgentRequest, AgentResponse};
    use wukong_gateway::GatewayError;

    struct MockBackend {
        replies: Mutex<VecDeque<String>>,
    }
    impl MockBackend {
        fn new(r: &[&str]) -> Self {
            Self {
                replies: Mutex::new(r.iter().map(|s| s.to_string()).collect()),
            }
        }
    }
    impl AiBackend for MockBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: self.replies.lock().unwrap().pop_front().unwrap_or_default(),
                session_id: None,
            })
        }
    }

    #[derive(Default)]
    struct RecordingBackend {
        requests: Mutex<Vec<AgentRequest>>,
        replies: Mutex<VecDeque<String>>,
    }

    impl RecordingBackend {
        fn new(r: &[&str]) -> Self {
            Self {
                requests: Mutex::new(Vec::new()),
                replies: Mutex::new(r.iter().map(|s| s.to_string()).collect()),
            }
        }
    }

    impl AiBackend for RecordingBackend {
        async fn run(&self, req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            self.requests.lock().unwrap().push(req);
            Ok(AgentResponse {
                text: self.replies.lock().unwrap().pop_front().unwrap_or_default(),
                session_id: None,
            })
        }
    }

    type QuestionReplyLog = Mutex<Vec<(String, String, Vec<Vec<String>>)>>;

    #[derive(Default)]
    struct RecordingResponder {
        replies: QuestionReplyLog,
        rejects: Mutex<Vec<(String, String)>>,
    }

    impl QuestionResponder for RecordingResponder {
        async fn reply_question(
            &self,
            session_id: &str,
            request_id: &str,
            answers: Vec<Vec<String>>,
        ) -> Result<(), GatewayError> {
            self.replies.lock().unwrap().push((
                session_id.to_string(),
                request_id.to_string(),
                answers,
            ));
            Ok(())
        }

        async fn reject_question(
            &self,
            session_id: &str,
            request_id: &str,
        ) -> Result<(), GatewayError> {
            self.rejects
                .lock()
                .unwrap()
                .push((session_id.to_string(), request_id.to_string()));
            Ok(())
        }
    }

    fn sample_pending_question(multiple: bool) -> PendingQuestion {
        PendingQuestion {
            chat_id: 7,
            session_id: "ses_1".to_string(),
            request_id: "que_1".to_string(),
            questions: vec![wukong_gateway::stream::QuestionInfo {
                question: if multiple { "選多個" } else { "選一個" }.to_string(),
                header: "偏好".to_string(),
                multiple,
                custom: true,
                options: vec![
                    wukong_gateway::stream::QuestionOption {
                        label: "A".to_string(),
                        description: "".to_string(),
                    },
                    wukong_gateway::stream::QuestionOption {
                        label: "B".to_string(),
                        description: "".to_string(),
                    },
                ],
            }],
            current_question_index: 0,
            answers: vec![Vec::new()],
            waiting_custom_question_index: None,
            deadline: std::time::Instant::now() + std::time::Duration::from_secs(600),
            message_id: Some(10),
            resume: ProgressResume::default(),
        }
    }

    #[test]
    fn question_callback_data_is_compact_and_parseable() {
        let data = question_callback_data(
            "que_1",
            QuestionAction::Pick {
                question: 0,
                option: 2,
            },
        );
        assert_eq!(data, "q:que_1:pick:0:2");
        assert_eq!(
            parse_question_callback(&data),
            Some(ParsedQuestionCallback {
                request_id: "que_1".to_string(),
                action: QuestionAction::Pick {
                    question: 0,
                    option: 2,
                },
            })
        );
    }

    #[test]
    fn render_single_choice_question_has_option_custom_and_cancel_buttons() {
        let pending = sample_pending_question(false);
        let (text, keyboard) = render_pending_question(&pending);

        assert!(text.contains("第 1 / 1 題"));
        assert!(text.contains("選一個"));
        assert_eq!(keyboard.len(), 4);
        assert_eq!(keyboard[0][0].text, "A");
        assert_eq!(keyboard[2][0].text, "自訂回答");
        assert_eq!(keyboard[3][0].text, "取消");
    }

    #[test]
    fn render_multi_choice_question_marks_selected_options() {
        let mut pending = sample_pending_question(true);
        pending.answers[0] = vec!["A".to_string()];
        let (_text, keyboard) = render_pending_question(&pending);

        assert_eq!(keyboard[0][0].text, "[x] A");
        assert_eq!(keyboard[0][1].text, "[ ] B");
        assert_eq!(keyboard[1][0].text, "送出");
    }

    fn callback(data: &str) -> TgCallbackQuery {
        TgCallbackQuery {
            update_id: 1,
            callback_query_id: "cb_1".to_string(),
            chat_id: 7,
            message_id: 10,
            data: data.to_string(),
        }
    }

    #[tokio::test]
    async fn single_choice_callback_records_answer_and_replies() {
        let client = MockTgClient::default();
        let responder = RecordingResponder::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        pending
            .lock()
            .unwrap()
            .insert(7, sample_pending_question(false));

        handle_callback_query(
            &client,
            &responder,
            &[7],
            pending.clone(),
            &callback("q:que_1:pick:0:0"),
        )
        .await;

        assert!(pending.lock().unwrap().get(&7).is_none());
        assert_eq!(
            responder.replies.lock().unwrap()[0],
            (
                "ses_1".to_string(),
                "que_1".to_string(),
                vec![vec!["A".to_string()]],
            )
        );
    }

    #[tokio::test]
    async fn multi_choice_callback_toggles_and_submit_replies() {
        let client = MockTgClient::default();
        let responder = RecordingResponder::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        pending
            .lock()
            .unwrap()
            .insert(7, sample_pending_question(true));

        handle_callback_query(
            &client,
            &responder,
            &[7],
            pending.clone(),
            &callback("q:que_1:toggle:0:0"),
        )
        .await;
        handle_callback_query(
            &client,
            &responder,
            &[7],
            pending.clone(),
            &callback("q:que_1:toggle:0:1"),
        )
        .await;
        handle_callback_query(
            &client,
            &responder,
            &[7],
            pending.clone(),
            &callback("q:que_1:next"),
        )
        .await;

        assert_eq!(
            responder.replies.lock().unwrap()[0].2,
            vec![vec!["A".to_string(), "B".to_string()]]
        );
    }

    #[tokio::test]
    async fn cancel_callback_rejects_and_clears_pending() {
        let client = MockTgClient::default();
        let responder = RecordingResponder::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        pending
            .lock()
            .unwrap()
            .insert(7, sample_pending_question(false));

        handle_callback_query(
            &client,
            &responder,
            &[7],
            pending.clone(),
            &callback("q:que_1:cancel"),
        )
        .await;

        assert!(pending.lock().unwrap().get(&7).is_none());
        assert_eq!(
            responder.rejects.lock().unwrap()[0],
            ("ses_1".to_string(), "que_1".to_string())
        );
    }

    #[tokio::test]
    async fn stale_callback_answers_callback_query_without_mutating_state() {
        let client = MockTgClient::default();
        let responder = RecordingResponder::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));

        handle_callback_query(
            &client,
            &responder,
            &[7],
            pending,
            &callback("q:que_1:pick:0:0"),
        )
        .await;

        assert_eq!(
            client.callback_answers.lock().unwrap()[0],
            ("cb_1".to_string(), "這個問題已失效".to_string())
        );
        assert!(responder.replies.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn callback_from_disallowed_chat_is_ignored() {
        let client = MockTgClient::default();
        let responder = RecordingResponder::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        pending
            .lock()
            .unwrap()
            .insert(7, sample_pending_question(false));

        // chat_id 7 is not in the allowlist → the callback must be dropped
        // without touching pending state or answering the query.
        handle_callback_query(
            &client,
            &responder,
            &[999],
            pending.clone(),
            &callback("q:que_1:pick:0:0"),
        )
        .await;

        assert!(pending.lock().unwrap().get(&7).is_some());
        assert!(client.callback_answers.lock().unwrap().is_empty());
        assert!(responder.replies.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn custom_text_is_consumed_as_answer_without_starting_turn() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = RecordingBackend::new(&["should not run"]);
        let responder = RecordingResponder::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let mut question = sample_pending_question(false);
        question.waiting_custom_question_index = Some(0);
        pending.lock().unwrap().insert(7, question);
        let msg = TgMessage {
            update_id: 2,
            chat_id: 7,
            text: "我的自訂答案".to_string(),
            attachments: Vec::new(),
        };

        handle_message_with_responder(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            &responder,
            None,
            &[7],
            pending,
            &msg,
        )
        .await;

        assert!(backend.requests.lock().unwrap().is_empty());
        assert_eq!(
            responder.replies.lock().unwrap()[0].2,
            vec![vec!["我的自訂答案".to_string()]]
        );
    }

    #[tokio::test]
    async fn attachment_only_custom_answer_is_rejected_with_prompt() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = RecordingBackend::new(&["should not run"]);
        let responder = RecordingResponder::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let mut question = sample_pending_question(false);
        question.waiting_custom_question_index = Some(0);
        pending.lock().unwrap().insert(7, question);
        let msg = TgMessage {
            update_id: 2,
            chat_id: 7,
            text: "附件說明".to_string(),
            attachments: vec![TgAttachment {
                kind: TgAttachmentKind::Document,
                file_id: "file_1".to_string(),
                unique_file_id: None,
                original_name: "a.txt".to_string(),
                mime_type: Some("text/plain".to_string()),
                size_bytes: Some(10),
            }],
        };

        handle_message_with_responder(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            &responder,
            None,
            &[7],
            pending.clone(),
            &msg,
        )
        .await;

        assert!(pending.lock().unwrap().contains_key(&7));
        assert!(responder.replies.lock().unwrap().is_empty());
        assert!(client.sent.lock().unwrap()[0].text.contains("請傳文字答案"));
    }

    #[tokio::test]
    async fn expired_question_rejects_and_clears_pending() {
        let client = MockTgClient::default();
        let responder = RecordingResponder::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let mut question = sample_pending_question(false);
        question.deadline = std::time::Instant::now() - std::time::Duration::from_secs(1);
        pending.lock().unwrap().insert(7, question);

        cleanup_expired_questions(&client, &responder, pending.clone()).await;

        assert!(pending.lock().unwrap().is_empty());
        assert_eq!(
            responder.rejects.lock().unwrap()[0],
            ("ses_1".to_string(), "que_1".to_string())
        );
    }

    async fn open_memory_with_url() -> (Memory, String) {
        let f = NamedTempFile::new().unwrap();
        let url = format!("sqlite://{}", f.path().display());
        std::mem::forget(f);
        (Memory::open(&url).await.unwrap(), url)
    }

    async fn open_memory() -> Memory {
        open_memory_with_url().await.0
    }

    fn base_cfg() -> GatewayConfig {
        GatewayConfig {
            scope: String::new(),
            db_url: String::new(),
            agent_command: vec![],
            default_model: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        }
    }

    #[tokio::test]
    async fn ignores_messages_outside_allowlist() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&["answer"]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 999,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        // No reply, no work.
        assert!(client.sent.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn turn_renders_answer_and_consolidates_messages() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        // 2026-11-26（#45）：回合只呼叫一次 primary agent，回覆含 markdown。
        let backend = MockBackend::new(&["**重點** 答案"]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "什麼是 BM25".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;

        // Status bubble deleted once the answer is ready. 2026-11-26（#45）：沒有派工的回合
        // 不更新角色（派工時見 a_dispatched_subagent_shows_in_the_status_bubble_and_live_events）。
        assert!(!client.deletes.lock().unwrap().is_empty());

        // Final answer sent as rendered HTML.
        {
            let sent = client.sent.lock().unwrap();
            assert!(sent
                .iter()
                .any(|s| s.html && s.text.contains("<b>重點</b>")));
        }

        // Stored under the per-chat scope.
        let r = mem
            .recall(wukong_memory::RecallQuery {
                query: "BM25".to_string(),
                top_k: 10,
                scope: Some(scope_for_chat(12)),
                mode: wukong_memory::RecallMode::Hybrid,
            })
            .await
            .unwrap();
        assert!(r.data.iter().any(|h| h.text.contains("User: 什麼是 BM25")));
    }

    #[tokio::test]
    async fn rejected_html_chunk_is_resent_as_plain_and_other_chunks_stay_html() {
        let client = MockTgClient::default().reject_html_containing(
            "乙",
            400,
            "Bad Request: can't parse entities: Unsupported start tag \"think\" at byte offset 13",
        );
        let mem = open_memory().await;
        // 三段各約 3000 bytes，切成三則；第二則被拒。
        let answer = ["甲", "乙", "丙"].map(|c| c.repeat(1000)).join("\n\n");
        let backend = MockBackend::new(&[&answer]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "長回答".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;

        let sent = client.sent.lock().unwrap();
        let answer_parts: Vec<(bool, String)> = sent
            .iter()
            .filter(|s| ["甲", "乙", "丙"].iter().any(|c| s.text.contains(c)))
            .map(|s| (s.html, s.text.trim_end().to_string()))
            .collect();
        assert_eq!(
            answer_parts,
            vec![
                (true, "甲".repeat(1000)),
                (false, "乙".repeat(1000)),
                (true, "丙".repeat(1000)),
            ]
        );
    }

    #[tokio::test]
    async fn turn_records_telegram_user_and_assistant_messages_in_chat_history() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = MockBackend::new(&["telegram answer"]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hello from tg".to_string(),
            attachments: Vec::new(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
        let messages = history.latest_messages(&thread, 10).await.unwrap();
        assert!(messages
            .iter()
            .any(|m| m.role == "user" && m.content == "hello from tg"));
        assert!(messages
            .iter()
            .any(|m| m.role == "assistant" && m.content == "telegram answer"));
    }

    #[tokio::test]
    async fn document_message_stores_attachment_and_passes_to_backend() {
        let upload_dir = tempfile::tempdir().unwrap();
        std::env::set_var("WUKONG_WORKSPACE", upload_dir.path());
        let mem = open_memory().await;
        let backend = RecordingBackend::new(&["attachment answer"]);
        let client =
            MockTgClient::default().with_file("doc_file", "documents/report.pdf", b"pdf".to_vec());
        let (_, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let msg = TgMessage {
            update_id: 1,
            chat_id: 7,
            text: "請分析".to_string(),
            attachments: vec![TgAttachment {
                kind: TgAttachmentKind::Document,
                file_id: "doc_file".to_string(),
                unique_file_id: Some("unique".to_string()),
                original_name: "report.pdf".to_string(),
                mime_type: Some("application/pdf".to_string()),
                size_bytes: Some(3),
            }],
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[7],
            &msg,
        )
        .await;
        std::env::remove_var("WUKONG_WORKSPACE");

        let thread = history.default_thread("user:tg-7").await.unwrap();
        let messages = history.latest_messages(&thread, 10).await.unwrap();
        let user = messages.iter().find(|m| m.role == "user").unwrap();
        let attachments = history.attachments_for_messages(&[user.id]).await.unwrap();
        assert_eq!(attachments.len(), 1);
        assert_eq!(attachments[0].original_name, "report.pdf");
        assert!(std::path::Path::new(&attachments[0].relative_path).is_relative());

        let requests = backend.requests.lock().unwrap();
        assert_eq!(requests.last().unwrap().attachments.len(), 1);
        let working_path = &requests.last().unwrap().attachments[0].path;
        assert!(working_path.ends_with("01-report.pdf"));
        assert!(working_path.to_string_lossy().contains(".wukong/workfiles"));
        let original_path = upload_dir
            .path()
            .join(".wukong/uploads")
            .join(&attachments[0].relative_path);
        assert_eq!(std::fs::read(original_path).unwrap(), b"pdf");
    }

    // #45 SCN-006：檔案互動規則在 agent 定義裡，每回合只附產出目錄這一行。
    #[test]
    fn the_artifact_instruction_is_only_the_directory_line() {
        let workspace = tempfile::tempdir().unwrap();
        let dir = workspace.path().join("out");
        std::fs::create_dir(&dir).unwrap();

        let prompt = prompt_with_artifact_instruction("做一份報表", &dir).unwrap();

        let visible = agent_visible_path(&dir).unwrap();
        assert_eq!(
            prompt,
            format!("做一份報表\n\n[產出目錄] {}", visible.display())
        );
    }

    #[tokio::test]
    async fn controlled_artifacts_are_returned_as_telegram_documents() {
        let artifact_dir = tempfile::tempdir().unwrap();
        std::fs::write(artifact_dir.path().join("fixed.csv"), b"a,b\n1,2\n").unwrap();
        std::fs::create_dir(artifact_dir.path().join("ignored-dir")).unwrap();
        let client = MockTgClient::default();

        let count = return_artifacts(&client, 7, artifact_dir.path())
            .await
            .unwrap();

        assert_eq!(count, 1);
        let documents = client.documents.lock().unwrap();
        assert_eq!(documents[0].chat_id, 7);
        assert_eq!(documents[0].filename, "fixed.csv");
        assert_eq!(documents[0].bytes, b"a,b\n1,2\n");
        assert_eq!(documents[0].caption.as_deref(), Some("OpenCode 產出"));
    }

    #[tokio::test]
    async fn command_records_telegram_user_and_reply_messages_in_chat_history() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = MockBackend::new(&[]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "/new".to_string(),
            attachments: Vec::new(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
        let messages = history.latest_messages(&thread, 10).await.unwrap();
        assert!(messages
            .iter()
            .any(|m| m.role == "user" && m.content == "/new"));
        assert!(messages
            .iter()
            .any(|m| m.role == "assistant" && m.content.contains("已開新")));
    }

    #[tokio::test]
    async fn set_models_command_persists_and_replies() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let dir = tempfile::tempdir().unwrap();
        let settings_path = dir.path().join("settings.json");
        std::env::set_var("WUKONG_SETTINGS_FILE", &settings_path);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 42,
            text: "/set_models opencode/deepseek-v4-flash-free".to_string(),
            attachments: Vec::new(),
        };

        handle_message(&client, &mem, &base_cfg(), &backend, None, &[42], &msg).await;
        std::env::remove_var("WUKONG_SETTINGS_FILE");

        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| s.text.contains("已設定預設模型")));
        drop(sent);

        let saved = wukong_settings::load_settings(&settings_path).unwrap();
        assert_eq!(
            saved.agent.default_model.as_deref(),
            Some("opencode/deepseek-v4-flash-free")
        );
    }

    struct ReasoningBackend;
    impl AiBackend for ReasoningBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "答案".to_string(),
                session_id: None,
            })
        }
        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::Reasoning("想一下".to_string()));
            self.run(req).await
        }
    }

    #[tokio::test]
    async fn reasoning_appears_in_status_bubble() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = ReasoningBackend;
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        let edits = client.edits.lock().unwrap();
        assert!(
            edits
                .iter()
                .any(|(_, _, t)| t.contains("💭") && t.contains("想一下")),
            "no reasoning edit: {edits:?}"
        );
    }

    #[tokio::test]
    async fn native_draft_streams_progress_without_persistent_status_message() {
        let client = MockTgClient::default().with_message_drafts();
        let mem = open_memory().await;
        let backend = ReasoningBackend;
        let msg = TgMessage {
            update_id: 42,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };

        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;

        let drafts = client.drafts.lock().unwrap();
        assert!(
            drafts.len() >= 2,
            "expected progressive draft updates: {drafts:?}"
        );
        assert!(drafts.iter().all(|(_, draft_id, _)| *draft_id == 42));
        assert!(
            drafts.iter().any(|(_, _, text)| text.contains("想一下")),
            "reasoning missing from drafts: {drafts:?}"
        );
        drop(drafts);

        assert!(client.edits.lock().unwrap().is_empty());
        assert!(client.deletes.lock().unwrap().is_empty());
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().all(|message| !message.text.contains("思考中")));
        assert!(sent.iter().any(|message| message.text.contains("答案")));
    }

    struct ToolBackend;
    impl AiBackend for ToolBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "done".to_string(),
                session_id: None,
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::ToolUse("read".to_string()));
            self.run(req).await
        }
    }

    /// #45 SCN-002：primary agent 派工給 fixer，fixer 回報之後 primary 回覆 done。
    struct DispatchBackend;
    impl AiBackend for DispatchBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "done".to_string(),
                session_id: None,
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::SubagentStarted {
                agent: "fixer".to_string(),
            });
            on_event(wukong_gateway::StreamEvent::SubagentReported {
                agent: "fixer".to_string(),
                output: "已建立 hello.txt".to_string(),
            });
            self.run(req).await
        }
    }

    // #45 SCN-002：派工時狀態泡泡顯示「悟空·fixer」，Web 的即時事件也有 role。
    #[tokio::test]
    async fn a_dispatched_subagent_shows_in_the_status_bubble_and_live_events() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "建立 hello.txt".to_string(),
            attachments: Vec::new(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &DispatchBackend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        {
            let edits = client.edits.lock().unwrap();
            assert!(
                edits
                    .iter()
                    .any(|(_, _, t)| t.starts_with("🐵　悟空·fixer")),
                "no role edit: {edits:?}"
            );
        }
        let events = history
            .live_events_after(&scope_for_chat(12), 0, 20)
            .await
            .unwrap();
        assert!(events
            .iter()
            .any(|e| e.kind == "role" && e.content == "fixer"));
    }

    struct QuestionBackend;
    impl AiBackend for QuestionBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "done".to_string(),
                session_id: Some("ses_1".to_string()),
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            on_event(wukong_gateway::StreamEvent::QuestionRequest(
                wukong_gateway::stream::QuestionRequest {
                    request_id: "que_1".to_string(),
                    session_id: "ses_1".to_string(),
                    questions: vec![wukong_gateway::stream::QuestionInfo {
                        question: "選一個".to_string(),
                        header: "偏好".to_string(),
                        multiple: false,
                        custom: true,
                        options: vec![wukong_gateway::stream::QuestionOption {
                            label: "A".to_string(),
                            description: "".to_string(),
                        }],
                    }],
                },
            ));
            self.run(req).await
        }
    }

    #[tokio::test]
    async fn question_request_sends_inline_keyboard_and_tracks_pending() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = QuestionBackend;
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };

        handle_message_with_pending(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            None,
            &[12],
            pending.clone(),
            &msg,
        )
        .await;

        assert!(client.inline_messages.lock().unwrap()[0]
            .1
            .contains("選一個"));
        assert_eq!(
            pending.lock().unwrap().get(&12).unwrap().request_id,
            "que_1"
        );
    }

    #[tokio::test]
    async fn question_request_records_live_question_event_for_web_stream() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = QuestionBackend;
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };

        handle_message_with_pending(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            pending,
            &msg,
        )
        .await;

        let events = history
            .live_events_after(&scope_for_chat(12), 0, 20)
            .await
            .unwrap();
        let question = events
            .iter()
            .find(|event| event.kind == "question")
            .expect("missing question live event");
        assert!(question.content.contains(r#""request_id":"que_1""#));
        assert!(question.content.contains("選一個"));
    }

    #[tokio::test]
    async fn tool_use_appears_in_status_bubble() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = ToolBackend;
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        let edits = client.edits.lock().unwrap();
        assert!(
            edits
                .iter()
                .any(|(_, _, text)| text.contains("使用工具 read")),
            "tool edit missing: {edits:?}"
        );
    }

    #[tokio::test]
    async fn turn_records_telegram_events_in_chat_history() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = ReasoningBackend;
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        let thread = history.default_thread(&scope_for_chat(12)).await.unwrap();
        let messages = history.latest_messages(&thread, 10).await.unwrap();
        let assistant = messages.iter().find(|m| m.role == "assistant").unwrap();
        assert!(assistant.event_count > 0);
        let events = history.list_events(assistant.id).await.unwrap();
        assert!(events.iter().any(|event| event.kind == "reasoning"));
    }

    #[tokio::test]
    async fn telegram_live_events_include_turn_progress_for_web_stream() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = ToolBackend;
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };

        handle_message(
            &client,
            &mem,
            &base_cfg(),
            &backend,
            Some(&history),
            &[12],
            &msg,
        )
        .await;

        let events = history
            .live_events_after(&scope_for_chat(12), 0, 20)
            .await
            .unwrap();
        assert!(events.iter().any(|e| e.kind == "user" && e.content == "hi"));
        // 2026-11-26（#45）：「role」事件只在派工時出現（a_dispatched_subagent_shows_in_the_status_bubble_and_live_events）。
        assert!(events
            .iter()
            .any(|e| e.kind == "tool" && e.label.as_deref() == Some("read")));
        assert!(events
            .iter()
            .any(|e| e.kind == "answer" && e.content == "<p>done</p>"));
    }

    #[test]
    fn bubble_text_keeps_full_reasoning() {
        let reasoning = format!("{}{}", "前段".repeat(80), "後段".repeat(80));
        let text = bubble_text(Some("explorer"), &reasoning);

        assert!(text.contains("💭　前段"), "reasoning head missing: {text}");
        assert!(text.contains("後段"), "reasoning tail missing: {text}");
        assert!(
            !text.contains("💭　…"),
            "reasoning should not be presented as a truncated tail: {text}"
        );
    }

    #[tokio::test]
    async fn turn_sends_status_bubble_and_typing() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&["答案"]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;

        // The first send is the plain status bubble.
        {
            let sent = client.sent.lock().unwrap();
            assert!(sent.iter().any(|s| !s.html && s.text.contains("思考中")));
        }
        assert!(!client.actions.lock().unwrap().is_empty()); // typing emitted
    }

    #[tokio::test]
    async fn new_command_clears_session_and_replies() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        mem.set_agent_session(&scope_for_chat(12), "ses_1")
            .await
            .unwrap();
        let backend = MockBackend::new(&[]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "/new".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        {
            let sent = client.sent.lock().unwrap();
            assert!(sent.iter().any(|s| s.text.contains("已開新")));
        }
        assert_eq!(mem.agent_session(&scope_for_chat(12)).await.unwrap(), None);
    }

    #[tokio::test]
    async fn unknown_command_still_unsupported() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "/model gpt".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        let sent = client.sent.lock().unwrap();
        assert!(sent.iter().any(|s| s.text.contains("尚未支援")));
    }

    #[tokio::test]
    async fn slash_command_replies_unsupported() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&[]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "/reset".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;
        let sent = client.sent.lock().unwrap();
        assert!(sent
            .iter()
            .any(|s| s.chat_id == 12 && s.text.contains("尚未支援")));
    }

    /// Asks one question, then waits for the answer path to signal before
    /// finishing; while waiting after the signal it records whether the
    /// expected progress text showed up before the turn ended (#39).
    struct AnswerWaitingBackend {
        client: MockTgClient,
        answered: Arc<tokio::sync::Notify>,
        expect: &'static str,
        asked: std::sync::atomic::AtomicBool,
        seen_before_end: std::sync::atomic::AtomicBool,
    }

    impl AnswerWaitingBackend {
        fn new(
            client: &MockTgClient,
            answered: Arc<tokio::sync::Notify>,
            expect: &'static str,
        ) -> Self {
            Self {
                client: client.clone(),
                answered,
                expect,
                asked: std::sync::atomic::AtomicBool::new(false),
                seen_before_end: std::sync::atomic::AtomicBool::new(false),
            }
        }

        fn progress_texts(&self) -> Vec<String> {
            let mut texts: Vec<String> = self
                .client
                .drafts
                .lock()
                .unwrap()
                .iter()
                .map(|(_, _, text)| text.clone())
                .collect();
            texts.extend(
                self.client
                    .sent
                    .lock()
                    .unwrap()
                    .iter()
                    .map(|s| s.text.clone()),
            );
            texts
        }
    }

    impl AiBackend for AnswerWaitingBackend {
        async fn run(&self, _req: AgentRequest) -> Result<AgentResponse, GatewayError> {
            Ok(AgentResponse {
                text: "done".to_string(),
                session_id: Some("ses_1".to_string()),
            })
        }

        async fn run_streaming(
            &self,
            req: AgentRequest,
            on_event: &mut dyn FnMut(wukong_gateway::StreamEvent),
        ) -> Result<AgentResponse, GatewayError> {
            if !self.asked.swap(true, std::sync::atomic::Ordering::SeqCst) {
                on_event(wukong_gateway::StreamEvent::QuestionRequest(
                    wukong_gateway::stream::QuestionRequest {
                        request_id: "que_1".to_string(),
                        session_id: "ses_1".to_string(),
                        questions: vec![wukong_gateway::stream::QuestionInfo {
                            question: "選一個".to_string(),
                            header: "偏好".to_string(),
                            multiple: false,
                            custom: true,
                            options: vec![wukong_gateway::stream::QuestionOption {
                                label: "A".to_string(),
                                description: "".to_string(),
                            }],
                        }],
                    },
                ));
                tokio::time::timeout(std::time::Duration::from_secs(5), self.answered.notified())
                    .await
                    .expect("answer path never signalled the backend");
                for _ in 0..30 {
                    if self
                        .progress_texts()
                        .iter()
                        .any(|t| t.contains(self.expect))
                    {
                        self.seen_before_end
                            .store(true, std::sync::atomic::Ordering::SeqCst);
                        break;
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            }
            self.run(req).await
        }
    }

    /// Signals the backend on reply/reject; `fail_reply` makes the reply fail.
    struct SignallingResponder {
        answered: Arc<tokio::sync::Notify>,
        fail_reply: bool,
    }

    impl QuestionResponder for SignallingResponder {
        async fn reply_question(
            &self,
            _session_id: &str,
            _request_id: &str,
            _answers: Vec<Vec<String>>,
        ) -> Result<(), GatewayError> {
            self.answered.notify_one();
            if self.fail_reply {
                Err(GatewayError::AgentFailed {
                    code: None,
                    stderr: "reply failed".to_string(),
                })
            } else {
                Ok(())
            }
        }

        async fn reject_question(
            &self,
            _session_id: &str,
            _request_id: &str,
        ) -> Result<(), GatewayError> {
            self.answered.notify_one();
            Ok(())
        }
    }

    enum Resolve {
        Pick,
        Cancel,
        Expire,
    }

    /// Run one turn that asks a question and resolve it from "outside", the way
    /// the bot's update loop would; returns whether `expect` appeared while the
    /// turn was still running.
    async fn run_question_turn(
        client: &MockTgClient,
        expect: &'static str,
        resolve: Resolve,
        fail_reply: bool,
    ) -> bool {
        let mem = open_memory().await;
        let answered = Arc::new(tokio::sync::Notify::new());
        let backend = AnswerWaitingBackend::new(client, answered.clone(), expect);
        let responder = SignallingResponder {
            answered,
            fail_reply,
        };
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let msg = TgMessage {
            update_id: 42,
            chat_id: 12,
            text: "hi".to_string(),
            attachments: Vec::new(),
        };
        let resolver = async {
            for _ in 0..500 {
                if pending.lock().unwrap().contains_key(&12) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
            assert!(
                !backend.progress_texts().iter().any(|t| t.contains(expect)),
                "progress text shown before the question was resolved"
            );
            let data = match resolve {
                Resolve::Pick => Some("q:que_1:pick:0:0".to_string()),
                Resolve::Cancel => Some(question_callback_data("que_1", QuestionAction::Cancel)),
                Resolve::Expire => None,
            };
            match data {
                Some(data) => {
                    let mut cb = callback(&data);
                    cb.chat_id = 12;
                    handle_callback_query(client, &responder, &[12], pending.clone(), &cb).await;
                }
                None => {
                    pending.lock().unwrap().get_mut(&12).unwrap().deadline =
                        std::time::Instant::now() - std::time::Duration::from_secs(1);
                    cleanup_expired_questions(client, &responder, pending.clone()).await;
                }
            }
        };
        let cfg = base_cfg();
        tokio::join!(
            handle_message_with_responder(
                client,
                &mem,
                &cfg,
                &backend,
                &responder,
                None,
                &[12],
                pending.clone(),
                &msg,
            ),
            resolver
        );
        backend
            .seen_before_end
            .load(std::sync::atomic::Ordering::SeqCst)
    }

    #[tokio::test]
    async fn answered_question_shows_progress_again_as_draft() {
        let client = MockTgClient::default().with_message_drafts();
        assert!(run_question_turn(&client, "已收到回答，繼續處理…", Resolve::Pick, false).await);
        let edits = client.edits.lock().unwrap();
        assert!(edits.iter().any(|(_, _, text)| text == "已送出回答。"));
        let drafts = client.drafts.lock().unwrap();
        let resumed = drafts
            .iter()
            .find(|(_, _, text)| text.contains("已收到回答，繼續處理…"))
            .expect("no resumed draft");
        assert_eq!(
            resumed.1, 42,
            "resumed draft must reuse the turn's draft id"
        );
    }

    #[tokio::test]
    async fn answered_question_shows_progress_again_as_status_message() {
        let client = MockTgClient::default();
        assert!(run_question_turn(&client, "已收到回答，繼續處理…", Resolve::Pick, false).await);
        assert!(client
            .edits
            .lock()
            .unwrap()
            .iter()
            .any(|(_, _, text)| text == "已送出回答。"));
        // The first status message goes when the question is asked, the resumed
        // one when the turn ends; the answer itself stays.
        assert_eq!(client.deletes.lock().unwrap().len(), 2);
        assert!(client
            .sent
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.text.contains("done")));
    }

    #[tokio::test]
    async fn failed_answer_does_not_show_progress_again() {
        let client = MockTgClient::default().with_message_drafts();
        assert!(!run_question_turn(&client, "繼續處理…", Resolve::Pick, true).await);
        assert!(client
            .sent
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.text.contains("回答送出失敗")));
    }

    #[tokio::test]
    async fn cancelled_question_shows_progress_again() {
        let client = MockTgClient::default().with_message_drafts();
        assert!(run_question_turn(&client, "繼續處理…", Resolve::Cancel, false).await);
        assert!(client
            .edits
            .lock()
            .unwrap()
            .iter()
            .any(|(_, _, text)| text == "已取消問題。"));
        assert!(!client
            .drafts
            .lock()
            .unwrap()
            .iter()
            .any(|(_, _, text)| text.contains("已收到回答")));
    }

    #[tokio::test]
    async fn expired_question_shows_progress_again() {
        let client = MockTgClient::default();
        assert!(run_question_turn(&client, "繼續處理…", Resolve::Expire, false).await);
        assert!(client
            .edits
            .lock()
            .unwrap()
            .iter()
            .any(|(_, _, text)| text == "問題已逾時，已取消。"));
        assert!(!client
            .sent
            .lock()
            .unwrap()
            .iter()
            .any(|s| s.text.contains("已收到回答")));
    }

    fn sample_question_request() -> QuestionRequest {
        QuestionRequest {
            request_id: "que_1".to_string(),
            session_id: "ses_1".to_string(),
            questions: sample_pending_question(false).questions,
        }
    }

    fn draft_log(client: &MockTgClient) -> Vec<(i64, String)> {
        client
            .drafts
            .lock()
            .unwrap()
            .iter()
            .map(|(_, id, text)| (*id, text.clone()))
            .collect()
    }

    #[tokio::test(start_paused = true)]
    async fn idle_draft_progress_is_resent_every_20_seconds_except_while_a_question_waits() {
        let client = MockTgClient::default().with_message_drafts();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(run_progress(
            client.clone(),
            12,
            ProgressDisplay::Draft { draft_id: 42 },
            "🐵 收到，思考中…".to_string(),
            rx,
            tx.downgrade(),
            pending.clone(),
        ));
        let second = std::time::Duration::from_secs(1);

        // No event yet: the opening status is resent with the same draft id.
        tokio::time::sleep(19 * second).await;
        assert!(draft_log(&client).is_empty(), "resent too early");
        tokio::time::sleep(2 * second).await;
        assert_eq!(
            draft_log(&client),
            vec![(42, "🐵 收到，思考中…".to_string())]
        );

        // After an event, the timer restarts and the current text is resent.
        tx.send(Progress::ToolUse("read".to_string())).unwrap();
        tokio::time::sleep(second).await;
        let shown = draft_log(&client).last().unwrap().clone();
        assert!(shown.1.contains("使用工具 read"));
        tokio::time::sleep(18 * second).await;
        assert_eq!(draft_log(&client).len(), 2, "resent before 20 idle seconds");
        tokio::time::sleep(2 * second).await;
        assert_eq!(draft_log(&client).len(), 3);
        assert_eq!(draft_log(&client).last().unwrap(), &shown);

        // While a question waits for the user, nothing is resent.
        tx.send(Progress::QuestionRequest(sample_question_request()))
            .unwrap();
        tokio::time::sleep(second).await;
        let asked = draft_log(&client).len();
        tokio::time::sleep(120 * second).await;
        assert_eq!(
            draft_log(&client).len(),
            asked,
            "resent while a question waits"
        );

        // Once answered, the resumed status shows and is kept alive too.
        tx.send(Progress::QuestionAnswered).unwrap();
        tokio::time::sleep(second).await;
        let resumed = draft_log(&client).last().unwrap().clone();
        assert!(resumed.1.contains("已收到回答，繼續處理…"));
        tokio::time::sleep(20 * second).await;
        assert_eq!(draft_log(&client).len(), asked + 2);
        assert_eq!(draft_log(&client).last().unwrap(), &resumed);

        // After the turn ends, nothing more is sent.
        drop(tx);
        task.await.unwrap();
        let ended = draft_log(&client).len();
        tokio::time::sleep(120 * second).await;
        assert_eq!(draft_log(&client).len(), ended);
    }

    #[tokio::test(start_paused = true)]
    async fn progress_events_while_a_question_waits_show_only_after_it_is_answered() {
        for draft in [true, false] {
            let client = if draft {
                MockTgClient::default().with_message_drafts()
            } else {
                MockTgClient::default()
            };
            let display = if draft {
                ProgressDisplay::Draft { draft_id: 42 }
            } else {
                ProgressDisplay::Message {
                    message_id: Some(1),
                }
            };
            let pending = Arc::new(Mutex::new(PendingQuestions::new()));
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let task = tokio::spawn(run_progress(
                client.clone(),
                12,
                display,
                "🐵 收到，思考中…".to_string(),
                rx,
                tx.downgrade(),
                pending,
            ));
            let shown = |client: &MockTgClient| {
                (
                    draft_log(client).len(),
                    client.sent.lock().unwrap().len(),
                    client.edits.lock().unwrap().len(),
                )
            };
            let second = std::time::Duration::from_secs(1);

            tx.send(Progress::QuestionRequest(sample_question_request()))
                .unwrap();
            tokio::time::sleep(second).await;
            let asked = shown(&client);

            // Another tool call of the same step may still report while the
            // question waits; the question message stays at the bottom.
            tx.send(Progress::Role(Role::Fixer)).unwrap();
            tx.send(Progress::Reasoning("想一下".to_string())).unwrap();
            tx.send(Progress::ToolUse("bash".to_string())).unwrap();
            tokio::time::sleep(second).await;
            assert_eq!(shown(&client), asked, "shown while a question waits");

            // Once answered, the progress shows again with what it gathered.
            tx.send(Progress::QuestionAnswered).unwrap();
            tokio::time::sleep(second).await;
            let resumed = if draft {
                draft_log(&client).last().unwrap().1.clone()
            } else {
                client.sent.lock().unwrap().last().unwrap().text.clone()
            };
            assert_eq!(resumed, "🐵　悟空·fixer 已收到回答，繼續處理…");
            drop(tx);
            task.await.unwrap();
        }
    }

    /// Length in UTF-16 units, never fewer than characters: at most 4096 of
    /// them is within Telegram's 4096-character limit (#39).
    fn utf16_len(text: &str) -> usize {
        text.encode_utf16().count()
    }

    #[test]
    fn bubble_text_keeps_the_reasoning_tail_within_the_telegram_limit() {
        let reasoning = format!("開頭{}結尾", "長".repeat(6000));
        let text = bubble_text(Some("oracle"), &reasoning);
        assert!(utf16_len(&text) <= 4096, "{} units", utf16_len(&text));
        assert!(
            text.starts_with("🐵　悟空·oracle 思考中…\n💭　…"),
            "{text:.40}"
        );
        assert!(text.ends_with("長結尾"));
        assert!(!text.contains("開頭"));
        // It keeps as much of the tail as fits (about 4000 units), not a scrap.
        assert!(utf16_len(&text) >= 3990, "{} units", utf16_len(&text));

        // An emoji takes 2 UTF-16 units. With one unit left over the next
        // emoji no longer fits, and the text stays within 4000 units.
        let emoji = format!("開頭{}結尾。", "😀".repeat(3000));
        let text = bubble_text(Some("oracle"), &emoji);
        assert!(utf16_len(&text) <= 4000, "{} units", utf16_len(&text));
        assert!(utf16_len(&text) >= 3990, "{} units", utf16_len(&text));
        assert!(text.ends_with("😀結尾。"));
    }

    #[test]
    fn bubble_text_is_cut_only_past_4000_units() {
        let prefix = "🐵　悟空·oracle 思考中…\n💭　";
        let fits = "長".repeat(4000 - utf16_len(prefix));
        assert_eq!(
            bubble_text(Some("oracle"), &fits),
            format!("{prefix}{fits}")
        );
        let over = format!("{fits}長");
        assert!(bubble_text(Some("oracle"), &over).starts_with(&format!("{prefix}…")));
    }

    #[tokio::test(start_paused = true)]
    async fn long_reasoning_progress_drafts_stay_within_the_telegram_limit() {
        let client = MockTgClient::default().with_message_drafts();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(run_progress(
            client.clone(),
            12,
            ProgressDisplay::Draft { draft_id: 42 },
            "🐵 收到，思考中…".to_string(),
            rx,
            tx.downgrade(),
            pending,
        ));
        tx.send(Progress::Role(Role::Oracle)).unwrap();
        // Distinct tool names keep each update's tail distinct.
        for i in 0..3 {
            tx.send(Progress::Reasoning("想".repeat(3000))).unwrap();
            tx.send(Progress::ToolUse(format!("bash{i}"))).unwrap();
        }
        // Idle long enough for the keep-alive resend of the long text too.
        tokio::time::sleep(std::time::Duration::from_secs(25)).await;
        drop(tx);
        task.await.unwrap();

        let drafts = draft_log(&client);
        assert!(drafts.len() >= 5, "{} drafts", drafts.len());
        for (_, text) in &drafts {
            assert!(utf16_len(text) <= 4096, "{} units", utf16_len(text));
        }
        assert!(drafts.last().unwrap().1.ends_with("▸ 使用工具 bash2"));
        // The last draft is the idle resend of the long text before it.
        let [.., shown, resent] = drafts.as_slice() else {
            panic!("{} drafts", drafts.len());
        };
        assert_eq!(resent, shown, "the long text was not resent when idle");
    }

    #[tokio::test(start_paused = true)]
    async fn status_message_progress_is_not_resent_when_idle() {
        let client = MockTgClient::default();
        let pending = Arc::new(Mutex::new(PendingQuestions::new()));
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(run_progress(
            client.clone(),
            12,
            ProgressDisplay::Message {
                message_id: Some(1),
            },
            "🐵 收到，思考中…".to_string(),
            rx,
            tx.downgrade(),
            pending,
        ));
        tx.send(Progress::ToolUse("read".to_string())).unwrap();
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        assert_eq!(client.edits.lock().unwrap().len(), 1);
        assert!(client.sent.lock().unwrap().is_empty());
        assert!(draft_log(&client).is_empty());
        drop(tx);
        task.await.unwrap();
    }
}
