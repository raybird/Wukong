//! Parsing of opencode `--format json` NDJSON events into render-relevant
//! StreamEvents. opencode emits one JSON object per line.

/// One render-relevant event parsed from the agent's `--format json` stream.
#[derive(Debug, Clone, PartialEq)]
pub enum StreamEvent {
    /// A chunk of assistant text (opencode "text" part).
    Text(String),
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
    match v.get("type")?.as_str()? {
        "text" => {
            let t = v.get("text").and_then(|t| t.as_str()).unwrap_or_default();
            Some(StreamEvent::Text(t.to_string()))
        }
        "tool_use" => {
            // tool name may live under "name" or "tool"; fall back to "tool".
            let name = v
                .get("name")
                .and_then(|n| n.as_str())
                .or_else(|| v.get("tool").and_then(|n| n.as_str()))
                .unwrap_or("tool");
            Some(StreamEvent::ToolUse(name.to_string()))
        }
        "step_start" => Some(StreamEvent::StepStart),
        "step_finish" => Some(StreamEvent::StepFinish),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_text_event() {
        let ev = parse_event(r#"{"type":"text","text":"hello"}"#);
        assert_eq!(ev, Some(StreamEvent::Text("hello".to_string())));
    }

    #[test]
    fn parses_tool_use_with_name_or_tool() {
        assert_eq!(
            parse_event(r#"{"type":"tool_use","name":"read"}"#),
            Some(StreamEvent::ToolUse("read".to_string()))
        );
        assert_eq!(
            parse_event(r#"{"type":"tool_use","tool":"edit"}"#),
            Some(StreamEvent::ToolUse("edit".to_string()))
        );
    }

    #[test]
    fn parses_step_events() {
        assert_eq!(parse_event(r#"{"type":"step_start"}"#), Some(StreamEvent::StepStart));
        assert_eq!(parse_event(r#"{"type":"step_finish"}"#), Some(StreamEvent::StepFinish));
    }

    #[test]
    fn ignores_malformed_and_unknown() {
        assert_eq!(parse_event("not json"), None);
        assert_eq!(parse_event(""), None);
        assert_eq!(parse_event(r#"{"type":"session.updated"}"#), None);
        assert_eq!(parse_event(r#"{"no_type":1}"#), None);
    }
}
