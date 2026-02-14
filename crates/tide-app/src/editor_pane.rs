// Editor pane: wraps EditorState with rendering helpers (mirrors TerminalPane).

use std::io;
use std::path::Path;

use unicode_width::UnicodeWidthChar;

use tide_core::{Color, PaneId, Rect, Renderer, TextStyle, Vec2};
use tide_editor::input::EditorAction;
use tide_editor::EditorState;
use tide_renderer::WgpuRenderer;

use crate::pane::Selection;
use crate::search::SearchState;
use crate::theme::{SCROLLBAR_CURRENT_MATCH, SCROLLBAR_MATCH, SCROLLBAR_THUMB, SCROLLBAR_TRACK, SCROLLBAR_WIDTH};

/// Color for line numbers in the gutter.
const GUTTER_TEXT: Color = Color::new(0.40, 0.42, 0.50, 1.0);
/// Color for the current line number.
const GUTTER_ACTIVE_TEXT: Color = Color::new(0.70, 0.72, 0.80, 1.0);

/// Width of the gutter (line numbers) in cells.
const GUTTER_WIDTH_CELLS: usize = 5;

pub struct EditorPane {
    #[allow(dead_code)]
    pub id: PaneId,
    pub editor: EditorState,
    pub search: Option<SearchState>,
    pub selection: Option<Selection>,
}

impl EditorPane {
    pub fn new_empty(id: PaneId) -> Self {
        let editor = EditorState::new_empty();
        Self { id, editor, search: None, selection: None }
    }

    pub fn open(id: PaneId, path: &Path) -> io::Result<Self> {
        let editor = EditorState::open(path)?;
        Ok(Self { id, editor, search: None, selection: None })
    }

    /// Render the editor grid cells into the cached grid layer.
    pub fn render_grid(&self, rect: Rect, renderer: &mut WgpuRenderer) {
        let cell_size = renderer.cell_size();
        let gutter_width = GUTTER_WIDTH_CELLS as f32 * cell_size.width;
        let content_x = rect.x + gutter_width;
        let scrollbar_reserved = if self.needs_scrollbar(rect, cell_size.height) {
            SCROLLBAR_WIDTH
        } else {
            0.0
        };
        let content_width = (rect.width - gutter_width - scrollbar_reserved).max(0.0);

        let visible_rows = (rect.height / cell_size.height).floor() as usize;
        let scroll = self.editor.scroll_offset();
        let h_scroll = self.editor.h_scroll_offset();

        // Get highlighted lines
        let highlighted = self.editor.visible_highlighted_lines(visible_rows);
        let cursor_line = self.editor.cursor_position().line;

        for (vi, spans) in highlighted.iter().enumerate() {
            let abs_line = scroll + vi;
            let y = rect.y + vi as f32 * cell_size.height;

            if y + cell_size.height > rect.y + rect.height {
                break;
            }

            // Draw line number in gutter
            let line_num = format!("{:>4} ", abs_line + 1);
            let gutter_color = if abs_line == cursor_line {
                GUTTER_ACTIVE_TEXT
            } else {
                GUTTER_TEXT
            };
            let gutter_style = TextStyle {
                foreground: gutter_color,
                background: None,
                bold: false,
                dim: false,
                italic: false,
                underline: false,
            };
            for (ci, ch) in line_num.chars().enumerate() {
                if ch != ' ' {
                    renderer.draw_grid_cell(
                        ch,
                        vi,
                        ci,
                        gutter_style,
                        cell_size,
                        Vec2::new(rect.x, rect.y),
                    );
                }
            }

            // Draw syntax-highlighted content with horizontal scroll
            let mut char_idx = 0usize; // character index in the line
            let mut display_col = 0usize; // visual column offset from h_scroll start
            for span in spans {
                for ch in span.text.chars() {
                    if ch == '\n' {
                        continue;
                    }
                    let char_w = ch.width().unwrap_or(1);
                    // Skip characters before h_scroll (h_scroll is character-indexed)
                    if char_idx < h_scroll {
                        char_idx += 1;
                        continue;
                    }
                    let px = content_x + display_col as f32 * cell_size.width;
                    if px >= content_x + content_width {
                        break;
                    }
                    if ch != ' ' || span.style.background.is_some() {
                        renderer.draw_grid_cell(
                            ch,
                            vi,
                            GUTTER_WIDTH_CELLS + display_col,
                            span.style,
                            cell_size,
                            Vec2::new(rect.x, rect.y),
                        );
                    }
                    display_col += char_w;
                    char_idx += 1;
                }
            }
        }
    }

