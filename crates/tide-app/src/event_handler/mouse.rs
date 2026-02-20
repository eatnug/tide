//! Mouse event handling — platform-agnostic.

use tide_core::{FileTreeSource, InputEvent, LayoutEngine, MouseButton, Rect, Renderer, Vec2};
use tide_platform::PlatformWindow;

use crate::drag_drop::PaneDragState;
use crate::pane::{PaneKind, Selection};
use crate::theme::*;
use crate::ui_state::FocusArea;
use crate::{App, PaneAreaMode};

impl App {
    pub(crate) fn handle_mouse_down(&mut self, button: MouseButton, window: &dyn PlatformWindow) {
        if button == MouseButton::Left {
            self.mouse_left_pressed = true;

            // Start text selection if clicking on pane content
            let mods = self.modifiers;
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
                    for (_, pane) in self.panes.iter_mut() {
                        match pane {
                            PaneKind::Terminal(p) => p.selection = None,
                            PaneKind::Editor(p) => p.selection = None,
                            PaneKind::Diff(_) => {}
                        }
                    }
                    let term_cell = self.pixel_to_cell(self.last_cursor_pos, pid);
                    let editor_cell = {
                        let cs = self.renderer.as_ref().map(|r| r.cell_size());
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
                    match self.panes.get_mut(&pid) {
                        Some(PaneKind::Terminal(pane)) => {
                            if let Some(cell) = term_cell {
                                pane.selection = Some(Selection {
                                    anchor: cell,
                                    end: cell,
                                });
                            }
                        }
                        Some(PaneKind::Editor(pane)) => {
                            if let Some((rr, rc)) = editor_cell {
                                let line = pane.editor.scroll_offset() + rr;
                                let col = pane.editor.h_scroll_offset() + rc;
                                pane.selection = Some(Selection {
                                    anchor: (line, col),
                                    end: (line, col),
                                });
                            }
                        }
                        Some(PaneKind::Diff(_)) => {}
                        None => {}
                    }
                }
            }
        }

        // Handle search bar clicks
        if button == MouseButton::Left {
            if self.check_search_bar_click() {
                self.needs_redraw = true;
                return;
            }
        }

        // Handle empty panel buttons
        if button == MouseButton::Left {
            if self.is_on_new_file_button(self.last_cursor_pos) {
                self.new_editor_pane();
                self.needs_redraw = true;
                return;
            }
            if self.is_on_open_file_button(self.last_cursor_pos) {
                self.open_file_finder();
                self.needs_redraw = true;
                return;
            }
        }

