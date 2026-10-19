//! A single-line text editor with readline (Emacs-style) key bindings.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

pub enum Outcome {
    /// The key changed the text or the cursor, or was ignored.
    Continue,
    /// Enter: accept the text.
    Submit,
    /// Esc: leave the editor and keep the text.
    Leave,
    /// Ctrl-C: leave the editor and discard the changes.
    Abort,
    /// Ctrl-X Ctrl-E: continue editing the text in $EDITOR.
    External,
}

#[derive(Default)]
pub struct LineEdit {
    buf: Vec<char>,
    cursor: usize,
    kill: Vec<char>,
    ctrl_x: bool,
}

impl LineEdit {
    pub fn new(text: &str) -> LineEdit {
        let buf: Vec<char> = text.chars().collect();
        let cursor = buf.len();
        LineEdit {
            buf,
            cursor,
            ..Default::default()
        }
    }

    pub fn text(&self) -> String {
        self.buf.iter().collect()
    }

    pub fn handle(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);

        if std::mem::take(&mut self.ctrl_x) && ctrl && key.code == KeyCode::Char('e') {
            return Outcome::External;
        }

        // Map the Ctrl aliases of the basic movement and delete keys.
        let code = match key.code {
            KeyCode::Char('b') if ctrl => KeyCode::Left,
            KeyCode::Char('f') if ctrl => KeyCode::Right,
            KeyCode::Char('a') if ctrl => KeyCode::Home,
            KeyCode::Char('e') if ctrl => KeyCode::End,
            KeyCode::Char('h') if ctrl => KeyCode::Backspace,
            KeyCode::Char('d') if ctrl => KeyCode::Delete,
            code => code,
        };

        match code {
            KeyCode::Enter => return Outcome::Submit,
            KeyCode::Esc => return Outcome::Leave,
            KeyCode::Char('c') if ctrl => return Outcome::Abort,
            KeyCode::Char('x') if ctrl => self.ctrl_x = true,

            KeyCode::Left if ctrl => self.cursor = self.word_left(),
            KeyCode::Right if ctrl => self.cursor = self.word_right(),
            KeyCode::Char('b') if alt => self.cursor = self.word_left(),
            KeyCode::Char('f') if alt => self.cursor = self.word_right(),
            KeyCode::Left => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::Right => self.cursor = (self.cursor + 1).min(self.buf.len()),
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.buf.len(),

            KeyCode::Backspace if alt => self.cut(self.word_left(), self.cursor),
            KeyCode::Char('w') if ctrl => self.cut(self.word_left(), self.cursor),
            KeyCode::Char('d') if alt => self.cut(self.cursor, self.word_right()),
            KeyCode::Char('u') if ctrl => self.cut(0, self.cursor),
            KeyCode::Char('k') if ctrl => self.cut(self.cursor, self.buf.len()),
            KeyCode::Char('y') if ctrl => self.insert(&self.kill.clone()),
            KeyCode::Backspace if self.cursor > 0 => {
                self.cursor -= 1;
                self.buf.remove(self.cursor);
            }
            KeyCode::Delete if self.cursor < self.buf.len() => {
                self.buf.remove(self.cursor);
            }
            KeyCode::Char('t') if ctrl => self.transpose(),

            KeyCode::Char(c) if !ctrl && !alt => self.insert(&[c]),
            _ => {}
        }
        Outcome::Continue
    }

    /// Return the visible part of the text for a field of `width` columns,
    /// and the cursor column inside it.
    pub fn view(&self, width: usize) -> (String, usize) {
        let width = width.max(1);
        let start = (self.cursor + 1).saturating_sub(width);
        let text = self.buf[start..].iter().take(width).collect();
        (text, self.cursor - start)
    }

    fn insert(&mut self, chars: &[char]) {
        self.buf
            .splice(self.cursor..self.cursor, chars.iter().copied());
        self.cursor += chars.len();
    }

    fn cut(&mut self, from: usize, to: usize) {
        if from < to {
            self.kill = self.buf.drain(from..to).collect();
            self.cursor = from;
        }
    }

    fn transpose(&mut self) {
        if self.buf.len() < 2 || self.cursor == 0 {
            return;
        }
        let at = self.cursor.min(self.buf.len() - 1);
        self.buf.swap(at - 1, at);
        self.cursor = at + 1;
    }

    fn word_left(&self) -> usize {
        let mut i = self.cursor;
        while i > 0 && !self.buf[i - 1].is_alphanumeric() {
            i -= 1;
        }
        while i > 0 && self.buf[i - 1].is_alphanumeric() {
            i -= 1;
        }
        i
    }

    fn word_right(&self) -> usize {
        let mut i = self.cursor;
        while i < self.buf.len() && !self.buf[i].is_alphanumeric() {
            i += 1;
        }
        while i < self.buf.len() && self.buf[i].is_alphanumeric() {
            i += 1;
        }
        i
    }
}
