//! A multi-line text editor with readline (Emacs-style) key bindings.

use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::Outcome;

pub struct TextArea {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    /// Enter submits the text instead of a line break.
    single_line: bool,
    /// The last cut text, for `ctrl-y`.
    kill: Vec<char>,
    ctrl_x: bool,
    /// The wrap width, the first visible row and the number of rows of the
    /// last view.
    width: Cell<usize>,
    scroll: Cell<usize>,
    total: Cell<usize>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl TextArea {
    /// Create an editor with the cursor at the end of `text`.
    pub fn new(text: &str, single_line: bool) -> TextArea {
        let lines: Vec<Vec<char>> = text.split('\n').map(|l| l.chars().collect()).collect();
        let row = lines.len() - 1;
        let col = lines[row].len();
        TextArea {
            lines,
            row,
            col,
            single_line,
            kill: Vec::new(),
            ctrl_x: false,
            width: Cell::new(1),
            scroll: Cell::new(0),
            total: Cell::new(0),
        }
    }

    pub fn text(&self) -> String {
        let lines: Vec<String> = self.lines.iter().map(|l| l.iter().collect()).collect();
        lines.join("\n")
    }

    pub fn handle(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if std::mem::take(&mut self.ctrl_x) && ctrl && key.code == KeyCode::Char('e') {
            return Outcome::External;
        }
        match key.code {
            KeyCode::Char('c') if ctrl => return Outcome::Abort,
            KeyCode::Char('x') if ctrl => {
                self.ctrl_x = true;
                return Outcome::Continue;
            }
            _ => {}
        }
        self.edit_key(key)
    }

    /// Insert pasted text at the cursor. A single-line editor gets the line
    /// breaks as spaces.
    pub fn paste_text(&mut self, text: &str) {
        let text = text.replace("\r\n", "\n").replace('\r', "\n");
        let text = if self.single_line {
            text.trim_end_matches('\n').replace('\n', " ")
        } else {
            text
        };
        if text.is_empty() {
            return;
        }
        let chars: Vec<char> = text.chars().collect();
        self.put_chars(&chars);
    }

    // --- view ---------------------------------------------------------------

    /// Return the rows visible in a `width` x `height` area, with the lines
    /// wrapped at `width` characters, and the cursor position in the area.
    pub fn view(&self, width: usize, height: usize) -> (Vec<String>, (usize, usize)) {
        let width = width.max(1);
        let height = height.max(1);
        self.width.set(width);

        let mut rows = Vec::new();
        let mut cursor = (0, 0);
        for (n, line) in self.lines.iter().enumerate() {
            if n == self.row {
                cursor = (self.col % width, rows.len() + self.col / width);
            }
            // One extra character of room keeps the cursor at the line end visible.
            for chunk in 0..line.len() / width + 1 {
                let end = ((chunk + 1) * width).min(line.len());
                rows.push(line[chunk * width..end].iter().collect());
            }
        }

        let mut scroll = self.scroll.get();
        if cursor.1 < scroll {
            scroll = cursor.1;
        } else if cursor.1 >= scroll + height {
            scroll = cursor.1 + 1 - height;
        }
        self.scroll.set(scroll);
        self.total.set(rows.len());
        let visible = rows.into_iter().skip(scroll).take(height).collect();
        (visible, (cursor.0, cursor.1 - scroll))
    }

    /// The number of rows and the first visible row of the last view.
    pub fn scroll_info(&self) -> (usize, usize) {
        (self.total.get(), self.scroll.get())
    }

    /// Move the cursor `dir` lines down (or up, when negative); the view follows.
    pub fn scroll_by(&mut self, dir: isize) {
        self.move_row(dir);
    }

    /// Move the cursor to the cell (`x`, `y`) of the last view.
    pub fn click(&mut self, x: usize, y: usize) {
        let width = self.width.get();
        let mut target = self.scroll.get() + y;
        for (n, line) in self.lines.iter().enumerate() {
            let rows = line.len() / width + 1;
            if target < rows {
                self.row = n;
                self.col = target * width + x;
                self.clamp_col();
                return;
            }
            target -= rows;
        }
        self.row = self.lines.len() - 1;
        self.col = self.lines[self.row].len();
        self.clamp_col();
    }

    // --- editing ------------------------------------------------------------

    fn edit_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        // Map the Ctrl aliases of the basic movement and delete keys.
        let code = match key.code {
            KeyCode::Char('b') if ctrl => KeyCode::Left,
            KeyCode::Char('f') if ctrl => KeyCode::Right,
            KeyCode::Char('a') if ctrl => KeyCode::Home,
            KeyCode::Char('e') if ctrl => KeyCode::End,
            KeyCode::Char('h') if ctrl => KeyCode::Backspace,
            KeyCode::Char('d') if ctrl => KeyCode::Delete,
            KeyCode::Char('p') if ctrl => KeyCode::Up,
            KeyCode::Char('n') if ctrl => KeyCode::Down,
            code => code,
        };
        let len = self.lines[self.row].len();
        match code {
            KeyCode::Esc => return Outcome::Leave,
            KeyCode::Enter if self.single_line => return Outcome::Submit,
            KeyCode::Enter => self.put_chars(&['\n']),

            KeyCode::Left if ctrl => self.col = self.word_back(),
            KeyCode::Right if ctrl => self.col = self.word_end_exclusive(),
            KeyCode::Char('b') if alt => self.col = self.word_back(),
            KeyCode::Char('f') if alt => self.col = self.word_end_exclusive(),
            KeyCode::Left => self.col = self.col.saturating_sub(1),
            KeyCode::Right => self.col = (self.col + 1).min(len),
            KeyCode::Up => self.move_row(-1),
            KeyCode::Down => self.move_row(1),
            KeyCode::Home => self.col = 0,
            KeyCode::End => self.col = len,

            KeyCode::Backspace if alt => self.cut_in_line(self.word_back(), self.col),
            KeyCode::Char('w') if ctrl => self.cut_in_line(self.word_back(), self.col),
            KeyCode::Char('d') if alt => self.cut_in_line(self.col, self.word_end_exclusive()),
            KeyCode::Char('u') if ctrl => self.cut_in_line(0, self.col),
            KeyCode::Char('k') if ctrl => self.cut_in_line(self.col, len),
            KeyCode::Char('y') if ctrl => self.put_chars(&self.kill.clone()),
            KeyCode::Backspace if self.col > 0 => {
                self.col -= 1;
                self.lines[self.row].remove(self.col);
            }
            KeyCode::Backspace if self.row > 0 => {
                let line = self.lines.remove(self.row);
                self.row -= 1;
                self.col = self.lines[self.row].len();
                self.lines[self.row].extend(line);
            }
            KeyCode::Delete if self.col < len => {
                self.lines[self.row].remove(self.col);
            }
            KeyCode::Delete if self.row + 1 < self.lines.len() => {
                let next = self.lines.remove(self.row + 1);
                self.lines[self.row].extend(next);
            }

            KeyCode::Char(c) if !ctrl && !alt => self.put_chars(&[c]),
            _ => {}
        }
        Outcome::Continue
    }

