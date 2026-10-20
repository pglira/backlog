//! A small Markdown renderer for the text of the fields.
//!
//! Block syntax: headings, bullet and numbered lists, task lists, block
//! quotes, fenced code blocks and horizontal rules. Inline syntax: `code`,
//! **bold**, *italic*, ~~strikethrough~~, [links](url) and backslash escapes.
//! Unknown or unclosed syntax shows as plain text.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::{CODE, COMMENT, GREEN, LINK, MARKER, PURPLE};

/// Render `text` as Markdown lines on top of the style `base`.
pub fn render(text: &str, base: Style) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let mut in_code = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            lines.push(Line::from(Span::styled(
                line.to_string(),
                base.patch(Style::new().fg(CODE)),
            )));
            continue;
        }
        lines.push(block(line, base));
    }
    lines
}

/// Render one line outside of a code block.
fn block(line: &str, base: Style) -> Line<'static> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = line[..indent_len].to_string();
    let rest = &line[indent_len..];

    if let Some((level, title)) = heading(rest) {
        let mut style = base.patch(Style::new().fg(PURPLE).add_modifier(Modifier::BOLD));
        if level == 1 {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        return Line::from(inline(title, style));
    }
    if is_rule(rest) {
        return Line::from(Span::styled(
            "─".repeat(24),
            base.patch(Style::new().fg(COMMENT)),
        ));
    }
    if let Some(quote) = rest.strip_prefix('>') {
        let mut spans = vec![Span::styled(
            format!("{indent}│ "),
            base.patch(Style::new().fg(COMMENT)),
        )];
        let style = base.patch(Style::new().add_modifier(Modifier::ITALIC));
        spans.extend(inline(quote.trim_start(), style));
        return Line::from(spans);
    }
    if let Some(item) = ["- ", "* ", "+ "].iter().find_map(|m| rest.strip_prefix(m)) {
        let mut spans = vec![Span::raw(indent)];
        let marker = base.patch(Style::new().fg(MARKER));
        if let Some(task) = item.strip_prefix("[ ] ") {
            spans.push(Span::styled("☐ ", marker));
            spans.extend(inline(task, base));
        } else if let Some(task) = item
            .strip_prefix("[x] ")
            .or_else(|| item.strip_prefix("[X] "))
        {
            spans.push(Span::styled("☑ ", base.patch(Style::new().fg(GREEN))));
            spans.extend(inline(task, base.patch(Style::new().fg(COMMENT))));
        } else {
            spans.push(Span::styled("• ", marker));
            spans.extend(inline(item, base));
        }
        return Line::from(spans);
    }
    if let Some((number, item)) = ordered_item(rest) {
        let mut spans = vec![
            Span::raw(indent),
            Span::styled(format!("{number}. "), base.patch(Style::new().fg(MARKER))),
        ];
        spans.extend(inline(item, base));
        return Line::from(spans);
    }
    let mut spans = vec![Span::raw(indent)];
    spans.extend(inline(rest, base));
    Line::from(spans)
}

/// Parse an ATX heading (`# Title` to `###### Title`).
fn heading(line: &str) -> Option<(usize, &str)> {
    let level = line.chars().take_while(|&c| c == '#').count();
    if !(1..=6).contains(&level) {
        return None;
    }
    let title = line[level..].strip_prefix(' ')?;
    Some((level, title.trim()))
}

/// Tell if a line is a horizontal rule: three or more `-`, `*` or `_`.
fn is_rule(line: &str) -> bool {
    let chars: Vec<char> = line.chars().filter(|c| !c.is_whitespace()).collect();
    chars.len() >= 3
        && ['-', '*', '_']
            .iter()
            .any(|&m| chars.iter().all(|&c| c == m))
}

/// Parse a numbered list item (`1. text` or `1) text`).
fn ordered_item(line: &str) -> Option<(&str, &str)> {
    let digits = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits == 0 || digits > 9 {
        return None;
    }
    let rest = &line[digits..];
    let item = rest
        .strip_prefix(". ")
        .or_else(|| rest.strip_prefix(") "))?;
    Some((&line[..digits], item))
}

/// Render the inline syntax of `text` on top of the style `base`.
pub fn inline(text: &str, base: Style) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    parse_inline(&chars, base, &mut spans);
    spans
}

