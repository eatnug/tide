mod pane_lifecycle;
mod focus_nav;
mod text_extract;
mod file_ops;

use std::time::Instant;

use tide_core::{InputEvent, LayoutEngine, Renderer, Size, SplitDirection, TerminalBackend, Vec2};
use tide_editor::input::EditorAction;
use tide_input::{Action, GlobalAction};
use crate::search::SearchState;

use crate::input::winit_modifiers_to_tide;
use crate::pane::PaneKind;
use crate::theme::*;
use crate::{App, PaneAreaMode};

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

                    // Ctrl+Click / Cmd+Click on terminal -> try to open URL or file at click position
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

                    // Click on editor pane -> move cursor
                    if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&id) {
                        if let Some(renderer) = self.renderer.as_ref() {
                            if let Some(&(_, rect)) = self.visual_pane_rects.iter().find(|(pid, _)| *pid == id) {
                                let cell_size = renderer.cell_size();
                                let content_top = self.pane_area_mode.content_top();
                                let inner_x = rect.x + PANE_PADDING;
                                let inner_y = rect.y + content_top;
                                let gutter_width = 5.0 * cell_size.width; // GUTTER_WIDTH_CELLS

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
                                // Intercept Save on untitled files -> open save-as input
                                if is_save && pane.editor.file_path().is_none() {
                                    let base_dir = self.resolve_base_dir();
                                    self.save_as_input = Some(crate::SaveAsInput::new(id, base_dir));
                                    return;
                                }
                                let was_modified = pane.editor.is_modified();
                                let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                                let content_top = self.pane_area_mode.content_top();
                                let (visible_rows, visible_cols) = if let Some(cs) = cell_size {
                                    let tree_rect = self.visual_pane_rects.iter()
                                        .find(|(pid, _)| *pid == id)
                                        .map(|(_, r)| *r);
                                    if let Some(r) = tree_rect {
                                        let rows = ((r.height - content_top - PANE_PADDING) / cs.height).floor() as usize;
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
                    let content_top = self.pane_area_mode.content_top();
                    let (visible_rows, visible_cols) = self.renderer.as_ref().map(|r| {
                        let cs = r.cell_size();
                        let rect = self.visual_pane_rects.iter()
                            .find(|(pid, _)| *pid == id)
                            .map(|(_, r)| *r);
                        if let Some(r) = rect {
                            let rows = ((r.height - content_top - PANE_PADDING) / cs.height).floor() as usize;
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
                    if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
                        self.pane_area_mode = PaneAreaMode::Stacked(new_id);
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
                    if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
                        self.pane_area_mode = PaneAreaMode::Stacked(new_id);
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
                    self.is_fullscreen = !self.is_fullscreen;
                    if self.is_fullscreen {
                        window.set_fullscreen(Some(winit::window::Fullscreen::Borderless(None)));
                        self.top_inset = 0.0;
                    } else {
                        window.set_fullscreen(None);
                        self.top_inset = if cfg!(target_os = "macos") { TITLEBAR_HEIGHT } else { 0.0 };
                    }
                    self.chrome_generation += 1;
                    self.compute_layout();
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
                        // Search already open -> just (re-)focus it
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
                self.handle_move_focus(direction);
            }
            GlobalAction::ToggleMaximizePane => {
                if let Some(focused) = self.focused {
                    let in_panel = self.editor_panel_tabs.contains(&focused)
                        || self.editor_panel_placeholder == Some(focused);
                    if in_panel {
                        // Toggle editor panel maximize
                        self.editor_panel_maximized = !self.editor_panel_maximized;
                        self.pane_area_mode = PaneAreaMode::Split; // mutually exclusive
                    } else if self.editor_panel_maximized {
                        // Editor panel is maximized but focus is elsewhere -- just unmaximize it
                        self.editor_panel_maximized = false;
                    } else {
                        // Toggle pane area mode between Split and Stacked
                        match self.pane_area_mode {
                            PaneAreaMode::Split => {
                                self.pane_area_mode = PaneAreaMode::Stacked(focused);
                            }
                            PaneAreaMode::Stacked(_) => {
                                self.pane_area_mode = PaneAreaMode::Split;
                            }
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
}
