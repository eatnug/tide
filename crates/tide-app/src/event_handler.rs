use std::time::Instant;

use winit::event::{ElementState, Ime, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};

use tide_core::{InputEvent, LayoutEngine, MouseButton, Rect, Renderer, SplitDirection, TerminalBackend, Vec2};

use crate::drag_drop::{DropDestination, PaneDragState};
use crate::input::{winit_key_to_tide, winit_modifiers_to_tide};
use crate::pane::{PaneKind, Selection};
use crate::search;
use crate::theme::*;
use crate::App;

impl App {
    /// Convert a pixel position to a terminal cell (row, col) within a pane's content area.
    /// Returns None if the position is outside any terminal pane's content area.
    pub(crate) fn pixel_to_cell(&self, pos: Vec2, pane_id: tide_core::PaneId) -> Option<(usize, usize)> {
        let (_, visual_rect) = self.visual_pane_rects.iter().find(|(id, _)| *id == pane_id)?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let inner_x = visual_rect.x + PANE_PADDING;
        let inner_y = visual_rect.y + TAB_BAR_HEIGHT;
        let col = ((pos.x - inner_x) / cell_size.width).floor() as isize;
        let row = ((pos.y - inner_y) / cell_size.height).floor() as isize;
        if row >= 0 && col >= 0 {
            Some((row as usize, col as usize))
        } else {
            None
        }
    }

}

