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
pub(crate) const GUTTER_WIDTH_CELLS: usize = 6;

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
    pub preview_h_scroll: usize,
    /// Last wrap_width passed to `ensure_preview_cache`, used to detect
    /// when the width has stabilised after a resize so we can defer the
    /// expensive markdown re-parse during continuous resize.
    preview_last_width: Option<usize>,
    /// Pending scroll ratio to apply after the preview cache is built (edit→preview).
    preview_scroll_pending_ratio: Option<f64>,
    /// Cached `is_modified()` for detecting transitions (drives tab ● indicator).
    pub last_is_modified: bool,
    /// Generation counter at the time of the last `is_modified()` check.
    /// Avoids expensive Vec<String> comparison every frame.
    pub last_checked_gen: u64,
}

impl EditorPane {
    pub fn new_empty(id: PaneId) -> Self {
        let editor = EditorState::new_empty();
        Self { id, editor, search: None, selection: None, disk_changed: false, file_deleted: false, diff_mode: false, disk_content: None, preview_mode: false, preview_cache: None, preview_scroll: 0, preview_h_scroll: 0, preview_last_width: None, preview_scroll_pending_ratio: None, last_is_modified: false, last_checked_gen: 0 }
    }

    pub fn open(id: PaneId, path: &Path) -> io::Result<Self> {
        let editor = EditorState::open(path)?;
        let is_markdown = path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| matches!(ext, "md" | "markdown" | "mdown" | "mkd"))
            .unwrap_or(false);
        Ok(Self { id, editor, search: None, selection: None, disk_changed: false, file_deleted: false, diff_mode: false, disk_content: None, preview_mode: is_markdown, preview_cache: None, preview_scroll: 0, preview_h_scroll: 0, preview_last_width: None, preview_scroll_pending_ratio: None, last_is_modified: false, last_checked_gen: 0 })
    }

    /// Whether this pane needs a notification bar (disk changed, diff mode, or file deleted).
    pub fn needs_notification_bar(&self) -> bool {
        self.disk_changed || self.diff_mode
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

    /// Extract selected text from the editor buffer or preview lines.
    /// In preview mode, selection coordinates refer to preview lines (display-cell indexed),
    /// so we read from the cached preview instead of the raw buffer.
    pub fn selected_text(&self, sel: &Selection) -> String {
        if self.preview_mode {
            return self.preview_selected_text(sel);
        }

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

    /// Extract selected text from cached preview lines.
    /// Selection columns are in display-cell units (CJK chars count as 2).
    fn preview_selected_text(&self, sel: &Selection) -> String {
        use unicode_width::UnicodeWidthChar;

        let preview_lines = match &self.preview_cache {
            Some((_, _, _, lines)) => lines,
            None => return String::new(),
        };

        let (start, end) = if sel.anchor < sel.end {
            (sel.anchor, sel.end)
        } else {
            (sel.end, sel.anchor)
        };

        let mut result = String::new();
        for row in start.0..=end.0 {
            if row >= preview_lines.len() {
                break;
            }
            let line = &preview_lines[row];
            // Flatten all spans into characters (excluding newlines)
            let chars: Vec<char> = line.spans.iter()
                .flat_map(|s| s.text.chars())
                .filter(|c| *c != '\n')
                .collect();

            let col_start = if row == start.0 { start.1 } else { 0 };
            let col_end = if row == end.0 {
                end.1
            } else {
                chars.iter().map(|c| c.width().unwrap_or(1)).sum()
            };

            // Walk characters by display width to extract the right range
            let mut display_col = 0usize;
            for &ch in &chars {
                let w = ch.width().unwrap_or(1);
                if display_col + w > col_end {
                    break;
                }
                if display_col >= col_start {
                    result.push(ch);
                }
                display_col += w;
            }

            if row != end.0 {
                result.push('\n');
            }
        }
        result
    }

    /// Select all text in the buffer (or preview lines in preview mode).
    pub fn select_all(&mut self) {
        if self.preview_mode {
            if let Some((_, _, _, ref lines)) = self.preview_cache {
                use unicode_width::UnicodeWidthChar;
                let last_line = lines.len().saturating_sub(1);
                let last_col = lines.get(last_line).map_or(0, |line| {
                    line.spans.iter()
                        .flat_map(|s| s.text.chars())
                        .filter(|c| *c != '\n')
                        .map(|c| c.width().unwrap_or(1))
                        .sum()
                });
                self.selection = Some(Selection {
                    anchor: (0, 0),
                    end: (last_line, last_col),
                });
            }
        } else {
            let last_line = self.editor.buffer.line_count().saturating_sub(1);
            let last_col = self.editor.buffer.line(last_line).map_or(0, |l| l.chars().count());
            self.selection = Some(Selection {
                anchor: (0, 0),
                end: (last_line, last_col),
            });
        }
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
            let cache_width = self.preview_cache.as_ref()
                .map(|(_, w, _, _)| *w as u64)
                .unwrap_or(0);
            self.editor.generation()
                .wrapping_add(self.preview_scroll as u64)
                .wrapping_add(self.preview_h_scroll as u64)
                .wrapping_add(cache_width)
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

    /// Toggle preview mode on/off, syncing scroll position proportionally.
    pub fn toggle_preview(&mut self) {
        let raw_line_count = self.editor.buffer.line_count().max(1);

        if self.preview_mode {
            // Preview → Edit: map preview_scroll to editor scroll
            let preview_total = self.preview_line_count().max(1);
            let ratio = self.preview_scroll as f64 / preview_total as f64;
            let target = (ratio * raw_line_count as f64).round() as usize;
            self.editor.set_scroll_offset(target.min(raw_line_count.saturating_sub(1)));
        } else {
            // Edit → Preview: save current scroll ratio to apply after cache is built
            let ratio = self.editor.scroll_offset() as f64 / raw_line_count as f64;
            // Store ratio temporarily; will be applied in ensure_preview_cache
            self.preview_scroll_pending_ratio = Some(ratio);
        }

        self.preview_mode = !self.preview_mode;
        self.preview_h_scroll = 0;
        self.preview_cache = None;
    }

    /// Ensure the preview cache is up to date.
    ///
    /// During continuous resize the wrap_width changes every frame.  Re-parsing
    /// the full markdown each time is expensive, so we defer when only the width
    /// changed.  The cache is rebuilt once the width stabilises (same value for
    /// two consecutive calls), which typically happens on the first frame after
    /// the resize stops — the deferred PTY-resize timer fires 50 ms later and
    /// triggers a redraw that picks this up.
    pub fn ensure_preview_cache(&mut self, wrap_width: usize, dark: bool) {
        let gen = self.editor.generation();
        if let Some((cached_gen, cached_width, cached_dark, _)) = &self.preview_cache {
            if *cached_gen == gen && *cached_width == wrap_width && *cached_dark == dark {
                return;
            }
        }

        // Detect mid-resize: width_stable is true only when the requested
        // wrap_width equals the value from the previous call.
        let width_stable = self.preview_last_width == Some(wrap_width);
        self.preview_last_width = Some(wrap_width);

        if !width_stable {
            // Width just changed — likely mid-resize.  If only the width
            // differs (content and theme unchanged), skip the expensive
            // recomputation and keep showing the stale cache.
            if let Some((cached_gen, _, cached_dark, _)) = &self.preview_cache {
                if *cached_gen == gen && *cached_dark == dark {
                    return;
                }
            }
        }

        let theme = if dark { MarkdownTheme::dark() } else { MarkdownTheme::light() };
        let lines = render_markdown_preview(&self.editor.buffer.lines, &theme, wrap_width);
        let line_count = lines.len();
        self.preview_cache = Some((gen, wrap_width, dark, lines));

        // Apply pending scroll ratio from edit→preview toggle
        if let Some(ratio) = self.preview_scroll_pending_ratio.take() {
            self.preview_scroll = (ratio * line_count as f64).round() as usize;
            if line_count > 0 {
                self.preview_scroll = self.preview_scroll.min(line_count.saturating_sub(1));
            }
        }
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

    /// Maximum display-width across all preview lines (for h-scroll clamping).
    pub fn preview_max_line_width(&self) -> usize {
        use unicode_width::UnicodeWidthChar;
        self.preview_lines().iter().map(|line| {
            line.spans.iter().map(|s| {
                s.text.chars().filter(|c| *c != '\n').map(|c| c.width().unwrap_or(1)).sum::<usize>()
            }).sum::<usize>()
        }).max().unwrap_or(0)
    }
}