fn parse_inline(s: &[char], style: Style, out: &mut Vec<Span<'static>>) {
    let mut plain = String::new();
    let flush = |plain: &mut String, out: &mut Vec<Span<'static>>| {
        if !plain.is_empty() {
            out.push(Span::styled(std::mem::take(plain), style));
        }
    };
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c == '\\' && i + 1 < s.len() && s[i + 1].is_ascii_punctuation() {
            plain.push(s[i + 1]);
            i += 2;
            continue;
        }
        if c == '`'
            && let Some(end) = find(s, i + 1, &['`'])
        {
            flush(&mut plain, out);
            let code: String = s[i + 1..end].iter().collect();
            out.push(Span::styled(code, style.patch(Style::new().fg(CODE))));
            i = end + 1;
            continue;
        }
        if c == '['
            && let Some((text_end, url_end)) = link(s, i)
        {
            flush(&mut plain, out);
            let link_style = style.patch(Style::new().fg(LINK).add_modifier(Modifier::UNDERLINED));
            parse_inline(&s[i + 1..text_end], link_style, out);
            i = url_end + 1;
            continue;
        }
        if let Some((open, end, modifier)) = emphasis(s, i) {
            flush(&mut plain, out);
            let inner = style.patch(Style::new().add_modifier(modifier));
            parse_inline(&s[i + open..end], inner, out);
            i = end + open;
            continue;
        }
        plain.push(c);
        i += 1;
    }
    flush(&mut plain, out);
}

/// Find the position of `pattern` in `s` at or after `from`.
fn find(s: &[char], from: usize, pattern: &[char]) -> Option<usize> {
    (from..=s.len().checked_sub(pattern.len())?).find(|&j| s[j..j + pattern.len()] == *pattern)
}

/// Parse `[text](url)` at `i`; return the positions of `]` and `)`.
fn link(s: &[char], i: usize) -> Option<(usize, usize)> {
    let text_end = find(s, i + 1, &[']'])?;
    if s.get(text_end + 1) != Some(&'(') || text_end == i + 1 {
        return None;
    }
    let url_end = find(s, text_end + 2, &[')'])?;
    Some((text_end, url_end))
}

/// Parse emphasis at `i`: `**bold**`, `__bold__`, `~~strike~~`, `*italic*`
/// or `_italic_`. Return the length of the delimiter, the start of the
/// closing delimiter and the modifier. An underscore inside a word, as in
/// `snake_case`, is no delimiter.
fn emphasis(s: &[char], i: usize) -> Option<(usize, usize, Modifier)> {
    let c = s[i];
    let (delim, modifier): (&[char], Modifier) = match (c, s.get(i + 1)) {
        ('*', Some('*')) => (&['*', '*'], Modifier::BOLD),
        ('_', Some('_')) => (&['_', '_'], Modifier::BOLD),
        ('~', Some('~')) => (&['~', '~'], Modifier::CROSSED_OUT),
        ('*', _) => (&['*'], Modifier::ITALIC),
        ('_', _) => (&['_'], Modifier::ITALIC),
        _ => return None,
    };
    let start = i + delim.len();
    if start >= s.len() || s[start].is_whitespace() {
        return None;
    }
    let word_char = |j: usize| s.get(j).is_some_and(|c| c.is_alphanumeric());
    if c == '_' && i > 0 && word_char(i - 1) {
        return None;
    }
    // A single delimiter does not close on a part of a double one.
    let single = delim.len() == 1;
    let mut from = start + 1;
    while let Some(end) = find(s, from, delim) {
        let blocked = s[end - 1].is_whitespace()
            || (c == '_' && word_char(end + delim.len()))
            || (single && (s[end - 1] == c || s.get(end + 1) == Some(&c)));
        if !blocked {
            return Some((delim.len(), end, modifier));
        }
        from = end + 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(spans: &[Span]) -> String {
        spans.iter().map(|s| s.content.as_ref()).collect()
    }

    fn has(spans: &[Span], content: &str, modifier: Modifier) -> bool {
        spans
            .iter()
            .any(|s| s.content == content && s.style.add_modifier.contains(modifier))
    }

    #[test]
    fn inline_syntax() {
        let spans = inline(
            "a **b** *c* `d` ~~e~~ [f](http://x) snake_case \\*g\\*",
            Style::new(),
        );
        assert_eq!(text(&spans), "a b c d e f snake_case *g*");
        assert!(has(&spans, "b", Modifier::BOLD));
        assert!(has(&spans, "c", Modifier::ITALIC));
        assert!(has(&spans, "e", Modifier::CROSSED_OUT));
        assert!(has(&spans, "f", Modifier::UNDERLINED));
    }

    #[test]
    fn unclosed_syntax_is_plain() {
        let spans = inline("2 * 3 = 6, a*b, `open, **x", Style::new());
        assert_eq!(text(&spans), "2 * 3 = 6, a*b, `open, **x");
    }

    #[test]
    fn blocks() {
        let lines = render(
            "# Head\n- item\n  - [x] done\n1. first\n> quote\n```\n**raw**\n```\n---",
            Style::new(),
        );
        let rows: Vec<String> = lines.iter().map(|l| text(&l.spans)).collect();
        assert_eq!(
            rows,
            [
                "Head",
                "• item",
                "  ☑ done",
                "1. first",
                "│ quote",
                "**raw**",
                "─".repeat(24).as_str()
            ]
        );
    }
}
