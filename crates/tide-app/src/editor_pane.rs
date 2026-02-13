// Editor pane: wraps EditorState with rendering helpers (mirrors TerminalPane).

use std::io;
use std::path::Path;

use tide_core::{Color, PaneId, Rect, Renderer, TextStyle, Vec2};
use tide_editor::input::EditorAction;
use tide_editor::EditorState;
use tide_renderer::WgpuRenderer;

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
}

impl EditorPane {
    pub fn open(id: PaneId, path: &Path) -> io::Result<Self> {
        let editor = EditorState::open(path)?;
        Ok(Self { id, editor })
    }

    /// Render the editor grid cells into the cached grid layer.
    pub fn render_grid(&self, rect: Rect, renderer: &mut WgpuRenderer) {
        let cell_size = renderer.cell_size();
        let gutter_width = GUTTER_WIDTH_CELLS as f32 * cell_size.width;
        let content_x = rect.x + gutter_width;
        let content_width = (rect.width - gutter_width).max(0.0);

        let visible_rows = (rect.height / cell_size.height).floor() as usize;
        let scroll = self.editor.scroll_offset();

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

            // Draw syntax-highlighted content
            let mut col = 0usize;
            for span in spans {
                for ch in span.text.chars() {
                    if ch == '\n' {
                        continue;
                    }
                    let px = content_x + col as f32 * cell_size.width;
                    if px >= content_x + content_width {
                        break;
                    }
                    if ch != ' ' || span.style.background.is_some() {
                        renderer.draw_grid_cell(
                            ch,
                            vi,
                            GUTTER_WIDTH_CELLS + col,
                            span.style,
                            cell_size,
                            Vec2::new(rect.x, rect.y),
                        );
                    }
                    col += 1;
                }
            }
        }
    }

    /// Render the editor cursor into the overlay layer (always redrawn).
    pub fn render_cursor(&self, rect: Rect, renderer: &mut WgpuRenderer) {
        let cell_size = renderer.cell_size();
        let pos = self.editor.cursor_position();
        let scroll = self.editor.scroll_offset();

        if pos.line < scroll {
            return;
        }
        let visual_row = pos.line - scroll;
        let visual_col = GUTTER_WIDTH_CELLS + pos.col;

        let cx = rect.x + visual_col as f32 * cell_size.width;
        let cy = rect.y + visual_row as f32 * cell_size.height;

        // Check if cursor is within visible area
        if cy + cell_size.height > rect.y + rect.height {
            return;
        }

        let cursor_color = Color::new(0.25, 0.5, 1.0, 0.9);
        // Always use beam cursor for editor
        renderer.draw_rect(Rect::new(cx, cy, 2.0, cell_size.height), cursor_color);
    }

    /// Handle an editor action.
    pub fn handle_action(&mut self, action: EditorAction, visible_rows: usize) {
        let is_scroll = matches!(action, EditorAction::ScrollUp(_) | EditorAction::ScrollDown(_));
        self.editor.handle_action(action);
        if !is_scroll {
            self.editor.ensure_cursor_visible(visible_rows);
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

    /// Get the generation counter for dirty checking.
    pub fn generation(&self) -> u64 {
        self.editor.generation()
    }
}
