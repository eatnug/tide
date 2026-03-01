// tide-editor: built-in file viewer/editor with syntax highlighting.

pub mod buffer;
pub mod cursor;
pub mod highlight;
pub mod input;
pub mod markdown;
mod undo;

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

    /// Reload the file from disk, preserving cursor position (clamped to valid bounds).
    pub fn reload(&mut self) -> io::Result<()> {
        let old_lines = self.buffer.lines.clone();
        self.buffer.reload()?;
        // Clamp cursor to valid position instead of resetting (VSCode-like behavior)
        if self.buffer.lines != old_lines {
            let max_line = self.buffer.line_count().saturating_sub(1);
            let new_line = self.cursor.position.line.min(max_line);
            let max_col = self.buffer.line(new_line).map_or(0, |l| l.len());
            let new_col = self.cursor.position.col.min(max_col);
            self.cursor.set_position(buffer::Position { line: new_line, col: new_col });
            // Clamp scroll offsets
            let max_scroll = self.buffer.line_count().saturating_sub(1);
            self.scroll_offset = self.scroll_offset.min(max_scroll);
        }
        self.generation += 1;
        Ok(())
    }

    /// Handle an editor action (from key mapping).
    pub fn handle_action(&mut self, action: EditorAction) {
        // Defensive: clamp cursor to valid buffer bounds before any operation.
        // This prevents panics if cursor drifts out of sync (e.g. after file reload).
        self.cursor.clamp(&self.buffer);

        match action {
            EditorAction::InsertChar(ch) => {
                self.buffer.insert_char(self.cursor.position, ch);
                self.cursor.position.col += ch.len_utf8();
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
                // Capture leading whitespace from current line for auto-indent
                let indent = if let Some(line) = self.buffer.line(self.cursor.position.line) {
                    let ws: String = line.chars()
                        .take_while(|c| *c == ' ' || *c == '\t')
                        .collect();
                    ws
                } else {
                    String::new()
                };
                let new_pos = self.buffer.insert_newline(self.cursor.position);
                // Insert the indent on the new line (handles empty string gracefully)
                let end_pos = self.buffer.insert_text(new_pos, &indent);
                self.cursor.set_position(end_pos);
                self.generation += 1;
            }
            EditorAction::MoveUp => self.cursor.move_up(&self.buffer),
            EditorAction::MoveDown => self.cursor.move_down(&self.buffer),
            EditorAction::MoveLeft => self.cursor.move_left(&self.buffer),
            EditorAction::MoveRight => self.cursor.move_right(&self.buffer),
            EditorAction::MoveWordLeft => self.cursor.move_word_left(&self.buffer),
            EditorAction::MoveWordRight => self.cursor.move_word_right(&self.buffer),
            EditorAction::MoveDocStart => self.cursor.move_doc_start(),
            EditorAction::MoveDocEnd => self.cursor.move_doc_end(&self.buffer),
            EditorAction::Home => self.cursor.move_home(),
            EditorAction::End => self.cursor.move_end(&self.buffer),
            EditorAction::PageUp => self.cursor.move_page_up(&self.buffer, 30),
            EditorAction::PageDown => self.cursor.move_page_down(&self.buffer, 30),
            EditorAction::SelectAll => {
                // Handled by the EditorPane wrapper (needs access to selection state)
            }
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
            EditorAction::DeleteWordLeft => {
                let new_pos = self.buffer.delete_word_left(self.cursor.position);
                self.cursor.set_position(new_pos);
                self.generation += 1;
            }
            EditorAction::DeleteWordRight => {
                self.buffer.delete_word_right(self.cursor.position);
                self.generation += 1;
            }
            EditorAction::DeleteToLineStart => {
                let new_pos = self.buffer.delete_to_line_start(self.cursor.position);
                self.cursor.set_position(new_pos);
                self.generation += 1;
            }
            EditorAction::DeleteToLineEnd => {
                self.buffer.delete_to_line_end(self.cursor.position);
                self.generation += 1;
            }
            EditorAction::DeleteLine => {
                let new_pos = self.buffer.delete_line(self.cursor.position.line);
                self.cursor.set_position(new_pos);
                self.generation += 1;
            }
            EditorAction::MoveLineUp => {
                if self.buffer.swap_line_up(self.cursor.position.line) {
                    self.cursor.position.line -= 1;
                    self.generation += 1;
                }
            }
            EditorAction::MoveLineDown => {
                if self.buffer.swap_line_down(self.cursor.position.line) {
                    self.cursor.position.line += 1;
                    self.generation += 1;
                }
            }
            EditorAction::Unindent => {
                let removed = self.buffer.unindent_line(self.cursor.position.line);
                if removed > 0 {
                    self.cursor.position.col = self.cursor.position.col.saturating_sub(removed);
                    self.cursor.desired_col = self.cursor.position.col;
                    self.generation += 1;
                }
            }
            EditorAction::SetCursor { line, col } => {
                let line = line.min(self.buffer.line_count().saturating_sub(1));
                // col is a character index (from mouse click) — convert to byte offset
                let byte_col = if let Some(line_str) = self.buffer.line(line) {
                    line_str.char_indices()
                        .nth(col)
                        .map(|(i, _)| i)
                        .unwrap_or(line_str.len())
                } else {
                    0
                };
                self.cursor.set_position(Position { line, col: byte_col });
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
                let max_line_chars = self.buffer.max_line_chars();
                self.h_scroll_offset = (self.h_scroll_offset + delta as usize).min(max_line_chars);
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

    /// Insert a block of text at the current cursor position (single undo entry).
    pub fn insert_text(&mut self, text: &str) {
        self.cursor.clamp(&self.buffer);
        let end_pos = self.buffer.insert_text(self.cursor.position, text);
        self.cursor.set_position(end_pos);
        self.generation += 1;
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
        // h_scroll_offset is character-indexed; convert cursor byte offset to char index
        let char_col = if let Some(line) = self.buffer.line(self.cursor.position.line) {
            let byte_col = self.cursor.position.col.min(line.len());
            line[..byte_col].chars().count()
        } else {
            0
        };
        if char_col < self.h_scroll_offset {
            self.h_scroll_offset = char_col;
        } else if char_col >= self.h_scroll_offset + visible_cols {
            self.h_scroll_offset = char_col - visible_cols + 1;
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

    /// Display name with up to two parent directories for disambiguation.
    /// e.g. "tide-app/src/event_loop.rs" instead of just "src/event_loop.rs".
    pub fn file_display_name(&self) -> String {
        match self.buffer.file_path.as_ref() {
            Some(path) => {
                let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("Untitled");
                let parent = path.parent();
                let parent_name = parent.and_then(|p| p.file_name()).and_then(|n| n.to_str());
                let grandparent_name = parent
                    .and_then(|p| p.parent())
                    .and_then(|p| p.file_name())
                    .and_then(|n| n.to_str());
                match (grandparent_name, parent_name) {
                    (Some(gp), Some(p)) => format!("{}/{}/{}", gp, p, name),
                    (None, Some(p)) => format!("{}/{}", p, name),
                    _ => name.to_string(),
                }
            }
            None => "Untitled".to_string(),
        }
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

    /// Detect and set syntax highlighting based on a file path.
    pub fn detect_and_set_syntax(&mut self, path: &Path) {
        self.syntax = self.highlighter.detect_syntax(path).map(|s| s.name.clone());
        self.generation += 1;
    }

    /// Switch syntax highlighting theme for dark/light mode.
    pub fn set_dark_mode(&mut self, dark: bool) {
        self.highlighter.set_dark_mode(dark);
        self.generation += 1;
    }

    /// Find the matching bracket for the bracket at (or near) the cursor position.
    /// Returns `Some((open_pos, close_pos))` if a matching pair is found.
    pub fn matching_bracket(&self) -> Option<(Position, Position)> {
        let pos = self.cursor.position;
        let line_text = self.buffer.line(pos.line)?;
        let byte_col = pos.col.min(line_text.len());

        // Find the bracket at cursor or just before cursor
        let (bracket_char, bracket_byte) = {
            let at_cursor = line_text.get(byte_col..byte_col + 1).and_then(|s| s.chars().next());
            let before_cursor = if byte_col > 0 {
                let prev_start = line_text.floor_char_boundary(byte_col.saturating_sub(1));
                line_text.get(prev_start..byte_col).and_then(|s| s.chars().next())
            } else {
                None
            };
            if let Some(ch) = at_cursor {
                if "()[]{}".contains(ch) {
                    (ch, byte_col)
                } else if let Some(ch2) = before_cursor {
                    if "()[]{}".contains(ch2) {
                        let prev_start = line_text.floor_char_boundary(byte_col.saturating_sub(1));
                        (ch2, prev_start)
                    } else {
                        return None;
                    }
                } else {
                    return None;
                }
            } else if let Some(ch2) = before_cursor {
                if "()[]{}".contains(ch2) {
                    let prev_start = line_text.floor_char_boundary(byte_col.saturating_sub(1));
                    (ch2, prev_start)
                } else {
                    return None;
                }
            } else {
                return None;
            }
        };

        let (open, close, forward) = match bracket_char {
            '(' => ('(', ')', true),
            ')' => ('(', ')', false),
            '[' => ('[', ']', true),
            ']' => ('[', ']', false),
            '{' => ('{', '}', true),
            '}' => ('{', '}', false),
            _ => return None,
        };

        let start_pos = Position { line: pos.line, col: bracket_byte };

        if forward {
            // Scan forward from start_pos
            let mut depth = 0i32;
            let mut line_idx = pos.line;
            let mut col_start = bracket_byte;
            let total_lines = self.buffer.line_count();
            while line_idx < total_lines {
                let text = self.buffer.line(line_idx)?;
                for (byte_i, ch) in text[col_start..].char_indices() {
                    let abs_byte = col_start + byte_i;
                    if ch == open { depth += 1; }
                    if ch == close { depth -= 1; }
                    if depth == 0 {
                        return Some((start_pos, Position { line: line_idx, col: abs_byte }));
                    }
                }
                line_idx += 1;
                col_start = 0;
            }
        } else {
            // Scan backward from start_pos
            let mut depth = 0i32;
            let mut line_idx = pos.line as isize;
            let mut scan_from_end = false;
            let first_col = bracket_byte;
            loop {
                let text = self.buffer.line(line_idx as usize)?;
                let scan_text = if !scan_from_end {
                    &text[..first_col + bracket_char.len_utf8()]
                } else {
                    text
                };
                for (byte_i, ch) in scan_text.char_indices().rev() {
                    if ch == close { depth += 1; }
                    if ch == open { depth -= 1; }
                    if depth == 0 {
                        return Some((Position { line: line_idx as usize, col: byte_i }, start_pos));
                    }
                }
                line_idx -= 1;
                if line_idx < 0 { break; }
                scan_from_end = true;
            }
        }

        None
    }
}
