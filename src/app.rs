//! Application state and key handling.

use ratatui::crossterm::event::{
    KeyCode, KeyEvent, KeyModifiers, MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::layout::{Margin, Position, Rect};
use ratatui_image::picker::Picker;
use std::cell::Cell;
use std::time::{Duration, Instant};

use crate::backlog::{Backlog, Issue, Status};
use crate::images::{self, Images};
use crate::input::{LineEdit, Outcome};
use crate::prompt;
use crate::settings::{self, Settings};
use crate::textarea::TextArea;
use crate::ui;

pub const FIELD_NAMES: [&str; 5] = ["Title", "Description", "Done when", "Notes", "Outcome"];
const TITLE: usize = 0;
const DESCRIPTION: usize = 1;
/// The outcome takes the right half of the form; the other fields are on the left.
const OUTCOME: usize = 4;
/// Minimum width of the main area for the list and the details side by side.
const WIDE: u16 = 90;
/// Minimum size of a pane along the split direction.
const MIN_PANE: u16 = 5;
/// Maximum time between the two clicks of a double click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);
/// Time that a status message stays on the screen.
const MESSAGE_TIME: Duration = Duration::from_secs(4);
/// Time in which a second paste of the same image is ignored. A terminal can
/// give one image both as `ctrl-v` and as a pasted file path.
const SAME_PASTE: Duration = Duration::from_secs(2);

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
    /// The settings popup, with the index of the selected setting.
    Settings(usize),
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
    /// The field on the left that `h` goes back to from the outcome.
    left: usize,
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

    /// Insert a link to the image at `path` at the cursor of the field in
    /// edit, with the next free image number of the form.
    fn insert_image_link(&mut self, path: &str) {
        let Some(edit) = self.edit.as_mut() else {
            return;
        };
        let current = edit.text();
        let others = self
            .fields
            .iter()
            .enumerate()
            .filter(|&(n, _)| n != self.sel)
            .map(|(_, t)| t.as_str());
        let n = images::last_number(others.chain([current.as_str()])) + 1;
        edit.paste_text(&format!("![Image #{n}]({path})"));
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

/// The width of the image previews from the settings; 0 if they are off.
fn image_width(settings: &Settings) -> u16 {
    if settings.is_on(settings::IMAGES) {
        settings.get(settings::IMAGE_WIDTH)
    } else {
        0
    }
}

pub struct App {
    pub backlog: Backlog,
    pub mode: Mode,
    /// Index into `visible()`.
    pub cursor: usize,
    pub query: String,
    pub message: Option<String>,
    /// The message on the screen and the time when it came.
    shown: Option<(String, Instant)>,
    page: usize,
    /// Fraction of the main area that the list takes.
    split: f32,
    /// The main area and the list area of the last draw.
    body: Rect,
    list_area: Rect,
    /// The index of the first visible row of the list.
    pub list_offset: usize,
    pub show_details: bool,
    pub settings: Settings,
    dragging: bool,
    last_click: Option<(Instant, u16, u16)>,
    /// The first key of a two-key list command (`gg`, `dd`).
    pending: Option<char>,
    pub images: Images,
    /// The path of the last pasted image and the time of the paste.
    last_image: Option<(String, Instant)>,
}

impl App {
    pub fn new(backlog: Backlog, picker: Picker, settings: Settings) -> App {
        let base = backlog.base_dir();
        App {
            backlog,
            mode: Mode::List,
            cursor: 0,
            query: String::new(),
            message: None,
            shown: None,
            page: 10,
            split: settings.get(settings::SPLIT) as f32 / 100.0,
            body: Rect::default(),
            list_area: Rect::default(),
            list_offset: 0,
            show_details: settings.is_on(settings::DETAILS),
            dragging: false,
            last_click: None,
            pending: None,
            images: Images::new(picker, base, image_width(&settings)),
            last_image: None,
            settings,
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

    /// Read the file again if another program changed it. Each change of the
    /// file starts with this, so that the save keeps the changes of the other
    /// program.
    pub fn sync(&mut self) {
        if self.backlog.changed_on_disk() {
            self.reload();
        }
    }

    /// Read the file again after another program changed it, and keep the
    /// cursor on the same issue.
    fn reload(&mut self) {
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

    /// Remove the status message after `MESSAGE_TIME`. A new message starts
    /// the time again.
    pub fn expire_message(&mut self) {
        let now = Instant::now();
        match (&self.message, &self.shown) {
            (None, _) => self.shown = None,
            (Some(msg), Some((old, since))) if msg == old => {
                if now.duration_since(*since) >= MESSAGE_TIME {
                    self.message = None;
                    self.shown = None;
                }
            }
            (Some(msg), _) => self.shown = Some((msg.clone(), now)),
        }
    }

    pub fn handle_mouse(&mut self, m: MouseEvent) {
        let double = self.double_click(m);
        match self.mode {
            Mode::List | Mode::Search(_) => self.list_mouse(m, double),
            Mode::Form(_) => self.form_mouse(m, double),
            Mode::Help if matches!(m.kind, MouseEventKind::Down(_)) => self.mode = Mode::List,
            Mode::Settings(_) if matches!(m.kind, MouseEventKind::Down(_)) => {
                self.mode = Mode::List
            }
            Mode::Prompt(ref mut edit) if m.kind == MouseEventKind::Down(MouseButton::Left) => {
                let text = ui::prompt_area(self.body).inner(Margin::new(1, 1));
                if text.contains(Position::new(m.column, m.row)) {
                    edit.click((m.column - text.x) as usize, (m.row - text.y) as usize);
                }
            }
            _ => {}
        }
    }

    /// Insert pasted text into the active editor. Without an editor, ignore
    /// it. The path of an image file inserts the image into a form field.
    pub fn handle_paste(&mut self, text: &str) {
        self.message = None;
        if let Mode::Form(form) = &self.mode
            && form.edit.is_some()
            && form.sel != TITLE
            && let Some((data, ext)) = images::from_pasted_path(text)
        {
            self.paste_image(&data, &ext);
            return;
        }
        match &mut self.mode {
            Mode::Search(edit) => {
                edit.paste(text);
                self.query = edit.text();
                self.clamp_cursor();
            }
            Mode::Form(form) if !form.confirm_discard => {
                if let Some(edit) = form.edit.as_mut() {
                    edit.paste_text(text);
                }
            }
            Mode::Prompt(edit) => edit.paste_text(text),
            _ => {}
        }
    }

    /// Insert the image from the clipboard into the form field in edit.
    fn paste_clipboard_image(&mut self) {
        match images::from_clipboard() {
            Some((data, ext)) => self.paste_image(&data, ext),
            None => self.message = Some("The clipboard holds no image".into()),
        }
    }

    /// Store the image `data` and insert a link to it into the form field
    /// in edit, except when the same image came within `SAME_PASTE`. The
    /// title takes no images.
    fn paste_image(&mut self, data: &[u8], ext: &str) {
        let Mode::Form(form) = &mut self.mode else {
            return;
        };
        if form.sel == TITLE {
            self.message = Some("The title cannot hold an image".into());
            return;
        }
        let path = match images::store(&self.backlog.base_dir(), data, ext) {
            Ok(path) => path,
            Err(e) => {
                self.message = Some(format!("Cannot store the image: {e}"));
                return;
            }
        };
        let repeated = self
            .last_image
            .as_ref()
            .is_some_and(|(last, t)| *last == path && t.elapsed() < SAME_PASTE);
        if !repeated {
            form.insert_image_link(&path);
        }
        self.last_image = Some((path, Instant::now()));
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
                if self.dragging {
                    let percent = (self.split * 100.0).round() as u16;
                    self.change_setting(settings::SPLIT, |s| s.set(settings::SPLIT, percent));
                }
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

    /// Indices into `backlog.issues` of the issues that match the search,
    /// without the done and cancelled issues if the settings hide them, in the
    /// order of the file or, if the settings say so, in the reverse order.
    pub fn visible(&self) -> Vec<usize> {
        let query = self.query.to_lowercase();
        let hide_closed = self.settings.is_on(settings::HIDE_CLOSED);
        let mut visible: Vec<usize> = self
            .backlog
            .issues
            .iter()
            .enumerate()
            .filter(|(_, i)| !(hide_closed && matches!(i.status, Status::Done | Status::Cancelled)))
            .filter(|(_, i)| {
                query.is_empty()
                    || [&i.title, &i.description, &i.done_when, &i.notes, &i.outcome]
                        .iter()
                        .any(|s| s.to_lowercase().contains(&query))
            })
            .map(|(n, _)| n)
            .collect();
        if self.settings.is_on(settings::NEWEST_FIRST) {
            visible.reverse();
        }
        visible
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
        self.sync();
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
            Mode::Settings(sel) => {
                self.settings_key(sel, key);
                Effect::None
            }
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
        self.sync();
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
            KeyCode::Char('S') => self.mode = Mode::Settings(0),

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

    /// Move in the settings popup, and change and save the selected setting:
    /// `space` toggles a toggle, and `h` / `l` change a number or a toggle.
    fn settings_key(&mut self, sel: usize, key: KeyEvent) {
        let last = settings::SETTINGS.len() - 1;
        let toggle = matches!(settings::SETTINGS[sel].kind, settings::Kind::Toggle);
        let dir = match key.code {
            KeyCode::Esc | KeyCode::Char('q' | 'S') => return,
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Tab => {
                self.mode = Mode::Settings((sel + 1).min(last));
                return;
            }
            KeyCode::Char('k') | KeyCode::Up | KeyCode::BackTab => {
                self.mode = Mode::Settings(sel.saturating_sub(1));
                return;
            }
            KeyCode::Enter | KeyCode::Char(' ' | 'x') if toggle => 1,
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Char('+') => 1,
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Char('-') => -1,
            _ => 0,
        };
        if dir != 0 {
            self.change_setting(sel, |s| s.change(sel, dir));
        }
        self.mode = Mode::Settings(sel);
    }

    /// Change the setting `n` with `f`, show an error if the file cannot be
    /// written, and apply the new value at once. The cursor stays on the same
    /// issue if it is still in the list.
    fn change_setting(&mut self, n: usize, f: impl FnOnce(&mut Settings) -> std::io::Result<()>) {
        let id = self.selected().map(|i| i.id);
        if let Err(e) = f(&mut self.settings) {
            self.message = Some(format!("Cannot save the settings: {e}"));
        }
        match n {
            settings::DETAILS => self.show_details = self.settings.is_on(n),
            settings::SPLIT => self.split = self.settings.get(n) as f32 / 100.0,
            settings::IMAGES | settings::IMAGE_WIDTH => {
                self.images.width = image_width(&self.settings)
            }
            _ => {}
        }
        match id {
            Some(id) => self.select_id(id),
            None => self.clamp_cursor(),
        }
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
            left: TITLE,
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
            left: TITLE,
        }));
    }

    fn form_key(&mut self, mut form: Box<Form>, key: KeyEvent) -> Effect {
        let mut effect = Effect::None;

        let ctrl_v =
            key.code == KeyCode::Char('v') && key.modifiers.contains(KeyModifiers::CONTROL);
        if form.confirm_discard {
            if key.code == KeyCode::Char('y') {
                return Effect::None;
            }
            form.confirm_discard = false;
        } else if ctrl_v && form.edit.is_some() {
            self.mode = Mode::Form(form);
            self.paste_clipboard_image();
            return Effect::None;
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
                KeyCode::Char('l') | KeyCode::Right if form.sel != OUTCOME => {
                    form.left = form.sel;
                    form.sel = OUTCOME;
                }
                KeyCode::Char('h') | KeyCode::Left if form.sel == OUTCOME => form.sel = form.left,
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn form_columns() {
        let dir = std::env::temp_dir().join(format!("backlog-columns-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".backlog.md");
        fs::write(&path, "# Backlog\n\n## #1 First\nstatus: open\n").unwrap();
        let mut app = App::new(
            Backlog::load(&path).unwrap(),
            Picker::halfblocks(),
            Settings::defaults(),
        );
        fs::remove_dir_all(&dir).unwrap();
        let press = |app: &mut App, c: char| {
            app.handle_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE));
        };
        let sel = |app: &App| match &app.mode {
            Mode::Form(form) => form.sel,
            _ => panic!("no form"),
        };

        press(&mut app, 'e');
        press(&mut app, 'j');
        assert_eq!(sel(&app), DESCRIPTION);
        press(&mut app, 'l');
        assert_eq!(sel(&app), OUTCOME);
        press(&mut app, 'l');
        assert_eq!(sel(&app), OUTCOME);
        press(&mut app, 'h');
        assert_eq!(sel(&app), DESCRIPTION);
        press(&mut app, 'h');
        assert_eq!(sel(&app), DESCRIPTION);
    }

    #[test]
    fn settings_popup() {
        let dir = std::env::temp_dir().join(format!("backlog-settings-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".backlog.md");
        fs::write(
            &path,
            "# Backlog\n\n## #1 First\nstatus: open\n\n## #2 Second\nstatus: done\n",
        )
        .unwrap();
        let mut app = App::new(
            Backlog::load(&path).unwrap(),
            Picker::halfblocks(),
            Settings::defaults(),
        );
        fs::remove_dir_all(&dir).unwrap();
        let press = |app: &mut App, code: KeyCode| {
            app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        };

        assert!(app.show_details);
        press(&mut app, KeyCode::Char('S'));
        assert!(matches!(app.mode, Mode::Settings(0)));
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.settings.is_on(settings::DETAILS));
        assert!(!app.show_details);
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.settings.get(settings::SPLIT), 50);
        assert_eq!(app.split, 0.5);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.visible().len(), 2);
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.visible().len(), 1);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(app.visible().len(), 2);
        assert_eq!(app.selected().map(|i| i.id), Some(1));
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.visible(), [1, 0]);
        assert_eq!(app.selected().map(|i| i.id), Some(1));
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(app.images.width, 0);
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.mode, Mode::List));
    }

    #[test]
    fn editor_keeps_changes_on_disk() {
        let dir = std::env::temp_dir().join(format!("backlog-test-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".backlog.md");
        fs::write(&path, "# Backlog\n\n## #1 First\nstatus: open\n").unwrap();
        let mut app = App::new(
            Backlog::load(&path).unwrap(),
            Picker::halfblocks(),
            Settings::defaults(),
        );
        app.mode = Mode::PromptExternal;

        // Another program changes the file while the editor runs.
        let text = "# Backlog\n\n## #1 First\nstatus: done\n\n## #2 Second\nstatus: open\n";
        fs::write(&path, text).unwrap();
        app.editor_done(Ok("Do #{id}.".into()));

        let saved = Backlog::load(&path).unwrap();
        fs::remove_dir_all(&dir).unwrap();
        assert_eq!(saved.prompt.as_deref(), Some("Do #{id}."));
        assert_eq!(saved.issues.len(), 2);
        assert_eq!(saved.issues[0].status, Status::Done);
    }
}
