use winit::event::{ElementState, MouseButton as WinitMouseButton};

use tide_core::{InputEvent, LayoutEngine, MouseButton, Rect, Renderer, Vec2};

use crate::drag_drop::PaneDragState;
use crate::input::winit_modifiers_to_tide;
use crate::pane::{PaneKind, Selection};
use crate::theme::*;
use crate::{App, PaneAreaMode};

impl App {
    pub(crate) fn handle_mouse_input(&mut self, state: ElementState, button: WinitMouseButton) {
        if state == ElementState::Pressed && button == WinitMouseButton::Left {
            self.mouse_left_pressed = true;

            // Start text selection if clicking on pane content
            // (but not on tab bars, borders, etc.)
            let mods = winit_modifiers_to_tide(self.modifiers);
            let content_top_offset = self.pane_area_mode.content_top();
            if !mods.ctrl && !mods.meta {
                if let Some((pane_id, _)) = self.visual_pane_rects.iter().find(|(_, r)| {
                    let content = Rect::new(
                        r.x + PANE_PADDING,
                        r.y + content_top_offset,
                        r.width - 2.0 * PANE_PADDING,
                        r.height - content_top_offset - PANE_PADDING,
                    );
                    content.contains(self.last_cursor_pos)
                }) {
                    let pid = *pane_id;
                    // Clear selection on all other panes
                    for (_, pane) in self.panes.iter_mut() {
                        match pane {
                            PaneKind::Terminal(p) => p.selection = None,
                            PaneKind::Editor(p) => p.selection = None,
                            PaneKind::Diff(_) => {}
                        }
                    }
                    // Pre-compute positions before mutable borrow
                    let term_cell = self.pixel_to_cell(self.last_cursor_pos, pid);
                    let editor_cell = {
                        let cs = self.renderer.as_ref().map(|r| r.cell_size());
                        if let (Some(cs), Some((_, rect))) = (cs, self.visual_pane_rects.iter().find(|(id, _)| *id == pid)) {
                            let gutter = 5.0 * cs.width;
                            let cx = rect.x + PANE_PADDING + gutter;
                            let cy = rect.y + content_top_offset;
                            let rc = ((self.last_cursor_pos.x - cx) / cs.width).floor() as isize;
                            let rr = ((self.last_cursor_pos.y - cy) / cs.height).floor() as isize;
                            if rr >= 0 && rc >= 0 { Some((rr as usize, rc as usize)) } else { None }
                        } else { None }
                    };
                    match self.panes.get_mut(&pid) {
                        Some(PaneKind::Terminal(pane)) => {
                            if let Some(cell) = term_cell {
                                pane.selection = Some(Selection { anchor: cell, end: cell });
                            }
                        }
                        Some(PaneKind::Editor(pane)) => {
                            if let Some((rr, rc)) = editor_cell {
                                let line = pane.editor.scroll_offset() + rr;
                                let col = pane.editor.h_scroll_offset() + rc;
                                pane.selection = Some(Selection { anchor: (line, col), end: (line, col) });
                            }
                        }
                        Some(PaneKind::Diff(_)) => {
                        }
                        None => {}
                    }
                }
            }
        }

        if state == ElementState::Released && button == WinitMouseButton::Left {
            self.mouse_left_pressed = false;
        }

        if state != ElementState::Pressed {
            // End handle drag on release → apply preview
            if self.sidebar_handle_dragging || self.dock_handle_dragging {
                if let Some(target_side) = self.handle_drag_preview.take() {
                    if self.sidebar_handle_dragging {
                        self.sidebar_side = target_side;
                    } else {
                        self.dock_side = target_side;
                    }
                }
                self.sidebar_handle_dragging = false;
                self.dock_handle_dragging = false;
                self.compute_layout();
                self.chrome_generation += 1;
                return;
            }

            // End file tree border resize on release
            if self.file_tree_border_dragging {
                self.file_tree_border_dragging = false;
                self.compute_layout();
                self.clamp_panel_tab_scroll();
                return;
            }

            // End panel border resize on release
            if self.panel_border_dragging {
                self.panel_border_dragging = false;
                self.compute_layout();
                self.clamp_panel_tab_scroll();
                return;
            }

            // Handle pane drag drop on mouse release
            let drag_state = std::mem::replace(&mut self.pane_drag, PaneDragState::Idle);
            match drag_state {
                PaneDragState::Dragging { source_pane, from_panel, drop_target: Some(dest), .. } => {
                    self.handle_drop(source_pane, from_panel, dest);
                    return;
                }
                PaneDragState::PendingDrag { source_pane, .. } => {
                    // Click (no drag): just focus the pane
                    if self.focused != Some(source_pane) {
                        self.focused = Some(source_pane);
                        self.router.set_focused(source_pane);
                        self.chrome_generation += 1;
                        self.update_file_tree_cwd();
                    }
                    return;
                }
                PaneDragState::Dragging { .. } => {
                    // Drop with no valid target: cancel
                    return;
                }
                PaneDragState::Idle => {}
            }

            let was_dragging = self.router.is_dragging_border();
            // End drag on mouse release
            self.layout.end_drag();
            self.router.end_drag();
            // Apply final PTY resize now that drag is over
            if was_dragging {
                self.compute_layout();
            }
            return;
        }

        let btn = match button {
            WinitMouseButton::Left => MouseButton::Left,
            WinitMouseButton::Right => MouseButton::Right,
            WinitMouseButton::Middle => MouseButton::Middle,
            _ => return,
        };

        if btn == MouseButton::Left {
            // Check top-edge drag handles (top strip of sidebar/dock panels)
            if self.last_cursor_pos.y < PANE_PADDING {
                if let Some(ft_rect) = self.file_tree_rect {
                    if self.last_cursor_pos.x >= ft_rect.x && self.last_cursor_pos.x < ft_rect.x + ft_rect.width {
                        self.sidebar_handle_dragging = true;
                        return;
                    }
                }
                if let Some(panel_rect) = self.editor_panel_rect {
                    if self.last_cursor_pos.x >= panel_rect.x && self.last_cursor_pos.x < panel_rect.x + panel_rect.width {
                        self.dock_handle_dragging = true;
                        return;
                    }
                }
            }

            // Check sidebar border (side resize handle)
            if let Some(ft_rect) = self.file_tree_rect {
                let border_x = if self.sidebar_side == crate::LayoutSide::Left {
                    ft_rect.x + ft_rect.width + PANE_GAP
                } else {
                    ft_rect.x - PANE_GAP
                };
                if (self.last_cursor_pos.x - border_x).abs() < 5.0 {
                    self.file_tree_border_dragging = true;
                    return;
                }
            }

            // Check dock border (side resize handle)
            if let Some(panel_rect) = self.editor_panel_rect {
                let border_x = if self.dock_side == crate::LayoutSide::Right {
                    panel_rect.x
                } else {
                    panel_rect.x + panel_rect.width + PANE_GAP
                };
                if (self.last_cursor_pos.x - border_x).abs() < 5.0 {
                    self.panel_border_dragging = true;
                    return;
                }
            }

            // Check panel tabs first for drag initiation
            if let Some(tab_id) = self.panel_tab_at(self.last_cursor_pos) {
                self.pane_drag = PaneDragState::PendingDrag {
                    source_pane: tab_id,
                    press_pos: self.last_cursor_pos,
                    from_panel: true,
                };
                // Activate and focus
                self.editor_panel_active = Some(tab_id);
                self.pane_generations.remove(&tab_id); // force grid rebuild
                if self.focused != Some(tab_id) {
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.chrome_generation += 1;
                }
                self.scroll_to_active_panel_tab();
                return;
            }

            // Check stacked tab close button
            if let Some(tab_id) = self.stacked_tab_close_at(self.last_cursor_pos) {
                self.close_specific_pane(tab_id);
                self.needs_redraw = true;
                return;
            }

            // Check stacked tabs for click-to-switch + drag initiation
            if let Some(tab_id) = self.stacked_tab_at(self.last_cursor_pos) {
                self.pane_drag = PaneDragState::PendingDrag {
                    source_pane: tab_id,
                    press_pos: self.last_cursor_pos,
                    from_panel: false,
                };
                // Activate and focus the clicked stacked tab
                self.pane_area_mode = PaneAreaMode::Stacked(tab_id);
                if self.focused != Some(tab_id) {
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.update_file_tree_cwd();
                }
                self.chrome_generation += 1;
                self.compute_layout();
                return;
            }

            // Check tree tab bars for drag initiation
            if let Some(pane_id) = self.pane_at_tab_bar(self.last_cursor_pos) {
                self.pane_drag = PaneDragState::PendingDrag {
                    source_pane: pane_id,
                    press_pos: self.last_cursor_pos,
                    from_panel: false,
                };
                // Focus the pane immediately
                if self.focused != Some(pane_id) {
                    self.focused = Some(pane_id);
                    self.router.set_focused(pane_id);
                    self.chrome_generation += 1;
                    self.update_file_tree_cwd();
                }
                return;
            }
        }

        let input = InputEvent::MouseClick {
            position: self.last_cursor_pos,
            button: btn,
        };

        let action = self.router.process(input, &self.pane_rects);
        self.handle_action(action, Some(input));
    }

