//! Rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Margin, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Scrollbar,
    ScrollbarOrientation, ScrollbarState, Wrap,
};
use std::cell::Cell;

use crate::app::{App, FIELD_NAMES, Form, Mode};
use crate::backlog::{Issue, Status};
use crate::images::Images;
use crate::input::LineEdit;
use crate::markdown::{self, Rendered};
use crate::settings::{self, Kind, SETTINGS, Settings};
use crate::textarea::{TextArea, VimMode};

// Dracula palette, https://draculatheme.com/contribute
pub const BG: Color = Color::Rgb(0x28, 0x2a, 0x36);
pub const CURRENT: Color = Color::Rgb(0x44, 0x47, 0x5a);
pub const FG: Color = Color::Rgb(0xf8, 0xf8, 0xf2);
pub const COMMENT: Color = Color::Rgb(0x62, 0x72, 0xa4);
pub const CYAN: Color = Color::Rgb(0x8b, 0xe9, 0xfd);
pub const GREEN: Color = Color::Rgb(0x50, 0xfa, 0x7b);
pub const ORANGE: Color = Color::Rgb(0xff, 0xb8, 0x6c);
pub const PINK: Color = Color::Rgb(0xff, 0x79, 0xc6);
pub const PURPLE: Color = Color::Rgb(0xbd, 0x93, 0xf9);
pub const RED: Color = Color::Rgb(0xff, 0x55, 0x55);
pub const YELLOW: Color = Color::Rgb(0xf1, 0xfa, 0x8c);

const ACCENT: Color = PURPLE;
pub const CODE: Color = GREEN;
pub const LINK: Color = CYAN;
pub const MARKER: Color = PINK;
const EDITING: Color = PINK;

fn status_style(s: Status) -> Style {
    match s {
        Status::Open => Style::new().fg(CYAN),
        Status::Active => Style::new().fg(ORANGE),
        Status::Done => Style::new().fg(GREEN),
        Status::Cancelled => Style::new().fg(COMMENT),
    }
}

fn status_symbol(s: Status) -> &'static str {
    match s {
        Status::Open => "○",
        Status::Active => "◐",
        Status::Done => "✓",
        Status::Cancelled => "✗",
    }
}

thread_local! {
    /// The type of all borders, from the settings of the current draw.
    static BORDER: Cell<BorderType> = const { Cell::new(BorderType::Rounded) };
}

fn border_type() -> BorderType {
    BORDER.with(Cell::get)
}

fn block(title: &str, focused: bool) -> Block<'static> {
    let color = if focused { ACCENT } else { COMMENT };
    Block::bordered()
        .border_type(border_type())
        .style(Style::new().fg(FG).bg(BG))
        .border_style(Style::new().fg(color))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(FG).bold(),
        ))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    BORDER.with(|b| {
        b.set(if app.settings.is_on(settings::ROUNDED) {
            BorderType::Rounded
        } else {
            BorderType::Plain
        })
    });
    let search_height = if matches!(app.mode, Mode::Search(_)) {
        3
    } else {
        0
    };
    let [header, body, search] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(search_height),
    ])
    .areas(f.area());
    f.render_widget(Block::new().style(Style::new().fg(FG).bg(BG)), f.area());

    let (list_area, detail_area) = app.split_body(body);
    app.set_page_height(list_area.height.saturating_sub(2));

    draw_header(f, app, header);
    app.list_offset = draw_list(f, app, list_area);
    if app.show_details {
        // A popup covers most of the details, so they show no images.
        let show = matches!(
            app.mode,
            Mode::List | Mode::Search(_) | Mode::PromptExternal
        );
        draw_detail(f, app.selected(), &app.images, show, detail_area);
    }

    match &app.mode {
        Mode::Form(form) => draw_form(f, form, &app.images, body),
        Mode::ConfirmDelete(id) => draw_confirm(f, &format!("Delete #{id} permanently?"), body),
        Mode::Help => draw_help(f, body),
        Mode::Settings(sel) => draw_settings(f, &app.settings, *sel, body),
        Mode::Prompt(edit) => draw_prompt(f, edit, body),
        Mode::Search(edit) => draw_search(f, edit, search),
        Mode::List | Mode::PromptExternal => {}
    }
    draw_notice(f, app, body);
    app.images.check_sent(f.buffer_mut());
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let count = |s: Status| app.backlog.issues.iter().filter(|i| i.status == s).count();
    let mut spans = vec![
        Span::styled(" backlog ", Style::new().fg(BG).bg(ACCENT).bold()),
        Span::raw(" "),
        Span::styled(
            app.backlog.path.display().to_string(),
            Style::new().fg(COMMENT),
        ),
        Span::raw("   "),
    ];
    for s in [
        Status::Open,
        Status::Active,
        Status::Done,
        Status::Cancelled,
    ] {
        spans.push(Span::styled(
            format!("{} {} {}  ", status_symbol(s), count(s), s.as_str()),
            status_style(s),
        ));
    }
    f.render_widget(Line::from(spans), area);
}

