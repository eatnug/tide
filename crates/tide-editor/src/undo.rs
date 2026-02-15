// Undo/redo subsystem for the editor buffer.

use crate::buffer::{Buffer, Position};

/// A single reversible edit operation.
#[derive(Debug, Clone)]
pub(crate) enum EditOp {
    /// Inserted a char at position.
    InsertChar { pos: Position, ch: char },
    /// Deleted a char at position (forward delete).
    DeleteChar { pos: Position, ch: char, merged_next: bool },
    /// Backspace: deleted char before position. `result_pos` is the cursor after backspace.
    Backspace { original_pos: Position, result_pos: Position, ch: Option<char>, merged_line: bool },
    /// Inserted a newline at position.
    InsertNewline { pos: Position },
}

impl Buffer {
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
        self.generation += 1;
        Some(cursor_before)
    }

    /// Redo the last undone edit. Returns the new cursor position, or None if nothing to redo.
    pub fn redo(&mut self) -> Option<Position> {
        let (op, cursor_before) = self.redo_stack.pop()?;
        let new_cursor = match &op {
            EditOp::InsertChar { pos, ch } => {
                self.lines[pos.line].insert(pos.col, *ch);
                Position { line: pos.line, col: pos.col + ch.len_utf8() }
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
        self.generation += 1;
        Some(new_cursor)
    }
}
