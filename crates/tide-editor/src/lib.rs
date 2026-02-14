// tide-editor: built-in file viewer/editor with syntax highlighting.

pub mod buffer;
pub mod cursor;
pub mod highlight;
pub mod input;

use std::io;
use std::path::Path;

use buffer::{Buffer, Position};
use cursor::EditorCursor;
use highlight::{Highlighter, StyledSpan};
use input::EditorAction;
use syntect::parsing::SyntaxReference;

pub use buffer::Position as EditorPosition;
pub use highlight::StyledSpan as EditorStyledSpan;
pub use input::{key_to_editor_action, EditorAction as EditorActionKind};

/// The main editor state orchestrator.
pub struct EditorState {
    pub buffer: Buffer,
    pub cursor: EditorCursor,
    highlighter: Highlighter,
    syntax: Option<String>, // syntax name, used to look up reference on demand
    scroll_offset: usize,
    h_scroll_offset: usize,
    generation: u64,
}

impl EditorState {
    /// Create a new empty editor (no file on disk).
    pub fn new_empty() -> Self {
        Self {
            buffer: Buffer::new(),
            cursor: EditorCursor::new(),
            highlighter: Highlighter::new(),
            syntax: None,
            scroll_offset: 0,
            h_scroll_offset: 0,
            generation: 0,
        }
    }

    /// Open a file for editing.
    pub fn open(path: &Path) -> io::Result<Self> {
        let buffer = Buffer::from_file(path)?;
        let highlighter = Highlighter::new();
        let syntax_name = highlighter
            .detect_syntax(path)
            .map(|s| s.name.clone());

        Ok(Self {
            buffer,
            cursor: EditorCursor::new(),
            highlighter,
            syntax: syntax_name,
            scroll_offset: 0,
            h_scroll_offset: 0,
            generation: 0,
        })
    }

    /// Reload the file from disk, resetting cursor and scroll.
    pub fn reload(&mut self) -> io::Result<()> {
        self.buffer.reload()?;
        self.cursor.set_position(buffer::Position { line: 0, col: 0 });
        self.scroll_offset = 0;
        self.h_scroll_offset = 0;
        self.generation += 1;
        Ok(())
    }

    /// Handle an editor action (from key mapping).
    pub fn handle_action(&mut self, action: EditorAction) {
        match action {
            EditorAction::InsertChar(ch) => {
                self.buffer.insert_char(self.cursor.position, ch);
                self.cursor.position.col += 1;
                self.cursor.desired_col = self.cursor.position.col;
                self.generation += 1;
            }
            EditorAction::Backspace => {
                let new_pos = self.buffer.backspace(self.cursor.position);
                self.cursor.set_position(new_pos);
                self.generation += 1;
            }
            EditorAction::Delete => {
                self.buffer.delete_char(self.cursor.position);
                self.generation += 1;
            }
            EditorAction::Enter => {
                let new_pos = self.buffer.insert_newline(self.cursor.position);
                self.cursor.set_position(new_pos);
                self.generation += 1;
            }
            EditorAction::MoveUp => self.cursor.move_up(&self.buffer),
            EditorAction::MoveDown => self.cursor.move_down(&self.buffer),
            EditorAction::MoveLeft => self.cursor.move_left(&self.buffer),
            EditorAction::MoveRight => self.cursor.move_right(&self.buffer),
            EditorAction::Home => self.cursor.move_home(),
            EditorAction::End => self.cursor.move_end(&self.buffer),
            EditorAction::PageUp => self.cursor.move_page_up(&self.buffer, 30),
            EditorAction::PageDown => self.cursor.move_page_down(&self.buffer, 30),
            EditorAction::Save => {
                if let Err(e) = self.buffer.save() {
                    log::error!("Failed to save file: {}", e);
                }
                self.generation += 1;
            }
            EditorAction::Undo => {
                if let Some(pos) = self.buffer.undo() {
                    self.cursor.set_position(pos);
                    self.generation += 1;
                }
            }
            EditorAction::Redo => {
                if let Some(pos) = self.buffer.redo() {
                    self.cursor.set_position(pos);
                    self.generation += 1;
                }
            }
            EditorAction::SetCursor { line, col } => {
                let line = line.min(self.buffer.line_count().saturating_sub(1));
                let col = col.min(self.buffer.line(line).map_or(0, |l| l.len()));
                self.cursor.set_position(Position { line, col });
            }
            EditorAction::ScrollUp(delta) => {
                let prev = self.scroll_offset;
                self.scroll_offset = self.scroll_offset.saturating_sub(delta as usize);
                if self.scroll_offset != prev {
                    self.generation += 1;
                }
            }
            EditorAction::ScrollDown(delta) => {
                let prev = self.scroll_offset;
                let max_scroll = self.buffer.line_count().saturating_sub(1);
                self.scroll_offset = (self.scroll_offset + delta as usize).min(max_scroll);
                if self.scroll_offset != prev {
                    self.generation += 1;
                }
            }
            EditorAction::ScrollLeft(delta) => {
                let prev = self.h_scroll_offset;
                self.h_scroll_offset = self.h_scroll_offset.saturating_sub(delta as usize);
                if self.h_scroll_offset != prev {
                    self.generation += 1;
                }
            }
            EditorAction::ScrollRight(delta) => {
                let prev = self.h_scroll_offset;
                self.h_scroll_offset += delta as usize;
                if self.h_scroll_offset != prev {
                    self.generation += 1;
                }
            }
        }
    }

