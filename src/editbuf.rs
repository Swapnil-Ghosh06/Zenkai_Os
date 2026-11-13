//! The text buffer behind the full-screen `edit` command: lines, a cursor that moves up and down and keeps its
//! column where it can, scrolling, and every edit (insert, split, join, cut). No screen and no keyboard in here,
//! so it is host-tested; `shell.rs` feeds it keys and `fb.rs` paints what it says.
//!
//! Everything is bounds-checked: a key that would break a limit is refused with a reason and changes nothing.

use alloc::vec;
use alloc::vec::Vec;

/// Longest line, in bytes.
pub const MAX_LINE_LEN: usize = 1024;
/// Most lines in one file.
pub const MAX_LINES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refused {
    /// A line may hold at most [`MAX_LINE_LEN`] characters.
    LineTooLong,
    /// A file may hold at most [`MAX_LINES`] lines.
    TooManyLines,
    /// The text would no longer fit in the RAM disk.
    TooBig,
    /// Only printable ASCII is typed in.
    NotPrintable,
}

impl Refused {
    pub fn message(self) -> &'static str {
        match self {
            Refused::LineTooLong => "line is full (1024 characters)",
            Refused::TooManyLines => "too many lines (4096)",
            Refused::TooBig => "the RAM disk has no room for more text",
            Refused::NotPrintable => "only letters, digits and symbols can be typed",
        }
    }
}

pub struct Doc {
    lines: Vec<Vec<u8>>,
    /// Cursor line and column (a byte index into the line, at most its length).
    row: usize,
    col: usize,
    /// The column Up and Down try to return to, so a short line in between does not lose it.
    want_col: usize,
    /// First visible line and first visible column.
    top: usize,
    left: usize,
    dirty: bool,
    /// The most bytes the whole text may take when saved.
    max_bytes: usize,
}

impl Doc {
    /// Opens `text` (what the file holds; empty for a new file). Lines end at `\n`; a missing last newline is fine.
    pub fn new(text: &[u8], max_bytes: usize) -> Doc {
        let body = text.strip_suffix(b"\n").unwrap_or(text);
        let mut lines: Vec<Vec<u8>> = if text.is_empty() { vec![Vec::new()] } else { body.split(|&b| b == b'\n').map(<[u8]>::to_vec).collect() };
        // A file that already breaks the limits is still shown; its long lines are cut, not lost silently:
        // the caller sees `truncated()` and warns before saving.
        lines.truncate(MAX_LINES);
        for l in &mut lines {
            l.truncate(MAX_LINE_LEN);
        }
        Doc { lines, row: 0, col: 0, want_col: 0, top: 0, left: 0, dirty: false, max_bytes }
    }

    /// The text to save: every line followed by `\n`; an empty buffer saves as an empty file.
    pub fn text(&self) -> Vec<u8> {
        if self.lines.len() == 1 && self.lines[0].is_empty() {
            return Vec::new();
        }
        let mut out = Vec::with_capacity(self.size());
        for l in &self.lines {
            out.extend_from_slice(l);
            out.push(b'\n');
        }
        out
    }

