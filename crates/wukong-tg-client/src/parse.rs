//! Pure parsing & policy helpers for the Telegram transport (no network).

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TgAttachmentKind {
    Document,
    Photo,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgAttachment {
    pub kind: TgAttachmentKind,
    pub file_id: String,
    pub unique_file_id: Option<String>,
    pub original_name: String,
    pub mime_type: Option<String>,
    pub size_bytes: Option<i64>,
}

/// A message extracted from a Telegram update.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgMessage {
    pub update_id: i64,
    pub chat_id: i64,
    pub text: String,
    pub attachments: Vec<TgAttachment>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TgCallbackQuery {
    pub update_id: i64,
    pub callback_query_id: String,
    pub chat_id: i64,
    pub message_id: i64,
    pub data: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TgUpdateEvent {
    Message(TgMessage),
    CallbackQuery(TgCallbackQuery),
}

/// Extract user messages from a getUpdates response. Updates without text,
/// captions, or supported attachments are skipped.
pub fn parse_updates(json: &serde_json::Value) -> Vec<TgMessage> {
    parse_update_events(json)
        .into_iter()
        .filter_map(|event| match event {
            TgUpdateEvent::Message(message) => Some(message),
            TgUpdateEvent::CallbackQuery(_) => None,
        })
        .collect()
}

pub fn parse_update_events(json: &serde_json::Value) -> Vec<TgUpdateEvent> {
    let Some(arr) = json.get("result").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|u| {
            parse_callback_query(u)
                .map(TgUpdateEvent::CallbackQuery)
                .or_else(|| parse_message_update(u).map(TgUpdateEvent::Message))
        })
        .collect()
}

fn parse_message_update(u: &serde_json::Value) -> Option<TgMessage> {
    let update_id = u.get("update_id")?.as_i64()?;
    let msg = u.get("message")?;
    let chat_id = msg.get("chat")?.get("id")?.as_i64()?;
    let attachments = parse_attachments(msg);
    let text = msg
        .get("text")
        .or_else(|| msg.get("caption"))
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .or_else(|| {
            attachments
                .first()
                .map(|a| fallback_prompt(&a.original_name))
        })?;
    Some(TgMessage {
        update_id,
        chat_id,
        text,
        attachments,
    })
}

fn parse_callback_query(u: &serde_json::Value) -> Option<TgCallbackQuery> {
    let update_id = u.get("update_id")?.as_i64()?;
    let callback = u.get("callback_query")?;
    let message = callback.get("message")?;
    Some(TgCallbackQuery {
        update_id,
        callback_query_id: callback.get("id")?.as_str()?.to_string(),
        chat_id: message.get("chat")?.get("id")?.as_i64()?,
        message_id: message.get("message_id")?.as_i64()?,
        data: callback.get("data")?.as_str()?.to_string(),
    })
}

fn parse_attachments(msg: &serde_json::Value) -> Vec<TgAttachment> {
    if let Some(document) = msg.get("document") {
        if let Some(file_id) = document.get("file_id").and_then(|v| v.as_str()) {
            return vec![TgAttachment {
                kind: TgAttachmentKind::Document,
                file_id: file_id.to_string(),
                unique_file_id: document
                    .get("file_unique_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                original_name: document
                    .get("file_name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("document")
                    .to_string(),
                mime_type: document
                    .get("mime_type")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                size_bytes: document.get("file_size").and_then(|v| v.as_i64()),
            }];
        }
    }

    msg.get("photo")
        .and_then(|v| v.as_array())
        .and_then(|photos| {
            photos
                .iter()
                .filter_map(|photo| Some((photo, photo.get("file_size")?.as_i64()?)))
                .max_by_key(|(_, size)| *size)
                .map(|(photo, _)| photo)
        })
        .and_then(|photo| {
            Some(TgAttachment {
                kind: TgAttachmentKind::Photo,
                file_id: photo.get("file_id")?.as_str()?.to_string(),
                unique_file_id: photo
                    .get("file_unique_id")
                    .and_then(|v| v.as_str())
                    .map(str::to_string),
                original_name: "photo.jpg".to_string(),
                mime_type: Some("image/jpeg".to_string()),
                size_bytes: photo.get("file_size").and_then(|v| v.as_i64()),
            })
        })
        .into_iter()
        .collect()
}

fn fallback_prompt(name: &str) -> String {
    format!("使用者上傳了 {name}，請分析附件內容。")
}

/// The highest update_id across ALL updates (any type), used to advance the
/// long-poll offset so non-text updates are not re-delivered forever.
pub fn highest_update_id(json: &serde_json::Value) -> Option<i64> {
    json.get("result")?
        .as_array()?
        .iter()
        .filter_map(|u| u.get("update_id").and_then(|v| v.as_i64()))
        .max()
}

/// Parse a comma-separated allowlist of chat ids. Whitespace tolerant; empty
/// entries skipped.
pub fn parse_allowlist(s: &str) -> Vec<i64> {
    s.split(',')
        .map(|t| t.trim())
        .filter(|t| !t.is_empty())
        .filter_map(|t| t.parse::<i64>().ok())
        .collect()
}

/// Whether a chat id is in the allowlist.
pub fn is_allowed(chat_id: i64, allow: &[i64]) -> bool {
    allow.contains(&chat_id)
}

/// The memory scope for a given chat.
pub fn scope_for_chat(chat_id: i64) -> String {
    format!("user:tg-{chat_id}")
}

/// Recover the Telegram chat id from a scope produced by `scope_for_chat`.
/// Returns None for scopes that are not Telegram-originated (e.g. `project:X`),
/// so non-Telegram scheduled jobs are never mistakenly delivered to a chat.
pub fn chat_id_from_scope(scope: &str) -> Option<i64> {
    scope
        .strip_prefix("user:tg-")
        .and_then(|s| s.parse::<i64>().ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_allowlist_handles_spaces_and_empties() {
        assert_eq!(parse_allowlist("12, 34 ,,56"), vec![12, 34, 56]);
        assert!(parse_allowlist("").is_empty());
        assert!(parse_allowlist("  ").is_empty());
    }

    #[test]
    fn is_allowed_checks_membership() {
        assert!(is_allowed(12, &[12, 34]));
        assert!(!is_allowed(99, &[12, 34]));
        assert!(!is_allowed(12, &[]));
    }

    #[test]
    fn scope_for_chat_formats_id() {
        assert_eq!(scope_for_chat(42), "user:tg-42");
        assert_eq!(scope_for_chat(-100), "user:tg--100");
    }

    #[test]
    fn chat_id_from_scope_round_trips_and_rejects_other_scopes() {
        assert_eq!(chat_id_from_scope("user:tg-42"), Some(42));
        // Group chats have negative ids.
        assert_eq!(chat_id_from_scope("user:tg--100"), Some(-100));
        assert_eq!(chat_id_from_scope(&scope_for_chat(123)), Some(123));
        // Non-Telegram scopes must not resolve to a chat.
        assert_eq!(chat_id_from_scope("project:Wukong"), None);
        assert_eq!(chat_id_from_scope("global"), None);
        assert_eq!(chat_id_from_scope("user:tg-abc"), None);
    }

    #[test]
    fn parse_updates_extracts_text_messages() {
        let json = serde_json::json!({
            "ok": true,
            "result": [
                {"update_id": 10, "message": {"chat": {"id": 12}, "text": "hello"}},
                {"update_id": 11, "message": {"chat": {"id": 34}, "text": "world"}}
            ]
        });
        let msgs = parse_updates(&json);
        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[0].update_id, 10);
        assert_eq!(msgs[0].chat_id, 12);
        assert_eq!(msgs[0].text, "hello");
        assert_eq!(msgs[1].chat_id, 34);
    }

    #[test]
    fn parse_updates_skips_non_text_updates() {
        let json = serde_json::json!({
            "result": [
                {"update_id": 1, "message": {"chat": {"id": 5}}},   // no text
                {"update_id": 2, "edited_message": {"chat": {"id": 5}, "text": "x"}}, // not "message"
                {"update_id": 3, "message": {"chat": {"id": 5}, "text": "ok"}}
            ]
        });
        let msgs = parse_updates(&json);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].update_id, 3);
        assert_eq!(msgs[0].text, "ok");
    }

    #[test]
    fn parse_update_events_extracts_callback_query() {
        let json = serde_json::json!({
            "result": [{
                "update_id": 42,
                "callback_query": {
                    "id": "cb_1",
                    "data": "q:que_1:pick:0:1",
                    "message": {
                        "message_id": 99,
                        "chat": { "id": 7 }
                    }
                }
            }]
        });

        assert_eq!(
            parse_update_events(&json),
            vec![TgUpdateEvent::CallbackQuery(TgCallbackQuery {
                update_id: 42,
                callback_query_id: "cb_1".to_string(),
                chat_id: 7,
                message_id: 99,
                data: "q:que_1:pick:0:1".to_string(),
            })]
        );
    }

    #[test]
    fn parse_updates_extracts_document_with_caption() {
        let json = serde_json::json!({
            "result": [{
                "update_id": 10,
                "message": {
                    "chat": {"id": 12},
                    "caption": "請分析",
                    "document": {
                        "file_id": "doc_file",
                        "file_unique_id": "doc_unique",
                        "file_name": "report.pdf",
                        "mime_type": "application/pdf",
                        "file_size": 1234
                    }
                }
            }]
        });
        let msgs = parse_updates(&json);
        assert_eq!(msgs.len(), 1);
        assert_eq!(msgs[0].text, "請分析");
        assert_eq!(msgs[0].attachments.len(), 1);
        assert_eq!(msgs[0].attachments[0].file_id, "doc_file");
        assert_eq!(msgs[0].attachments[0].original_name, "report.pdf");
    }

    #[test]
    fn parse_updates_extracts_largest_photo_with_fallback_prompt() {
        let json = serde_json::json!({
            "result": [{
                "update_id": 11,
                "message": {
                    "chat": {"id": 12},
                    "photo": [
                        {"file_id": "small", "file_unique_id": "u1", "file_size": 10, "width": 100, "height": 100},
                        {"file_id": "large", "file_unique_id": "u2", "file_size": 20, "width": 1200, "height": 900}
                    ]
                }
            }]
        });
        let msgs = parse_updates(&json);
        assert_eq!(msgs.len(), 1);
        assert!(
            msgs[0].text.contains("使用者上傳了 photo.jpg"),
            "{}",
            msgs[0].text
        );
        assert_eq!(msgs[0].attachments[0].file_id, "large");
        assert_eq!(
            msgs[0].attachments[0].mime_type.as_deref(),
            Some("image/jpeg")
        );
    }

    #[test]
    fn highest_update_id_scans_all_updates() {
        let json = serde_json::json!({
            "result": [
                {"update_id": 7, "message": {"chat": {"id": 5}}},
                {"update_id": 9, "edited_message": {}},
                {"update_id": 8, "message": {"chat": {"id": 5}, "text": "ok"}}
            ]
        });
        // Must advance past ALL updates, even non-text ones, or they re-deliver.
        assert_eq!(highest_update_id(&json), Some(9));
    }

    #[test]
    fn highest_update_id_none_for_empty() {
        let json = serde_json::json!({ "result": [] });
        assert_eq!(highest_update_id(&json), None);
    }
}
