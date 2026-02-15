// Diff pane: displays git-changed files with inline unified diffs.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use tide_core::{Color, PaneId, Rect, Renderer, TextStyle, Vec2};
use tide_renderer::WgpuRenderer;
use tide_terminal::git;

/// A line in a unified diff.
#[derive(Debug, Clone)]
pub enum DiffLine {
    Context(String),
    Added(String),
    Removed(String),
    Header(String),
}

/// A file entry in the diff pane.
#[derive(Debug, Clone)]
pub struct DiffFileEntry {
    pub status: String,
    pub path: String,
    pub additions: usize,
    pub deletions: usize,
}

pub struct DiffPane {
    pub id: PaneId,
    pub cwd: PathBuf,
    pub files: Vec<DiffFileEntry>,
    pub expanded: HashSet<usize>,
    pub diff_cache: HashMap<usize, Vec<DiffLine>>,
    pub scroll: f32,
    pub scroll_target: f32,
    pub selected: Option<usize>,
    pub generation: u64,
}

impl DiffPane {
    pub fn new(id: PaneId, cwd: PathBuf) -> Self {
        let mut dp = Self {
            id,
            cwd,
            files: Vec::new(),
            expanded: HashSet::new(),
            diff_cache: HashMap::new(),
            scroll: 0.0,
            scroll_target: 0.0,
            selected: None,
            generation: 1,
        };
        dp.refresh();
        dp
    }

    /// Reload file list from git status.
    pub fn refresh(&mut self) {
        let entries = git::status_files(&self.cwd);

        // Get numstat for additions/deletions
        let numstat = self.load_numstat();

        self.files = entries
            .into_iter()
            .map(|e| {
                let (add, del) = numstat.get(&e.path).copied().unwrap_or((0, 0));
                DiffFileEntry {
                    status: e.status.clone(),
                    path: e.path,
                    additions: add,
                    deletions: del,
                }
            })
            .collect();

        // Auto-expand all files and preload their diffs
        self.expanded.clear();
        self.diff_cache.clear();
        for i in 0..self.files.len() {
            let lines = self.load_diff_lines(&self.files[i].path.clone());
            self.diff_cache.insert(i, lines);
            self.expanded.insert(i);
        }
        self.generation = self.generation.wrapping_add(1);
    }

