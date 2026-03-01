use tide_core::{FileTreeSource, Rect, SplitDirection, TerminalBackend, Vec2};

use crate::drag_drop::{DropDestination, HoverTarget};
use crate::header::{HeaderHitAction, HeaderHitZone};
use crate::pane::{PaneKind, Selection};
use crate::theme::*;
use crate::ui_state::FocusArea;
use crate::{App, GitSwitcherMode, GitSwitcherState, PaneAreaMode, shell_escape};

impl App {
    /// Convert a pixel position to a terminal cell (row, col) within a pane's content area.
    /// Returns None if the position is outside any terminal pane's content area.
    pub(crate) fn pixel_to_cell(&self, pos: Vec2, pane_id: tide_core::PaneId) -> Option<(usize, usize)> {
        let (_, visual_rect) = self.visual_pane_rects.iter().find(|(id, _)| *id == pane_id)?;
        let cell_size = self.cell_size();
        let content_top = self.pane_area_mode.content_top();
        let inner_x = visual_rect.x + PANE_PADDING;
        let inner_y = visual_rect.y + content_top;
        let col = ((pos.x - inner_x) / cell_size.width).floor() as isize;
        let row = ((pos.y - inner_y) / cell_size.height).floor() as isize;
        if row >= 0 && col >= 0 {
            Some((row as usize, col as usize))
        } else {
            None
        }
    }