    /// Insert characters at the cursor; a '\n' starts a new line.
    fn put_chars(&mut self, chars: &[char]) {
        for &c in chars {
            if c == '\n' {
                let rest = self.lines[self.row].split_off(self.col);
                self.lines.insert(self.row + 1, rest);
                self.row += 1;
                self.col = 0;
            } else {
                self.lines[self.row].insert(self.col, c);
                self.col += 1;
            }
        }
    }

    fn cut_in_line(&mut self, from: usize, to: usize) {
        if from < to {
            self.kill = self.lines[self.row].drain(from..to).collect();
            self.col = from;
        }
    }

    // --- motions ------------------------------------------------------------

    fn move_row(&mut self, dir: isize) {
        if let Some(row) = self.row.checked_add_signed(dir)
            && row < self.lines.len()
        {
            self.row = row;
        }
        self.clamp_col();
    }

    /// Keep the cursor at most one past the last character of its line.
    fn clamp_col(&mut self) {
        self.row = self.row.min(self.lines.len() - 1);
        self.col = self.col.min(self.lines[self.row].len());
    }

    /// The position just after the end of the word at or after the cursor.
    fn word_end_exclusive(&self) -> usize {
        let line = &self.lines[self.row];
        let mut i = self.col;
        while i < line.len() && line[i].is_whitespace() {
            i += 1;
        }
        if i < line.len() {
            let word = is_word(line[i]);
            while i < line.len() && !line[i].is_whitespace() && is_word(line[i]) == word {
                i += 1;
            }
        }
        i
    }

    /// The start of the word before the cursor.
    fn word_back(&self) -> usize {
        let line = &self.lines[self.row];
        let mut i = self.col.min(line.len());
        while i > 0 && line[i - 1].is_whitespace() {
            i -= 1;
        }
        if i > 0 {
            let word = is_word(line[i - 1]);
            while i > 0 && !line[i - 1].is_whitespace() && is_word(line[i - 1]) == word {
                i -= 1;
            }
        }
        i
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(t: &mut TextArea, s: &str) {
        for c in s.chars() {
            let code = match c {
                '\x1b' => KeyCode::Esc,
                '\n' => KeyCode::Enter,
                c => KeyCode::Char(c),
            };
            t.handle(KeyEvent::new(code, KeyModifiers::NONE));
        }
    }

    #[test]
    fn readline_keys() {
        let mut t = TextArea::new("", false);
        keys(&mut t, "one two\nthree");
        assert_eq!(t.text(), "one two\nthree");
        t.handle(KeyEvent::new(KeyCode::Char('w'), KeyModifiers::CONTROL));
        assert_eq!(t.text(), "one two\n");
        t.handle(KeyEvent::new(KeyCode::Char('y'), KeyModifiers::CONTROL));
        assert_eq!(t.text(), "one two\nthree");
        let out = t.handle(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(matches!(out, Outcome::Leave));
    }

    #[test]
    fn single_line_submits_on_enter() {
        let mut t = TextArea::new("title", true);
        let out = t.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(out, Outcome::Submit));
        assert_eq!(t.text(), "title");
    }

    #[test]
    fn paste_keeps_line_breaks() {
        let mut t = TextArea::new("ab", false);
        t.paste_text("one\r\ntwo\n");
        assert_eq!(t.text(), "abone\ntwo\n");

        let mut t = TextArea::new("", true);
        t.paste_text("one\ntwo\n");
        assert_eq!(t.text(), "one two");
    }

    #[test]
    fn view_wraps_long_lines() {
        let t = TextArea::new("abcdefg\nhi", false);
        let (rows, cursor) = t.view(3, 10);
        assert_eq!(rows, ["abc", "def", "g", "hi"]);
        assert_eq!(cursor, (2, 3));
    }
}
