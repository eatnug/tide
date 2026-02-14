use std::path::PathBuf;
use std::time::Instant;

use tide_core::{InputEvent, LayoutEngine, Renderer, Size, SplitDirection, TerminalBackend, Vec2};
use tide_editor::input::EditorAction;
use tide_input::{Action, Direction, GlobalAction};
use crate::search::SearchState;

use crate::editor_pane::EditorPane;
use crate::input::winit_modifiers_to_tide;
use crate::pane::{PaneKind, TerminalPane};
use crate::theme::*;
use crate::App;

impl App {
    pub(crate) fn handle_action(&mut self, action: Action, event: Option<InputEvent>) {
        match action {
            Action::RouteToPane(id) => {
                // Update focus
                if let Some(InputEvent::MouseClick { position, .. }) = event {
                    if self.focused != Some(id) {
                        self.focused = Some(id);
                        self.router.set_focused(id);
                        self.chrome_generation += 1;
                        self.update_file_tree_cwd();
                    }

                    // Ctrl+Click / Cmd+Click on terminal → try to open file at click position
                    let mods = winit_modifiers_to_tide(self.modifiers);
                    if mods.ctrl || mods.meta {
                        if let Some(path) = self.extract_file_path_at(id, position) {
                            self.open_editor_pane(path);
                            return;
                        }
                    }

                    // Click on editor pane → move cursor
                    if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&id) {
                        if let Some(renderer) = self.renderer.as_ref() {
                            if let Some(&(_, rect)) = self.visual_pane_rects.iter().find(|(pid, _)| *pid == id) {
                                let cell_size = renderer.cell_size();
                                let inner_x = rect.x + PANE_PADDING;
                                let inner_y = rect.y + TAB_BAR_HEIGHT;
                                let gutter_width = 5.0 * cell_size.width; // GUTTER_WIDTH_CELLS

                                let content_x = inner_x + gutter_width;
                                let rel_col = ((position.x - content_x) / cell_size.width).floor() as isize;
                                let rel_row = ((position.y - inner_y) / cell_size.height).floor() as isize;

                                if rel_row >= 0 && rel_col >= 0 {
                                    let line = pane.editor.scroll_offset() + rel_row as usize;
                                    let col = pane.editor.h_scroll_offset() + rel_col as usize;
                                    let visible_rows = ((rect.height - TAB_BAR_HEIGHT - PANE_PADDING) / cell_size.height).floor() as usize;
                                    pane.handle_action(EditorAction::SetCursor { line, col }, visible_rows);
                                }
                            }
                        }
                    }
                }

                // Forward keyboard input to the pane
                if let Some(InputEvent::KeyPress { key, modifiers }) = event {
                    match self.panes.get_mut(&id) {
                        Some(PaneKind::Terminal(pane)) => {
                            pane.selection = None; // Clear selection on key input
                            pane.handle_key(&key, &modifiers);
                            self.input_just_sent = true;
                            self.input_sent_at = Some(Instant::now());
                        }
                        Some(PaneKind::Editor(pane)) => {
                            pane.selection = None; // Clear selection on key input
                            if let Some(action) = tide_editor::key_to_editor_action(&key, &modifiers) {
                                let was_modified = pane.editor.is_modified();
                                let is_save = matches!(action, tide_editor::EditorActionKind::Save);
                                let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                                let (visible_rows, visible_cols) = if let Some(cs) = cell_size {
                                    let tree_rect = self.visual_pane_rects.iter()
                                        .find(|(pid, _)| *pid == id)
                                        .map(|(_, r)| *r);
                                    if let Some(r) = tree_rect {
                                        let rows = ((r.height - TAB_BAR_HEIGHT - PANE_PADDING) / cs.height).floor() as usize;
                                        let gutter_width = 5.0 * cs.width;
                                        let cols = ((r.width - 2.0 * PANE_PADDING - gutter_width) / cs.width).floor() as usize;
                                        (rows, cols)
                                    } else if let Some(pr) = self.editor_panel_rect {
                                        let content_height = (pr.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                                        let rows = (content_height / cs.height).floor() as usize;
                                        let gutter_width = 5.0 * cs.width;
                                        let cols = ((pr.width - 2.0 * PANE_PADDING - gutter_width) / cs.width).floor() as usize;
                                        (rows, cols)
                                    } else {
                                        (30, 80)
                                    }
                                } else {
                                    (30, 80)
                                };
                                pane.handle_action_with_size(action, visible_rows, visible_cols);
                                // Clear disk_changed on save (user's version wins)
                                if is_save {
                                    pane.disk_changed = false;
                                }
                                // Redraw tab label when modified indicator changes
                                if pane.editor.is_modified() != was_modified || is_save {
                                    self.chrome_generation += 1;
                                }
                            }
                        }
                        None => {}
                    }
                }

                // Forward mouse scroll to pane
                if let Some(InputEvent::MouseScroll { delta, .. }) = event {
                    // Compute actual visible rows/cols for the pane
                    let (visible_rows, visible_cols) = self.renderer.as_ref().map(|r| {
                        let cs = r.cell_size();
                        let rect = self.visual_pane_rects.iter()
                            .find(|(pid, _)| *pid == id)
                            .map(|(_, r)| *r);
                        if let Some(r) = rect {
                            let rows = ((r.height - TAB_BAR_HEIGHT - PANE_PADDING) / cs.height).floor() as usize;
                            let gutter_width = 5.0 * cs.width;
                            let cols = ((r.width - 2.0 * PANE_PADDING - gutter_width) / cs.width).floor() as usize;
                            (rows.max(1), cols.max(1))
                        } else if let Some(pr) = self.editor_panel_rect {
                            let content_height = (pr.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                            let rows = (content_height / cs.height).floor() as usize;
                            let gutter_width = 5.0 * cs.width;
                            let cols = ((pr.width - 2.0 * PANE_PADDING - gutter_width) / cs.width).floor() as usize;
                            (rows.max(1), cols.max(1))
                        } else {
                            (30, 80)
                        }
                    }).unwrap_or((30, 80));
                    match self.panes.get_mut(&id) {
                        Some(PaneKind::Editor(pane)) => {
                            if delta > 0.0 {
                                pane.handle_action_with_size(EditorAction::ScrollUp(delta.abs()), visible_rows, visible_cols);
                            } else {
                                pane.handle_action_with_size(EditorAction::ScrollDown(delta.abs()), visible_rows, visible_cols);
                            }
                        }
                        Some(PaneKind::Terminal(pane)) => {
                            // Accumulate sub-pixel scroll deltas to prevent jitter
                            let acc = self.scroll_accumulator.entry(id).or_insert(0.0);
                            *acc += delta;
                            let lines = acc.trunc() as i32;
                            if lines != 0 {
                                *acc -= lines as f32;
                                pane.scroll_display(lines);
                            }
                        }
                        None => {}
                    }
                }
            }
            Action::GlobalAction(global) => {
                self.handle_global_action(global);
            }
            Action::DragBorder(pos) => {
                let drag_pos = if self.show_file_tree {
                    Vec2::new(pos.x - FILE_TREE_WIDTH, pos.y)
                } else {
                    pos
                };
                let terminal_area = if self.show_file_tree {
                    Size::new(
                        (self.logical_size().width - FILE_TREE_WIDTH).max(100.0),
                        self.logical_size().height,
                    )
                } else {
                    self.logical_size()
                };
                self.layout.begin_drag(drag_pos, terminal_area);
                self.layout.drag_border(drag_pos);
                self.compute_layout();
            }
            Action::None => {}
        }
    }

    pub(crate) fn handle_global_action(&mut self, action: GlobalAction) {
        match action {
            GlobalAction::SplitVertical => {
                if let Some(focused) = self.focused {
                    self.maximized_pane = None;
                    let cwd = self.focused_terminal_cwd();
                    let new_id = self.layout.split(focused, SplitDirection::Vertical);
                    self.create_terminal_pane(new_id, cwd);
                    self.focused = Some(new_id);
                    self.router.set_focused(new_id);
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
            }
            GlobalAction::SplitHorizontal => {
                if let Some(focused) = self.focused {
                    self.maximized_pane = None;
                    let cwd = self.focused_terminal_cwd();
                    let new_id = self.layout.split(focused, SplitDirection::Horizontal);
                    self.create_terminal_pane(new_id, cwd);
                    self.focused = Some(new_id);
                    self.router.set_focused(new_id);
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
            }
            GlobalAction::ClosePane => {
                if let Some(focused) = self.focused {
                    // If focused pane is in the editor panel, close panel tab
                    if self.editor_panel_tabs.contains(&focused) {
                        self.close_editor_panel_tab(focused);
                        self.update_file_tree_cwd();
                        return;
                    }

                    // Clear maximize if the maximized pane is being closed
                    if self.maximized_pane == Some(focused) {
                        self.maximized_pane = None;
                    }

                    let remaining = self.layout.pane_ids();
                    if remaining.len() <= 1 && self.editor_panel_tabs.is_empty() {
                        // Don't close the last pane — exit the app instead
                        std::process::exit(0);
                    }
                    if remaining.len() <= 1 {
                        // Last tree pane but panel has tabs — focus panel instead
                        if let Some(active) = self.editor_panel_active {
                            self.focused = Some(active);
                            self.router.set_focused(active);
                            self.chrome_generation += 1;
                        }
                        return;
                    }

                    self.layout.remove(focused);
                    self.panes.remove(&focused);

                    // Focus the first remaining pane
                    let remaining = self.layout.pane_ids();
                    if let Some(&next) = remaining.first() {
                        self.focused = Some(next);
                        self.router.set_focused(next);
                    } else {
                        self.focused = None;
                    }

                    self.chrome_generation += 1;
                    self.compute_layout();
                    self.update_file_tree_cwd();
                }
            }
            GlobalAction::ToggleFileTree => {
                self.show_file_tree = !self.show_file_tree;
                self.chrome_generation += 1;
                self.compute_layout();
                if self.show_file_tree {
                    self.update_file_tree_cwd();
                }
            }
            GlobalAction::OpenFile => {
                // Open file tree so user can pick a file
                if !self.show_file_tree {
                    self.show_file_tree = true;
                    self.chrome_generation += 1;
                    self.compute_layout();
                    self.update_file_tree_cwd();
                }
            }
            GlobalAction::ToggleFullscreen => {
                if let Some(window) = &self.window {
                    if window.fullscreen().is_some() {
                        window.set_fullscreen(None);
                    } else {
                        window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                    }
                }
            }
            GlobalAction::Paste => {
                if let Some(focused_id) = self.focused {
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&focused_id) {
                        if let Ok(mut clipboard) = arboard::Clipboard::new() {
                            if let Ok(text) = clipboard.get_text() {
                                if !text.is_empty() {
                                    // Bracket paste mode for safety
                                    let mut data = Vec::new();
                                    data.extend_from_slice(b"\x1b[200~");
                                    data.extend_from_slice(text.as_bytes());
                                    data.extend_from_slice(b"\x1b[201~");
                                    pane.backend.write(&data);
                                    self.input_just_sent = true;
                                    self.input_sent_at = Some(Instant::now());
                                }
                            }
                        }
                    }
                }
            }
            GlobalAction::Copy => {
                if let Some(focused_id) = self.focused {
                    match self.panes.get(&focused_id) {
                        Some(PaneKind::Terminal(pane)) => {
                            if let Some(ref sel) = pane.selection {
                                let text = pane.selected_text(sel);
                                if !text.is_empty() {
                                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                        let _ = clipboard.set_text(&text);
                                    }
                                }
                            }
                        }
                        Some(PaneKind::Editor(pane)) => {
                            if let Some(ref sel) = pane.selection {
                                let text = pane.selected_text(sel);
                                if !text.is_empty() {
                                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                        let _ = clipboard.set_text(&text);
                                    }
                                }
                            }
                        }
                        None => {}
                    }
                }
            }
            GlobalAction::Find => {
                if let Some(focused_id) = self.focused {
                    let has_search = match self.panes.get(&focused_id) {
                        Some(PaneKind::Terminal(pane)) => pane.search.is_some(),
                        Some(PaneKind::Editor(pane)) => pane.search.is_some(),
                        None => false,
                    };
                    if has_search {
                        // Search already open → just (re-)focus it
                        self.search_focus = Some(focused_id);
                    } else {
                        // Open new search
                        match self.panes.get_mut(&focused_id) {
                            Some(PaneKind::Terminal(pane)) => {
                                pane.search = Some(SearchState::new());
                            }
                            Some(PaneKind::Editor(pane)) => {
                                pane.search = Some(SearchState::new());
                            }
                            None => {}
                        }
                        self.search_focus = Some(focused_id);
                    }
                }
            }
            GlobalAction::MoveFocus(direction) => {
                let current_id = match self.focused {
                    Some(id) => id,
                    None => return,
                };

                // Phase A: Tab cycling when focused on editor panel
                let in_editor_panel = self.editor_panel_tabs.contains(&current_id);
                if in_editor_panel {
                    if let Some(active) = self.editor_panel_active {
                        if let Some(idx) = self.editor_panel_tabs.iter().position(|&id| id == active) {
                            match direction {
                                Direction::Left => {
                                    if idx > 0 {
                                        // Switch to previous tab
                                        let prev_id = self.editor_panel_tabs[idx - 1];
                                        self.editor_panel_active = Some(prev_id);
                                        self.pane_generations.remove(&prev_id); // force grid rebuild
                                        self.focused = Some(prev_id);
                                        self.router.set_focused(prev_id);
                                        self.chrome_generation += 1;
                                        self.scroll_to_active_panel_tab();
                                        return;
                                    }
                                    // On first tab when maximized: stay put
                                    if self.editor_panel_maximized {
                                        return;
                                    }
                                    // On first tab: fall through to navigate to tree panes
                                }
                                Direction::Right => {
                                    if idx + 1 < self.editor_panel_tabs.len() {
                                        // Switch to next tab
                                        let next_id = self.editor_panel_tabs[idx + 1];
                                        self.editor_panel_active = Some(next_id);
                                        self.pane_generations.remove(&next_id); // force grid rebuild
                                        self.focused = Some(next_id);
                                        self.router.set_focused(next_id);
                                        self.chrome_generation += 1;
                                        self.scroll_to_active_panel_tab();
                                        return;
                                    }
                                    // On last tab: nothing to the right
                                    return;
                                }
                                Direction::Up | Direction::Down => {
                                    // Fall through to standard navigation
                                }
                            }
                        }
                    }
                }

                // Phase B: Standard navigation with editor panel included
                // Unmaximize first so all pane rects are available for navigation
                if self.maximized_pane.is_some() {
                    self.maximized_pane = None;
                    self.compute_layout();
                }
                if self.editor_panel_maximized {
                    self.editor_panel_maximized = false;
                    self.compute_layout();
                }

                // Build combined rect list: tree panes + editor panel
                let mut all_rects = self.pane_rects.clone();
                if let (Some(panel_rect), Some(active_tab)) = (self.editor_panel_rect, self.editor_panel_active) {
                    all_rects.push((active_tab, panel_rect));
                }

                if all_rects.len() < 2 {
                    return;
                }

                let current_rect = match all_rects.iter().find(|(id, _)| *id == current_id) {
                    Some((_, r)) => *r,
                    None => return,
                };
                let cx = current_rect.x + current_rect.width / 2.0;
                let cy = current_rect.y + current_rect.height / 2.0;

                // Find the closest pane in the given direction.
                // For Left/Right: prefer panes that vertically overlap, rank by horizontal distance.
                // For Up/Down: prefer panes that horizontally overlap, rank by vertical distance.
                let mut best: Option<(tide_core::PaneId, f32)> = None;
                for &(id, rect) in &all_rects {
                    if id == current_id {
                        continue;
                    }
                    let ox = rect.x + rect.width / 2.0;
                    let oy = rect.y + rect.height / 2.0;
                    let dx = ox - cx;
                    let dy = oy - cy;

                    let (valid, overlaps, dist) = match direction {
                        Direction::Left => (
                            dx < -1.0,
                            rect.y < current_rect.y + current_rect.height && rect.y + rect.height > current_rect.y,
                            dx.abs(),
                        ),
                        Direction::Right => (
                            dx > 1.0,
                            rect.y < current_rect.y + current_rect.height && rect.y + rect.height > current_rect.y,
                            dx.abs(),
                        ),
                        Direction::Up => (
                            dy < -1.0,
                            rect.x < current_rect.x + current_rect.width && rect.x + rect.width > current_rect.x,
                            dy.abs(),
                        ),
                        Direction::Down => (
                            dy > 1.0,
                            rect.x < current_rect.x + current_rect.width && rect.x + rect.width > current_rect.x,
                            dy.abs(),
                        ),
                    };

                    if !valid {
                        continue;
                    }

                    // Prefer overlapping panes; among those, pick the closest on the primary axis
                    let score = if overlaps { dist } else { dist + 100000.0 };
                    if best.is_none_or(|(_, d)| score < d) {
                        best = Some((id, score));
                    }
                }

                if let Some((next_id, _)) = best {
                    self.focused = Some(next_id);
                    self.router.set_focused(next_id);
                    self.chrome_generation += 1;
                    self.update_file_tree_cwd();
                }
            }
            GlobalAction::ToggleMaximizePane => {
                if let Some(focused) = self.focused {
                    if self.editor_panel_tabs.contains(&focused) {
                        // Toggle editor panel maximize
                        self.editor_panel_maximized = !self.editor_panel_maximized;
                        self.maximized_pane = None; // mutually exclusive
                    } else {
                        // Toggle tree pane maximize
                        if self.maximized_pane == Some(focused) {
                            self.maximized_pane = None;
                        } else {
                            self.maximized_pane = Some(focused);
                        }
                        self.editor_panel_maximized = false; // mutually exclusive
                    }
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
            }
            GlobalAction::ToggleEditorPanel => {
                self.show_editor_panel = !self.show_editor_panel;
                self.chrome_generation += 1;
                // If hiding, move focus to tree (preserve maximize state)
                if !self.show_editor_panel {
                    if let Some(focused) = self.focused {
                        if self.editor_panel_tabs.contains(&focused) {
                            if let Some(&first) = self.layout.pane_ids().first() {
                                self.focused = Some(first);
                                self.router.set_focused(first);
                            }
                        }
                    }
                }
                self.compute_layout();
            }
            GlobalAction::NewEditorFile => {
                self.new_editor_pane();
            }
        }
    }

    pub(crate) fn create_terminal_pane(&mut self, id: tide_core::PaneId, cwd: Option<std::path::PathBuf>) {
        let cell_size = self.renderer.as_ref().unwrap().cell_size();
        let logical = self.logical_size();
        let cols = (logical.width / 2.0 / cell_size.width).max(1.0) as u16;
        let rows = (logical.height / cell_size.height).max(1.0) as u16;

        match TerminalPane::with_cwd(id, cols, rows, cwd) {
            Ok(pane) => {
                self.panes.insert(id, PaneKind::Terminal(pane));
            }
            Err(e) => {
                log::error!("Failed to create terminal pane: {}", e);
            }
        }
    }

    /// Get the CWD of the currently focused terminal pane, if any.
    fn focused_terminal_cwd(&self) -> Option<std::path::PathBuf> {
        let focused = self.focused?;
        match self.panes.get(&focused) {
            Some(PaneKind::Terminal(p)) => p.backend.detect_cwd_fallback(),
            _ => None,
        }
    }

    /// Create a new empty editor pane in the panel.
    /// Auto-shows the editor panel if it was hidden.
    pub(crate) fn new_editor_pane(&mut self) {
        if !self.show_editor_panel {
            self.show_editor_panel = true;
        }
        let panel_was_visible = !self.editor_panel_tabs.is_empty();
        let new_id = self.layout.alloc_id();
        let pane = EditorPane::new_empty(new_id);
        self.panes.insert(new_id, PaneKind::Editor(pane));
        self.editor_panel_tabs.push(new_id);
        self.editor_panel_active = Some(new_id);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.chrome_generation += 1;
        if !panel_was_visible {
            self.compute_layout();
        }
        self.scroll_to_active_panel_tab();
    }

    /// Open a file in the editor panel. If already open, activate its tab.
    /// Auto-shows the editor panel if it was hidden.
    pub(crate) fn open_editor_pane(&mut self, path: PathBuf) {
        // Auto-show editor panel if hidden
        if !self.show_editor_panel {
            self.show_editor_panel = true;
        }
        // Check if already open in panel tabs → activate & focus
        for &tab_id in &self.editor_panel_tabs {
            if let Some(PaneKind::Editor(editor)) = self.panes.get(&tab_id) {
                if editor.editor.file_path() == Some(path.as_path()) {
                    self.editor_panel_active = Some(tab_id);
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.chrome_generation += 1;
                    self.scroll_to_active_panel_tab();
                    return;
                }
            }
        }

        // Check if already open in split tree → focus
        for (&id, pane) in &self.panes {
            if let PaneKind::Editor(editor) = pane {
                if editor.editor.file_path() == Some(path.as_path()) {
                    self.focused = Some(id);
                    self.router.set_focused(id);
                    self.chrome_generation += 1;
                    return;
                }
            }
        }

        // Create new editor pane in the panel
        let panel_was_visible = !self.editor_panel_tabs.is_empty();
        let new_id = self.layout.alloc_id();
        match EditorPane::open(new_id, &path) {
            Ok(pane) => {
                self.panes.insert(new_id, PaneKind::Editor(pane));
                self.editor_panel_tabs.push(new_id);
                self.editor_panel_active = Some(new_id);
                self.focused = Some(new_id);
                self.router.set_focused(new_id);
                self.chrome_generation += 1;
                // Watch the file for external changes
                self.watch_file(&path);
                // Only recompute layout if the panel just became visible (causes terminal resize)
                if !panel_was_visible {
                    self.compute_layout();
                }
                self.scroll_to_active_panel_tab();
            }
            Err(e) => {
                log::error!("Failed to open editor for {:?}: {}", path, e);
            }
        }
    }

    /// Close an editor panel tab.
    pub(crate) fn close_editor_panel_tab(&mut self, tab_id: tide_core::PaneId) {
        // Unwatch the file before removing the pane
        let watch_path = if let Some(PaneKind::Editor(editor)) = self.panes.get(&tab_id) {
            editor.editor.file_path().map(|p| p.to_path_buf())
        } else {
            None
        };
        if let Some(path) = watch_path {
            self.unwatch_file(&path);
        }
        self.editor_panel_tabs.retain(|&id| id != tab_id);
        self.panes.remove(&tab_id);
        self.pane_generations.remove(&tab_id);

        // Switch active to last remaining tab (or None)
        if self.editor_panel_active == Some(tab_id) {
            self.editor_panel_active = self.editor_panel_tabs.last().copied();
        }

        // If focused pane was the closed tab, switch focus
        if self.focused == Some(tab_id) {
            if let Some(active) = self.editor_panel_active {
                self.focused = Some(active);
                self.router.set_focused(active);
            } else if let Some(&first) = self.layout.pane_ids().first() {
                self.focused = Some(first);
                self.router.set_focused(first);
            } else {
                self.focused = None;
            }
        }

        self.chrome_generation += 1;
        self.compute_layout();
        self.clamp_panel_tab_scroll();
        self.scroll_to_active_panel_tab();
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
        let cell_size = self.renderer.as_ref()?.cell_size();

        let inner_x = visual_rect.x + PANE_PADDING;
        let inner_y = visual_rect.y + TAB_BAR_HEIGHT;

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
