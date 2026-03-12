mod pane_lifecycle;
mod focus_nav;
mod text_extract;
mod file_ops;

pub(crate) use pane_lifecycle::LauncherChoice;

use std::time::Instant;

use tide_core::{InputEvent, LayoutEngine, Size, SplitDirection, TerminalBackend, Vec2};
use tide_editor::input::EditorAction;
use tide_input::{Action, AreaSlot, GlobalAction};
use crate::search::SearchState;

use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui_state::FocusArea;
use crate::App;

impl App {
    fn cleanup_closed_pane_state(&mut self, pane_id: tide_core::PaneId) {
        self.cache.invalidate_pane(pane_id);
        self.interaction.scroll_accumulator.remove(&pane_id);
        self.ime.pending_removes.push(pane_id);
        // Clear IME composition if the closing pane was the composition target.
        // Without this, last_target points to a deleted pane and preedit text is lost.
        if self.ime.last_target == Some(pane_id) {
            self.ime.clear_composition();
            self.ime.last_target = None;
        }
        if let Some(renderer) = self.renderer.as_mut() {
            renderer.remove_pane_cache(pane_id);
        }
    }

    /// Switch primary focus to a pane, setting focus to PaneArea.
    pub(crate) fn focus_terminal(&mut self, id: tide_core::PaneId) {
        self.focus_area = FocusArea::PaneArea;
        if self.focused == Some(id) {
            return;
        }
        self.focused = Some(id);
        self.router.set_focused(id);
        self.cache.invalidate_chrome();
        self.update_file_tree_cwd();
        // Immediately sync webview visibility so the browser hides/shows
        // without waiting for the next update() tick (which may be gated by
        // is_rapid).
        self.sync_browser_webview_frames();
    }

    /// Resolve the effective target pane for actions like Copy/Paste/Find.
    fn action_target_id(&self) -> Option<tide_core::PaneId> {
        self.focused
    }

    /// Build the left-to-right ordering of focus areas based on sidebar_side.
    pub(crate) fn area_ordering(&self) -> Vec<FocusArea> {
        let mut areas = Vec::with_capacity(2);
        if self.sidebar_side == crate::LayoutSide::Left { areas.push(FocusArea::FileTree); }
        areas.push(FocusArea::PaneArea);
        if self.sidebar_side == crate::LayoutSide::Right { areas.push(FocusArea::FileTree); }
        areas
    }

    /// Resolve an AreaSlot (Cmd+1/2) to a FocusArea based on sidebar_side.
    /// Slot1 = leftmost, Slot2 = rightmost.
    fn resolve_slot(&self, slot: AreaSlot) -> FocusArea {
        let areas = self.area_ordering();

        match slot {
            AreaSlot::Slot1 => areas[0],
            AreaSlot::Slot2 => if areas.len() >= 2 { areas[1] } else { areas[0] },
            AreaSlot::Slot3 => areas.last().copied().unwrap_or(FocusArea::PaneArea),
        }
    }

