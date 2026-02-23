use tide_core::{Rect, Renderer};

use crate::pane::PaneKind;
use crate::search;
use crate::theme::*;
use crate::App;

impl App {
    // ── Search bar click handling ────────────────

    /// Check if the current mouse position clicks on a visible search bar.
    /// Returns true if the click was consumed.
    pub(crate) fn check_search_bar_click(&mut self) -> bool {
        let pos = self.last_cursor_pos;
        if self.renderer.is_none() {
            return false;
        }

        // Check all visual pane rects
        let pane_rects: Vec<_> = self.visual_pane_rects.clone();
        for &(id, rect) in &pane_rects {
            if self.check_search_bar_at(pos, id, rect) {
                return true;
            }
        }

        // Check panel editor
        if let (Some(active_id), Some(panel_rect)) = (self.active_editor_tab(), self.editor_panel_rect) {
            if self.check_search_bar_at(pos, active_id, panel_rect) {
                return true;
            }
        }

        // Click not on any search bar — clear search focus
        if self.search_focus.is_some() {
            self.search_focus = None;
        }

        false
    }

    fn check_search_bar_at(&mut self, pos: tide_core::Vec2, id: tide_core::PaneId, rect: Rect) -> bool {
        let has_search = match self.panes.get(&id) {
            Some(PaneKind::Terminal(p)) => p.search.as_ref().is_some_and(|s| s.visible),
            Some(PaneKind::Editor(p)) => p.search.as_ref().is_some_and(|s| s.visible),
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => false,
            None => false,
        };
        if !has_search {
            return false;
        }

        let bar_w = SEARCH_BAR_WIDTH.min(rect.width - 16.0);
        if bar_w < 80.0 { return false; }
        let bar_h = SEARCH_BAR_HEIGHT;
        let bar_x = rect.x + rect.width - bar_w - 8.0;
        let bar_y = rect.y + self.pane_area_mode.content_top() + 4.0;
        let bar_rect = Rect::new(bar_x, bar_y, bar_w, bar_h);

        if !bar_rect.contains(pos) {
            return false;
        }

        // Check close button (rightmost SEARCH_BAR_CLOSE_SIZE px)
        let close_x = bar_x + bar_w - SEARCH_BAR_CLOSE_SIZE;
        if pos.x >= close_x {
            // Close search
            match self.panes.get_mut(&id) {
                Some(PaneKind::Terminal(pane)) => { pane.search = None; }
                Some(PaneKind::Editor(pane)) => { pane.search = None; }
                Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
                None => {}
            }
            if self.search_focus == Some(id) {
                self.search_focus = None;
            }
        } else {
            // Focus the search bar
            self.search_focus = Some(id);
        }

        true
    }

    // ── Search bar helpers ──────────────────────

