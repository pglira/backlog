//! A small Markdown renderer for the text of the fields.
//!
//! Block syntax: headings, bullet and numbered lists, task lists, block
//! quotes, fenced code blocks, horizontal rules and tables. Inline syntax:
//! `code`, **bold**, *italic*, ~~strikethrough~~, [links](url), ![images](url)
//! and backslash escapes.
//! Unknown or unclosed syntax shows as plain text.

use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::ui::{CODE, COMMENT, GREEN, LINK, MARKER, PURPLE};

/// Rendered Markdown: the lines, and the images that go below them.
pub struct Rendered {
    pub lines: Vec<Line<'static>>,
    /// The index of a line and the URL of an image in it, in the order of
    /// the text.
    pub images: Vec<(usize, String)>,
}

impl Rendered {
    /// Append `other` and keep its images below their lines.
    pub fn extend(&mut self, other: Rendered) {
        let offset = self.lines.len();
        self.lines.extend(other.lines);
        self.images
            .extend(other.images.into_iter().map(|(n, url)| (n + offset, url)));
    }
}

impl From<Vec<Line<'static>>> for Rendered {
    fn from(lines: Vec<Line<'static>>) -> Rendered {
        Rendered {
            lines,
            images: Vec::new(),
        }
    }
}

/// Render `text` as Markdown lines on top of the style `base`. The images
/// show as their alt text in the lines; an image in a table shows only so.
pub fn render(text: &str, base: Style) -> Rendered {
    let source: Vec<&str> = text.lines().collect();
    let mut lines = Vec::new();
    let mut images = Vec::new();
    let mut in_code = false;
    let mut i = 0;
    while i < source.len() {
        let line = source[i];
        i += 1;
        if line.trim_start().starts_with("```") {
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
        if let Some(aligns) = source.get(i).and_then(|next| delimiter_row(line, next)) {
            let mut rows = vec![cells(line)];
            i += 1;
            while i < source.len() && is_table_row(source[i]) {
                rows.push(cells(source[i]));
                i += 1;
            }
            lines.extend(table(&rows, &aligns, base));
            continue;
        }
        let mut urls = Vec::new();
        lines.push(block(line, base, &mut urls));
        images.extend(urls.into_iter().map(|url| (lines.len() - 1, url)));
    }
    Rendered { lines, images }
}

#[derive(Clone, Copy)]
enum Align {
    Left,
    Center,
    Right,
}

/// Tell if a line can be a row of a table: it is not blank and has a `|`.
fn is_table_row(line: &str) -> bool {
    !line.trim().is_empty() && line.contains('|')
}

/// Split a table row into its cells. A `\|` is part of a cell.
fn cells(line: &str) -> Vec<&str> {
    let line = line.trim();
    let line = line.strip_prefix('|').unwrap_or(line);
    let line = match line.strip_suffix('|') {
        Some(rest) if !rest.ends_with('\\') => rest,
        _ => line,
    };
    let mut cells = Vec::new();
    let mut start = 0;
    let mut escaped = false;
    for (n, c) in line.char_indices() {
        if c == '|' && !escaped {
            cells.push(line[start..n].trim());
            start = n + 1;
        }
        escaped = c == '\\' && !escaped;
    }
    cells.push(line[start..].trim());
    cells
}

/// Parse `next` as the delimiter row (`| --- | :-: |`) of a table with the
/// header `header`; return the alignment of each column.
fn delimiter_row(header: &str, next: &str) -> Option<Vec<Align>> {
    if !is_table_row(header) || !is_table_row(next) {
        return None;
    }
    let aligns = cells(next)
        .iter()
        .map(|cell| {
            let left = cell.starts_with(':');
            let right = cell.ends_with(':');
            let dashes = cell.trim_start_matches(':').trim_end_matches(':');
            if dashes.is_empty() || !dashes.chars().all(|c| c == '-') {
                return None;
            }
            Some(match (left, right) {
                (true, true) => Align::Center,
                (false, true) => Align::Right,
                _ => Align::Left,
            })
        })
        .collect::<Option<Vec<_>>>()?;
    (aligns.len() == cells(header).len()).then_some(aligns)
}

/// Render the table `rows` (the header first) with the columns aligned. A row
/// with too few cells gets empty cells; extra cells are dropped. The last
/// column gets no padding after the text, so that a full-width table does
/// not wrap.
fn table(rows: &[Vec<&str>], aligns: &[Align], base: Style) -> Vec<Line<'static>> {
    let header_style = base.patch(Style::new().add_modifier(Modifier::BOLD));
    let rendered: Vec<Vec<Vec<Span<'static>>>> = rows
        .iter()
        .enumerate()
        .map(|(r, row)| {
            let style = if r == 0 { header_style } else { base };
            (0..aligns.len())
                .map(|c| inline(row.get(c).copied().unwrap_or(""), style))
                .collect()
        })
        .collect();
    let width = |cell: &[Span]| cell.iter().map(Span::width).sum::<usize>();
    let widths: Vec<usize> = (0..aligns.len())
        .map(|c| rendered.iter().map(|row| width(&row[c])).max().unwrap_or(0))
        .collect();

    let border = base.patch(Style::new().fg(COMMENT));
    let mut lines = Vec::new();
    let last = aligns.len() - 1;
    for (r, row) in rendered.into_iter().enumerate() {
        let mut spans = Vec::new();
        for (c, cell) in row.into_iter().enumerate() {
            if c > 0 {
                spans.push(Span::styled(" │ ", border));
            }
            let pad = widths[c] - width(&cell);
            let before = match aligns[c] {
                Align::Left => 0,
                Align::Center => pad / 2,
                Align::Right => pad,
            };
            let after = if c == last { 0 } else { pad - before };
            if before > 0 {
                spans.push(Span::raw(" ".repeat(before)));
            }
            spans.extend(cell);
            if after > 0 {
                spans.push(Span::raw(" ".repeat(after)));
            }
        }
        lines.push(Line::from(spans));
        if r == 0 {
            let rule: Vec<String> = widths.iter().map(|&w| "─".repeat(w)).collect();
            lines.push(Line::from(Span::styled(rule.join("─┼─"), border)));
        }
    }
    lines
}

