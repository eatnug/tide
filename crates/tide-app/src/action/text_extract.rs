use std::path::PathBuf;

use tide_core::{TerminalBackend, Vec2};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;

impl App {
    /// Try to extract a URL from the terminal grid at the given click position.
    /// Checks if the click is within a detected URL range and extracts the URL string.
    pub(crate) fn extract_url_at(&self, pane_id: tide_core::PaneId, position: Vec2) -> Option<String> {
        let pane = match self.panes.get(&pane_id) {
            Some(PaneKind::Terminal(p)) => p,
            _ => return None,
        };

        let (_, visual_rect) = self
            .visual_pane_rects
            .iter()
            .find(|(id, _)| *id == pane_id)?;
        let cell_size = self.cell_size();

        let content_top = self.pane_area_mode.content_top();
        let inner_x = visual_rect.x + PANE_PADDING;
        let inner_y = visual_rect.y + content_top;

        // Center offset matching render_grid
        let max_cols = ((visual_rect.width - 2.0 * PANE_PADDING) / cell_size.width).floor() as usize;
        let actual_width = max_cols as f32 * cell_size.width;
        let extra_x = ((visual_rect.width - 2.0 * PANE_PADDING) - actual_width) / 2.0;

        let col = ((position.x - inner_x - extra_x) / cell_size.width) as usize;
        let row = ((position.y - inner_y) / cell_size.height) as usize;

        let url_ranges = pane.backend.url_ranges();
        if row >= url_ranges.len() {
            return None;
        }

        // Check if click column is within a URL range
        for &(start_col, end_col) in &url_ranges[row] {
            if col >= start_col && col < end_col {
                // Extract URL text from grid cells
                let grid = pane.backend.grid();
                if row >= grid.cells.len() {
                    return None;
                }
                let line = &grid.cells[row];
                let url: String = line.iter()
                    .skip(start_col)
                    .take(end_col - start_col)
                    .map(|c| if c.character == '\0' { ' ' } else { c.character })
                    .collect();
                let url = url.trim().to_string();
                if !url.is_empty() {
                    return Some(url);
                }
            }
        }
        None
    }

    /// Try to extract a file path from the terminal grid at the given click position.
    /// Scans the clicked row for path-like text and resolves against the terminal's CWD.
    pub(crate) fn extract_file_path_at(&self, pane_id: tide_core::PaneId, position: Vec2) -> Option<PathBuf> {
        let pane = match self.panes.get(&pane_id) {
            Some(PaneKind::Terminal(p)) => p,
            _ => return None,
        };

        let (_, visual_rect) = self
            .visual_pane_rects
            .iter()
            .find(|(id, _)| *id == pane_id)?;
        let cell_size = self.cell_size();

        let content_top = self.pane_area_mode.content_top();
        let inner_x = visual_rect.x + PANE_PADDING;
        let inner_y = visual_rect.y + content_top;

        let col = ((position.x - inner_x) / cell_size.width) as usize;
        let row = ((position.y - inner_y) / cell_size.height) as usize;

        let grid = pane.backend.grid();
        if row >= grid.cells.len() {
            return None;
        }
        let line = &grid.cells[row];

        // Build the full text of the row
        let row_text: String = line.iter().map(|c| c.character).collect();
        let row_text = row_text.trim_end();

        if row_text.is_empty() {
            return None;
        }

        // Find the word/path segment under the cursor.
        // Expand left and right from the click column to find path-like characters.
        let chars: Vec<char> = row_text.chars().collect();
        if col >= chars.len() {
            return None;
        }

        let is_path_char = |c: char| -> bool {
            c.is_alphanumeric() || matches!(c, '/' | '\\' | '.' | '-' | '_' | '~')
        };

        let mut start = col;
        while start > 0 && is_path_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = col;
        while end < chars.len() && is_path_char(chars[end]) {
            end += 1;
        }

        // Also skip trailing colon+number (e.g., "file.rs:42")
        let segment: String = chars[start..end].iter().collect();
        let path_str = segment.split(':').next().unwrap_or(&segment);

        if path_str.is_empty() || !path_str.contains('.') && !path_str.contains('/') {
            return None;
        }

        let path = std::path::Path::new(path_str);

        // If relative, resolve against terminal CWD
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            let cwd = pane.backend.detect_cwd_fallback()?;
            cwd.join(path)
        };

        // Only return if the file actually exists
        if resolved.is_file() {
            Some(resolved)
        } else {
            None
        }
    }
}