/// Draw the issue list and return the index of its first visible row.
fn draw_list(f: &mut Frame, app: &App, area: Rect) -> usize {
    let vis = app.visible();
    let width = app
        .backlog
        .issues
        .iter()
        .map(|i| i.id)
        .max()
        .unwrap_or(1)
        .to_string()
        .len();
    let items: Vec<ListItem> = vis
        .iter()
        .map(|&n| {
            let i = &app.backlog.issues[n];
            let mut title = Style::new();
            if i.status.is_closed() {
                title = title.fg(COMMENT);
            }
            if i.status == Status::Cancelled {
                title = title.add_modifier(Modifier::CROSSED_OUT);
            }
            let mut spans = vec![
                Span::styled(format!(" {:>width$} ", i.id), Style::new().fg(COMMENT)),
                Span::styled(
                    format!("{} ", status_symbol(i.status)),
                    status_style(i.status),
                ),
            ];
            spans.extend(title_spans(i, title));
            ListItem::new(Line::from(spans))
        })
        .collect();

    let mut info = Vec::new();
    if !app.query.is_empty() {
        info.push(format!("/{}", app.query));
    }
    if app.hidden_count() > 0 {
        info.push(format!("{} hidden", app.hidden_count()));
    }
    let mut b = block(
        &format!("Issues {}", vis.len()),
        matches!(app.mode, Mode::List | Mode::Search(_)),
    );
    if !info.is_empty() {
        b = b.title(
            Line::from(format!(" {} ", info.join(" · ")))
                .right_aligned()
                .fg(COMMENT),
        );
    }

    if items.is_empty() {
        let hint = if app.backlog.issues.is_empty() {
            "No issues yet. Press a to add one."
        } else {
            "No issues match. Press Esc to clear the search."
        };
        f.render_widget(
            Paragraph::new(hint)
                .fg(COMMENT)
                .wrap(Wrap { trim: true })
                .block(b),
            area,
        );
        return 0;
    }
    let list = List::new(items)
        .block(b)
        .highlight_style(Style::new().bg(CURRENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("▌");
    let mut state = ListState::default()
        .with_selected(Some(app.cursor))
        .with_offset(app.list_offset);
    f.render_stateful_widget(list, area, &mut state);
    state.offset()
}

/// The title of an issue as rendered Markdown. An issue without a title
/// shows "(no title)" and the start of its description.
fn title_spans(i: &Issue, style: Style) -> Vec<Span<'static>> {
    if !i.title.is_empty() {
        return markdown::inline(&i.title, style);
    }
    let first = i.description.lines().next().unwrap_or("");
    let mut start: String = first.chars().take(60).collect();
    if first.chars().count() > 60 {
        start.push('…');
    }
    let dim = Style::new().fg(COMMENT).italic();
    vec![
        Span::styled("(no title) ", dim.bold()),
        Span::styled(start, dim),
    ]
}

fn section(name: &'static str, body: &str) -> Rendered {
    let mut r = Rendered::from(vec![Line::styled(name, Style::new().fg(PINK).bold())]);
    if body.trim().is_empty() {
        r.lines.push(Line::styled("—", Style::new().fg(COMMENT)));
    } else {
        r.extend(markdown::render(body, Style::new()));
    }
    r.lines.push(Line::raw(""));
    r
}

/// Draw the details of `issue`. Without `show_images`, the images show only
/// as their alt text.
fn draw_detail(
    f: &mut Frame,
    issue: Option<&Issue>,
    images: &Images,
    show_images: bool,
    area: Rect,
) {
    let Some(i) = issue else {
        f.render_widget(block("Details", false), area);
        return;
    };
    let mut r = Rendered::from(vec![
        Line::from(title_spans(i, Style::new().bold())),
        Line::styled(
            format!("{} {}", status_symbol(i.status), i.status.as_str()),
            status_style(i.status),
        ),
        Line::raw(""),
    ]);
    r.extend(section("Description", &i.description));
    r.extend(section("Done when", &i.done_when));
    r.extend(section("Notes", &i.notes));
    r.extend(section("Outcome", &i.outcome));
    if !show_images {
        r.images.clear();
    }
    let b = block(&format!("#{}", i.id), false);
    let inner = b.inner(area);
    f.render_widget(b, area);
    RichText::new(r, images, inner.width).draw(f, images, inner, 0);
}

/// Rendered Markdown wrapped to a width, with empty rows below the lines
/// that hold images. The images go into these rows.
struct RichText {
    lines: Vec<Line<'static>>,
    /// The first row and the URL of each image.
    images: Vec<(usize, String)>,
    /// The width of the images in cells.
    image_width: u16,
    rows: usize,
}

impl RichText {
    /// Wrap `r` to `width` and make room for the images, each scaled to
    /// `images.width` cells, or to `width` if that is less. An image that
    /// cannot be read, or any image if `images.width` is 0, shows only as its
    /// alt text.
    fn new(r: Rendered, images: &Images, width: u16) -> RichText {
        let mut text = RichText {
            lines: Vec::new(),
            images: Vec::new(),
            image_width: images.width.min(width),
            rows: 0,
        };
        if width == 0 {
            return text;
        }
        let shown = if text.image_width == 0 {
            Vec::new()
        } else {
            r.images
        };
        let mut pending = shown.into_iter().peekable();
        for (n, line) in r.lines.into_iter().enumerate() {
            text.rows += Paragraph::new(line.clone())
                .wrap(Wrap { trim: false })
                .line_count(width);
            text.lines.push(line);
            while let Some((_, url)) = pending.next_if(|(m, _)| *m == n) {
                let Some(size) = images.size(&url, text.image_width) else {
                    continue;
                };
                text.images.push((text.rows, url));
                let height = size.height as usize;
                text.lines
                    .extend(std::iter::repeat_n(Line::raw(""), height));
                text.rows += height;
            }
        }
        text
    }

    /// Draw the rows from `scroll` on into `area`.
    fn draw(self, f: &mut Frame, images: &Images, area: Rect, scroll: u16) {
        let p = Paragraph::new(self.lines).wrap(Wrap { trim: false });
        f.render_widget(p.scroll((scroll, 0)), area);
        for (row, url) in self.images {
            let y = (row as i64 - scroll as i64).clamp(i16::MIN as i64, i16::MAX as i64);
            images.draw(f, &url, self.image_width, area, (0, y as i16).into());
        }
    }
}

fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (key, what) in pairs {
        spans.push(Span::styled(key.to_string(), Style::new().fg(GREEN).bold()));
        spans.push(Span::styled(format!(" {what}  "), Style::new().fg(COMMENT)));
    }
    Line::from(spans)
}

