//! Mouse event handling — platform-agnostic.

use tide_core::{FileTreeSource, InputEvent, LayoutEngine, MouseButton, Rect, Vec2};
use tide_platform::WindowProxy;

use crate::drag_drop::PaneDragState;
use crate::pane::{PaneKind, Selection};
use crate::theme::*;
use crate::ui_state::FocusArea;
use crate::App;

impl App {
    pub(crate) fn handle_mouse_down(&mut self, button: MouseButton, window: &WindowProxy) {
        if button == MouseButton::Left {
            self.interaction.mouse_left_pressed = true;

            // Check editor scrollbar click
            if self.check_scrollbar_click(self.last_cursor_pos) {
                self.cache.needs_redraw = true;
                return;
            }

            // Start text selection if clicking on pane content
            let mods = self.modifiers;
            let content_top_offset = TAB_BAR_HEIGHT;
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
                    for (_, pane) in self.panes.iter_mut() {
                        match pane {
                            PaneKind::Terminal(p) => p.selection = None,
                            PaneKind::Editor(p) => p.selection = None,
                            PaneKind::Diff(_) | PaneKind::Browser(_) | PaneKind::Launcher(_) => {}
                        }
                    }
                    let term_cell = self.pixel_to_cell(self.last_cursor_pos, pid);
                    let editor_cell = {
                        let cs = Some(self.cell_size());
                        if let (Some(cs), Some((_, rect))) =
                            (cs, self.visual_pane_rects.iter().find(|(id, _)| *id == pid))
                        {
                            let gutter = 5.0 * cs.width;
                            let cx = rect.x + PANE_PADDING + gutter;
                            let cy = rect.y + content_top_offset;
                            let rc = ((self.last_cursor_pos.x - cx) / cs.width).floor() as isize;
                            let rr = ((self.last_cursor_pos.y - cy) / cs.height).floor() as isize;
                            if rr >= 0 && rc >= 0 {
                                Some((rr as usize, rc as usize))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    };
                    let cell_size_cached = self.cell_size();
                    match self.panes.get_mut(&pid) {
                        Some(PaneKind::Terminal(pane)) => {
                            if let Some(cell) = term_cell {
                                pane.selection = Some(Selection {
                                    anchor: cell,
                                    end: cell,
                                });
                            }
                        }
                        Some(PaneKind::Browser(_)) => {}
                        Some(PaneKind::Editor(pane)) => {
                            if pane.preview_mode {
                                // Preview mode: no gutter, use preview_scroll/h_scroll
                                let cs = Some(cell_size_cached);
                                if let (Some(cs), Some((_, rect))) = (cs, self.visual_pane_rects.iter().find(|(id, _)| *id == pid)) {
                                    let cx = rect.x + PANE_PADDING;
                                    let cy = rect.y + content_top_offset;
                                    let rc = ((self.last_cursor_pos.x - cx) / cs.width).floor() as isize;
                                    let rr = ((self.last_cursor_pos.y - cy) / cs.height).floor() as isize;
                                    if rr >= 0 && rc >= 0 {
                                        let line = pane.preview_scroll + rr as usize;
                                        let col = pane.preview_h_scroll + rc as usize;
                                        pane.selection = Some(Selection {
                                            anchor: (line, col),
                                            end: (line, col),
                                        });
                                    }
                                }
                            } else if let Some((rr, rc)) = editor_cell {
                                let line = pane.editor.scroll_offset() + rr;
                                let col = pane.editor.h_scroll_offset() + rc;
                                pane.selection = Some(Selection {
                                    anchor: (line, col),
                                    end: (line, col),
                                });
                            }
                        }
                        Some(PaneKind::Diff(_)) => {}
                        Some(PaneKind::Launcher(_)) => {}
                        None => {}
                    }
                }
            }
        }

        // Handle search bar clicks
        if button == MouseButton::Left {
            if self.check_search_bar_click() {
                self.cache.needs_redraw = true;
                return;
            }
        }

        // Handle file finder click
        if button == MouseButton::Left {
            if self.modal.context_menu.is_some() {
                if let Some(idx) = self.context_menu_item_at(self.last_cursor_pos) {
                    self.execute_context_menu_action(idx);
                }
                self.modal.context_menu = None;
                self.cache.needs_redraw = true;
                return;
            }

            if self.modal.save_as_input.is_some() {
                if !self.save_as_contains(self.last_cursor_pos) {
                    self.modal.save_as_input = None;
                }
                self.cache.needs_redraw = true;
                return;
            }

            if self.modal.file_finder.is_some() {
                if let Some(idx) = self.file_finder_item_at(self.last_cursor_pos) {
                    if let Some(ref finder) = self.modal.file_finder {
                        if let Some(&entry_idx) = finder.filtered.get(idx) {
                            let path = finder.base_dir.join(&finder.entries[entry_idx]);
                            self.close_file_finder();
                            self.open_editor_pane(path);
                            self.cache.needs_redraw = true;
                            return;
                        }
                    }
                } else if !self.file_finder_contains(self.last_cursor_pos) {
                    self.close_file_finder();
                }
                self.cache.needs_redraw = true;
                return;
            }

            if self.modal.git_switcher.is_some() {
                // Tab click: switch between Branches / Worktrees
                if let Some(mode) = self.git_switcher_tab_at(self.last_cursor_pos) {
                    if let Some(ref mut gs) = self.modal.git_switcher {
                        if gs.mode != mode {
                            gs.set_mode(mode);
                            self.cache.invalidate_chrome();
                        }
                    }
                    self.cache.needs_redraw = true;
                    return;
                }
                if let Some(btn) = self.git_switcher_button_at(self.last_cursor_pos) {
                    self.handle_git_switcher_button(btn);
                    self.cache.needs_redraw = true;
                    return;
                }
                if let Some(idx) = self.git_switcher_item_at(self.last_cursor_pos) {
                    if let Some(ref mut gs) = self.modal.git_switcher {
                        gs.selected = idx;
                        self.cache.invalidate_chrome();
                    }
                    self.cache.needs_redraw = true;
                    return;
                } else if !self.git_switcher_contains(self.last_cursor_pos) {
                    self.modal.git_switcher = None;
                    self.cache.needs_redraw = true;
                    return;
                }
            }
        }

        // Branch cleanup bar clicks
        if button == MouseButton::Left && self.modal.branch_cleanup.is_some() {
            if self.handle_branch_cleanup_click(self.last_cursor_pos) {
                return;
            }
        }

        // Notification bar clicks
        if button == MouseButton::Left {
            if self.handle_notification_bar_click(self.last_cursor_pos) {
                return;
            }
        }

        // Header clicks
        if button == MouseButton::Left {
            if self.check_header_click() {
                return;
            }
        }

        // Pane tab close
        if button == MouseButton::Left {
            if let Some(pane_id) = self.pane_tab_close_at(self.last_cursor_pos) {
                self.close_specific_pane(pane_id);
                self.cache.needs_redraw = true;
                return;
            }
        }

        // Right-click on file tree
        if button == MouseButton::Right {
            if self.ft.visible {
                if let Some(ft_rect) = self.ft.rect {
                    let pos = self.last_cursor_pos;
                    if pos.x >= ft_rect.x
                        && pos.x < ft_rect.x + ft_rect.width
                        && pos.y >= ft_rect.y + PANE_CORNER_RADIUS + FILE_TREE_HEADER_HEIGHT
                    {
                        {
                            let cell_size = self.cell_size();
                            let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
                            let content_y = ft_rect.y + PANE_CORNER_RADIUS;
                            let adjusted_y = pos.y - content_y - FILE_TREE_HEADER_HEIGHT;
                            let index =
                                ((adjusted_y + self.ft.scroll) / line_height) as usize;

                            if let Some(tree) = self.ft.tree.as_ref() {
                                let entries = tree.visible_entries();
                                if index < entries.len() {
                                    let entry = &entries[index];
                                    self.modal.context_menu = None;
                                    self.modal.file_tree_rename = None;
                                    let shell_idle = self.focused
                                        .and_then(|tid| self.panes.get(&tid))
                                        .map(|pk| if let crate::PaneKind::Terminal(tp) = pk { tp.shell_idle } else { false })
                                        .unwrap_or(false);
                                    self.modal.context_menu = Some(crate::ContextMenuState {
                                        entry_index: index,
                                        path: entry.entry.path.clone(),
                                        is_dir: entry.entry.is_dir,
                                        shell_idle,
                                        position: pos,
                                        selected: 0,
                                    });
                                    self.cache.needs_redraw = true;
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }

        // File tree clicks
        if button == MouseButton::Left {
            if self.ft.visible {
                if let Some(ft_rect) = self.ft.rect {
                    let pos = self.last_cursor_pos;
                    if pos.x >= ft_rect.x
                        && pos.x < ft_rect.x + ft_rect.width
                        && pos.y >= ft_rect.y + PANE_CORNER_RADIUS + FILE_TREE_HEADER_HEIGHT
                    {
                        self.handle_file_tree_click(pos);
                        return;
                    }
                }
            }
        }

        // Config page
        if button == MouseButton::Left && self.modal.config_page.is_some() {
            self.handle_config_page_click(self.last_cursor_pos);
            self.cache.needs_redraw = true;
            return;
        }

        // General mouse input routing
        self.handle_mouse_input_core(button, window);
        self.cache.needs_redraw = true;
    }

    fn handle_mouse_input_core(&mut self, button: MouseButton, _window: &WindowProxy) {
        if button == MouseButton::Left {
            // Workspace sidebar (always clickable, including fullscreen)
            match &self.interaction.hover_target {
                Some(crate::drag_drop::HoverTarget::WorkspaceSidebarItem(idx)) => {
                    let idx = *idx;
                    // Start pending drag
                    self.ws.drag = Some((idx, self.last_cursor_pos.y, idx));
                    return;
                }
                Some(crate::drag_drop::HoverTarget::WorkspaceSidebarNewBtn) => {
                    self.new_workspace();
                    return;
                }
                _ => {}
            }

            // Titlebar buttons (only when titlebar is visible)
            if self.top_inset > 0.0 {
                match &self.interaction.hover_target {
                    Some(crate::drag_drop::HoverTarget::TitlebarSettings) => {
                        self.toggle_config_page();
                        return;
                    }
                    Some(crate::drag_drop::HoverTarget::TitlebarTheme) => {
                        self.handle_global_action(tide_input::GlobalAction::ToggleTheme);
                        return;
                    }
                    Some(crate::drag_drop::HoverTarget::TitlebarSwap) => {
                        self.sidebar_side = match self.sidebar_side {
                            crate::LayoutSide::Left => crate::LayoutSide::Right,
                            crate::LayoutSide::Right => crate::LayoutSide::Left,
                        };
                        self.compute_layout();
                        self.cache.invalidate_chrome();
                        return;
                    }
                    Some(crate::drag_drop::HoverTarget::TitlebarFileTree) => {
                        self.handle_focus_area(FocusArea::FileTree);
                        return;
                    }
                    Some(crate::drag_drop::HoverTarget::TitlebarPaneArea) => {
                        self.handle_focus_area(FocusArea::PaneArea);
                        return;
                    }
                    _ => {}
                }
            }


            // Browser navigation bar clicks
            match &self.interaction.hover_target {
                Some(target @ crate::drag_drop::HoverTarget::BrowserBack)
                | Some(target @ crate::drag_drop::HoverTarget::BrowserForward)
                | Some(target @ crate::drag_drop::HoverTarget::BrowserRefresh)
                | Some(target @ crate::drag_drop::HoverTarget::BrowserUrlBar) => {
                    let target = target.clone();
                    // Focus the browser pane first
                    for &(id, rect) in &self.visual_pane_rects {
                        if let Some(crate::pane::PaneKind::Browser(_)) = self.panes.get(&id) {
                            if rect.contains(self.last_cursor_pos) {
                                self.focus_terminal(id);
                                break;
                            }
                        }
                    }
                    self.handle_browser_nav_click(&target);
                    return;
                }
                _ => {}
            }

            // Handle drags — sidebar handle
            if let Some(ft_rect) = self.ft.rect {
                if self.last_cursor_pos.y >= ft_rect.y
                    && self.last_cursor_pos.y < ft_rect.y + PANE_PADDING
                    && self.last_cursor_pos.x >= ft_rect.x
                    && self.last_cursor_pos.x < ft_rect.x + ft_rect.width
                {
                    self.sidebar_handle_dragging = true;
                    return;
                }
            }

            // Workspace sidebar border
            if let Some(ws_rect) = self.ws.sidebar_rect {
                let border_x = ws_rect.x + ws_rect.width + PANE_GAP;
                if (self.last_cursor_pos.x - border_x).abs() < 5.0 {
                    self.ws.border_dragging = true;
                    return;
                }
            }

            // Sidebar border
            if let Some(ft_rect) = self.ft.rect {
                let border_x = if self.sidebar_side == crate::LayoutSide::Left {
                    ft_rect.x + ft_rect.width + PANE_GAP
                } else {
                    ft_rect.x - PANE_GAP
                };
                if (self.last_cursor_pos.x - border_x).abs() < 5.0 {
                    self.ft.border_dragging = true;
                    return;
                }
            }

            // Pane tab drag init — check header_hit_zones first for accurate tab ID
            // in multi-tab groups (visual_pane_rects only contains the active tab).
            {
                let pos = self.last_cursor_pos;
                let mut tab_pane_id = None;
                for zone in &self.header_hit_zones {
                    if zone.rect.contains(pos) {
                        if let crate::header::HeaderHitAction::Tab(id) = zone.action {
                            tab_pane_id = Some(id);
                            break;
                        }
                    }
                }
                // Fall back to pane_at_tab_bar for single-pane headers (no Tab hit zones).
                let drag_pane = tab_pane_id.or_else(|| self.pane_at_tab_bar(pos));
                if let Some(pane_id) = drag_pane {
                    self.interaction.pane_drag = PaneDragState::PendingDrag {
                        source_pane: pane_id,
                        press_pos: pos,
                    };
                    self.focus_terminal(pane_id);
                    return;
                }
            }
        }

        let input = InputEvent::MouseClick {
            position: self.last_cursor_pos,
            button,
        };
        let action = self.router.process(input, &self.pane_rects);
        self.handle_action(action, Some(input));
    }

    pub(crate) fn handle_mouse_up(&mut self, button: MouseButton) {
        if button == MouseButton::Left {
            self.interaction.mouse_left_pressed = false;
        }

        // End workspace sidebar drag
        // ws_drag = (source_index, press_y, gap_index)
        if let Some((src, press_y, gap)) = self.ws.drag.take() {
            let moved = (self.last_cursor_pos.y - press_y).abs() > DRAG_THRESHOLD;
            // Convert gap to target index: gap after src position is a no-op
            let target = if gap <= src { gap } else { gap - 1 };
            if moved && target != src {
                let ws = self.ws.workspaces.remove(src);
                self.ws.workspaces.insert(target, ws);
                // Fix active_workspace index
                if self.ws.active == src {
                    self.ws.active = target;
                } else if src < self.ws.active && target >= self.ws.active {
                    self.ws.active -= 1;
                } else if src > self.ws.active && target <= self.ws.active {
                    self.ws.active += 1;
                }
            } else if !moved {
                // Click without drag — switch to workspace
                self.switch_workspace(src);
            }
            self.cache.invalidate_chrome();
            return;
        }

        // End scrollbar drag
        if self.interaction.scrollbar_dragging.is_some() {
            self.interaction.scrollbar_dragging = None;
            self.interaction.scrollbar_drag_rect = None;
            return;
        }

        // End sidebar handle drag on release
        if self.sidebar_handle_dragging {
            self.sidebar_handle_dragging = false;
            self.compute_layout();
            self.cache.invalidate_chrome();
            return;
        }

        if self.ft.border_dragging {
            self.ft.border_dragging = false;
            self.compute_layout();
            self.cache.invalidate_chrome();
            return;
        }

        if self.ws.border_dragging {
            self.ws.border_dragging = false;
            self.compute_layout();
            self.cache.invalidate_chrome();
            return;
        }

        let drag_state = std::mem::replace(&mut self.interaction.pane_drag, PaneDragState::Idle);
        match drag_state {
            PaneDragState::Dragging {
                source_pane,
                drop_target: Some(dest),
                ..
            } => {
                self.handle_drop(source_pane, dest);
                return;
            }
            PaneDragState::PendingDrag { source_pane, .. } => {
                // Switch to the clicked tab (handles both single and multi-tab groups)
                self.layout.set_active_tab(source_pane);
                self.focused = Some(source_pane);
                self.router.set_focused(source_pane);
                self.focus_area = FocusArea::PaneArea;
                self.cache.invalidate_chrome();
                self.cache.pane_generations.clear();
                self.compute_layout();
                self.cache.needs_redraw = true;
                return;
            }
            PaneDragState::Dragging { .. } => {
                return;
            }
            PaneDragState::Idle => {}
        }

        let was_dragging = self.router.is_dragging_border();
        self.layout.end_drag();
        self.router.end_drag();
        if was_dragging {
            self.compute_layout();
        }
    }

    pub(crate) fn handle_cursor_moved_logical(
        &mut self,
        pos: Vec2,
        window: &WindowProxy,
    ) {
        self.last_cursor_pos = pos;

        // Handle workspace sidebar drag
        // ws_drag stores (source_index, press_y, gap_index)
        // gap_index is the insertion gap: 0 = before first, N = after last
        if let Some((src, press_y, _)) = self.ws.drag {
            if (pos.y - press_y).abs() > DRAG_THRESHOLD {
                let gap = if let Some(geo) = self.ws_sidebar_geometry() {
                    let mut result = self.ws.workspaces.len();
                    for i in 0..self.ws.workspaces.len() {
                        let r = geo.item_rect(i);
                        if pos.y < r.y + r.height / 2.0 {
                            result = i;
                            break;
                        }
                    }
                    result
                } else {
                    src
                };
                self.ws.drag = Some((src, press_y, gap));
                self.cache.invalidate_chrome();
            }
            return;
        }

        // Handle scrollbar drag
        if let (Some(pane_id), Some(rect)) = (self.interaction.scrollbar_dragging, self.interaction.scrollbar_drag_rect) {
            self.apply_scrollbar_drag(pane_id, rect, pos.y);
            self.cache.needs_redraw = true;
            return;
        }

        // Handle workspace sidebar border resize
        if self.ws.border_dragging {
            let logical = self.logical_size();
            let ws_x = self.ws.sidebar_rect.map(|r| r.x).unwrap_or(PANE_GAP);
            let max_w = (logical.width - 200.0).max(80.0);
            let new_width = (pos.x - ws_x).max(80.0).min(max_w);
            self.ws.width = new_width;
            self.compute_layout();
            self.cache.invalidate_chrome();
            return;
        }

        // Handle border resizes
        if self.ft.border_dragging {
            let logical = self.logical_size();
            let max_w = (logical.width - 100.0).max(120.0);
            let new_width = match self.sidebar_side {
                crate::LayoutSide::Left => {
                    let ft_x = self.ft.rect.map(|r| r.x).unwrap_or(0.0);
                    (pos.x - ft_x).max(120.0).min(max_w)
                }
                crate::LayoutSide::Right => (logical.width - pos.x).max(120.0).min(max_w),
            };
            self.ft.width = new_width;
            self.compute_layout();
            self.cache.invalidate_chrome();
            return;
        }

        // Handle side drag preview (sidebar)
        if self.sidebar_handle_dragging {
            let logical = self.logical_size();
            let win_center = logical.width / 2.0;
            let target_side = if pos.x < win_center {
                crate::LayoutSide::Left
            } else {
                crate::LayoutSide::Right
            };
            self.sidebar_side = target_side;
            self.compute_layout();
            self.cache.invalidate_chrome();
            return;
        }

        // Handle pane drag
        match &self.interaction.pane_drag {
            PaneDragState::PendingDrag {
                source_pane,
                press_pos,
            } => {
                let dx = pos.x - press_pos.x;
                let dy = pos.y - press_pos.y;
                if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD {
                    let source = *source_pane;
                    let target = self.compute_drop_destination(pos, source);
                    let preview = self.compute_drop_preview_rect(source, &target);
                    self.interaction.pane_drag = PaneDragState::Dragging {
                        source_pane: source,
                        drop_target: target,
                        cached_preview_rect: preview,
                    };
                }
                self.cache.needs_redraw = true;
                return;
            }
            PaneDragState::Dragging { source_pane, drop_target: prev_target, .. } => {
                let source = *source_pane;
                let prev_target = prev_target.clone();
                let new_target = self.compute_drop_destination(pos, source);
                // Only recompute expensive simulate_drop when target actually changes
                let preview = if new_target == prev_target {
                    match &self.interaction.pane_drag {
                        PaneDragState::Dragging { cached_preview_rect, .. } => *cached_preview_rect,
                        _ => None,
                    }
                } else {
                    self.compute_drop_preview_rect(source, &new_target)
                };
                self.interaction.pane_drag = PaneDragState::Dragging {
                    source_pane: source,
                    drop_target: new_target,
                    cached_preview_rect: preview,
                };
                self.cache.needs_redraw = true;
                return;
            }
            PaneDragState::Idle => {}
        }

        if self.router.is_dragging_border() {
            let mut left = 0.0_f32;
            if self.ft.visible && self.sidebar_side == crate::LayoutSide::Left {
                left += self.ft.width;
            }
            let drag_pos = Vec2::new(pos.x - left, pos.y);
            self.layout.drag_border(drag_pos);
            self.compute_layout();
            self.cache.needs_redraw = true;
        } else {
            // Text selection drag
            if self.interaction.mouse_left_pressed {
                let cell_size = Some(self.cell_size());
                let drag_top_offset = TAB_BAR_HEIGHT;

                let pane_rects: Vec<_> = self
                    .visual_pane_rects
                    .iter()
                    .map(|(id, r)| (*id, *r))
                    .collect();
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
                    let editor_cell = if let Some(cs) = cell_size {
                        let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
                        let content_x = rect.x + PANE_PADDING + gutter_width;
                        let content_y = rect.y + drag_top_offset;
                        let rel_col = ((pos.x - content_x) / cs.width).floor() as isize;
                        let rel_row = ((pos.y - content_y) / cs.height).floor() as isize;
                        if rel_row >= 0 && rel_col >= 0 {
                            Some((rel_row as usize, rel_col as usize))
                        } else {
                            None
                        }
                    } else {
                        None
                    };

                    match self.panes.get_mut(&pid) {
                        Some(PaneKind::Terminal(pane)) => {
                            if let (Some(ref mut sel), Some(c)) = (&mut pane.selection, cell) {
                                sel.end = c;
                            }
                        }
                        Some(PaneKind::Browser(_)) => {}
                        Some(PaneKind::Editor(pane)) => {
                            if pane.preview_mode {
                                if let (Some(ref mut sel), Some(cs)) = (&mut pane.selection, cell_size) {
                                    let cx = rect.x + PANE_PADDING;
                                    let cy = rect.y + drag_top_offset;
                                    let rc = ((pos.x - cx) / cs.width).floor() as isize;
                                    let rr = ((pos.y - cy) / cs.height).floor() as isize;
                                    if rr >= 0 && rc >= 0 {
                                        sel.end = (
                                            pane.preview_scroll + rr as usize,
                                            pane.preview_h_scroll + rc as usize,
                                        );
                                    }
                                }
                            } else if let (Some(ref mut sel), Some((rel_row, rel_col))) =
                                (&mut pane.selection, editor_cell)
                            {
                                sel.end = (
                                    pane.editor.scroll_offset() + rel_row,
                                    pane.editor.h_scroll_offset() + rel_col,
                                );
                            }
                        }
                        Some(PaneKind::Diff(_)) => {}
                        Some(PaneKind::Launcher(_)) => {}
                        None => {}
                    }
                }
                self.cache.needs_redraw = true;
            }

            // Hover target
            let new_hover = self.compute_hover_target(pos);
            if new_hover != self.interaction.hover_target {
                // Bump chrome_generation only when entering/leaving chrome-rendered hover targets
                let chrome_affected =
                    self.interaction.hover_target.as_ref().map_or(false, |h| h.affects_chrome())
                    || new_hover.as_ref().map_or(false, |h| h.affects_chrome());
                // Only trigger redraw when at least one side has visual feedback.
                // Transitions between cursor-only targets (e.g. BrowserUrlBar ↔ None)
                // don't need a render frame — the OS cursor is updated separately.
                let visual_changed =
                    self.interaction.hover_target.as_ref().map_or(false, |h| h.has_visual_feedback())
                    || new_hover.as_ref().map_or(false, |h| h.has_visual_feedback());
                self.interaction.hover_target = new_hover;
                self.update_cursor_icon(window);
                if chrome_affected {
                    self.cache.invalidate_chrome();
                }
                if visual_changed {
                    self.cache.needs_redraw = true;
                }
            }

            let input = InputEvent::MouseMove { position: pos };
            let _ = self.router.process(input, &self.pane_rects);
        }
    }

    /// Check if a click position hits an editor scrollbar. If so, starts
    /// scrollbar drag and applies the initial jump. Returns true if consumed.
    fn check_scrollbar_click(&mut self, pos: Vec2) -> bool {
        let cell_height = self.cell_size().height;
        let hit_width = 16.0_f32; // wider hit area than visual scrollbar

        // Check editor panes in the split tree
        let content_top_offset = TAB_BAR_HEIGHT;
        let rects: Vec<_> = self.visual_pane_rects.iter().map(|(id, r)| (*id, *r)).collect();
        for (pid, vrect) in rects {
            if let Some(PaneKind::Editor(pane)) = self.panes.get(&pid) {
                let inner = Rect::new(
                    vrect.x + PANE_PADDING,
                    vrect.y + content_top_offset,
                    vrect.width - 2.0 * PANE_PADDING,
                    vrect.height - content_top_offset - PANE_PADDING,
                );
                let scrollbar_right = inner.x + inner.width;
                let scrollbar_left = scrollbar_right - hit_width;
                if pos.x >= scrollbar_left && pos.x <= scrollbar_right
                    && pos.y >= inner.y && pos.y <= inner.y + inner.height
                    && pane.needs_scrollbar(inner, cell_height)
                {
                    self.interaction.scrollbar_dragging = Some(pid);
                    self.interaction.scrollbar_drag_rect = Some(inner);
                    self.apply_scrollbar_drag(pid, inner, pos.y);
                    return true;
                }
            }
        }

        false
    }

    /// Apply scrollbar drag: set scroll position based on mouse Y within rect.
    fn apply_scrollbar_drag(&mut self, pane_id: tide_core::PaneId, rect: Rect, mouse_y: f32) {
        let cell_height = self.cell_size().height;
        let visible_rows = (rect.height / cell_height).floor() as usize;
        let ratio = ((mouse_y - rect.y) / rect.height).clamp(0.0, 1.0);

        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
            let (total_lines, _) = if pane.preview_mode {
                (pane.preview_line_count(), pane.preview_scroll)
            } else {
                (pane.editor.buffer.line_count(), pane.editor.scroll_offset())
            };
            let max_scroll = total_lines.saturating_sub(visible_rows);
            // Center viewport around click position
            let center = (ratio * total_lines as f32).round() as usize;
            let target = center.saturating_sub(visible_rows / 2).min(max_scroll);

            if pane.preview_mode {
                pane.preview_scroll = target;
            } else {
                pane.editor.set_scroll_offset(target);
            }
            self.cache.invalidate_pane(pane_id);
        }
    }

}
