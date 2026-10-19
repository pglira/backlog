//! Application state and key handling.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::Rect;

use crate::backlog::{Backlog, Issue, Status};
use crate::input::{LineEdit, Outcome};

pub const FIELD_NAMES: [&str; 4] = ["Title", "Description", "Done when", "Notes"];
const TITLE: usize = 0;
/// Minimum width of the main area for the list and the details side by side.
const WIDE: u16 = 90;
/// Minimum size of a pane along the split direction.
const MIN_PANE: u16 = 5;

pub enum Mode {
    List,
    Search(LineEdit),
    Form(Box<Form>),
    ConfirmDelete(u32),
    Help,
}

/// The editor for one issue. `id` is `None` for a new issue.
pub struct Form {
    pub id: Option<u32>,
    pub fields: [String; 4],
    orig: [String; 4],
    pub sel: usize,
    pub edit: Option<LineEdit>,
    pub confirm_discard: bool,
}

impl Form {
    fn is_dirty(&self) -> bool {
        self.fields != self.orig
    }

    /// Start inline editing of the selected field. A multi-line field opens
    /// in the external editor, because the inline editor holds one line.
    fn start_edit(&mut self, clear: bool) -> Effect {
        if clear {
            self.fields[self.sel].clear();
        }
        if self.fields[self.sel].contains('\n') {
            return self.external(self.fields[self.sel].clone());
        }
        self.edit = Some(LineEdit::new(&self.fields[self.sel]));
        Effect::None
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
    Editor { text: String, single_line: bool },
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
    /// The main area of the last draw.
    body: Rect,
    dragging: bool,
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
            dragging: false,
        }
    }

    /// Split the main area into the list and the details: side by side when
    /// it is wide enough, else one above the other.
    pub fn split_body(&mut self, body: Rect) -> (Rect, Rect) {
        self.body = body;
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

    /// Drag the border between the list and the details with the left button.
    pub fn handle_mouse(&mut self, m: MouseEvent) {
        if !matches!(self.mode, Mode::List | Mode::Search(_)) {
            return;
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
            MouseEventKind::Up(MouseButton::Left) => self.dragging = false,
            _ => {}
        }
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
                    || [&i.title, &i.description, &i.done_when, &i.notes]
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
            Mode::Help => Effect::None,
        }
    }

    /// Put the text from the external editor into the selected form field.
    pub fn editor_done(&mut self, result: std::io::Result<String>) {
        let Mode::Form(form) = &mut self.mode else {
            return;
        };
        match result {
            Ok(text) => form.fields[form.sel] = text,
            Err(e) => self.message = Some(format!("Editor failed: {e}")),
        }
    }

    fn list_key(&mut self, key: KeyEvent) -> Effect {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.visible().len();
        match key.code {
            KeyCode::Char('q') => return Effect::Quit,
            KeyCode::Char('c') if ctrl => return Effect::Quit,
            KeyCode::Char('?') => self.mode = Mode::Help,

            KeyCode::Char('j') | KeyCode::Down => self.cursor += 1,
            KeyCode::Char('k') | KeyCode::Up => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Char('g') | KeyCode::Home => self.cursor = 0,
            KeyCode::Char('G') | KeyCode::End => self.cursor = len,
            KeyCode::Char('d') if ctrl => self.cursor += self.page / 2,
            KeyCode::Char('u') if ctrl => self.cursor = self.cursor.saturating_sub(self.page / 2),
            KeyCode::PageDown => self.cursor += self.page,
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(self.page),

            KeyCode::Char('a') => self.open_new(),
            KeyCode::Enter | KeyCode::Char('e') | KeyCode::Char('l') => self.open_edit(),

            KeyCode::Char('x') => self.toggle_status(Status::Done),
            KeyCode::Char('s') => self.toggle_status(Status::Active),
            KeyCode::Char('c') => self.toggle_status(Status::Cancelled),
            KeyCode::Char('D') => {
                if let Some(i) = self.selected() {
                    self.mode = Mode::ConfirmDelete(i.id);
                }
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
        }));
    }

    fn open_edit(&mut self) {
        let Some(i) = self.selected() else { return };
        let fields = [
            i.title.clone(),
            i.description.clone(),
            i.done_when.clone(),
            i.notes.clone(),
        ];
        self.mode = Mode::Form(Box::new(Form {
            id: Some(i.id),
            orig: fields.clone(),
            fields,
            sel: TITLE,
            edit: None,
            confirm_discard: false,
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
                    form.fields[form.sel] = edit.text();
                    form.edit = None;
                    if form.sel + 1 < FIELD_NAMES.len() {
                        form.sel += 1;
                        effect = form.start_edit(false);
                    }
                }
                Outcome::Leave => {
                    form.fields[form.sel] = edit.text();
                    form.edit = None;
                }
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
                KeyCode::Char('i') | KeyCode::Char('a') | KeyCode::Enter => {
                    effect = form.start_edit(false)
                }
                KeyCode::Char('c') | KeyCode::Char('S') => effect = form.start_edit(true),
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
        if form.fields[TITLE].is_empty() {
            self.message = Some("The title must not be empty".into());
            form.sel = TITLE;
            self.mode = Mode::Form(form);
            return Effect::None;
        }
        let [title, description, done_when, notes] = form.fields;
        let id = match form.id {
            Some(id) => {
                if let Some(i) = self.backlog.issues.iter_mut().find(|i| i.id == id) {
                    *i = Issue {
                        title,
                        description,
                        done_when,
                        notes,
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
