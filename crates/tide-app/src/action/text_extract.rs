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
    /// Returns the resolved path and an optional line number (from `:42` suffix).
    pub(crate) fn extract_file_path_at(&self, pane_id: tide_core::PaneId, position: Vec2) -> Option<(PathBuf, Option<usize>)> {
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

        // Capture :line_number suffix (e.g., "file.rs:42")
        if end < chars.len() && chars[end] == ':' {
            let mut probe = end + 1;
            while probe < chars.len() && chars[probe].is_ascii_digit() {
                probe += 1;
            }
            if probe > end + 1 {
                end = probe;
            }
        }

        let segment: String = chars[start..end].iter().collect();
        let mut parts = segment.splitn(3, ':');
        let path_str = parts.next().unwrap_or("");
        let line_number: Option<usize> = parts.next().and_then(|s| s.parse().ok());

        if path_str.is_empty() || !path_str.contains('.') && !path_str.contains('/') {
            return None;
        }

        let path = std::path::Path::new(path_str);

        // If relative, resolve against terminal CWD
        let cwd = pane.backend.detect_cwd_fallback();
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else if let Some(ref cwd) = cwd {
            cwd.join(path)
        } else {
            return None;
        };

        // Return if the file exists at the resolved path
        if resolved.is_file() {
            return Some((resolved, line_number));
        }

        // If not found directly, search the project tree recursively
        if let Some(found) = self.find_file_in_project(pane_id, path_str) {
            return Some((found, line_number));
        }

        None
    }

    /// Search for a file by name/relative-path in the project root directory.
    fn find_file_in_project(&self, pane_id: tide_core::PaneId, filename: &str) -> Option<PathBuf> {
        let pane = match self.panes.get(&pane_id) {
            Some(PaneKind::Terminal(p)) => p,
            _ => return None,
        };
        let cwd = pane.backend.detect_cwd_fallback()?;
        let root = self.cached_repo_roots.get(&cwd)?.as_ref()?;
        Self::find_file_recursive(root, filename, 10)
    }

    /// Recursively search for a target file under `dir`, up to `max_depth`.
    /// If `target` contains `/`, match as a relative path suffix; otherwise match filename only.
    fn find_file_recursive(dir: &std::path::Path, target: &str, max_depth: usize) -> Option<PathBuf> {
        if max_depth == 0 {
            return None;
        }
        let has_slash = target.contains('/');
        let read_dir = std::fs::read_dir(dir).ok()?;
        let mut subdirs: Vec<PathBuf> = Vec::new();
        for entry in read_dir.flatten() {
            let path = entry.path();
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            // Skip hidden and common ignored directories
            if name_str.starts_with('.') || name_str == "node_modules" || name_str == "target" || name_str == "__pycache__" {
                continue;
            }

            if path.is_dir() {
                subdirs.push(path);
            } else if path.is_file() {
                if has_slash {
                    // Match as relative path suffix
                    if let Some(s) = path.to_str() {
                        if s.ends_with(target) {
                            let prefix_end = s.len() - target.len();
                            if prefix_end == 0 || s.as_bytes()[prefix_end - 1] == b'/' {
                                return Some(path);
                            }
                        }
                    }
                } else if name_str == target {
                    return Some(path);
                }
            }
        }
        for subdir in subdirs {
            if let Some(found) = Self::find_file_recursive(&subdir, target, max_depth - 1) {
                return Some(found);
            }
        }
        None
    }
}
