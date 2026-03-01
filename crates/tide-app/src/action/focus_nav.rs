use tide_core::FileTreeSource;
use tide_editor::input::EditorAction;
use tide_input::Direction;

use crate::pane::PaneKind;
use crate::ui_state::FocusArea;
use crate::{App, PaneAreaMode};

/// Number of lines to scroll per Cmd+J/K press.
const KEYBOARD_SCROLL_LINES: f32 = 3.0;

impl App {
    /// Navigate dock tabs: H = prev, L = next (wrapping). J/K = scroll content.
    pub(super) fn navigate_dock_tabs(&mut self, direction: Direction) {
        // J/K: scroll the active dock editor content
        if matches!(direction, Direction::Up | Direction::Down) {
            if let Some(editor_id) = self.active_editor_tab() {
                self.scroll_pane_content(editor_id, direction);
            }
            return;
        }

        let tid = self.focused_terminal_id();
        let tabs = self.active_editor_tabs().to_vec();
        let active = self.active_editor_tab();
        if let (Some(tid), Some(active)) = (tid, active) {
            if let Some(idx) = tabs.iter().position(|&id| id == active) {
                if tabs.is_empty() {
                    return;
                }
                let new_idx = match direction {
                    Direction::Left => {
                        if idx > 0 { idx - 1 } else { tabs.len() - 1 }
                    }
                    Direction::Right => {
                        if idx + 1 < tabs.len() { idx + 1 } else { 0 }
                    }
                    _ => unreachable!(),
                };
                let new_tab = tabs[new_idx];
                if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                    tp.active_editor = Some(new_tab);
                }
                self.pane_generations.remove(&active);
                self.pane_generations.remove(&new_tab);
                self.chrome_generation += 1;
                self.scroll_to_active_panel_tab();
            }
        }
    }

    /// Navigate file tree cursor: J(Down) = next, K(Up) = prev. H/L ignored.
    pub(super) fn navigate_file_tree(&mut self, direction: Direction) {
        let entry_count = self.file_tree.as_ref()
            .map(|t| t.visible_entries().len())
            .unwrap_or(0);
        if entry_count == 0 {
            return;
        }
        match direction {
            Direction::Down => {
                if self.file_tree_cursor + 1 < entry_count {
                    self.file_tree_cursor += 1;
                    self.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            Direction::Up => {
                if self.file_tree_cursor > 0 {
                    self.file_tree_cursor -= 1;
                    self.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            _ => {} // H/L ignored in file tree
        }
        self.needs_redraw = true;
    }

    /// Handle MoveFocus direction navigation between terminal panes.
    pub(super) fn handle_move_focus(&mut self, direction: Direction) {
        self.focus_area = FocusArea::PaneArea;
        self.save_as_input = None;
        let current_id = match self.focused {
            Some(id) => id,
            None => return,
        };

        // Stacked mode: H = prev, L = next (wrapping). J/K = scroll content.
        if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
            // J/K: scroll the focused pane content
            if matches!(direction, Direction::Up | Direction::Down) {
                self.scroll_pane_content(current_id, direction);
                return;
            }

            let pane_ids = self.layout.pane_ids();
            if pane_ids.len() < 2 {
                return;
            }
            if let Some(pos) = pane_ids.iter().position(|&id| id == current_id) {
                let next_pos = match direction {
                    Direction::Left => {
                        if pos > 0 { pos - 1 } else { pane_ids.len() - 1 }
                    }
                    Direction::Right => {
                        if pos + 1 < pane_ids.len() { pos + 1 } else { 0 }
                    }
                    _ => unreachable!(),
                };
                let next_id = pane_ids[next_pos];
                self.pane_area_mode = PaneAreaMode::Stacked(next_id);
                self.focus_terminal(next_id);
                self.compute_layout();
                return;
            }
        }

        // Split mode: spatial navigation
        if self.editor_panel_maximized || self.pane_area_maximized {
            self.editor_panel_maximized = false;
            self.pane_area_maximized = false;
            self.compute_layout();
        }

        let all_rects = self.pane_rects.clone();
        if all_rects.len() < 2 {
            return;
        }

        let current_rect = match all_rects.iter().find(|(id, _)| *id == current_id) {
            Some((_, r)) => *r,
            None => return,
        };
        let cx = current_rect.x + current_rect.width / 2.0;
        let cy = current_rect.y + current_rect.height / 2.0;

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

            let score = if overlaps { dist } else { dist + 100000.0 };
            if best.is_none_or(|(_, d)| score < d) {
                best = Some((id, score));
            }
        }

        if let Some((next_id, _)) = best {
            self.focus_terminal(next_id);
        }
    }

    /// Scroll the focused pane by half a page (Cmd+U / Cmd+D).
    pub(super) fn scroll_half_page(&mut self, direction: Direction) {
        let pane_id = match self.focus_area {
            FocusArea::EditorDock => self.active_editor_tab(),
            _ => self.focused,
        };
        let pane_id = match pane_id {
            Some(id) => id,
            None => return,
        };

        let cs = self.cell_size();
        let rect = self.visual_pane_rects.iter()
            .find(|(pid, _)| *pid == pane_id)
            .map(|(_, r)| *r)
            .or(self.editor_panel_rect);
        let visible_rows = rect
            .map(|r| (r.height / cs.height).floor() as usize)
            .unwrap_or(30);
        let half = (visible_rows / 2).max(1) as f32;

        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(tp)) => {
                let lines = match direction {
                    Direction::Up => half as i32,
                    Direction::Down => -(half as i32),
                    _ => return,
                };
                tp.scroll_display(lines);
            }
            Some(PaneKind::Editor(ep)) => {
                let action = match direction {
                    Direction::Up => EditorAction::ScrollUp(half),
                    Direction::Down => EditorAction::ScrollDown(half),
                    _ => return,
                };
                ep.handle_action(action, visible_rows);
            }
            _ => return,
        }
        self.pane_generations.remove(&pane_id);
        self.needs_redraw = true;
    }

    /// Scroll a pane's content by a fixed number of lines (Cmd+J / Cmd+K).
    fn scroll_pane_content(&mut self, pane_id: tide_core::PaneId, direction: Direction) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(tp)) => {
                let lines = match direction {
                    Direction::Up => KEYBOARD_SCROLL_LINES as i32,
                    Direction::Down => -(KEYBOARD_SCROLL_LINES as i32),
                    _ => return,
                };
                tp.scroll_display(lines);
            }
            Some(PaneKind::Editor(ep)) => {
                let action = match direction {
                    Direction::Up => EditorAction::ScrollUp(KEYBOARD_SCROLL_LINES),
                    Direction::Down => EditorAction::ScrollDown(KEYBOARD_SCROLL_LINES),
                    _ => return,
                };
                let visible_rows = 30; // approximate; scroll amount is fixed lines
                ep.handle_action(action, visible_rows);
            }
            _ => return,
        }
        self.pane_generations.remove(&pane_id);
        self.needs_redraw = true;
    }
}