/// Render one line outside of a code block.
fn block(line: &str, base: Style, images: &mut Vec<String>) -> Line<'static> {
    let indent_len = line.len() - line.trim_start().len();
    let indent = line[..indent_len].to_string();
    let rest = &line[indent_len..];

    if let Some((level, title)) = heading(rest) {
        let mut style = base.patch(Style::new().fg(PURPLE).add_modifier(Modifier::BOLD));
        if level == 1 {
            style = style.add_modifier(Modifier::UNDERLINED);
        }
        return Line::from(images_inline(title, style, images));
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
        spans.extend(images_inline(quote.trim_start(), style, images));
        return Line::from(spans);
    }
    if let Some(item) = ["- ", "* ", "+ "].iter().find_map(|m| rest.strip_prefix(m)) {
        let mut spans = vec![Span::raw(indent)];
        let marker = base.patch(Style::new().fg(MARKER));
        if let Some(task) = item.strip_prefix("[ ] ") {
            spans.push(Span::styled("☐ ", marker));
            spans.extend(images_inline(task, base, images));
        } else if let Some(task) = item
            .strip_prefix("[x] ")
            .or_else(|| item.strip_prefix("[X] "))
        {
            spans.push(Span::styled("☑ ", base.patch(Style::new().fg(GREEN))));
            spans.extend(images_inline(
                task,
                base.patch(Style::new().fg(COMMENT)),
                images,
            ));
        } else {
            spans.push(Span::styled("• ", marker));
            spans.extend(images_inline(item, base, images));
        }
        return Line::from(spans);
    }
    if let Some((number, item)) = ordered_item(rest) {
        let mut spans = vec![
            Span::raw(indent),
            Span::styled(format!("{number}. "), base.patch(Style::new().fg(MARKER))),
        ];
        spans.extend(images_inline(item, base, images));
        return Line::from(spans);
    }
    let mut spans = vec![Span::raw(indent)];
    spans.extend(images_inline(rest, base, images));
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
    images_inline(text, base, &mut Vec::new())
}

