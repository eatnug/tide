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
    /// Deleted a range of text (e.g. selection delete). Stores the deleted lines for undo.
    DeleteRange { start: Position, end: Position, deleted_lines: Vec<String> },
    /// Inserted a block of text (e.g. paste). Stores the text and end position for undo.
    InsertText { pos: Position, text: String, end_pos: Position },
    /// Deleted an entire line.
    DeleteLine { line: usize, content: String },
    /// Swapped two adjacent lines.
    SwapLines { line_a: usize, line_b: usize },
}

impl Buffer {
    /// Undo the last edit. Returns the cursor position to restore, or None if nothing to undo.
    pub fn undo(&mut self) -> Option<Position> {
        let (op, cursor_before) = self.undo_stack.pop()?;
        let ok = match &op {
            EditOp::InsertChar { pos, .. } => {
                // Reverse of insert: remove the char
                if pos.line < self.lines.len() && pos.col < self.lines[pos.line].len() {
                    self.lines[pos.line].remove(pos.col);
                    true
                } else {
                    false
                }
            }
            EditOp::DeleteChar { pos, ch, merged_next } => {
                if pos.line >= self.lines.len() {
                    false
                } else if *merged_next {
                    // Reverse of merge: split line at pos.col
                    let col = pos.col.min(self.lines[pos.line].len());
                    let rest = self.lines[pos.line][col..].to_string();
                    self.lines[pos.line].truncate(col);
                    self.lines.insert(pos.line + 1, rest);
                    true
                } else {
                    // Reverse of delete: re-insert the char
                    let col = pos.col.min(self.lines[pos.line].len());
                    self.lines[pos.line].insert(col, *ch);
                    true
                }
            }
            EditOp::Backspace { original_pos, result_pos, ch, merged_line } => {
                if *merged_line {
                    // Reverse of line merge: split line at result_pos.col
                    if original_pos.line > 0 && original_pos.line - 1 < self.lines.len() {
                        let merge_line = original_pos.line - 1;
                        let col = result_pos.col.min(self.lines[merge_line].len());
                        let rest = self.lines[merge_line][col..].to_string();
                        self.lines[merge_line].truncate(col);
                        self.lines.insert(original_pos.line, rest);
                        true
                    } else {
                        false
                    }
                } else if let Some(c) = ch {
                    // Reverse of char deletion: re-insert at result_pos.col
                    if result_pos.line < self.lines.len() {
                        let col = result_pos.col.min(self.lines[result_pos.line].len());
                        self.lines[result_pos.line].insert(col, *c);
                        true
                    } else {
                        false
                    }
                } else {
                    true
                }
            }
            EditOp::InsertNewline { pos } => {
                // Reverse of newline: merge line+1 back into line
                if pos.line + 1 < self.lines.len() {
                    let next = self.lines.remove(pos.line + 1);
                    self.lines[pos.line].push_str(&next);
                    true
                } else {
                    false
                }
            }
            EditOp::DeleteRange { start, end: _, ref deleted_lines } => {
                if start.line >= self.lines.len() || deleted_lines.is_empty() {
                    false
                } else {
                    // Reverse of range delete: re-insert the deleted text.
                    let start_col = start.col.min(self.lines[start.line].len());
                    let suffix = self.lines[start.line][start_col..].to_string();
                    self.lines[start.line].truncate(start_col);
                    if deleted_lines.len() == 1 {
                        self.lines[start.line].push_str(&deleted_lines[0]);
                        self.lines[start.line].push_str(&suffix);
                    } else {
                        self.lines[start.line].push_str(&deleted_lines[0]);
                        for (i, line) in deleted_lines[1..deleted_lines.len() - 1].iter().enumerate() {
                            self.lines.insert(start.line + 1 + i, line.clone());
                        }
                        let last_idx = start.line + deleted_lines.len() - 1;
                        let mut last_line = deleted_lines.last().unwrap().clone();
                        last_line.push_str(&suffix);
                        self.lines.insert(last_idx, last_line);
                    }
                    true
                }
            }
            EditOp::InsertText { pos, ref text, end_pos } => {
                // Reverse of text insert: delete the range [pos..end_pos]
                if pos.line >= self.lines.len() {
                    false
                } else {
                    let end_line = end_pos.line.min(self.lines.len() - 1);
                    let end_col = end_pos.col.min(self.lines[end_line].len());
                    let start_col = pos.col.min(self.lines[pos.line].len());
                    if pos.line == end_line {
                        self.lines[pos.line].drain(start_col..end_col);
                    } else {
                        let suffix = self.lines[end_line][end_col..].to_string();
                        self.lines[pos.line].truncate(start_col);
                        self.lines[pos.line].push_str(&suffix);
                        if pos.line + 1 <= end_line {
                            self.lines.drain((pos.line + 1)..=end_line);
                        }
                    }
                    let _ = text;
                    true
                }
            }
            EditOp::DeleteLine { line, ref content } => {
                // Reverse of delete line: re-insert the line
                if self.lines.len() == 1 && self.lines[0].is_empty() && *line == 0 {
                    self.lines[0] = content.clone();
                } else {
                    self.lines.insert(*line, content.clone());
                }
                true
            }
            EditOp::SwapLines { line_a, line_b } => {
                // Reverse of swap: swap back
                if *line_a < self.lines.len() && *line_b < self.lines.len() {
                    self.lines.swap(*line_a, *line_b);
                    true
                } else {
                    false
                }
            }
        };
        if ok {
            self.redo_stack.push((op, cursor_before));
            self.generation += 1;
            Some(cursor_before)
        } else {
            // Buffer state is out of sync with undo history; drop the op silently.
            None
        }
    }

