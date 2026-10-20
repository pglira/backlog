//! Application state and key handling.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Margin, Position, Rect};
use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::backlog::{Backlog, Issue, Status};
use crate::input::{LineEdit, Outcome};
use crate::prompt;
use crate::textarea::TextArea;
use crate::ui;

pub const FIELD_NAMES: [&str; 5] = ["Title", "Description", "Done when", "Notes", "Outcome"];
const TITLE: usize = 0;
const DESCRIPTION: usize = 1;
/// Minimum width of the main area for the list and the details side by side.
const WIDE: u16 = 90;
/// Minimum size of a pane along the split direction.
const MIN_PANE: u16 = 5;
/// Maximum time between the two clicks of a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

pub enum Mode {
    List,
    Search(LineEdit),
    Form(Box<Form>),
    ConfirmDelete(u32),
    Help,
    /// Edit the template of the agent prompt.
    Prompt(TextArea),
    /// Wait for the external editor with the template of the agent prompt.
    PromptExternal,
}

/// The editor for one issue. `id` is `None` for a new issue.
pub struct Form {
    pub id: Option<u32>,
    pub fields: [String; 5],
    orig: [String; 5],
    pub sel: usize,
    pub edit: Option<TextArea>,
    pub confirm_discard: bool,
    /// The first visible row of each field that is not in edit. The draw
    /// clamps it to the length of the text.
    pub scroll: [Cell<u16>; 5],
}

impl Form {
    /// Scroll the field `n` that is not in edit by `dir` rows.
    fn scroll_field(&self, n: usize, dir: i32) {
        let row = (self.scroll[n].get() as i32 + dir).max(0) as u16;
        self.scroll[n].set(row);
    }

    fn is_dirty(&self) -> bool {
        self.fields != self.orig
    }

    /// Start inline editing of the selected field. The title holds one line.
    fn start_edit(&mut self, clear: bool) {
        if clear {
            self.fields[self.sel].clear();
        }
        self.edit = Some(TextArea::new(&self.fields[self.sel], self.sel == TITLE));
    }

    /// Stop inline editing and keep the text.
    fn commit_edit(&mut self) {
        if let Some(edit) = self.edit.take() {
            self.fields[self.sel] = edit.text();
        }
    }

    fn external(&self, text: String) -> Effect {
        Effect::Editor {
            text,
            single_line: self.sel == TITLE,
        }
    }
}

/// Work that the main loop does outside of the TUI.
pub enum Effect {
    None,
    Quit,
    Editor {
        text: String,
        single_line: bool,
    },
    /// Copy the text to the clipboard.
    Copy(String),
}

pub struct App {
    pub backlog: Backlog,
    pub mode: Mode,
    /// Index into `visible()`.
    pub cursor: usize,
    pub query: String,
    pub message: Option<String>,
    page: usize,
    /// Fraction of the main area that the list takes.
    split: f32,
    /// The main area and the list area of the last draw.
    body: Rect,
    list_area: Rect,
    /// The index of the first visible row of the list.
    pub list_offset: usize,
    pub show_details: bool,
    dragging: bool,
    last_click: Option<(Instant, u16, u16)>,
    /// The first key of a two-key list command (`gg`, `dd`).
    pending: Option<char>,
}

impl App {
    pub fn new(backlog: Backlog) -> App {
        App {
            backlog,
            mode: Mode::List,
            cursor: 0,
            query: String::new(),
            message: None,
            page: 10,
            split: 0.45,
            body: Rect::default(),
            list_area: Rect::default(),
            list_offset: 0,
            show_details: true,
            dragging: false,
            last_click: None,
            pending: None,
        }
    }

    /// Split the main area into the list and the details: side by side when
    /// it is wide enough, else one above the other. Without details, the list
    /// takes the full area.
    pub fn split_body(&mut self, body: Rect) -> (Rect, Rect) {
        self.body = body;
        let (list, details) = self.split_rects(body);
        self.list_area = list;
        (list, details)
    }

