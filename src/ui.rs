//! Rendering.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Clear, List, ListItem, ListState, Paragraph, Wrap};

use crate::app::{App, FIELD_NAMES, Form, Mode};
use crate::backlog::{Issue, Status};
use crate::input::LineEdit;

// Dracula palette, https://draculatheme.com/contribute
const BG: Color = Color::Rgb(0x28, 0x2a, 0x36);
const CURRENT: Color = Color::Rgb(0x44, 0x47, 0x5a);
const FG: Color = Color::Rgb(0xf8, 0xf8, 0xf2);
const COMMENT: Color = Color::Rgb(0x62, 0x72, 0xa4);
const CYAN: Color = Color::Rgb(0x8b, 0xe9, 0xfd);
const GREEN: Color = Color::Rgb(0x50, 0xfa, 0x7b);
const ORANGE: Color = Color::Rgb(0xff, 0xb8, 0x6c);
const PINK: Color = Color::Rgb(0xff, 0x79, 0xc6);
const PURPLE: Color = Color::Rgb(0xbd, 0x93, 0xf9);
const RED: Color = Color::Rgb(0xff, 0x55, 0x55);
const YELLOW: Color = Color::Rgb(0xf1, 0xfa, 0x8c);

const ACCENT: Color = PURPLE;
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

fn block(title: &str, focused: bool) -> Block<'static> {
    let color = if focused { ACCENT } else { COMMENT };
    Block::bordered()
        .border_type(BorderType::Rounded)
        .style(Style::new().fg(FG).bg(BG))
        .border_style(Style::new().fg(color))
        .title(Span::styled(
            format!(" {title} "),
            Style::new().fg(FG).bold(),
        ))
}

pub fn draw(f: &mut Frame, app: &mut App) {
    let [header, body, footer] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(f.area());
    f.render_widget(Block::new().style(Style::new().fg(FG).bg(BG)), f.area());

    let (list_area, detail_area) = app.split_body(body);
    app.set_page_height(list_area.height.saturating_sub(2));

    draw_header(f, app, header);
    draw_list(f, app, list_area);
    draw_detail(f, app.selected(), detail_area);
    draw_footer(f, app, footer);

    match &app.mode {
        Mode::Form(form) => draw_form(f, form, body),
        Mode::ConfirmDelete(id) => draw_confirm(f, &format!("Delete #{id} permanently?"), body),
        Mode::Help => draw_help(f, body),
        Mode::List | Mode::Search(_) => {}
    }
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

fn draw_list(f: &mut Frame, app: &App, area: Rect) {
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
            ListItem::new(Line::from(vec![
                Span::styled(format!(" {:>width$} ", i.id), Style::new().fg(COMMENT)),
                Span::styled(
                    format!("{} ", status_symbol(i.status)),
                    status_style(i.status),
                ),
                Span::styled(i.title.clone(), title),
            ]))
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
        return;
    }
    let list = List::new(items)
        .block(b)
        .highlight_style(Style::new().bg(CURRENT).add_modifier(Modifier::BOLD))
        .highlight_symbol("▌");
    let mut state = ListState::default().with_selected(Some(app.cursor));
    f.render_stateful_widget(list, area, &mut state);
}

fn section<'a>(name: &'a str, body: &'a str) -> Vec<Line<'a>> {
    let mut lines = vec![Line::styled(name, Style::new().fg(PINK).bold())];
    if body.trim().is_empty() {
        lines.push(Line::styled("—", Style::new().fg(COMMENT)));
    } else {
        lines.extend(body.lines().map(Line::raw));
    }
    lines.push(Line::raw(""));
    lines
}

fn draw_detail(f: &mut Frame, issue: Option<&Issue>, area: Rect) {
    let Some(i) = issue else {
        f.render_widget(block("Details", false), area);
        return;
    };
    let mut lines = vec![
        Line::styled(i.title.as_str(), Style::new().bold()),
        Line::styled(
            format!("{} {}", status_symbol(i.status), i.status.as_str()),
            status_style(i.status),
        ),
        Line::raw(""),
    ];
    lines.extend(section("Description", &i.description));
    lines.extend(section("Done when", &i.done_when));
    lines.extend(section("Notes", &i.notes));
    let p = Paragraph::new(Text::from(lines))
        .wrap(Wrap { trim: false })
        .block(block(&format!("#{}", i.id), false));
    f.render_widget(p, area);
}