    fn load_numstat(&self) -> HashMap<String, (usize, usize)> {
        let mut map = HashMap::new();
        if let Ok(output) = std::process::Command::new("git")
            .args(["diff", "--numstat"])
            .current_dir(&self.cwd)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
        {
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                for line in text.lines() {
                    let parts: Vec<&str> = line.split('\t').collect();
                    if parts.len() >= 3 {
                        let add = parts[0].parse().unwrap_or(0);
                        let del = parts[1].parse().unwrap_or(0);
                        map.insert(parts[2].to_string(), (add, del));
                    }
                }
            }
        }
        map
    }

    /// Toggle expand/collapse of a file entry.
    pub fn toggle_expand(&mut self, index: usize) {
        if self.expanded.contains(&index) {
            self.expanded.remove(&index);
        } else {
            // Lazily load diff
            if !self.diff_cache.contains_key(&index) {
                if let Some(entry) = self.files.get(index) {
                    let lines = self.load_diff_lines(&entry.path);
                    self.diff_cache.insert(index, lines);
                }
            }
            self.expanded.insert(index);
        }
        self.generation = self.generation.wrapping_add(1);
    }

    fn load_diff_lines(&self, path: &str) -> Vec<DiffLine> {
        match git::file_diff(&self.cwd, path) {
            Some(diff_text) => {
                diff_text
                    .lines()
                    .filter_map(|l| {
                        if l.starts_with("@@") {
                            Some(DiffLine::Header(l.to_string()))
                        } else if l.starts_with('+') && !l.starts_with("+++") {
                            Some(DiffLine::Added(l[1..].to_string()))
                        } else if l.starts_with('-') && !l.starts_with("---") {
                            Some(DiffLine::Removed(l[1..].to_string()))
                        } else if !l.starts_with("diff ") && !l.starts_with("index ") && !l.starts_with("---") && !l.starts_with("+++") {
                            Some(DiffLine::Context(l.to_string()))
                        } else {
                            None
                        }
                    })
                    .collect()
            }
            None => Vec::new(),
        }
    }

    /// Total lines for the diff pane (file entries + expanded diff lines).
    pub fn total_lines(&self) -> usize {
        let mut count = 0;
        for (i, _) in self.files.iter().enumerate() {
            count += 1; // file entry
            if self.expanded.contains(&i) {
                if let Some(lines) = self.diff_cache.get(&i) {
                    count += lines.len();
                }
            }
        }
        count
    }

    /// Summary stats across all files.
    pub fn total_stats(&self) -> (usize, usize) {
        let add: usize = self.files.iter().map(|f| f.additions).sum();
        let del: usize = self.files.iter().map(|f| f.deletions).sum();
        (add, del)
    }

    /// Render the diff pane content into the grid layer.
    pub fn render_grid(
        &self,
        rect: Rect,
        renderer: &mut WgpuRenderer,
        text_color: Color,
        dimmed_color: Color,
        added_bg: Color,
        removed_bg: Color,
        added_gutter: Color,
        removed_gutter: Color,
    ) {
        let cell_size = renderer.cell_size();
        let visible_rows = (rect.height / cell_size.height).floor() as usize;
        let scroll = self.scroll as usize;

        let mut row_idx = 0usize; // global virtual row
        let mut vi = 0usize; // visual row being drawn

        for (fi, file) in self.files.iter().enumerate() {
            // File header row
            if row_idx >= scroll && vi < visible_rows {
                let y = rect.y + vi as f32 * cell_size.height;
                let is_expanded = self.expanded.contains(&fi);
                let is_selected = self.selected == Some(fi);

                // Selection highlight
                if is_selected {
                    let sel_rect = Rect::new(rect.x, y, rect.width, cell_size.height);
                    renderer.draw_grid_rect(sel_rect, Color::new(1.0, 1.0, 1.0, 0.05));
                }

                // Expand indicator
                let indicator = if is_expanded { "\u{f0d7} " } else { "\u{f0da} " }; // ▾ / ▸
                let ind_style = TextStyle {
                    foreground: dimmed_color,
                    background: None,
                    bold: false,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                for (ci, ch) in indicator.chars().enumerate() {
                    renderer.draw_grid_cell(ch, vi, ci, ind_style, cell_size, Vec2::new(rect.x, rect.y));
                }

                // Status indicator
                let status_color = match file.status.trim() {
                    "M" | " M" => added_gutter,
                    "D" | " D" => removed_gutter,
                    "A" | "??" => added_gutter,
                    _ => text_color,
                };
                let status_display = format!("[{}] ", file.status.trim());
                let status_style = TextStyle {
                    foreground: status_color,
                    background: None,
                    bold: true,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                let col_offset = 2;
                for (ci, ch) in status_display.chars().enumerate() {
                    renderer.draw_grid_cell(ch, vi, col_offset + ci, status_style, cell_size, Vec2::new(rect.x, rect.y));
                }

                // File path
                let path_offset = col_offset + status_display.chars().count();
                let path_style = TextStyle {
                    foreground: text_color,
                    background: None,
                    bold: is_selected,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                for (ci, ch) in file.path.chars().enumerate() {
                    let col = path_offset + ci;
                    if (col as f32) * cell_size.width > rect.width - 80.0 {
                        break;
                    }
                    renderer.draw_grid_cell(ch, vi, col, path_style, cell_size, Vec2::new(rect.x, rect.y));
                }

                // Stats at end of line
                if file.additions > 0 || file.deletions > 0 {
                    let stats = format!("+{} -{}", file.additions, file.deletions);
                    let stats_chars: Vec<char> = stats.chars().collect();
                    let max_cols = (rect.width / cell_size.width).floor() as usize;
                    let start_col = max_cols.saturating_sub(stats_chars.len() + 1);
                    for (ci, &ch) in stats_chars.iter().enumerate() {
                        let col = start_col + ci;
                        let c = if ch == '+' || (ci > 0 && stats_chars[ci - 1] == '+') {
                            added_gutter
                        } else {
                            removed_gutter
                        };
                        // Simple: + section is green, - section is red
                        let is_add_section = stats[..stats.find('-').unwrap_or(stats.len())].contains(ch) && ci <= stats.find('-').unwrap_or(stats.len());
                        let color = if ci < stats.find('-').unwrap_or(stats.len()) {
                            added_gutter
                        } else {
                            removed_gutter
                        };
                        let _ = c; // suppress warning
                        let stat_style = TextStyle {
                            foreground: color,
                            background: None,
                            bold: false,
                            dim: false,
                            italic: false,
                            underline: false,
                        };
                        let _ = is_add_section;
                        renderer.draw_grid_cell(ch, vi, col, stat_style, cell_size, Vec2::new(rect.x, rect.y));
                    }
                }

                vi += 1;
            } else if row_idx >= scroll {
                // past visible area
            }
            row_idx += 1;

            // Expanded diff lines
            if self.expanded.contains(&fi) {
                if let Some(lines) = self.diff_cache.get(&fi) {
                    for line in lines {
                        if row_idx >= scroll && vi < visible_rows {
                            let y = rect.y + vi as f32 * cell_size.height;
                            let (text, fg, bg) = match line {
                                DiffLine::Added(t) => (t.as_str(), added_gutter, Some(added_bg)),
                                DiffLine::Removed(t) => (t.as_str(), removed_gutter, Some(removed_bg)),
                                DiffLine::Header(t) => (t.as_str(), dimmed_color, None),
                                DiffLine::Context(t) => (t.as_str(), dimmed_color, None),
                            };

                            // Background for added/removed
                            if let Some(bg_color) = bg {
                                renderer.draw_grid_rect(
                                    Rect::new(rect.x, y, rect.width, cell_size.height),
                                    bg_color,
                                );
                            }

                            // Gutter indicator
                            let gutter_ch = match line {
                                DiffLine::Added(_) => '+',
                                DiffLine::Removed(_) => '-',
                                DiffLine::Header(_) => '@',
                                DiffLine::Context(_) => ' ',
                            };
                            let gutter_style = TextStyle {
                                foreground: fg,
                                background: None,
                                bold: false,
                                dim: false,
                                italic: false,
                                underline: false,
                            };
                            renderer.draw_grid_cell(gutter_ch, vi, 2, gutter_style, cell_size, Vec2::new(rect.x, rect.y));

                            // Content
                            let content_style = TextStyle {
                                foreground: fg,
                                background: None,
                                bold: false,
                                dim: matches!(line, DiffLine::Context(_)),
                                italic: false,
                                underline: false,
                            };
                            for (ci, ch) in text.chars().enumerate().take(200) {
                                if ch != ' ' && ch != '\t' {
                                    renderer.draw_grid_cell(ch, vi, 4 + ci, content_style, cell_size, Vec2::new(rect.x, rect.y));
                                }
                            }

                            vi += 1;
                        }
                        row_idx += 1;
                    }
                }
            }
        }
    }

    /// Get the file index at a visual row (accounting for scroll and expanded diffs).
    pub fn file_at_row(&self, visual_row: usize) -> Option<usize> {
        let target_row = self.scroll as usize + visual_row;
        let mut row_idx = 0;
        for (fi, _) in self.files.iter().enumerate() {
            if row_idx == target_row {
                return Some(fi);
            }
            row_idx += 1;
            if self.expanded.contains(&fi) {
                if let Some(lines) = self.diff_cache.get(&fi) {
                    row_idx += lines.len();
                }
            }
            if row_idx > target_row {
                return None; // clicked on a diff line, not a file header
            }
        }
        None
    }

    pub fn generation(&self) -> u64 {
        self.generation
    }
}
