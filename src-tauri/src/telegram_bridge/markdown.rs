//! Minimal CommonMark → Telegram-HTML converter for notification bodies.
//!
//! Telegram's `parse_mode: HTML` supports a narrow whitelist: `<b>`, `<i>`,
//! `<u>`, `<s>`, `<code>`, `<pre>`, `<pre><code class="language-…">…</code></pre>`,
//! `<a href="…">`, `<blockquote>`. Anything else is rejected with Bad Request.
//!
//! Not a full parser — handles the subset Claude actually produces:
//! fenced code blocks, inline code, bold, italic, ATX headers, bullet / numbered
//! lists (rendered as plain text with their markers kept), and links.
//! Tables, HTML passthrough, setext headers and other rarely-used constructs
//! are rendered as escaped text.

/// Escape the three characters Telegram's HTML parser treats as meta.
pub fn escape_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
    out
}

/// Convert a markdown body to Telegram HTML.
pub fn to_telegram_html(md: &str) -> String {
    let mut out = String::with_capacity(md.len() + 32);
    let mut in_code_block = false;
    let mut code_lang = String::new();
    let mut code_buf = String::new();

    for raw_line in md.lines() {
        // Fence: ``` optionally followed by a language tag.
        if let Some(fence_rest) = raw_line.trim_start().strip_prefix("```") {
            if in_code_block {
                out.push_str(&render_code_block(&code_lang, &code_buf));
                code_buf.clear();
                code_lang.clear();
                in_code_block = false;
            } else {
                code_lang = fence_rest.trim().to_string();
                in_code_block = true;
            }
            continue;
        }

        if in_code_block {
            code_buf.push_str(raw_line);
            code_buf.push('\n');
            continue;
        }

        out.push_str(&render_line(raw_line));
        out.push('\n');
    }

    // Unclosed fence: emit what we have as a plain code block.
    if in_code_block && !code_buf.is_empty() {
        out.push_str(&render_code_block(&code_lang, &code_buf));
    }

    // Collapse three or more consecutive newlines that creep in around
    // block boundaries into two — Telegram still shows a paragraph break.
    collapse_blank_runs(&out)
}

fn render_code_block(lang: &str, body: &str) -> String {
    // Preserve the final newline inside <pre> — it's semantic whitespace
    // and tests / Telegram clients both expect it to round-trip.
    if lang.is_empty() {
        format!("<pre>{}</pre>\n", escape_html(body))
    } else {
        format!(
            "<pre><code class=\"language-{}\">{}</code></pre>\n",
            escape_html(lang),
            escape_html(body)
        )
    }
}

fn render_line(line: &str) -> String {
    let trimmed = line.trim_start();

    // ATX headers: 1-6 '#' then space. Telegram has no <h*> — map to bold.
    if let Some(after_hashes) = strip_atx_header(trimmed) {
        return format!("<b>{}</b>", render_inline(after_hashes));
    }

    // Horizontal rule: three or more -, *, or _ on a line. Drop — Telegram
    // has no HR; emit a blank line.
    if is_horizontal_rule(trimmed) {
        return String::new();
    }

    // Bullet list item: keep the marker as a visible bullet.
    if let Some(rest) = strip_bullet(trimmed) {
        let indent = &line[..line.len() - trimmed.len()];
        return format!("{indent}• {}", render_inline(rest));
    }

    // Numbered list item: keep the digit + dot.
    if let Some((num, rest)) = strip_numbered(trimmed) {
        let indent = &line[..line.len() - trimmed.len()];
        return format!("{indent}{num}. {}", render_inline(rest));
    }

    // Blockquote '>': wrap in <blockquote>.
    if let Some(rest) = trimmed.strip_prefix("> ") {
        return format!("<blockquote>{}</blockquote>", render_inline(rest));
    }

    render_inline(line)
}

/// Inline transformations: escape HTML first, then substitute
/// backtick-delimited code and paired *, _, ** tokens. Must run on the
/// already-escaped string so generated tags don't get re-escaped.
fn render_inline(s: &str) -> String {
    let escaped = escape_html(s);
    let after_code = replace_inline_code(&escaped);
    // Bold before italic so ** isn't eaten by single-* rule.
    let after_bold = replace_paired(&after_code, "**", "<b>", "</b>");
    let after_bold = replace_paired(&after_bold, "__", "<b>", "</b>");
    let after_italic = replace_single(&after_bold, '*', "<i>", "</i>");
    replace_single(&after_italic, '_', "<i>", "</i>")
}

