//! Message classification: the seam where future slash commands plug in.
//! v1 only distinguishes "/cmd" (replied as unsupported) from plain turns.
//! Future commands (e.g. /reset, /compact, /model) add arms in the dispatcher
//! without touching this parser.

/// What a received message resolves to.
#[derive(Debug, Clone, PartialEq)]
pub enum MessageAction {
    /// A normal conversational turn.
    Turn(String),
    /// A slash command: name (without '/') and the remaining argument string.
    Command { name: String, args: String },
}

/// Classify a raw message body. Leading/trailing whitespace ignored. A body
/// starting with '/' becomes a Command (name = first token without '/',
/// args = the rest); everything else is a Turn.
pub fn classify_message(text: &str) -> MessageAction {
    let trimmed = text.trim();
    if let Some(rest) = trimmed.strip_prefix('/') {
        let mut parts = rest.splitn(2, char::is_whitespace);
        let name = parts.next().unwrap_or("").to_string();
        let args = parts.next().unwrap_or("").trim().to_string();
        MessageAction::Command { name, args }
    } else {
        MessageAction::Turn(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_a_turn() {
        assert_eq!(
            classify_message("hello there"),
            MessageAction::Turn("hello there".to_string())
        );
    }

    #[test]
    fn slash_becomes_command_with_args() {
        assert_eq!(
            classify_message("/reset now please"),
            MessageAction::Command {
                name: "reset".to_string(),
                args: "now please".to_string()
            }
        );
    }

    #[test]
    fn slash_without_args() {
        assert_eq!(
            classify_message("  /compact  "),
            MessageAction::Command {
                name: "compact".to_string(),
                args: String::new()
            }
        );
    }
}
