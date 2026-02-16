// UI state structs extracted from main.rs

use std::path::PathBuf;

use tide_core::{PaneId, Rect};
use crate::theme::{TAB_BAR_HEIGHT, PANE_PADDING, PANEL_TAB_HEIGHT, PANE_GAP, PANEL_TAB_WIDTH, PANEL_TAB_GAP, PANEL_TAB_CLOSE_SIZE, PANEL_TAB_CLOSE_PADDING};

// ──────────────────────────────────────────────
// Layout side: which edge a sidebar/dock component is on
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutSide {
    Left,
    Right,
}

/// Layout mode for the main terminal pane area.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PaneAreaMode {
    /// 2D spatial layout with per-pane headers (default).
    Split,
    /// Dock-like stacked view: tab bar + single visible pane, linear navigation.
    /// The `PaneId` is the currently active (visible) pane.
    Stacked(PaneId),
}

impl Default for PaneAreaMode {
    fn default() -> Self {
        PaneAreaMode::Split
    }
}

impl PaneAreaMode {
    /// Height from pane rect top to the start of the content area.
    /// Stacked mode uses a taller tab bar than Split mode headers.
    pub(crate) fn content_top(&self) -> f32 {
        match self {
            PaneAreaMode::Split => TAB_BAR_HEIGHT,
            PaneAreaMode::Stacked(_) => PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP,
        }
    }
}

/// Computed geometry for a horizontal tab bar (stacked panes or editor panel).
/// Extracted to share layout math between hit-testing, rendering, and hover.
pub(crate) struct TabBarGeometry {
    pub tab_bar_top: f32,
    pub tab_start_x: f32,
}

impl TabBarGeometry {
    /// X position of the tab at `index`.
    pub fn tab_x(&self, index: usize) -> f32 {
        self.tab_start_x + index as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP)
    }

    /// Bounding rect of the tab at `index`.
    pub fn tab_rect(&self, index: usize) -> Rect {
        Rect::new(self.tab_x(index), self.tab_bar_top, PANEL_TAB_WIDTH, PANEL_TAB_HEIGHT)
    }

    /// Bounding rect of the close button inside the tab at `index`.
    pub fn close_rect(&self, index: usize) -> Rect {
        let tx = self.tab_x(index);
        let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - PANEL_TAB_CLOSE_PADDING;
        let close_y = self.tab_bar_top + (PANEL_TAB_HEIGHT - PANEL_TAB_CLOSE_SIZE) / 2.0;
        Rect::new(close_x, close_y, PANEL_TAB_CLOSE_SIZE, PANEL_TAB_CLOSE_SIZE)
    }
}

// ──────────────────────────────────────────────
// Save-as input state (inline filename entry for untitled files)
// ──────────────────────────────────────────────

pub(crate) struct SaveAsInput {
    pub pane_id: PaneId,
    pub query: String,
    pub cursor: usize,
}

impl SaveAsInput {
    pub fn new(pane_id: PaneId) -> Self {
        Self {
            pane_id,
            query: String::new(),
            cursor: 0,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.query.len() {
            let next = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
            self.query.drain(self.cursor..next);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            self.cursor = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
        }
    }
}

// ──────────────────────────────────────────────
// Save confirm state (inline bar when closing dirty editors)
// ──────────────────────────────────────────────

pub(crate) struct SaveConfirmState {
    pub pane_id: PaneId,
}

// ──────────────────────────────────────────────
// File finder state (in-panel file search/open UI)
// ──────────────────────────────────────────────

pub(crate) struct FileFinderState {
    pub query: String,
    pub cursor: usize,
    pub base_dir: PathBuf,
    pub entries: Vec<PathBuf>,          // all files (relative to base_dir)
    pub filtered: Vec<usize>,           // indices into entries
    pub selected: usize,                // index into filtered
    pub scroll_offset: usize,           // scroll offset in filtered list
}

impl FileFinderState {
    pub fn new(base_dir: PathBuf, entries: Vec<PathBuf>) -> Self {
        let filtered: Vec<usize> = (0..entries.len()).collect();
        Self {
            query: String::new(),
            cursor: 0,
            base_dir,
            entries,
            filtered,
            selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.filter();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
            self.filter();
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.query.len() {
            let next = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
            self.query.drain(self.cursor..next);
            self.filter();
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            self.cursor = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn select_down(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        let idx = *self.filtered.get(self.selected)?;
        let rel = self.entries.get(idx)?;
        Some(self.base_dir.join(rel))
    }

    fn filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let query_lower = self.query.to_lowercase();
            self.filtered = self.entries.iter().enumerate()
                .filter(|(_, path)| {
                    let name = path.to_string_lossy().to_lowercase();
                    name.contains(&query_lower)
                })
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

// ──────────────────────────────────────────────
// Branch switcher popup state
// ──────────────────────────────────────────────

pub(crate) struct BranchSwitcherState {
    pub pane_id: PaneId,
    pub query: String,
    pub cursor: usize,
    pub branches: Vec<tide_terminal::git::BranchInfo>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub anchor_rect: Rect,
}

impl BranchSwitcherState {
    pub fn new(pane_id: PaneId, branches: Vec<tide_terminal::git::BranchInfo>, anchor_rect: Rect) -> Self {
        let filtered: Vec<usize> = (0..branches.len()).collect();
        Self {
            pane_id,
            query: String::new(),
            cursor: 0,
            branches,
            filtered,
            selected: 0,
            scroll_offset: 0,
            anchor_rect,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.filter();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
            self.filter();
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn select_down(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn selected_branch(&self) -> Option<&tide_terminal::git::BranchInfo> {
        let idx = *self.filtered.get(self.selected)?;
        self.branches.get(idx)
    }

    fn filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.branches.len()).collect();
        } else {
            let query_lower = self.query.to_lowercase();
            self.filtered = self.branches.iter().enumerate()
                .filter(|(_, b)| b.name.to_lowercase().contains(&query_lower))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

// ──────────────────────────────────────────────
// File switcher popup (open files list for editor panel)
// ──────────────────────────────────────────────

pub(crate) struct FileSwitcherEntry {
    pub pane_id: PaneId,
    pub name: String,
    pub is_active: bool,
}

pub(crate) struct FileSwitcherState {
    pub query: String,
    pub cursor: usize,
    pub entries: Vec<FileSwitcherEntry>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub anchor_rect: tide_core::Rect,
}

impl FileSwitcherState {
    pub fn new(entries: Vec<FileSwitcherEntry>, anchor_rect: tide_core::Rect) -> Self {
        let filtered: Vec<usize> = (0..entries.len()).collect();
        // Pre-select the active entry
        let selected = entries.iter().position(|e| e.is_active).unwrap_or(0);
        Self {
            query: String::new(),
            cursor: 0,
            entries,
            filtered,
            selected,
            scroll_offset: 0,
            anchor_rect,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.filter();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
            self.filter();
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn select_down(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn selected_entry(&self) -> Option<&FileSwitcherEntry> {
        let idx = *self.filtered.get(self.selected)?;
        self.entries.get(idx)
    }

    fn filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let query_lower = self.query.to_lowercase();
            self.filtered = self.entries.iter().enumerate()
                .filter(|(_, e)| e.name.to_lowercase().contains(&query_lower))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }
}
