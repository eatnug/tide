mod pane_lifecycle;
mod focus_nav;
mod text_extract;
mod file_ops;

use std::time::Instant;

use tide_core::{InputEvent, LayoutEngine, Size, SplitDirection, TerminalBackend, Vec2};
use tide_editor::input::EditorAction;
use tide_input::{Action, AreaSlot, GlobalAction};
use crate::search::SearchState;

use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui_state::FocusArea;
use crate::{App, LayoutSide, PaneAreaMode};

impl App {
    fn cleanup_closed_pane_state(&mut self, pane_id: tide_core::PaneId) {
        self.pane_generations.remove(&pane_id);
        self.scroll_accumulator.remove(&pane_id);
        self.pending_ime_proxy_removes.push(pane_id);
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.remove_pane_cache(pane_id);
        }
    }

    /// Switch primary focus to a terminal pane, setting focus to PaneArea.
    /// Handles all common side effects: file tree CWD update, panel tab scroll
    /// reset, auto-hide panel when switching away from a terminal with editors.
    pub(crate) fn focus_terminal(&mut self, id: tide_core::PaneId) {
        self.focus_area = FocusArea::PaneArea;
        if self.focused == Some(id) {
            return;
        }
        let old_tid = self.focused_terminal_id();
        self.focused = Some(id);
        self.router.set_focused(id);
        self.chrome_generation += 1;
        self.update_file_tree_cwd();
        if self.focused_terminal_id() != old_tid {
            self.panel_tab_scroll = 0.0;
            self.panel_tab_scroll_target = 0.0;
            if self.editor_panel_auto_shown && self.active_editor_tabs().is_empty() {
                self.show_editor_panel = false;
                self.editor_panel_auto_shown = false;
                self.compute_layout();
            }
        }
        // Immediately sync webview visibility so the browser hides/shows
        // without waiting for the next update() tick (which may be gated by
        // is_rapid).
        self.sync_browser_webview_frames();
    }

    /// Resolve the effective target pane for actions like Copy/Paste/Find.
    /// When focus_area is EditorDock, targets the active editor tab instead of the
    /// focused terminal.
    fn action_target_id(&self) -> Option<tide_core::PaneId> {
        if self.focus_area == FocusArea::EditorDock {
            self.active_editor_tab()
        } else {
            self.focused
        }
    }

    /// Reverse-resolve a FocusArea to its slot number (1, 2, or 3) based on current layout.
    /// Used by titlebar buttons to show the correct ⌘N hint.
    pub(crate) fn slot_number_for_area(&self, target: FocusArea) -> u8 {
        let areas = self.area_ordering();
        areas.iter().position(|&a| a == target).map(|i| (i + 1) as u8).unwrap_or(0)
    }

    /// Build the left-to-right ordering of focus areas based on sidebar_side / dock_side.
    pub(crate) fn area_ordering(&self) -> Vec<FocusArea> {
        let mut areas = Vec::with_capacity(3);
        if self.sidebar_side == LayoutSide::Left { areas.push(FocusArea::FileTree); }
        if self.dock_side == LayoutSide::Left { areas.push(FocusArea::EditorDock); }
        areas.push(FocusArea::PaneArea);
        if self.dock_side == LayoutSide::Right { areas.push(FocusArea::EditorDock); }
        if self.sidebar_side == LayoutSide::Right { areas.push(FocusArea::FileTree); }
        areas
    }

    /// Resolve an AreaSlot (Cmd+1/2/3) to a FocusArea based on sidebar_side / dock_side.
    /// Left-to-right ordering: sidebar(if Left), dock(if Left), PaneArea, dock(if Right), sidebar(if Right)
    /// Slot1 = leftmost, Slot2 = middle, Slot3 = rightmost.
    fn resolve_slot(&self, slot: AreaSlot) -> FocusArea {
        let areas = self.area_ordering();

        match slot {
            AreaSlot::Slot1 => areas[0],
            AreaSlot::Slot2 => if areas.len() >= 2 { areas[1] } else { areas[0] },
            AreaSlot::Slot3 => if areas.len() >= 3 { areas[2] } else { areas.last().copied().unwrap_or(FocusArea::PaneArea) },
        }
    }

    /// Handle FocusArea(slot) — 3-stage toggle:
    /// FileTree/EditorDock: hidden→show+focus / unfocused→focus / focused→hide+PaneArea
    /// PaneArea: unfocused→focus / focused→Split↔Stacked
    pub(crate) fn handle_focus_area(&mut self, target: FocusArea) {
        // If zoomed into a different area, unzoom first
        if self.editor_panel_maximized && target != FocusArea::EditorDock {
            self.editor_panel_maximized = false;
            self.compute_layout();
        }
        if self.pane_area_maximized && target != FocusArea::PaneArea {
            self.pane_area_maximized = false;
            self.pane_area_mode = PaneAreaMode::Split;
            self.compute_layout();
        }

        match target {
            FocusArea::FileTree => {
                if self.focus_area == FocusArea::FileTree {
                    // Focused → hide + return to PaneArea
                    self.show_file_tree = false;
                    self.focus_area = FocusArea::PaneArea;
                    self.chrome_generation += 1;
                    self.compute_layout();
                } else if self.show_file_tree {
                    // Visible but not focused → focus
                    self.focus_area = FocusArea::FileTree;
                } else {
                    // Hidden → show + focus
                    self.show_file_tree = true;
                    self.focus_area = FocusArea::FileTree;
                    self.update_file_tree_cwd();
                    self.compute_layout();
                }
            }
            FocusArea::EditorDock => {
                if self.focus_area == FocusArea::EditorDock {
                    // Focused → hide + return to PaneArea
                    self.show_editor_panel = false;
                    self.editor_panel_auto_shown = false;
                    self.editor_panel_maximized = false;
                    self.editor_panel_width_manual = false;
                    self.focus_area = FocusArea::PaneArea;
                    self.chrome_generation += 1;
                    self.compute_layout();
                } else if self.show_editor_panel {
                    // Visible but not focused → focus
                    self.focus_area = FocusArea::EditorDock;
                } else {
                    // Hidden → show + focus
                    self.show_editor_panel = true;
                    self.focus_area = FocusArea::EditorDock;
                    self.compute_layout();
                }
            }
            FocusArea::PaneArea => {
                if self.focus_area == FocusArea::PaneArea {
                    // Already focused → toggle Split ↔ Stacked
                    if let Some(focused) = self.focused {
                        match self.pane_area_mode {
                            PaneAreaMode::Split => {
                                self.pane_area_mode = PaneAreaMode::Stacked(focused);
                            }
                            PaneAreaMode::Stacked(_) => {
                                self.pane_area_mode = PaneAreaMode::Split;
                            }
                        }
                        self.compute_layout();
                    }
                } else {
                    // Not focused → focus terminal
                    if let Some(tid) = self.focused_terminal_id() {
                        self.focus_terminal(tid);
                    } else {
                        self.focus_area = FocusArea::PaneArea;
                    }
                }
            }
        }
        self.chrome_generation += 1;
        self.needs_redraw = true;
    }

    /// Handle Navigate(direction) — route based on focus_area.
    fn handle_navigate(&mut self, direction: tide_input::Direction) {
        match self.focus_area {
            FocusArea::FileTree => {
                self.navigate_file_tree(direction);
            }
            FocusArea::PaneArea => {
                self.handle_move_focus(direction);
            }
            FocusArea::EditorDock => {
                self.navigate_dock_tabs(direction);
            }
        }
    }

    /// Handle ToggleZoom — context-dependent zoom.
    fn handle_toggle_zoom(&mut self) {
        match self.focus_area {
            FocusArea::EditorDock => {
                self.pane_area_maximized = false;
                self.editor_panel_maximized = !self.editor_panel_maximized;
                self.chrome_generation += 1;
                self.compute_layout();
            }
            FocusArea::PaneArea => {
                if let Some(focused) = self.focused {
                    self.editor_panel_maximized = false;
                    if self.pane_area_maximized {
                        // Unzoom: restore split mode
                        self.pane_area_maximized = false;
                        self.pane_area_mode = PaneAreaMode::Split;
                    } else {
                        // Zoom: stacked + maximize (hide dock)
                        self.pane_area_maximized = true;
                        self.pane_area_mode = PaneAreaMode::Stacked(focused);
                    }
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
            }
            FocusArea::FileTree => {
                // No-op for file tree zoom
            }
        }
    }

    pub(crate) fn handle_action(&mut self, action: Action, event: Option<InputEvent>) {
        match action {
            Action::RouteToPane(id) => {
                // Update focus
                if let Some(InputEvent::MouseClick { position, .. }) = event {
                    self.focus_terminal(id);

                    // Ctrl+Click / Cmd+Click on terminal -> try to open URL or file at click position
                    let mods = self.modifiers;
                    if mods.ctrl || mods.meta {
                        // Try URL first — open in embedded browser panel
                        if let Some(url) = self.extract_url_at(id, position) {
                            self.open_browser_pane(Some(url));
                            return;
                        }
                        if let Some(path) = self.extract_file_path_at(id, position) {
                            self.open_editor_pane(path);
                            return;
                        }
                    }

                    // Click on editor pane -> move cursor (skip in preview mode)
                    if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&id) {
                        if pane.preview_mode { return; }
                    }
                    let cell_size = self.cell_size();
                    if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&id) {
                        {
                            if let Some(&(_, rect)) = self.visual_pane_rects.iter().find(|(pid, _)| *pid == id) {
                                let content_top = self.pane_area_mode.content_top();
                                let inner_x = rect.x + PANE_PADDING;
                                let inner_y = rect.y + content_top;
                                let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cell_size.width;

                                let content_x = inner_x + gutter_width;
                                let rel_col = ((position.x - content_x) / cell_size.width).floor() as isize;
                                let rel_row = ((position.y - inner_y) / cell_size.height).floor() as isize;

                                if rel_row >= 0 && rel_col >= 0 {
                                    let line = pane.editor.scroll_offset() + rel_row as usize;
                                    let col = pane.editor.h_scroll_offset() + rel_col as usize;
                                    let visible_rows = ((rect.height - content_top - PANE_PADDING) / cell_size.height).floor() as usize;
                                    pane.handle_action(EditorAction::SetCursor { line, col }, visible_rows);
                                }
                            }
                        }
                    }
                }

                // Forward keyboard input to the pane
                let cs_for_keys = self.cell_size();
                if let Some(InputEvent::KeyPress { key, modifiers }) = event {
                    match self.panes.get_mut(&id) {
                        Some(PaneKind::Terminal(pane)) => {
                            pane.selection = None; // Clear selection on key input
                            pane.handle_key(&key, &modifiers);
                            self.input_just_sent = true;
                            self.input_sent_at = Some(Instant::now());
                        }
                        Some(PaneKind::Editor(pane)) => {
                            // Cmd+Shift+M / Ctrl+Shift+M: toggle markdown preview
                            if (modifiers.meta || modifiers.ctrl) && modifiers.shift {
                                if let tide_core::Key::Char('m') | tide_core::Key::Char('M') = &key {
                                    if pane.is_markdown() {
                                        pane.toggle_preview();
                                        self.chrome_generation += 1;
                                        self.pane_generations.remove(&id);
                                        self.needs_redraw = true;
                                        return;
                                    }
                                }
                            }

                            // In preview mode, only allow Escape, scroll keys
                            if pane.preview_mode {
                                // Compute visible rows for scroll clamping
                                let visible_rows = self.visual_pane_rects.iter().find(|(pid, _)| *pid == id)
                                    .map(|(_, rect)| {
                                        let content_top = self.pane_area_mode.content_top();
                                        ((rect.height - content_top - PANE_PADDING) / cs_for_keys.height).floor() as usize
                                    })
                                    .unwrap_or(30);
                                let total = pane.preview_line_count();
                                let max_scroll = total.saturating_sub(visible_rows);
                                match &key {
                                    tide_core::Key::Escape => {
                                        pane.toggle_preview();
                                        self.chrome_generation += 1;
                                        self.pane_generations.remove(&id);
                                        self.needs_redraw = true;
                                    }
                                    tide_core::Key::Char('j') | tide_core::Key::Down => {
                                        if pane.preview_scroll < max_scroll {
                                            pane.preview_scroll += 1;
                                            self.pane_generations.remove(&id);
                                            self.needs_redraw = true;
                                        }
                                    }
                                    tide_core::Key::Char('k') | tide_core::Key::Up => {
                                        if pane.preview_scroll > 0 {
                                            pane.preview_scroll -= 1;
                                            self.pane_generations.remove(&id);
                                            self.needs_redraw = true;
                                        }
                                    }
                                    tide_core::Key::Char('l') | tide_core::Key::Right => {
                                        let max_w = pane.preview_max_line_width();
                                        if pane.preview_h_scroll < max_w {
                                            pane.preview_h_scroll += 2;
                                            self.pane_generations.remove(&id);
                                            self.needs_redraw = true;
                                        }
                                    }
                                    tide_core::Key::Char('h') | tide_core::Key::Left => {
                                        if pane.preview_h_scroll > 0 {
                                            pane.preview_h_scroll = pane.preview_h_scroll.saturating_sub(2);
                                            self.pane_generations.remove(&id);
                                            self.needs_redraw = true;
                                        }
                                    }
                                    tide_core::Key::PageDown => {
                                        pane.preview_scroll = (pane.preview_scroll + 30).min(max_scroll);
                                        self.pane_generations.remove(&id);
                                        self.needs_redraw = true;
                                    }
                                    tide_core::Key::PageUp => {
                                        pane.preview_scroll = pane.preview_scroll.saturating_sub(30);
                                        self.pane_generations.remove(&id);
                                        self.needs_redraw = true;
                                    }
                                    _ => {} // Block all other input
                                }
                                return;
                            }

                            if let Some(action) = tide_editor::key_to_editor_action(&key, &modifiers) {
                                // Handle SelectAll: set selection, don't clear it
                                if matches!(action, tide_editor::EditorActionKind::SelectAll) {
                                    pane.select_all();
                                    return;
                                }
                                // Delete selection on editing actions (insert, backspace, delete, enter)
                                match &action {
                                    tide_editor::EditorActionKind::InsertChar(_)
                                    | tide_editor::EditorActionKind::Backspace
                                    | tide_editor::EditorActionKind::Delete
                                    | tide_editor::EditorActionKind::Enter => {
                                        pane.delete_selection();
                                    }
                                    _ => {}
                                }
                                // Clear selection on movement and editing keys
                                pane.selection = None;
                                let is_save = matches!(action, tide_editor::EditorActionKind::Save);
                                // Intercept Save on untitled files -> open save-as input
                                if is_save && pane.editor.file_path().is_none() {
                                    let base_dir = self.resolve_base_dir();
                                    let anchor = self.active_panel_tab_rect()
                                        .unwrap_or_else(|| tide_core::Rect::new(0.0, 0.0, 0.0, 0.0));
                                    self.save_as_input = Some(crate::SaveAsInput::new(id, base_dir, anchor));
                                    return;
                                }
                                let was_modified = pane.editor.is_modified();
                                let cell_size = Some(cs_for_keys);
                                let content_top = self.pane_area_mode.content_top();
                                let (visible_rows, visible_cols) = if let Some(cs) = cell_size {
                                    let tree_rect = self.visual_pane_rects.iter()
                                        .find(|(pid, _)| *pid == id)
                                        .map(|(_, r)| *r);
                                    if let Some(r) = tree_rect {
                                        let rows = ((r.height - content_top - PANE_PADDING) / cs.height).floor() as usize;
                                        let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
                                        let cols = ((r.width - 2.0 * PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
                                        (rows, cols)
                                    } else if let Some(pr) = self.editor_panel_rect {
                                        let content_height = (pr.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                                        let rows = (content_height / cs.height).floor() as usize;
                                        let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
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
                                // Refresh git status on save (async via git poller)
                                if is_save {
                                    self.trigger_git_poll();
                                }
                                // Invalidate cached pane texture and request redraw
                                self.pane_generations.remove(&id);
                                self.needs_redraw = true;
                            }
                        }
                        Some(PaneKind::Diff(_)) => {} // Diff pane has no keyboard input
                        Some(PaneKind::Browser(_)) => {} // Browser keyboard handled by webview / URL bar
                        None => {}
                    }
                }

                // Forward mouse scroll to pane
                if let Some(InputEvent::MouseScroll { delta, .. }) = event {
                    // Compute actual visible rows/cols for the pane
                    let content_top = self.pane_area_mode.content_top();
                    let (visible_rows, visible_cols) = {
                        let cs = self.cell_size();
                        let rect = self.visual_pane_rects.iter()
                            .find(|(pid, _)| *pid == id)
                            .map(|(_, r)| *r);
                        if let Some(r) = rect {
                            let rows = ((r.height - content_top - PANE_PADDING) / cs.height).floor() as usize;
                            let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
                            let cols = ((r.width - 2.0 * PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
                            (rows.max(1), cols.max(1))
                        } else if let Some(pr) = self.editor_panel_rect {
                            let content_height = (pr.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                            let rows = (content_height / cs.height).floor() as usize;
                            let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
                            let cols = ((pr.width - 2.0 * PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
                            (rows.max(1), cols.max(1))
                        } else {
                            (30, 80)
                        }
                    };
                    match self.panes.get_mut(&id) {
                        Some(PaneKind::Editor(pane)) if pane.preview_mode => {
                            let total = pane.preview_line_count();
                            let max_scroll = total.saturating_sub(visible_rows);
                            let scroll_amount = delta.abs() as usize;
                            if delta > 0.0 {
                                pane.preview_scroll = pane.preview_scroll.saturating_sub(scroll_amount);
                            } else {
                                pane.preview_scroll = (pane.preview_scroll + scroll_amount).min(max_scroll);
                            }
                            self.pane_generations.remove(&id);
                            self.needs_redraw = true;
                        }
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
                        Some(PaneKind::Browser(_)) => {} // Scroll handled by native WKWebView
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

    pub(crate) fn split_pane(&mut self, direction: SplitDirection, cwd: Option<std::path::PathBuf>) {
        if let Some(focused) = self.focused {
            let new_id = self.layout.split(focused, direction);
            self.create_terminal_pane(new_id, cwd);
            if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
                self.pane_area_mode = PaneAreaMode::Stacked(new_id);
            }
            self.focused = Some(new_id);
            self.router.set_focused(new_id);
            self.chrome_generation += 1;
            self.compute_layout();
        }
    }

    pub(crate) fn handle_global_action(&mut self, action: GlobalAction) {
        match action {
            GlobalAction::SplitVertical => {
                self.split_pane(SplitDirection::Vertical, None);
            }
            GlobalAction::SplitHorizontal => {
                self.split_pane(SplitDirection::Horizontal, None);
            }
            GlobalAction::SplitVerticalHere => {
                let cwd = self.focused_terminal_cwd();
                self.split_pane(SplitDirection::Vertical, cwd);
            }
            GlobalAction::SplitHorizontalHere => {
                let cwd = self.focused_terminal_cwd();
                self.split_pane(SplitDirection::Horizontal, cwd);
            }
            GlobalAction::ClosePane => {
                if self.focus_area == FocusArea::EditorDock {
                    // Close the active dock tab (editor/browser/diff)
                    if let Some(tab_id) = self.active_editor_tab() {
                        self.close_editor_panel_tab(tab_id);
                    } else {
                        // No active tab (empty panel) — hide it
                        self.show_editor_panel = false;
                        self.editor_panel_auto_shown = false;
                        self.editor_panel_maximized = false;
                        self.editor_panel_width_manual = false;
                        self.focus_area = FocusArea::PaneArea;
                        self.chrome_generation += 1;
                        self.compute_layout();
                    }
                } else if let Some(focused) = self.focused {
                    self.close_specific_pane(focused);
                }
            }
            GlobalAction::FocusArea(slot) => {
                let target = self.resolve_slot(slot);
                self.handle_focus_area(target);
            }
            GlobalAction::Navigate(direction) => {
                self.handle_navigate(direction);
            }
            GlobalAction::ToggleZoom => {
                self.handle_toggle_zoom();
            }
            GlobalAction::DockTabPrev => {
                self.navigate_dock_tabs(tide_input::Direction::Left);
            }
            GlobalAction::DockTabNext => {
                self.navigate_dock_tabs(tide_input::Direction::Right);
            }
            GlobalAction::FileFinder => {
                self.open_file_finder();
            }
            GlobalAction::ToggleFullscreen => {
                self.pending_fullscreen_toggle = true;
            }
            GlobalAction::Paste => {
                if let Some(target_id) = self.action_target_id() {
                    match self.panes.get_mut(&target_id) {
                        Some(PaneKind::Terminal(pane)) => {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                if let Ok(text) = clipboard.get_text() {
                                    if !text.is_empty() {
                                        // Scroll to bottom so pasted text is visible
                                        if pane.backend.display_offset() > 0 {
                                            pane.backend.request_scroll_to_bottom();
                                        }
                                        let bracketed = pane.backend.is_bracketed_paste_mode();
                                        let mut data = Vec::new();
                                        if bracketed {
                                            data.extend_from_slice(b"\x1b[200~");
                                            // Sanitize: strip the bracket-close sequence from
                                            // clipboard text to prevent pastejacking attacks
                                            // that escape bracketed paste mode.
                                            let safe = text.replace("\x1b[201~", "");
                                            data.extend_from_slice(safe.as_bytes());
                                        } else {
                                            data.extend_from_slice(text.as_bytes());
                                        }
                                        if bracketed {
                                            data.extend_from_slice(b"\x1b[201~");
                                            // Nudge shell to redraw and clear paste standout
                                            // (left + right arrow = net-zero cursor move that
                                            // triggers zsh/bash/fish to re-render without
                                            // the INVERSE highlight on pasted text).
                                            data.extend_from_slice(b"\x1b[D\x1b[C");
                                        }
                                        pane.backend.write(&data);
                                        self.input_just_sent = true;
                                        self.input_sent_at = Some(Instant::now());
                                    }
                                }
                            }
                        }
                        Some(PaneKind::Editor(pane)) => {
                            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                                if let Ok(text) = clipboard.get_text() {
                                    if !text.is_empty() {
                                        pane.delete_selection();
                                        pane.editor.insert_text(&text);
                                    }
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            GlobalAction::Copy => {
                if let Some(target_id) = self.action_target_id() {
                    match self.panes.get(&target_id) {
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
                        _ => {}
                    }
                }
            }
            GlobalAction::Find => {
                if let Some(target_id) = self.action_target_id() {
                    let has_search = match self.panes.get(&target_id) {
                        Some(PaneKind::Terminal(pane)) => pane.search.is_some(),
                        Some(PaneKind::Editor(pane)) => pane.search.is_some(),
                        _ => false,
                    };
                    if has_search {
                        self.search_focus = Some(target_id);
                    } else {
                        match self.panes.get_mut(&target_id) {
                            Some(PaneKind::Terminal(pane)) => {
                                pane.search = Some(SearchState::new());
                            }
                            Some(PaneKind::Editor(pane)) => {
                                pane.search = Some(SearchState::new());
                            }
                            _ => {}
                        }
                        self.search_focus = Some(target_id);
                    }
                }
            }
            GlobalAction::FontSizeUp => {
                self.apply_font_size(self.current_font_size + 1.0);
            }
            GlobalAction::FontSizeDown => {
                self.apply_font_size(self.current_font_size - 1.0);
            }
            GlobalAction::FontSizeReset => {
                self.apply_font_size(14.0);
            }
            GlobalAction::NewWindow => {
                if let Ok(exe) = std::env::current_exe() {
                    let _ = std::process::Command::new(exe).spawn();
                }
            }
            GlobalAction::NewFile => {
                self.new_editor_pane();
            }
            GlobalAction::OpenBrowser => {
                self.open_browser_pane(None);
            }
            GlobalAction::BrowserBack => {
                if let Some(active_id) = self.active_editor_tab() {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&active_id) {
                        bp.go_back();
                    }
                }
            }
            GlobalAction::BrowserForward => {
                if let Some(active_id) = self.active_editor_tab() {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&active_id) {
                        bp.go_forward();
                    }
                }
            }
            GlobalAction::OpenConfig => {
                self.toggle_config_page();
            }
            GlobalAction::ToggleTheme => {
                self.dark_mode = !self.dark_mode;
                let border_color = self.palette().border_color;
                if let Some(renderer) = &mut self.renderer {
                    renderer.clear_color = border_color;
                }
                let dark = self.dark_mode;
                for pane in self.panes.values_mut() {
                    match pane {
                        crate::pane::PaneKind::Terminal(tp) => {
                            tp.backend.set_dark_mode(dark);
                        }
                        crate::pane::PaneKind::Editor(ep) => {
                            ep.editor.set_dark_mode(dark);
                        }
                        crate::pane::PaneKind::Diff(_) => {}
                        crate::pane::PaneKind::Browser(_) => {}
                    }
                }
                self.chrome_generation += 1;
                self.layout_generation = self.layout_generation.wrapping_add(1);
                self.pane_generations.clear();
            }
        }
    }

    pub(crate) fn toggle_config_page(&mut self) {
        if self.config_page.is_some() {
            self.close_config_page();
        } else {
            self.open_config_page();
        }
        self.needs_redraw = true;
    }

    fn open_config_page(&mut self) {
        use tide_input::{GlobalAction as GA, KeybindingMap};

        let map = self.router.keybinding_map.as_ref();
        let all_actions = GA::all_actions();

        let bindings: Vec<(GA, tide_input::Hotkey)> = all_actions
            .into_iter()
            .map(|action| {
                let hotkey = map
                    .and_then(|m| m.hotkey_for(&action).cloned())
                    .or_else(|| {
                        let defaults = KeybindingMap::new();
                        defaults.hotkey_for(&action).cloned()
                    })
                    .unwrap_or(tide_input::Hotkey::new(
                        tide_core::Key::Char('?'),
                        false, false, false, false,
                    ));
                (action, hotkey)
            })
            .collect();

        let worktree_pattern = self.settings.worktree.base_dir_pattern
            .clone()
            .unwrap_or_default();

        let copy_files = self.settings.worktree.copy_files
            .as_ref()
            .map(|v| v.join(", "))
            .unwrap_or_default();

        self.config_page = Some(crate::ConfigPageState::new(bindings, worktree_pattern, copy_files));
        self.chrome_generation += 1;
    }

    pub(crate) fn close_config_page(&mut self) {
        let page = match self.config_page.take() {
            Some(p) => p,
            None => return,
        };

        if page.dirty {
            // Save keybinding overrides
            let defaults = tide_input::KeybindingMap::default_bindings();
            let overrides: Vec<crate::settings::KeybindingOverride> = page
                .bindings
                .iter()
                .filter(|(action, hotkey)| {
                    // Only save if different from default
                    !defaults.iter().any(|(dh, da)| {
                        da.action_key() == action.action_key()
                            && dh.key_name() == hotkey.key_name()
                            && dh.shift == hotkey.shift
                            && dh.ctrl == hotkey.ctrl
                            && dh.meta == hotkey.meta
                            && dh.alt == hotkey.alt
                    })
                })
                .map(|(action, hotkey)| {
                    crate::settings::KeybindingOverride::from_binding(hotkey, action)
                })
                .collect();

            self.settings.keybindings = overrides;

            // Save worktree pattern
            let wt_text = page.worktree_input.text.trim().to_string();
            self.settings.worktree.base_dir_pattern = if wt_text.is_empty() {
                None
            } else {
                Some(wt_text)
            };

            // Save copy files
            let cf_text = page.copy_files_input.text.trim().to_string();
            self.settings.worktree.copy_files = if cf_text.is_empty() {
                None
            } else {
                let files: Vec<String> = cf_text
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if files.is_empty() { None } else { Some(files) }
            };

            crate::settings::save_settings(&self.settings);

            // Rebuild keybinding map on router
            let map = crate::settings::build_keybinding_map(&self.settings);
            if map.bindings.len() == tide_input::KeybindingMap::default_bindings().len()
                && self.settings.keybindings.is_empty()
            {
                self.router.keybinding_map = None;
            } else {
                self.router.keybinding_map = Some(map);
            }
        }

        self.chrome_generation += 1;
    }
}
