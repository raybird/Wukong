//! wukong-render: render LLM markdown into transport-specific formats.
//! Telegram now (HTML subset); web (to_web_html) reserved for later.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};

/// Escape the three characters Telegram's HTML parse_mode is sensitive to.
fn escape_html(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Whether a link/image URL is safe to emit as a clickable `href`/`src`.
///
/// Only `http`, `https`, `mailto`, `tel` and scheme-relative/relative URLs are
/// allowed. Everything else (notably `javascript:` and `data:`) is rejected so
/// an LLM answer cannot inject a clickable script URL. Whitespace and control
/// characters inside the scheme are ignored to defeat obfuscation like
/// `java\tscript:`.
fn is_safe_url(url: &str) -> bool {
    let mut scheme = String::new();
    for ch in url.trim_start().chars() {
        match ch {
            ':' => {
                return matches!(
                    scheme.to_ascii_lowercase().as_str(),
                    "http" | "https" | "mailto" | "tel"
                );
            }
            // A path/query/fragment separator before any ':' means the URL is
            // relative — no scheme, so it is safe.
            '/' | '?' | '#' => return true,
            c if c.is_ascii_whitespace() || c.is_control() => continue,
            c => scheme.push(c),
        }
    }
    // No ':' at all → relative URL.
    true
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
    let mut events: Vec<Event> = Vec::new();
    let mut stripped_link = 0usize;
    let mut stripped_image = 0usize;
    for ev in Parser::new_ext(markdown, opts) {
        match ev {
            // Treat any raw HTML as literal text → push_html will escape it.
            Event::Html(t) => events.push(Event::Text(t)),
            Event::InlineHtml(t) => events.push(Event::Text(t)),
            // Drop anchors/images with an unsafe scheme but keep their inner
            // text, so a `javascript:`/`data:` URL never becomes clickable.
            Event::Start(Tag::Link { ref dest_url, .. }) if !is_safe_url(dest_url) => {
                stripped_link += 1;
            }
            Event::End(TagEnd::Link) if stripped_link > 0 => {
                stripped_link -= 1;
            }
            Event::Start(Tag::Image { ref dest_url, .. }) if !is_safe_url(dest_url) => {
                stripped_image += 1;
            }
            Event::End(TagEnd::Image) if stripped_image > 0 => {
                stripped_image -= 1;
            }
            other => events.push(other),
        }
    }
    let mut html = String::new();
    pulldown_cmark::html::push_html(&mut html, events.into_iter());
    html.trim().to_string()
}

/// Indent added per nested list level in Telegram output.
const LIST_INDENT: &str = "  ";
/// Bullet per nesting level; deeper levels reuse the last one.
const BULLETS: [&str; 3] = ["• ", "◦ ", "▪ "];

/// Cut `<think>…</think>` regions out of the markdown source before rendering,
/// so the renderer only ever sees a complete document and its output stays
/// balanced. Tags are looked for only in raw-HTML events, so examples inside
/// code spans and code blocks stay. Nested blocks are counted by depth; an
/// unclosed block runs to the end of the message.
fn strip_think(markdown: &str, opts: Options) -> String {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    let mut ranges = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (ev, range) in Parser::new_ext(markdown, opts).into_offset_iter() {
        if !matches!(ev, Event::Html(_) | Event::InlineHtml(_)) {
            continue;
        }
        let fragment = &markdown[range.clone()];
        for (i, _) in fragment.match_indices('<') {
            let at = range.start + i;
            if fragment[i..].starts_with(OPEN) {
                if depth == 0 {
                    start = at;
                }
                depth += 1;
            } else if depth > 0 && fragment[i..].starts_with(CLOSE) {
                depth -= 1;
                if depth == 0 {
                    ranges.push(start..at + CLOSE.len());
                }
            }
        }
    }
    if depth > 0 {
        ranges.push(start..markdown.len());
    }
    let mut visible = String::with_capacity(markdown.len());
    let mut kept_from = 0;
    for r in ranges {
        visible.push_str(&markdown[kept_from..r.start]);
        kept_from = r.end;
    }
    visible.push_str(&markdown[kept_from..]);
    visible
}

/// Walk markdown events and emit a Telegram HTML-subset string.
fn render_html(markdown: &str) -> String {
    let mut opts = Options::empty();
    opts.insert(Options::ENABLE_TABLES);
    opts.insert(Options::ENABLE_STRIKETHROUGH);
    let markdown = strip_think(markdown, opts);
    let parser = Parser::new_ext(&markdown, opts);

    let mut out = String::new();
    // Table accumulation state.
    let mut in_table = false;
    let mut table: Vec<Vec<String>> = Vec::new();
    let mut row: Vec<String> = Vec::new();
    let mut cell = String::new();
    // True while inside a link whose scheme was rejected: emit the text, no anchor.
    let mut link_stripped = false;
    // One entry per open list: None = bullet list, Some(n) = next item number.
    let mut lists: Vec<Option<u64>> = Vec::new();
    // One entry per open list item: the column where its text starts, so
    // continuation lines and paragraphs line up under it.
    let mut item_cols: Vec<usize> = Vec::new();
    // Telegram HTML has no nested blockquote; only the outermost level is emitted.
    let mut quote_depth = 0usize;

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
            Event::Start(Tag::Heading {
                level: HeadingLevel::H1,
                ..
            }) => out.push_str("<u><b>"),
            Event::End(TagEnd::Heading(HeadingLevel::H1)) => out.push_str("</b></u>\n"),
            Event::Start(Tag::Heading { .. }) => out.push_str("<b>"),
            Event::End(TagEnd::Heading(_)) => out.push_str("</b>\n"),
            Event::Start(Tag::BlockQuote(_)) => {
                quote_depth += 1;
                if quote_depth == 1 {
                    out.push_str("<blockquote>");
                }
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                quote_depth -= 1;
                if quote_depth == 0 {
                    out.push_str("</blockquote>\n");
                }
            }
            Event::Start(Tag::CodeBlock(_)) => out.push_str("<pre>"),
            Event::End(TagEnd::CodeBlock) => out.push_str("</pre>\n"),
            Event::Start(Tag::List(start)) => {
                // A nested list starts inside its parent item's line.
                if !lists.is_empty() && !out.ends_with('\n') {
                    out.push('\n');
                }
                lists.push(start);
            }
            Event::End(TagEnd::List(_)) => {
                lists.pop();
            }
            Event::Start(Tag::Item) => {
                let depth = lists.len().max(1);
                let indent = LIST_INDENT.repeat(depth - 1);
                let marker = match lists.last_mut() {
                    Some(Some(n)) => {
                        *n += 1;
                        format!("{}. ", *n - 1)
                    }
                    _ => BULLETS[(depth - 1).min(BULLETS.len() - 1)].to_string(),
                };
                item_cols.push(indent.len() + marker.chars().count());
                out.push_str(&indent);
                out.push_str(&marker);
            }
            Event::End(TagEnd::Item) => {
                item_cols.pop();
                if !out.ends_with('\n') {
                    out.push('\n');
                }
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                if is_safe_url(&dest_url) {
                    out.push_str(&format!(r#"<a href="{}">"#, escape_html(&dest_url)));
                } else {
                    link_stripped = true;
                }
            }
            Event::End(TagEnd::Link) => {
                if link_stripped {
                    link_stripped = false;
                } else {
                    out.push_str("</a>");
                }
            }
            Event::Start(Tag::Table(_)) => {
                in_table = true;
                table.clear();
            }
            // A later paragraph of a list item starts on a fresh line.
            Event::Start(Tag::Paragraph) => {
                if let (Some(col), true) = (item_cols.last(), out.ends_with('\n')) {
                    out.push_str(&" ".repeat(*col));
                }
            }
            Event::End(TagEnd::Paragraph) => out.push_str("\n\n"),
            Event::Code(t) => out.push_str(&format!("<code>{}</code>", escape_html(&t))),
            Event::Text(t) => out.push_str(&escape_html(&t)),
            // Raw HTML in the source is treated as literal text (escaped) for safety.
            Event::Html(t) | Event::InlineHtml(t) => out.push_str(&escape_html(&t)),
            Event::SoftBreak | Event::HardBreak => {
                out.push('\n');
                if let Some(col) = item_cols.last() {
                    out.push_str(&" ".repeat(*col));
                }
            }
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

/// A token from our own Telegram HTML output: either a full tag (`<pre>`,
/// `</pre>`, `<a href="…">`) or a run of literal text. Because `render_html`
/// escapes every `<`/`>`/`&` in text, any literal `<` here starts a real tag,
/// which makes tokenizing unambiguous.
enum Token<'a> {
    Tag(&'a str),
    Text(&'a str),
}

fn tokenize(html: &str) -> Vec<Token<'_>> {
    let mut toks = Vec::new();
    let bytes = html.as_bytes();
    let (mut i, mut text_start) = (0usize, 0usize);
    while i < bytes.len() {
        if bytes[i] == b'<' {
            if text_start < i {
                toks.push(Token::Text(&html[text_start..i]));
            }
            match html[i..].find('>') {
                Some(rel) => {
                    let end = i + rel + 1;
                    toks.push(Token::Tag(&html[i..end]));
                    i = end;
                    text_start = end;
                }
                // Unterminated '<' (should not happen) — treat the rest as text.
                None => break,
            }
        } else {
            i += 1;
        }
    }
    if text_start < bytes.len() {
        toks.push(Token::Text(&html[text_start..]));
    }
    toks
}

/// Tag name from a full tag string: `<pre>`→`pre`, `</pre>`→`pre`, `<a href=…>`→`a`.
fn tag_name(tag: &str) -> &str {
    let inner = tag
        .trim_start_matches('<')
        .trim_start_matches('/')
        .trim_end_matches('>');
    let end = inner.find([' ', '\t']).unwrap_or(inner.len());
    &inner[..end]
}

fn close_tag_for(open_tag: &str) -> String {
    format!("</{}>", tag_name(open_tag))
}

/// Bytes needed to close every currently-open tag (`</name>` = name + 3).
fn closing_len(stack: &[&str]) -> usize {
    stack.iter().map(|t| tag_name(t).len() + 3).sum()
}

/// Bytes needed to reopen every currently-open tag on a fresh chunk.
fn reopen_len(stack: &[&str]) -> usize {
    stack.iter().map(|t| t.len()).sum()
}

/// Close all open tags (reverse order), push the chunk, then seed the next chunk
/// by reopening the same tags — so every emitted chunk is individually balanced.
fn flush_balanced(cur: &mut String, stack: &[&str], chunks: &mut Vec<String>) {
    for t in stack.iter().rev() {
        cur.push_str(&close_tag_for(t));
    }
    let finished = std::mem::take(cur);
    if !finished.trim().is_empty() {
        chunks.push(finished);
    }
    for t in stack.iter() {
        cur.push_str(t);
    }
}

/// Largest char-boundary cut ≤ `avail`, preferring to break just after the last
/// newline so we split on line boundaries when possible.
fn best_break(s: &str, avail: usize) -> usize {
    let mut hard = avail.min(s.len());
    while hard > 0 && !s.is_char_boundary(hard) {
        hard -= 1;
    }
    match s[..hard].rfind('\n') {
        Some(nl) => nl + 1,
        None => hard,
    }
}

/// Split rendered Telegram HTML into chunks ≤ `max` bytes. Never splits a
/// multi-byte character (Task 2.1) and never leaves a tag unclosed: open tags
/// are closed at a chunk's end and reopened at the next chunk's start (Task 2.2).
fn split_chunks(html: &str, max: usize) -> Vec<String> {
    if html.len() <= max {
        return vec![html.to_string()];
    }
    let mut chunks: Vec<String> = Vec::new();
    let mut stack: Vec<&str> = Vec::new();
    let mut cur = String::new();

    for tok in tokenize(html) {
        match tok {
            Token::Tag(tag) if tag.starts_with("</") => {
                cur.push_str(tag);
                let name = tag_name(tag);
                if let Some(pos) = stack.iter().rposition(|t| tag_name(t) == name) {
                    stack.remove(pos);
                }
            }
            Token::Tag(tag) => {
                // Reserve room for this tag plus its eventual closing tag.
                let projected =
                    cur.len() + tag.len() + closing_len(&stack) + tag_name(tag).len() + 3;
                if projected > max && cur.len() > reopen_len(&stack) {
                    flush_balanced(&mut cur, &stack, &mut chunks);
                }
                cur.push_str(tag);
                stack.push(tag);
            }
            Token::Text(t) => {
                let mut remaining = t;
                loop {
                    let avail = max
                        .saturating_sub(closing_len(&stack))
                        .saturating_sub(cur.len());
                    if remaining.len() <= avail {
                        cur.push_str(remaining);
                        break;
                    }
                    if avail == 0 {
                        // On a pathologically small budget the tags alone may fill
                        // a fresh chunk; hard-slice one char-safe piece to guarantee
                        // forward progress before flushing.
                        if cur.len() <= reopen_len(&stack) {
                            let cap = max.saturating_sub(cur.len()).max(1);
                            let mut cut = cap.min(remaining.len());
                            while cut > 0 && !remaining.is_char_boundary(cut) {
                                cut -= 1;
                            }
                            if cut == 0 {
                                cut = remaining.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
                            }
                            if cut == 0 {
                                break;
                            }
                            let (head, tail) = remaining.split_at(cut);
                            cur.push_str(head);
                            remaining = tail;
                        }
                        flush_balanced(&mut cur, &stack, &mut chunks);
                        continue;
                    }
                    let cut = best_break(remaining, avail);
                    if cut == 0 {
                        flush_balanced(&mut cur, &stack, &mut chunks);
                        continue;
                    }
                    let (head, tail) = remaining.split_at(cut);
                    cur.push_str(head);
                    remaining = tail;
                    flush_balanced(&mut cur, &stack, &mut chunks);
                }
            }
        }
    }

    for t in stack.iter().rev() {
        cur.push_str(&close_tag_for(t));
    }
    let last = cur.trim_end().to_string();
    if !last.trim().is_empty() {
        chunks.push(last);
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

    // ---- Task 2.1: never split a multi-byte character ----

    #[test]
    fn long_cjk_paragraph_splits_without_panicking() {
        // 3000 CJK chars (3 bytes each = 9000 bytes) in one paragraph, no newline.
        let md = "測".repeat(3000);
        let chunks = to_telegram_html(&md);
        assert!(chunks.len() > 1, "expected a split, got {}", chunks.len());
        for c in &chunks {
            assert!(c.len() <= 4096, "chunk over limit: {} bytes", c.len());
        }
        // No character is lost or corrupted across the split.
        assert_eq!(chunks.join("").matches('測').count(), 3000);
    }

    #[test]
    fn split_chunks_respects_char_boundaries_with_small_max() {
        let s = "日本語テキスト".repeat(50); // multi-byte, no newline
        let chunks = split_chunks(&s, 20);
        for c in &chunks {
            assert!(c.len() <= 20, "chunk over limit: {} bytes", c.len());
            // String is UTF-8 by construction; the real assertion is "no panic".
        }
        assert_eq!(chunks.join("").matches('語').count(), 50);
    }

    // ---- Task 2.2: chunks stay tag-balanced ----

    #[test]
    fn long_pre_block_stays_balanced_across_chunks() {
        let code = (0..600)
            .map(|i| format!("line {i} of some code"))
            .collect::<Vec<_>>()
            .join("\n");
        let md = format!("```\n{code}\n```");
        let chunks = to_telegram_html(&md);
        assert!(chunks.len() > 1, "expected a split, got {}", chunks.len());
        for c in &chunks {
            assert!(c.len() <= 4096, "chunk over limit: {} bytes", c.len());
            assert_eq!(
                c.matches("<pre>").count(),
                c.matches("</pre>").count(),
                "unbalanced <pre> in chunk: {c}"
            );
        }
    }

    #[test]
    fn long_blockquote_stays_balanced_across_chunks() {
        let body = "> ".to_string() + &"引用文字很長很長。".repeat(600);
        let chunks = to_telegram_html(&body);
        assert!(chunks.len() > 1, "expected a split, got {}", chunks.len());
        for c in &chunks {
            assert_eq!(
                c.matches("<blockquote>").count(),
                c.matches("</blockquote>").count(),
                "unbalanced <blockquote> in chunk"
            );
        }
    }

    // ---- Task 2.3: reject dangerous URL schemes ----

    #[test]
    fn is_safe_url_accepts_and_rejects() {
        for ok in [
            "https://x.io",
            "http://x.io",
            "mailto:a@b.com",
            "tel:+123",
            "/rel/path",
            "#frag",
            "?q=1",
            "path/only",
            "",
        ] {
            assert!(is_safe_url(ok), "should be safe: {ok:?}");
        }
        for bad in [
            "javascript:alert(1)",
            "JavaScript:alert(1)",
            "data:text/html,x",
            "vbscript:x",
            "  javascript:x",
            "java\tscript:alert(1)",
        ] {
            assert!(!is_safe_url(bad), "should be unsafe: {bad:?}");
        }
    }

    #[test]
    fn web_strips_javascript_link_keeps_text() {
        let out = to_web_html("[click me](javascript:alert(document.cookie))");
        assert!(!out.contains("javascript:"), "js url leaked: {out}");
        assert!(!out.contains("<a "), "anchor emitted for unsafe url: {out}");
        assert!(out.contains("click me"), "inner text dropped: {out}");
    }

    #[test]
    fn web_strips_data_url() {
        let out = to_web_html("[x](data:text/html,hi)");
        assert!(!out.to_ascii_lowercase().contains("data:text/html"));
        assert!(!out.contains("<a "));
    }

    #[test]
    fn web_keeps_safe_links() {
        assert!(to_web_html("[x](https://example.com)").contains(r#"href="https://example.com""#));
        assert!(to_web_html("[x](mailto:a@b.com)").contains("mailto:a@b.com"));
        assert!(to_web_html("[x](/local/path)").contains(r#"href="/local/path""#));
    }

    #[test]
    fn telegram_strips_javascript_link_keeps_text() {
        let out = to_telegram_html("[click me](javascript:alert(1))").join("");
        assert!(!out.contains("javascript:"), "js url leaked: {out}");
        assert!(
            !out.contains("<a href"),
            "anchor emitted for unsafe url: {out}"
        );
        assert!(out.contains("click me"), "inner text dropped: {out}");
    }

    // issue-0031 AC-6：不含清單、標題、<think>、巢狀引言的輸入，輸出與改動前逐字相同。
    // 期望值是 2026-11-13 改動前的程式實際輸出。
    #[test]
    fn telegram_output_unchanged_for_unaffected_structures() {
        let cases = [
            (
                "**粗** *斜* ~~刪~~ `code` 文字 & < >",
                "<b>粗</b> <i>斜</i> <s>刪</s> <code>code</code> 文字 &amp; &lt; &gt;",
            ),
            (
                "```\nfn main() { a < b }\n```\n",
                "<pre>fn main() { a &lt; b }\n</pre>",
            ),
            (
                "| a | b |\n|---|---|\n| 1 | 2 |\n",
                "<pre>a  b\n1  2\n</pre>",
            ),
            (
                "[docs](https://x.io) 與 [bad](javascript:alert(1))",
                "<a href=\"https://x.io\">docs</a> 與 bad",
            ),
            (
                "> 單層引言\n> 第二行\n\n後文",
                "<blockquote>單層引言\n第二行\n\n</blockquote>\n後文",
            ),
            (
                "段落一\n\n---\n\n段落二\n第二行",
                "段落一\n\n——————\n段落二\n第二行",
            ),
            ("文字 <u>底線</u> 結尾", "文字 &lt;u&gt;底線&lt;/u&gt; 結尾"),
        ];
        for (md, expected) in cases {
            assert_eq!(to_telegram_html(md).join(""), expected, "input: {md:?}");
        }
    }

    // issue-0031 AC-1：每項另起一行，子清單每層多縮 2 空白、符號依層級 •◦▪，編號保留原號碼。
    #[test]
    fn telegram_nested_lists_keep_lines_levels_and_numbers() {
        let md = "3. 第三步\n4. 第四步\n   - 子項 A\n     - 孫項\n   - 子項 B\n";
        assert_eq!(
            to_telegram_html(md).join(""),
            "3. 第三步\n4. 第四步\n  ◦ 子項 A\n    ▪ 孫項\n  ◦ 子項 B"
        );
        let md = "- a\n  1. x\n  2. y\n- b\n";
        assert_eq!(to_telegram_html(md).join(""), "• a\n  1. x\n  2. y\n• b");
        let md = "- 1\n  - 2\n    - 3\n      - 4\n";
        assert_eq!(
            to_telegram_html(md).join(""),
            "• 1\n  ◦ 2\n    ▪ 3\n      ▪ 4"
        );
    }

    // issue-0031 AC-2：H1 底線在粗體外層，H2～H6 只有粗體。
    #[test]
    fn telegram_h1_is_underlined_bold_and_lower_levels_are_bold() {
        assert_eq!(
            to_telegram_html("# 一級\n## 二級\n###### 六級").join(""),
            "<u><b>一級</b></u>\n<b>二級</b>\n<b>六級</b>"
        );
    }

    // issue-0031 AC-3：<think> 連同內容不輸出，前後文字照常輸出。
    #[test]
    fn telegram_drops_think_blocks_and_keeps_surrounding_text() {
        let cases = [
            ("前文\n\n<think>secret</think>\n\n後文", "前文\n\n後文"),
            ("<think>\n推理一\n\n推理二\n</think>\n\n答案", "答案"),
            ("答 <think>x</think>案", "答 案"),
        ];
        for (md, expected) in cases {
            let out = to_telegram_html(md).join("");
            assert!(!out.contains("think"), "think tag leaked for {md:?}: {out}");
            assert_eq!(out, expected, "input: {md:?}");
        }
    }

    // issue-0031 AC-4：巢狀引言攤平成一層，內文全部保留。
    #[test]
    fn telegram_flattens_nested_blockquotes() {
        let out = to_telegram_html("> 引言\n> > 巢狀\n\n後文").join("");
        assert_eq!(out.matches("<blockquote>").count(), 1, "{out}");
        assert_eq!(out.matches("</blockquote>").count(), 1, "{out}");
        assert!(
            out.contains("引言") && out.contains("巢狀") && out.contains("後文"),
            "{out}"
        );
    }

    // issue-0031 AC-3（PR #33 審查 P1）：表格儲存格裡的 think 不外露，前後文字保留。
    #[test]
    fn telegram_drops_think_inside_table_cells() {
        let md = "| 項目 | 內容 |\n|---|---|\n| A | <think>hidden reasoning</think>答案 |\n";
        assert_eq!(
            to_telegram_html(md).join(""),
            "<pre>項目  內容\nA   答案\n</pre>"
        );
    }

    // issue-0031 AC-3（PR #33 審查 P1）：think 跨引言或清單邊界時，輸出的標籤仍成對，
    // 後面的清單照常編號。
    #[test]
    fn telegram_think_across_blocks_keeps_html_balanced() {
        let out = to_telegram_html("> 引言 <think>隱藏內容\n\n</think>\n\n正常回答").join("");
        assert_eq!(out, "<blockquote>引言\n\n</blockquote>\n正常回答");
        let out = to_telegram_html("- 一 <think>隱藏\n\n- 二</think> 後文\n- 三\n\n1. 甲\n2. 乙\n")
            .join("");
        assert_eq!(out, "• 一  後文\n• 三\n1. 甲\n2. 乙");
    }

    // issue-0031 AC-3（PR #33 審查 P2）：巢狀與連續的 think 都拿掉；沒有結尾的 think
    // 拿掉到訊息結尾，寧可少顯示也不外露推理。
    #[test]
    fn telegram_drops_nested_consecutive_and_unclosed_think() {
        let cases = [
            (
                "<think>\n外層推理\n<think>內層推理</think>\n剩餘推理\n</think>\n\n最終答案",
                "最終答案",
            ),
            ("前文<think>a</think>中<think>b</think>後", "前文中後"),
            ("答案開頭\n\n<think>沒有結尾的推理\n\n更多\n", "答案開頭"),
        ];
        for (md, expected) in cases {
            assert_eq!(to_telegram_html(md).join(""), expected, "input: {md:?}");
        }
    }

    // issue-0031 AC-3（PR #33 審查 P2）：程式碼裡的 think 標籤是內容，照原本跳脫後保留。
    #[test]
    fn telegram_keeps_think_tags_inside_code() {
        let md = "inline `<think>x</think>` 保留\n\n```\n<think>y</think>\n```\n";
        assert_eq!(
            to_telegram_html(md).join(""),
            "inline <code>&lt;think&gt;x&lt;/think&gt;</code> 保留\n\n<pre>&lt;think&gt;y&lt;/think&gt;\n</pre>"
        );
    }

    // issue-0031 AC-7：清單項目裡的接續段落與換行，縮到該項文字的位置。
    #[test]
    fn telegram_indents_continuation_lines_inside_list_items() {
        let md = "- 第一項\n\n  第一項第二段\n\n  - 子項\n\n    子項第二段\n\n- 第二項\n";
        assert_eq!(
            to_telegram_html(md).join(""),
            "• 第一項\n\n  第一項第二段\n\n  ◦ 子項\n\n    子項第二段\n\n• 第二項"
        );
        let md = "10. 步驟\n    說明\n";
        assert_eq!(to_telegram_html(md).join(""), "10. 步驟\n    說明");
    }
}
