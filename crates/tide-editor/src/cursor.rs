// Cursor management for the editor.

use crate::buffer::{Buffer, Position};

pub struct EditorCursor {
    pub position: Position,
    /// The column the cursor "wants" to be at (for up/down movement through short lines).
    pub desired_col: usize,
}

impl EditorCursor {
    pub fn new() -> Self {
        Self {
            position: Position { line: 0, col: 0 },
            desired_col: 0,
        }
    }

    pub fn move_up(&mut self, buffer: &Buffer) {
        if self.position.line > 0 {
            self.position.line -= 1;
            let line_len = buffer.line(self.position.line).map_or(0, |l| l.len());
            self.position.col = self.desired_col.min(line_len);
        }
    }

    pub fn move_down(&mut self, buffer: &Buffer) {
        if self.position.line + 1 < buffer.line_count() {
            self.position.line += 1;
            let line_len = buffer.line(self.position.line).map_or(0, |l| l.len());
            self.position.col = self.desired_col.min(line_len);
        }
    }

    pub fn move_left(&mut self, buffer: &Buffer) {
        if self.position.col > 0 {
            self.position.col -= 1;
        } else if self.position.line > 0 {
            self.position.line -= 1;
            self.position.col = buffer.line(self.position.line).map_or(0, |l| l.len());
        }
        self.desired_col = self.position.col;
    }

    pub fn move_right(&mut self, buffer: &Buffer) {
        let line_len = buffer.line(self.position.line).map_or(0, |l| l.len());
        if self.position.col < line_len {
            self.position.col += 1;
        } else if self.position.line + 1 < buffer.line_count() {
            self.position.line += 1;
            self.position.col = 0;
        }
        self.desired_col = self.position.col;
    }

    pub fn move_home(&mut self) {
        self.position.col = 0;
        self.desired_col = 0;
    }

    pub fn move_end(&mut self, buffer: &Buffer) {
        let line_len = buffer.line(self.position.line).map_or(0, |l| l.len());
        self.position.col = line_len;
        self.desired_col = self.position.col;
    }

    pub fn move_page_up(&mut self, buffer: &Buffer, visible_rows: usize) {
        let jump = visible_rows.saturating_sub(1).max(1);
        self.position.line = self.position.line.saturating_sub(jump);
        let line_len = buffer.line(self.position.line).map_or(0, |l| l.len());
        self.position.col = self.desired_col.min(line_len);
    }

    pub fn move_page_down(&mut self, buffer: &Buffer, visible_rows: usize) {
        let jump = visible_rows.saturating_sub(1).max(1);
        self.position.line = (self.position.line + jump).min(buffer.line_count().saturating_sub(1));
        let line_len = buffer.line(self.position.line).map_or(0, |l| l.len());
        self.position.col = self.desired_col.min(line_len);
    }

    /// Clamp cursor to valid position within buffer bounds.
    pub fn clamp(&mut self, buffer: &Buffer) {
        if buffer.line_count() == 0 {
            self.position = Position { line: 0, col: 0 };
            return;
        }
        self.position.line = self.position.line.min(buffer.line_count() - 1);
        let line_len = buffer.line(self.position.line).map_or(0, |l| l.len());
        self.position.col = self.position.col.min(line_len);
    }

    /// Set cursor to a specific position, updating desired_col.
    pub fn set_position(&mut self, pos: Position) {
        self.position = pos;
        self.desired_col = pos.col;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_buffer(lines: &[&str]) -> Buffer {
        let mut buf = Buffer::new();
        buf.lines = lines.iter().map(|s| s.to_string()).collect();
        buf
    }

    #[test]
    fn move_up_from_first_line_stays() {
        let buf = make_buffer(&["hello", "world"]);
        let mut cur = EditorCursor::new();
        cur.move_up(&buf);
        assert_eq!(cur.position, Position { line: 0, col: 0 });
    }

    #[test]
    fn move_down_wraps_col_to_shorter_line() {
        let buf = make_buffer(&["hello", "hi"]);
        let mut cur = EditorCursor::new();
        cur.position.col = 4;
        cur.desired_col = 4;
        cur.move_down(&buf);
        assert_eq!(cur.position, Position { line: 1, col: 2 });
        // desired_col preserved
        assert_eq!(cur.desired_col, 4);
    }

    #[test]
    fn move_left_wraps_to_prev_line() {
        let buf = make_buffer(&["abc", "def"]);
        let mut cur = EditorCursor::new();
        cur.position = Position { line: 1, col: 0 };
        cur.move_left(&buf);
        assert_eq!(cur.position, Position { line: 0, col: 3 });
    }

    #[test]
    fn move_right_wraps_to_next_line() {
        let buf = make_buffer(&["ab", "cd"]);
        let mut cur = EditorCursor::new();
        cur.position = Position { line: 0, col: 2 };
        cur.move_right(&buf);
        assert_eq!(cur.position, Position { line: 1, col: 0 });
    }

    #[test]
    fn page_up_and_down() {
        let buf = make_buffer(&["0", "1", "2", "3", "4", "5", "6", "7", "8", "9"]);
        let mut cur = EditorCursor::new();
        cur.position = Position { line: 5, col: 0 };
        cur.move_page_up(&buf, 3);
        assert_eq!(cur.position.line, 3);
        cur.move_page_down(&buf, 3);
        assert_eq!(cur.position.line, 5);
    }
}
