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
    generation: u64,
}

impl EditorState {
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
            generation: 0,
        })
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
            EditorAction::ScrollUp(delta) => {
                self.scroll_offset = self.scroll_offset.saturating_sub(delta as usize);
            }
            EditorAction::ScrollDown(delta) => {
                let max_scroll = self.buffer.line_count().saturating_sub(1);
                self.scroll_offset = (self.scroll_offset + delta as usize).min(max_scroll);
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

    /// Ensure the cursor is visible within the viewport.
    pub fn ensure_cursor_visible(&mut self, visible_rows: usize) {
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

    pub fn set_scroll_offset(&mut self, offset: usize) {
        let max = self.buffer.line_count().saturating_sub(1);
        self.scroll_offset = offset.min(max);
    }

    pub fn generation(&self) -> u64 {
        self.generation.wrapping_add(self.buffer.generation())
    }

    pub fn is_modified(&self) -> bool {
        self.buffer.is_modified()
    }
}
