//! A multi-line text editor with Vim modes. Insert mode also takes the
//! readline (Emacs-style) keys.

use std::cell::Cell;

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::Outcome;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum VimMode {
    Normal,
    Insert,
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
}

pub struct TextArea {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    pub mode: VimMode,
    /// Enter submits the text instead of a line break.
    single_line: bool,
    /// The yanked or deleted text, and whether it holds whole lines.
    register: (Vec<char>, bool),
    undo: Vec<Snapshot>,
    redo: Vec<Snapshot>,
    /// The first key of a two-key command (`dd`, `gg`, `r<char>`, ...).
    pending: Option<char>,
    ctrl_x: bool,
    /// The wrap width and the first visible row of the last view.
    width: Cell<usize>,
    scroll: Cell<usize>,
}

fn is_word(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

impl TextArea {
    /// Create an editor in insert mode with the cursor at the end of `text`.
    pub fn new(text: &str, single_line: bool) -> TextArea {
        let lines: Vec<Vec<char>> = text.split('\n').map(|l| l.chars().collect()).collect();
        let row = lines.len() - 1;
        let col = lines[row].len();
        TextArea {
            lines,
            row,
            col,
            mode: VimMode::Insert,
            single_line,
            register: (Vec::new(), false),
            undo: Vec::new(),
            redo: Vec::new(),
            pending: None,
            ctrl_x: false,
            width: Cell::new(1),
            scroll: Cell::new(0),
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
        match self.mode {
            VimMode::Insert => self.insert_key(key),
            VimMode::Normal => self.normal_key(key),
        }
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
        let visible = rows.into_iter().skip(scroll).take(height).collect();
        (visible, (cursor.0, cursor.1 - scroll))
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

    // --- insert mode --------------------------------------------------------

    fn insert_key(&mut self, key: KeyEvent) -> Outcome {
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
            KeyCode::Esc => {
                self.mode = VimMode::Normal;
                self.col = self.col.saturating_sub(1);
            }
            KeyCode::Enter if self.single_line => return Outcome::Submit,
            KeyCode::Enter => {
                let rest = self.lines[self.row].split_off(self.col);
                self.lines.insert(self.row + 1, rest);
                self.row += 1;
                self.col = 0;
            }

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
            KeyCode::Char('y') if ctrl => self.put_chars(&self.register.0.clone()),
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
            self.register = (self.lines[self.row].drain(from..to).collect(), false);
            self.col = from;
        }
    }

    // --- normal mode --------------------------------------------------------

    fn normal_key(&mut self, key: KeyEvent) -> Outcome {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if let Some(op) = self.pending.take() {
            self.operator(op, key.code);
            return Outcome::Continue;
        }
        let len = self.lines[self.row].len();
        match key.code {
            KeyCode::Esc => return Outcome::Leave,
            KeyCode::Enter if self.single_line => return Outcome::Submit,
            KeyCode::Char('r') if ctrl => self.redo(),

            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => {
                self.col = self.col.saturating_sub(1)
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Char(' ') => self.col += 1,
            KeyCode::Char('j') | KeyCode::Down | KeyCode::Enter => self.move_row(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_row(-1),
            KeyCode::Char('w') => self.word_forward(),
            KeyCode::Char('b') => self.word_backward(),
            KeyCode::Char('e') => self.col = self.word_end_exclusive().saturating_sub(1),
            KeyCode::Char('0') | KeyCode::Home => self.col = 0,
            KeyCode::Char('^') => self.col = self.first_non_blank(),
            KeyCode::Char('$') | KeyCode::End => self.col = len,
            KeyCode::Char('G') => self.row = self.lines.len() - 1,

            KeyCode::Char('i') => self.insert_at(self.col),
            KeyCode::Char('a') => self.insert_at((self.col + 1).min(len)),
            KeyCode::Char('I') => self.insert_at(self.first_non_blank()),
            KeyCode::Char('A') => self.insert_at(len),
            KeyCode::Char('o') => self.open_line(self.row + 1),
            KeyCode::Char('O') => self.open_line(self.row),

            KeyCode::Char('x') => self.delete_range(self.col, (self.col + 1).min(len)),
            KeyCode::Char('X') if self.col > 0 => self.delete_range(self.col - 1, self.col),
            KeyCode::Char('D') => self.delete_range(self.col, len),
            KeyCode::Char('C') => {
                self.delete_range(self.col, len);
                self.insert_at(len);
            }
            KeyCode::Char('s') => {
                self.delete_range(self.col, (self.col + 1).min(len));
                self.insert_at(self.col);
            }
            KeyCode::Char('S') => self.operator('c', KeyCode::Char('c')),
            KeyCode::Char('J') => self.join_lines(),
            KeyCode::Char('p') => self.paste(true),
            KeyCode::Char('P') => self.paste(false),
            KeyCode::Char('u') => self.undo(),
            KeyCode::Char(c @ ('d' | 'c' | 'y' | 'g' | 'r')) => self.pending = Some(c),
            _ => {}
        }
        self.clamp_col();
        Outcome::Continue
    }

    /// Run the operator `op` (`d`, `c`, `y`, `g` or `r`) with the key that follows it.
    fn operator(&mut self, op: char, code: KeyCode) {
        let KeyCode::Char(c) = code else { return };
        let len = self.lines[self.row].len();
        match (op, c) {
            ('g', 'g') => self.row = 0,
            ('r', c) if self.col < len => {
                self.checkpoint();
                self.lines[self.row][self.col] = c;
            }
            ('d', 'd') => {
                self.checkpoint();
                let line = self.lines.remove(self.row);
                self.register = (line, true);
                if self.lines.is_empty() {
                    self.lines.push(Vec::new());
                }
                self.row = self.row.min(self.lines.len() - 1);
                self.col = self.first_non_blank();
            }
            ('c', 'c') => {
                self.checkpoint();
                self.register = (std::mem::take(&mut self.lines[self.row]), true);
                self.col = 0;
                self.mode = VimMode::Insert;
            }
            ('y', 'y') => self.register = (self.lines[self.row].clone(), true),
            (op @ ('d' | 'c' | 'y'), motion) => {
                // `cw` changes to the end of the word, as `ce` does.
                let target = match motion {
                    'w' if op == 'c' => self.word_end_exclusive(),
                    'w' => self.next_word_start(),
                    'e' => self.word_end_exclusive(),
                    'b' => self.word_back(),
                    '$' => len,
                    '0' => 0,
                    '^' => self.first_non_blank(),
                    'h' => self.col.saturating_sub(1),
                    'l' => (self.col + 1).min(len),
                    _ => return,
                };
                let (from, to) = (self.col.min(target), self.col.max(target));
                match op {
                    'y' => self.register = (self.lines[self.row][from..to].to_vec(), false),
                    'd' => self.delete_range(from, to),
                    _ => {
                        self.delete_range(from, to);
                        self.mode = VimMode::Insert;
                        self.col = from;
                        return;
                    }
                }
            }
            _ => {}
        }
        self.clamp_col();
    }

    fn insert_at(&mut self, col: usize) {
        self.checkpoint();
        self.col = col;
        self.mode = VimMode::Insert;
    }

    fn open_line(&mut self, at: usize) {
        if self.single_line {
            return;
        }
        self.checkpoint();
        self.lines.insert(at, Vec::new());
        self.row = at;
        self.col = 0;
        self.mode = VimMode::Insert;
    }

    fn delete_range(&mut self, from: usize, to: usize) {
        if from < to {
            self.checkpoint();
            self.register = (self.lines[self.row].drain(from..to).collect(), false);
            self.col = from;
        }
    }

    fn join_lines(&mut self) {
        if self.row + 1 >= self.lines.len() {
            return;
        }
        self.checkpoint();
        let next = self.lines.remove(self.row + 1);
        let line = &mut self.lines[self.row];
        while line.last() == Some(&' ') {
            line.pop();
        }
        self.col = line.len();
        let next: Vec<char> = next.into_iter().skip_while(|c| *c == ' ').collect();
        if !line.is_empty() && !next.is_empty() {
            line.push(' ');
        }
        line.extend(next);
    }

    /// Paste the register after (`after`) or before the cursor.
    fn paste(&mut self, after: bool) {
        let (text, linewise) = self.register.clone();
        if text.is_empty() && !linewise {
            return;
        }
        self.checkpoint();
        if linewise {
            if self.single_line {
                self.lines[self.row].extend(text);
                return;
            }
            let at = if after { self.row + 1 } else { self.row };
            self.lines.insert(at, text);
            self.row = at;
            self.col = self.first_non_blank();
        } else {
            if after && !self.lines[self.row].is_empty() {
                self.col += 1;
            }
            self.put_chars(&text);
            self.col = self.col.saturating_sub(1);
        }
    }

    // --- undo ---------------------------------------------------------------

    fn snapshot(&self) -> Snapshot {
        Snapshot {
            lines: self.lines.clone(),
            row: self.row,
            col: self.col,
        }
    }

    fn restore(&mut self, s: Snapshot) {
        self.lines = s.lines;
        self.row = s.row;
        self.col = s.col;
        self.clamp_col();
    }

    /// Save the state before a change, as one undo step.
    fn checkpoint(&mut self) {
        self.undo.push(self.snapshot());
        self.redo.clear();
    }

    fn undo(&mut self) {
        if let Some(s) = self.undo.pop() {
            self.redo.push(self.snapshot());
            self.restore(s);
        }
    }

    fn redo(&mut self) {
        if let Some(s) = self.redo.pop() {
            self.undo.push(self.snapshot());
            self.restore(s);
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

    /// Keep the cursor on a character in normal mode, or at most one past the
    /// last character in insert mode.
    fn clamp_col(&mut self) {
        self.row = self.row.min(self.lines.len() - 1);
        let len = self.lines[self.row].len();
        let max = match self.mode {
            VimMode::Normal => len.saturating_sub(1),
            VimMode::Insert => len,
        };
        self.col = self.col.min(max);
    }

    fn first_non_blank(&self) -> usize {
        let line = &self.lines[self.row];
        line.iter().position(|c| !c.is_whitespace()).unwrap_or(0)
    }

    /// The start of the next word in the line, or the line end.
    fn next_word_start(&self) -> usize {
        let line = &self.lines[self.row];
        let mut i = self.col;
        if i < line.len() && !line[i].is_whitespace() {
            let word = is_word(line[i]);
            while i < line.len() && !line[i].is_whitespace() && is_word(line[i]) == word {
                i += 1;
            }
        }
        while i < line.len() && line[i].is_whitespace() {
            i += 1;
        }
        i
    }

    /// The position just after the end of the word at or after the cursor.
    fn word_end_exclusive(&self) -> usize {
        let line = &self.lines[self.row];
        let mut i = (self.col + 1).min(line.len());
        if self.mode == VimMode::Insert {
            i = self.col;
        }
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

    /// `w`: go to the next word start, across lines.
    fn word_forward(&mut self) {
        let next = self.next_word_start();
        if next < self.lines[self.row].len() || self.row + 1 >= self.lines.len() {
            self.col = next;
        } else {
            self.row += 1;
            self.col = self.first_non_blank();
        }
    }

    /// `b`: go to the previous word start, across lines.
    fn word_backward(&mut self) {
        let back = self.word_back();
        if back < self.col || self.row == 0 {
            self.col = back;
        } else {
            self.row -= 1;
            self.col = self.lines[self.row].len();
            self.col = self.word_back();
        }
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
    fn insert_and_normal_mode() {
        let mut t = TextArea::new("", false);
        keys(&mut t, "one two\nthree\x1b");
        assert_eq!(t.text(), "one two\nthree");
        keys(&mut t, "gg0wcwTWO\x1b");
        assert_eq!(t.text(), "one TWO\nthree");
        keys(&mut t, "jddu");
        assert_eq!(t.text(), "one TWO\nthree");
        keys(&mut t, "ggddp");
        assert_eq!(t.text(), "three\none TWO");
        keys(&mut t, "ggJ");
        assert_eq!(t.text(), "three one TWO");
        keys(&mut t, "0dwx");
        assert_eq!(t.text(), "ne TWO");
        keys(&mut t, "$rX");
        assert_eq!(t.text(), "ne TWX");
    }

    #[test]
    fn single_line_submits_on_enter() {
        let mut t = TextArea::new("title", true);
        let out = t.handle(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(matches!(out, Outcome::Submit));
        assert_eq!(t.text(), "title");
    }

    #[test]
    fn view_wraps_long_lines() {
        let t = TextArea::new("abcdefg\nhi", false);
        let (rows, cursor) = t.view(3, 10);
        assert_eq!(rows, ["abc", "def", "g", "hi"]);
        assert_eq!(cursor, (2, 3));
    }
}
