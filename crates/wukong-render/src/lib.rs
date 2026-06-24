//! wukong-render: render LLM markdown into transport-specific formats.
//! Telegram now (HTML subset); web (to_web_html) reserved for later.

use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

/// Escape the three characters Telegram's HTML parse_mode is sensitive to.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Render GFM markdown into Telegram-supported HTML, split into chunks of at
/// most 4096 chars. Empty input yields an empty Vec.
pub fn to_telegram_html(markdown: &str) -> Vec<String> {
    if markdown.trim().is_empty() {
        return Vec::new();
    }
    let html = render_html(markdown);
    let trimmed = html.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    split_chunks(trimmed, 4096)
}

/// Render GFM markdown into complete, browser-native HTML (real `<table>`,
/// `<pre><code>`, lists). Raw HTML in the source is mapped to text so it is
/// escaped by the renderer — this prevents an LLM from injecting `<script>`.
/// Empty input yields an empty string.
pub fn to_web_html(markdown: &str) -> String {
    if markdown.trim().is_empty() {
        return String::new();
    }
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let events = Parser::new_ext(markdown, opts).map(|ev| match ev {
        // Treat any raw HTML as literal text → push_html will escape it.
        Event::Html(t) => Event::Text(t),
        Event::InlineHtml(t) => Event::Text(t),
        other => other,
    });
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events);
    html.trim().to_string()
}

/// Walk markdown events and emit a Telegram HTML-subset string.
fn render_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let parser = Parser::new_ext(markdown, opts);

    let mut out = String::new();
    // Table accumulation state.
    let mut in_table = false;
    let mut table: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();

    for ev in parser {
        if in_table {
            match ev {
                Event::Start(Tag::TableHead) | Event::Start(Tag::TableRow) => row = Vec::new(),
                Event::Start(Tag::TableCell) => cell = String::new(),
                Event::End(TagEnd::TableCell) => row.push(cell.trim().to_string()),
                Event::End(TagEnd::TableHead) | Event::End(TagEnd::TableRow) => {
                    table.push(std::mem::take(&mut row))
                }
                Event::End(TagEnd::Table) => {
                    out.push_str(&render_table(&table));
                    in_table = false;
                    table.clear();
                }
                Event::Text(t) | Event::Code(t) => cell.push_str(&t),
                _ => {}
            }
            continue;
        }
        match ev {
            Event::Start(Tag::Strong) => out.push_str("<b>"),
            Event::End(TagEnd::Strong) => out.push_str("</b>"),
            Event::Start(Tag::Emphasis) => out.push_str("<i>"),
            Event::End(TagEnd::Emphasis) => out.push_str("</i>"),
            Event::Start(Tag::Strikethrough) => out.push_str("<s>"),
            Event::End(TagEnd::Strikethrough) => out.push_str("</s>"),
            Event::Start(Tag::Heading { .. }) => out.push_str("<b>"),
            Event::End(TagEnd::Heading(_)) => out.push_str("</b>\n"),
            Event::Start(Tag::BlockQuote(_)) => out.push_str("<blockquote>"),
            Event::End(TagEnd::BlockQuote(_)) => out.push_str("</blockquote>\n"),
            Event::Start(Tag::CodeBlock(_)) => out.push_str("<pre>"),
            Event::End(TagEnd::CodeBlock) => out.push_str("</pre>\n"),
            Event::Start(Tag::Item) => out.push_str("• "),
            Event::End(TagEnd::Item) => out.push('\n'),
            Event::Start(Tag::Link { dest_url, .. }) => {
                out.push_str(&format!(r#"<a href="{}">"#, escape_html(&dest_url)));
            }
            Event::End(TagEnd::Link) => out.push_str("</a>"),
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                table.clear();
            }
            Event::End(TagEnd::Paragraph) => out.push_str("\n\n"),
            Event::Code(t) => out.push_str(&format!("<code>{}</code>", escape_html(&t))),
            Event::Text(t) => out.push_str(&escape_html(&t)),
            // Raw HTML in the source is treated as literal text (escaped) for safety.
            Event::Html(t) | Event::InlineHtml(t) => out.push_str(&escape_html(&t)),
            Event::SoftBreak | Event::HardBreak => out.push('\n'),
            Event::Rule => out.push_str("——————\n"),
            _ => {}
        }
    }
    out
}