fn hints(pairs: &[(&str, &str)]) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (key, what) in pairs {
        spans.push(Span::styled(key.to_string(), Style::new().fg(GREEN).bold()));
        spans.push(Span::styled(format!(" {what}  "), Style::new().fg(COMMENT)));
    }
    Line::from(spans)
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    if let Mode::Search(edit) = &app.mode {
        f.render_widget(Span::styled("/", Style::new().fg(ACCENT).bold()), area);
        let field = Rect {
            x: area.x + 1,
            width: area.width.saturating_sub(1),
            ..area
        };
        draw_line_edit(f, edit, field);
        return;
    }
    if let Some(msg) = &app.message {
        f.render_widget(
            Line::styled(format!(" {msg}"), Style::new().fg(YELLOW)),
            area,
        );
        return;
    }
    let line = match &app.mode {
        Mode::Form(form) if form.confirm_discard => {
            hints(&[("y", "discard changes"), ("any key", "go back")])
        }
        Mode::Form(form) if form.edit.is_some() => hints(&[
            ("enter", "next field"),
            ("esc", "done"),
            ("ctrl-c", "undo"),
            ("ctrl-x ctrl-e", "$EDITOR"),
        ]),
        Mode::Form(_) => hints(&[
            ("j/k", "field"),
            ("i", "edit"),
            ("c", "replace"),
            ("e", "$EDITOR"),
            ("w", "save"),
            ("esc", "cancel"),
        ]),
        Mode::ConfirmDelete(_) => hints(&[("y", "delete"), ("any key", "keep")]),
        _ => hints(&[
            ("j/k", "move"),
            ("a", "add"),
            ("enter", "edit"),
            ("x", "done"),
            ("s", "active"),
            ("c", "cancel"),
            ("/", "search"),
            ("?", "help"),
            ("q", "quit"),
        ]),
    };
    f.render_widget(line, area);
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

fn draw_form(f: &mut Frame, form: &Form, area: Rect) {
    let area = popup(
        area,
        90.max(area.width * 4 / 5),
        24.max(area.height * 4 / 5),
    );
    f.render_widget(Clear, area);
    let title = match form.id {
        Some(id) => format!("Edit #{id}"),
        None => "New issue".to_string(),
    };
    let outer = block(&title, true);
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let rows = Layout::vertical([
        Constraint::Length(3),
        Constraint::Fill(1),
        Constraint::Fill(1),
        Constraint::Fill(1),
    ])
    .areas::<4>(inner);
    for (n, rect) in rows.into_iter().enumerate() {
        let selected = n == form.sel;
        let editing = selected && form.edit.is_some();
        let color = match (selected, editing) {
            (_, true) => EDITING,
            (true, false) => ACCENT,
            _ => COMMENT,
        };
        let b = Block::bordered()
            .border_type(BorderType::Rounded)
            .style(Style::new().fg(FG).bg(BG))
            .border_style(Style::new().fg(color))
            .title(Span::styled(
                format!(" {} ", FIELD_NAMES[n]),
                Style::new().fg(color).bold(),
            ));
        let content = b.inner(rect);
        f.render_widget(b, rect);
        match &form.edit {
            Some(edit) if editing => draw_line_edit(f, edit, content),
            _ if form.fields[n].is_empty() => f.render_widget(
                Span::styled("empty", Style::new().fg(COMMENT).italic()),
                content,
            ),
            _ => f.render_widget(
                Paragraph::new(form.fields[n].as_str()).wrap(Wrap { trim: false }),
                content,
            ),
        }
    }
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

fn draw_help(f: &mut Frame, area: Rect) {
    const KEYS: &[(&str, &[(&str, &str)])] = &[
        (
            "List",
            &[
                ("j k ↓ ↑", "move down / up"),
                ("g G", "first / last issue"),
                ("ctrl-d ctrl-u", "half page down / up"),
                ("a", "add issue"),
                ("enter e l", "edit issue"),
                ("x", "toggle done"),
                ("s", "toggle active"),
                ("c", "toggle cancelled"),
                ("D", "delete issue"),
                ("/ esc", "search / clear search"),
                ("q", "quit"),
            ],
        ),
        (
            "Form",
            &[
                ("j k tab", "next / previous field"),
                ("i a enter", "edit field inline"),
                ("c", "replace field inline"),
                ("e", "edit field in $EDITOR"),
                ("w ctrl-s", "save"),
                ("esc q", "cancel"),
            ],
        ),
        (
            "Inline editing",
            &[
                ("ctrl-a ctrl-e", "start / end of line"),
                ("ctrl-b ctrl-f", "character left / right"),
                ("alt-b alt-f", "word left / right"),
                ("ctrl-w alt-d", "cut word before / after"),
                ("ctrl-u ctrl-k", "cut to start / end"),
                ("ctrl-y", "paste cut text"),
                ("ctrl-x ctrl-e", "continue in $EDITOR"),
                ("enter esc ctrl-c", "next field / done / undo"),
            ],
        ),
    ];
    let mut lines = Vec::new();
    for (name, keys) in KEYS {
        lines.push(Line::styled(*name, Style::new().fg(PINK).bold()));
        for (k, what) in *keys {
            lines.push(Line::from(vec![
                Span::styled(format!("  {k:<18}"), Style::new().bold()),
                Span::raw(*what),
            ]));
        }
        lines.push(Line::raw(""));
    }
    let height = lines.len() as u16 + 2;
    let area = popup(area, 60, height);
    f.render_widget(Clear, area);
    f.render_widget(
        Paragraph::new(lines).block(block("Keys · any key to close", true)),
        area,
    );
}