    /// Handle FocusArea(slot) — toggle:
    /// FileTree: hidden→show+focus / unfocused→focus / focused→hide+PaneArea
    /// PaneArea: unfocused→focus
    pub(crate) fn handle_focus_area(&mut self, target: FocusArea) {
        match target {
            FocusArea::FileTree => {
                if self.focus_area == FocusArea::FileTree {
                    // Focused → hide + return to PaneArea
                    self.ft.visible = false;
                    self.focus_area = FocusArea::PaneArea;
                    self.cache.invalidate_chrome();
                    self.compute_layout();
                } else if self.ft.visible {
                    // Visible but not focused → focus
                    self.focus_area = FocusArea::FileTree;
                } else {
                    // Hidden → show + focus
                    self.ft.visible = true;
                    self.focus_area = FocusArea::FileTree;
                    self.update_file_tree_cwd();
                    self.compute_layout();
                }
            }
            FocusArea::PaneArea => {
                if self.focus_area != FocusArea::PaneArea {
                    // Not focused → focus
                    if let Some(focused) = self.focused {
                        self.focus_terminal(focused);
                    } else {
                        self.focus_area = FocusArea::PaneArea;
                    }
                }
            }
        }
        self.cache.invalidate_chrome();
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
        }
    }

    /// Handle ToggleZoom — context-dependent zoom.
    fn handle_toggle_zoom(&mut self) {
        match self.focus_area {
            FocusArea::PaneArea => {
                if let Some(focused) = self.focused {
                    if self.zoomed_pane == Some(focused) {
                        // Unzoom
                        self.zoomed_pane = None;
                    } else {
                        // Zoom the focused pane
                        self.zoomed_pane = Some(focused);
                    }
                    self.cache.pane_generations.clear();
                    self.cache.invalidate_chrome();
                    self.compute_layout();
                }
            }
            FocusArea::FileTree => {}
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
                        if let Some((path, line)) = self.extract_file_path_at(id, position) {
                            self.open_editor_pane_at_line(path, line);
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
                                let content_top = TAB_BAR_HEIGHT;
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
                            if pane.child_dead {
                                // Dead terminal: any key respawns a new shell
                                self.respawn_terminal(id);
                            } else {
                                pane.selection = None; // Clear selection on key input
                                pane.handle_key(&key, &modifiers);
                                self.input_just_sent = true;
                                self.input_sent_at = Some(Instant::now());
                            }
                        }
                        Some(PaneKind::Editor(pane)) => {
                            // Cmd+Shift+M / Ctrl+Shift+M: toggle markdown preview
                            if (modifiers.meta || modifiers.ctrl) && modifiers.shift {
                                if let tide_core::Key::Char('m') | tide_core::Key::Char('M') = &key {
                                    if pane.is_markdown() {
                                        pane.toggle_preview();
                                        self.cache.invalidate_chrome();
                                        self.cache.invalidate_pane(id);
                                        return;
                                    }
                                }
                            }

                            // Preview mode: Escape exits, all other keys ignored.
                            // Scrolling handled by Cmd+D/U (ScrollHalfPage) and mouse/trackpad.
                            if pane.preview_mode {
                                if matches!(key, tide_core::Key::Escape) {
                                    pane.toggle_preview();
                                    self.cache.invalidate_chrome();
                                    self.cache.invalidate_pane(id);
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
                                    let anchor = self.visual_pane_rects.iter()
                                        .find(|(pid, _)| *pid == id)
                                        .map(|(_, r)| tide_core::Rect::new(r.x, r.y, r.width, crate::theme::TAB_BAR_HEIGHT))
                                        .unwrap_or_else(|| tide_core::Rect::new(0.0, 0.0, 0.0, 0.0));
                                    self.modal.save_as_input = Some(crate::SaveAsInput::new(id, base_dir, anchor));
                                    return;
                                }
                                let was_modified = pane.editor.is_modified();
                                let cell_size = Some(cs_for_keys);
                                let content_top = TAB_BAR_HEIGHT;
                                let (visible_rows, visible_cols) = if let Some(cs) = cell_size {
                                    let tree_rect = self.visual_pane_rects.iter()
                                        .find(|(pid, _)| *pid == id)
                                        .map(|(_, r)| *r);
                                    if let Some(r) = tree_rect {
                                        let rows = ((r.height - content_top - PANE_PADDING) / cs.height).floor() as usize;
                                        let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
                                        let cols = ((r.width - 2.0 * PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
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
                                    self.cache.invalidate_chrome();
                                }
                                // Refresh git status on save (async via git poller)
                                if is_save {
                                    self.trigger_git_poll();
                                }
                                // Invalidate cached pane texture and request redraw
                                self.cache.invalidate_pane(id);
                            }
                        }
                        Some(PaneKind::Diff(_)) => {} // Diff pane has no keyboard input
                        Some(PaneKind::Browser(_)) => {} // Browser keyboard handled by webview / URL bar
                        Some(PaneKind::Launcher(_)) => {
                            // Launcher key handling: T/E/O/B to select pane type, Escape to close
                            let choice = match key {
                                tide_core::Key::Char('t') | tide_core::Key::Char('T') => {
                                    Some(crate::action::pane_lifecycle::LauncherChoice::Terminal)
                                }
                                tide_core::Key::Char('e') | tide_core::Key::Char('E') => {
                                    Some(crate::action::pane_lifecycle::LauncherChoice::NewFile)
                                }
                                tide_core::Key::Char('o') | tide_core::Key::Char('O') => {
                                    Some(crate::action::pane_lifecycle::LauncherChoice::OpenFile)
                                }
                                tide_core::Key::Char('b') | tide_core::Key::Char('B') => {
                                    Some(crate::action::pane_lifecycle::LauncherChoice::Browser)
                                }
                                tide_core::Key::Escape => {
                                    self.close_specific_pane(id);
                                    None
                                }
                                _ => None,
                            };
                            if let Some(c) = choice {
                                self.resolve_launcher(id, c);
                            }
                        }
                        None => {}
                    }
                }

                // Forward mouse scroll to pane
                if let Some(InputEvent::MouseScroll { delta, .. }) = event {
                    // Compute actual visible rows/cols for the pane
                    let content_top = TAB_BAR_HEIGHT;
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
                        } else {
                            (30, 80)
                        }
                    };
                    match self.panes.get_mut(&id) {
                        Some(PaneKind::Editor(pane)) if pane.preview_mode => {
                            let acc = self.interaction.scroll_accumulator.entry(id).or_insert(0.0);
                            *acc += delta;
                            let lines = acc.trunc() as i32;
                            if lines != 0 {
                                *acc -= lines as f32;
                                let total = pane.preview_line_count();
                                let max_scroll = total.saturating_sub(visible_rows);
                                if lines > 0 {
                                    pane.preview_scroll = pane.preview_scroll.saturating_sub(lines.unsigned_abs() as usize);
                                } else {
                                    pane.preview_scroll = (pane.preview_scroll + lines.unsigned_abs() as usize).min(max_scroll);
                                }
                                self.cache.invalidate_pane(id);
                            }
                        }
                        Some(PaneKind::Editor(pane)) => {
                            // Accumulate sub-pixel scroll deltas (like terminal)
                            let acc = self.interaction.scroll_accumulator.entry(id).or_insert(0.0);
                            *acc += delta;
                            let lines = acc.trunc();
                            if lines.abs() >= 1.0 {
                                *acc -= lines;
                                if lines > 0.0 {
                                    pane.handle_action_with_size(EditorAction::ScrollUp(lines.abs()), visible_rows, visible_cols);
                                } else {
                                    pane.handle_action_with_size(EditorAction::ScrollDown(lines.abs()), visible_rows, visible_cols);
                                }
                                self.cache.invalidate_pane(id);
                            }
                        }
                        Some(PaneKind::Terminal(pane)) => {
                            // Accumulate sub-pixel scroll deltas to prevent jitter
                            let acc = self.interaction.scroll_accumulator.entry(id).or_insert(0.0);
                            *acc += delta;
                            let lines = acc.trunc() as i32;
                            if lines != 0 {
                                *acc -= lines as f32;
                                pane.scroll_display(lines);
                                pane.backend.process();
                                self.cache.invalidate_pane(id);
                            }
                        }
                        Some(PaneKind::Diff(dp)) => {
                            let total = dp.total_lines() as f32;
                            dp.scroll_target = (dp.scroll_target - delta).clamp(0.0, total.max(0.0));
                            dp.scroll = dp.scroll_target;
                            dp.generation = dp.generation.wrapping_add(1);
                            self.cache.invalidate_pane(id);
                        }
                        Some(PaneKind::Browser(_)) => {} // Scroll handled by native WKWebView
                        Some(PaneKind::Launcher(_)) => {}
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
                if self.ft.visible {
                    match self.sidebar_side {
                        crate::LayoutSide::Left => left += self.ft.width,
                        crate::LayoutSide::Right => right += self.ft.width,
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
            self.split_pane_from(focused, direction, cwd);
        }
    }

    /// Split from a specific source pane, creating a new terminal pane with
    /// proper focus, chrome updates.
    /// Returns the new pane ID on success.
    pub(crate) fn split_pane_from(
        &mut self,
        source: tide_core::PaneId,
        direction: SplitDirection,
        cwd: Option<std::path::PathBuf>,
    ) -> Option<tide_core::PaneId> {
        // Unzoom before splitting so both panes are visible
        if self.zoomed_pane.is_some() {
            self.zoomed_pane = None;
            self.cache.pane_generations.clear();
        }
        let new_id = self.layout.split(source, direction);
        self.create_terminal_pane(new_id, cwd);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.cache.invalidate_chrome();
        self.compute_layout();
        Some(new_id)
    }

    pub(crate) fn handle_global_action(&mut self, action: GlobalAction) {
        match action {
            GlobalAction::SplitVertical | GlobalAction::SplitVerticalHere => {
                self.split_with_launcher(SplitDirection::Vertical);
            }
            GlobalAction::SplitHorizontal | GlobalAction::SplitHorizontalHere => {
                self.split_with_launcher(SplitDirection::Horizontal);
            }
            GlobalAction::ClosePane => {
                if let Some(focused) = self.focused {
                    self.close_specific_pane(focused);
                }
            }
            GlobalAction::FocusArea(slot) => {
                let target = self.resolve_slot(slot);
                self.handle_focus_area(target);
            }
            GlobalAction::WorkspacePrev => {
                let len = self.ws.workspaces.len();
                if len > 0 {
                    let prev = if self.ws.active == 0 { len - 1 } else { self.ws.active - 1 };
                    self.switch_workspace(prev);
                }
            }
            GlobalAction::WorkspaceNext => {
                let len = self.ws.workspaces.len();
                if len > 0 {
                    let next = if self.ws.active + 1 >= len { 0 } else { self.ws.active + 1 };
                    self.switch_workspace(next);
                }
            }
            GlobalAction::NewWorkspace => {
                self.new_workspace();
            }
            GlobalAction::CloseWorkspace => {
                self.close_workspace();
            }
            GlobalAction::ToggleFileTree => {
                self.handle_focus_area(FocusArea::FileTree);
            }
            GlobalAction::ToggleWorkspaceSidebar => {
                self.ws.show_sidebar = !self.ws.show_sidebar;
                self.cache.invalidate_chrome();
                self.compute_layout();
            }
            GlobalAction::Navigate(direction) => {
                self.handle_navigate(direction);
            }
            GlobalAction::ToggleZoom => {
                self.handle_toggle_zoom();
            }
            GlobalAction::TabPrev => {
                // Navigate to previous tab in the current tab group
                self.navigate_tab_group(tide_input::Direction::Left);
            }
            GlobalAction::TabNext => {
                // Navigate to next tab in the current tab group
                self.navigate_tab_group(tide_input::Direction::Right);
            }
            GlobalAction::NewTab => {
                // Open a new terminal tab in the focused pane's TabGroup
                self.new_terminal_tab();
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
                if let Some(focused) = self.focused {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&focused) {
                        bp.go_back();
                    }
                }
            }
            GlobalAction::BrowserForward => {
                if let Some(focused) = self.focused {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&focused) {
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
                        crate::pane::PaneKind::Launcher(_) => {}
                    }
                }
                self.cache.invalidate_chrome();
                self.cache.layout_generation = self.cache.layout_generation.wrapping_add(1);
                self.cache.pane_generations.clear();
            }
            GlobalAction::ScrollHalfPageUp => {
                self.scroll_half_page(tide_input::Direction::Up);
            }
            GlobalAction::ScrollHalfPageDown => {
                self.scroll_half_page(tide_input::Direction::Down);
            }
        }
    }

    pub(crate) fn toggle_config_page(&mut self) {
        if self.modal.config_page.is_some() {
            self.close_config_page();
        } else {
            self.open_config_page();
        }
        self.cache.needs_redraw = true;
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

        self.modal.config_page = Some(crate::ConfigPageState::new(bindings, worktree_pattern, copy_files));
        self.cache.invalidate_chrome();
    }

    pub(crate) fn close_config_page(&mut self) {
        let page = match self.modal.config_page.take() {
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

        self.cache.invalidate_chrome();
    }

    /// Navigate tabs within the current pane's tab group (Left = prev, Right = next).
    fn navigate_tab_group(&mut self, direction: tide_input::Direction) {
        let current_id = match self.focused {
            Some(id) => id,
            None => return,
        };
        // Scope the immutable borrow of self.layout so it's dropped before set_active_tab
        let new_tab = {
            let tg = match self.layout.tab_group_containing(current_id) {
                Some(tg) => tg,
                None => return,
            };
            if tg.tabs.len() < 2 {
                return;
            }
            let idx = match tg.tabs.iter().position(|&id| id == current_id) {
                Some(i) => i,
                None => return,
            };
            let len = tg.tabs.len();
            let new_idx = match direction {
                tide_input::Direction::Left => {
                    if idx > 0 { idx - 1 } else { len - 1 }
                }
                tide_input::Direction::Right => {
                    if idx + 1 < len { idx + 1 } else { 0 }
                }
                _ => return,
            };
            tg.tabs[new_idx]
        };
        self.layout.set_active_tab(new_tab);
        self.cache.invalidate_pane(current_id);
        self.cache.invalidate_pane(new_tab);
        self.focused = Some(new_tab);
        self.router.set_focused(new_tab);
        // Keep zoom on the new tab so it stays fullscreen
        if self.zoomed_pane.is_some() {
            self.zoomed_pane = Some(new_tab);
        }
        self.cache.invalidate_chrome();
        self.compute_layout();
    }
}
