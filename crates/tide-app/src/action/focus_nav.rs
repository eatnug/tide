use tide_core::FileTreeSource;
use tide_editor::input::EditorAction;
use tide_input::Direction;

use crate::pane::PaneKind;
use crate::ui_state::FocusArea;
use crate::App;

/// Number of lines to scroll per Cmd+J/K press.
const KEYBOARD_SCROLL_LINES: f32 = 3.0;

impl App {
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

    /// Handle MoveFocus direction navigation between panes.
    pub(super) fn handle_move_focus(&mut self, direction: Direction) {
        self.focus_area = FocusArea::PaneArea;
        self.save_as_input = None;
        let current_id = match self.focused {
            Some(id) => id,
            None => return,
        };

        // Spatial navigation in the split tree
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
        let pane_id = match self.focused {
            Some(id) => id,
            None => return,
        };

        let cs = self.cell_size();
        let rect = self.visual_pane_rects.iter()
            .find(|(pid, _)| *pid == pane_id)
            .map(|(_, r)| *r);
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
