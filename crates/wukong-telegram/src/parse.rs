//! Pure parsing & policy helpers for the Telegram transport (no network).

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
}