    pub(crate) fn handle_cursor_moved(&mut self, position: winit::dpi::PhysicalPosition<f64>) {
        let pos = Vec2::new(
            position.x as f32 / self.scale_factor,
            position.y as f32 / self.scale_factor,
        );
        self.last_cursor_pos = pos;

        // Handle file tree border resize drag
        // Sidebar is always outermost, so no offset needed.
        if self.file_tree_border_dragging {
            let logical = self.logical_size();
            let dock_w = if self.show_editor_panel { self.editor_panel_width } else { 0.0 };
            let max_w = (logical.width - dock_w - 100.0).max(120.0);
            let new_width = match self.sidebar_side {
                crate::LayoutSide::Left => pos.x.max(120.0).min(max_w),
                crate::LayoutSide::Right => (logical.width - pos.x).max(120.0).min(max_w),
            };
            self.file_tree_width = new_width;
            self.compute_layout();
            self.clamp_panel_tab_scroll();
            self.chrome_generation += 1;
            return;
        }

        // Handle panel border resize drag
        // Dock is always inner when on the same side as sidebar.
        if self.panel_border_dragging {
            let logical = self.logical_size();
            let sidebar_w = if self.show_file_tree { self.file_tree_width } else { 0.0 };
            let same_side_sidebar = self.show_file_tree && self.sidebar_side == self.dock_side;
            let max_w = (logical.width - sidebar_w - 100.0).max(150.0);
            let new_width = match self.dock_side {
                crate::LayoutSide::Right => {
                    // When same side, sidebar is outer → subtract its width
                    let offset = if same_side_sidebar { sidebar_w } else { 0.0 };
                    (logical.width - offset - pos.x).max(150.0).min(max_w)
                }
                crate::LayoutSide::Left => {
                    // If sidebar is on the left (same or different side), it's always outer → subtract
                    let offset = if self.show_file_tree && self.sidebar_side == crate::LayoutSide::Left {
                        sidebar_w
                    } else {
                        0.0
                    };
                    (pos.x - offset).max(150.0).min(max_w)
                }
            };
            self.editor_panel_width = new_width;
            self.editor_panel_width_manual = true;
            self.compute_layout();
            self.clamp_panel_tab_scroll();
            return;
        }

        // Handle sidebar/dock handle drag → compute preview (apply on release)
        // Sidebar is always outermost, so only the target side matters.
        if self.sidebar_handle_dragging || self.dock_handle_dragging {
            let logical = self.logical_size();
            let win_center = logical.width / 2.0;
            let target_side = if pos.x < win_center { crate::LayoutSide::Left } else { crate::LayoutSide::Right };

            let new_preview = Some(target_side);
            if self.handle_drag_preview != new_preview {
                self.handle_drag_preview = new_preview;
                self.chrome_generation += 1;
            }
            return;
        }

        // Auto-unstack when drag threshold exceeded in Stacked mode
        if let PaneDragState::PendingDrag { press_pos, .. } = &self.pane_drag {
            let dx = pos.x - press_pos.x;
            let dy = pos.y - press_pos.y;
            if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD && matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
                self.pane_area_mode = PaneAreaMode::Split;
                self.compute_layout();
            }
        }