fn draw_search(f: &mut Frame, edit: &LineEdit, area: Rect) {
    let b = block("Search", true);
    let inner = b.inner(area);
    f.render_widget(b, area);
    let [slash, field] =
        Layout::horizontal([Constraint::Length(2), Constraint::Min(0)]).areas(inner);
    f.render_widget(Span::styled("/", Style::new().fg(ACCENT).bold()), slash);
    draw_line_edit(f, edit, field);
}

/// Draw the status message, or the question to discard the form changes, in a
/// box at the bottom right of `area`.
fn draw_notice(f: &mut Frame, app: &App, area: Rect) {
    let (title, line) = if let Some(msg) = &app.message {
        (
            "Message",
            Line::styled(format!(" {msg} "), Style::new().fg(YELLOW)),
        )
    } else if let Mode::Form(form) = &app.mode
        && form.confirm_discard
    {
        (
            "Discard?",
            hints(&[("y", "discard changes"), ("any key", "go back")]),
        )
    } else {
        return;
    };
    let w = (line.width().max(title.len() + 4) as u16 + 2).min(area.width);
    let h = 3.min(area.height);
    let area = Rect::new(area.right() - w, area.bottom() - h, w, h);
    f.render_widget(Clear, area);
    let b = block(title, true).border_style(Style::new().fg(YELLOW));
    f.render_widget(Paragraph::new(line).block(b), area);
}