    /// Redo the last undone edit. Returns the new cursor position, or None if nothing to redo.
    pub fn redo(&mut self) -> Option<Position> {
        let (op, cursor_before) = self.redo_stack.pop()?;
        let new_cursor = match &op {
            EditOp::InsertChar { pos, ch } => {
                if pos.line < self.lines.len() {
                    let col = pos.col.min(self.lines[pos.line].len());
                    self.lines[pos.line].insert(col, *ch);
                    Some(Position { line: pos.line, col: col + ch.len_utf8() })
                } else {
                    None
                }
            }
            EditOp::DeleteChar { pos, ch, merged_next } => {
                if pos.line >= self.lines.len() {
                    None
                } else if *merged_next {
                    if pos.line + 1 < self.lines.len() {
                        let next = self.lines.remove(pos.line + 1);
                        self.lines[pos.line].push_str(&next);
                        Some(*pos)
                    } else {
                        None
                    }
                } else {
                    let col = pos.col.min(self.lines[pos.line].len());
                    if col < self.lines[pos.line].len() {
                        self.lines[pos.line].remove(col);
                        let _ = ch;
                        Some(*pos)
                    } else {
                        None
                    }
                }
            }
            EditOp::Backspace { result_pos, ch, merged_line, original_pos } => {
                if *merged_line {
                    if original_pos.line < self.lines.len() && original_pos.line > 0 {
                        let current = self.lines.remove(original_pos.line);
                        self.lines[original_pos.line - 1].push_str(&current);
                        Some(*result_pos)
                    } else {
                        None
                    }
                } else if ch.is_some() {
                    if result_pos.line < self.lines.len() {
                        let col = result_pos.col.min(self.lines[result_pos.line].len());
                        if col < self.lines[result_pos.line].len() {
                            self.lines[result_pos.line].remove(col);
                            Some(*result_pos)
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    Some(*result_pos)
                }
            }
            EditOp::InsertNewline { pos } => {
                if pos.line < self.lines.len() {
                    let col = pos.col.min(self.lines[pos.line].len());
                    let rest = self.lines[pos.line][col..].to_string();
                    self.lines[pos.line].truncate(col);
                    self.lines.insert(pos.line + 1, rest);
                    Some(Position { line: pos.line + 1, col: 0 })
                } else {
                    None
                }
            }
            EditOp::DeleteRange { start, end, .. } => {
                if start.line >= self.lines.len() {
                    None
                } else {
                    // Re-apply the range deletion
                    let end_line = end.line.min(self.lines.len() - 1);
                    let end_col = end.col.min(self.lines[end_line].len());
                    let start_col = start.col.min(self.lines[start.line].len());
                    if start.line == end_line {
                        if start_col <= end_col {
                            self.lines[start.line].drain(start_col..end_col);
                        }
                    } else {
                        let suffix = self.lines[end_line][end_col..].to_string();
                        self.lines[start.line].truncate(start_col);
                        self.lines[start.line].push_str(&suffix);
                        if start.line + 1 <= end_line {
                            self.lines.drain((start.line + 1)..=end_line);
                        }
                    }
                    Some(*start)
                }
            }
            EditOp::InsertText { pos, ref text, .. } => {
                // Re-apply the text insertion
                if pos.line >= self.lines.len() {
                    None
                } else {
                    let col = pos.col.min(self.lines[pos.line].len());
                    let suffix = self.lines[pos.line][col..].to_string();
                    self.lines[pos.line].truncate(col);
                    let text_lines: Vec<&str> = text.split('\n').collect();
                    if text_lines.len() == 1 {
                        self.lines[pos.line].push_str(text_lines[0]);
                        let end_col = self.lines[pos.line].len();
                        self.lines[pos.line].push_str(&suffix);
                        Some(Position { line: pos.line, col: end_col })
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
                        Some(Position { line: last_idx, col: end_col })
                    }
                }
            }
            EditOp::DeleteLine { line, .. } => {
                // Re-apply: delete the line again
                if *line < self.lines.len() {
                    if self.lines.len() == 1 {
                        self.lines[0] = String::new();
                    } else {
                        self.lines.remove(*line);
                    }
                    let new_line = (*line).min(self.lines.len().saturating_sub(1));
                    Some(Position { line: new_line, col: 0 })
                } else {
                    None
                }
            }
            EditOp::SwapLines { line_a, line_b } => {
                // Re-apply: swap again
                if *line_a < self.lines.len() && *line_b < self.lines.len() {
                    self.lines.swap(*line_a, *line_b);
                    // Determine cursor: if original was swap_up (cursor was at line_b),
                    // cursor goes to line_a; if swap_down (cursor at line_a), goes to line_b
                    let cursor_line = if cursor_before.line == *line_b { *line_a } else { *line_b };
                    Some(Position { line: cursor_line, col: cursor_before.col })
                } else {
                    None
                }
            }
        };
        if let Some(cursor) = new_cursor {
            self.undo_stack.push((op, cursor_before));
            self.generation += 1;
            Some(cursor)
        } else {
            // Buffer state is out of sync with redo history; drop the op silently.
            None
        }
    }
}
