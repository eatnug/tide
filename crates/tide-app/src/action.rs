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
    fn cleanup_closed_pane_state(&mut self, pane_id: tide_core::PaneId) {
        self.pane_generations.remove(&pane_id);
        self.scroll_accumulator.remove(&pane_id);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.remove_pane_cache(pane_id);
        }
    }

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

                    // Ctrl+Click / Cmd+Click on terminal → try to open URL or file at click position
                    let mods = winit_modifiers_to_tide(self.modifiers);
                    if mods.ctrl || mods.meta {
                        // Try URL first
                        if let Some(url) = self.extract_url_at(id, position) {
                            let _ = open::that(&url);
                            return;
                        }
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
                                let is_save = matches!(action, tide_editor::EditorActionKind::Save);
                                // Intercept Save on untitled files → open save-as input
                                if is_save && pane.editor.file_path().is_none() {
                                    self.save_as_input = Some(crate::SaveAsInput::new(id));
                                    return;
                                }
                                let was_modified = pane.editor.is_modified();
                                let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                                let (visible_rows, visible_cols) = if let Some(cs) = cell_size {
                                    let tree_rect = self.visual_pane_rects.iter()
                                        .find(|(pid, _)| *pid == id)
                                        .map(|(_, r)| *r);
                                    if let Some(r) = tree_rect {
                                        let rows = ((r.height - TAB_BAR_HEIGHT - PANE_PADDING) / cs.height).floor() as usize;
                                        let gutter_width = 5.0 * cs.width;
                                        let cols = ((r.width - 2.0 * PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
                                        (rows, cols)
                                    } else if let Some(pr) = self.editor_panel_rect {
                                        let content_height = (pr.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                                        let rows = (content_height / cs.height).floor() as usize;
                                        let gutter_width = 5.0 * cs.width;
                                        let cols = ((pr.width - 2.0 * PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
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
                                    pane.diff_mode = false;
                                    pane.disk_content = None;
                                    pane.file_deleted = false;
                                }
                                // Redraw tab label when modified indicator changes
                                if pane.editor.is_modified() != was_modified || is_save {
                                    self.chrome_generation += 1;
                                }
                            }
                        }
                        Some(PaneKind::Diff(_)) => {} // Diff pane has no keyboard input
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
                            let cols = ((r.width - 2.0 * PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
                            (rows.max(1), cols.max(1))
                        } else if let Some(pr) = self.editor_panel_rect {
                            let content_height = (pr.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                            let rows = (content_height / cs.height).floor() as usize;
                            let gutter_width = 5.0 * cs.width;
                            let cols = ((pr.width - 2.0 * PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
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
                        Some(PaneKind::Diff(dp)) => {
                            let total = dp.total_lines() as f32;
                            dp.scroll_target = (dp.scroll_target - delta).clamp(0.0, total.max(0.0));
                            dp.scroll = dp.scroll_target;
                            dp.generation = dp.generation.wrapping_add(1);
                        }
                        None => {}
                    }
                }
            }
            Action::GlobalAction(global) => {
                self.handle_global_action(global);
            }
            Action::DragBorder(pos) => {
                let logical = self.logical_size();
                let mut left = 0.0_f32;
                let mut right = 0.0_f32;
                if self.show_file_tree {
                    match self.sidebar_side {
                        crate::LayoutSide::Left => left += self.file_tree_width,
                        crate::LayoutSide::Right => right += self.file_tree_width,
                    }
                }
                if self.show_editor_panel {
                    match self.dock_side {
                        crate::LayoutSide::Left => left += self.editor_panel_width,
                        crate::LayoutSide::Right => right += self.editor_panel_width,
                    }
                }
                let drag_pos = Vec2::new(pos.x - left, pos.y);
                let terminal_area = Size::new(
                    (logical.width - left - right).max(100.0),
                    logical.height,
                );
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
                    let cwd = self.focused_terminal_cwd();
                    let new_id = self.layout.split(focused, SplitDirection::Vertical);
                    self.create_terminal_pane(new_id, cwd);
                    if self.maximized_pane.is_some() {
                        self.maximized_pane = Some(new_id);
                    }
                    self.focused = Some(new_id);
                    self.router.set_focused(new_id);
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
            }
            GlobalAction::SplitHorizontal => {
                if let Some(focused) = self.focused {
                    let cwd = self.focused_terminal_cwd();
                    let new_id = self.layout.split(focused, SplitDirection::Horizontal);
                    self.create_terminal_pane(new_id, cwd);
                    if self.maximized_pane.is_some() {
                        self.maximized_pane = Some(new_id);
                    }
                    self.focused = Some(new_id);
                    self.router.set_focused(new_id);
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
            }
            GlobalAction::ClosePane => {
                if let Some(focused) = self.focused {
                    self.close_specific_pane(focused);
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
                self.open_file_finder();
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
                        Some(PaneKind::Diff(_)) => {} // no selection in diff pane
                        None => {}
                    }
                }
            }
            GlobalAction::Find => {
                if let Some(focused_id) = self.focused {
                    let has_search = match self.panes.get(&focused_id) {
                        Some(PaneKind::Terminal(pane)) => pane.search.is_some(),
                        Some(PaneKind::Editor(pane)) => pane.search.is_some(),
                        Some(PaneKind::Diff(_)) => false,
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
                            Some(PaneKind::Diff(_)) => {} // no search in diff pane
                            None => {}
                        }
                        self.search_focus = Some(focused_id);
                    }
                }
            }
            GlobalAction::MoveFocus(direction) => {
                self.save_as_input = None;
                let current_id = match self.focused {
                    Some(id) => id,
                    None => return,
                };

                // Phase A: Tab cycling when focused on editor panel
                let in_editor_panel = self.editor_panel_tabs.contains(&current_id)
                    || self.editor_panel_placeholder == Some(current_id);
                if in_editor_panel {
                    if let Some(active) = self.editor_panel_active {
                        if let Some(idx) = self.editor_panel_tabs.iter().position(|&id| id == active) {
                            match direction {
                                Direction::Left => {
                                    if idx > 0 {
                                        let prev_id = self.editor_panel_tabs[idx - 1];
                                        self.editor_panel_active = Some(prev_id);
                                        self.pane_generations.remove(&prev_id);
                                        self.focused = Some(prev_id);
                                        self.router.set_focused(prev_id);
                                        self.chrome_generation += 1;
                                        self.scroll_to_active_panel_tab();
                                        return;
                                    }
                                    // First tab boundary:
                                    if self.editor_panel_maximized {
                                        return;
                                    }
                                    if self.dock_side == crate::LayoutSide::Left {
                                        // Dock on left → left edge is window edge, nothing further
                                        return;
                                    }
                                    // Dock on right → fall through to Phase C (panes are left)
                                }
                                Direction::Right => {
                                    if idx + 1 < self.editor_panel_tabs.len() {
                                        let next_id = self.editor_panel_tabs[idx + 1];
                                        self.editor_panel_active = Some(next_id);
                                        self.pane_generations.remove(&next_id);
                                        self.focused = Some(next_id);
                                        self.router.set_focused(next_id);
                                        self.chrome_generation += 1;
                                        self.scroll_to_active_panel_tab();
                                        return;
                                    }
                                    // Last tab boundary:
                                    if self.editor_panel_maximized {
                                        return;
                                    }
                                    if self.dock_side == crate::LayoutSide::Right {
                                        // Dock on right → right edge is window edge, nothing further
                                        return;
                                    }
                                    // Dock on left → fall through to Phase C (panes are right)
                                }
                                Direction::Up | Direction::Down => {
                                    return;
                                }
                            }
                        }
                    }
                }

                // Phase B: Maximized pane navigation (Left/Right cycle, Up/Down no-op)
                if self.maximized_pane.is_some() {
                    let pane_ids = self.layout.pane_ids();
                    if let Some(pos) = pane_ids.iter().position(|&id| id == current_id) {
                        match direction {
                            Direction::Left => {
                                if pos > 0 {
                                    let prev_id = pane_ids[pos - 1];
                                    self.maximized_pane = Some(prev_id);
                                    self.focused = Some(prev_id);
                                    self.router.set_focused(prev_id);
                                    self.chrome_generation += 1;
                                    self.compute_layout();
                                    self.update_file_tree_cwd();
                                }
                            }
                            Direction::Right => {
                                if pos + 1 < pane_ids.len() {
                                    let next_id = pane_ids[pos + 1];
                                    self.maximized_pane = Some(next_id);
                                    self.focused = Some(next_id);
                                    self.router.set_focused(next_id);
                                    self.chrome_generation += 1;
                                    self.compute_layout();
                                    self.update_file_tree_cwd();
                                }
                            }
                            Direction::Up | Direction::Down => {
                                // no-op while maximized
                            }
                        }
                    }
                    return;
                }

                // Phase C: Standard navigation with editor panel included
                if self.editor_panel_maximized {
                    self.editor_panel_maximized = false;
                    self.compute_layout();
                }

                // Build rect list: tree panes + editor panel (if visible).
                let mut all_rects = self.pane_rects.clone();
                // Include editor panel as a navigation target
                if self.show_editor_panel {
                    if let Some(panel_rect) = self.editor_panel_rect {
                        let focus_id = match self.editor_panel_active {
                            Some(id) => id,
                            None => self.get_or_alloc_placeholder(),
                        };
                        all_rects.push((focus_id, panel_rect));
                    }
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
                    let in_panel = self.editor_panel_tabs.contains(&focused)
                        || self.editor_panel_placeholder == Some(focused);
                    if in_panel {
                        // Toggle editor panel maximize
                        self.editor_panel_maximized = !self.editor_panel_maximized;
                        self.maximized_pane = None; // mutually exclusive
                    } else if self.editor_panel_maximized {
                        // Editor panel is maximized but focus is elsewhere — just unmaximize it
                        self.editor_panel_maximized = false;
                    } else {
                        // Toggle tree pane maximize
                        if self.maximized_pane == Some(focused) {
                            self.maximized_pane = None;
                        } else {
                            self.maximized_pane = Some(focused);
                        }
                    }
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
            }
            GlobalAction::ToggleEditorPanel => {
                self.show_editor_panel = !self.show_editor_panel;
                self.chrome_generation += 1;
                // If hiding, move focus to tree and reset manual width + maximize
                if !self.show_editor_panel {
                    self.editor_panel_maximized = false;
                    self.editor_panel_width_manual = false;
                    if let Some(focused) = self.focused {
                        let in_panel = self.editor_panel_tabs.contains(&focused)
                            || self.editor_panel_placeholder == Some(focused);
                        if in_panel {
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
            GlobalAction::FontSizeUp => {
                if let Some(renderer) = &mut self.renderer {
                    let new_size = renderer.font_size() + 1.0;
                    renderer.set_font_size(new_size);
                }
                self.pane_generations.clear();
                self.chrome_generation += 1;
                self.layout_generation = self.layout_generation.wrapping_add(1);
                self.compute_layout();
            }
            GlobalAction::FontSizeDown => {
                if let Some(renderer) = &mut self.renderer {
                    let new_size = renderer.font_size() - 1.0;
                    renderer.set_font_size(new_size);
                }
                self.pane_generations.clear();
                self.chrome_generation += 1;
                self.layout_generation = self.layout_generation.wrapping_add(1);
                self.compute_layout();
            }
            GlobalAction::FontSizeReset => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.set_font_size(14.0);
                }
                self.pane_generations.clear();
                self.chrome_generation += 1;
                self.layout_generation = self.layout_generation.wrapping_add(1);
                self.compute_layout();
            }
            GlobalAction::ToggleTheme => {
                self.dark_mode = !self.dark_mode;
                let border_color = self.palette().border_color;
                if let Some(renderer) = &mut self.renderer {
                    renderer.clear_color = border_color;
                }
                // Update color palettes for all panes
                let dark = self.dark_mode;
                for pane in self.panes.values_mut() {
                    match pane {
                        crate::pane::PaneKind::Terminal(tp) => {
                            tp.backend.set_dark_mode(dark);
                        }
                        crate::pane::PaneKind::Editor(ep) => {
                            ep.editor.set_dark_mode(dark);
                        }
                        crate::pane::PaneKind::Diff(_) => {} // no color palette
                    }
                }
                self.chrome_generation += 1;
                self.layout_generation = self.layout_generation.wrapping_add(1);
                // Force full grid rebuild for all panes (terminal colors change)
                self.pane_generations.clear();
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
                self.install_pty_waker(&pane);
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

    /// Get a working directory for file operations: try focused terminal, then any terminal,
    /// then file tree root, then std::env::current_dir.
    fn resolve_base_dir(&self) -> PathBuf {
        // 1. Focused terminal CWD
        if let Some(cwd) = self.focused_terminal_cwd() {
            return cwd;
        }
        // 2. Any terminal pane's CWD
        for pane in self.panes.values() {
            if let PaneKind::Terminal(p) = pane {
                if let Some(cwd) = p.backend.detect_cwd_fallback() {
                    return cwd;
                }
            }
        }
        // 3. File tree root
        if let Some(ref tree) = self.file_tree {
            use tide_core::FileTreeSource;
            let root = tree.root();
            if root.is_dir() {
                return root.to_path_buf();
            }
        }
        // 4. Fallback
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
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
            if !self.editor_panel_width_manual {
                self.editor_panel_width = self.auto_editor_panel_width();
            }
            self.compute_layout();
        }
        self.scroll_to_active_panel_tab();
    }

    /// Open a file in the editor panel. If already open, activate its tab.
    /// Auto-shows the editor panel if it was hidden.
    pub(crate) fn open_editor_pane(&mut self, path: PathBuf) {
        // Track whether panel needs layout recompute (becoming visible)
        let needs_layout = !self.show_editor_panel
            || (self.show_editor_panel && self.editor_panel_tabs.is_empty());

        // Auto-show editor panel if hidden
        if !self.show_editor_panel {
            self.show_editor_panel = true;
        }
        // Check if already open in panel tabs → activate & focus
        for &tab_id in &self.editor_panel_tabs {
            if let Some(PaneKind::Editor(editor)) = self.panes.get(&tab_id) {
                if editor.editor.file_path() == Some(path.as_path()) {
                    self.editor_panel_active = Some(tab_id);
                    self.pane_generations.remove(&tab_id);
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.chrome_generation += 1;
                    if needs_layout {
                        if !self.editor_panel_width_manual {
                            self.editor_panel_width = self.auto_editor_panel_width();
                        }
                        self.compute_layout();
                    }
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
                // Recompute layout if the panel just became visible (causes terminal resize)
                if needs_layout {
                    if !self.editor_panel_width_manual {
                        self.editor_panel_width = self.auto_editor_panel_width();
                    }
                    self.compute_layout();
                }
                self.scroll_to_active_panel_tab();
            }
            Err(e) => {
                log::error!("Failed to open editor for {:?}: {}", path, e);
            }
        }
    }

    /// Close an editor panel tab. If dirty, show save confirm bar instead.
    pub(crate) fn close_editor_panel_tab(&mut self, tab_id: tide_core::PaneId) {
        // Check if editor is dirty → show save confirm bar
        if let Some(PaneKind::Editor(pane)) = self.panes.get(&tab_id) {
            if pane.editor.is_modified() {
                self.save_confirm = Some(crate::SaveConfirmState { pane_id: tab_id });
                // Ensure this tab is active and focused so the bar is visible
                if self.editor_panel_tabs.contains(&tab_id) {
                    self.editor_panel_active = Some(tab_id);
                }
                self.focused = Some(tab_id);
                self.router.set_focused(tab_id);
                self.chrome_generation += 1;
                self.pane_generations.remove(&tab_id);
                return;
            }
        }
        self.force_close_editor_panel_tab(tab_id);
    }

    /// Force close an editor panel tab (no dirty check).
    pub(crate) fn force_close_editor_panel_tab(&mut self, tab_id: tide_core::PaneId) {
        // Cancel save-as if the target pane is being closed
        if self.save_as_input.as_ref().is_some_and(|s| s.pane_id == tab_id) {
            self.save_as_input = None;
        }
        // Cancel save confirm if the target pane is being closed
        if self.save_confirm.as_ref().is_some_and(|s| s.pane_id == tab_id) {
            self.save_confirm = None;
        }
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
        self.cleanup_closed_pane_state(tab_id);

        if self.editor_panel_tabs.is_empty() {
            self.show_editor_panel = false;
            self.editor_panel_maximized = false;
            self.editor_panel_width_manual = false;
        }

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

        self.pane_generations.clear();
        self.chrome_generation += 1;
        self.compute_layout();
        self.clamp_panel_tab_scroll();
        self.scroll_to_active_panel_tab();
    }

    /// Complete the save-as flow: resolve path, set file_path, detect syntax, save, watch.
    pub(crate) fn complete_save_as(&mut self, pane_id: tide_core::PaneId, filename: &str) {
        let path = if std::path::Path::new(filename).is_absolute() {
            PathBuf::from(filename)
        } else {
            self.resolve_base_dir().join(filename)
        };

        // Create parent dirs if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
            pane.editor.buffer.file_path = Some(path.clone());
            pane.editor.detect_and_set_syntax(&path);
            if let Err(e) = pane.editor.buffer.save() {
                log::error!("Failed to save file: {}", e);
            }
            pane.disk_changed = false;
        }

        self.watch_file(&path);
        self.chrome_generation += 1;
    }

    /// Close a specific pane by its ID (used by close button clicks).
    pub(crate) fn close_specific_pane(&mut self, pane_id: tide_core::PaneId) {
        // If the pane is in the editor panel, close the panel tab (with dirty check)
        if self.editor_panel_tabs.contains(&pane_id) {
            self.close_editor_panel_tab(pane_id);
            self.update_file_tree_cwd();
            return;
        }

        // Check if editor is dirty → show save confirm bar
        if let Some(PaneKind::Editor(pane)) = self.panes.get(&pane_id) {
            if pane.editor.is_modified() {
                self.save_confirm = Some(crate::SaveConfirmState { pane_id });
                self.focused = Some(pane_id);
                self.router.set_focused(pane_id);
                self.chrome_generation += 1;
                self.pane_generations.remove(&pane_id);
                return;
            }
        }

        self.force_close_specific_pane(pane_id);
    }

    /// Force close a specific pane (no dirty check).
    pub(crate) fn force_close_specific_pane(&mut self, pane_id: tide_core::PaneId) {
        // Cancel save-as if the target pane is being closed
        if self.save_as_input.as_ref().is_some_and(|s| s.pane_id == pane_id) {
            self.save_as_input = None;
        }
        // Cancel save confirm
        if self.save_confirm.as_ref().is_some_and(|s| s.pane_id == pane_id) {
            self.save_confirm = None;
        }
        // If the pane is in the editor panel, force close the panel tab
        if self.editor_panel_tabs.contains(&pane_id) {
            self.force_close_editor_panel_tab(pane_id);
            self.update_file_tree_cwd();
            return;
        }

        // Clear maximize if the maximized pane is being closed
        if self.maximized_pane == Some(pane_id) {
            self.maximized_pane = None;
        }

        let remaining = self.layout.pane_ids();
        if remaining.len() <= 1 && self.editor_panel_tabs.is_empty() {
            let session = crate::session::Session::from_app(self);
            crate::session::save_session(&session);
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

        self.layout.remove(pane_id);
        self.panes.remove(&pane_id);
        self.cleanup_closed_pane_state(pane_id);

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
        let cell_size = self.renderer.as_ref()?.cell_size();

        let inner_x = visual_rect.x + PANE_PADDING;
        let inner_y = visual_rect.y + TAB_BAR_HEIGHT;

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

    /// Save and close the pane from the save confirm bar.
    pub(crate) fn confirm_save_and_close(&mut self) {
        let pane_id = match self.save_confirm.take() {
            Some(sc) => sc.pane_id,
            None => return,
        };
        // Save
        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
            if pane.editor.file_path().is_none() {
                // Untitled file → open save-as input
                self.save_as_input = Some(crate::SaveAsInput::new(pane_id));
                return;
            }
            if let Err(e) = pane.editor.buffer.save() {
                log::error!("Save failed: {}", e);
                return;
            }
            pane.disk_changed = false;
        }
        // Close
        if self.editor_panel_tabs.contains(&pane_id) {
            self.force_close_editor_panel_tab(pane_id);
        } else {
            self.force_close_specific_pane(pane_id);
        }
    }

    /// Discard changes and close the pane from the save confirm bar.
    pub(crate) fn confirm_discard_and_close(&mut self) {
        let pane_id = match self.save_confirm.take() {
            Some(sc) => sc.pane_id,
            None => return,
        };
        if self.editor_panel_tabs.contains(&pane_id) {
            self.force_close_editor_panel_tab(pane_id);
        } else {
            self.force_close_specific_pane(pane_id);
        }
    }

    /// Cancel the save confirm bar.
    pub(crate) fn cancel_save_confirm(&mut self) {
        if self.save_confirm.is_some() {
            self.save_confirm = None;
            self.chrome_generation += 1;
            self.pane_generations.clear();
        }
    }

    /// Open the file finder UI in the editor panel.
    pub(crate) fn open_file_finder(&mut self) {
        let base_dir = self.resolve_base_dir();
        let mut entries: Vec<PathBuf> = Vec::new();
        Self::scan_dir(&base_dir, &base_dir, &mut entries, 0, 8);
        entries.sort();

        self.file_finder = Some(crate::FileFinderState::new(base_dir, entries));
        if !self.show_editor_panel {
            self.show_editor_panel = true;
            if !self.editor_panel_width_manual {
                self.editor_panel_width = self.auto_editor_panel_width();
            }
            self.compute_layout();
        }
        self.chrome_generation += 1;
    }

    /// Close the file finder UI.
    pub(crate) fn close_file_finder(&mut self) {
        if self.file_finder.is_some() {
            self.file_finder = None;
            self.chrome_generation += 1;
            // If no tabs are open, hide the editor panel
            if self.editor_panel_tabs.is_empty() {
                self.show_editor_panel = false;
                self.editor_panel_maximized = false;
                self.editor_panel_width_manual = false;
                self.compute_layout();
            }
        }
    }

    /// Recursively scan a directory, collecting file paths relative to base_dir.
    fn scan_dir(dir: &std::path::Path, base_dir: &std::path::Path, entries: &mut Vec<PathBuf>, depth: usize, max_depth: usize) {
        if depth > max_depth {
            return;
        }
        let read_dir = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        let mut subdirs: Vec<PathBuf> = Vec::new();
        for entry in read_dir.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Skip hidden and common ignored directories
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "__pycache__" {
                continue;
            }

            if path.is_dir() {
                subdirs.push(path);
            } else if path.is_file() {
                if let Ok(rel) = path.strip_prefix(base_dir) {
                    entries.push(rel.to_path_buf());
                }
            }
        }
        for subdir in subdirs {
            Self::scan_dir(&subdir, base_dir, entries, depth + 1, max_depth);
        }
    }

    /// Open or focus a DiffPane for the given CWD.
    /// If a DiffPane with the same CWD already exists in the panel, focus and refresh it.
    pub(crate) fn open_diff_pane(&mut self, cwd: PathBuf) {
        // Check if already open
        for &tab_id in &self.editor_panel_tabs {
            if let Some(PaneKind::Diff(dp)) = self.panes.get_mut(&tab_id) {
                if dp.cwd == cwd {
                    dp.refresh();
                    self.editor_panel_active = Some(tab_id);
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.chrome_generation += 1;
                    self.pane_generations.remove(&tab_id);
                    self.scroll_to_active_panel_tab();
                    return;
                }
            }
        }

        // Create new DiffPane in the editor panel
        if !self.show_editor_panel {
            self.show_editor_panel = true;
        }
        let needs_layout = self.editor_panel_tabs.is_empty();
        let new_id = self.layout.alloc_id();
        let dp = crate::diff_pane::DiffPane::new(new_id, cwd);
        self.panes.insert(new_id, PaneKind::Diff(dp));
        self.editor_panel_tabs.push(new_id);
        self.editor_panel_active = Some(new_id);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.chrome_generation += 1;
        if needs_layout {
            if !self.editor_panel_width_manual {
                self.editor_panel_width = self.auto_editor_panel_width();
            }
            self.compute_layout();
        }
        self.scroll_to_active_panel_tab();
    }
}