    /// Get syntax-highlighted lines for the visible viewport.
    pub fn visible_highlighted_lines(&self, visible_rows: usize) -> Vec<Vec<StyledSpan>> {
        let syntax_ref = self.syntax.as_ref().and_then(|name| {
            self.highlighter.syntax_set().find_syntax_by_name(name)
        });
        let syntax: &SyntaxReference = match syntax_ref {
            Some(s) => s,
            None => self.highlighter.plain_text_syntax(),
        };
        self.highlighter.highlight_lines(
            &self.buffer.lines,
            syntax,
            self.scroll_offset,
            visible_rows,
        )
    }

    /// Ensure the cursor is visible within the viewport (both vertically and horizontally).
    pub fn ensure_cursor_visible(&mut self, visible_rows: usize) {
        self.ensure_cursor_visible_v(visible_rows);
    }

    /// Ensure the cursor is vertically visible.
    fn ensure_cursor_visible_v(&mut self, visible_rows: usize) {
        if visible_rows == 0 {
            return;
        }
        let line = self.cursor.position.line;
        if line < self.scroll_offset {
            self.scroll_offset = line;
        } else if line >= self.scroll_offset + visible_rows {
            self.scroll_offset = line - visible_rows + 1;
        }
    }

    /// Ensure the cursor is horizontally visible.
    pub fn ensure_cursor_visible_h(&mut self, visible_cols: usize) {
        if visible_cols == 0 {
            return;
        }
        let col = self.cursor.position.col;
        if col < self.h_scroll_offset {
            self.h_scroll_offset = col;
        } else if col >= self.h_scroll_offset + visible_cols {
            self.h_scroll_offset = col - visible_cols + 1;
        }
    }

    pub fn file_name(&self) -> &str {
        self.buffer
            .file_path
            .as_ref()
            .and_then(|p| p.file_name())
            .and_then(|n| n.to_str())
            .unwrap_or("Untitled")
    }

    pub fn file_path(&self) -> Option<&Path> {
        self.buffer.file_path.as_deref()
    }

    pub fn cursor_position(&self) -> Position {
        self.cursor.position
    }

    pub fn scroll_offset(&self) -> usize {
        self.scroll_offset
    }

    pub fn h_scroll_offset(&self) -> usize {
        self.h_scroll_offset
    }

    pub fn set_scroll_offset(&mut self, offset: usize) {
        let max = self.buffer.line_count().saturating_sub(1);
        let new_offset = offset.min(max);
        if new_offset != self.scroll_offset {
            self.scroll_offset = new_offset;
            self.generation += 1;
        }
    }

    pub fn set_h_scroll_offset(&mut self, offset: usize) {
        if offset != self.h_scroll_offset {
            self.h_scroll_offset = offset;
            self.generation += 1;
        }
    }

    pub fn generation(&self) -> u64 {
        self.generation.wrapping_add(self.buffer.generation())
    }

    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }
}
