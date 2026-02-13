// File buffer: line-based text storage with basic editing operations.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

/// A single reversible edit operation.
#[derive(Debug, Clone)]
enum EditOp {
    /// Inserted a char at position.
    InsertChar { pos: Position, ch: char },
    /// Deleted a char at position (forward delete).
    DeleteChar { pos: Position, ch: char, merged_next: bool },
    /// Backspace: deleted char before position. `result_pos` is the cursor after backspace.
    Backspace { original_pos: Position, result_pos: Position, ch: Option<char>, merged_line: bool },
    /// Inserted a newline at position.
    InsertNewline { pos: Position },
}

pub struct Buffer {
    pub lines: Vec<String>,
    pub file_path: Option<PathBuf>,
    pub modified: bool,
    generation: u64,
    undo_stack: Vec<(EditOp, Position)>, // (op, cursor_before)
    redo_stack: Vec<(EditOp, Position)>,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            file_path: None,
            modified: false,
            generation: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        }
    }

    pub fn from_file(path: &Path) -> io::Result<Self> {
        let content = fs::read_to_string(path)?;
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(String::from).collect()
        };
        // Ensure at least one line
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        Ok(Self {
            lines,
            file_path: Some(path.to_path_buf()),
            modified: false,
            generation: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        })
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No file path set"))?;
        let content = self.lines.join("\n");
        fs::write(path, &content)?;
        self.modified = false;
        self.generation += 1;
        Ok(())
    }

    pub fn insert_char(&mut self, pos: Position, ch: char) {
        if pos.line >= self.lines.len() {
            return;
        }
        let col = pos.col.min(self.lines[pos.line].len());
        let actual_pos = Position { line: pos.line, col };
        self.undo_stack.push((EditOp::InsertChar { pos: actual_pos, ch }, pos));
        self.redo_stack.clear();
        self.lines[pos.line].insert(col, ch);
        self.modified = true;
        self.generation += 1;
    }

    pub fn delete_char(&mut self, pos: Position) {
        if pos.line >= self.lines.len() {
            return;
        }
        let line_len = self.lines[pos.line].len();
        if pos.col < line_len {
            let ch = self.lines[pos.line].remove(pos.col);
            self.undo_stack.push((EditOp::DeleteChar { pos, ch, merged_next: false }, pos));
            self.redo_stack.clear();
            self.modified = true;
            self.generation += 1;
        } else if pos.line + 1 < self.lines.len() {
            // Delete at end of line: merge with next line
            let next = self.lines.remove(pos.line + 1);
            self.undo_stack.push((EditOp::DeleteChar { pos, ch: '\n', merged_next: true }, pos));
            self.redo_stack.clear();
            self.lines[pos.line].push_str(&next);
            self.modified = true;
            self.generation += 1;
        }
    }

    /// Backspace: delete the character before pos, returning the new cursor position.
    pub fn backspace(&mut self, pos: Position) -> Position {
        if pos.col > 0 {
            let col = pos.col.min(self.lines[pos.line].len());
            if col > 0 {
                let ch = self.lines[pos.line].remove(col - 1);
                let result_pos = Position { line: pos.line, col: col - 1 };
                self.undo_stack.push((EditOp::Backspace {
                    original_pos: pos,
                    result_pos,
                    ch: Some(ch),
                    merged_line: false,
                }, pos));
                self.redo_stack.clear();
                self.modified = true;
                self.generation += 1;
                return result_pos;
            }
            Position {
                line: pos.line,
                col: col.saturating_sub(1),
            }
        } else if pos.line > 0 {
            // Backspace at start of line: merge with previous line
            let current = self.lines.remove(pos.line);
            let new_col = self.lines[pos.line - 1].len();
            self.lines[pos.line - 1].push_str(&current);
            let result_pos = Position { line: pos.line - 1, col: new_col };
            self.undo_stack.push((EditOp::Backspace {
                original_pos: pos,
                result_pos,
                ch: None,
                merged_line: true,
            }, pos));
            self.redo_stack.clear();
            self.modified = true;
            self.generation += 1;
            result_pos
        } else {
            pos
        }
    }

    pub fn insert_newline(&mut self, pos: Position) -> Position {
        if pos.line >= self.lines.len() {
            return pos;
        }
        let col = pos.col.min(self.lines[pos.line].len());
        let actual_pos = Position { line: pos.line, col };
        self.undo_stack.push((EditOp::InsertNewline { pos: actual_pos }, pos));
        self.redo_stack.clear();
        let rest = self.lines[pos.line][col..].to_string();
        self.lines[pos.line].truncate(col);
        self.lines.insert(pos.line + 1, rest);
        self.modified = true;
        self.generation += 1;
        Position {
            line: pos.line + 1,
            col: 0,
        }
    }

    /// Undo the last edit. Returns the cursor position to restore, or None if nothing to undo.
    pub fn undo(&mut self) -> Option<Position> {
        let (op, cursor_before) = self.undo_stack.pop()?;
        match &op {
            EditOp::InsertChar { pos, .. } => {
                // Reverse of insert: remove the char
                self.lines[pos.line].remove(pos.col);
            }
            EditOp::DeleteChar { pos, ch, merged_next } => {
                if *merged_next {
                    // Reverse of merge: split line at pos.col
                    let rest = self.lines[pos.line][pos.col..].to_string();
                    self.lines[pos.line].truncate(pos.col);
                    self.lines.insert(pos.line + 1, rest);
                } else {
                    // Reverse of delete: re-insert the char
                    self.lines[pos.line].insert(pos.col, *ch);
                }
            }
            EditOp::Backspace { original_pos, result_pos, ch, merged_line } => {
                if *merged_line {
                    // Reverse of line merge: split line at result_pos.col
                    let rest = self.lines[original_pos.line - 1][result_pos.col..].to_string();
                    self.lines[original_pos.line - 1].truncate(result_pos.col);
                    self.lines.insert(original_pos.line, rest);
                } else if let Some(c) = ch {
                    // Reverse of char deletion: re-insert at result_pos.col
                    self.lines[result_pos.line].insert(result_pos.col, *c);
                }
            }
            EditOp::InsertNewline { pos } => {
                // Reverse of newline: merge line+1 back into line
                if pos.line + 1 < self.lines.len() {
                    let next = self.lines.remove(pos.line + 1);
                    self.lines[pos.line].push_str(&next);
                }
            }
        }
        self.redo_stack.push((op, cursor_before));
        self.modified = true;
        self.generation += 1;
        Some(cursor_before)
    }

    /// Redo the last undone edit. Returns the new cursor position, or None if nothing to redo.
    pub fn redo(&mut self) -> Option<Position> {
        let (op, cursor_before) = self.redo_stack.pop()?;
        let new_cursor = match &op {
            EditOp::InsertChar { pos, ch } => {
                self.lines[pos.line].insert(pos.col, *ch);
                Position { line: pos.line, col: pos.col + 1 }
            }
            EditOp::DeleteChar { pos, ch, merged_next } => {
                if *merged_next {
                    let next = self.lines.remove(pos.line + 1);
                    self.lines[pos.line].push_str(&next);
                } else {
                    self.lines[pos.line].remove(pos.col);
                    let _ = ch;
                }
                *pos
            }
            EditOp::Backspace { result_pos, ch, merged_line, original_pos } => {
                if *merged_line {
                    let current = self.lines.remove(original_pos.line);
                    self.lines[original_pos.line - 1].push_str(&current);
                } else if ch.is_some() {
                    self.lines[result_pos.line].remove(result_pos.col);
                }
                *result_pos
            }
            EditOp::InsertNewline { pos } => {
                let col = pos.col.min(self.lines[pos.line].len());
                let rest = self.lines[pos.line][col..].to_string();
                self.lines[pos.line].truncate(col);
                self.lines.insert(pos.line + 1, rest);
                Position { line: pos.line + 1, col: 0 }
            }
        };
        self.undo_stack.push((op, cursor_before));
        self.modified = true;
        self.generation += 1;
        Some(new_cursor)
    }

    pub fn line(&self, idx: usize) -> Option<&str> {
        self.lines.get(idx).map(|s| s.as_str())
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    pub fn is_modified(&self) -> bool {
        self.modified
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_buffer_has_one_empty_line() {
        let buf = Buffer::new();
        assert_eq!(buf.line_count(), 1);
        assert_eq!(buf.line(0), Some(""));
    }

    #[test]
    fn insert_char_basic() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, 'H');
        buf.insert_char(Position { line: 0, col: 1 }, 'i');
        assert_eq!(buf.line(0), Some("Hi"));
        assert!(buf.is_modified());
    }

    #[test]
    fn insert_newline_splits_line() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, 'A');
        buf.insert_char(Position { line: 0, col: 1 }, 'B');
        let pos = buf.insert_newline(Position { line: 0, col: 1 });
        assert_eq!(pos, Position { line: 1, col: 0 });
        assert_eq!(buf.line(0), Some("A"));
        assert_eq!(buf.line(1), Some("B"));
    }

    #[test]
    fn backspace_merges_lines() {
        let mut buf = Buffer::new();
        buf.lines = vec!["Hello".into(), "World".into()];
        let pos = buf.backspace(Position { line: 1, col: 0 });
        assert_eq!(pos, Position { line: 0, col: 5 });
        assert_eq!(buf.line(0), Some("HelloWorld"));
        assert_eq!(buf.line_count(), 1);
    }

    #[test]
    fn delete_char_merges_at_eol() {
        let mut buf = Buffer::new();
        buf.lines = vec!["AB".into(), "CD".into()];
        buf.delete_char(Position { line: 0, col: 2 });
        assert_eq!(buf.line(0), Some("ABCD"));
        assert_eq!(buf.line_count(), 1);
    }

    #[test]
    fn generation_increments_on_edits() {
        let mut buf = Buffer::new();
        let g0 = buf.generation();
        buf.insert_char(Position { line: 0, col: 0 }, 'x');
        assert!(buf.generation() > g0);
    }

    #[test]
    fn undo_insert_char() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, 'A');
        buf.insert_char(Position { line: 0, col: 1 }, 'B');
        assert_eq!(buf.line(0), Some("AB"));

        let pos = buf.undo();
        assert_eq!(pos, Some(Position { line: 0, col: 1 }));
        assert_eq!(buf.line(0), Some("A"));

        let pos = buf.undo();
        assert_eq!(pos, Some(Position { line: 0, col: 0 }));
        assert_eq!(buf.line(0), Some(""));
    }

    #[test]
    fn undo_backspace() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, 'A');
        buf.insert_char(Position { line: 0, col: 1 }, 'B');
        buf.backspace(Position { line: 0, col: 2 });
        assert_eq!(buf.line(0), Some("A"));

        let pos = buf.undo();
        assert_eq!(pos, Some(Position { line: 0, col: 2 }));
        assert_eq!(buf.line(0), Some("AB"));
    }

    #[test]
    fn undo_backspace_merge() {
        let mut buf = Buffer::new();
        buf.lines = vec!["Hello".into(), "World".into()];
        buf.backspace(Position { line: 1, col: 0 });
        assert_eq!(buf.line(0), Some("HelloWorld"));
        assert_eq!(buf.line_count(), 1);

        let pos = buf.undo();
        assert_eq!(pos, Some(Position { line: 1, col: 0 }));
        assert_eq!(buf.line(0), Some("Hello"));
        assert_eq!(buf.line(1), Some("World"));
        assert_eq!(buf.line_count(), 2);
    }

    #[test]
    fn undo_delete_char() {
        let mut buf = Buffer::new();
        buf.lines = vec!["AB".into()];
        buf.delete_char(Position { line: 0, col: 0 });
        assert_eq!(buf.line(0), Some("B"));

        let pos = buf.undo();
        assert_eq!(pos, Some(Position { line: 0, col: 0 }));
        assert_eq!(buf.line(0), Some("AB"));
    }

    #[test]
    fn undo_delete_merge() {
        let mut buf = Buffer::new();
        buf.lines = vec!["AB".into(), "CD".into()];
        buf.delete_char(Position { line: 0, col: 2 });
        assert_eq!(buf.line(0), Some("ABCD"));

        let pos = buf.undo();
        assert_eq!(pos, Some(Position { line: 0, col: 2 }));
        assert_eq!(buf.line(0), Some("AB"));
        assert_eq!(buf.line(1), Some("CD"));
    }

    #[test]
    fn undo_insert_newline() {
        let mut buf = Buffer::new();
        buf.lines = vec!["ABCD".into()];
        buf.insert_newline(Position { line: 0, col: 2 });
        assert_eq!(buf.line(0), Some("AB"));
        assert_eq!(buf.line(1), Some("CD"));

        let pos = buf.undo();
        assert_eq!(pos, Some(Position { line: 0, col: 2 }));
        assert_eq!(buf.line(0), Some("ABCD"));
        assert_eq!(buf.line_count(), 1);
    }

    #[test]
    fn redo_insert_char() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, 'A');
        buf.undo();
        assert_eq!(buf.line(0), Some(""));

        let pos = buf.redo();
        assert_eq!(pos, Some(Position { line: 0, col: 1 }));
        assert_eq!(buf.line(0), Some("A"));
    }

    #[test]
    fn redo_cleared_on_new_edit() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, 'A');
        buf.undo();
        buf.insert_char(Position { line: 0, col: 0 }, 'B');
        assert_eq!(buf.redo(), None);
    }

    #[test]
    fn undo_empty_returns_none() {
        let mut buf = Buffer::new();
        assert_eq!(buf.undo(), None);
    }
}