/// Draw a line editor and put the terminal cursor at its cursor.
fn draw_line_edit(f: &mut Frame, edit: &LineEdit, area: Rect) {
    let (text, col) = edit.view(area.width as usize);
    f.render_widget(Span::raw(text), area);
    f.set_cursor_position(Position::new(area.x + col as u16, area.y));
}

fn popup(area: Rect, width: u16, height: u16) -> Rect {
    let w = width.min(area.width);
    let h = height.min(area.height);
    Rect::new(
        area.x + (area.width - w) / 2,
        area.y + (area.height - h) / 2,
        w,
        h,
    )
}

fn form_area(body: Rect) -> Rect {
    popup(
        body,
        90.max(body.width * 4 / 5),
        32.max(body.height * 4 / 5),
    )
}

/// The areas of the form fields, with their borders, for the main area `body`.
/// The left half holds one text row for the title, 13 for the description,
/// two for the "done when" condition, and the rest for the notes. The outcome
/// takes the right half.
pub fn form_fields(body: Rect) -> [Rect; 5] {
    let [left, outcome] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)])
        .areas(form_area(body).inner(Margin::new(1, 1)));
    let [title, description, done_when, notes] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(15),
        Constraint::Length(4),
        Constraint::Fill(1),
    ])
    .areas(left);
    [title, description, done_when, notes, outcome]
}

/// Draw a vertical scroll bar on the right border of `area` when `total`
/// rows do not fit into the `height` rows inside the border.
fn draw_scrollbar(f: &mut Frame, area: Rect, total: usize, offset: usize, color: Color) {
    let height = area.height.saturating_sub(2) as usize;
    if total <= height {
        return;
    }
    let mut state = ScrollbarState::new(total - height + 1)
        .viewport_content_length(height)
        .position(offset);
    let bar = Scrollbar::new(ScrollbarOrientation::VerticalRight)
        .begin_symbol(None)
        .end_symbol(None)
        .track_symbol(Some("│"))
        .thumb_symbol("┃")
        .track_style(Style::new().fg(COMMENT))
        .thumb_style(Style::new().fg(color));
    f.render_stateful_widget(bar, area.inner(Margin::new(0, 1)), &mut state);
}

/// Draw a text editor inside the border `area`, with a scroll bar, and put
/// the terminal cursor at its cursor.
fn draw_text_area(f: &mut Frame, edit: &TextArea, area: Rect, color: Color) {
    let inner = area.inner(Margin::new(1, 1));
    let (rows, (x, y)) = edit.view(inner.width as usize, inner.height as usize);
    let lines: Vec<Line> = rows.into_iter().map(Line::raw).collect();
    f.render_widget(Paragraph::new(lines), inner);
    f.set_cursor_position(Position::new(inner.x + x as u16, inner.y + y as u16));
    let (total, scroll) = edit.scroll_info();
    draw_scrollbar(f, area, total, scroll, color);
}