        // Handle file finder click
        if button == MouseButton::Left {
            if self.context_menu.is_some() {
                if let Some(idx) = self.context_menu_item_at(self.last_cursor_pos) {
                    self.execute_context_menu_action(idx);
                }
                self.context_menu = None;
                self.needs_redraw = true;
                return;
            }

            if self.save_as_input.is_some() {
                if !self.save_as_contains(self.last_cursor_pos) {
                    self.save_as_input = None;
                }
                self.needs_redraw = true;
                return;
            }

            if self.file_finder.is_some() {
                if let Some(idx) = self.file_finder_item_at(self.last_cursor_pos) {
                    if let Some(ref finder) = self.file_finder {
                        if let Some(&entry_idx) = finder.filtered.get(idx) {
                            let path = finder.base_dir.join(&finder.entries[entry_idx]);
                            self.close_file_finder();
                            self.open_editor_pane(path);
                            self.needs_redraw = true;
                            return;
                        }
                    }
                } else if !self.file_finder_contains(self.last_cursor_pos) {
                    self.close_file_finder();
                }
                self.needs_redraw = true;
                return;
            }

            if self.git_switcher.is_some() {
                if let Some(btn) = self.git_switcher_button_at(self.last_cursor_pos) {
                    self.handle_git_switcher_button(btn);
                    self.needs_redraw = true;
                    return;
                }
                if let Some(idx) = self.git_switcher_item_at(self.last_cursor_pos) {
                    if let Some(ref mut gs) = self.git_switcher {
                        gs.selected = idx;
                        self.chrome_generation += 1;
                    }
                    self.needs_redraw = true;
                    return;
                } else if !self.git_switcher_contains(self.last_cursor_pos) {
                    self.git_switcher = None;
                    self.needs_redraw = true;
                    return;
                }
            }

            if self.file_switcher.is_some() {
                if let Some(idx) = self.file_switcher_item_at(self.last_cursor_pos) {
                    let selected_pane_id = self.file_switcher.as_ref().and_then(|fs| {
                        let entry_idx = *fs.filtered.get(idx)?;
                        Some(fs.entries.get(entry_idx)?.pane_id)
                    });
                    self.file_switcher = None;
                    if let Some(pane_id) = selected_pane_id {
                        if let Some(tid) = self.terminal_owning(pane_id) {
                            if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                                tp.active_editor = Some(pane_id);
                            }
                        }
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&pane_id);
                    }
                    self.needs_redraw = true;
                    return;
                } else if !self.file_switcher_contains(self.last_cursor_pos) {
                    self.file_switcher = None;
                    self.needs_redraw = true;
                    return;
                }
            }
        }

        // Editor panel clicks
        if button == MouseButton::Left {
            if let Some(ref panel_rect) = self.editor_panel_rect {
                let near_border = (self.last_cursor_pos.x - panel_rect.x).abs() < 5.0;
                let in_handle_strip = self.last_cursor_pos.y >= panel_rect.y
                    && self.last_cursor_pos.y < panel_rect.y + PANE_PADDING;
                if panel_rect.contains(self.last_cursor_pos) && !near_border && !in_handle_strip {
                    if let Some(tab_id) = self.panel_tab_close_at(self.last_cursor_pos) {
                        self.close_editor_panel_tab(tab_id);
                        self.needs_redraw = true;
                        return;
                    }
                    if self.panel_tab_at(self.last_cursor_pos).is_some() {
                        self.cancel_save_confirm();
                    } else if self.handle_notification_bar_click(self.last_cursor_pos) {
                        return;
                    } else {
                        use crate::drag_drop::HoverTarget;
                        match self.hover_target {
                            Some(HoverTarget::DockPreviewToggle) => {
                                if let Some(active_id) = self.active_editor_tab() {
                                    if let Some(PaneKind::Editor(pane)) =
                                        self.panes.get_mut(&active_id)
                                    {
                                        pane.toggle_preview();
                                    }
                                    self.chrome_generation += 1;
                                    self.pane_generations.remove(&active_id);
                                    self.needs_redraw = true;
                                    return;
                                }
                            }
                            Some(HoverTarget::DockMaximize) => {
                                self.pane_area_maximized = false;
                                self.editor_panel_maximized = !self.editor_panel_maximized;
                                self.chrome_generation += 1;
                                self.compute_layout();
                                self.needs_redraw = true;
                                return;
                            }
                            _ => {}
                        }
                        self.mouse_left_pressed = true;
                        self.handle_editor_panel_click(self.last_cursor_pos);
                        self.needs_redraw = true;
                        return;
                    }
                }
            }
        }

        // Notification bar clicks
        if button == MouseButton::Left {
            let in_panel = self
                .editor_panel_rect
                .is_some_and(|pr| pr.contains(self.last_cursor_pos));
            if !in_panel && self.handle_notification_bar_click(self.last_cursor_pos) {
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
                self.needs_redraw = true;
                return;
            }
        }

        // Right-click on file tree
        if button == MouseButton::Right {
            if self.show_file_tree {
                if let Some(ft_rect) = self.file_tree_rect {
                    let pos = self.last_cursor_pos;
                    if pos.x >= ft_rect.x
                        && pos.x < ft_rect.x + ft_rect.width
                        && pos.y >= ft_rect.y + PANE_CORNER_RADIUS + FILE_TREE_HEADER_HEIGHT
                    {
                        if let Some(renderer) = self.renderer.as_ref() {
                            let cell_size = renderer.cell_size();
                            let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
                            let content_y = ft_rect.y + PANE_CORNER_RADIUS;
                            let adjusted_y = pos.y - content_y - FILE_TREE_HEADER_HEIGHT;
                            let index =
                                ((adjusted_y + self.file_tree_scroll) / line_height) as usize;

                            if let Some(tree) = self.file_tree.as_ref() {
                                let entries = tree.visible_entries();
                                if index < entries.len() {
                                    let entry = &entries[index];
                                    self.context_menu = None;
                                    self.file_tree_rename = None;
                                    self.context_menu = Some(crate::ContextMenuState {
                                        entry_index: index,
                                        path: entry.entry.path.clone(),
                                        is_dir: entry.entry.is_dir,
                                        position: pos,
                                        selected: 0,
                                    });
                                    self.needs_redraw = true;
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
            if self.show_file_tree {
                if let Some(ft_rect) = self.file_tree_rect {
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
        if button == MouseButton::Left && self.config_page.is_some() {
            self.handle_config_page_click(self.last_cursor_pos);
            self.needs_redraw = true;
            return;
        }

        // General mouse input routing (same as before, minus winit conversion)
        self.handle_mouse_input_core(button, window);
        self.needs_redraw = true;
    }

    fn handle_mouse_input_core(&mut self, button: MouseButton, _window: &dyn PlatformWindow) {
        if button == MouseButton::Left {
            // Titlebar buttons
            if self.top_inset > 0.0 {
                match &self.hover_target {
                    Some(crate::drag_drop::HoverTarget::TitlebarSwap) => {
                        self.dock_side = match self.dock_side {
                            crate::LayoutSide::Left => crate::LayoutSide::Right,
                            crate::LayoutSide::Right => crate::LayoutSide::Left,
                        };
                        self.compute_layout();
                        self.chrome_generation += 1;
                        self.needs_redraw = true;
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
                    Some(crate::drag_drop::HoverTarget::TitlebarDock) => {
                        self.handle_focus_area(FocusArea::EditorDock);
                        return;
                    }
                    _ => {}
                }
            }

            // Badge buttons
            if matches!(
                self.hover_target,
                Some(crate::drag_drop::HoverTarget::DockPreviewToggle)
            ) {
                if let Some(active_id) = self.active_editor_tab() {
                    if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&active_id) {
                        pane.toggle_preview();
                    }
                    self.chrome_generation += 1;
                    self.pane_generations.remove(&active_id);
                    self.needs_redraw = true;
                    return;
                }
            }

            if matches!(
                self.hover_target,
                Some(crate::drag_drop::HoverTarget::DockMaximize)
            ) {
                self.pane_area_maximized = false;
                self.editor_panel_maximized = !self.editor_panel_maximized;
                self.chrome_generation += 1;
                self.compute_layout();
                self.needs_redraw = true;
                return;
            }

            if matches!(
                self.hover_target,
                Some(crate::drag_drop::HoverTarget::PaneAreaMaximize)
            ) {
                self.editor_panel_maximized = false;
                self.pane_area_maximized = !self.pane_area_maximized;
                self.chrome_generation += 1;
                self.compute_layout();
                self.needs_redraw = true;
                return;
            }

            // Handle drags
            if let Some(ft_rect) = self.file_tree_rect {
                if self.last_cursor_pos.y >= ft_rect.y
                    && self.last_cursor_pos.y < ft_rect.y + PANE_PADDING
                    && self.last_cursor_pos.x >= ft_rect.x
                    && self.last_cursor_pos.x < ft_rect.x + ft_rect.width
                {
                    self.sidebar_handle_dragging = true;
                    return;
                }
            }
            if let Some(panel_rect) = self.editor_panel_rect {
                if self.last_cursor_pos.y >= panel_rect.y
                    && self.last_cursor_pos.y < panel_rect.y + PANE_PADDING
                    && self.last_cursor_pos.x >= panel_rect.x
                    && self.last_cursor_pos.x < panel_rect.x + panel_rect.width
                {
                    self.dock_handle_dragging = true;
                    return;
                }
            }

            // Sidebar border
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

            // Dock border
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

            // Panel tabs
            if let Some(tab_id) = self.panel_tab_at(self.last_cursor_pos) {
                if let Some(tid) = self.terminal_owning(tab_id) {
                    if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                        tp.active_editor = Some(tab_id);
                    }
                }
                self.pane_generations.remove(&tab_id);
                self.focus_area = FocusArea::EditorDock;
                self.chrome_generation += 1;
                self.scroll_to_active_panel_tab();
                return;
            }

            // Stacked mode toggle
            if matches!(
                self.hover_target,
                Some(crate::drag_drop::HoverTarget::PaneModeToggle)
            ) {
                self.pane_area_mode = PaneAreaMode::Split;
                self.pane_area_maximized = false;
                self.compute_layout();
                self.chrome_generation += 1;
                self.needs_redraw = true;
                return;
            }

            // Stacked tab close
            if let Some(tab_id) = self.stacked_tab_close_at(self.last_cursor_pos) {
                self.close_specific_pane(tab_id);
                self.needs_redraw = true;
                return;
            }

            // Stacked tab click + drag init
            if let Some(tab_id) = self.stacked_tab_at(self.last_cursor_pos) {
                self.pane_drag = PaneDragState::PendingDrag {
                    source_pane: tab_id,
                    press_pos: self.last_cursor_pos,
                };
                self.pane_area_mode = PaneAreaMode::Stacked(tab_id);
                self.focus_terminal(tab_id);
                self.compute_layout();
                return;
            }

            // Pane tab drag init
            if let Some(pane_id) = self.pane_at_tab_bar(self.last_cursor_pos) {
                self.pane_drag = PaneDragState::PendingDrag {
                    source_pane: pane_id,
                    press_pos: self.last_cursor_pos,
                };
                self.focus_terminal(pane_id);
                return;
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
            self.mouse_left_pressed = false;
        }

        // End handle drag on release
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

        if self.file_tree_border_dragging {
            self.file_tree_border_dragging = false;
            self.compute_layout();
            self.clamp_panel_tab_scroll();
            return;
        }

        if self.panel_border_dragging {
            self.panel_border_dragging = false;
            self.compute_layout();
            self.clamp_panel_tab_scroll();
            return;
        }

        let drag_state = std::mem::replace(&mut self.pane_drag, PaneDragState::Idle);
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
                self.focus_terminal(source_pane);
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
        window: &dyn PlatformWindow,
    ) {
        self.last_cursor_pos = pos;

        // Handle border resizes
        if self.file_tree_border_dragging {
            let logical = self.logical_size();
            let dock_w = if self.show_editor_panel {
                self.editor_panel_width
            } else {
                0.0
            };
            let max_w = (logical.width - dock_w - 100.0).max(120.0);
            let new_width = match self.sidebar_side {
                crate::LayoutSide::Left => pos.x.max(120.0).min(max_w),
                crate::LayoutSide::Right => (logical.width - pos.x).max(120.0).min(max_w),
            };
            self.file_tree_width = new_width;
            self.compute_layout();
            self.clamp_panel_tab_scroll();
            self.chrome_generation += 1;
            self.needs_redraw = true;
            return;
        }

        if self.panel_border_dragging {
            let logical = self.logical_size();
            let sidebar_w = if self.show_file_tree {
                self.file_tree_width
            } else {
                0.0
            };
            let same_side_sidebar = self.show_file_tree && self.sidebar_side == self.dock_side;
            let max_w = (logical.width - sidebar_w - 100.0).max(150.0);
            let new_width = match self.dock_side {
                crate::LayoutSide::Right => {
                    let offset = if same_side_sidebar { sidebar_w } else { 0.0 };
                    (logical.width - offset - pos.x).max(150.0).min(max_w)
                }
                crate::LayoutSide::Left => {
                    let offset = if self.show_file_tree
                        && self.sidebar_side == crate::LayoutSide::Left
                    {
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
            self.needs_redraw = true;
            return;
        }

        // Handle side drag preview
        if self.sidebar_handle_dragging || self.dock_handle_dragging {
            let logical = self.logical_size();
            let win_center = logical.width / 2.0;
            let target_side = if pos.x < win_center {
                crate::LayoutSide::Left
            } else {
                crate::LayoutSide::Right
            };
            let new_preview = Some(target_side);
            if self.handle_drag_preview != new_preview {
                self.handle_drag_preview = new_preview;
                self.chrome_generation += 1;
            }
            self.needs_redraw = true;
            return;
        }

        // Auto-unstack
        if let PaneDragState::PendingDrag { press_pos, .. } = &self.pane_drag {
            let dx = pos.x - press_pos.x;
            let dy = pos.y - press_pos.y;
            if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD
                && matches!(self.pane_area_mode, PaneAreaMode::Stacked(_))
            {
                self.pane_area_mode = PaneAreaMode::Split;
                self.compute_layout();
            }
        }

        // Handle pane drag
        match &self.pane_drag {
            PaneDragState::PendingDrag {
                source_pane,
                press_pos,
            } => {
                let dx = pos.x - press_pos.x;
                let dy = pos.y - press_pos.y;
                if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD {
                    let source = *source_pane;
                    let target = self.compute_drop_destination(pos, source);
                    self.pane_drag = PaneDragState::Dragging {
                        source_pane: source,
                        drop_target: target,
                    };
                }
                self.needs_redraw = true;
                return;
            }
            PaneDragState::Dragging { source_pane, .. } => {
                let source = *source_pane;
                let target = self.compute_drop_destination(pos, source);
                self.pane_drag = PaneDragState::Dragging {
                    source_pane: source,
                    drop_target: target,
                };
                self.needs_redraw = true;
                return;
            }
            PaneDragState::Idle => {}
        }

        if self.router.is_dragging_border() {
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
            self.needs_redraw = true;
        } else {
            // Text selection drag
            if self.mouse_left_pressed {
                let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                let drag_top_offset = self.pane_area_mode.content_top();

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
                        let gutter_width = 5.0 * cs.width;
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
                        Some(PaneKind::Editor(pane)) => {
                            if let (Some(ref mut sel), Some((rel_row, rel_col))) =
                                (&mut pane.selection, editor_cell)
                            {
                                sel.end = (
                                    pane.editor.scroll_offset() + rel_row,
                                    pane.editor.h_scroll_offset() + rel_col,
                                );
                            }
                        }
                        Some(PaneKind::Diff(_)) => {}
                        None => {}
                    }
                }
                // Panel editor selection
                if let (Some(active_id), Some(panel_rect), Some(cs)) =
                    (self.active_editor_tab(), self.editor_panel_rect, cell_size)
                {
                    let gutter_width = 5.0 * cs.width;
                    let content_x = panel_rect.x + PANE_PADDING + gutter_width;
                    let content_y = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                    let rel_col = ((pos.x - content_x) / cs.width).floor() as isize;
                    let rel_row = ((pos.y - content_y) / cs.height).floor() as isize;
                    if rel_row >= 0 && rel_col >= 0 {
                        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&active_id) {
                            if let Some(ref mut sel) = pane.selection {
                                sel.end = (
                                    pane.editor.scroll_offset() + rel_row as usize,
                                    pane.editor.h_scroll_offset() + rel_col as usize,
                                );
                            }
                        }
                    }
                }
                self.needs_redraw = true;
            }

            // Hover target
            let new_hover = self.compute_hover_target(pos);
            if new_hover != self.hover_target {
                self.hover_target = new_hover;
                self.update_cursor_icon(window);
                self.needs_redraw = true;
            }

            let input = InputEvent::MouseMove { position: pos };
            let _ = self.router.process(input, &self.pane_rects);
        }
    }
}
