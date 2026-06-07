//! wukong-render: render LLM markdown into transport-specific formats.
//! Telegram now (HTML subset); web (to_web_html) reserved for later.

/// Escape the three characters Telegram's HTML parse_mode is sensitive to.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_replaces_specials() {
        assert_eq!(escape_html("a < b & c > d"), "a &lt; b &amp; c &gt; d");
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
    }
}
