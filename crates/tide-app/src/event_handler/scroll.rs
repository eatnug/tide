use tide_core::InputEvent;

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
        } else {
            // Route scroll to the pane under the cursor via the input router
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
                let cs = self.cell_size();
                let scroll_top_off = TAB_BAR_HEIGHT;
                match self.panes.get_mut(&pid) {
                    Some(PaneKind::Editor(pane)) if pane.preview_mode => {
                        let delta = (editor_dx.abs() * 3.0).ceil() as usize;
                        let max_w = pane.preview_max_line_width();
                        let preview_visible_cols = (rect.width / cs.width).floor() as usize;
                        let max_h_scroll = max_w.saturating_sub(preview_visible_cols);
                        if editor_dx > 0.0 {
                            pane.preview_h_scroll = pane.preview_h_scroll.saturating_sub(delta);
                        } else {
                            pane.preview_h_scroll = (pane.preview_h_scroll + delta).min(max_h_scroll);
                        }
                        self.pane_generations.remove(&pid);
                        self.needs_redraw = true;
                    }
                    Some(PaneKind::Editor(pane)) => {
                        use tide_editor::input::EditorAction;
                        let visible_cols = {
                            let gutter = 5.0 * cs.width;
                            ((rect.width - 2.0 * PANE_PADDING - 2.0 * gutter) / cs.width).floor() as usize
                        };
                        let visible_rows = {
                            ((rect.height - scroll_top_off - PANE_PADDING) / cs.height).floor() as usize
                        };
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
                        let vis_cols = {
                            (rect.width / cs.width).floor() as usize
                        };
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
