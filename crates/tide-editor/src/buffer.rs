// File buffer: line-based text storage with basic editing operations.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::undo::EditOp;

/// Find the largest byte offset <= idx that is a valid char boundary in the string.
pub fn floor_char_boundary(s: &str, idx: usize) -> usize {
    if idx >= s.len() {
        return s.len();
    }
    let mut i = idx;
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

pub struct Buffer {
    pub lines: Vec<String>,
    pub file_path: Option<PathBuf>,
    pub(crate) generation: u64,
    /// Snapshot of the content at the last save (or load) point.
    /// Used for content-based dirty tracking.
    saved_content: Vec<String>,
    pub(crate) undo_stack: Vec<(EditOp, Position)>, // (op, cursor_before)
    pub(crate) redo_stack: Vec<(EditOp, Position)>,
}

impl Buffer {
    pub fn new() -> Self {
        let lines = vec![String::new()];
        Self {
            saved_content: lines.clone(),
            lines,
            file_path: None,
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
            saved_content: lines.clone(),
            lines,
            file_path: Some(path.to_path_buf()),
            generation: 0,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
        })
    }

    pub fn reload(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No file path set"))?;
        let content = fs::read_to_string(path)?;
        let lines: Vec<String> = if content.is_empty() {
            vec![String::new()]
        } else {
            content.lines().map(String::from).collect()
        };
        let lines = if lines.is_empty() {
            vec![String::new()]
        } else {
            lines
        };
        if self.lines == lines {
            // Content matches — just update saved_content, keep undo/redo stacks intact
            self.saved_content = lines;
        } else {
            self.saved_content = lines.clone();
            self.lines = lines;
            self.undo_stack.clear();
            self.redo_stack.clear();
        }
        self.generation += 1;
        Ok(())
    }

    pub fn save(&mut self) -> io::Result<()> {
        let path = self
            .file_path
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::Other, "No file path set"))?;
        let content = self.lines.join("\n");
        fs::write(path, &content)?;
        self.saved_content = self.lines.clone();
        self.generation += 1;
        Ok(())
    }

    pub fn insert_char(&mut self, pos: Position, ch: char) {
        if pos.line >= self.lines.len() {
            return;
        }
        let col = floor_char_boundary(&self.lines[pos.line], pos.col.min(self.lines[pos.line].len()));
        let actual_pos = Position { line: pos.line, col };
        self.undo_stack.push((EditOp::InsertChar { pos: actual_pos, ch }, pos));
        self.redo_stack.clear();
        self.lines[pos.line].insert(col, ch);
        self.generation += 1;
    }

    pub fn delete_char(&mut self, pos: Position) {
        if pos.line >= self.lines.len() {
            return;
        }
        let line_len = self.lines[pos.line].len();
        let col = floor_char_boundary(&self.lines[pos.line], pos.col);
        if col < line_len {
            let ch = self.lines[pos.line].remove(col);
            let actual_pos = Position { line: pos.line, col };
            self.undo_stack.push((EditOp::DeleteChar { pos: actual_pos, ch, merged_next: false }, pos));
            self.redo_stack.clear();
            self.generation += 1;
        } else if pos.line + 1 < self.lines.len() {
            // Delete at end of line: merge with next line
            let next = self.lines.remove(pos.line + 1);
            self.undo_stack.push((EditOp::DeleteChar { pos, ch: '\n', merged_next: true }, pos));
            self.redo_stack.clear();
            self.lines[pos.line].push_str(&next);
            self.generation += 1;
        }
    }

    /// Backspace: delete the character before pos, returning the new cursor position.
    pub fn backspace(&mut self, pos: Position) -> Position {
        if pos.line >= self.lines.len() {
            return pos;
        }
        if pos.col > 0 {
            let col = floor_char_boundary(&self.lines[pos.line], pos.col.min(self.lines[pos.line].len()));
            if col > 0 {
                let prev = floor_char_boundary(&self.lines[pos.line], col - 1);
                let ch = self.lines[pos.line].remove(prev);
                let result_pos = Position { line: pos.line, col: prev };
                self.undo_stack.push((EditOp::Backspace {
                    original_pos: pos,
                    result_pos,
                    ch: Some(ch),
                    merged_line: false,
                }, pos));
                self.redo_stack.clear();
                self.generation += 1;
                return result_pos;
            }
            Position {
                line: pos.line,
                col: 0,
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
        let col = floor_char_boundary(&self.lines[pos.line], pos.col.min(self.lines[pos.line].len()));
        let actual_pos = Position { line: pos.line, col };
        self.undo_stack.push((EditOp::InsertNewline { pos: actual_pos }, pos));
        self.redo_stack.clear();
        let rest = self.lines[pos.line][col..].to_string();
        self.lines[pos.line].truncate(col);
        self.lines.insert(pos.line + 1, rest);
        self.generation += 1;
        Position {
            line: pos.line + 1,
            col: 0,
        }
    }

    pub fn line(&self, idx: usize) -> Option<&str> {
        self.lines.get(idx).map(|s| s.as_str())
    }

    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Return the character count of the longest line.
    pub fn max_line_chars(&self) -> usize {
        self.lines.iter().map(|l| l.chars().count()).max().unwrap_or(0)
    }

    /// Delete text between two byte-offset positions, returning the new cursor position (start).
    /// `start` and `end` are (line, byte_col) positions; start must be <= end.
    pub fn delete_range(&mut self, start: Position, end: Position) -> Position {
        if start == end || start.line >= self.lines.len() {
            return start;
        }
        let end_line = end.line.min(self.lines.len() - 1);
        let end_col = end.col.min(self.lines[end_line].len());
        let start_col = start.col.min(self.lines[start.line].len());

        // Capture the deleted text for undo
        let mut deleted_lines = Vec::new();
        if start.line == end_line {
            deleted_lines.push(self.lines[start.line][start_col..end_col].to_string());
        } else {
            deleted_lines.push(self.lines[start.line][start_col..].to_string());
            for line_idx in (start.line + 1)..end_line {
                deleted_lines.push(self.lines[line_idx].clone());
            }
            deleted_lines.push(self.lines[end_line][..end_col].to_string());
        }

        let actual_start = Position { line: start.line, col: start_col };
        let actual_end = Position { line: end_line, col: end_col };
        self.undo_stack.push((
            crate::undo::EditOp::DeleteRange {
                start: actual_start,
                end: actual_end,
                deleted_lines,
            },
            start,
        ));
        self.redo_stack.clear();

        if start.line == end_line {
            self.lines[start.line].drain(start_col..end_col);
        } else {
            let suffix = self.lines[end_line][end_col..].to_string();
            self.lines[start.line].truncate(start_col);
            self.lines[start.line].push_str(&suffix);
            self.lines.drain((start.line + 1)..=end_line);
        }
        self.generation += 1;
        Position { line: start.line, col: start_col }
    }

    /// Insert a block of text at `pos`, returning the end position after insertion.
    /// The entire insertion is a single undo entry.
    pub fn insert_text(&mut self, pos: Position, text: &str) -> Position {
        if pos.line >= self.lines.len() || text.is_empty() {
            return pos;
        }
        let col = floor_char_boundary(&self.lines[pos.line], pos.col.min(self.lines[pos.line].len()));
        let actual_pos = Position { line: pos.line, col };

        let suffix = self.lines[pos.line][col..].to_string();
        self.lines[pos.line].truncate(col);

        // Normalize \r\n to \n, skip standalone \r
        let normalized: String = text.replace("\r\n", "\n").replace('\r', "");
        let text_lines: Vec<&str> = normalized.split('\n').collect();

        let end_pos = if text_lines.len() == 1 {
            self.lines[pos.line].push_str(text_lines[0]);
            let end_col = self.lines[pos.line].len();
            self.lines[pos.line].push_str(&suffix);
            Position { line: pos.line, col: end_col }
        } else {
            self.lines[pos.line].push_str(text_lines[0]);
            for (i, tl) in text_lines[1..text_lines.len() - 1].iter().enumerate() {
                self.lines.insert(pos.line + 1 + i, tl.to_string());
            }
            let last_idx = pos.line + text_lines.len() - 1;
            let mut last_line = text_lines.last().unwrap().to_string();
            let end_col = last_line.len();
            last_line.push_str(&suffix);
            self.lines.insert(last_idx, last_line);
            Position { line: last_idx, col: end_col }
        };

        self.undo_stack.push((EditOp::InsertText { pos: actual_pos, text: normalized, end_pos }, pos));
        self.redo_stack.clear();
        self.generation += 1;
        end_pos
    }

    pub fn is_modified(&self) -> bool {
        self.lines != self.saved_content
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

    #[test]
    fn insert_then_backspace_not_modified() {
        let mut buf = Buffer::new();
        assert!(!buf.is_modified());
        buf.insert_char(Position { line: 0, col: 0 }, 'a');
        assert!(buf.is_modified());
        buf.backspace(Position { line: 0, col: 1 });
        assert!(!buf.is_modified()); // content matches original
    }

    #[test]
    fn insert_then_delete_not_modified() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, 'a');
        assert!(buf.is_modified());
        buf.delete_char(Position { line: 0, col: 0 });
        assert!(!buf.is_modified());
    }

    #[test]
    fn undo_all_not_modified() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, 'x');
        buf.insert_char(Position { line: 0, col: 1 }, 'y');
        assert!(buf.is_modified());
        buf.undo();
        buf.undo();
        assert!(!buf.is_modified());
    }

    #[test]
    fn insert_multibyte_chars() {
        let mut buf = Buffer::new();
        // '가' is 3 bytes in UTF-8
        buf.insert_char(Position { line: 0, col: 0 }, '가');
        assert_eq!(buf.line(0), Some("가"));
        // Insert second char after first (at byte offset 3)
        buf.insert_char(Position { line: 0, col: 3 }, '나');
        assert_eq!(buf.line(0), Some("가나"));
        // Insert between the two (at byte offset 3)
        buf.insert_char(Position { line: 0, col: 3 }, 'A');
        assert_eq!(buf.line(0), Some("가A나"));
    }

    #[test]
    fn backspace_multibyte_char() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, '가');
        buf.insert_char(Position { line: 0, col: 3 }, '나');
        assert_eq!(buf.line(0), Some("가나"));
        // Backspace from end (byte offset 6) should remove '나'
        let pos = buf.backspace(Position { line: 0, col: 6 });
        assert_eq!(pos, Position { line: 0, col: 3 });
        assert_eq!(buf.line(0), Some("가"));
        // Backspace from byte offset 3 should remove '가'
        let pos = buf.backspace(Position { line: 0, col: 3 });
        assert_eq!(pos, Position { line: 0, col: 0 });
        assert_eq!(buf.line(0), Some(""));
    }

    #[test]
    fn delete_multibyte_char() {
        let mut buf = Buffer::new();
        buf.lines = vec!["가나다".into()];
        // Delete at byte offset 0 removes '가'
        buf.delete_char(Position { line: 0, col: 0 });
        assert_eq!(buf.line(0), Some("나다"));
        // Delete at byte offset 0 now removes '나'
        buf.delete_char(Position { line: 0, col: 0 });
        assert_eq!(buf.line(0), Some("다"));
    }

    #[test]
    fn undo_redo_multibyte() {
        let mut buf = Buffer::new();
        buf.insert_char(Position { line: 0, col: 0 }, '한');
        buf.insert_char(Position { line: 0, col: 3 }, '글');
        assert_eq!(buf.line(0), Some("한글"));

        let pos = buf.undo();
        assert_eq!(pos, Some(Position { line: 0, col: 3 }));
        assert_eq!(buf.line(0), Some("한"));

        let pos = buf.redo();
        assert_eq!(pos, Some(Position { line: 0, col: 6 }));
        assert_eq!(buf.line(0), Some("한글"));
    }
}
