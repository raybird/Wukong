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
/// unclosed block runs to the end of the message. Think tags written inside
/// another tag's quoted attribute value, a comment, a processing instruction,
/// CDATA or a declaration are content, not tags (issue-0034). An HTML block is
/// scanned as a whole, not per line event, so those constructs may span lines.
fn strip_think(markdown: &str, opts: Options) -> String {
    const OPEN: &str = "<think>";
    const CLOSE: &str = "</think>";
    let mut ranges = Vec::new();
    let (mut depth, mut start) = (0usize, 0usize);
    for (ev, range) in Parser::new_ext(markdown, opts).into_offset_iter() {
        if !matches!(ev, Event::Start(Tag::HtmlBlock) | Event::InlineHtml(_)) {
            continue;
        }
        let fragment = &markdown[range.clone()];
        let mut i = 0;
        while let Some(found) = fragment[i..].find('<') {
            i += found;
            let rest = &fragment[i..];
            if rest.starts_with(OPEN) {
                if depth == 0 {
                    start = range.start + i;
                }
                depth += 1;
                i += OPEN.len();
            } else if depth > 0 && rest.starts_with(CLOSE) {
                depth -= 1;
                if depth == 0 {
                    ranges.push(start..range.start + i + CLOSE.len());
                }
                i += CLOSE.len();
            } else {
                i += markup_len(rest);
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

/// Length of the comment, processing instruction, CDATA, declaration or tag
/// that `s` (starting with `<`) opens, so a `<` inside it is not read as a tag.
/// Only constructs that end inside `s` count; anything else is 1, so an
/// unterminated quote or comment can never hide a later think tag.
fn markup_len(s: &str) -> usize {
    let until = |from: usize, end: &str| s[from..].find(end).map_or(1, |p| from + p + end.len());
    if s.starts_with("<!--") {
        return until(4, "-->");
    }
    if s.starts_with("<?") {
        return until(2, "?>");
    }
    if s.starts_with("<![CDATA[") {
        return until(9, "]]>");
    }
    if s.starts_with("<!") {
        return until(2, ">");
    }
    let name = s.strip_prefix("</").unwrap_or(&s[1..]);
    if !name.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return 1;
    }
    // Quotes open an attribute value only right after `=` (spaces allowed).
    let (mut quote, mut prev) = (None, '<');
    for (i, c) in s.char_indices().skip(1) {
        match quote {
            Some(q) if c == q => quote = None,
            Some(_) => {}
            None if (c == '"' || c == '\'') && prev == '=' => quote = Some(c),
            None if c == '>' => return i + 1,
            None if c == '<' => return 1,
            None => {}
        }
        if !c.is_whitespace() {
            prev = c;
        }
    }
    1
}

/// Longest escaped `href` kept as a link. `split_chunks` reopens open tags on
/// every chunk, so a longer one would leave almost no room for text in a
/// 4096-byte message; such links are written out as `text (url)` instead.
/// This is our own budget (about half a message), not a Bot API URL limit:
/// a 2048-byte href was accepted as a link when sent on 2026-11-18 (#36).
const MAX_HREF_BYTES: usize = 2048;

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
    // Set while inside a link whose href is too long to reopen on every chunk:
    // the text is emitted, then the URL in parentheses.
    let mut link_plain: Option<String> = None;
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
                    // Telegram silently drops a link whose URL contains `"`, `<`
                    // or `>` (even as entities); percent-encoded, it keeps it.
                    let href = escape_html(
                        &dest_url
                            .replace('"', "%22")
                            .replace('<', "%3C")
                            .replace('>', "%3E"),
                    );
                    if href.len() > MAX_HREF_BYTES {
                        link_plain = Some(escape_html(&dest_url));
                    } else {
                        out.push_str(&format!(r#"<a href="{href}">"#));
                    }
                } else {
                    link_stripped = true;
                }
            }
            Event::End(TagEnd::Link) => {
                if link_stripped {
                    link_stripped = false;
                } else if let Some(url) = link_plain.take() {
                    out.push_str(&format!(" ({url})"));
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

/// If `cut` falls inside an entity (`&amp;`), the position of its `&`. Every
/// `&` in rendered text starts an entity, because `escape_html` escapes it.
fn entity_start(s: &str, cut: usize) -> Option<usize> {
    let amp = s[..cut].rfind('&')?;
    (!s[amp..cut].contains(';')).then_some(amp)
}

/// Largest char-boundary cut ≤ `avail` that does not split an entity,
/// preferring to break just after the last newline so we split on line
/// boundaries when possible.
fn best_break(s: &str, avail: usize) -> usize {
    let mut hard = avail.min(s.len());
    while hard > 0 && !s.is_char_boundary(hard) {
        hard -= 1;
    }
    if let Some(amp) = entity_start(s, hard) {
        hard = amp;
    }
    match s[..hard].rfind('\n') {
        Some(nl) => nl + 1,
        None => hard,
    }
}

/// Split rendered Telegram HTML into chunks ≤ `max` bytes. Never splits a
/// multi-byte character (Task 2.1) or an entity (#36) and never leaves a tag
/// unclosed: open tags are closed at a chunk's end and reopened at the next
/// chunk's start (Task 2.2).
///
/// The `max` guarantee needs room for the reopened tags, their closing tags
/// and one character or entity. With less, a chunk takes one character or
/// entity anyway and goes over `max`, so splitting still ends. For Telegram
/// (`max` 4096) that cannot happen in practice: the only long tag is a link,
/// capped by `MAX_HREF_BYTES`.
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
                    let cut = best_break(remaining, avail);
                    if cut == 0 {
                        // On a pathologically small budget the tags alone may fill
                        // a fresh chunk, or leave no room for one char or entity;
                        // hard-slice one char-safe piece to guarantee forward
                        // progress before flushing.
                        if cur.len() <= reopen_len(&stack) {
                            let cap = max.saturating_sub(cur.len()).max(1);
                            let mut cut = cap.min(remaining.len());
                            while cut > 0 && !remaining.is_char_boundary(cut) {
                                cut -= 1;
                            }
                            if cut == 0 {
                                cut = remaining.chars().next().map(|c| c.len_utf8()).unwrap_or(0);
                            }
                            if let Some(amp) = entity_start(remaining, cut) {
                                cut = match amp {
                                    0 => remaining.find(';').map_or(remaining.len(), |i| i + 1),
                                    _ => amp,
                                };
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

    // issue-0034 AC-4：HTML 區塊裡獨立的 think 標籤照樣連同內容拿掉。
    #[test]
    fn telegram_drops_standalone_think_inside_html_block() {
        let md = "<div>\n前文 <think>x</think> 後\n</div>\n\n答案";
        assert_eq!(
            to_telegram_html(md).join(""),
            "&lt;div&gt;\n前文  後\n&lt;/div&gt;\n答案"
        );
        // 沒有結尾的引號或註解不會把後面真正的 think 藏起來而讓推理外露。
        for md in [
            "<div>\n<b title=\"x> <think>secret</think> 後\n</div>\n\n答案",
            "<div>\n<!-- x <think>secret</think> 後\n</div>\n\n答案",
            // PR #37 審查：跨行時同樣不外露。
            "<div>\n<!--\n<think>secret</think>\n</div>\n\n答案",
            "<!--\n<think>secret</think>\n\n答案",
            "> <div title=\"\n> <think>secret</think>\n\n答案",
        ] {
            let out = to_telegram_html(md).join("");
            assert!(
                !out.contains("secret") && out.ends_with("答案"),
                "{md:?}: {out}"
            );
        }
    }

    // issue-0034 AC-1、AC-2：屬性值、註解、處理指令、CDATA、宣告裡的 think 字串不算標籤，
    // 後面的回答完整輸出，這些寫法照原本跳脫後顯示。期望值取自把 think 換成其他字的輸出。
    #[test]
    fn telegram_ignores_think_inside_attributes_and_comments() {
        let cases = [
            (
                "前文 <span title=\"<think>\">文字</span>\n\n後續正常回答",
                "前文 &lt;span title=\"&lt;think&gt;\"&gt;文字&lt;/span&gt;\n\n後續正常回答",
            ),
            (
                "<div title=\"<think>\">\n\n後續正常回答",
                "&lt;div title=\"&lt;think&gt;\"&gt;\n後續正常回答",
            ),
            (
                "前文 <span title='<think>'>文字</span> 後文",
                "前文 &lt;span title='&lt;think&gt;'&gt;文字&lt;/span&gt; 後文",
            ),
            (
                "前文 <!-- <think> -->\n\n後續正常回答",
                "前文 &lt;!-- &lt;think&gt; --&gt;\n\n後續正常回答",
            ),
            (
                "<!-- <think> -->\n\n後續正常回答",
                "&lt;!-- &lt;think&gt; --&gt;\n後續正常回答",
            ),
            (
                "前文 <?pi <think> ?> 後\n\n答案",
                "前文 &lt;?pi &lt;think&gt; ?&gt; 後\n\n答案",
            ),
            (
                "前文 <![CDATA[<think>]]> 後\n\n答案",
                "前文 &lt;![CDATA[&lt;think&gt;]]&gt; 後\n\n答案",
            ),
            (
                "前文 <!X <think> 後\n\n答案",
                "前文 &lt;!X &lt;think&gt; 後\n\n答案",
            ),
        ];
        for (md, expected) in cases {
            assert_eq!(to_telegram_html(md).join(""), expected, "input: {md:?}");
        }
    }

    // issue-0034 AC-3：think 區塊內，屬性值或註解裡的結尾標籤字串不會讓 think 提早結束。
    #[test]
    fn telegram_think_is_not_closed_by_end_tag_inside_attribute_or_comment() {
        let cases = [
            (
                "前文<think>推理 <span title=\"</think>\">內</span> 更多</think>後文",
                "前文後文",
            ),
            (
                "前文<think>推理 <!-- </think> --> 更多</think>後文",
                "前文後文",
            ),
            (
                "<think>\n推理 <![CDATA[</think>]]>\n更多\n</think>\n\n答案",
                "答案",
            ),
        ];
        for (md, expected) in cases {
            assert_eq!(to_telegram_html(md).join(""), expected, "input: {md:?}");
        }
    }

    // issue-0034 AC-1～AC-3（PR #37 審查 P1、P2）：HTML 區塊逐行切成多個事件，跨行的屬性值、
    // 註解、處理指令、CDATA、宣告仍視為一個整體；行內跨行的寫法本來就是單一事件。
    #[test]
    fn telegram_ignores_think_inside_multiline_html_constructs() {
        let cases = [
            (
                "<think>\n內部推理\n<!--\n</think>\n-->\n不應顯示的推理內容\n</think>\n\n正常回答",
                "正常回答",
            ),
            (
                "<think>\n推理\n<div title=\"\n</think>\n\">\n不應顯示\n</think>\n\n正常回答",
                "正常回答",
            ),
            (
                "<div title=\"\n<think>\n\">\n正常內容\n</div>\n\n後續正常回答",
                "&lt;div title=\"\n&lt;think&gt;\n\"&gt;\n正常內容\n&lt;/div&gt;\n後續正常回答",
            ),
            (
                "<div title='\n<think>\n'>\n正常內容\n</div>\n\n後續正常回答",
                "&lt;div title='\n&lt;think&gt;\n'&gt;\n正常內容\n&lt;/div&gt;\n後續正常回答",
            ),
            (
                "<!--\n<think>\n-->\n\n後續正常回答",
                "&lt;!--\n&lt;think&gt;\n--&gt;\n後續正常回答",
            ),
            (
                "<?pi\n<think>\n?>\n\n答案",
                "&lt;?pi\n&lt;think&gt;\n?&gt;\n答案",
            ),
            (
                "<![CDATA[\n<think>\n]]>\n\n答案",
                "&lt;![CDATA[\n&lt;think&gt;\n]]&gt;\n答案",
            ),
            ("<!X\n<think>\n\n答案", "&lt;!X\n&lt;think&gt;\n答案"),
            (
                "> <!--\n> <think>\n> -->\n\n後續正常回答",
                "<blockquote>&lt;!--\n&lt;think&gt;\n--&gt;\n</blockquote>\n後續正常回答",
            ),
            (
                "前文 <span title=\"\n<think>\n\">文字</span>\n\n後續正常回答",
                "前文 &lt;span title=\"\n&lt;think&gt;\n\"&gt;文字&lt;/span&gt;\n\n後續正常回答",
            ),
            (
                "前文<think>推理 <!--\n</think>\n--> 更多</think>後文",
                "前文後文",
            ),
        ];
        for (md, expected) in cases {
            assert_eq!(to_telegram_html(md).join(""), expected, "input: {md:?}");
        }
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

    // issue-0036 AC-1：href 裡的 " < > 改成百分比編碼（Telegram 會無聲丟掉解碼後含這些
    // 字元的連結，2026-11-18 實送確認），一般文字照舊。
    #[test]
    fn telegram_percent_encodes_quote_and_angle_brackets_inside_href_only() {
        assert_eq!(
            to_telegram_html("[docs](https://x.io/a\"b)"),
            vec![r#"<a href="https://x.io/a%22b">docs</a>"#]
        );
        assert_eq!(
            to_telegram_html("[docs](https://x.io/a<b>c?d=1&e=2)"),
            vec![r#"<a href="https://x.io/a%3Cb%3Ec?d=1&amp;e=2">docs</a>"#]
        );
        assert_eq!(
            to_telegram_html("他說 \"好\" & <ok>"),
            vec!["他說 \"好\" &amp; &lt;ok&gt;"]
        );
    }

    // issue-0036 AC-2：切點不落在實體中間。
    #[test]
    fn telegram_split_never_cuts_an_entity_in_half() {
        for md in [
            "a&".repeat(2100),
            "&".repeat(1500),
            "a<".repeat(2100),
            "x>".repeat(2100),
        ] {
            let chunks = to_telegram_html(&md);
            assert!(chunks.len() > 1, "expected a split for {:?}", &md[..4]);
            for c in &chunks {
                assert!(c.len() <= 4096, "chunk over limit: {} bytes", c.len());
                let rest = c
                    .replace("&amp;", "")
                    .replace("&lt;", "")
                    .replace("&gt;", "");
                assert!(
                    !rest.contains('&') && !rest.contains(';'),
                    "partial entity in chunk: head={:?} tail={:?}",
                    &c[..12],
                    &c[c.len() - 12..]
                );
            }
            assert_eq!(chunks.join(""), render_html(&md).trim());
        }

        // 預算比一個實體還小時仍要前進（不能無限迴圈），寧可單段超過預算也不切半個實體。
        let s = "&amp;&lt;x".repeat(30);
        let chunks = split_chunks(&s, 4);
        for c in &chunks {
            let rest = c.replace("&amp;", "").replace("&lt;", "");
            assert!(
                !rest.contains('&') && !rest.contains(';'),
                "partial entity: {c:?}"
            );
        }
        assert_eq!(chunks.join(""), s);
    }

    // issue-0036 AC-3：網址超過 2048 bytes 的連結改寫成「文字 (網址)」，每段不超過 4096 bytes。
    #[test]
    fn telegram_overlong_link_becomes_plain_text_within_limit() {
        let url = format!("https://x.io/{}", "a".repeat(5000));
        let chunks = to_telegram_html(&format!("[link text here]({url})"));
        assert!(chunks.len() <= 2, "got {} chunks", chunks.len());
        assert!(chunks.iter().all(|c| c.len() <= 4096 && !c.contains("<a ")));
        assert_eq!(chunks.join(""), format!("link text here ({url})"));

        let url = format!("https://x.io/{}", "a".repeat(4500));
        let text = "字".repeat(300);
        let chunks = to_telegram_html(&format!("[{text}]({url})"));
        assert!(chunks.len() <= 2, "got {} chunks", chunks.len());
        assert!(chunks.iter().all(|c| c.len() <= 4096 && !c.contains("<a ")));
        assert_eq!(chunks.join(""), format!("{text} ({url})"));

        // 對照組：短網址時是 "前文\n\n<a href=\"https://x.io/s\">x</a>\n\n後文"。
        let url = format!("https://x.io/{}", "a".repeat(4090));
        // 切段優先在換行處切：前文一段，4103 bytes 的網址本身要兩段。
        let chunks = to_telegram_html(&format!("前文\n\n[x]({url})\n\n後文"));
        assert!(chunks.len() <= 3, "got {} chunks", chunks.len());
        assert!(chunks.iter().all(|c| c.len() <= 4096 && !c.contains("<a ")));
        assert_eq!(chunks.join(""), format!("前文\n\nx ({url})\n\n後文"));
    }

    // issue-0036 AC-3：門檻兩側，以及長連結文字配 2048 bytes 網址仍不超過 4096。
    #[test]
    fn telegram_link_url_threshold_is_2048_bytes() {
        let url = format!("https://x.io/{}", "a".repeat(2048 - 13));
        assert_eq!(url.len(), 2048);
        assert_eq!(
            to_telegram_html(&format!("[x]({url})")),
            vec![format!(r#"<a href="{url}">x</a>"#)]
        );
        let url = format!("https://x.io/{}", "a".repeat(2049 - 13));
        assert_eq!(
            to_telegram_html(&format!("[x]({url})")),
            vec![format!("x ({url})")]
        );
        // 跳脫後才算：2044 個字元的網址含兩個 &（各變成 5 bytes）就超過門檻。
        let url = format!("https://x.io/?a=1&b=2&{}", "a".repeat(2044 - 22));
        assert_eq!(url.len(), 2044);
        let out = to_telegram_html(&format!("[x]({url})")).join("");
        assert!(
            !out.contains("<a "),
            "escaped href is 2052 bytes: {out:.40}"
        );

        let url = format!("https://x.io/{}", "a".repeat(2048 - 13));
        let text = "字".repeat(3000);
        let chunks = to_telegram_html(&format!("[{text}]({url})"));
        assert!(chunks.len() > 1);
        for c in &chunks {
            assert!(c.len() <= 4096, "chunk over limit: {} bytes", c.len());
            assert!(c.starts_with(&format!(r#"<a href="{url}">"#)) && c.ends_with("</a>"));
        }
        assert_eq!(chunks.join("").matches('字').count(), 3000);
    }

    // issue-0036 PR #40 第一輪審查：引言、粗體、接近門檻的長連結、斜體、刪除線與實體
    // 一起跨段時，每段不超過 4096、標籤平衡、實體完整，拿掉標籤後內容與不切段相同。
    #[test]
    fn telegram_nested_long_link_with_entities_splits_cleanly() {
        let url = format!("https://x.io/{}?a=1&b=2", "a".repeat(2000));
        // 以 & 為主，讓切點幾乎一定落在實體中間。
        let body = "粗 *斜&&&&&&* ~~刪&&&&&&~~ &&&&<c>&&&& \"q\" ".repeat(150);
        let md = format!("> **[{body}]({url})** 結尾 &amp; 文字");
        let href = url.replace('&', "&amp;");
        assert!(href.len() <= 2048);

        let strip = |s: &str| {
            let (mut out, mut in_tag) = (String::new(), false);
            for ch in s.chars() {
                match ch {
                    '<' => in_tag = true,
                    '>' if in_tag => in_tag = false,
                    c if !in_tag => out.push(c),
                    _ => {}
                }
            }
            out
        };

        let chunks = to_telegram_html(&md);
        assert!(chunks.len() > 2, "got {} chunks", chunks.len());
        for c in &chunks {
            assert!(c.len() <= 4096, "chunk over limit: {} bytes", c.len());
            for (open, close) in [
                ("<blockquote>", "</blockquote>"),
                ("<b>", "</b>"),
                ("<i>", "</i>"),
                ("<s>", "</s>"),
                ("<a href=", "</a>"),
            ] {
                assert_eq!(c.matches(open).count(), c.matches(close).count(), "{open}");
            }
            let rest = c
                .replace("&amp;", "")
                .replace("&lt;", "")
                .replace("&gt;", "");
            assert!(!rest.contains('&') && !rest.contains(';'), "partial entity");
        }
        assert!(chunks
            .iter()
            .any(|c| c.contains(&format!(r#"<a href="{href}">"#))));
        assert!(chunks
            .iter()
            .all(|c| c.contains("<i>") && c.contains("<s>")));
        assert_eq!(
            strip(&chunks.join("")),
            strip(render_html(&md).trim()),
            "text lost or duplicated across chunks"
        );
    }
}