        // Handle pane drag state machine
        match &self.pane_drag {
            PaneDragState::PendingDrag { source_pane, press_pos, from_panel } => {
                let dx = pos.x - press_pos.x;
                let dy = pos.y - press_pos.y;
                if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD {
                    let source = *source_pane;
                    let fp = *from_panel;
                    let target = self.compute_drop_destination(pos, source, fp);
                    self.pane_drag = PaneDragState::Dragging {
                        source_pane: source,
                        from_panel: fp,
                        drop_target: target,
                    };
                }
                return;
            }
            PaneDragState::Dragging { source_pane, from_panel, .. } => {
                let source = *source_pane;
                let fp = *from_panel;
                let target = self.compute_drop_destination(pos, source, fp);
                self.pane_drag = PaneDragState::Dragging {
                    source_pane: source,
                    from_panel: fp,
                    drop_target: target,
                };
                return;
            }
            PaneDragState::Idle => {}
        }

        if self.router.is_dragging_border() {
            // Adjust position for left-side reserved space
            let mut left = 0.0_f32;
            if self.show_file_tree && self.sidebar_side == crate::LayoutSide::Left {
                left += self.file_tree_width;
            }
            if self.show_editor_panel && self.dock_side == crate::LayoutSide::Left {
                left += self.editor_panel_width;
            }
            let drag_pos = Vec2::new(pos.x - left, pos.y);
            self.layout.drag_border(drag_pos);
            self.compute_layout();
        } else {
            // Update text selection while mouse is pressed
            if self.mouse_left_pressed {
                // Pre-compute cell positions before mutably borrowing panes
                let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                let drag_top_offset = self.pane_area_mode.content_top();

                // Update selection only for the pane that has an active selection,
                // and only if the cursor is within that pane's content area.
                let pane_rects: Vec<_> = self.visual_pane_rects.iter().map(|(id, r)| (*id, *r)).collect();
                for (pid, rect) in pane_rects {
                    let content = Rect::new(
                        rect.x + PANE_PADDING,
                        rect.y + drag_top_offset,
                        rect.width - 2.0 * PANE_PADDING,
                        rect.height - drag_top_offset - PANE_PADDING,
                    );
                    if !content.contains(pos) {
                        continue;
                    }
                    let cell = self.pixel_to_cell(pos, pid);
                    // Compute editor cell without borrowing panes
                    let editor_cell = if let Some(cs) = cell_size {
                        let gutter_width = 5.0 * cs.width;
                        let content_x = rect.x + PANE_PADDING + gutter_width;
                        let content_y = rect.y + drag_top_offset;
                        let rel_col = ((pos.x - content_x) / cs.width).floor() as isize;
                        let rel_row = ((pos.y - content_y) / cs.height).floor() as isize;
                        if rel_row >= 0 && rel_col >= 0 { Some((rel_row as usize, rel_col as usize)) } else { None }
                    } else { None };

                    match self.panes.get_mut(&pid) {
                        Some(PaneKind::Terminal(pane)) => {
                            if let (Some(ref mut sel), Some(c)) = (&mut pane.selection, cell) {
                                sel.end = c;
                            }
                        }
                        Some(PaneKind::Editor(pane)) => {
                            if let (Some(ref mut sel), Some((rel_row, rel_col))) = (&mut pane.selection, editor_cell) {
                                sel.end = (pane.editor.scroll_offset() + rel_row, pane.editor.h_scroll_offset() + rel_col);
                            }
                        }
                        Some(PaneKind::Diff(_)) => {}
                        None => {}
                    }
                }
                // Update selection for panel editor
                if let (Some(active_id), Some(panel_rect), Some(cs)) = (self.editor_panel_active, self.editor_panel_rect, cell_size) {
                    let gutter_width = 5.0 * cs.width;
                    let content_x = panel_rect.x + PANE_PADDING + gutter_width;
                    let content_y = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                    let rel_col = ((pos.x - content_x) / cs.width).floor() as isize;
                    let rel_row = ((pos.y - content_y) / cs.height).floor() as isize;
                    if rel_row >= 0 && rel_col >= 0 {
                        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&active_id) {
                            if let Some(ref mut sel) = pane.selection {
                                sel.end = (pane.editor.scroll_offset() + rel_row as usize, pane.editor.h_scroll_offset() + rel_col as usize);
                            }
                        }
                    }
                }
            }

            // Update hover target for interactive feedback
            let new_hover = self.compute_hover_target(pos);
            if new_hover != self.hover_target {
                self.hover_target = new_hover;
                self.update_cursor_icon();
            }

            let input = InputEvent::MouseMove { position: pos };
            let _ = self.router.process(input, &self.pane_rects);
        }
    }
}
