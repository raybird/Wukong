//! Per-message dispatch: allowlist → classify → run_turn → reply.

use crate::client::{InlineKeyboard, InlineKeyboardButton, TgClient};
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
            ));
        }
    }

    if let Some(pending) = edit_pending {
        edit_pending_question(client, &pending).await;
    }
    if let Some((session_id, request_id, answers, message_id)) = reply_payload {
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
    format!("{base}\n💭　{r}")
}

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
    Ok(format!(
        "{input}\n\n[Wukong 檔案互動規則]\n上傳附件已是可修改的工作副本，不要修改 .wukong/uploads 內的原始檔。若使用者要求建立、轉換、修改或回傳檔案，請將每個最終成品直接寫入此目錄：{}。只把要回傳給使用者的成品放入該目錄。",
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
            let planner_preferences = wukong_settings::effective_planner_preferences(&settings);
            cfg.apply_planner_preferences(
                planner_preferences.enabled,
                planner_preferences.roles,
                planner_preferences.skills,
            );
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
            let planner_preferences = wukong_settings::effective_planner_preferences(&settings);
            cfg.apply_planner_preferences(
                planner_preferences.enabled,
                planner_preferences.roles,
                planner_preferences.skills,
            );
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
            let progress_display =
                match start_progress_display(client, chat_id, draft_id, "🐵 收到，思考中…").await
                {
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
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Progress>();
            let progress = {
                let c = client.clone();
                let pending_questions = pending_questions.clone();
                tokio::spawn(async move {
                    let mut progress_display = progress_display;
                    let mut role: Option<String> = None;
                    let mut reasoning = String::new();
                    let mut last_reasoning_edit: Option<std::time::Instant> = None;
                    while let Some(msg) = rx.recv().await {
                        match msg {
                            Progress::Role(r) => {
                                role = Some(r.name().to_string());
                                update_progress_display(
                                    &c,
                                    chat_id,
                                    &mut progress_display,
                                    &bubble_text(role.as_deref(), &reasoning),
                                )
                                .await;
                            }
                            Progress::Reasoning(t) => {
                                reasoning.push_str(&t);
                                // Throttle reasoning edits (~1.5s) but never block
                                // the first one (so it always shows up promptly).
                                let due = last_reasoning_edit.is_none_or(|i| {
                                    i.elapsed() >= std::time::Duration::from_millis(1500)
                                });
                                if due {
                                    update_progress_display(
                                        &c,
                                        chat_id,
                                        &mut progress_display,
                                        &bubble_text(role.as_deref(), &reasoning),
                                    )
                                    .await;
                                    last_reasoning_edit = Some(std::time::Instant::now());
                                }
                            }
                            Progress::ToolUse(name) => {
                                reasoning.push_str("\n▸ 使用工具 ");
                                reasoning.push_str(&name);
                                reasoning.push('\n');
                                update_progress_display(
                                    &c,
                                    chat_id,
                                    &mut progress_display,
                                    &bubble_text(role.as_deref(), &reasoning),
                                )
                                .await;
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
                                };
                                let (text, keyboard) = render_pending_question(&pending);
                                if let Ok(message_id) = c
                                    .send_message_with_inline_keyboard(chat_id, &text, keyboard)
                                    .await
                                {
                                    let mut pending = pending;
                                    pending.message_id = Some(message_id);
                                    pending_questions.lock().unwrap().insert(chat_id, pending);
                                }
                            }
                        }
                    }
                    clear_progress_display(&c, chat_id, &mut progress_display).await;
                })
            };

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
                            log_send(client.send_message_html(chat_id, c).await, "answer");
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
            planner_preferences: None,
            thinking: true,
            recall_top_k: 5,
            stream: false,
        }
    }

    #[tokio::test]
    async fn ignores_messages_outside_allowlist() {
        let client = MockTgClient::default();
        let mem = open_memory().await;
        let backend = MockBackend::new(&["oracle", "answer"]);
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
        // planner -> single role; then execute answer with markdown.
        let backend = MockBackend::new(&["oracle", "**重點** 答案"]);
        let msg = TgMessage {
            update_id: 1,
            chat_id: 12,
            text: "什麼是 BM25".to_string(),
            attachments: Vec::new(),
        };
        handle_message(&client, &mem, &base_cfg(), &backend, None, &[12], &msg).await;

        // Status bubble edited per role, then deleted.
        assert!(!client.edits.lock().unwrap().is_empty());
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
    async fn turn_records_telegram_user_and_assistant_messages_in_chat_history() {
        let client = MockTgClient::default();
        let (mem, db_url) = open_memory_with_url().await;
        let history = wukong_chat_history::ChatHistoryStore::open(&db_url)
            .await
            .unwrap();
        let backend = MockBackend::new(&["oracle", "telegram answer"]);
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
        let backend = RecordingBackend::new(&["oracle", "attachment answer"]);
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
        assert!(events.iter().any(|e| e.kind == "role"));
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
        let backend = MockBackend::new(&["oracle", "答案"]);
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
}