    /// Compute the number of visible rows for an editor pane.
    fn editor_visible_rows(&self, pane_id: tide_core::PaneId) -> usize {
        let cs = match self.renderer.as_ref() {
            Some(r) => r.cell_size(),
            None => return 30,
        };
        if let Some(&(_, rect)) = self.visual_pane_rects.iter().find(|(id, _)| *id == pane_id) {
            return ((rect.height - self.pane_area_mode.content_top() - PANE_PADDING) / cs.height).floor() as usize;
        }
        if let Some(panel_rect) = self.editor_panel_rect {
            if self.active_editor_tab() == Some(pane_id) {
                let ch = (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                return (ch / cs.height).floor() as usize;
            }
        }
        30
    }

    fn editor_visible_cols(&self, pane_id: tide_core::PaneId) -> usize {
        let cs = match self.renderer.as_ref() {
            Some(r) => r.cell_size(),
            None => return 80,
        };
        let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
        if let Some(&(_, rect)) = self.visual_pane_rects.iter().find(|(id, _)| *id == pane_id) {
            let cw = rect.width - 2.0 * PANE_PADDING - 2.0 * gutter_width;
            return (cw / cs.width).floor().max(1.0) as usize;
        }
        if let Some(panel_rect) = self.editor_panel_rect {
            if self.active_editor_tab() == Some(pane_id) {
                let cw = panel_rect.width - 2.0 * PANE_PADDING - 2.0 * gutter_width;
                return (cw / cs.width).floor().max(1.0) as usize;
            }
        }
        80
    }

    pub(crate) fn search_bar_insert(&mut self, pane_id: tide_core::PaneId, ch: char) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.input.insert_char(ch);
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.input.insert_char(ch);
                }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => return,
            None => return,
        }
        self.execute_search(pane_id);
        self.search_scroll_to_current(pane_id);
    }

    pub(crate) fn search_bar_backspace(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.input.backspace();
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.input.backspace();
                }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => return,
            None => return,
        }
        self.execute_search(pane_id);
        self.search_scroll_to_current(pane_id);
    }

    pub(crate) fn search_bar_delete(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.input.delete_char();
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.input.delete_char();
                }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => return,
            None => return,
        }
        self.execute_search(pane_id);
        self.search_scroll_to_current(pane_id);
    }

    pub(crate) fn search_bar_cursor_left(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search { s.input.move_cursor_left(); }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search { s.input.move_cursor_left(); }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
            None => {}
        }
    }

    pub(crate) fn search_bar_cursor_right(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search { s.input.move_cursor_right(); }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search { s.input.move_cursor_right(); }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
            None => {}
        }
    }

    fn execute_search(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    search::execute_search_terminal(s, &pane.backend);
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    search::execute_search_editor(s, &pane.editor.buffer.lines);
                }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
            None => {}
        }
    }

    /// Scroll the viewport to show the current match (without advancing).
    fn search_scroll_to_current(&mut self, pane_id: tide_core::PaneId) {
        let visible_rows = self.editor_visible_rows(pane_id);
        let visible_cols = self.editor_visible_cols(pane_id);
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref s) = pane.search {
                    if let Some(idx) = s.current {
                        let match_line = s.matches[idx].line;
                        let history_size = pane.backend.history_size();
                        let rows = pane.backend.current_rows() as usize;
                        let screen_start = history_size + rows;
                        if match_line < screen_start {
                            let desired_offset = screen_start.saturating_sub(match_line).saturating_sub(rows / 2);
                            let current_offset = pane.backend.display_offset();
                            let delta = desired_offset as i32 - current_offset as i32;
                            if delta != 0 {
                                pane.backend.scroll_display(delta);
                            }
                        }
                    }
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref s) = pane.search {
                    if let Some(idx) = s.current {
                        let m = &s.matches[idx];
                        let line_count = pane.editor.buffer.line_count();
                        let max_scroll = line_count.saturating_sub(visible_rows);
                        let offset = m.line.saturating_sub(visible_rows / 2).min(max_scroll);
                        pane.editor.set_scroll_offset(offset);
                        // Horizontal scroll: ensure match column is visible
                        let h_scroll = pane.editor.h_scroll_offset();
                        if m.col < h_scroll {
                            pane.editor.set_h_scroll_offset(m.col.saturating_sub(4));
                        } else if m.col + m.len > h_scroll + visible_cols {
                            pane.editor.set_h_scroll_offset((m.col + m.len).saturating_sub(visible_cols).saturating_add(4));
                        }
                    }
                }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
            None => {}
        }
    }

    pub(crate) fn search_next_match(&mut self, pane_id: tide_core::PaneId) {
        let visible_rows = self.editor_visible_rows(pane_id);
        let visible_cols = self.editor_visible_cols(pane_id);
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.next_match();
                    if let Some(idx) = s.current {
                        let match_line = s.matches[idx].line;
                        let history_size = pane.backend.history_size();
                        let rows = pane.backend.current_rows() as usize;
                        let screen_start = history_size + rows;
                        if match_line < screen_start {
                            let desired_offset = screen_start.saturating_sub(match_line).saturating_sub(rows / 2);
                            let current_offset = pane.backend.display_offset();
                            let delta = desired_offset as i32 - current_offset as i32;
                            if delta != 0 {
                                pane.backend.scroll_display(delta);
                            }
                        }
                    }
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.next_match();
                    if let Some(idx) = s.current {
                        let m = &s.matches[idx];
                        let line_count = pane.editor.buffer.line_count();
                        let max_scroll = line_count.saturating_sub(visible_rows);
                        let offset = m.line.saturating_sub(visible_rows / 2).min(max_scroll);
                        pane.editor.set_scroll_offset(offset);
                        let h_scroll = pane.editor.h_scroll_offset();
                        if m.col < h_scroll {
                            pane.editor.set_h_scroll_offset(m.col.saturating_sub(4));
                        } else if m.col + m.len > h_scroll + visible_cols {
                            pane.editor.set_h_scroll_offset((m.col + m.len).saturating_sub(visible_cols).saturating_add(4));
                        }
                    }
                }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
            None => {}
        }
    }

    pub(crate) fn search_prev_match(&mut self, pane_id: tide_core::PaneId) {
        let visible_rows = self.editor_visible_rows(pane_id);
        let visible_cols = self.editor_visible_cols(pane_id);
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.prev_match();
                    if let Some(idx) = s.current {
                        let match_line = s.matches[idx].line;
                        let history_size = pane.backend.history_size();
                        let rows = pane.backend.current_rows() as usize;
                        let screen_start = history_size + rows;
                        if match_line < screen_start {
                            let desired_offset = screen_start.saturating_sub(match_line).saturating_sub(rows / 2);
                            let current_offset = pane.backend.display_offset();
                            let delta = desired_offset as i32 - current_offset as i32;
                            if delta != 0 {
                                pane.backend.scroll_display(delta);
                            }
                        }
                    }
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.prev_match();
                    if let Some(idx) = s.current {
                        let m = &s.matches[idx];
                        let line_count = pane.editor.buffer.line_count();
                        let max_scroll = line_count.saturating_sub(visible_rows);
                        let offset = m.line.saturating_sub(visible_rows / 2).min(max_scroll);
                        pane.editor.set_scroll_offset(offset);
                        let h_scroll = pane.editor.h_scroll_offset();
                        if m.col < h_scroll {
                            pane.editor.set_h_scroll_offset(m.col.saturating_sub(4));
                        } else if m.col + m.len > h_scroll + visible_cols {
                            pane.editor.set_h_scroll_offset((m.col + m.len).saturating_sub(visible_cols).saturating_add(4));
                        }
                    }
                }
            }
            Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
            None => {}
        }
    }
}
