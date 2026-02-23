use tide_core::{InputEvent, Renderer};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;

impl App {
    /// Handle scroll event with pre-processed delta values.
    /// dx/dy are in "line" units (platform normalizes pixel/line deltas).
    pub(crate) fn handle_scroll(&mut self, dx: f32, dy: f32) {
        // Popup scroll: config page
        if let Some(ref mut cp) = self.config_page {
            if matches!(cp.section, crate::ui_state::ConfigSection::Keybindings) {
                let lines = if dy.abs() >= 1.0 { dy.abs().ceil() as usize } else { 1 };
                let max_visible = CONFIG_PAGE_MAX_VISIBLE;
                if dy > 0.0 {
                    cp.scroll_offset = cp.scroll_offset.saturating_sub(lines);
                } else if dy < 0.0 {
                    let max_off = cp.bindings.len().saturating_sub(max_visible);
                    cp.scroll_offset = (cp.scroll_offset + lines).min(max_off);
                }
                self.chrome_generation += 1;
            }
            self.needs_redraw = true;
            return;
        }

        // Popup scroll: git switcher
        if self.git_switcher.is_some() && self.git_switcher_contains(self.last_cursor_pos) {
            if let Some(ref mut gs) = self.git_switcher {
                let max_visible = crate::GIT_SWITCHER_MAX_VISIBLE;
                let lines = if dy.abs() >= 1.0 { dy.abs().ceil() as usize } else { 1 };
                let filtered_len = gs.current_filtered_len();
                if dy > 0.0 {
                    gs.scroll_offset = gs.scroll_offset.saturating_sub(lines);
                } else if dy < 0.0 {
                    let max_off = filtered_len.saturating_sub(max_visible);
                    gs.scroll_offset = (gs.scroll_offset + lines).min(max_off);
                }
                self.chrome_generation += 1;
            }
            self.needs_redraw = true;
            return;
        }

        // Popup scroll: file switcher
        if self.file_switcher.is_some() && self.file_switcher_contains(self.last_cursor_pos) {
            if let Some(ref mut fs) = self.file_switcher {
                let max_visible = 10usize;
                let lines = if dy.abs() >= 1.0 { dy.abs().ceil() as usize } else { 1 };
                if dy > 0.0 {
                    fs.scroll_offset = fs.scroll_offset.saturating_sub(lines);
                } else if dy < 0.0 {
                    let max_off = fs.filtered.len().saturating_sub(max_visible);
                    fs.scroll_offset = (fs.scroll_offset + lines).min(max_off);
                }
                self.chrome_generation += 1;
            }
            self.needs_redraw = true;
            return;
        }

        // File finder scroll (mouse wheel over editor panel while finder is open)
        if self.file_finder.is_some() {
            if let Some(panel_rect) = self.editor_panel_rect {
                if panel_rect.contains(self.last_cursor_pos) {
                    if let Some(ref mut finder) = self.file_finder {
                        let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                        if let Some(cs) = cell_size {
                            let line_height = cs.height * FILE_TREE_LINE_SPACING;
                            let input_y = panel_rect.y + PANE_PADDING + 8.0;
                            let input_h = cs.height + 12.0;
                            let list_top = input_y + input_h + 8.0;
                            let list_bottom = panel_rect.y + panel_rect.height - PANE_PADDING;
                            let visible_rows = ((list_bottom - list_top) / line_height).floor() as usize;
                            let lines = if dy.abs() >= 1.0 { dy.abs().ceil() as usize } else { 1 };
                            if dy > 0.0 {
                                finder.scroll_offset = finder.scroll_offset.saturating_sub(lines);
                            } else if dy < 0.0 {
                                let max_off = finder.filtered.len().saturating_sub(visible_rows);
                                finder.scroll_offset = (finder.scroll_offset + lines).min(max_off);
                            }
                            self.chrome_generation += 1;
                        }
                    }
                    self.needs_redraw = true;
                    return;
                }
            }
        }

        // Axis isolation for editor content: only apply dominant scroll axis
        let (editor_dx, editor_dy) = if dx.abs() > dy.abs() {
            (dx, 0.0)
        } else {
            (0.0, dy)
        };

        // Check if scrolling over the file tree
        if self.show_file_tree && self.file_tree_rect.is_some_and(|r| self.last_cursor_pos.x >= r.x && self.last_cursor_pos.x < r.x + r.width) {
            let max_scroll = self.file_tree_max_scroll();
            let new_val = (self.file_tree_scroll - dy * 30.0).clamp(0.0, max_scroll);
            if new_val != self.file_tree_scroll {
                self.file_tree_scroll = new_val;
                self.file_tree_scroll_target = new_val;
                self.chrome_generation += 1;
                self.needs_redraw = true;
            }
        } else if self.is_over_panel_tab_bar(self.last_cursor_pos) {
            // Horizontal scroll for panel tab bar
            self.panel_tab_scroll_target -= dx * 20.0;
            self.panel_tab_scroll_target -= dy * 20.0;
            self.clamp_panel_tab_scroll();
        } else if let Some(panel_rect) = self.editor_panel_rect {
            if panel_rect.contains(self.last_cursor_pos) {
                // Route scroll to active panel editor
                if let Some(active_id) = self.active_editor_tab() {
                    let (visible_rows, visible_cols) = self.renderer.as_ref().map(|r| {
                        let cs = r.cell_size();
                        let content_height = (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                        let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
                        let content_width = (panel_rect.width - 2.0 * PANE_PADDING - 2.0 * gutter_width).max(1.0);
                        let rows = (content_height / cs.height).floor() as usize;
                        let cols = (content_width / cs.width).floor() as usize;
                        (rows, cols)
                    }).unwrap_or((30, 80));
                    match self.panes.get_mut(&active_id) {
                        Some(PaneKind::Editor(pane)) if pane.preview_mode => {
                            let total = pane.preview_line_count();
                            let max_scroll = total.saturating_sub(visible_rows);
                            let scroll_lines = if editor_dy.abs() >= 1.0 { editor_dy.abs().ceil() as usize } else { 1 };
                            if editor_dy > 0.0 {
                                pane.preview_scroll = pane.preview_scroll.saturating_sub(scroll_lines);
                            } else if editor_dy < 0.0 {
                                pane.preview_scroll = (pane.preview_scroll + scroll_lines).min(max_scroll);
                            }
                            self.pane_generations.remove(&active_id);
                            self.needs_redraw = true;
                        }
                        Some(PaneKind::Editor(pane)) => {
                            use tide_editor::input::EditorAction;
                            if editor_dy > 0.0 {
                                pane.handle_action_with_size(EditorAction::ScrollUp(editor_dy.abs()), visible_rows, visible_cols);
                            } else if editor_dy < 0.0 {
                                pane.handle_action_with_size(EditorAction::ScrollDown(editor_dy.abs()), visible_rows, visible_cols);
                            }
                            if editor_dx > 0.0 {
                                pane.handle_action_with_size(EditorAction::ScrollLeft(editor_dx.abs()), visible_rows, visible_cols);
                            } else if editor_dx < 0.0 {
                                pane.handle_action_with_size(EditorAction::ScrollRight(editor_dx.abs()), visible_rows, visible_cols);
                            }
                            self.pane_generations.remove(&active_id);
                            self.needs_redraw = true;
                        }
                        Some(PaneKind::Diff(dp)) => {
                            let total = dp.total_lines();
                            let max_scroll = total.saturating_sub(visible_rows) as f32;
                            if editor_dy > 0.0 {
                                dp.scroll_target = (dp.scroll_target - editor_dy * 3.0).max(0.0);
                            } else if editor_dy < 0.0 {
                                dp.scroll_target = (dp.scroll_target - editor_dy * 3.0).min(max_scroll);
                            }
                            if editor_dx != 0.0 {
                                let delta = (editor_dx.abs() * 3.0).ceil() as usize;
                                let max_h = dp.max_line_len().saturating_sub(visible_cols.saturating_sub(4));
                                if editor_dx > 0.0 {
                                    dp.h_scroll = dp.h_scroll.saturating_sub(delta);
                                } else {
                                    dp.h_scroll = (dp.h_scroll + delta).min(max_h);
                                }
                            }
                            dp.scroll = dp.scroll_target;
                            dp.generation = dp.generation.wrapping_add(1);
                            self.pane_generations.remove(&active_id);
                            self.needs_redraw = true;
                        }
                        _ => {}
                    }
                }
            } else {
                let input = InputEvent::MouseScroll {
                    delta: editor_dy,
                    position: self.last_cursor_pos,
                };
                let action = self.router.process(input, &self.pane_rects);
                self.handle_action(action, Some(input));
            }
        } else {
            let input = InputEvent::MouseScroll {
                delta: editor_dy,
                position: self.last_cursor_pos,
            };
            let action = self.router.process(input, &self.pane_rects);
            self.handle_action(action, Some(input));
        }
        // Horizontal scroll for editor/diff panes (trackpad two-finger swipe)
        if editor_dx != 0.0 {
            let editor_pane_id = self.visual_pane_rects.iter()
                .find(|(_, r)| r.contains(self.last_cursor_pos))
                .map(|(id, r)| (*id, *r));
            if let Some((pid, rect)) = editor_pane_id {
                match self.panes.get_mut(&pid) {
                    Some(PaneKind::Editor(pane)) => {
                        use tide_editor::input::EditorAction;
                        let visible_cols = self.renderer.as_ref().map(|r| {
                            let cs = r.cell_size();
                            let gutter = 5.0 * cs.width;
                            ((rect.width - 2.0 * PANE_PADDING - 2.0 * gutter) / cs.width).floor() as usize
                        }).unwrap_or(80);
                        let scroll_top_off = self.pane_area_mode.content_top();
                        let visible_rows = self.renderer.as_ref().map(|r| {
                            let cs = r.cell_size();
                            ((rect.height - scroll_top_off - PANE_PADDING) / cs.height).floor() as usize
                        }).unwrap_or(30);
                        if editor_dx > 0.0 {
                            pane.handle_action_with_size(EditorAction::ScrollLeft(editor_dx.abs()), visible_rows, visible_cols);
                        } else {
                            pane.handle_action_with_size(EditorAction::ScrollRight(editor_dx.abs()), visible_rows, visible_cols);
                        }
                        self.pane_generations.remove(&pid);
                        self.needs_redraw = true;
                    }
                    Some(PaneKind::Diff(dp)) => {
                        let delta = (editor_dx.abs() * 3.0).ceil() as usize;
                        let vis_cols = self.renderer.as_ref().map(|r| {
                            let cs = r.cell_size();
                            (rect.width / cs.width).floor() as usize
                        }).unwrap_or(80);
                        let max_h = dp.max_line_len().saturating_sub(vis_cols.saturating_sub(4));
                        if editor_dx > 0.0 {
                            dp.h_scroll = dp.h_scroll.saturating_sub(delta);
                        } else {
                            dp.h_scroll = (dp.h_scroll + delta).min(max_h);
                        }
                        dp.generation = dp.generation.wrapping_add(1);
                        self.pane_generations.remove(&pid);
                        self.needs_redraw = true;
                    }
                    _ => {}
                }
            }
        }
    }
}
