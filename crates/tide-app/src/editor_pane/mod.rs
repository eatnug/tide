// Editor pane: wraps EditorState with rendering helpers (mirrors TerminalPane).

mod rendering;

use std::io;
use std::path::Path;

use tide_core::PaneId;
use tide_editor::input::EditorAction;
use tide_editor::EditorState;

use crate::pane::Selection;


/// Width of the gutter (line numbers) in cells.
const GUTTER_WIDTH_CELLS: usize = 5;

pub struct EditorPane {
    #[allow(dead_code)]
    pub id: PaneId,
    pub editor: EditorState,
    pub search: Option<crate::search::SearchState>,
    pub selection: Option<Selection>,
    pub disk_changed: bool,
    pub file_deleted: bool,
    pub diff_mode: bool,
    pub disk_content: Option<Vec<String>>,
}

impl EditorPane {
    pub fn new_empty(id: PaneId) -> Self {
        let editor = EditorState::new_empty();
        Self { id, editor, search: None, selection: None, disk_changed: false, file_deleted: false, diff_mode: false, disk_content: None }
    }

    pub fn open(id: PaneId, path: &Path) -> io::Result<Self> {
        let editor = EditorState::open(path)?;
        Ok(Self { id, editor, search: None, selection: None, disk_changed: false, file_deleted: false, diff_mode: false, disk_content: None })
    }

    /// Whether this pane needs a notification bar (diff mode or file deleted).
    pub fn needs_notification_bar(&self) -> bool {
        self.diff_mode || (self.file_deleted && self.disk_changed)
    }

    /// Handle an editor action (visible_cols defaults to 80 for scroll clamping).
    pub fn handle_action(&mut self, action: EditorAction, visible_rows: usize) {
        let is_scroll = matches!(action, EditorAction::ScrollUp(_) | EditorAction::ScrollDown(_) | EditorAction::ScrollLeft(_) | EditorAction::ScrollRight(_));
        self.editor.handle_action(action);
        if !is_scroll {
            self.editor.ensure_cursor_visible(visible_rows);
        }
        self.clamp_scroll(visible_rows);
        self.clamp_h_scroll(80);
    }

    /// Handle an editor action with both vertical and horizontal visibility.
    pub fn handle_action_with_size(&mut self, action: EditorAction, visible_rows: usize, visible_cols: usize) {
        let is_scroll = matches!(action, EditorAction::ScrollUp(_) | EditorAction::ScrollDown(_) | EditorAction::ScrollLeft(_) | EditorAction::ScrollRight(_));
        self.editor.handle_action(action);
        if !is_scroll {
            self.editor.ensure_cursor_visible(visible_rows);
            self.editor.ensure_cursor_visible_h(visible_cols);
        }
        self.clamp_scroll(visible_rows);
        self.clamp_h_scroll(visible_cols);
    }

    /// Prevent vertical over-scrolling: last line should stick to bottom.
    fn clamp_scroll(&mut self, visible_rows: usize) {
        let max_scroll = self.editor.buffer.line_count().saturating_sub(visible_rows);
        if self.editor.scroll_offset() > max_scroll {
            self.editor.set_scroll_offset(max_scroll);
        }
    }

    /// Prevent horizontal over-scrolling: end of longest line stays at right edge.
    fn clamp_h_scroll(&mut self, visible_cols: usize) {
        let max_len = self.editor.buffer.lines.iter().map(|l| l.chars().count()).max().unwrap_or(0);
        let max_h = max_len.saturating_sub(visible_cols);
        if self.editor.h_scroll_offset() > max_h {
            self.editor.set_h_scroll_offset(max_h);
        }
    }

    /// Get the file name for display in the tab bar.
    pub fn title(&self) -> String {
        self.editor.file_display_name()
    }

    /// Extract selected text from the editor buffer.
    pub fn selected_text(&self, sel: &Selection) -> String {
        let (start, end) = if sel.anchor < sel.end {
            (sel.anchor, sel.end)
        } else {
            (sel.end, sel.anchor)
        };

        let mut result = String::new();
        let line_count = self.editor.buffer.line_count();
        for row in start.0..=end.0 {
            if row >= line_count {
                break;
            }
            let line = match self.editor.buffer.line(row) {
                Some(l) => l,
                None => break,
            };
            let char_count = line.chars().count();
            let col_start = if row == start.0 { start.1.min(char_count) } else { 0 };
            let col_end = if row == end.0 { end.1.min(char_count) } else { char_count };
            if col_start <= col_end {
                // Get chars from col_start to col_end (both are character indices)
                let text: String = line.chars().skip(col_start).take(col_end - col_start).collect();
                result.push_str(&text);
            }
            if row != end.0 {
                result.push('\n');
            }
        }
        result
    }

    /// Get the generation counter for dirty checking.
    pub fn generation(&self) -> u64 {
        self.editor.generation()
    }
}
