//! Pure parsing & policy helpers for the Telegram transport (no network).

/// A text message extracted from a Telegram update.
#[derive(Debug, Clone, PartialEq)]
pub struct TgMessage {
    pub update_id: i64,
    pub chat_id: i64,
    pub text: String,
}

/// Extract text messages from a getUpdates response. Updates without a
/// top-level `message.text` (edits, photos, etc.) are skipped.
pub fn parse_updates(json: &serde_json::Value) -> Vec<TgMessage> {
    let Some(arr) = json.get("result").and_then(|r| r.as_array()) else {
        return Vec::new();
    };
    arr.iter()
        .filter_map(|u| {
            let update_id = u.get("update_id")?.as_i64()?;
            let msg = u.get("message")?;
            let chat_id = msg.get("chat")?.get("id")?.as_i64()?;
            let text = msg.get("text")?.as_str()?.to_string();
            Some(TgMessage { update_id, chat_id, text })
        })
        .collect()
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
    scope.strip_prefix("user:tg-").and_then(|s| s.parse::<i64>().ok())
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