    /// Render the editor cursor into the overlay layer (always redrawn).
    pub fn render_cursor(&self, rect: Rect, renderer: &mut WgpuRenderer) {
        let cell_size = renderer.cell_size();
        let pos = self.editor.cursor_position();
        let scroll = self.editor.scroll_offset();
        let h_scroll = self.editor.h_scroll_offset();

        if pos.line < scroll {
            return;
        }
        if pos.col < h_scroll {
            return;
        }
        let visual_row = pos.line - scroll;
        // Compute visual column accounting for wide characters
        let visual_col_offset = if let Some(line_text) = self.editor.buffer.line(pos.line) {
            line_text.chars()
                .skip(h_scroll)
                .take(pos.col.saturating_sub(h_scroll))
                .map(|c| c.width().unwrap_or(1))
                .sum::<usize>()
        } else {
            pos.col - h_scroll
        };
        let visual_col = GUTTER_WIDTH_CELLS + visual_col_offset;

        let cx = rect.x + visual_col as f32 * cell_size.width;
        let cy = rect.y + visual_row as f32 * cell_size.height;

        // Check if cursor is within visible area
        if cy + cell_size.height > rect.y + rect.height {
            return;
        }
        let gutter_width = GUTTER_WIDTH_CELLS as f32 * cell_size.width;
        if cx > rect.x + rect.width || cx < rect.x + gutter_width {
            return;
        }

        let cursor_color = Color::new(0.25, 0.5, 1.0, 0.9);
        // Top layer so beam is visible above grid glyphs
        renderer.draw_top_rect(Rect::new(cx, cy, 2.0, cell_size.height), cursor_color);
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
        let name = self.editor.file_name().to_string();
        if self.editor.is_modified() {
            format!("{} \u{f111}", name) // dot indicator
        } else {
            name
        }
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
            let col_start = if row == start.0 { start.1.min(line.len()) } else { 0 };
            let col_end = if row == end.0 { end.1.min(line.len()) } else { line.len() };
            if col_start <= col_end {
                // Get chars from col_start to col_end
                let text: String = line.chars().skip(col_start).take(col_end - col_start).collect();
                result.push_str(&text);
            }
            if row != end.0 {
                result.push('\n');
            }
        }
        result
    }

    /// Whether the file is long enough to need a scrollbar.
    pub fn needs_scrollbar(&self, rect: Rect, cell_height: f32) -> bool {
        let visible_rows = (rect.height / cell_height).floor() as usize;
        self.editor.buffer.line_count() > visible_rows
    }

    /// Render a scrollbar on the right edge of the editor area.
    /// Includes match markers from search results when search is active.
    pub fn render_scrollbar(&self, rect: Rect, renderer: &mut WgpuRenderer, search: Option<&SearchState>) {
        let cell_size = renderer.cell_size();
        let visible_rows = (rect.height / cell_size.height).floor() as usize;
        let total_lines = self.editor.buffer.line_count();
        if total_lines <= visible_rows {
            return;
        }

        let track_x = rect.x + rect.width - SCROLLBAR_WIDTH;
        let track_rect = Rect::new(track_x, rect.y, SCROLLBAR_WIDTH, rect.height);

        // Track background
        renderer.draw_rect(track_rect, SCROLLBAR_TRACK);

        // Thumb
        let scroll = self.editor.scroll_offset();
        let thumb_ratio_start = scroll as f32 / total_lines as f32;
        let thumb_ratio_end = (scroll + visible_rows) as f32 / total_lines as f32;
        let thumb_y = rect.y + thumb_ratio_start * rect.height;
        let thumb_h = (thumb_ratio_end - thumb_ratio_start) * rect.height;
        let thumb_h = thumb_h.max(4.0); // minimum thumb height
        renderer.draw_rect(Rect::new(track_x, thumb_y, SCROLLBAR_WIDTH, thumb_h), SCROLLBAR_THUMB);

        // Search match markers
        if let Some(search) = search {
            if search.visible && !search.query.is_empty() {
                let marker_h = 2.0_f32;
                for (mi, m) in search.matches.iter().enumerate() {
                    let ratio = m.line as f32 / total_lines as f32;
                    let my = rect.y + (ratio * rect.height).min(rect.height - marker_h);
                    let color = if search.current == Some(mi) {
                        SCROLLBAR_CURRENT_MATCH
                    } else {
                        SCROLLBAR_MATCH
                    };
                    renderer.draw_rect(Rect::new(track_x, my, SCROLLBAR_WIDTH, marker_h), color);
                }
            }
        }
    }

    /// Get the generation counter for dirty checking.
    pub fn generation(&self) -> u64 {
        self.editor.generation()
    }
}
