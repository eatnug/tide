// Rendering methods for EditorPane: grid, cursor, and scrollbar.

use unicode_width::UnicodeWidthChar;

use tide_core::{Color, Rect, Renderer, TextStyle, Vec2};
use tide_renderer::WgpuRenderer;

use crate::search::SearchState;
use crate::theme::SCROLLBAR_WIDTH;

use crate::theme::ThemePalette;

use super::{EditorPane, GUTTER_WIDTH_CELLS};

impl EditorPane {
    /// Render the editor grid cells into the cached grid layer, with optional diff colors.
    pub fn render_grid_full(
        &self,
        rect: Rect,
        renderer: &mut WgpuRenderer,
        gutter_text: Color,
        gutter_active_text: Color,
        diff_added_bg: Option<Color>,
        diff_removed_bg: Option<Color>,
        diff_added_gutter: Option<Color>,
        diff_removed_gutter: Option<Color>,
    ) {
        if self.diff_mode {
            if let Some(ref disk_content) = self.disk_content {
                self.render_diff_grid(rect, renderer, gutter_text, disk_content,
                    diff_added_bg.unwrap_or(gutter_text),
                    diff_removed_bg.unwrap_or(gutter_text),
                    diff_added_gutter.unwrap_or(gutter_text),
                    diff_removed_gutter.unwrap_or(gutter_text),
                );
                return;
            }
        }

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
                gutter_active_text
            } else {
                gutter_text
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

    /// Render the diff view grid.
    fn render_diff_grid(
        &self,
        rect: Rect,
        renderer: &mut WgpuRenderer,
        gutter_text: Color,
        disk_content: &[String],
        added_bg: Color,
        removed_bg: Color,
        added_gutter: Color,
        removed_gutter: Color,
    ) {
        use crate::diff::{compute_diff, DiffOp};

        let cell_size = renderer.cell_size();
        let gutter_width = GUTTER_WIDTH_CELLS as f32 * cell_size.width;
        let content_x = rect.x + gutter_width;
        let content_width = (rect.width - gutter_width).max(0.0);

        let diff_ops = compute_diff(disk_content, &self.editor.buffer.lines);
        let visible_rows = (rect.height / cell_size.height).floor() as usize;
        let scroll = self.editor.scroll_offset();
        let h_scroll = self.editor.h_scroll_offset();

        // Render visible virtual lines
        for (vi, op) in diff_ops.iter().skip(scroll).take(visible_rows).enumerate() {
            let y = rect.y + vi as f32 * cell_size.height;
            if y + cell_size.height > rect.y + rect.height {
                break;
            }

            match op {
                DiffOp::Equal(buf_idx) | DiffOp::Insert(buf_idx) => {
                    let is_added = matches!(op, DiffOp::Insert(_));

                    // Draw full-row background rect for added lines
                    if is_added {
                        let row_rect = Rect::new(rect.x, y, rect.width, cell_size.height);
                        renderer.draw_grid_rect(row_rect, added_bg);
                    }

                    // Gutter: line number or + marker
                    let gutter_str = if is_added {
                        format!("{:>3}+ ", buf_idx + 1)
                    } else {
                        format!("{:>4} ", buf_idx + 1)
                    };
                    let gc = if is_added { added_gutter } else { gutter_text };
                    let gutter_style = TextStyle {
                        foreground: gc,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    for (ci, ch) in gutter_str.chars().enumerate().take(GUTTER_WIDTH_CELLS) {
                        if ch != ' ' {
                            renderer.draw_grid_cell(ch, vi, ci, gutter_style, cell_size, Vec2::new(rect.x, rect.y));
                        }
                    }

                    // Content
                    if let Some(line) = self.editor.buffer.line(*buf_idx) {
                        let text_style = TextStyle {
                            foreground: Color::new(0.88, 0.88, 0.88, 1.0),
                            background: None,
                            bold: false,
                            dim: false,
                            italic: false,
                            underline: false,
                        };
                        let mut char_idx = 0usize;
                        let mut display_col = 0usize;
                        for ch in line.chars() {
                            if ch == '\n' { continue; }
                            let char_w = ch.width().unwrap_or(1);
                            if char_idx < h_scroll { char_idx += 1; continue; }
                            let px = content_x + display_col as f32 * cell_size.width;
                            if px >= content_x + content_width { break; }
                            if ch != ' ' {
                                renderer.draw_grid_cell(ch, vi, GUTTER_WIDTH_CELLS + display_col, text_style, cell_size, Vec2::new(rect.x, rect.y));
                            }
                            display_col += char_w;
                            char_idx += 1;
                        }
                    }
                }
                DiffOp::Delete(disk_idx) => {
                    // Draw full-row background rect for removed lines
                    let row_rect = Rect::new(rect.x, y, rect.width, cell_size.height);
                    renderer.draw_grid_rect(row_rect, removed_bg);

                    // Gutter: - marker
                    let gutter_str = format!("{:>3}- ", disk_idx + 1);
                    let gutter_style = TextStyle {
                        foreground: removed_gutter,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    for (ci, ch) in gutter_str.chars().enumerate().take(GUTTER_WIDTH_CELLS) {
                        if ch != ' ' {
                            renderer.draw_grid_cell(ch, vi, ci, gutter_style, cell_size, Vec2::new(rect.x, rect.y));
                        }
                    }

                    // Content from disk
                    if let Some(line) = disk_content.get(*disk_idx) {
                        let text_style = TextStyle {
                            foreground: Color::new(0.65, 0.65, 0.65, 1.0),
                            background: None,
                            bold: false,
                            dim: true,
                            italic: false,
                            underline: false,
                        };
                        let mut char_idx = 0usize;
                        let mut display_col = 0usize;
                        for ch in line.chars() {
                            if ch == '\n' { continue; }
                            let char_w = ch.width().unwrap_or(1);
                            if char_idx < h_scroll { char_idx += 1; continue; }
                            let px = content_x + display_col as f32 * cell_size.width;
                            if px >= content_x + content_width { break; }
                            if ch != ' ' {
                                renderer.draw_grid_cell(ch, vi, GUTTER_WIDTH_CELLS + display_col, text_style, cell_size, Vec2::new(rect.x, rect.y));
                            }
                            display_col += char_w;
                            char_idx += 1;
                        }
                    }
                }
            }
        }
    }

    /// Render the editor cursor into the overlay layer (always redrawn).
    pub fn render_cursor(&self, rect: Rect, renderer: &mut WgpuRenderer, cursor_color: Color) {
        let cell_size = renderer.cell_size();
        let pos = self.editor.cursor_position();
        let scroll = self.editor.scroll_offset();
        let h_scroll = self.editor.h_scroll_offset();

        // In diff mode, map buffer cursor line to virtual diff line
        let visual_row = if self.diff_mode {
            if let Some(ref disk_content) = self.disk_content {
                use crate::diff::{compute_diff, DiffOp};
                let diff_ops = compute_diff(disk_content, &self.editor.buffer.lines);
                let mut vline = None;
                for (vi, op) in diff_ops.iter().enumerate() {
                    match op {
                        DiffOp::Equal(buf_idx) | DiffOp::Insert(buf_idx) => {
                            if *buf_idx == pos.line {
                                vline = Some(vi);
                                break;
                            }
                        }
                        DiffOp::Delete(_) => {}
                    }
                }
                match vline {
                    Some(vl) if vl >= scroll => vl - scroll,
                    _ => return,
                }
            } else {
                return;
            }
        } else {
            if pos.line < scroll {
                return;
            }
            pos.line - scroll
        };

        // Convert byte offset to char index for comparison with h_scroll (char-indexed)
        let cursor_char_col = if let Some(line_text) = self.editor.buffer.line(pos.line) {
            let byte_col = pos.col.min(line_text.len());
            line_text[..byte_col].chars().count()
        } else {
            0
        };
        if cursor_char_col < h_scroll {
            return;
        }
        // Compute visual column accounting for wide characters
        let visual_col_offset = if let Some(line_text) = self.editor.buffer.line(pos.line) {
            line_text.chars()
                .skip(h_scroll)
                .take(cursor_char_col - h_scroll)
                .map(|c| c.width().unwrap_or(1))
                .sum::<usize>()
        } else {
            cursor_char_col - h_scroll
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

        // Top layer so beam is visible above grid glyphs
        renderer.draw_top_rect(Rect::new(cx, cy, 2.0, cell_size.height), cursor_color);
    }

    /// Whether the file is long enough to need a scrollbar.
    pub fn needs_scrollbar(&self, rect: Rect, cell_height: f32) -> bool {
        let visible_rows = (rect.height / cell_height).floor() as usize;
        self.editor.buffer.line_count() > visible_rows
    }

    /// Render a scrollbar on the right edge of the editor area.
    /// Includes match markers from search results when search is active.
    pub fn render_scrollbar(&self, rect: Rect, renderer: &mut WgpuRenderer, search: Option<&SearchState>, palette: &ThemePalette) {
        let cell_size = renderer.cell_size();
        let visible_rows = (rect.height / cell_size.height).floor() as usize;
        let total_lines = self.editor.buffer.line_count();
        if total_lines <= visible_rows {
            return;
        }

        let track_x = rect.x + rect.width - SCROLLBAR_WIDTH;
        let track_rect = Rect::new(track_x, rect.y, SCROLLBAR_WIDTH, rect.height);

        // Track background
        renderer.draw_rect(track_rect, palette.scrollbar_track);

        // Thumb
        let scroll = self.editor.scroll_offset();
        let thumb_ratio_start = scroll as f32 / total_lines as f32;
        let thumb_ratio_end = (scroll + visible_rows) as f32 / total_lines as f32;
        let thumb_y = rect.y + thumb_ratio_start * rect.height;
        let thumb_h = (thumb_ratio_end - thumb_ratio_start) * rect.height;
        let thumb_h = thumb_h.max(4.0); // minimum thumb height
        renderer.draw_rect(Rect::new(track_x, thumb_y, SCROLLBAR_WIDTH, thumb_h), palette.scrollbar_thumb);

        // Search match markers
        if let Some(search) = search {
            if search.visible && !search.input.is_empty() {
                let marker_h = 2.0_f32;
                for (mi, m) in search.matches.iter().enumerate() {
                    let ratio = m.line as f32 / total_lines as f32;
                    let my = rect.y + (ratio * rect.height).min(rect.height - marker_h);
                    let color = if search.current == Some(mi) {
                        palette.scrollbar_current
                    } else {
                        palette.scrollbar_match
                    };
                    renderer.draw_rect(Rect::new(track_x, my, SCROLLBAR_WIDTH, marker_h), color);
                }
            }
        }
    }
}
