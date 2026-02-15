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
    pub h_scroll: usize,
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
            h_scroll: 0,
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

    /// Longest content line length across all expanded diffs.
    pub fn max_line_len(&self) -> usize {
        let mut max = 0;
        for (i, _) in self.files.iter().enumerate() {
            if self.expanded.contains(&i) {
                if let Some(lines) = self.diff_cache.get(&i) {
                    for line in lines {
                        let len = match line {
                            DiffLine::Added(t) | DiffLine::Removed(t)
                            | DiffLine::Header(t) | DiffLine::Context(t) => t.chars().count(),
                        };
                        if len > max { max = len; }
                    }
                }
            }
        }
        max
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

                // Subtle background for file header rows
                let header_bg = Color::new(1.0, 1.0, 1.0, if is_selected { 0.08 } else { 0.03 });
                renderer.draw_grid_rect(Rect::new(rect.x, y, rect.width, cell_size.height), header_bg);

                let max_cols = (rect.width / cell_size.width).floor() as usize;
                let mut col = 0usize;

                // Expand indicator: simple arrow
                let arrow = if is_expanded { '▾' } else { '▸' };
                let dim_style = TextStyle {
                    foreground: dimmed_color, background: None,
                    bold: false, dim: false, italic: false, underline: false,
                };
                renderer.draw_grid_cell(arrow, vi, col, dim_style, cell_size, Vec2::new(rect.x, rect.y));
                col += 2; // arrow + space

                // Status letter (colored, no brackets)
                let status_str = file.status.trim();
                let status_color = match status_str {
                    "M" | " M" => added_gutter,
                    "D" | " D" => removed_gutter,
                    "A" | "??" => added_gutter,
                    _ => text_color,
                };
                let status_ch = match status_str {
                    "M" | " M" => 'M',
                    "D" | " D" => 'D',
                    "A" => 'A',
                    "??" => 'U',
                    _ => '?',
                };
                let status_style = TextStyle {
                    foreground: status_color, background: None,
                    bold: true, dim: false, italic: false, underline: false,
                };
                renderer.draw_grid_cell(status_ch, vi, col, status_style, cell_size, Vec2::new(rect.x, rect.y));
                col += 2; // status + space

                // File path: directory/ dimmed, filename bold
                let (dir_part, file_part) = if let Some(pos) = file.path.rfind('/') {
                    (&file.path[..=pos], &file.path[pos + 1..])
                } else {
                    ("", file.path.as_str())
                };
                let dir_style = TextStyle {
                    foreground: dimmed_color, background: None,
                    bold: false, dim: true, italic: false, underline: false,
                };
                let file_style = TextStyle {
                    foreground: text_color, background: None,
                    bold: true, dim: false, italic: false, underline: false,
                };
                // Build stats string early so we know how much space to reserve
                let stats_str = if file.additions > 0 || file.deletions > 0 {
                    format!("+{}  -{}", file.additions, file.deletions)
                } else {
                    String::new()
                };
                let stats_reserve = stats_str.chars().count() + 2;
                let path_max = max_cols.saturating_sub(col + stats_reserve);
                for (ci, ch) in dir_part.chars().enumerate() {
                    if ci >= path_max { break; }
                    renderer.draw_grid_cell(ch, vi, col + ci, dir_style, cell_size, Vec2::new(rect.x, rect.y));
                }
                let file_col = col + dir_part.chars().count();
                for (ci, ch) in file_part.chars().enumerate() {
                    if dir_part.chars().count() + ci >= path_max { break; }
                    renderer.draw_grid_cell(ch, vi, file_col + ci, file_style, cell_size, Vec2::new(rect.x, rect.y));
                }

                // Stats at end: +N  -N
                if !stats_str.is_empty() {
                    let stats_chars: Vec<char> = stats_str.chars().collect();
                    let start_col = max_cols.saturating_sub(stats_chars.len() + 1);
                    let dash_pos = stats_str.find('-').unwrap_or(stats_str.len());
                    for (ci, &ch) in stats_chars.iter().enumerate() {
                        let color = if ci < dash_pos { added_gutter } else { removed_gutter };
                        let stat_style = TextStyle {
                            foreground: color, background: None,
                            bold: false, dim: false, italic: false, underline: false,
                        };
                        renderer.draw_grid_cell(ch, vi, start_col + ci, stat_style, cell_size, Vec2::new(rect.x, rect.y));
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

                            // Content (with horizontal scroll)
                            let content_style = TextStyle {
                                foreground: fg,
                                background: None,
                                bold: false,
                                dim: matches!(line, DiffLine::Context(_)),
                                italic: false,
                                underline: false,
                            };
                            let max_cols = (rect.width / cell_size.width).floor() as usize;
                            for (ci, ch) in text.chars().skip(self.h_scroll).enumerate().take(max_cols.saturating_sub(4)) {
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