fn draw_form(f: &mut Frame, form: &Form, images: &Images, body: Rect) {
    let area = form_area(body);
    f.render_widget(Clear, area);
    let title = match form.id {
        Some(id) => format!("Edit #{id}"),
        None => "New issue".to_string(),
    };
    f.render_widget(block(&title, true), area);

    for (n, rect) in form_fields(body).into_iter().enumerate() {
        let selected = n == form.sel;
        let editing = selected && form.edit.is_some();
        let color = match (selected, editing) {
            (_, true) => EDITING,
            (true, false) => ACCENT,
            _ => COMMENT,
        };
        let mode = match &form.edit {
            Some(edit) if editing => match edit.mode {
                VimMode::Insert => " · INSERT",
                VimMode::Normal => " · NORMAL",
            },
            _ => "",
        };
        let b = Block::bordered()
            .border_type(border_type())
            .style(Style::new().fg(FG).bg(BG))
            .border_style(Style::new().fg(color))
            .title(Span::styled(
                format!(" {}{mode} ", FIELD_NAMES[n]),
                Style::new().fg(color).bold(),
            ));
        let content = b.inner(rect);
        f.render_widget(b, rect);
        match &form.edit {
            Some(edit) if editing => draw_text_area(f, edit, rect, color),
            _ if form.fields[n].is_empty() => f.render_widget(
                Span::styled("empty", Style::new().fg(COMMENT).italic()),
                content,
            ),
            // Show the fields that are not in edit as rendered Markdown.
            _ => {
                let r = if n == 0 {
                    Rendered::from(vec![Line::from(markdown::inline(
                        &form.fields[n],
                        Style::new(),
                    ))])
                } else {
                    markdown::render(&form.fields[n], Style::new())
                };
                let text = RichText::new(r, images, content.width);
                let total = text.rows;
                let max = total.saturating_sub(content.height as usize) as u16;
                let offset = form.scroll[n].get().min(max);
                form.scroll[n].set(offset);
                text.draw(f, images, content, offset);
                draw_scrollbar(f, rect, total, offset as usize, color);
            }
        }
    }
}

pub fn prompt_area(body: Rect) -> Rect {
    popup(body, 90.max(body.width * 4 / 5), 14)
}

fn draw_prompt(f: &mut Frame, edit: &TextArea, body: Rect) {
    let area = prompt_area(body);
    f.render_widget(Clear, area);
    let mode = match edit.mode {
        VimMode::Insert => "INSERT",
        VimMode::Normal => "NORMAL",
    };
    let b = block(&format!("Agent prompt · {mode}"), true)
        .border_style(Style::new().fg(EDITING))
        .title_bottom(
            Line::from(" {id} {title} {path} are replaced · empty: default ")
                .right_aligned()
                .fg(COMMENT),
        );
    f.render_widget(b, area);
    draw_text_area(f, edit, area, EDITING);
}

fn draw_confirm(f: &mut Frame, question: &str, area: Rect) {
    let area = popup(area, question.len() as u16 + 8, 4);
    f.render_widget(Clear, area);
    let text = vec![
        Line::raw(question).bold(),
        hints(&[("y", "yes"), ("n", "no")]),
    ];
    let b = block("Confirm", true).border_style(Style::new().fg(RED));
    f.render_widget(Paragraph::new(text).centered().block(b), area);
}

fn draw_settings(f: &mut Frame, settings: &Settings, sel: usize, area: Rect) {
    let mut lines: Vec<Line> = SETTINGS
        .iter()
        .enumerate()
        .map(|(n, t)| {
            let (mark, color) = match t.kind {
                Kind::Toggle if settings.is_on(n) => ("[x]".to_string(), GREEN),
                Kind::Toggle => ("[ ]".to_string(), COMMENT),
                Kind::Number { unit, .. } => (format!("{}{unit}", settings.get(n)), CYAN),
            };
            let style = if n == sel {
                Style::new().bg(CURRENT).bold()
            } else {
                Style::new()
            };
            Line::from(vec![
                Span::styled(format!(" {mark:<9}"), style.fg(color)),
                Span::styled(format!("{} ", t.label), style),
            ])
        })
        .collect();
    lines.push(Line::raw(""));
    lines.push(hints(&[
        ("space", "toggle"),
        ("h l", "change"),
        ("esc", "close"),
    ]));
    let width = lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4;
    let area = popup(area, width.max(30), lines.len() as u16 + 2);
    f.render_widget(Clear, area);
    f.render_widget(Paragraph::new(lines).block(block("Settings", true)), area);
}

type KeyHelp = &'static [(&'static str, &'static [(&'static str, &'static str)])];

fn help_lines(sections: KeyHelp) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for (name, keys) in sections {
        lines.push(Line::styled(*name, Style::new().fg(PINK).bold()));
        for (k, what) in *keys {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<16}"), Style::new().bold()),
                Span::raw(*what),
            ]));
        }
        lines.push(Line::raw(""));
    }
    lines
}