impl App {
    pub(crate) fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                std::process::exit(0);
            }
            WindowEvent::Resized(new_size) => {
                self.window_size = new_size;
                self.reconfigure_surface();
                self.compute_layout();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor as f32;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Commit(text) => {
                    // IME composed text (Korean, CJK, etc.) → write directly to terminal
                    if let Some(focused_id) = self.focused {
                        if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&focused_id) {
                            // Scroll back to bottom on input (applied atomically during next grid sync)
                            if pane.backend.display_offset() > 0 {
                                pane.backend.request_scroll_to_bottom();
                            }
                            pane.backend.write(text.as_bytes());
                            self.input_just_sent = true;
                            self.input_sent_at = Some(Instant::now());
                        }
                    }
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                }
                Ime::Preedit(text, _) => {
                    self.ime_composing = !text.is_empty();
                    self.ime_preedit = text;
                }
                _ => {}
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                // Cancel pane drag on Escape
                if !matches!(self.pane_drag, PaneDragState::Idle) {
                    if event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) {
                        self.pane_drag = PaneDragState::Idle;
                        return;
                    }
                }

                // During IME composition, only handle non-character keys
                if self.ime_composing
                    && matches!(event.logical_key, winit::keyboard::Key::Character(_))
                {
                    return;
                }

                if let Some(key) = winit_key_to_tide(&event.logical_key) {
                    let modifiers = winit_modifiers_to_tide(self.modifiers);

                    // Search bar key interception: when search is focused, consume keys
                    if let Some(search_pane_id) = self.search_focus {
                        // Cmd+F while search is focused → close search (toggle)
                        if matches!(key, tide_core::Key::Char('f') | tide_core::Key::Char('F'))
                            && (modifiers.meta || modifiers.ctrl)
                            && !(modifiers.meta && modifiers.ctrl)
                        {
                            match self.panes.get_mut(&search_pane_id) {
                                Some(PaneKind::Terminal(pane)) => { pane.search = None; }
                                Some(PaneKind::Editor(pane)) => { pane.search = None; }
                                None => {}
                            }
                            self.search_focus = None;
                            return;
                        }

                        match key {
                            tide_core::Key::Escape => {
                                // Close search
                                match self.panes.get_mut(&search_pane_id) {
                                    Some(PaneKind::Terminal(pane)) => { pane.search = None; }
                                    Some(PaneKind::Editor(pane)) => { pane.search = None; }
                                    None => {}
                                }
                                self.search_focus = None;
                            }
                            tide_core::Key::Enter => {
                                if modifiers.shift {
                                    self.search_prev_match(search_pane_id);
                                } else {
                                    self.search_next_match(search_pane_id);
                                }
                            }
                            tide_core::Key::Backspace => {
                                self.search_bar_backspace(search_pane_id);
                            }
                            tide_core::Key::Delete => {
                                self.search_bar_delete(search_pane_id);
                            }
                            tide_core::Key::Left => {
                                self.search_bar_cursor_left(search_pane_id);
                            }
                            tide_core::Key::Right => {
                                self.search_bar_cursor_right(search_pane_id);
                            }
                            tide_core::Key::Char(ch) => {
                                if !modifiers.ctrl && !modifiers.meta {
                                    self.search_bar_insert(search_pane_id, ch);
                                }
                            }
                            _ => {} // consume all other keys
                        }
                        return;
                    }

                    let input = InputEvent::KeyPress { key, modifiers };

                    let action = self.router.process(input, &self.pane_rects);
                    self.handle_action(action, Some(input));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state == ElementState::Pressed && button == WinitMouseButton::Left {
                    self.mouse_left_pressed = true;

                    // Start text selection if clicking on pane content
                    // (but not on tab bars, borders, etc.)
                    let mods = winit_modifiers_to_tide(self.modifiers);
                    if !mods.ctrl && !mods.meta {
                        if let Some((pane_id, _)) = self.visual_pane_rects.iter().find(|(_, r)| {
                            let content = Rect::new(
                                r.x + PANE_PADDING,
                                r.y + TAB_BAR_HEIGHT,
                                r.width - 2.0 * PANE_PADDING,
                                r.height - TAB_BAR_HEIGHT - PANE_PADDING,
                            );
                            content.contains(self.last_cursor_pos)
                        }) {
                            let pid = *pane_id;
                            // Pre-compute positions before mutable borrow
                            let term_cell = self.pixel_to_cell(self.last_cursor_pos, pid);
                            let editor_cell = {
                                let cs = self.renderer.as_ref().map(|r| r.cell_size());
                                if let (Some(cs), Some((_, rect))) = (cs, self.visual_pane_rects.iter().find(|(id, _)| *id == pid)) {
                                    let gutter = 5.0 * cs.width;
                                    let cx = rect.x + PANE_PADDING + gutter;
                                    let cy = rect.y + TAB_BAR_HEIGHT;
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
                                None => {}
                            }
                        }
                    }
                }

                if state == ElementState::Released && button == WinitMouseButton::Left {
                    self.mouse_left_pressed = false;
                }

                if state != ElementState::Pressed {
                    // End panel border resize on release
                    if self.panel_border_dragging {
                        self.panel_border_dragging = false;
                        self.compute_layout();
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
                    // Check panel border for resize
                    if let Some(panel_rect) = self.editor_panel_rect {
                        let border_x = panel_rect.x;
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
            WindowEvent::CursorMoved { position, .. } => {
                let pos = Vec2::new(
                    position.x as f32 / self.scale_factor,
                    position.y as f32 / self.scale_factor,
                );
                self.last_cursor_pos = pos;

                // Handle panel border resize drag
                if self.panel_border_dragging {
                    let logical = self.logical_size();
                    let left = if self.show_file_tree { FILE_TREE_WIDTH } else { 0.0 };
                    let new_width = (logical.width - pos.x).max(150.0).min(logical.width - left - 100.0);
                    self.editor_panel_width = new_width;
                    self.compute_layout();
                    return;
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
                    // Adjust position for file tree offset
                    let drag_pos = if self.show_file_tree {
                        Vec2::new(pos.x - FILE_TREE_WIDTH, pos.y)
                    } else {
                        pos
                    };
                    self.layout.drag_border(drag_pos);
                    self.compute_layout();
                } else {
                    // Update text selection while mouse is pressed
                    if self.mouse_left_pressed {
                        // Pre-compute cell positions before mutably borrowing panes
                        let cell_size = self.renderer.as_ref().map(|r| r.cell_size());

                        // Update selection for panes (terminal + editor)
                        let pane_ids: Vec<_> = self.visual_pane_rects.iter().map(|(id, _)| *id).collect();
                        for pid in pane_ids {
                            let cell = self.pixel_to_cell(pos, pid);
                            // Compute editor cell without borrowing panes
                            let editor_cell = if let Some(cs) = cell_size {
                                if let Some((_, visual_rect)) = self.visual_pane_rects.iter().find(|(id, _)| *id == pid) {
                                    let gutter_width = 5.0 * cs.width;
                                    let content_x = visual_rect.x + PANE_PADDING + gutter_width;
                                    let content_y = visual_rect.y + TAB_BAR_HEIGHT;
                                    let rel_col = ((pos.x - content_x) / cs.width).floor() as isize;
                                    let rel_row = ((pos.y - content_y) / cs.height).floor() as isize;
                                    if rel_row >= 0 && rel_col >= 0 { Some((rel_row as usize, rel_col as usize)) } else { None }
                                } else { None }
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

                    let input = InputEvent::MouseMove { position: pos };
                    let _ = self.router.process(input, &self.pane_rects);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 3.0, y * 3.0),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / 10.0, p.y as f32 / 10.0),
                };

                // Check if scrolling over the file tree
                if self.show_file_tree && self.last_cursor_pos.x < FILE_TREE_WIDTH {
                    let new_scroll = (self.file_tree_scroll - dy * 10.0).max(0.0);
                    if new_scroll != self.file_tree_scroll {
                        self.file_tree_scroll = new_scroll;
                        self.chrome_generation += 1;
                    }
                } else if self.is_over_panel_tab_bar(self.last_cursor_pos) {
                    // Horizontal scroll for panel tab bar
                    self.panel_tab_scroll -= dy * 20.0;
                    self.clamp_panel_tab_scroll();
                    self.chrome_generation += 1;
                } else if let Some(panel_rect) = self.editor_panel_rect {
                    if panel_rect.contains(self.last_cursor_pos) {
                        // Route scroll to active panel editor
                        if let Some(active_id) = self.editor_panel_active {
                            let (visible_rows, visible_cols) = self.renderer.as_ref().map(|r| {
                                let cs = r.cell_size();
                                let content_height = (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                                let gutter_width = 5.0 * cs.width;
                                let content_width = (panel_rect.width - 2.0 * PANE_PADDING - gutter_width).max(1.0);
                                let rows = (content_height / cs.height).floor() as usize;
                                let cols = (content_width / cs.width).floor() as usize;
                                (rows, cols)
                            }).unwrap_or((30, 80));
                            if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&active_id) {
                                use tide_editor::input::EditorAction;
                                if dy > 0.0 {
                                    pane.handle_action_with_size(EditorAction::ScrollUp(dy.abs()), visible_rows, visible_cols);
                                } else if dy < 0.0 {
                                    pane.handle_action_with_size(EditorAction::ScrollDown(dy.abs()), visible_rows, visible_cols);
                                }
                                if dx > 0.0 {
                                    pane.handle_action_with_size(EditorAction::ScrollLeft(dx.abs()), visible_rows, visible_cols);
                                } else if dx < 0.0 {
                                    pane.handle_action_with_size(EditorAction::ScrollRight(dx.abs()), visible_rows, visible_cols);
                                }
                            }
                        }
                    } else {
                        let input = InputEvent::MouseScroll {
                            delta: dy,
                            position: self.last_cursor_pos,
                        };
                        let action = self.router.process(input, &self.pane_rects);
                        self.handle_action(action, Some(input));
                    }
                } else {
                    let input = InputEvent::MouseScroll {
                        delta: dy,
                        position: self.last_cursor_pos,
                    };
                    let action = self.router.process(input, &self.pane_rects);
                    self.handle_action(action, Some(input));
                }
                // Horizontal scroll for editor panes (trackpad two-finger swipe)
                if dx != 0.0 {
                    let editor_pane_id = self.visual_pane_rects.iter()
                        .find(|(_, r)| r.contains(self.last_cursor_pos))
                        .map(|(id, r)| (*id, *r));
                    if let Some((pid, rect)) = editor_pane_id {
                        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pid) {
                            use tide_editor::input::EditorAction;
                            let visible_cols = self.renderer.as_ref().map(|r| {
                                let cs = r.cell_size();
                                let gutter = 5.0 * cs.width;
                                ((rect.width - 2.0 * PANE_PADDING - gutter) / cs.width).floor() as usize
                            }).unwrap_or(80);
                            let visible_rows = self.renderer.as_ref().map(|r| {
                                let cs = r.cell_size();
                                ((rect.height - TAB_BAR_HEIGHT - PANE_PADDING) / cs.height).floor() as usize
                            }).unwrap_or(30);
                            if dx > 0.0 {
                                pane.handle_action_with_size(EditorAction::ScrollLeft(dx.abs()), visible_rows, visible_cols);
                            } else {
                                pane.handle_action_with_size(EditorAction::ScrollRight(dx.abs()), visible_rows, visible_cols);
                            }
                        }
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                self.update();
                self.render();
                self.needs_redraw = false;
                self.last_frame = Instant::now();
            }
            _ => {}
        }
    }

    /// Handle editor panel content area click: focus and move cursor.
    pub(crate) fn handle_editor_panel_click(&mut self, pos: Vec2) {
        // Content area click → focus and move cursor
        if let Some(active_id) = self.editor_panel_active {
            if self.focused != Some(active_id) {
                self.focused = Some(active_id);
                self.router.set_focused(active_id);
                self.chrome_generation += 1;
            }

            // Move cursor to click position + start selection
            if let (Some(panel_rect), Some(cell_size)) = (self.editor_panel_rect, self.renderer.as_ref().map(|r| r.cell_size())) {
                let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                let content_x = panel_rect.x + PANE_PADDING + 5.0 * cell_size.width; // gutter
                let rel_col = ((pos.x - content_x) / cell_size.width).floor() as isize;
                let rel_row = ((pos.y - content_top) / cell_size.height).floor() as isize;

                if rel_row >= 0 && rel_col >= 0 {
                    if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&active_id) {
                        use tide_editor::input::EditorAction;
                        let line = pane.editor.scroll_offset() + rel_row as usize;
                        let col = pane.editor.h_scroll_offset() + rel_col as usize;
                        let content_height = (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                        let visible_rows = (content_height / cell_size.height).floor() as usize;
                        pane.handle_action(EditorAction::SetCursor { line, col }, visible_rows);
                        // Start selection
                        pane.selection = Some(Selection {
                            anchor: (line, col),
                            end: (line, col),
                        });
                    }
                }
            }
        }
    }

    /// Handle a completed drop operation.
    fn handle_drop(&mut self, source: tide_core::PaneId, from_panel: bool, dest: DropDestination) {
        match dest {
            DropDestination::TreeRoot(zone) => {
                if from_panel {
                    // Moving from panel to tree root: remove from panel, wrap tree root
                    self.editor_panel_tabs.retain(|&id| id != source);
                    if self.editor_panel_active == Some(source) {
                        self.editor_panel_active = self.editor_panel_tabs.last().copied();
                    }

                    if self.layout.insert_at_root(source, zone) {
                        self.focused = Some(source);
                        self.router.set_focused(source);
                        self.chrome_generation += 1;
                        self.compute_layout();
                    }
                } else {
                    // Tree to tree root: use move_pane_to_root
                    if self.layout.move_pane_to_root(source, zone) {
                        self.chrome_generation += 1;
                        self.compute_layout();
                    }
                }
            }
            DropDestination::TreePane(target_id, zone) => {
                if from_panel {
                    // Moving from panel to tree: remove from panel, insert into tree
                    self.editor_panel_tabs.retain(|&id| id != source);
                    if self.editor_panel_active == Some(source) {
                        self.editor_panel_active = self.editor_panel_tabs.last().copied();
                    }

                    let (direction, insert_first) = match zone {
                        tide_core::DropZone::Top => (SplitDirection::Vertical, true),
                        tide_core::DropZone::Bottom => (SplitDirection::Vertical, false),
                        tide_core::DropZone::Left => (SplitDirection::Horizontal, true),
                        tide_core::DropZone::Right => (SplitDirection::Horizontal, false),
                        tide_core::DropZone::Center => {
                            // Swap: panel source takes target's place in tree, target goes to panel
                            // For simplicity, insert next to target on the right
                            (SplitDirection::Horizontal, false)
                        }
                    };

                    if zone == tide_core::DropZone::Center {
                        // For center drop from panel: just insert next to target
                        self.layout.insert_pane(target_id, source, direction, insert_first);
                    } else {
                        self.layout.insert_pane(target_id, source, direction, insert_first);
                    }

                    self.focused = Some(source);
                    self.router.set_focused(source);
                    self.chrome_generation += 1;
                    self.compute_layout();
                } else {
                    // Tree to tree: use existing move_pane
                    if self.layout.move_pane(source, target_id, zone) {
                        self.chrome_generation += 1;
                        self.compute_layout();
                    }
                }
            }
            DropDestination::EditorPanel => {
                // Moving from tree to panel
                // Only editor panes; terminal panes are rejected at compute_drop_destination
                self.layout.remove(source);
                if !self.editor_panel_tabs.contains(&source) {
                    self.editor_panel_tabs.push(source);
                }
                self.editor_panel_active = Some(source);
                self.focused = Some(source);
                self.router.set_focused(source);
                self.chrome_generation += 1;
                self.compute_layout();
                self.scroll_to_active_panel_tab();
            }
        }
    }

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
        if let (Some(active_id), Some(panel_rect)) = (self.editor_panel_active, self.editor_panel_rect) {
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
            None => false,
        };
        if !has_search {
            return false;
        }

        let bar_w = SEARCH_BAR_WIDTH;
        let bar_h = SEARCH_BAR_HEIGHT;
        let bar_x = rect.x + rect.width - bar_w - 8.0;
        let bar_y = rect.y + TAB_BAR_HEIGHT + 4.0;
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
            return ((rect.height - TAB_BAR_HEIGHT - PANE_PADDING) / cs.height).floor() as usize;
        }
        if let Some(panel_rect) = self.editor_panel_rect {
            if self.editor_panel_active == Some(pane_id) {
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
        let gutter_width = 5.0 * cs.width;
        if let Some(&(_, rect)) = self.visual_pane_rects.iter().find(|(id, _)| *id == pane_id) {
            let cw = rect.width - 2.0 * PANE_PADDING - gutter_width;
            return (cw / cs.width).floor().max(1.0) as usize;
        }
        if let Some(panel_rect) = self.editor_panel_rect {
            if self.editor_panel_active == Some(pane_id) {
                let cw = panel_rect.width - 2.0 * PANE_PADDING - gutter_width;
                return (cw / cs.width).floor().max(1.0) as usize;
            }
        }
        80
    }

    fn search_bar_insert(&mut self, pane_id: tide_core::PaneId, ch: char) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.insert_char(ch);
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.insert_char(ch);
                }
            }
            None => return,
        }
        self.execute_search(pane_id);
        self.search_scroll_to_current(pane_id);
    }

    fn search_bar_backspace(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.backspace();
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.backspace();
                }
            }
            None => return,
        }
        self.execute_search(pane_id);
        self.search_scroll_to_current(pane_id);
    }

    fn search_bar_delete(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.delete_char();
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search {
                    s.delete_char();
                }
            }
            None => return,
        }
        self.execute_search(pane_id);
        self.search_scroll_to_current(pane_id);
    }

    fn search_bar_cursor_left(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search { s.move_cursor_left(); }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search { s.move_cursor_left(); }
            }
            None => {}
        }
    }

    fn search_bar_cursor_right(&mut self, pane_id: tide_core::PaneId) {
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                if let Some(ref mut s) = pane.search { s.move_cursor_right(); }
            }
            Some(PaneKind::Editor(pane)) => {
                if let Some(ref mut s) = pane.search { s.move_cursor_right(); }
            }
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
            None => {}
        }
    }

    fn search_next_match(&mut self, pane_id: tide_core::PaneId) {
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
            None => {}
        }
    }

    fn search_prev_match(&mut self, pane_id: tide_core::PaneId) {
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
            None => {}
        }
    }
}
