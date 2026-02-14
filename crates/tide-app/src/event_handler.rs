use std::time::Instant;

use winit::event::{ElementState, Ime, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};

use tide_core::{FileTreeSource, InputEvent, LayoutEngine, MouseButton, Rect, Renderer, SplitDirection, TerminalBackend, Vec2};

use crate::drag_drop::{DropDestination, HoverTarget, PaneDragState};
use crate::input::{winit_key_to_tide, winit_modifiers_to_tide, winit_physical_key_to_tide};
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

    /// Compute the hover target for a given cursor position.
    /// Priority: PanelBorder → SplitBorder → PanelTabClose → PanelTab → PaneTabBar → FileTreeBorder → FileTreeEntry → None
    pub(crate) fn compute_hover_target(&self, pos: Vec2) -> Option<HoverTarget> {
        // Panel border (resize handle)
        if let Some(panel_rect) = self.editor_panel_rect {
            let border_x = panel_rect.x;
            if (pos.x - border_x).abs() < 5.0 {
                return Some(HoverTarget::PanelBorder);
            }
        }

        // File finder item hover
        if let Some(idx) = self.file_finder_item_at(pos) {
            return Some(HoverTarget::FileFinderItem(idx));
        }

        // Empty panel "New File" button
        if self.is_on_new_file_button(pos) {
            return Some(HoverTarget::EmptyPanelButton);
        }

        // Empty panel "Open File" button
        if self.is_on_open_file_button(pos) {
            return Some(HoverTarget::EmptyPanelOpenFile);
        }

        // Split pane border (resize handle between tiled panes)
        if let Some(dir) = self.split_border_at(pos) {
            return Some(HoverTarget::SplitBorder(dir));
        }

        // Panel tab close button
        if let Some(tab_id) = self.panel_tab_close_at(pos) {
            return Some(HoverTarget::PanelTabClose(tab_id));
        }

        // Panel tab
        if let Some(tab_id) = self.panel_tab_at(pos) {
            return Some(HoverTarget::PanelTab(tab_id));
        }

        // Pane tab bar close button (before general tab bar check)
        if let Some(pane_id) = self.pane_tab_close_at(pos) {
            return Some(HoverTarget::PaneTabClose(pane_id));
        }

        // Pane tab bar (split tree panes)
        if let Some(pane_id) = self.pane_at_tab_bar(pos) {
            return Some(HoverTarget::PaneTabBar(pane_id));
        }

        // File tree border (resize handle)
        if self.show_file_tree {
            let border_x = self.file_tree_width;
            if (pos.x - border_x).abs() < 5.0 {
                return Some(HoverTarget::FileTreeBorder);
            }
        }

        // File tree entry
        if self.show_file_tree && pos.x < self.file_tree_width {
            if let Some(renderer) = &self.renderer {
                let cell_size = renderer.cell_size();
                let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
                let adjusted_y = pos.y - PANE_PADDING;
                let index = ((adjusted_y + self.file_tree_scroll) / line_height) as usize;
                if let Some(tree) = &self.file_tree {
                    let entries = tree.visible_entries();
                    if index < entries.len() {
                        return Some(HoverTarget::FileTreeEntry(index));
                    }
                }
            }
        }

        None
    }

    /// Check if cursor is near an internal border between split panes.
    /// Returns the split direction (Horizontal for vertical line, Vertical for horizontal line).
    fn split_border_at(&self, pos: Vec2) -> Option<SplitDirection> {
        let t = 5.0_f32;
        let rects = &self.pane_rects;
        if rects.len() < 2 {
            return None;
        }
        for &(id_a, rect_a) in rects {
            // Check right edge → adjacent left edge = Horizontal split (side by side)
            let right_edge = rect_a.x + rect_a.width;
            if (pos.x - right_edge).abs() <= t
                && pos.y >= rect_a.y
                && pos.y <= rect_a.y + rect_a.height
            {
                for &(id_b, rect_b) in rects {
                    if id_b != id_a
                        && (rect_b.x - right_edge).abs() <= t * 2.0
                        && pos.y >= rect_b.y
                        && pos.y <= rect_b.y + rect_b.height
                    {
                        return Some(SplitDirection::Horizontal);
                    }
                }
            }
            // Check bottom edge → adjacent top edge = Vertical split (stacked)
            let bottom_edge = rect_a.y + rect_a.height;
            if (pos.y - bottom_edge).abs() <= t
                && pos.x >= rect_a.x
                && pos.x <= rect_a.x + rect_a.width
            {
                for &(id_b, rect_b) in rects {
                    if id_b != id_a
                        && (rect_b.y - bottom_edge).abs() <= t * 2.0
                        && pos.x >= rect_b.x
                        && pos.x <= rect_b.x + rect_b.width
                    {
                        return Some(SplitDirection::Vertical);
                    }
                }
            }
        }
        None
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
                Ime::Enabled => {
                    self.ime_active = true;
                }
                Ime::Disabled => {
                    self.ime_active = false;
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                    self.pending_hangul_initial = None;
                }
                Ime::Commit(text) => {
                    // If we have a pending initial from a pre-IME keystroke,
                    // try to combine it with the committed text.
                    let output = if let Some(initial) = self.pending_hangul_initial.take() {
                        combine_initial_with_text(initial, &text)
                            .unwrap_or_else(|| { let mut s = String::new(); s.push(initial); s.push_str(&text); s })
                    } else {
                        text
                    };
                    // IME composed text → route to file finder, save-as input, search bar, or focused pane
                    if self.file_finder.is_some() {
                        for ch in output.chars() {
                            if let Some(ref mut finder) = self.file_finder {
                                finder.insert_char(ch);
                                self.chrome_generation += 1;
                            }
                        }
                        self.ime_composing = false;
                        self.ime_preedit.clear();
                        self.needs_redraw = true;
                        return;
                    }
                    if self.save_as_input.is_some() {
                        for ch in output.chars() {
                            if let Some(ref mut input) = self.save_as_input {
                                input.insert_char(ch);
                            }
                        }
                        self.ime_composing = false;
                        self.ime_preedit.clear();
                        self.needs_redraw = true;
                        return;
                    }
                    if let Some(search_pane_id) = self.search_focus {
                        for ch in output.chars() {
                            self.search_bar_insert(search_pane_id, ch);
                        }
                    } else if let Some(focused_id) = self.focused {
                        match self.panes.get_mut(&focused_id) {
                            Some(PaneKind::Terminal(pane)) => {
                                if pane.backend.display_offset() > 0 {
                                    pane.backend.request_scroll_to_bottom();
                                }
                                pane.backend.write(output.as_bytes());
                                self.input_just_sent = true;
                                self.input_sent_at = Some(Instant::now());
                            }
                            Some(PaneKind::Editor(pane)) => {
                                for ch in output.chars() {
                                    pane.editor.handle_action(tide_editor::EditorActionKind::InsertChar(ch));
                                }
                            }
                            None => {}
                        }
                    }
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                }
                Ime::Preedit(text, _cursor) => {
                    self.ime_composing = !text.is_empty();
                    // If we have a pending initial, combine it with the
                    // preedit text for display (e.g. ㅇ + ㅏ → 아).
                    if !text.is_empty() {
                        if let Some(initial) = self.pending_hangul_initial {
                            if let Some(combined) = combine_initial_with_text(initial, &text) {
                                self.ime_preedit = combined;
                                return;
                            }
                        }
                    }
                    self.ime_preedit = text;
                }
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

                // Skip character keys that IME is handling.
                if matches!(event.logical_key, winit::keyboard::Key::Character(_)) {
                    if self.ime_composing {
                        return;
                    }

                    // Handle Hangul characters from KeyboardInput.
                    if let winit::keyboard::Key::Character(ref s) = event.logical_key {
                        if let Some(c) = s.chars().next() {
                            if is_hangul_char(c)
                                && !self.modifiers.control_key()
                                && !self.modifiers.super_key()
                                && !self.modifiers.alt_key()
                            {
                                if self.ime_active {
                                    // IME is active — it will deliver via Preedit/Commit.
                                    return;
                                }
                                // IME not yet active (first char after language switch).
                                // Store as pending and show as preedit; the next
                                // Ime::Preedit/Commit will combine it.
                                self.pending_hangul_initial = Some(c);
                                self.ime_preedit = s.to_string();
                                return;
                            }
                        }
                    }

                    // For non-Hangul characters: skip only when the system didn't produce
                    // committed text (event.text is None), meaning IME consumed the
                    // keystroke and will deliver it via Ime::Commit.
                    // Previous code also checked `ime_active` here, but that flag can
                    // get stuck on macOS (한/영 toggle doesn't always fire Ime::Disabled),
                    // causing numbers and ASCII to be silently dropped.
                    if event.text.is_none()
                        && !self.modifiers.control_key()
                        && !self.modifiers.super_key()
                        && !self.modifiers.alt_key()
                    {
                        return;
                    }
                }

                // When Cmd/Ctrl is held, prefer physical key so hotkeys work
                // regardless of IME language (e.g. Korean Cmd+ㅠ → physical B → Cmd+B)
                let modifiers = winit_modifiers_to_tide(self.modifiers);
                let key_opt = if modifiers.ctrl || modifiers.meta {
                    winit_physical_key_to_tide(&event.physical_key)
                        .or_else(|| winit_key_to_tide(&event.logical_key))
                } else {
                    winit_key_to_tide(&event.logical_key)
                };

                if let Some(key) = key_opt {
                    // Cmd+Q / Ctrl+Q → quit the app
                    if matches!(key, tide_core::Key::Char('q'))
                        && modifiers.meta
                        && !modifiers.ctrl
                        && !modifiers.shift
                        && !modifiers.alt
                    {
                        std::process::exit(0);
                    }

                    // File finder interception: consume all keys when active
                    if self.file_finder.is_some() {
                        match key {
                            tide_core::Key::Escape => {
                                self.close_file_finder();
                            }
                            tide_core::Key::Enter => {
                                let path = self.file_finder.as_ref().and_then(|f| f.selected_path());
                                self.close_file_finder();
                                if let Some(path) = path {
                                    self.open_editor_pane(path);
                                }
                            }
                            tide_core::Key::Up => {
                                if let Some(ref mut finder) = self.file_finder {
                                    finder.select_up();
                                    self.chrome_generation += 1;
                                }
                            }
                            tide_core::Key::Down => {
                                if let Some(ref mut finder) = self.file_finder {
                                    finder.select_down();
                                    // Auto-scroll: ensure selected item is visible
                                    let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                                    if let (Some(cs), Some(panel_rect)) = (cell_size, self.editor_panel_rect) {
                                        let line_height = cs.height * crate::theme::FILE_TREE_LINE_SPACING;
                                        let input_y = panel_rect.y + crate::theme::PANE_PADDING + 8.0;
                                        let input_h = cs.height + 12.0;
                                        let list_top = input_y + input_h + 8.0;
                                        let list_bottom = panel_rect.y + panel_rect.height - crate::theme::PANE_PADDING;
                                        let visible_rows = ((list_bottom - list_top) / line_height).floor() as usize;
                                        if finder.selected >= finder.scroll_offset + visible_rows {
                                            finder.scroll_offset = finder.selected.saturating_sub(visible_rows - 1);
                                        }
                                    }
                                    self.chrome_generation += 1;
                                }
                            }
                            tide_core::Key::Backspace => {
                                if let Some(ref mut finder) = self.file_finder {
                                    finder.backspace();
                                    self.chrome_generation += 1;
                                }
                            }
                            tide_core::Key::Delete => {
                                if let Some(ref mut finder) = self.file_finder {
                                    finder.delete_char();
                                    self.chrome_generation += 1;
                                }
                            }
                            tide_core::Key::Left => {
                                if let Some(ref mut finder) = self.file_finder {
                                    finder.move_cursor_left();
                                }
                            }
                            tide_core::Key::Right => {
                                if let Some(ref mut finder) = self.file_finder {
                                    finder.move_cursor_right();
                                }
                            }
                            tide_core::Key::Char(ch) => {
                                if !modifiers.ctrl && !modifiers.meta {
                                    if let Some(ref mut finder) = self.file_finder {
                                        finder.insert_char(ch);
                                        self.chrome_generation += 1;
                                    }
                                }
                            }
                            _ => {} // consume all other keys
                        }
                        self.needs_redraw = true;
                        return;
                    }

                    // Save-as input interception: consume all keys when active
                    if self.save_as_input.is_some() {
                        match key {
                            tide_core::Key::Escape => {
                                self.save_as_input = None;
                            }
                            tide_core::Key::Enter => {
                                let pane_id = self.save_as_input.as_ref().unwrap().pane_id;
                                let filename = self.save_as_input.as_ref().unwrap().query.clone();
                                self.save_as_input = None;
                                if !filename.is_empty() {
                                    self.complete_save_as(pane_id, &filename);
                                }
                            }
                            tide_core::Key::Backspace => {
                                if let Some(ref mut input) = self.save_as_input {
                                    input.backspace();
                                }
                            }
                            tide_core::Key::Delete => {
                                if let Some(ref mut input) = self.save_as_input {
                                    input.delete_char();
                                }
                            }
                            tide_core::Key::Left => {
                                if let Some(ref mut input) = self.save_as_input {
                                    input.move_cursor_left();
                                }
                            }
                            tide_core::Key::Right => {
                                if let Some(ref mut input) = self.save_as_input {
                                    input.move_cursor_right();
                                }
                            }
                            tide_core::Key::Char(ch) => {
                                if !modifiers.ctrl && !modifiers.meta {
                                    if let Some(ref mut input) = self.save_as_input {
                                        input.insert_char(ch);
                                    }
                                }
                            }
                            _ => {} // consume all other keys
                        }
                        self.needs_redraw = true;
                        return;
                    }

                    // Save confirm bar interception: Escape cancels, block other keys
                    if self.save_confirm.is_some() {
                        if matches!(key, tide_core::Key::Escape) {
                            self.cancel_save_confirm();
                        }
                        // Block all other keys while save confirm is active
                        self.needs_redraw = true;
                        return;
                    }

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
                            // Clear selection on all other panes
                            for (_, pane) in self.panes.iter_mut() {
                                match pane {
                                    PaneKind::Terminal(p) => p.selection = None,
                                    PaneKind::Editor(p) => p.selection = None,
                                }
                            }
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
                    // Check file tree border for resize
                    if self.show_file_tree {
                        let border_x = self.file_tree_width;
                        if (self.last_cursor_pos.x - border_x).abs() < 5.0 {
                            self.file_tree_border_dragging = true;
                            return;
                        }
                    }

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

                // Handle file tree border resize drag
                if self.file_tree_border_dragging {
                    let logical = self.logical_size();
                    let new_width = pos.x.max(120.0).min(logical.width * 0.5);
                    self.file_tree_width = new_width;
                    self.compute_layout();
                    self.clamp_panel_tab_scroll();
                    self.chrome_generation += 1;
                    return;
                }

                // Handle panel border resize drag
                if self.panel_border_dragging {
                    let logical = self.logical_size();
                    let left = if self.show_file_tree { self.file_tree_width } else { 0.0 };
                    let new_width = (logical.width - pos.x).max(150.0).min(logical.width - left - 100.0);
                    self.editor_panel_width = new_width;
                    self.compute_layout();
                    self.clamp_panel_tab_scroll();
                    return;
                }

                // Auto-unmaximize when drag threshold exceeded
                if let PaneDragState::PendingDrag { press_pos, .. } = &self.pane_drag {
                    let dx = pos.x - press_pos.x;
                    let dy = pos.y - press_pos.y;
                    if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD && self.maximized_pane.is_some() {
                        self.maximized_pane = None;
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
                    // Adjust position for file tree offset
                    let drag_pos = if self.show_file_tree {
                        Vec2::new(pos.x - self.file_tree_width, pos.y)
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

                        // Update selection only for the pane that has an active selection,
                        // and only if the cursor is within that pane's content area.
                        let pane_rects: Vec<_> = self.visual_pane_rects.iter().map(|(id, r)| (*id, *r)).collect();
                        for (pid, rect) in pane_rects {
                            let content = Rect::new(
                                rect.x + PANE_PADDING,
                                rect.y + TAB_BAR_HEIGHT,
                                rect.width - 2.0 * PANE_PADDING,
                                rect.height - TAB_BAR_HEIGHT - PANE_PADDING,
                            );
                            if !content.contains(pos) {
                                continue;
                            }
                            let cell = self.pixel_to_cell(pos, pid);
                            // Compute editor cell without borrowing panes
                            let editor_cell = if let Some(cs) = cell_size {
                                let gutter_width = 5.0 * cs.width;
                                let content_x = rect.x + PANE_PADDING + gutter_width;
                                let content_y = rect.y + TAB_BAR_HEIGHT;
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
            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    MouseScrollDelta::LineDelta(x, y) => (x * 3.0, y * 3.0),
                    MouseScrollDelta::PixelDelta(p) => (p.x as f32 / 10.0, p.y as f32 / 10.0),
                };

                // Axis isolation for editor content: only apply dominant scroll axis
                let (editor_dx, editor_dy) = if dx.abs() > dy.abs() {
                    (dx, 0.0)
                } else {
                    (0.0, dy)
                };

                // Check if scrolling over the file tree
                if self.show_file_tree && self.last_cursor_pos.x < self.file_tree_width {
                    let max_scroll = self.file_tree_max_scroll();
                    let new_target = (self.file_tree_scroll_target - dy * 10.0).clamp(0.0, max_scroll);
                    if new_target != self.file_tree_scroll_target {
                        self.file_tree_scroll_target = new_target;
                    }
                } else if self.is_over_panel_tab_bar(self.last_cursor_pos) {
                    // Horizontal scroll for panel tab bar
                    self.panel_tab_scroll_target -= dx * 20.0;
                    self.panel_tab_scroll_target -= dy * 20.0;
                    self.clamp_panel_tab_scroll();
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
                // Horizontal scroll for editor panes (trackpad two-finger swipe)
                if editor_dx != 0.0 {
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
                            if editor_dx > 0.0 {
                                pane.handle_action_with_size(EditorAction::ScrollLeft(editor_dx.abs()), visible_rows, visible_cols);
                            } else {
                                pane.handle_action_with_size(EditorAction::ScrollRight(editor_dx.abs()), visible_rows, visible_cols);
                            }
                        }
                    }
                }
            }
            // RedrawRequested is handled directly in window_event() with early return
            // to avoid the unconditional `needs_redraw = true` at the end.
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

    /// Handle notification bar button clicks (conflict bar + save confirm bar).
    /// Checks all editor panes (panel + left-side). Returns true if the click was consumed.
    pub(crate) fn handle_notification_bar_click(&mut self, pos: Vec2) -> bool {
        // Try save confirm bar first
        if let Some(ref sc) = self.save_confirm {
            let pane_id = sc.pane_id;
            if let Some(bar_rect) = self.notification_bar_rect(pane_id) {
                if pos.y >= bar_rect.y && pos.y <= bar_rect.y + bar_rect.height
                    && pos.x >= bar_rect.x && pos.x <= bar_rect.x + bar_rect.width
                {
                    let cell_size = match self.renderer.as_ref().map(|r| r.cell_size()) {
                        Some(cs) => cs,
                        None => return false,
                    };
                    let btn_pad = 8.0;

                    // Cancel (rightmost)
                    let cancel_w = 6.0 * cell_size.width + btn_pad * 2.0;
                    let cancel_x = bar_rect.x + bar_rect.width - cancel_w - 4.0;

                    // Don't Save
                    let dont_save_w = 10.0 * cell_size.width + btn_pad * 2.0;
                    let dont_save_x = cancel_x - dont_save_w - 4.0;

                    // Save
                    let save_w = 4.0 * cell_size.width + btn_pad * 2.0;
                    let save_x = dont_save_x - save_w - 4.0;

                    if pos.x >= cancel_x {
                        self.cancel_save_confirm();
                    } else if pos.x >= dont_save_x {
                        self.confirm_discard_and_close();
                    } else if pos.x >= save_x {
                        self.confirm_save_and_close();
                    }
                    self.needs_redraw = true;
                    return true;
                }
            }
        }

        // Try conflict bar
        if self.handle_conflict_bar_click_inner(pos) {
            return true;
        }

        false
    }

    /// Get the notification bar rect for a pane (either in panel or left-side).
    fn notification_bar_rect(&self, pane_id: tide_core::PaneId) -> Option<Rect> {
        // Check panel editor
        if let (Some(active_id), Some(panel_rect)) = (self.editor_panel_active, self.editor_panel_rect) {
            if active_id == pane_id {
                let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                let bar_x = panel_rect.x + PANE_PADDING;
                let bar_w = panel_rect.width - 2.0 * PANE_PADDING;
                return Some(Rect::new(bar_x, content_top, bar_w, CONFLICT_BAR_HEIGHT));
            }
        }
        // Check left-side panes
        if let Some(&(_, rect)) = self.visual_pane_rects.iter().find(|(id, _)| *id == pane_id) {
            let content_top = rect.y + TAB_BAR_HEIGHT;
            let bar_x = rect.x + PANE_PADDING;
            let bar_w = rect.width - 2.0 * PANE_PADDING;
            return Some(Rect::new(bar_x, content_top, bar_w, CONFLICT_BAR_HEIGHT));
        }
        None
    }

    /// Handle conflict bar button click for any pane. Returns true if the click was consumed.
    fn handle_conflict_bar_click_inner(&mut self, pos: Vec2) -> bool {
        // Find which pane has a conflict bar under the click
        let mut target_pane: Option<(tide_core::PaneId, Rect)> = None;

        // Check panel editor
        if let (Some(active_id), Some(panel_rect)) = (self.editor_panel_active, self.editor_panel_rect) {
            if let Some(PaneKind::Editor(pane)) = self.panes.get(&active_id) {
                if pane.needs_notification_bar() {
                    let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                    let bar_x = panel_rect.x + PANE_PADDING;
                    let bar_w = panel_rect.width - 2.0 * PANE_PADDING;
                    let bar_rect = Rect::new(bar_x, content_top, bar_w, CONFLICT_BAR_HEIGHT);
                    if pos.y >= bar_rect.y && pos.y <= bar_rect.y + CONFLICT_BAR_HEIGHT
                        && pos.x >= bar_rect.x && pos.x <= bar_rect.x + bar_rect.width
                    {
                        target_pane = Some((active_id, bar_rect));
                    }
                }
            }
        }

        // Check left-side panes
        if target_pane.is_none() {
            for &(id, rect) in &self.visual_pane_rects {
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&id) {
                    if pane.needs_notification_bar() {
                        let content_top = rect.y + TAB_BAR_HEIGHT;
                        let bar_x = rect.x + PANE_PADDING;
                        let bar_w = rect.width - 2.0 * PANE_PADDING;
                        let bar_rect = Rect::new(bar_x, content_top, bar_w, CONFLICT_BAR_HEIGHT);
                        if pos.y >= bar_rect.y && pos.y <= bar_rect.y + CONFLICT_BAR_HEIGHT
                            && pos.x >= bar_rect.x && pos.x <= bar_rect.x + bar_rect.width
                        {
                            target_pane = Some((id, bar_rect));
                            break;
                        }
                    }
                }
            }
        }

        let (pane_id, bar_rect) = match target_pane {
            Some(t) => t,
            None => return false,
        };

        let cell_size = match self.renderer.as_ref().map(|r| r.cell_size()) {
            Some(cs) => cs,
            None => return false,
        };

        let is_deleted = self.panes.get(&pane_id)
            .and_then(|pk| if let PaneKind::Editor(ep) = pk { Some(ep.file_deleted) } else { None })
            .unwrap_or(false);

        let btn_pad = 8.0;

        // Overwrite button (rightmost)
        let overwrite_w = 9.0 * cell_size.width + btn_pad * 2.0;
        let overwrite_x = bar_rect.x + bar_rect.width - overwrite_w - 4.0;

        // Compare button (only for non-deleted)
        let compare_w = 7.0 * cell_size.width + btn_pad * 2.0;
        let compare_x = overwrite_x - compare_w - 4.0;

        if pos.x >= overwrite_x {
            // Overwrite
            if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
                if let Err(e) = pane.editor.buffer.save() {
                    log::error!("Conflict overwrite failed: {}", e);
                }
                pane.disk_changed = false;
                pane.file_deleted = false;
                pane.diff_mode = false;
                pane.disk_content = None;
            }
        } else if !is_deleted && pos.x >= compare_x {
            // Compare — enter diff mode
            if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
                // Load disk content for diff
                if let Some(path) = pane.editor.file_path().map(|p| p.to_path_buf()) {
                    match std::fs::read_to_string(&path) {
                        Ok(content) => {
                            let lines: Vec<String> = content.lines().map(String::from).collect();
                            pane.disk_content = Some(lines);
                            pane.diff_mode = true;
                        }
                        Err(e) => {
                            log::error!("Failed to read disk content for diff: {}", e);
                        }
                    }
                }
            }
        }

        self.chrome_generation += 1;
        self.pane_generations.remove(&pane_id);
        self.needs_redraw = true;
        true
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
                    // Tree to tree root: use restructure for proper tree rebuilding
                    let pane_area_size = self.pane_area_rect
                        .map(|r| tide_core::Size::new(r.width, r.height))
                        .unwrap_or_else(|| {
                            let ls = self.logical_size();
                            tide_core::Size::new(ls.width, ls.height)
                        });
                    if self.layout.restructure_move_to_root(source, zone, pane_area_size) {
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
                    // Tree to tree: use restructure for proper tree rebuilding
                    let pane_area_size = self.pane_area_rect
                        .map(|r| tide_core::Size::new(r.width, r.height))
                        .unwrap_or_else(|| {
                            let ls = self.logical_size();
                            tide_core::Size::new(ls.width, ls.height)
                        });
                    if self.layout.restructure_move_pane(source, target_id, zone, pane_area_size) {
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

        let bar_w = SEARCH_BAR_WIDTH.min(rect.width - 16.0);
        if bar_w < 80.0 { return false; }
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

/// Check if a character is in a Hangul Unicode range.
/// Covers Jamo, Compatibility Jamo, Syllables, and Extended Jamo blocks.
fn is_hangul_char(c: char) -> bool {
    matches!(c,
        '\u{1100}'..='\u{11FF}'   // Hangul Jamo
        | '\u{3130}'..='\u{318F}' // Hangul Compatibility Jamo
        | '\u{A960}'..='\u{A97F}' // Hangul Jamo Extended-A
        | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
        | '\u{D7B0}'..='\u{D7FF}' // Hangul Jamo Extended-B
    )
}

/// Map a Compatibility Jamo consonant to its Choseong (initial) index (0..18).
fn choseong_index(c: char) -> Option<u32> {
    // Compatibility Jamo consonants → Choseong index
    // ㄱ ㄲ ㄴ ㄷ ㄸ ㄹ ㅁ ㅂ ㅃ ㅅ ㅆ ㅇ ㅈ ㅉ ㅊ ㅋ ㅌ ㅍ ㅎ
    match c {
        'ㄱ' => Some(0),  'ㄲ' => Some(1),  'ㄴ' => Some(2),
        'ㄷ' => Some(3),  'ㄸ' => Some(4),  'ㄹ' => Some(5),
        'ㅁ' => Some(6),  'ㅂ' => Some(7),  'ㅃ' => Some(8),
        'ㅅ' => Some(9),  'ㅆ' => Some(10), 'ㅇ' => Some(11),
        'ㅈ' => Some(12), 'ㅉ' => Some(13), 'ㅊ' => Some(14),
        'ㅋ' => Some(15), 'ㅌ' => Some(16), 'ㅍ' => Some(17),
        'ㅎ' => Some(18),
        _ => None,
    }
}

/// Map a Compatibility Jamo vowel to its Jungseong (medial) index (0..20).
fn jungseong_index(c: char) -> Option<u32> {
    let code = c as u32;
    // ㅏ (0x314F) .. ㅣ (0x3163) → indices 0..20
    if (0x314F..=0x3163).contains(&code) {
        Some(code - 0x314F)
    } else {
        None
    }
}

/// Try to combine a Choseong (initial consonant) with a string that starts
/// with a Jungseong (vowel).  Returns the combined string if successful.
/// e.g. 'ㅇ' + "ㅏ" → "아",  'ㅇ' + "ㅏㄴ" → "안" (won't happen here).
fn combine_initial_with_text(initial: char, text: &str) -> Option<String> {
    let cho = choseong_index(initial)?;
    let first = text.chars().next()?;
    let jung = jungseong_index(first)?;
    let syllable = char::from_u32(0xAC00 + (cho * 21 + jung) * 28)?;
    let mut result = String::new();
    result.push(syllable);
    result.extend(text.chars().skip(1));
    Some(result)
}
