use tide_input::Direction;

use crate::pane::PaneKind;
use crate::{App, PaneAreaMode};

impl App {
    /// Handle MoveFocus direction navigation.
    pub(super) fn handle_move_focus(&mut self, direction: Direction) {
        self.save_as_input = None;
        let current_id = match self.focused {
            Some(id) => id,
            None => return,
        };

        // Phase A: Tab cycling when focused on editor panel
        let in_editor_panel = self.is_dock_editor(current_id)
            || self.editor_panel_placeholder == Some(current_id);
        if in_editor_panel {
            let tid = self.focused_terminal_id();
            let tabs: Vec<tide_core::PaneId> = self.active_editor_tabs().to_vec();
            let active = self.active_editor_tab();
            if let Some(active) = active {
                if let Some(idx) = tabs.iter().position(|&id| id == active) {
                    match direction {
                        Direction::Left => {
                            if idx > 0 {
                                let prev_id = tabs[idx - 1];
                                if let Some(tid) = tid {
                                    if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                                        tp.active_editor = Some(prev_id);
                                    }
                                }
                                self.pane_generations.remove(&prev_id);
                                self.focused = Some(prev_id);
                                self.router.set_focused(prev_id);
                                self.chrome_generation += 1;
                                self.scroll_to_active_panel_tab();
                                return;
                            }
                            // First tab boundary:
                            if self.dock_side == crate::LayoutSide::Left {
                                // Dock on left -> left edge is window edge, nothing further
                                return;
                            }
                            // Dock on right -> fall through to Phase C (panes are left)
                        }
                        Direction::Right => {
                            if idx + 1 < tabs.len() {
                                let next_id = tabs[idx + 1];
                                if let Some(tid) = tid {
                                    if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                                        tp.active_editor = Some(next_id);
                                    }
                                }
                                self.pane_generations.remove(&next_id);
                                self.focused = Some(next_id);
                                self.router.set_focused(next_id);
                                self.chrome_generation += 1;
                                self.scroll_to_active_panel_tab();
                                return;
                            }
                            // Last tab boundary:
                            if self.dock_side == crate::LayoutSide::Right {
                                // Dock on right -> right edge is window edge, nothing further
                                return;
                            }
                            // Dock on left -> fall through to Phase C (panes are right)
                        }
                        Direction::Up | Direction::Down => {
                            return;
                        }
                    }
                }
            }
        }

        // Phase B: Stacked mode tab cycling (no wrap — matches dock behavior)
        if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
            let pane_ids = self.layout.pane_ids();
            if let Some(pos) = pane_ids.iter().position(|&id| id == current_id) {
                match direction {
                    Direction::Left => {
                        if pos > 0 {
                            let next_id = pane_ids[pos - 1];
                            self.pane_area_mode = PaneAreaMode::Stacked(next_id);
                            self.focused = Some(next_id);
                            self.router.set_focused(next_id);
                            self.chrome_generation += 1;
                            self.panel_tab_scroll = 0.0;
                            self.panel_tab_scroll_target = 0.0;
                            self.compute_layout();
                            self.update_file_tree_cwd();
                            return;
                        }
                        // First tab boundary: exit to dock if dock is left, else stop
                        if !self.show_editor_panel || self.dock_side != crate::LayoutSide::Left {
                            return;
                        }
                        // Fall through to Phase C
                    }
                    Direction::Right => {
                        if pos + 1 < pane_ids.len() {
                            let next_id = pane_ids[pos + 1];
                            self.pane_area_mode = PaneAreaMode::Stacked(next_id);
                            self.focused = Some(next_id);
                            self.router.set_focused(next_id);
                            self.chrome_generation += 1;
                            self.panel_tab_scroll = 0.0;
                            self.panel_tab_scroll_target = 0.0;
                            self.compute_layout();
                            self.update_file_tree_cwd();
                            return;
                        }
                        // Last tab boundary: exit to dock if dock is right, else stop
                        if !self.show_editor_panel || self.dock_side != crate::LayoutSide::Right {
                            return;
                        }
                        // Fall through to Phase C
                    }
                    Direction::Up | Direction::Down => return,
                }
            }
            // current_id not in stacked panes (e.g. editor panel) → fall through to Phase C
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
                let focus_id = match self.active_editor_tab() {
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
            let old_tid = self.focused_terminal_id();
            self.focused = Some(next_id);
            self.router.set_focused(next_id);
            self.chrome_generation += 1;
            self.update_file_tree_cwd();
            // Reset panel tab scroll when switching terminal context
            if self.focused_terminal_id() != old_tid {
                self.panel_tab_scroll = 0.0;
                self.panel_tab_scroll_target = 0.0;
            }
        }
    }
}
