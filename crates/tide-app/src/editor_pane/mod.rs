// Editor pane: wraps EditorState with rendering helpers (mirrors TerminalPane).

mod rendering;

use std::io;
use std::path::Path;

use tide_core::PaneId;
use tide_editor::input::EditorAction;
use tide_editor::EditorState;

use tide_editor::markdown::{PreviewLine, render_markdown_preview, MarkdownTheme};

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
    pub preview_mode: bool,
    preview_cache: Option<(u64, usize, bool, Vec<PreviewLine>)>,
    pub preview_scroll: usize,
}

impl EditorPane {
    pub fn new_empty(id: PaneId) -> Self {
        let editor = EditorState::new_empty();
        Self { id, editor, search: None, selection: None, disk_changed: false, file_deleted: false, diff_mode: false, disk_content: None, preview_mode: false, preview_cache: None, preview_scroll: 0 }
    }

    pub fn open(id: PaneId, path: &Path) -> io::Result<Self> {
        let editor = EditorState::open(path)?;
        let is_markdown = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "md" | "markdown" | "mdown" | "mkd"))
            .unwrap_or(false);
        Ok(Self { id, editor, search: None, selection: None, disk_changed: false, file_deleted: false, diff_mode: false, disk_content: None, preview_mode: is_markdown, preview_cache: None, preview_scroll: 0 })
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
    /// h_scroll_offset is character-indexed, visible_cols is in display cells.
    fn clamp_h_scroll(&mut self, visible_cols: usize) {
        use unicode_width::UnicodeWidthChar;
        // For each line, find the max character offset such that remaining chars fit in visible_cols.
        let max_scroll = self.editor.buffer.lines.iter().map(|l| {
            let display_width: usize = l.chars().map(|c| c.width().unwrap_or(1)).sum();
            if display_width <= visible_cols {
                return 0;
            }
            // Walk from the end to find how many chars fit in visible_cols
            let total_chars = l.chars().count();
            let mut width_from_end = 0;
            let mut chars_from_end = 0;
            for ch in l.chars().rev() {
                let w = ch.width().unwrap_or(1);
                if width_from_end + w > visible_cols {
                    break;
                }
                width_from_end += w;
                chars_from_end += 1;
            }
            total_chars - chars_from_end
        }).max().unwrap_or(0);
        if self.editor.h_scroll_offset() > max_scroll {
            self.editor.set_h_scroll_offset(max_scroll);
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

    /// Select all text in the buffer.
    pub fn select_all(&mut self) {
        let last_line = self.editor.buffer.line_count().saturating_sub(1);
        let last_col = self.editor.buffer.line(last_line).map_or(0, |l| l.chars().count());
        self.selection = Some(Selection {
            anchor: (0, 0),
            end: (last_line, last_col),
        });
    }

    /// Convert a selection (char-indexed) to byte-offset positions for buffer operations.
    /// Returns (start, end) where start <= end in document order.
    pub fn selection_byte_range(&self, sel: &Selection) -> (tide_editor::EditorPosition, tide_editor::EditorPosition) {
        let (start, end) = if sel.anchor <= sel.end {
            (sel.anchor, sel.end)
        } else {
            (sel.end, sel.anchor)
        };
        let start_byte = self.char_col_to_byte(start.0, start.1);
        let end_byte = self.char_col_to_byte(end.0, end.1);
        (
            tide_editor::EditorPosition { line: start.0, col: start_byte },
            tide_editor::EditorPosition { line: end.0, col: end_byte },
        )
    }

    /// Convert a character column index to a byte offset for a given line.
    fn char_col_to_byte(&self, line: usize, char_col: usize) -> usize {
        if let Some(text) = self.editor.buffer.line(line) {
            text.char_indices()
                .nth(char_col)
                .map(|(i, _)| i)
                .unwrap_or(text.len())
        } else {
            0
        }
    }

    /// Delete the current selection, clear it, and set cursor to start.
    /// Returns true if a selection was deleted.
    pub fn delete_selection(&mut self) -> bool {
        if let Some(sel) = self.selection.take() {
            let (start, end) = self.selection_byte_range(&sel);
            let new_pos = self.editor.buffer.delete_range(start, end);
            self.editor.cursor.set_position(new_pos);
            true
        } else {
            false
        }
    }

    /// Get the generation counter for dirty checking.
    pub fn generation(&self) -> u64 {
        if self.preview_mode {
            self.editor.generation().wrapping_add(self.preview_scroll as u64)
        } else {
            self.editor.generation()
        }
    }

    /// Check if this file is a markdown file.
    pub fn is_markdown(&self) -> bool {
        self.editor.file_path()
            .and_then(|p| p.extension())
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "md" | "markdown" | "mdown" | "mkd"))
            .unwrap_or(false)
    }

    /// Toggle preview mode on/off.
    pub fn toggle_preview(&mut self) {
        self.preview_mode = !self.preview_mode;
        self.preview_scroll = 0;
        self.preview_cache = None;
    }

    /// Ensure the preview cache is up to date.
    pub fn ensure_preview_cache(&mut self, wrap_width: usize, dark: bool) {
        let gen = self.editor.generation();
        if let Some((cached_gen, cached_width, cached_dark, _)) = &self.preview_cache {
            if *cached_gen == gen && *cached_width == wrap_width && *cached_dark == dark {
                return;
            }
        }
        let theme = if dark { MarkdownTheme::dark() } else { MarkdownTheme::light() };
        let lines = render_markdown_preview(&self.editor.buffer.lines, &theme, wrap_width);
        self.preview_cache = Some((gen, wrap_width, dark, lines));
    }

    /// Get a reference to the cached preview lines.
    pub fn preview_lines(&self) -> &[PreviewLine] {
        match &self.preview_cache {
            Some((_, _, _, lines)) => lines,
            None => &[],
        }
    }

    /// Total number of preview lines (for scroll clamping).
    pub fn preview_line_count(&self) -> usize {
        match &self.preview_cache {
            Some((_, _, _, lines)) => lines.len(),
            None => 0,
        }
    }
}