/// Replace every pair of single-backtick runs with `<code>...</code>`.
/// Double-backtick + inner backtick is not handled (CommonMark corner case).
fn replace_inline_code(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'`' {
            if let Some(end) = find_byte(bytes, b'`', i + 1) {
                out.push_str("<code>");
                // We're inside an escaped string → safe to push raw bytes.
                out.push_str(&s[i + 1..end]);
                out.push_str("</code>");
                i = end + 1;
                continue;
            }
        }
        // UTF-8 safe char copy.
        let c = s[i..].chars().next().unwrap();
        out.push(c);
        i += c.len_utf8();
    }
    out
}

fn find_byte(bytes: &[u8], needle: u8, start: usize) -> Option<usize> {
    bytes[start..].iter().position(|b| *b == needle).map(|p| p + start)
}

/// Replace every pair of `marker` (e.g. `**`) with `open`/`close` tags.
/// Non-paired occurrences are left untouched.
fn replace_paired(s: &str, marker: &str, open: &str, close: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut remaining = s;
    loop {
        let Some(start) = remaining.find(marker) else {
            out.push_str(remaining);
            break;
        };
        let after_start = &remaining[start + marker.len()..];
        let Some(end_rel) = after_start.find(marker) else {
            out.push_str(&remaining[..start + marker.len()]);
            remaining = after_start;
            continue;
        };
        out.push_str(&remaining[..start]);
        out.push_str(open);
        out.push_str(&after_start[..end_rel]);
        out.push_str(close);
        remaining = &after_start[end_rel + marker.len()..];
    }
    out
}