fn draw_help(f: &mut Frame, area: Rect) {
    const LEFT: KeyHelp = &[
        (
            "List",
            &[
                ("j k ↓ ↑", "move down / up"),
                ("gg G", "first / last issue"),
                ("ctrl-d ctrl-u", "half page down / up"),
                ("a o", "add issue"),
                ("enter e l", "edit issue"),
                ("x", "toggle done"),
                ("s", "toggle active"),
                ("c", "toggle cancelled"),
                ("dd", "delete issue"),
                ("y", "copy a prompt for an agent"),
                ("P", "edit the agent prompt"),
                ("E", "edit the agent prompt in $EDITOR"),
                ("p", "show / hide details"),
                ("S", "settings"),
                ("/ esc", "search / clear search"),
                ("q", "quit"),
            ],
        ),
        (
            "Mouse",
            &[
                ("click", "select issue or field"),
                ("double-click", "edit issue or field"),
                ("wheel", "move down / up; scroll a form field"),
                ("drag border", "resize list and details"),
            ],
        ),
        (
            "Form",
            &[
                ("j k tab", "next / previous field"),
                ("h l", "left fields / outcome"),
                ("i a enter", "edit field inline"),
                ("c", "replace field inline"),
                ("e", "edit field in $EDITOR"),
                ("ctrl-e ctrl-y", "scroll field down / up"),
                ("w ctrl-s", "save"),
                ("esc q", "cancel"),
            ],
        ),
    ];
    const RIGHT: KeyHelp = &[
        (
            "Field: insert mode",
            &[
                ("esc", "normal mode"),
                ("enter", "new line (title: next field)"),
                ("ctrl-a ctrl-e", "start / end of line"),
                ("ctrl-b ctrl-f", "character left / right"),
                ("alt-b alt-f", "word left / right"),
                ("ctrl-w alt-d", "cut word before / after"),
                ("ctrl-u ctrl-k", "cut to start / end"),
                ("ctrl-y", "paste cut text"),
            ],
        ),
        (
            "Field: normal mode",
            &[
                ("h j k l w b e", "move"),
                ("0 ^ $ gg G", "line start / end, first / last line"),
                ("i a I A o O", "insert mode"),
                ("x dd D dw d$", "delete"),
                ("cw cc C s S", "change"),
                ("yy yw p P", "copy / paste"),
                ("J r<char>", "join lines / replace character"),
                ("u ctrl-r", "undo / redo"),
                ("esc", "stop editing the field"),
            ],
        ),
        (
            "Field: both modes",
            &[
                ("ctrl-x ctrl-e", "continue in $EDITOR"),
                ("ctrl-v", "paste an image (not in the title)"),
                ("ctrl-c", "discard the changes"),
            ],
        ),
    ];
    let (left, right) = (help_lines(LEFT), help_lines(RIGHT));
    let height = left.len().max(right.len()) as u16 + 2;
    let area = popup(area, 108, height);
    f.render_widget(Clear, area);
    let outer = block("Keys · any key to close", true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);
    let [a, b] = Layout::horizontal([Constraint::Fill(1), Constraint::Fill(1)]).areas(inner);
    f.render_widget(Paragraph::new(left), a);
    f.render_widget(Paragraph::new(right), b);
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use ratatui_image::picker::Picker;

    #[test]
    fn images_below_their_lines() {
        let dir = std::env::temp_dir().join(format!("backlog-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let img = image::RgbImage::from_pixel(40, 40, image::Rgb([255, 0, 0]));
        img.save(dir.join("red.png")).unwrap();
        let images = Images::new(Picker::halfblocks(), dir.clone(), 20);

        let r = markdown::render("a ![Image #1](red.png)\nb ![x](missing.png)", Style::new());
        let text = RichText::new(r, &images, 20);
        let mut terminal = Terminal::new(TestBackend::new(20, 12)).unwrap();
        terminal
            .draw(|f| text.draw(f, &images, f.area(), 0))
            .unwrap();
        std::fs::remove_dir_all(&dir).unwrap();

        let buf = terminal.backend().buffer();
        let row = |y: u16| -> String { (0..20).map(|x| buf[(x, y)].symbol()).collect() };
        assert_eq!(row(0).trim_end(), "a [Image #1]");
        // The image takes the rows below its line; the missing image shows
        // only as its alt text.
        assert_eq!(buf[(0, 1)].bg, Color::Rgb(255, 0, 0));
        let last = (1..12).find(|&y| row(y).starts_with('b')).unwrap();
        assert!(last > 1);
        assert_eq!(row(last).trim_end(), "b [x]");
    }
}