/// Render an accumulated table as an aligned monospace <pre> block.
fn render_table(rows: &[Vec<String>]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cols = rows.iter().map(|r| r.len()).max().unwrap_or(0);
    let mut widths = vec![0usize; cols];
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            widths[i] = widths[i].max(c.chars().count());
        }
    }
    let mut s = String::from("<pre>");
    for r in rows {
        let mut line = String::new();
        for (i, w) in widths.iter().enumerate() {
            let cellv = r.get(i).map(|x| x.as_str()).unwrap_or("");
            let pad = w.saturating_sub(cellv.chars().count());
            line.push_str(cellv);
            line.push_str(&" ".repeat(pad));
            if i + 1 < cols {
                line.push_str("  ");
            }
        }
        s.push_str(&escape_html(line.trim_end()));
        s.push('\n');
    }
    s.push_str("</pre>\n");
    s
}

/// Split rendered HTML into chunks ≤ max chars, breaking on newline boundaries
/// so HTML tags are never split mid-tag.
fn split_chunks(html: &str, max: usize) -> Vec<String> {
    if html.len() <= max {
        return vec![html.to_string()];
    }
    let mut chunks = Vec::new();
    let mut cur = String::new();
    for line in html.split_inclusive('\n') {
        if cur.len() + line.len() > max && !cur.is_empty() {
            chunks.push(std::mem::take(&mut cur).trim_end().to_string());
        }
        if line.len() > max {
            let mut rest = line;
            while rest.len() > max {
                let (a, b) = rest.split_at(max);
                chunks.push(a.to_string());
                rest = b;
            }
            cur.push_str(rest);
        } else {
            cur.push_str(line);
        }
    }
    if !cur.trim().is_empty() {
        chunks.push(cur.trim_end().to_string());
    }
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_html_replaces_specials() {
        assert_eq!(escape_html("a < b & c > d"), "a &lt; b &amp; c &gt; d");
        assert_eq!(escape_html("<script>"), "&lt;script&gt;");
    }

    #[test]
    fn renders_bold_italic_inline_code() {
        let out = to_telegram_html("**bold** and *it* and `co`").join("");
        assert!(out.contains("<b>bold</b>"));
        assert!(out.contains("<i>it</i>"));
        assert!(out.contains("<code>co</code>"));
    }

    #[test]
    fn renders_code_block_as_pre() {
        let out = to_telegram_html("```\nlet x = 1;\n```").join("");
        assert!(out.contains("<pre>"));
        assert!(out.contains("let x = 1;"));
        assert!(out.contains("</pre>"));
    }

    #[test]
    fn renders_heading_as_bold() {
        let out = to_telegram_html("# Title").join("");
        assert!(out.contains("<b>Title</b>"));
    }

    #[test]
    fn renders_link() {
        let out = to_telegram_html("[docs](https://x.io)").join("");
        assert!(out.contains(r#"<a href="https://x.io">docs</a>"#));
    }

    #[test]
    fn renders_list_items_with_bullets() {
        let out = to_telegram_html("- one\n- two").join("");
        assert!(out.contains("• one"));
        assert!(out.contains("• two"));
    }

    #[test]
    fn escapes_text_content() {
        let out = to_telegram_html("a <script> tag").join("");
        assert!(out.contains("&lt;script&gt;"));
        assert!(!out.contains("<script>"));
    }

    #[test]
    fn empty_input_yields_empty_vec() {
        assert!(to_telegram_html("").is_empty());
    }

    #[test]
    fn renders_table_as_pre_block() {
        let md = "| a | b |\n| - | - |\n| 1 | 2 |";
        let out = to_telegram_html(md).join("");
        assert!(out.contains("<pre>"));
        assert!(out.contains('a') && out.contains('b'));
        assert!(out.contains('1') && out.contains('2'));
        assert!(out.contains("</pre>"));
    }

    #[test]
    fn web_renders_bold_and_table() {
        let out = to_web_html("**ans**\n\n| a | b |\n| - | - |\n| 1 | 2 |");
        assert!(out.contains("<strong>ans</strong>"));
        assert!(out.contains("<table>"));
        assert!(out.contains("<td>1</td>"));
    }

    #[test]
    fn web_renders_code_block() {
        let out = to_web_html("```\nlet x = 1;\n```");
        assert!(out.contains("<pre><code"));
        assert!(out.contains("let x = 1;"));
    }

    #[test]
    fn web_escapes_raw_html() {
        let out = to_web_html("a <script>alert(1)</script> tag");
        assert!(out.contains("&lt;script&gt;"));
        assert!(!out.contains("<script>"));
    }

    #[test]
    fn web_empty_input_is_empty_string() {
        assert_eq!(to_web_html(""), "");
    }

    #[test]
    fn long_output_splits_into_multiple_chunks() {
        let md = (0..200)
            .map(|i| format!("line number {i} with some words"))
            .collect::<Vec<_>>()
            .join("\n\n");
        let chunks = to_telegram_html(&md);
        assert!(
            chunks.len() > 1,
            "expected multiple chunks, got {}",
            chunks.len()
        );
        assert!(chunks.iter().all(|c| c.len() <= 4096));
    }
}