    /// Bytes the saved text takes.
    pub fn size(&self) -> usize {
        if self.lines.len() == 1 && self.lines[0].is_empty() {
            0
        } else {
            self.lines.iter().map(|l| l.len() + 1).sum()
        }
    }

    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Call after a successful save.
    pub fn mark_saved(&mut self) {
        self.dirty = false;
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Cursor position as (line, column), both counted from 0.
    pub fn cursor(&self) -> (usize, usize) {
        (self.row, self.col)
    }

    pub fn line(&self, i: usize) -> &[u8] {
        self.lines.get(i).map_or(&[], Vec::as_slice)
    }

    fn cur(&mut self) -> &mut Vec<u8> {
        &mut self.lines[self.row]
    }

    fn set_col(&mut self, col: usize) {
        self.col = col;
        self.want_col = col;
    }

    // ---- moving ----

    pub fn left(&mut self) {
        if self.col > 0 {
            self.set_col(self.col - 1);
        } else if self.row > 0 {
            self.row -= 1;
            self.set_col(self.lines[self.row].len());
        }
    }

    pub fn right(&mut self) {
        if self.col < self.lines[self.row].len() {
            self.set_col(self.col + 1);
        } else if self.row + 1 < self.lines.len() {
            self.row += 1;
            self.set_col(0);
        }
    }

    fn go_to_row(&mut self, row: usize) {
        self.row = row.min(self.lines.len() - 1);
        self.col = self.want_col.min(self.lines[self.row].len());
    }

    pub fn up(&mut self) {
        self.go_to_row(self.row.saturating_sub(1));
    }

    pub fn down(&mut self) {
        self.go_to_row(self.row + 1);
    }

    pub fn page_up(&mut self, rows: usize) {
        self.go_to_row(self.row.saturating_sub(rows.saturating_sub(1).max(1)));
    }

    pub fn page_down(&mut self, rows: usize) {
        self.go_to_row(self.row.saturating_add(rows.saturating_sub(1).max(1)));
    }

    pub fn home(&mut self) {
        self.set_col(0);
    }

    pub fn end(&mut self) {
        self.set_col(self.lines[self.row].len());
    }

    pub fn top_of_file(&mut self) {
        self.row = 0;
        self.set_col(0);
    }

    pub fn bottom_of_file(&mut self) {
        self.row = self.lines.len() - 1;
        self.set_col(self.lines[self.row].len());
    }

    // ---- changing ----

    fn room_for(&self, extra: usize) -> Result<(), Refused> {
        // An empty single line saves as nothing, so the first character costs two bytes (it and the newline).
        let now = self.size();
        let after = if now == 0 { extra + 1 } else { now + extra };
        if after > self.max_bytes {
            Err(Refused::TooBig)
        } else {
            Ok(())
        }
    }

    pub fn insert(&mut self, c: u8) -> Result<(), Refused> {
        if !(0x20..0x7f).contains(&c) {
            return Err(Refused::NotPrintable);
        }
        if self.lines[self.row].len() >= MAX_LINE_LEN {
            return Err(Refused::LineTooLong);
        }
        self.room_for(1)?;
        let col = self.col;
        self.cur().insert(col, c);
        self.set_col(col + 1);
        self.dirty = true;
        Ok(())
    }

    /// Four spaces (or fewer when the line is nearly full).
    pub fn tab(&mut self) -> Result<(), Refused> {
        for _ in 0..4 {
            self.insert(b' ')?;
        }
        Ok(())
    }

    /// Enter: the text after the cursor moves to a new line below.
    pub fn enter(&mut self) -> Result<(), Refused> {
        if self.lines.len() >= MAX_LINES {
            return Err(Refused::TooManyLines);
        }
        self.room_for(1)?;
        let col = self.col;
        let rest = self.cur().split_off(col);
        self.lines.insert(self.row + 1, rest);
        self.row += 1;
        self.set_col(0);
        self.dirty = true;
        Ok(())
    }

    /// Backspace: removes the character before the cursor, or joins this line onto the one above.
    pub fn backspace(&mut self) -> Result<(), Refused> {
        if self.col > 0 {
            let col = self.col;
            self.cur().remove(col - 1);
            self.set_col(col - 1);
            self.dirty = true;
        } else if self.row > 0 {
            if self.lines[self.row - 1].len() + self.lines[self.row].len() > MAX_LINE_LEN {
                return Err(Refused::LineTooLong);
            }
            let tail = self.lines.remove(self.row);
            self.row -= 1;
            let at = self.lines[self.row].len();
            self.lines[self.row].extend_from_slice(&tail);
            self.set_col(at);
            self.dirty = true;
        }
        Ok(())
    }

    /// Delete: removes the character under the cursor, or joins the line below onto this one.
    pub fn delete(&mut self) -> Result<(), Refused> {
        if self.col < self.lines[self.row].len() {
            let col = self.col;
            self.cur().remove(col);
            self.dirty = true;
        } else if self.row + 1 < self.lines.len() {
            if self.lines[self.row].len() + self.lines[self.row + 1].len() > MAX_LINE_LEN {
                return Err(Refused::LineTooLong);
            }
            let tail = self.lines.remove(self.row + 1);
            self.lines[self.row].extend_from_slice(&tail);
            self.dirty = true;
        }
        Ok(())
    }

    /// Ctrl+K: cuts the whole line (the file keeps at least one, empty, line).
    pub fn cut_line(&mut self) {
        if self.lines.len() == 1 {
            if !self.lines[0].is_empty() {
                self.lines[0].clear();
                self.dirty = true;
            }
        } else {
            self.lines.remove(self.row);
            self.dirty = true;
        }
        self.row = self.row.min(self.lines.len() - 1);
        self.col = self.want_col.min(self.lines[self.row].len());
    }

    // ---- showing ----

    /// Scrolls so the cursor is inside a window of `rows` lines by `cols` columns. Call before drawing.
    pub fn fit(&mut self, rows: usize, cols: usize) {
        let rows = rows.max(1);
        let cols = cols.max(2);
        if self.row < self.top {
            self.top = self.row;
        } else if self.row >= self.top + rows {
            self.top = self.row + 1 - rows;
        }
        if self.col < cols {
            self.left = 0;
        } else if self.col < self.left {
            self.left = self.col;
        } else if self.col >= self.left + cols {
            self.left = self.col + 1 - cols;
        }
    }

    /// What shows on window row `r` (0-based): the visible part of that line, with control characters as `?`.
    pub fn window_row(&self, r: usize, cols: usize) -> Vec<u8> {
        let Some(line) = self.lines.get(self.top + r) else { return Vec::new() };
        line.iter().skip(self.left).take(cols).map(|&b| if (0x20..0x7f).contains(&b) { b } else { b'?' }).collect()
    }

    /// Where the cursor is inside the window: (row, column).
    pub fn window_cursor(&self) -> (usize, usize) {
        (self.row - self.top, self.col - self.left)
    }

    /// True when `new` had to cut something off (a line over the limit or too many lines).
    pub fn truncated(original: &[u8]) -> bool {
        let body = original.strip_suffix(b"\n").unwrap_or(original);
        !original.is_empty() && (body.split(|&b| b == b'\n').count() > MAX_LINES || body.split(|&b| b == b'\n').any(|l| l.len() > MAX_LINE_LEN))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BIG: usize = 1 << 20;

    fn doc(s: &str) -> Doc {
        Doc::new(s.as_bytes(), BIG)
    }

    fn type_str(d: &mut Doc, s: &str) {
        for b in s.bytes() {
            d.insert(b).unwrap();
        }
    }

    #[test]
    fn loads_and_saves_the_same_text() {
        for s in ["", "one\n", "one\ntwo\n", "one\ntwo\n\nfour\n"] {
            assert_eq!(doc(s).text(), s.as_bytes(), "{s:?}");
        }
        // no final newline: it is added on save
        assert_eq!(doc("one\ntwo").text(), b"one\ntwo\n");
        assert_eq!(doc("").line_count(), 1);
        assert!(!doc("a").is_dirty());
    }

    #[test]
    fn typing_and_enter_build_lines() {
        let mut d = doc("");
        type_str(&mut d, "one");
        d.enter().unwrap();
        type_str(&mut d, "two");
        assert_eq!(d.text(), b"one\ntwo\n");
        assert_eq!(d.cursor(), (1, 3));
        assert!(d.is_dirty());
    }

    #[test]
    fn up_and_down_move_between_lines_and_remember_the_column() {
        let mut d = doc("abcdef\nab\nabcdef\n");
        d.end();
        assert_eq!(d.cursor(), (0, 6));
        d.down();
        assert_eq!(d.cursor(), (1, 2), "short line: cursor at its end");
        d.down();
        assert_eq!(d.cursor(), (2, 6), "the wanted column comes back on the long line");
        d.up();
        d.up();
        assert_eq!(d.cursor(), (0, 6));
        d.up();
        assert_eq!(d.cursor(), (0, 6), "Up on the first line stays");
        d.bottom_of_file();
        d.down();
        assert_eq!(d.cursor(), (2, 6), "Down on the last line stays");
    }

    #[test]
    fn left_and_right_cross_line_ends() {
        let mut d = doc("ab\ncd\n");
        d.right();
        d.right();
        assert_eq!(d.cursor(), (0, 2));
        d.right();
        assert_eq!(d.cursor(), (1, 0), "Right at the end goes to the next line");
        d.left();
        assert_eq!(d.cursor(), (0, 2), "Left at the start goes to the end of the line above");
        d.top_of_file();
        d.left();
        assert_eq!(d.cursor(), (0, 0));
        d.bottom_of_file();
        d.right();
        assert_eq!(d.cursor(), (1, 2));
    }

    #[test]
    fn enter_splits_and_backspace_joins() {
        let mut d = doc("hello world\n");
        for _ in 0..5 {
            d.right();
        }
        d.enter().unwrap();
        assert_eq!(d.text(), b"hello\n world\n");
        assert_eq!(d.cursor(), (1, 0));
        d.backspace().unwrap();
        assert_eq!(d.text(), b"hello world\n");
        assert_eq!(d.cursor(), (0, 5), "the cursor sits at the join");
    }

    #[test]
    fn delete_removes_under_the_cursor_or_joins_the_next_line() {
        let mut d = doc("ab\ncd\n");
        d.delete().unwrap();
        assert_eq!(d.text(), b"b\ncd\n");
        d.end();
        d.delete().unwrap();
        assert_eq!(d.text(), b"bcd\n");
        assert_eq!(d.cursor(), (0, 1), "the cursor stays at the join");
        d.bottom_of_file();
        d.delete().unwrap();
        assert_eq!(d.text(), b"bcd\n", "Delete at the very end does nothing");
        d.top_of_file();
        d.backspace().unwrap();
        assert_eq!(d.text(), b"bcd\n", "Backspace at the very start does nothing");
    }

    #[test]
    fn cut_line_removes_the_whole_line_and_keeps_one() {
        let mut d = doc("a\nb\nc\n");
        d.down();
        d.cut_line();
        assert_eq!(d.text(), b"a\nc\n");
        assert_eq!(d.cursor(), (1, 0));
        d.cut_line();
        assert_eq!(d.text(), b"a\n");
        assert_eq!(d.cursor(), (0, 0));
        d.cut_line();
        assert_eq!(d.text(), b"", "the last line is emptied, not removed");
        assert_eq!(d.line_count(), 1);
    }

    #[test]
    fn tab_is_four_spaces() {
        let mut d = doc("");
        d.tab().unwrap();
        type_str(&mut d, "x");
        assert_eq!(d.text(), b"    x\n");
    }

    #[test]
    fn only_printable_ascii_goes_in() {
        let mut d = doc("");
        assert_eq!(d.insert(0x07), Err(Refused::NotPrintable));
        assert_eq!(d.insert(0x7f), Err(Refused::NotPrintable));
        assert_eq!(d.insert(0xc3), Err(Refused::NotPrintable));
        assert_eq!(d.text(), b"");
        assert!(!d.is_dirty());
    }

    #[test]
    fn limits_refuse_and_change_nothing() {
        let mut d = doc("");
        for _ in 0..MAX_LINE_LEN {
            d.insert(b'a').unwrap();
        }
        assert_eq!(d.insert(b'b'), Err(Refused::LineTooLong));
        assert_eq!(d.line(0).len(), MAX_LINE_LEN);
        // joining two lines that would be too long is refused
        d.enter().unwrap();
        type_str(&mut d, "x");
        d.home();
        assert_eq!(d.backspace(), Err(Refused::LineTooLong));
        assert_eq!(d.line_count(), 2);
        // too many lines
        let mut d = doc("");
        for _ in 0..MAX_LINES - 1 {
            d.enter().unwrap();
        }
        assert_eq!(d.line_count(), MAX_LINES);
        assert_eq!(d.enter(), Err(Refused::TooManyLines));
        // the RAM disk
        let mut d = Doc::new(b"", 10);
        for _ in 0..9 {
            d.insert(b'a').unwrap();
        }
        assert_eq!(d.size(), 10);
        assert_eq!(d.insert(b'a'), Err(Refused::TooBig));
        assert_eq!(d.enter(), Err(Refused::TooBig));
        assert_eq!(d.text().len(), 10);
        // removing text makes room again
        d.backspace().unwrap();
        assert!(d.insert(b'z').is_ok());
    }

    #[test]
    fn the_first_character_counts_its_newline() {
        let mut d = Doc::new(b"", 1);
        assert_eq!(d.insert(b'a'), Err(Refused::TooBig), "'a' and its newline need 2 bytes");
        let mut d = Doc::new(b"", 2);
        assert!(d.insert(b'a').is_ok());
    }

    #[test]
    fn scrolling_keeps_the_cursor_in_the_window() {
        let text: String = (0..50).map(|i| format!("line {i}\n")).collect();
        let mut d = doc(&text);
        d.fit(10, 40);
        assert_eq!(d.window_cursor(), (0, 0));
        for _ in 0..25 {
            d.down();
        }
        d.fit(10, 40);
        let (r, c) = d.window_cursor();
        assert!(r < 10 && c == 0, "row {r}");
        assert_eq!(d.window_row(r, 40), b"line 25");
        d.page_down(10);
        d.fit(10, 40);
        assert_eq!(d.cursor().0, 34, "a page is rows-1 lines");
        d.page_up(10);
        d.page_up(10);
        d.page_up(10);
        d.page_up(10);
        assert_eq!(d.cursor().0, 0);
        d.bottom_of_file();
        d.fit(10, 40);
        assert_eq!(d.cursor().0, 49);
        assert!(d.window_cursor().0 < 10);
        d.top_of_file();
        d.fit(10, 40);
        assert_eq!(d.window_row(0, 40), b"line 0");
    }

    #[test]
    fn long_lines_scroll_sideways() {
        let long: String = "0123456789".repeat(10);
        let mut d = doc(&format!("{long}\nshort\n"));
        d.end();
        d.fit(5, 20);
        let (_, c) = d.window_cursor();
        assert!(c < 20);
        assert_eq!(c, 19, "the cursor is at the right edge of the window");
        assert_eq!(d.window_row(0, 20).len(), 19, "the window starts at column 81");
        d.down();
        d.fit(5, 20);
        assert_eq!(d.window_row(1, 20), b"short", "a short line scrolls the view back to the left");
        assert_eq!(d.window_cursor().1, 5);
    }

    #[test]
    fn control_characters_in_a_loaded_file_show_as_question_marks_and_survive_a_save() {
        let d = Doc::new(b"a\tb\x01c\n", BIG);
        assert_eq!(d.window_row(0, 20), b"a?b?c");
        assert_eq!(d.text(), b"a\tb\x01c\n");
    }

    #[test]
    fn oversized_files_are_cut_and_reported() {
        let long = vec![b'x'; MAX_LINE_LEN + 5];
        assert!(Doc::truncated(&long));
        assert_eq!(Doc::new(&long, BIG).line(0).len(), MAX_LINE_LEN);
        assert!(!Doc::truncated(b"fine\n"));
        assert!(!Doc::truncated(b""));
    }

    #[test]
    fn random_key_storms_keep_the_invariants() {
        let mut x: u64 = 0x1234_5678_9abc_def1;
        let mut next = || {
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            x
        };
        for _ in 0..300 {
            let mut d = Doc::new(b"alpha\nbeta\n\ngamma delta\n", 400);
            for _ in 0..400 {
                match next() % 16 {
                    0 => d.up(),
                    1 => d.down(),
                    2 => d.left(),
                    3 => d.right(),
                    4 => d.home(),
                    5 => d.end(),
                    6 => {
                        let _ = d.enter();
                    }
                    7 => {
                        let _ = d.backspace();
                    }
                    8 => {
                        let _ = d.delete();
                    }
                    9 => d.cut_line(),
                    10 => d.page_down(((next() % 12) + 1) as usize),
                    11 => d.page_up(((next() % 12) + 1) as usize),
                    12 => {
                        let _ = d.tab();
                    }
                    _ => {
                        let _ = d.insert(0x20 + (next() % 95) as u8);
                    }
                }
                d.fit(((next() % 8) + 1) as usize, ((next() % 30) + 2) as usize);
                let (r, c) = d.cursor();
                assert!(r < d.line_count());
                assert!(c <= d.line(r).len());
                assert!(d.size() <= 400, "size {} over the budget", d.size());
                assert_eq!(d.text().len(), d.size());
                let (wr, wc) = d.window_cursor();
                assert!(wr < 8 && wc < 31);
            }
        }
    }
}