    /// Compute the hover target for a given cursor position.
    /// Priority: TopHandles → PanelBorder → SplitBorder → PanelTabClose → PanelTab → PaneTabBar → FileTreeBorder → FileTreeEntry → None
    pub(crate) fn compute_hover_target(&self, pos: Vec2) -> Option<HoverTarget> {
        // Titlebar buttons (right-to-left: swap icon, dock toggle, sidebar toggle)
        if self.top_inset > 0.0 {
            let logical = self.logical_size();
            let cs = self.cell_size();

            // Swap icon dimensions (enlarged)
            let swap_icon_h = 16.0_f32;
            let swap_rect_w = 7.0_f32;
            let swap_gap = 3.0_f32;
            let swap_icon_w = swap_rect_w * 2.0 + swap_gap;
            let swap_x = logical.width - PANE_PADDING - swap_icon_w;
            let swap_y = (self.top_inset - swap_icon_h) / 2.0;
            let swap_pad = 4.0_f32;
            if pos.x >= swap_x - swap_pad && pos.x <= swap_x + swap_icon_w + swap_pad
                && pos.y >= swap_y - swap_pad && pos.y <= swap_y + swap_icon_h + swap_pad
            {
                return Some(HoverTarget::TitlebarSwap);
            }

            // Settings gear icon (between toggle buttons and swap icon)
            let gear_pad = 4.0_f32;
            let gear_w = cs.width + gear_pad * 2.0;
            let gear_h = cs.height + 6.0;
            let gear_x = swap_x - gear_w - 8.0;
            let gear_y = (self.top_inset - gear_h) / 2.0;
            if pos.x >= gear_x && pos.x <= gear_x + gear_w
                && pos.y >= gear_y && pos.y <= gear_y + gear_h
            {
                return Some(HoverTarget::TitlebarSettings);
            }

            // Theme toggle icon (between settings and toggle buttons)
            let theme_pad = 4.0_f32;
            let theme_w = cs.width + theme_pad * 2.0;
            let theme_h = cs.height + 6.0;
            let theme_x = gear_x - theme_w - 8.0;
            let theme_y = (self.top_inset - theme_h) / 2.0;
            if pos.x >= theme_x && pos.x <= theme_x + theme_w
                && pos.y >= theme_y && pos.y <= theme_y + theme_h
            {
                return Some(HoverTarget::TitlebarTheme);
            }

            // Titlebar toggle buttons: rendered based on area_ordering() with fixed ⌘1/2/3
            // All buttons have same width: icon(1) + space(1) + hint(2) = 4 chars + padding
            let btn_pad_h = 6.0_f32;
            let btn_chars = 4.0_f32;
            let btn_w = btn_chars * cs.width + btn_pad_h * 2.0;
            let btn_h = cs.height + 6.0;
            let btn_y = (self.top_inset - btn_h) / 2.0;

            // Hit test right-to-left matching render order (slot 3, 2, 1)
            let areas = self.area_ordering();
            let mut cur_right = theme_x - TITLEBAR_BUTTON_GAP;
            for area in areas.iter().rev() {
                let btn_x = cur_right - btn_w;
                if pos.x >= btn_x && pos.x <= btn_x + btn_w
                    && pos.y >= btn_y && pos.y <= btn_y + btn_h
                {
                    return Some(match area {
                        FocusArea::FileTree => HoverTarget::TitlebarFileTree,
                        FocusArea::PaneArea => HoverTarget::TitlebarPaneArea,
                        FocusArea::EditorDock => HoverTarget::TitlebarDock,
                    });
                }
                cur_right -= btn_w + TITLEBAR_BUTTON_GAP;
            }
        }

        // Top-edge drag handles (top strip of sidebar/dock panels)
        if let Some(ft_rect) = self.file_tree_rect {
            if pos.y >= ft_rect.y && pos.y < ft_rect.y + PANE_PADDING
                && pos.x >= ft_rect.x && pos.x < ft_rect.x + ft_rect.width
            {
                return Some(HoverTarget::SidebarHandle);
            }
        }
        // Dock close, maximize, and preview toggle badges (checked before DockHandle
        // so clicks on badges in the tab bar aren't intercepted as handle drags)
        if let Some(panel_rect) = self.editor_panel_rect {
            let cs = self.cell_size();
            let cell_w = cs.width;
            let tab_bar_y = panel_rect.y + PANE_CORNER_RADIUS;
            let badge_gap = 6.0_f32;

            // Close button (far right)
            let close_w = cell_w + BADGE_PADDING_H * 2.0;
            let close_x = panel_rect.x + panel_rect.width - PANE_PADDING - close_w;

            // Maximize button (left of close)
            let max_w = cell_w + BADGE_PADDING_H * 2.0;
            let max_x = close_x - badge_gap - max_w;
            let max_rect = Rect::new(max_x, tab_bar_y, max_w, PANEL_TAB_HEIGHT);
            if max_rect.contains(pos) {
                return Some(HoverTarget::DockMaximize);
            }

            // Dock preview toggle badge (left of maximize)
            if let Some(active_id) = self.active_editor_tab() {
                if let Some(PaneKind::Editor(ep)) = self.panes.get(&active_id) {
                    if ep.is_markdown() && !ep.diff_mode {
                        let preview_text = if ep.preview_mode { "edit" } else { "preview" };
                        let badge_w = preview_text.len() as f32 * cell_w + BADGE_PADDING_H * 2.0;
                        let badge_x = max_x - BADGE_GAP - badge_w;
                        let badge_rect = Rect::new(badge_x, tab_bar_y, badge_w, PANEL_TAB_HEIGHT);
                        if badge_rect.contains(pos) {
                            return Some(HoverTarget::DockPreviewToggle);
                        }
                    }
                }
            }

            // Browser nav bar hover targets
            if let Some(active_id) = self.active_editor_tab() {
                if let Some(PaneKind::Browser(_bp)) = self.panes.get(&active_id) {
                    let cell_height_actual = cs.height;
                    let nav_h = (cell_height_actual * 1.5).round();
                    let nav_y = panel_rect.y + PANEL_TAB_HEIGHT + 2.0;
                    let nav_x = panel_rect.x + PANE_PADDING;
                    let nav_w = panel_rect.width - PANE_PADDING * 2.0;

                    if pos.y >= nav_y && pos.y <= nav_y + nav_h
                        && pos.x >= nav_x && pos.x <= nav_x + nav_w
                    {
                        let mut cx = nav_x + 8.0;
                        let btn_w = cell_w * 2.0;

                        // Back button
                        if pos.x >= cx && pos.x < cx + btn_w {
                            return Some(HoverTarget::BrowserBack);
                        }
                        cx += btn_w;

                        // Forward button
                        if pos.x >= cx && pos.x < cx + btn_w {
                            return Some(HoverTarget::BrowserForward);
                        }
                        cx += btn_w;

                        // Refresh button
                        if pos.x >= cx && pos.x < cx + btn_w {
                            return Some(HoverTarget::BrowserRefresh);
                        }
                        cx += btn_w + 4.0;

                        // URL bar (rest of nav area)
                        if pos.x >= cx {
                            return Some(HoverTarget::BrowserUrlBar);
                        }
                    }
                }
            }
        }

        if let Some(panel_rect) = self.editor_panel_rect {
            if pos.y >= panel_rect.y && pos.y < panel_rect.y + PANE_PADDING
                && pos.x >= panel_rect.x && pos.x < panel_rect.x + panel_rect.width
            {
                return Some(HoverTarget::DockHandle);
            }
        }

        // Panel border (resize handle) — position depends on dock side
        if let Some(panel_rect) = self.editor_panel_rect {
            let border_x = if self.dock_side == crate::LayoutSide::Right {
                panel_rect.x
            } else {
                panel_rect.x + panel_rect.width + PANE_GAP
            };
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

        // Per-tab close indicator (check before general panel_tab_at)
        if let Some(tab_id) = self.panel_tab_item_close_at(pos) {
            return Some(HoverTarget::PanelTabItemClose(tab_id));
        }

        // Panel tab close button (header close)
        if let Some(tab_id) = self.panel_tab_close_at(pos) {
            return Some(HoverTarget::PanelTabClose(tab_id));
        }

        // Panel tab
        if let Some(tab_id) = self.panel_tab_at(pos) {
            return Some(HoverTarget::PanelTab(tab_id));
        }

        // Stacked mode: [mode toggle] [maximize] [close] — right side controls
        if let PaneAreaMode::Stacked(_) = self.pane_area_mode {
            if let Some(&(_, rect)) = self.visual_pane_rects.first() {
                let cs = self.cell_size();
                let cell_w = cs.width;
                let cell_h = cs.height;
                let content_right = rect.x + rect.width - PANE_PADDING;
                let close_w = cell_w + BADGE_PADDING_H * 2.0;
                let close_x = content_right - close_w;
                let badge_gap = 6.0_f32;
                let badge_pad = 6.0_f32;
                let badge_h = cell_h + 4.0;

                // Maximize button (between mode toggle and close)
                let max_badge_w = cell_w + badge_pad * 2.0;
                let max_badge_x = close_x - badge_gap - max_badge_w;
                let max_badge_y = rect.y + (TAB_BAR_HEIGHT - badge_h) / 2.0;
                if pos.x >= max_badge_x && pos.x <= max_badge_x + max_badge_w
                    && pos.y >= max_badge_y && pos.y <= max_badge_y + badge_h
                {
                    return Some(HoverTarget::PaneAreaMaximize);
                }

                // Mode toggle badge (leftmost of right-side controls)
                let mode_hint_len = 2;
                let mode_badge_chars = (1 + 1 + mode_hint_len) as f32;
                let mode_badge_w = mode_badge_chars * cell_w + badge_pad * 2.0;
                let mode_badge_x = max_badge_x - badge_gap - mode_badge_w;
                let mode_badge_y = rect.y + (TAB_BAR_HEIGHT - badge_h) / 2.0;
                if pos.x >= mode_badge_x && pos.x <= mode_badge_x + mode_badge_w
                    && pos.y >= mode_badge_y && pos.y <= mode_badge_y + badge_h
                {
                    return Some(HoverTarget::PaneModeToggle);
                }
            }
        }

        // Stacked tab bar close button (before general stacked tab check)
        if let Some(tab_id) = self.stacked_tab_close_at(pos) {
            return Some(HoverTarget::StackedTabClose(tab_id));
        }

        // Stacked tab bar
        if let Some(tab_id) = self.stacked_tab_at(pos) {
            return Some(HoverTarget::StackedTab(tab_id));
        }

        // Pane tab bar close button (before general tab bar check)
        if let Some(pane_id) = self.pane_tab_close_at(pos) {
            return Some(HoverTarget::PaneTabClose(pane_id));
        }

        // Pane header maximize button (between close and badges)
        if let Some(pane_id) = self.pane_maximize_at(pos) {
            return Some(HoverTarget::PaneMaximize(pane_id));
        }

        // Pane tab bar (split tree panes)
        if let Some(pane_id) = self.pane_at_tab_bar(pos) {
            return Some(HoverTarget::PaneTabBar(pane_id));
        }

        // File tree border (resize handle) — position depends on sidebar side
        if let Some(ft_rect) = self.file_tree_rect {
            let border_x = if self.sidebar_side == crate::LayoutSide::Left {
                ft_rect.x + ft_rect.width + PANE_GAP
            } else {
                ft_rect.x - PANE_GAP
            };
            if (pos.x - border_x).abs() < 5.0 {
                return Some(HoverTarget::FileTreeBorder);
            }
        }

        // File tree entry
        if self.show_file_tree && self.file_tree_rect.is_some_and(|r| pos.x >= r.x && pos.x < r.x + r.width) {
            let ft_rect = self.file_tree_rect.unwrap();
            let cell_size = self.cell_size();
            let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
            let content_y = ft_rect.y + PANE_CORNER_RADIUS;
            if pos.y < content_y + FILE_TREE_HEADER_HEIGHT {
                return None;
            }
            let adjusted_y = pos.y - content_y - FILE_TREE_HEADER_HEIGHT;
            let index = ((adjusted_y + self.file_tree_scroll) / line_height) as usize;
            if let Some(tree) = &self.file_tree {
                let entries = tree.visible_entries();
                if index < entries.len() {
                    return Some(HoverTarget::FileTreeEntry(index));
                }
            }
        }

        // Editor scrollbar hover (pane area or dock panel)
        {
            let cell_size = self.cell_size();
            // Check dock panel scrollbar
            if let (Some(active_id), Some(panel_rect)) = (self.active_editor_tab(), self.editor_panel_rect) {
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&active_id) {
                    let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                    let inner = Rect::new(
                        panel_rect.x + PANE_PADDING,
                        content_top,
                        panel_rect.width - 2.0 * PANE_PADDING,
                        (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0),
                    );
                    if pane.needs_scrollbar(inner, cell_size.height) {
                        let sb_x = inner.x + inner.width - SCROLLBAR_WIDTH_HOVER;
                        if pos.x >= sb_x && pos.x <= inner.x + inner.width && pos.y >= inner.y && pos.y <= inner.y + inner.height {
                            return Some(HoverTarget::EditorScrollbar(active_id));
                        }
                    }
                }
            }
            // Check pane area editor scrollbars
            let top_offset = self.pane_area_mode.content_top();
            for &(id, rect) in &self.visual_pane_rects {
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&id) {
                    let inner = Rect::new(
                        rect.x + PANE_PADDING,
                        rect.y + top_offset,
                        rect.width - 2.0 * PANE_PADDING,
                        (rect.height - top_offset - PANE_PADDING).max(1.0),
                    );
                    if pane.needs_scrollbar(inner, cell_size.height) {
                        let sb_x = inner.x + inner.width - SCROLLBAR_WIDTH_HOVER;
                        if pos.x >= sb_x && pos.x <= inner.x + inner.width && pos.y >= inner.y && pos.y <= inner.y + inner.height {
                            return Some(HoverTarget::EditorScrollbar(id));
                        }
                    }
                }
            }
        }

