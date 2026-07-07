#[derive(Default)]
pub(super) struct SseParser {
    data_lines: Vec<String>,
}

impl SseParser {
    pub(super) fn feed_line(&mut self, line: &str) -> Option<String> {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            if self.data_lines.is_empty() {
                return None;
            }
            return Some(std::mem::take(&mut self.data_lines).join("\n"));
        }
        if line.starts_with(':') {
            return None;
        }
        if let Some(data) = line.strip_prefix("data:") {
            self.data_lines
                .push(data.strip_prefix(' ').unwrap_or(data).to_string());
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sse_parser_collects_single_data_event() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line("data: {\"hello\":true}"), None);
        assert_eq!(parser.feed_line(""), Some("{\"hello\":true}".to_string()));
    }

    #[test]
    fn sse_parser_joins_multiline_data_and_ignores_comments() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line(": keep-alive"), None);
        assert_eq!(parser.feed_line("event: message"), None);
        assert_eq!(parser.feed_line("data: {\"a\":"), None);
        assert_eq!(parser.feed_line("data: 1}"), None);

        assert_eq!(parser.feed_line(""), Some("{\"a\":\n1}".to_string()));
    }

    #[test]
    fn sse_parser_ignores_blank_events() {
        let mut parser = SseParser::default();

        assert_eq!(parser.feed_line("event: ping"), None);
        assert_eq!(parser.feed_line(""), None);
    }
}