    fn split_rects(&self, body: Rect) -> (Rect, Rect) {
        if !self.show_details {
            return (body, Rect::default());
        }
        let wide = body.width >= WIDE;
        let first = if wide {
            self.split_size(body.width)
        } else {
            self.split_size(body.height)
        };
        let (a, b) = if wide {
            let a = Rect {
                width: first,
                ..body
            };
            let b = Rect {
                x: body.x + first,
                width: body.width - first,
                ..body
            };
            (a, b)
        } else {
            let a = Rect {
                height: first,
                ..body
            };
            let b = Rect {
                y: body.y + first,
                height: body.height - first,
                ..body
            };
            (a, b)
        };
        (a, b)
    }

    fn split_size(&self, total: u16) -> u16 {
        let max = total.saturating_sub(MIN_PANE);
        let size = (total as f32 * self.split).round() as u16;
        size.clamp(MIN_PANE.min(max), max)
    }

    /// Read the file again after another program changed it, and keep the
    /// cursor on the same issue.
    pub fn reload(&mut self) {
        let id = self.selected().map(|i| i.id);
        match Backlog::load(&self.backlog.path) {
            Ok(backlog) => {
                self.backlog = backlog;
                if let Some(id) = id {
                    self.select_id(id);
                }
                self.clamp_cursor();
                self.message = Some("The file changed on disk; reloaded".into());
            }
            Err(e) => {
                self.message = Some(format!("Cannot read {}: {e}", self.backlog.path.display()))
            }
        }
    }

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        let double = self.double_click(m);
        match self.mode {
            Mode::List | Mode::Search(_) => self.list_mouse(m, double),
            Mode::Form(_) => self.form_mouse(m, double),
            Mode::Help if matches!(m.kind, MouseEventKind::Down(_)) => self.mode = Mode::List,
            Mode::Prompt(ref mut edit) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                let text = ui::prompt_area(self.body).inner(Margin::new(1, 1));
                if text.contains(Position::new(m.column, m.row)) {
                    edit.click((m.column - text.x) as usize, (m.row - text.y) as usize);
                }
            }
            _ => {}
        }
    }

    /// Tell if a left button press is the second click of a double click.
    fn double_click(&mut self, m: MouseEvent) -> bool {
        if m.kind != MouseEventKind::Down(MouseButton::Left) {
            return false;
        }
        let now = Instant::now();
        let double = self
            .last_click
            .is_some_and(|(t, col, row)| col == m.column && row == m.row && now - t < DOUBLE_CLICK);
        self.last_click = if double {
            None
        } else {
            Some((now, m.column, m.row))
        };
        double
    }

    /// Click to select an issue, double-click to edit it, scroll to move the
    /// cursor, and drag the border between the list and the details.
    fn list_mouse(&mut self, m: MouseEvent, double: bool) {
        if self.drag_border(m) {
            return;
        }
        let pos = Position::new(m.column, m.row);
        let rows = self.list_area.inner(Margin::new(1, 1));
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) if rows.contains(pos) => {
                let n = self.list_offset + (m.row - rows.y) as usize;
                if n < self.visible().len() {
                    self.cursor = n;
                    if double {
                        self.open_edit();
                    }
                }
            }
            MouseEventKind::ScrollDown => {
                self.cursor += 1;
                self.clamp_cursor();
            }
            MouseEventKind::ScrollUp => self.cursor = self.cursor.saturating_sub(1),
            _ => {}
        }
    }

    /// Click a field to select it, double-click to edit it, and click into
    /// the field in edit to move the cursor.
    fn form_mouse(&mut self, m: MouseEvent, double: bool) {
        let Mode::Form(form) = &mut self.mode else {
            return;
        };
        if form.confirm_discard {
            return;
        }
        let pos = Position::new(m.column, m.row);
        let fields = ui::form_fields(self.body);
        let Some(n) = fields.iter().position(|r| r.contains(pos)) else {
            return;
        };
        let dir = match m.kind {
            MouseEventKind::ScrollDown => 1,
            MouseEventKind::ScrollUp => -1,
            MouseEventKind::Down(MouseButton::Left) => 0,
            _ => return,
        };
        if dir != 0 {
            match form.edit.as_mut() {
                Some(edit) if n == form.sel => edit.scroll_by(dir as isize),
                _ => form.scroll_field(n, dir),
            }
            return;
        }
        let text = fields[n].inner(Margin::new(1, 1));
        match form.edit.as_mut() {
            Some(edit) if n == form.sel => {
                if text.contains(pos) {
                    edit.click((m.column - text.x) as usize, (m.row - text.y) as usize);
                }
            }
            _ => {
                form.commit_edit();
                form.sel = n;
                if double {
                    form.start_edit(false);
                }
            }
        }
    }

    /// Drag the border between the list and the details with the left button.
    /// Return true when the event belongs to a drag.
    fn drag_border(&mut self, m: MouseEvent) -> bool {
        if !self.show_details {
            return false;
        }
        let b = self.body;
        let wide = b.width >= WIDE;
        // `pos` runs along the split direction, `across` along the border.
        let (pos, start, total, across, lo, len) = if wide {
            (m.column, b.x, b.width, m.row, b.y, b.height)
        } else {
            (m.row, b.y, b.height, m.column, b.x, b.width)
        };
        // The border is the last cell of the list and the first cell of the details.
        let border = start + self.split_size(total);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                self.dragging = (border.saturating_sub(1)..=border).contains(&pos)
                    && (lo..lo + len).contains(&across);
            }
            MouseEventKind::Drag(MouseButton::Left) if self.dragging && total > 0 => {
                self.split =
                    ((pos + 1).saturating_sub(start) as f32 / total as f32).clamp(0.0, 1.0);
            }
            MouseEventKind::Up(MouseButton::Left) => {
                return std::mem::take(&mut self.dragging);
            }
            _ => {}
        }
        self.dragging
    }

    /// The prompt for a coding agent to work on the selected issue.
    fn agent_instruction(&self) -> Option<String> {
        let i = self.selected()?;
        let template = self.template();
        let path = self.backlog.path.display().to_string();
        Some(prompt::fill(template, i.id, &i.title, &path))
    }

    /// Keep the edited prompt template in the backlog file. An empty
    /// template, or one equal to the default, removes it from the file.
    /// The template of the agent prompt.
    fn template(&self) -> &str {
        self.backlog.prompt.as_deref().unwrap_or(prompt::DEFAULT)
    }

    fn save_prompt(&mut self, text: &str) {
        let text = text.trim();
        self.backlog.prompt = if text.is_empty() || text == prompt::DEFAULT {
            None
        } else {
            Some(text.to_string())
        };
        self.message = Some(match self.backlog.prompt {
            Some(_) => "Saved the agent prompt".into(),
            None => "The agent prompt is the default".into(),
        });
        self.save();
    }

    pub fn set_page_height(&mut self, rows: u16) {
        self.page = (rows as usize).max(2);
    }

    /// Indices into `backlog.issues` of the issues that match the search.
    pub fn visible(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        self.backlog
            .issues
            .iter()
            .enumerate()
            .filter(|(_, i)| {
                query.is_empty()
                    || [&i.title, &i.description, &i.done_when, &i.notes, &i.outcome]
                        .iter()
                        .any(|s| s.to_lowercase().contains(&query))
            })
            .map(|(n, _)| n)
            .collect()
    }

    pub fn selected(&self) -> Option<&Issue> {
        self.visible()
            .get(self.cursor)
            .map(|&n| &self.backlog.issues[n])
    }

    pub fn hidden_count(&self) -> usize {
        self.backlog.issues.len() - self.visible().len()
    }

    fn clamp_cursor(&mut self) {
        self.cursor = self.cursor.min(self.visible().len().saturating_sub(1));
    }

    fn select_id(&mut self, id: u32) {
        if let Some(pos) = self
            .visible()
            .iter()
            .position(|&n| self.backlog.issues[n].id == id)
        {
            self.cursor = pos;
        }
        self.clamp_cursor();
    }

    fn save(&mut self) {
        if let Err(e) = self.backlog.save() {
            self.message = Some(format!("Cannot write {}: {e}", self.backlog.path.display()));
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> Effect {
        self.message = None;
        match std::mem::replace(&mut self.mode, Mode::List) {
            Mode::List => self.list_key(key),
            Mode::Search(edit) => {
                self.search_key(edit, key);
                Effect::None
            }
            Mode::Form(form) => self.form_key(form, key),
            Mode::ConfirmDelete(id) => {
                if key.code == KeyCode::Char('y') {
                    self.backlog.issues.retain(|i| i.id != id);
                    self.message = Some(format!("Deleted #{id}"));
                    self.save();
                    self.clamp_cursor();
                }
                Effect::None
            }
            Mode::Help | Mode::PromptExternal => Effect::None,
            Mode::Prompt(mut edit) => match edit.handle(key) {
                Outcome::Leave | Outcome::Submit => {
                    self.save_prompt(&edit.text());
                    Effect::None
                }
                Outcome::Abort => Effect::None,
                Outcome::Continue => {
                    self.mode = Mode::Prompt(edit);
                    Effect::None
                }
                Outcome::External => {
                    let text = edit.text();
                    self.mode = Mode::Prompt(edit);
                    Effect::Editor {
                        text,
                        single_line: false,
                    }
                }
            },
        }
    }

    /// Put the text from the external editor into the selected form field or
    /// into the prompt editor, or save it as the agent prompt.
    pub fn editor_done(&mut self, result: std::io::Result<String>) {
        let external = matches!(self.mode, Mode::PromptExternal);
        if external {
            self.mode = Mode::List;
        }
        let text = match result {
            Ok(text) => text,
            Err(e) => {
                self.message = Some(format!("Editor failed: {e}"));
                return;
            }
        };
        if external {
            self.save_prompt(&text);
            return;
        }
        match &mut self.mode {
            Mode::Form(form) => form.fields[form.sel] = text,
            Mode::Prompt(edit) => *edit = TextArea::new(&text, false),
            _ => {}
        }
    }

    fn list_key(&mut self, key: KeyEvent) -> Effect {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.visible().len();
        let pending = self.pending.take();
        match key.code {
            KeyCode::Char('g') if pending == Some('g') => self.cursor = 0,
            KeyCode::Char('d') if pending == Some('d') => {
                if let Some(i) = self.selected() {
                    self.mode = Mode::ConfirmDelete(i.id);
                }
            }
            KeyCode::Char(c @ ('g' | 'd')) if !ctrl => self.pending = Some(c),

            KeyCode::Char('q') => return Effect::Quit,
            KeyCode::Char('c') if ctrl => return Effect::Quit,
            KeyCode::Char('?') => self.mode = Mode::Help,

            KeyCode::Char('j') | KeyCode::Down => self.cursor += 1,
            KeyCode::Char('k') | KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Home => self.cursor = 0,
            KeyCode::Char('G') | KeyCode::End => self.cursor = len,
            KeyCode::Char('d') if ctrl => self.cursor += self.page / 2,
            KeyCode::Char('u') if ctrl => self.cursor = self.cursor.saturating_sub(self.page / 2),
            KeyCode::PageDown => self.cursor += self.page,
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(self.page),

            KeyCode::Char('a') | KeyCode::Char('o') => self.open_new(),
            KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char('l') => self.open_edit(),

            KeyCode::Char('x') => self.toggle_status(Status::Done),
            KeyCode::Char('s') => self.toggle_status(Status::Active),
            KeyCode::Char('c') => self.toggle_status(Status::Cancelled),
            KeyCode::Char('y') => {
                if let Some(text) = self.agent_instruction() {
                    return Effect::Copy(text);
                }
            }
            KeyCode::Char('p') => self.show_details = !self.show_details,
            KeyCode::Char('P') => self.mode = Mode::Prompt(TextArea::new(self.template(), false)),
            KeyCode::Char('E') => {
                self.mode = Mode::PromptExternal;
                return Effect::Editor {
                    text: self.template().to_string(),
                    single_line: false,
                };
            }

            KeyCode::Char('/') => self.mode = Mode::Search(LineEdit::new(&self.query)),
            KeyCode::Esc => self.query.clear(),
            _ => {}
        }
        self.clamp_cursor();
        Effect::None
    }

    fn search_key(&mut self, mut edit: LineEdit, key: KeyEvent) {
        match edit.handle(key) {
            Outcome::Submit => {}
            Outcome::Leave | Outcome::Abort => self.query.clear(),
            Outcome::Continue | Outcome::External => {
                self.query = edit.text();
                self.mode = Mode::Search(edit);
            }
        }
        self.clamp_cursor();
    }

    fn toggle_status(&mut self, status: Status) {
        let Some(&n) = self.visible().get(self.cursor) else {
            return;
        };
        let issue = &mut self.backlog.issues[n];
        issue.status = if issue.status == status {
            Status::Open
        } else {
            status
        };
        self.message = Some(format!("#{} {}", issue.id, issue.status.as_str()));
        self.save();
        self.clamp_cursor();
    }

    fn open_new(&mut self) {
        self.mode = Mode::Form(Box::new(Form {
            id: None,
            fields: Default::default(),
            orig: Default::default(),
            sel: TITLE,
            edit: None,
            confirm_discard: false,
            scroll: Default::default(),
        }));
    }

    fn open_edit(&mut self) {
        let Some(i) = self.selected() else { return };
        let fields = [
            i.title.clone(),
            i.description.clone(),
            i.done_when.clone(),
            i.notes.clone(),
            i.outcome.clone(),
        ];
        self.mode = Mode::Form(Box::new(Form {
            id: Some(i.id),
            orig: fields.clone(),
            fields,
            sel: TITLE,
            edit: None,
            confirm_discard: false,
            scroll: Default::default(),
        }));
    }

    fn form_key(&mut self, mut form: Box<Form>, key: KeyEvent) -> Effect {
        let mut effect = Effect::None;

        if form.confirm_discard {
            if key.code == KeyCode::Char('y') {
                return Effect::None;
            }
            form.confirm_discard = false;
        } else if let Some(edit) = form.edit.as_mut() {
            match edit.handle(key) {
                Outcome::Continue => {}
                Outcome::Submit => {
                    form.commit_edit();
                    if form.sel + 1 < FIELD_NAMES.len() {
                        form.sel += 1;
                        form.start_edit(false);
                    }
                }
                Outcome::Leave => form.commit_edit(),
                Outcome::Abort => form.edit = None,
                Outcome::External => {
                    let text = edit.text();
                    form.edit = None;
                    effect = form.external(text);
                }
            }
        } else {
            let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
            match key.code {
                KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                    form.sel = (form.sel + 1) % FIELD_NAMES.len()
                }
                KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                    form.sel = (form.sel + FIELD_NAMES.len() - 1) % FIELD_NAMES.len()
                }
                KeyCode::Char('i') | KeyCode::Char('a') | KeyCode::Enter => form.start_edit(false),
                KeyCode::Char('c') | KeyCode::Char('S') => form.start_edit(true),
                KeyCode::Char('e') if ctrl => form.scroll_field(form.sel, 1),
                KeyCode::Char('y') if ctrl => form.scroll_field(form.sel, -1),
                KeyCode::Char('E') | KeyCode::Char('e') => {
                    effect = form.external(form.fields[form.sel].clone())
                }
                KeyCode::Char('w') => return self.submit(form),
                KeyCode::Char('s') if ctrl => return self.submit(form),
                KeyCode::Esc | KeyCode::Char('q') => {
                    if !form.is_dirty() {
                        return Effect::None;
                    }
                    form.confirm_discard = true;
                }
                _ => {}
            }
        }
        self.mode = Mode::Form(form);
        effect
    }

    fn submit(&mut self, mut form: Box<Form>) -> Effect {
        for f in &mut form.fields {
            *f = f.trim().to_string();
        }
        // An issue without a title needs a description; the agent prompt asks
        // for a title from the description.
        if form.fields[TITLE].is_empty() && form.fields[DESCRIPTION].is_empty() {
            self.message = Some("Enter a title or a description".into());
            form.sel = TITLE;
            self.mode = Mode::Form(form);
            return Effect::None;
        }
        let [title, description, done_when, notes, outcome] = form.fields;
        let id = match form.id {
            Some(id) => {
                if let Some(i) = self.backlog.issues.iter_mut().find(|i| i.id == id) {
                    *i = Issue {
                        title,
                        description,
                        done_when,
                        notes,
                        outcome,
                        ..i.clone()
                    };
                }
                self.message = Some(format!("Saved #{id}"));
                id
            }
            None => {
                let id = self.backlog.next_id();
                let issue = Issue {
                    id,
                    title,
                    description,
                    done_when,
                    notes,
                    outcome,
                    status: Status::Open,
                };
                self.backlog.issues.push(issue);
                self.message = Some(format!("Added #{id}"));
                id
            }
        };
        self.save();
        self.select_id(id);
        Effect::None
    }
}
