// Terminal pane: wraps a terminal backend with rendering helpers.

use tide_core::{Color, CursorShape, Key, Modifiers, Rect, Renderer, Size, TerminalBackend, Vec2};
use tide_renderer::WgpuRenderer;
use tide_terminal::Terminal;

use crate::editor_pane::EditorPane;
use crate::search::SearchState;

pub type PaneId = tide_core::PaneId;

/// Polymorphic pane: either a terminal or an editor.
pub enum PaneKind {
    Terminal(TerminalPane),
    Editor(EditorPane),
}

/// Text selection state (anchor = drag start, end = current position).
#[derive(Debug, Clone)]
pub struct Selection {
    pub anchor: (usize, usize), // (row, col)
    pub end: (usize, usize),    // (row, col)
}

pub struct TerminalPane {
    #[allow(dead_code)]
    pub id: PaneId,
    pub backend: Terminal,
    pub selection: Option<Selection>,
    pub search: Option<SearchState>,
}

impl TerminalPane {
    pub fn new(id: PaneId, cols: u16, rows: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let backend = Terminal::new(cols, rows)?;
        Ok(Self { id, backend, selection: None, search: None })
    }

    /// Extract selected text from the terminal grid.
    pub fn selected_text(&self, sel: &Selection) -> String {
        let grid = self.backend.grid();
        let (start, end) = if sel.anchor < sel.end {
            (sel.anchor, sel.end)
        } else {
            (sel.end, sel.anchor)
        };

        let mut result = String::new();
        for row in start.0..=end.0 {
            if row >= grid.cells.len() {
                break;
            }
            let line = &grid.cells[row];
            let col_start = if row == start.0 { start.1 } else { 0 };
            let col_end = if row == end.0 {
                end.1.min(line.len())
            } else {
                line.len()
            };
            for col in col_start..col_end {
                if col < line.len() {
                    let ch = line[col].character;
                    if ch != '\0' {
                        result.push(ch);
                    }
                }
            }
            if row != end.0 {
                // Trim trailing spaces from line before adding newline
                let trimmed = result.trim_end_matches(' ');
                result.truncate(trimmed.len());
                result.push('\n');
            }
        }
        result
    }

    /// Render the grid cells into the cached grid layer.
    pub fn render_grid(&self, rect: Rect, renderer: &mut WgpuRenderer) {
        let cell_size = renderer.cell_size();
        let grid = self.backend.grid();
        let offset = Vec2::new(rect.x, rect.y);

        // Clamp to the number of rows/cols that fit within the pane rect
        let max_rows = (rect.height / cell_size.height).ceil() as usize;
        let max_cols = (rect.width / cell_size.width).ceil() as usize;
        let rows = (grid.rows as usize).min(max_rows).min(grid.cells.len());
        let cols = (grid.cols as usize).min(max_cols);

        for row in 0..rows {
            for col in 0..cols {
                if col >= grid.cells[row].len() {
                    break;
                }
                let cell = &grid.cells[row][col];
                if cell.character == '\0'
                    || (cell.character == ' ' && cell.style.background.is_none())
                {
                    continue;
                }
                renderer.draw_grid_cell(cell.character, row, col, cell.style, cell_size, offset);
            }
        }
    }

    /// Render the cursor into the overlay layer (always redrawn).
    pub fn render_cursor(&self, rect: Rect, renderer: &mut WgpuRenderer) {
        let cell_size = renderer.cell_size();
        let cursor = self.backend.cursor();
        // Hide cursor when scrolled into history (cursor is at the prompt below viewport)
        if cursor.visible && self.backend.display_offset() == 0 {
            let cx = rect.x + cursor.col as f32 * cell_size.width;
            let cy = rect.y + cursor.row as f32 * cell_size.height;

            let cursor_color = Color::new(0.25, 0.5, 1.0, 0.9);
            match cursor.shape {
                CursorShape::Block => {
                    renderer.draw_rect(
                        Rect::new(cx, cy, cell_size.width, cell_size.height),
                        cursor_color,
                    );
                }
                CursorShape::Beam => {
                    renderer.draw_rect(Rect::new(cx, cy, 2.0, cell_size.height), cursor_color);
                }
                CursorShape::Underline => {
                    renderer.draw_rect(
                        Rect::new(cx, cy + cell_size.height - 2.0, cell_size.width, 2.0),
                        cursor_color,
                    );
                }
            }
        }
    }

    pub fn handle_key(&mut self, key: &Key, modifiers: &Modifiers) {
        let bytes = Terminal::key_to_bytes(key, modifiers);
        if !bytes.is_empty() {
            // Scroll back to bottom on user input (applied atomically during next grid sync)
            if self.backend.display_offset() > 0 {
                self.backend.request_scroll_to_bottom();
            }
            self.backend.write(&bytes);
        }
    }

    pub fn scroll_display(&mut self, delta: i32) {
        self.backend.scroll_display(delta);
    }

    pub fn resize_to_rect(&mut self, rect: Rect, cell_size: Size) {
        let cols = (rect.width / cell_size.width).max(1.0) as u16;
        let rows = (rect.height / cell_size.height).max(1.0) as u16;
        self.backend.resize(cols, rows);
    }
}