/// Render the inline syntax of `text` on top of the style `base`, and add
/// the URLs of its images to `images`.
fn images_inline(text: &str, base: Style, images: &mut Vec<String>) -> Vec<Span<'static>> {
    let chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    parse_inline(&chars, base, &mut spans, images);
    spans
}

fn parse_inline(s: &[char], style: Style, out: &mut Vec<Span<'static>>, images: &mut Vec<String>) {
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
        if c == '!'
            && s.get(i + 1) == Some(&'[')
            && let Some((text_end, url_end)) = link(s, i + 1)
        {
            flush(&mut plain, out);
            let alt: String = s[i + 2..text_end].iter().collect();
            let alt = if alt.is_empty() { "image".into() } else { alt };
            out.push(Span::styled(
                format!("[{alt}]"),
                style.patch(Style::new().fg(LINK)),
            ));
            images.push(s[text_end + 2..url_end].iter().collect());
            i = url_end + 1;
            continue;
        }
        if c == '['
            && let Some((text_end, url_end)) = link(s, i)
            && text_end > i + 1
        {
            flush(&mut plain, out);
            let link_style = style.patch(Style::new().fg(LINK).add_modifier(Modifier::UNDERLINED));
            parse_inline(&s[i + 1..text_end], link_style, out, images);
            i = url_end + 1;
            continue;
        }
        if let Some((open, end, modifier)) = emphasis(s, i) {
            flush(&mut plain, out);
            let inner = style.patch(Style::new().add_modifier(modifier));
            parse_inline(&s[i + open..end], inner, out, images);
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

/// Parse `[text](url)` at `i`; return the positions of `]` and `)`. The
/// text can be empty.
fn link(s: &[char], i: usize) -> Option<(usize, usize)> {
    let text_end = find(s, i + 1, &[']'])?;
    if s.get(text_end + 1) != Some(&'(') {
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
    fn tables() {
        let lines = render(
            "| Name | **N** |\n|:-----|---:|\n| a \\| b | 1 |\n| long name | 22 |\nafter",
            Style::new(),
        )
        .lines;
        let rows: Vec<String> = lines.iter().map(|l| text(&l.spans)).collect();
        assert_eq!(
            rows,
            [
                "Name      │  N",
                "──────────┼───",
                "a | b     │  1",
                "long name │ 22",
                "after"
            ]
        );
        assert!(has(&lines[0].spans, "N", Modifier::BOLD));
    }

    #[test]
    fn pipe_without_delimiter_row_is_text() {
        let lines = render("a | b\nc | d", Style::new()).lines;
        let rows: Vec<String> = lines.iter().map(|l| text(&l.spans)).collect();
        assert_eq!(rows, ["a | b", "c | d"]);
    }

    #[test]
    fn blocks() {
        let lines = render(
            "# Head\n- item\n  - [x] done\n1. first\n> quote\n```\n**raw**\n```\n---",
            Style::new(),
        )
        .lines;
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

    #[test]
    fn images() {
        let r = render(
            "text\n- ![Image #1](a.png) and ![](b.png)\n`![x](c.png)`",
            Style::new(),
        );
        let rows: Vec<String> = r.lines.iter().map(|l| text(&l.spans)).collect();
        assert_eq!(rows, ["text", "• [Image #1] and [image]", "![x](c.png)"]);
        assert_eq!(r.images, [(1, "a.png".into()), (1, "b.png".into())]);
    }
}