/// Pair up single-char markers (`*` or `_`) into `<open>`/`<close>` tags.
/// Two-pass: first find positions of every lone marker (skipping doubled
/// ones that `replace_paired` already consumed), then only wrap markers
/// that have a partner. A lone orphan stays as a literal character.
fn replace_single(s: &str, marker: char, open: &str, close: &str) -> String {
    let chars: Vec<char> = s.chars().collect();

    // Collect indices of single (non-doubled) markers.
    let mut positions = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == marker {
            if i + 1 < chars.len() && chars[i + 1] == marker {
                i += 2;
                continue;
            }
            positions.push(i);
        }
        i += 1;
    }

    // Pair them up; an odd final marker is orphaned.
    let paired: std::collections::HashSet<usize> = positions
        .chunks_exact(2)
        .flat_map(|pair| pair.iter().copied())
        .collect();

    let mut out = String::with_capacity(s.len() + positions.len() * 4);
    let mut i = 0;
    let mut in_tag = false;
    while i < chars.len() {
        if paired.contains(&i) {
            out.push_str(if in_tag { close } else { open });
            in_tag = !in_tag;
            i += 1;
        } else if chars[i] == marker
            && i + 1 < chars.len()
            && chars[i + 1] == marker
        {
            // Doubled marker: emit both literally.
            out.push(marker);
            out.push(marker);
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn strip_atx_header(line: &str) -> Option<&str> {
    let mut n = 0;
    for c in line.chars() {
        if c == '#' {
            n += 1;
            if n > 6 {
                return None;
            }
        } else if c == ' ' && n >= 1 {
            return Some(line[n + 1..].trim_end_matches('#').trim_end());
        } else {
            return None;
        }
    }
    None
}

fn strip_bullet(line: &str) -> Option<&str> {
    for marker in &["- ", "* ", "+ "] {
        if let Some(rest) = line.strip_prefix(*marker) {
            return Some(rest);
        }
    }
    None
}

fn strip_numbered(line: &str) -> Option<(&str, &str)> {
    let digits: String = line.chars().take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        return None;
    }
    let rest = &line[digits.len()..];
    let rest = rest.strip_prefix('.').or_else(|| rest.strip_prefix(')'))?;
    let rest = rest.strip_prefix(' ')?;
    Some((&line[..digits.len()], rest))
}

fn is_horizontal_rule(line: &str) -> bool {
    if line.len() < 3 {
        return false;
    }
    let ch = line.chars().next().unwrap();
    if !matches!(ch, '-' | '*' | '_') {
        return false;
    }
    line.chars().all(|c| c == ch || c.is_whitespace())
        && line.chars().filter(|c| *c == ch).count() >= 3
}

fn collapse_blank_runs(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut consecutive_newlines = 0;
    for c in s.chars() {
        if c == '\n' {
            consecutive_newlines += 1;
            if consecutive_newlines <= 2 {
                out.push(c);
            }
        } else {
            consecutive_newlines = 0;
            out.push(c);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escapes_angle_brackets_and_amp() {
        assert_eq!(escape_html("a < b & c > d"), "a &lt; b &amp; c &gt; d");
    }

    #[test]
    fn plain_paragraph_round_trips() {
        let md = "Hello world.";
        let html = to_telegram_html(md);
        assert_eq!(html.trim(), "Hello world.");
    }

    #[test]
    fn bold_and_italic_convert() {
        let html = to_telegram_html("**strong** and *soft*");
        assert!(html.contains("<b>strong</b>"));
        assert!(html.contains("<i>soft</i>"));
    }

    #[test]
    fn inline_code_wraps_in_code_tag() {
        let html = to_telegram_html("use `HashMap::new()` then");
        assert!(html.contains("<code>HashMap::new()</code>"));
    }

    #[test]
    fn fenced_code_block_with_language() {
        let md = "before\n```rust\nlet x = 1;\n```\nafter";
        let html = to_telegram_html(md);
        assert!(html.contains("<pre><code class=\"language-rust\">let x = 1;\n</code></pre>"));
        assert!(html.contains("before"));
        assert!(html.contains("after"));
    }

    #[test]
    fn fenced_code_block_without_language() {
        let md = "```\nplain\n```";
        let html = to_telegram_html(md);
        assert!(html.contains("<pre>plain\n</pre>"));
        assert!(!html.contains("<code "));
    }

    #[test]
    fn code_inside_block_escapes_html() {
        let md = "```\n<script>alert(1)</script>\n```";
        let html = to_telegram_html(md);
        assert!(html.contains("&lt;script&gt;alert(1)&lt;/script&gt;"));
        // And the outer <pre> / </pre> stay as tags, not escaped.
        assert!(html.contains("<pre>"));
        assert!(html.contains("</pre>"));
    }

    #[test]
    fn header_becomes_bold() {
        let html = to_telegram_html("## A Title");
        assert!(html.contains("<b>A Title</b>"));
    }

    #[test]
    fn bullet_list_preserves_bullets() {
        let md = "- first\n- second";
        let html = to_telegram_html(md);
        assert!(html.contains("• first"));
        assert!(html.contains("• second"));
    }

    #[test]
    fn numbered_list_preserves_numbers() {
        let md = "1. one\n2. two";
        let html = to_telegram_html(md);
        assert!(html.contains("1. one"));
        assert!(html.contains("2. two"));
    }

    #[test]
    fn blockquote_wraps_in_tag() {
        let html = to_telegram_html("> quoted");
        assert!(html.contains("<blockquote>quoted</blockquote>"));
    }

    #[test]
    fn angle_brackets_in_prose_are_escaped() {
        let html = to_telegram_html("compare a<b and c>d");
        assert!(html.contains("a&lt;b"));
        assert!(html.contains("c&gt;d"));
    }

    #[test]
    fn lone_asterisk_stays_literal() {
        // A single unpaired * must not produce <i> — that would break parse_mode.
        let html = to_telegram_html("star *alone without close");
        assert!(!html.contains("<i>"));
        assert!(html.contains("*alone"));
    }

    #[test]
    fn unclosed_fence_still_renders_as_pre() {
        let md = "```\nhanging code";
        let html = to_telegram_html(md);
        assert!(html.contains("<pre>hanging code"));
        assert!(html.contains("</pre>"));
    }

    #[test]
    fn headers_strip_trailing_hashes() {
        let html = to_telegram_html("### Closed ###");
        assert!(html.contains("<b>Closed</b>"));
    }

    #[test]
    fn horizontal_rule_collapses_to_blank() {
        let md = "before\n---\nafter";
        let html = to_telegram_html(md);
        assert!(html.contains("before"));
        assert!(html.contains("after"));
        assert!(!html.contains("---"));
    }

    #[test]
    fn multiple_inline_code_spans_on_one_line() {
        let html = to_telegram_html("use `foo` or `bar`");
        assert!(html.contains("<code>foo</code>"));
        assert!(html.contains("<code>bar</code>"));
    }

    #[test]
    fn cyrillic_passes_through() {
        let html = to_telegram_html("Привет, **мир**!");
        assert!(html.contains("Привет"));
        assert!(html.contains("<b>мир</b>"));
    }
}