        None
    }

    /// Check if the current cursor position clicks on a header badge or close button.
    /// Returns true if the click was consumed.
    pub(crate) fn check_header_click(&mut self) -> bool {
        let pos = self.last_cursor_pos;
        let zones: Vec<HeaderHitZone> = self.header_hit_zones.clone();
        for zone in &zones {
            if zone.rect.contains(pos) {
                match zone.action {
                    HeaderHitAction::Close => {
                        self.close_specific_pane(zone.pane_id);
                        self.needs_redraw = true;
                        return true;
                    }
                    HeaderHitAction::GitBranch => {
                        // Single badge opens git switcher; popup tabs handle branch/worktree switching
                        self.open_git_switcher(zone.pane_id, GitSwitcherMode::Branches, zone.rect);
                        self.needs_redraw = true;
                        return true;
                    }
                    HeaderHitAction::GitStatus => {
                        // Open or focus the Diff pane for this terminal's CWD
                        let cwd = if let Some(PaneKind::Terminal(pane)) = self.panes.get(&zone.pane_id) {
                            pane.cwd.clone()
                        } else {
                            None
                        };
                        if let Some(cwd) = cwd {
                            self.open_diff_pane(cwd);
                        }
                        self.needs_redraw = true;
                        return true;
                    }
                    HeaderHitAction::EditorCompare => {
                        // Enter diff mode (load disk content)
                        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&zone.pane_id) {
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
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&zone.pane_id);
                        self.needs_redraw = true;
                        return true;
                    }
                    HeaderHitAction::EditorBack => {
                        // Exit diff mode, return to conflict state
                        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&zone.pane_id) {
                            pane.diff_mode = false;
                            pane.disk_content = None;
                        }
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&zone.pane_id);
                        self.needs_redraw = true;
                        return true;
                    }
                    HeaderHitAction::MarkdownPreview => {
                        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&zone.pane_id) {
                            pane.toggle_preview();
                        }
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&zone.pane_id);
                        self.needs_redraw = true;
                        return true;
                    }
                    HeaderHitAction::EditorFileName => {
                        // Click on file name badge: open file switcher popup
                        let anchor_rect = zone.rect;
                        let active_tab = self.active_editor_tab();
                        let entries: Vec<crate::FileSwitcherEntry> = self.active_editor_tabs().iter()
                            .filter_map(|&tab_id| {
                                let name = match self.panes.get(&tab_id) {
                                    Some(PaneKind::Editor(ep)) => ep.title(),
                                    Some(PaneKind::Diff(_)) => "Git Changes".to_string(),
                                    _ => return None,
                                };
                                Some(crate::FileSwitcherEntry {
                                    pane_id: tab_id,
                                    name,
                                    is_active: active_tab == Some(tab_id),
                                })
                            })
                            .collect();
                        if !entries.is_empty() {
                            self.file_switcher = Some(crate::FileSwitcherState::new(entries, anchor_rect));
                        }
                        self.needs_redraw = true;
                        return true;
                    }
                    HeaderHitAction::DiffRefresh => {
                        // Refresh the DiffPane
                        if let Some(PaneKind::Diff(dp)) = self.panes.get_mut(&zone.pane_id) {
                            dp.refresh();
                        }
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&zone.pane_id);
                        self.needs_redraw = true;
                        return true;
                    }
                    HeaderHitAction::Maximize => {
                        // Toggle pane area maximize (hide/show dock) without changing mode
                        self.focus_terminal(zone.pane_id);
                        self.editor_panel_maximized = false;
                        self.pane_area_maximized = !self.pane_area_maximized;
                        self.chrome_generation += 1;
                        self.compute_layout();
                        self.needs_redraw = true;
                        return true;
                    }
                }
            }
        }
        false
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

    /// Handle a browser nav bar click based on hover target.
    pub(crate) fn handle_browser_nav_click(&mut self, target: &HoverTarget) {
        let active_id = match self.active_editor_tab() {
            Some(id) => id,
            None => return,
        };
        match target {
            HoverTarget::BrowserBack => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&active_id) {
                    bp.go_back();
                }
            }
            HoverTarget::BrowserForward => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&active_id) {
                    bp.go_forward();
                }
            }
            HoverTarget::BrowserRefresh => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&active_id) {
                    bp.reload();
                }
            }
            HoverTarget::BrowserUrlBar => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&active_id) {
                    bp.url_input_focused = true;
                    bp.url_input = bp.url.clone();
                    bp.url_input_cursor = bp.url_input.chars().count();
                }
            }
            _ => {}
        }
        self.chrome_generation += 1;
        self.needs_redraw = true;
    }

    /// Handle editor panel content area click: focus and move cursor.
    pub(crate) fn handle_editor_panel_click(&mut self, pos: Vec2) {
        // Content area click → focus and move cursor
        if let Some(active_id) = self.active_editor_tab() {
            self.focus_area = FocusArea::EditorDock;
            self.chrome_generation += 1;

            // Move cursor to click position + start selection
            if let Some(panel_rect) = self.editor_panel_rect {
                let cell_size = self.cell_size();
                let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cell_size.width;
                let editor_content_x = panel_rect.x + PANE_PADDING + gutter_width;
                let preview_content_x = panel_rect.x + PANE_PADDING;
                let rel_row = ((pos.y - content_top) / cell_size.height).floor() as isize;
                let rel_col = ((pos.x - editor_content_x) / cell_size.width).floor() as isize;
                let preview_rel_col = ((pos.x - preview_content_x) / cell_size.width).floor() as isize;

                if rel_row >= 0 {
                    match self.panes.get_mut(&active_id) {
                        Some(PaneKind::Editor(pane)) if pane.preview_mode => {
                            // Allow text selection in preview mode (no gutter, no cursor movement)
                            let line = pane.preview_scroll + rel_row as usize;
                            let col = preview_rel_col.max(0) as usize;
                            pane.selection = Some(Selection {
                                anchor: (line, col),
                                end: (line, col),
                            });
                        }
                        Some(PaneKind::Editor(pane)) if rel_col >= 0 => {
                            use tide_editor::input::EditorAction;
                            let line = pane.editor.scroll_offset() + rel_row as usize;
                            let col = pane.editor.h_scroll_offset() + rel_col as usize;
                            let content_height = (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0);
                            let visible_rows = (content_height / cell_size.height).floor() as usize;
                            pane.handle_action(EditorAction::SetCursor { line, col }, visible_rows);
                            pane.selection = Some(Selection {
                                anchor: (line, col),
                                end: (line, col),
                            });
                        }
                        Some(PaneKind::Diff(dp)) => {
                            let visual_row = rel_row as usize;
                            if let Some(fi) = dp.file_at_row(visual_row) {
                                dp.toggle_expand(fi);
                                self.pane_generations.remove(&active_id);
                            }
                        }
                        _ => {}
                    }
                }
            }
        } else if self.show_editor_panel {
            // Empty panel: set focus to EditorDock without changing focused terminal
            self.focus_area = FocusArea::EditorDock;
            self.chrome_generation += 1;
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
                    let cell_size = self.cell_size();
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
        if let (Some(active_id), Some(panel_rect)) = (self.active_editor_tab(), self.editor_panel_rect) {
            if active_id == pane_id {
                let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                let bar_x = panel_rect.x + PANE_PADDING;
                let bar_w = panel_rect.width - 2.0 * PANE_PADDING;
                return Some(Rect::new(bar_x, content_top, bar_w, CONFLICT_BAR_HEIGHT));
            }
        }
        // Check left-side panes
        let content_top_off = self.pane_area_mode.content_top();
        if let Some(&(_, rect)) = self.visual_pane_rects.iter().find(|(id, _)| *id == pane_id) {
            let content_top = rect.y + content_top_off;
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
        if let (Some(active_id), Some(panel_rect)) = (self.active_editor_tab(), self.editor_panel_rect) {
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
        let content_top_off = self.pane_area_mode.content_top();
        if target_pane.is_none() {
            for &(id, rect) in &self.visual_pane_rects {
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&id) {
                    if pane.needs_notification_bar() {
                        let content_top = rect.y + content_top_off;
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

        let cell_size = self.cell_size();

        let (is_deleted, is_diff_mode) = self.panes.get(&pane_id)
            .and_then(|pk| if let PaneKind::Editor(ep) = pk { Some((ep.file_deleted, ep.diff_mode)) } else { None })
            .unwrap_or((false, false));

        let btn_pad = 8.0;

        // Overwrite button (rightmost)
        let overwrite_w = 9.0 * cell_size.width + btn_pad * 2.0;
        let overwrite_x = bar_rect.x + bar_rect.width - overwrite_w - 4.0;

        // Reload button (diff mode only, not for deleted files)
        let reload_w = 6.0 * cell_size.width + btn_pad * 2.0;
        let reload_x = overwrite_x - reload_w - 4.0;

        if pos.x >= overwrite_x {
            // Overwrite — save buffer to disk, clear all conflict/diff state
            if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
                if let Err(e) = pane.editor.buffer.save() {
                    log::error!("Conflict overwrite failed: {}", e);
                }
                pane.disk_changed = false;
                pane.file_deleted = false;
                pane.diff_mode = false;
                pane.disk_content = None;
            }
        } else if is_diff_mode && !is_deleted && pos.x >= reload_x {
            // Reload — reload from disk, discard local edits
            if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
                if let Err(e) = pane.editor.reload() {
                    log::error!("Reload failed: {}", e);
                }
                pane.disk_changed = false;
                pane.file_deleted = false;
                pane.diff_mode = false;
                pane.disk_content = None;
            }
        }

        self.chrome_generation += 1;
        self.pane_generations.remove(&pane_id);
        self.needs_redraw = true;
        true
    }

    /// Open the git switcher popup (works even when a process is running).
    /// Clicking the same badge again closes the popup (toggle behavior).
    fn open_git_switcher(&mut self, pane_id: tide_core::PaneId, mode: GitSwitcherMode, anchor_rect: Rect) {
        // Toggle: close if already open for the same pane and mode
        if let Some(ref gs) = self.git_switcher {
            if gs.pane_id == pane_id && gs.mode == mode {
                self.git_switcher = None;
                return;
            }
        }
        if let Some(PaneKind::Terminal(pane)) = self.panes.get(&pane_id) {
            let shell_busy = !pane.shell_idle;
            if let Some(ref cwd) = pane.cwd {
                let branches = tide_terminal::git::list_branches(cwd);
                let worktrees = tide_terminal::git::list_worktrees(cwd);
                let mut gs = GitSwitcherState::new(
                    pane_id, mode, branches, worktrees, anchor_rect,
                );
                gs.shell_busy = shell_busy;
                self.git_switcher = Some(gs);
            }
        }
    }

    /// Get the cwd of the terminal pane associated with the git switcher.
    fn git_switcher_pane_cwd(&self) -> Option<std::path::PathBuf> {
        let gs = self.git_switcher.as_ref()?;
        match self.panes.get(&gs.pane_id) {
            Some(PaneKind::Terminal(p)) => p.cwd.clone(),
            _ => None,
        }
    }

    /// Handle a git switcher popup button click.
    pub(crate) fn handle_git_switcher_button(&mut self, btn: crate::SwitcherButton) {
        match btn {
            crate::SwitcherButton::Switch(fi) => {
                let gs = match self.git_switcher.as_ref() {
                    Some(gs) => gs,
                    None => return,
                };
                let pane_id = gs.pane_id;

                if gs.is_create_row(fi) {
                    // Create row
                    let query = gs.input.text.trim().to_string();
                    let mode = gs.mode;
                    let cwd = self.git_switcher_pane_cwd();
                    self.git_switcher = None;
                    if let Some(cwd) = cwd {
                        match mode {
                            crate::GitSwitcherMode::Branches => {
                                // git checkout -b <query>
                                if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&pane_id) {
                                    if pane.shell_idle {
                                        let cmd = format!("git checkout -b {}\n", shell_escape(&query));
                                        pane.backend.write(cmd.as_bytes());
                                    }
                                }
                            }
                            crate::GitSwitcherMode::Worktrees => {
                                // add_worktree + cd path
                                let root = tide_terminal::git::repo_root(&cwd).unwrap_or_else(|| cwd.clone());
                                let settings = crate::settings::load_settings();
                                let wt_path = settings.worktree.compute_worktree_path(&root, &query);
                                let new_branch = !tide_terminal::git::branch_exists(&cwd, &query);
                                match tide_terminal::git::add_worktree(&cwd, &wt_path, &query, new_branch) {
                                    Ok(()) => {
                                        settings.worktree.copy_files_to_worktree(&root, &wt_path);
                                        if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&pane_id) {
                                            if pane.shell_idle {
                                                let cmd = format!("cd {}\n", shell_escape(&wt_path.to_string_lossy()));
                                                pane.backend.write(cmd.as_bytes());
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("Failed to create worktree: {}", e);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    match gs.mode {
                        crate::GitSwitcherMode::Branches => {
                            let action = {
                                let entry_idx = match gs.filtered_branches.get(fi) {
                                    Some(&i) => i,
                                    None => { self.git_switcher = None; return; }
                                };
                                let branch = &gs.branches[entry_idx];
                                if branch.is_current { self.git_switcher = None; return; }
                                let has_wt = gs.worktree_branch_names.contains(&branch.name);
                                if has_wt {
                                    // Find the worktree path for this branch
                                    let wt_path = gs.worktrees.iter()
                                        .find(|wt| wt.branch.as_deref() == Some(&branch.name))
                                        .map(|wt| wt.path.to_string_lossy().to_string());
                                    (branch.name.clone(), wt_path)
                                } else {
                                    (branch.name.clone(), None)
                                }
                            };
                            self.git_switcher = None;
                            if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&pane_id) {
                                if pane.shell_idle {
                                    let cmd = if let Some(wt_path) = action.1 {
                                        format!("cd {}\n", shell_escape(&wt_path))
                                    } else {
                                        format!("git checkout {}\n", shell_escape(&action.0))
                                    };
                                    pane.backend.write(cmd.as_bytes());
                                }
                            }
                        }
                        crate::GitSwitcherMode::Worktrees => {
                            let action = gs.filtered_worktrees.get(fi).and_then(|&entry_idx| {
                                let wt = gs.worktrees.get(entry_idx)?;
                                Some(wt.path.to_string_lossy().to_string())
                            });
                            self.git_switcher = None;
                            if let Some(path) = action {
                                if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&pane_id) {
                                    if pane.shell_idle {
                                        let cmd = format!("cd {}\n", shell_escape(&path));
                                        pane.backend.write(cmd.as_bytes());
                                    }
                                }
                            }
                        }
                    }
                }
            }
            crate::SwitcherButton::Delete(fi) => {
                // Check create row and confirmation state with immutable borrow first
                let (is_create, already_confirmed, mode) = match self.git_switcher.as_ref() {
                    Some(gs) => (gs.is_create_row(fi), gs.delete_confirm == Some(fi), gs.mode),
                    None => return,
                };
                if is_create { return; }

                // First click: show confirmation. Second click (same row): execute.
                if !already_confirmed {
                    if let Some(ref mut gs) = self.git_switcher {
                        gs.delete_confirm = Some(fi);
                    }
                    self.chrome_generation += 1;
                    self.needs_redraw = true;
                    return;
                }
                // Confirmed — proceed with delete
                if let Some(ref mut gs) = self.git_switcher {
                    gs.delete_confirm = None;
                }

                let cwd = self.git_switcher_pane_cwd();

                match mode {
                    crate::GitSwitcherMode::Branches => {
                        let (branch_name, wt_path) = {
                            let gs = self.git_switcher.as_ref().unwrap();
                            let entry_idx = match gs.filtered_branches.get(fi) {
                                Some(&i) => i,
                                None => return,
                            };
                            let branch = &gs.branches[entry_idx];
                            if branch.is_current { return; }
                            let wt_path = gs.worktrees.iter()
                                .find(|wt| wt.branch.as_deref() == Some(&branch.name))
                                .map(|wt| wt.path.clone());
                            (branch.name.clone(), wt_path)
                        };
                        if let Some(cwd) = cwd {
                            // If branch has a worktree, remove it first
                            if let Some(ref wt_path) = wt_path {
                                if let Err(e) = tide_terminal::git::remove_worktree(&cwd, wt_path, true) {
                                    log::error!("Failed to remove worktree: {}", e);
                                }
                            }
                            // Delete the branch (force to handle unmerged)
                            if let Err(e) = tide_terminal::git::delete_branch(&cwd, &branch_name, true) {
                                log::error!("Failed to delete branch: {}", e);
                            }
                        }
                    }
                    crate::GitSwitcherMode::Worktrees => {
                        let (wt_path, branch_name, is_main) = {
                            let gs = self.git_switcher.as_ref().unwrap();
                            let entry_idx = match gs.filtered_worktrees.get(fi) {
                                Some(&i) => i,
                                None => return,
                            };
                            let wt = &gs.worktrees[entry_idx];
                            if wt.is_current || wt.is_main { return; }
                            (wt.path.clone(), wt.branch.clone(), wt.is_main)
                        };
                        if let Some(cwd) = cwd {
                            if !is_main {
                                if let Err(e) = tide_terminal::git::remove_worktree(&cwd, &wt_path, true) {
                                    log::error!("Failed to remove worktree: {}", e);
                                }
                                // Delete the branch too (unless main/master)
                                if let Some(ref branch) = branch_name {
                                    if branch != "main" && branch != "master" {
                                        if let Err(e) = tide_terminal::git::delete_branch(&cwd, branch, true) {
                                            log::error!("Failed to delete branch: {}", e);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }

                // Refresh the git switcher in-place
                self.refresh_git_switcher();
                self.chrome_generation += 1;
                self.needs_redraw = true;
                return;
            }
            crate::SwitcherButton::NewPane(fi) => {
                let gs = match self.git_switcher.as_ref() {
                    Some(gs) => gs,
                    None => return,
                };
                let pane_id = gs.pane_id;

                if gs.is_create_row(fi) {
                    let query = gs.input.text.trim().to_string();
                    let mode = gs.mode;
                    let cwd = self.git_switcher_pane_cwd();
                    self.git_switcher = None;
                    if let Some(cwd) = cwd {
                        match mode {
                            crate::GitSwitcherMode::Branches => {
                                // new pane in same repo + git checkout -b <query>
                                use tide_core::{LayoutEngine, SplitDirection};
                                let new_id = self.layout.split(pane_id, SplitDirection::Horizontal);
                                self.create_terminal_pane(new_id, Some(cwd));
                                if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&new_id) {
                                    let cmd = format!("git checkout -b {}\n", shell_escape(&query));
                                    pane.backend.write(cmd.as_bytes());
                                }
                                self.focused = Some(new_id);
                                self.router.set_focused(new_id);
                                self.compute_layout();
                            }
                            crate::GitSwitcherMode::Worktrees => {
                                // add_worktree + new pane in path
                                let root = tide_terminal::git::repo_root(&cwd).unwrap_or_else(|| cwd.clone());
                                let settings = crate::settings::load_settings();
                                let wt_path = settings.worktree.compute_worktree_path(&root, &query);
                                let new_branch = !tide_terminal::git::branch_exists(&cwd, &query);
                                match tide_terminal::git::add_worktree(&cwd, &wt_path, &query, new_branch) {
                                    Ok(()) => {
                                        settings.worktree.copy_files_to_worktree(&root, &wt_path);
                                        use tide_core::{LayoutEngine, SplitDirection};
                                        let new_id = self.layout.split(pane_id, SplitDirection::Horizontal);
                                        self.create_terminal_pane(new_id, Some(wt_path));
                                        if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
                                            self.pane_area_mode = PaneAreaMode::Stacked(new_id);
                                        }
                                        self.focused = Some(new_id);
                                        self.router.set_focused(new_id);
                                        self.chrome_generation += 1;
                                        self.compute_layout();
                                        self.scroll_to_active_stacked_tab();
                                    }
                                    Err(e) => {
                                        log::error!("Failed to create worktree: {}", e);
                                    }
                                }
                            }
                        }
                    }
                } else {
                    match gs.mode {
                        crate::GitSwitcherMode::Branches => {
                            let action = {
                                let entry_idx = match gs.filtered_branches.get(fi) {
                                    Some(&i) => i,
                                    None => { self.git_switcher = None; return; }
                                };
                                let branch = &gs.branches[entry_idx];
                                if branch.is_current { self.git_switcher = None; return; }
                                let has_wt = gs.worktree_branch_names.contains(&branch.name);
                                if has_wt {
                                    let wt_path = gs.worktrees.iter()
                                        .find(|wt| wt.branch.as_deref() == Some(&branch.name))
                                        .map(|wt| wt.path.clone());
                                    (branch.name.clone(), wt_path)
                                } else {
                                    (branch.name.clone(), None)
                                }
                            };
                            // Get cwd before clearing git_switcher
                            let pane_cwd = self.panes.get(&pane_id)
                                .and_then(|pk| if let PaneKind::Terminal(p) = pk { p.cwd.clone() } else { None });
                            self.git_switcher = None;
                            use tide_core::{LayoutEngine, SplitDirection};
                            if let Some(wt_path) = action.1 {
                                // Has worktree → new pane in worktree path
                                let new_id = self.layout.split(pane_id, SplitDirection::Horizontal);
                                self.create_terminal_pane(new_id, Some(wt_path));
                                self.focused = Some(new_id);
                                self.router.set_focused(new_id);
                                self.compute_layout();
                            } else {
                                // No worktree → new pane in same repo + git checkout
                                let new_id = self.layout.split(pane_id, SplitDirection::Horizontal);
                                self.create_terminal_pane(new_id, pane_cwd);
                                if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&new_id) {
                                    let cmd = format!("git checkout {}\n", shell_escape(&action.0));
                                    pane.backend.write(cmd.as_bytes());
                                }
                                self.focused = Some(new_id);
                                self.router.set_focused(new_id);
                                self.compute_layout();
                            }
                        }
                        crate::GitSwitcherMode::Worktrees => {
                            let wt_path = gs.filtered_worktrees.get(fi).and_then(|&entry_idx| {
                                let wt = gs.worktrees.get(entry_idx)?;
                                Some(wt.path.clone())
                            });
                            self.git_switcher = None;
                            if let Some(wt_path) = wt_path {
                                use tide_core::{LayoutEngine, SplitDirection};
                                let new_id = self.layout.split(pane_id, SplitDirection::Horizontal);
                                self.create_terminal_pane(new_id, Some(wt_path));
                                self.focused = Some(new_id);
                                self.router.set_focused(new_id);
                                self.compute_layout();
                            }
                        }
                    }
                }
            }
        }
        self.chrome_generation += 1;
        self.needs_redraw = true;
    }

    /// Handle click when config page is open.
    pub(crate) fn handle_config_page_click(&mut self, pos: Vec2) {
        use crate::ui_state::ConfigSection;

        let logical = self.logical_size();
        let popup_w = crate::theme::CONFIG_PAGE_W.min(logical.width - 80.0).max(300.0);
        let popup_h = crate::theme::CONFIG_PAGE_MAX_H.min(logical.height - 80.0).max(200.0);
        let popup_x = (logical.width - popup_w) / 2.0;
        let popup_y = (logical.height - popup_h) / 2.0;
        let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

        // Click outside popup → close
        if !popup_rect.contains(pos) {
            self.close_config_page();
            return;
        }

        let cell_size = self.cell_size();
        let cell_height = cell_size.height;

        // Title bar area
        let title_h = crate::theme::CONFIG_PAGE_TITLE_H;
        let title_y = popup_y + 2.0;

        // Tab bar area
        let tab_h = crate::theme::CONFIG_PAGE_TAB_H;
        let tab_y = title_y + title_h + 1.0;
        let half_w = popup_w / 2.0;

        // Click on tab bar → switch section
        if pos.y >= tab_y && pos.y < tab_y + tab_h {
            if let Some(ref mut page) = self.config_page {
                if pos.x < popup_x + half_w {
                    page.section = ConfigSection::Keybindings;
                } else {
                    page.section = ConfigSection::Worktree;
                }
                page.selected = 0;
                page.scroll_offset = 0;
            }
            self.chrome_generation += 1;
            return;
        }

        // Content area
        let content_top = tab_y + tab_h + 1.0;
        let hint_bar_h = crate::theme::CONFIG_PAGE_HINT_BAR_H;
        let content_bottom = popup_y + popup_h - hint_bar_h;
        let line_height = 32.0_f32.max(cell_height + crate::theme::POPUP_LINE_EXTRA);

        if pos.y >= content_top && pos.y < content_bottom {
            if let Some(ref mut page) = self.config_page {
                match page.section {
                    ConfigSection::Keybindings => {
                        let vi = ((pos.y - content_top) / line_height).floor() as usize;
                        let fi = page.scroll_offset + vi;
                        if fi < page.bindings.len() {
                            page.selected = fi;
                        }
                    }
                    ConfigSection::Worktree => {
                        let input_h = cell_height + crate::theme::POPUP_INPUT_PADDING;

                        // Base dir pattern input field
                        let wt_input_y = content_top + 8.0 + line_height + 4.0;
                        if pos.y >= wt_input_y && pos.y < wt_input_y + input_h {
                            page.selected_field = 0;
                            page.worktree_editing = true;
                            page.copy_files_editing = false;
                        }

                        // Copy files input field
                        let help_y = wt_input_y + input_h + 8.0;
                        let cf_label_y = help_y + cell_height + 12.0;
                        let cf_input_y = cf_label_y + line_height + 4.0;
                        if pos.y >= cf_input_y && pos.y < cf_input_y + input_h {
                            page.selected_field = 1;
                            page.copy_files_editing = true;
                            page.worktree_editing = false;
                        }
                    }
                }
            }
            self.chrome_generation += 1;
        }
    }

    /// Refresh the git switcher popup in-place after a delete operation.
    /// Re-fetches branches/worktrees and rebuilds state while preserving input, mode, scroll.
    fn refresh_git_switcher(&mut self) {
        let gs = match self.git_switcher.as_ref() {
            Some(gs) => gs,
            None => return,
        };
        let pane_id = gs.pane_id;
        let mode = gs.mode;
        let input_text = gs.input.text.clone();
        let input_cursor = gs.input.cursor;
        let anchor_rect = gs.anchor_rect;
        let shell_busy = gs.shell_busy;

        let cwd = match self.panes.get(&pane_id) {
            Some(PaneKind::Terminal(p)) => p.cwd.clone(),
            _ => None,
        };
        if let Some(cwd) = cwd {
            let branches = tide_terminal::git::list_branches(&cwd);
            let worktrees = tide_terminal::git::list_worktrees(&cwd);
            let mut new_gs = GitSwitcherState::new(
                pane_id, mode, branches, worktrees, anchor_rect,
            );
            new_gs.shell_busy = shell_busy;
            new_gs.input.text = input_text;
            new_gs.input.cursor = input_cursor;
            // Re-filter with existing input
            if !new_gs.input.is_empty() {
                // Trigger filter by inserting+removing a char... or just call filter directly
                // We can't call private filter, so insert empty and backspace:
                // Actually, let's just re-build filter manually:
                let query_lower = new_gs.input.text.to_lowercase();
                new_gs.filtered_branches = new_gs.branches.iter().enumerate()
                    .filter(|(_, b)| b.name.to_lowercase().contains(&query_lower))
                    .map(|(i, _)| i)
                    .collect();
                new_gs.filtered_worktrees = new_gs.worktrees.iter().enumerate()
                    .filter(|(_, wt)| {
                        let branch_match = wt.branch.as_ref()
                            .map(|b| b.to_lowercase().contains(&query_lower))
                            .unwrap_or_else(|| "(detached)".contains(&query_lower));
                        let path_match = wt.path.to_string_lossy().to_lowercase().contains(&query_lower);
                        branch_match || path_match
                    })
                    .map(|(i, _)| i)
                    .collect();
            }
            // Clamp selected to valid range
            let len = new_gs.current_filtered_len();
            if new_gs.selected >= len && len > 0 {
                new_gs.selected = len - 1;
            }
            self.git_switcher = Some(new_gs);
        }
    }

    /// Handle branch cleanup bar button clicks.
    /// Returns true if the click was consumed.
    pub(crate) fn handle_branch_cleanup_click(&mut self, pos: tide_core::Vec2) -> bool {
        let bc_pane_id = match self.branch_cleanup {
            Some(ref bc) => bc.pane_id,
            None => return false,
        };
        let bar_rect = match self.notification_bar_rect(bc_pane_id) {
            Some(r) => r,
            None => return false,
        };
        if pos.y < bar_rect.y || pos.y > bar_rect.y + bar_rect.height
            || pos.x < bar_rect.x || pos.x > bar_rect.x + bar_rect.width
        {
            return false;
        }
        let cell_size = self.cell_size();
        let btn_pad = 8.0;

        // Cancel (rightmost)
        let cancel_w = 6.0 * cell_size.width + btn_pad * 2.0;
        let cancel_x = bar_rect.x + bar_rect.width - cancel_w - 4.0;

        // Keep
        let keep_w = 4.0 * cell_size.width + btn_pad * 2.0;
        let keep_x = cancel_x - keep_w - 4.0;

        // Delete
        let delete_w = 6.0 * cell_size.width + btn_pad * 2.0;
        let delete_x = keep_x - delete_w - 4.0;

        if pos.x >= cancel_x {
            self.cancel_branch_cleanup();
        } else if pos.x >= keep_x {
            self.confirm_branch_keep();
        } else if pos.x >= delete_x {
            self.confirm_branch_delete();
        }
        self.needs_redraw = true;
        true
    }

    /// Handle a completed drop operation (tree-to-tree only).
    pub(crate) fn handle_drop(&mut self, source: tide_core::PaneId, dest: DropDestination) {
        match dest {
            DropDestination::TreeRoot(zone) => {
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
            DropDestination::TreePane(target_id, zone) => {
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
    }
}
