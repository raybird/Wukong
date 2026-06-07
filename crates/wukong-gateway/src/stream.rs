//! Parsing of opencode `--format json` NDJSON events into render-relevant
//! StreamEvents. opencode emits one JSON object per line.

/// One render-relevant event parsed from the agent's `--format json` stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A chunk of assistant text (opencode "text" part).
    Text(String),
    /// A chunk of reasoning/thinking text (opencode "reasoning" part).
    Reasoning(String),
    /// A tool invocation by name (opencode "tool_use").
    ToolUse(String),
    /// A step begins (drives the spinner).
    StepStart,
    /// A step ends.
    StepFinish,
}

/// Parse one NDJSON line into a StreamEvent. Unrecognized or malformed lines
/// return None and are ignored by callers.
pub fn parse_event(line: &str) -> Option<StreamEvent> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    // opencode nests payload under "part": text at part.text, tool at part.tool.
    let part = v.get("part");
    match v.get("type")?.as_str()? {
        "text" => {
            let t = part
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or_default();
            Some(StreamEvent::Text(t.to_string()))
        }
        "reasoning" => {
            let t = part
                .and_then(|p| p.get("text"))
                .and_then(|t| t.as_str())
                .unwrap_or_default();
            Some(StreamEvent::Reasoning(t.to_string()))
        }
        "tool_use" => {
            let name = part
                .and_then(|p| p.get("tool"))
                .and_then(|n| n.as_str())
                .unwrap_or("tool");
            Some(StreamEvent::ToolUse(name.to_string()))
        }
        "step_start" => Some(StreamEvent::StepStart),
        "step_finish" => Some(StreamEvent::StepFinish),
        _ => None,
    }
}

/// Extract the opencode session id from one NDJSON line (top-level `sessionID`).
pub fn parse_session_id(line: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    v.get("sessionID")?.as_str().map(|s| s.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_event() {
        // opencode nests the assistant text under part.text.
        let ev = parse_event(r#"{"type":"text","part":{"type":"text","text":"hello"}}"#);
        assert_eq!(ev, Some(StreamEvent::Text("hello".to_string())));
    }

    #[test]
    fn parses_tool_use_from_part_tool() {
        assert_eq!(
            parse_event(r#"{"type":"tool_use","part":{"type":"tool","tool":"read"}}"#),
            Some(StreamEvent::ToolUse("read".to_string()))
        );
        // Missing tool name falls back to a generic label.
        assert_eq!(
            parse_event(r#"{"type":"tool_use","part":{"type":"tool"}}"#),
            Some(StreamEvent::ToolUse("tool".to_string()))
        );
    }

    #[test]
    fn parses_step_events() {
        assert_eq!(parse_event(r#"{"type":"step_start"}"#), Some(StreamEvent::StepStart));
        assert_eq!(parse_event(r#"{"type":"step_finish"}"#), Some(StreamEvent::StepFinish));
    }

    #[test]
    fn parses_reasoning_event() {
        let ev = parse_event(r#"{"type":"reasoning","part":{"type":"reasoning","text":"hmm"}}"#);
        assert_eq!(ev, Some(StreamEvent::Reasoning("hmm".to_string())));
    }

    #[test]
    fn extracts_session_id() {
        assert_eq!(
            parse_session_id(r#"{"type":"text","sessionID":"ses_abc","part":{}}"#),
            Some("ses_abc".to_string())
        );
        assert_eq!(parse_session_id("not json"), None);
        assert_eq!(parse_session_id(r#"{"type":"text"}"#), None);
    }

    #[test]
    fn ignores_malformed_and_unknown() {
        assert_eq!(parse_event("not json"), None);
        assert_eq!(parse_event(""), None);
        assert_eq!(parse_event(r#"{"type":"session.updated"}"#), None);
        assert_eq!(parse_event(r#"{"no_type":1}"#), None);
    }
}
