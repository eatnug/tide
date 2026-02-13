// File buffer: line-based text storage with basic editing operations.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub line: usize,
    pub col: usize,
}

pub struct Buffer {
    pub lines: Vec<String>,
    pub file_path: Option<PathBuf>,
    pub modified: bool,
    generation: u64,
}

impl Buffer {
    pub fn new() -> Self {
        Self {
            lines: vec![String::new()],
            file_path: None,
            modified: false,
            generation: 0,
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
            self.lines[pos.line].remove(pos.col);
            self.modified = true;
            self.generation += 1;
        } else if pos.line + 1 < self.lines.len() {
            // Delete at end of line: merge with next line
            let next = self.lines.remove(pos.line + 1);
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
                self.lines[pos.line].remove(col - 1);
                self.modified = true;
                self.generation += 1;
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
            self.modified = true;
            self.generation += 1;
            Position {
                line: pos.line - 1,
                col: new_col,
            }
        } else {
            pos
        }
    }

    pub fn insert_newline(&mut self, pos: Position) -> Position {
        if pos.line >= self.lines.len() {
            return pos;
        }
        let col = pos.col.min(self.lines[pos.line].len());
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
}
