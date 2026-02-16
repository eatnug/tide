// Layout computation and geometry utility methods extracted from main.rs

use tide_core::{LayoutEngine, PaneDecorations, Rect, Renderer, Size, SplitDirection};

use crate::drag_drop::HoverTarget;
use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui_state::LayoutSide;
use crate::{App, PaneAreaMode};

impl App {
    pub(crate) fn update_cursor_icon(&self) {
        use winit::window::CursorIcon;
        let icon = match &self.hover_target {
            Some(HoverTarget::FileTreeEntry(_))
            | Some(HoverTarget::PaneTabBar(_))
            | Some(HoverTarget::PaneTabClose(_))
            | Some(HoverTarget::PanelTab(_))
            | Some(HoverTarget::PanelTabClose(_))
            | Some(HoverTarget::StackedTab(_))
            | Some(HoverTarget::StackedTabClose(_))
            | Some(HoverTarget::EmptyPanelButton)
            | Some(HoverTarget::EmptyPanelOpenFile)
            | Some(HoverTarget::FileFinderItem(_)) => CursorIcon::Pointer,
            Some(HoverTarget::SidebarHandle)
            | Some(HoverTarget::DockHandle) => CursorIcon::Grab,
            Some(HoverTarget::FileTreeBorder) => CursorIcon::ColResize,
            Some(HoverTarget::PanelBorder) => CursorIcon::ColResize,
            Some(HoverTarget::SplitBorder(SplitDirection::Horizontal)) => CursorIcon::ColResize,
            Some(HoverTarget::SplitBorder(SplitDirection::Vertical)) => CursorIcon::RowResize,
            None => CursorIcon::Default,
        };
        if let Some(window) = &self.window {
            window.set_cursor(icon);
        }
    }

    /// Compute the geometry for buttons in the empty editor panel.
    /// Returns (new_file_rect, open_file_rect) or None if not applicable.
    pub(crate) fn empty_panel_button_rects(&self) -> Option<(Rect, Rect)> {
        if !self.active_editor_tabs().is_empty() || self.file_finder.is_some() {
            return None;
        }
        let panel_rect = self.editor_panel_rect?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let cell_height = cell_size.height;
        let label_y = panel_rect.y + panel_rect.height * 0.38;

        let new_btn_text = "New File";
        let new_hint_text = "  Cmd+Shift+E";
        let new_btn_w = (new_btn_text.len() + new_hint_text.len()) as f32 * cell_size.width + 24.0;
        let btn_h = cell_height + 12.0;
        let new_btn_x = panel_rect.x + (panel_rect.width - new_btn_w) / 2.0;
        let new_btn_y = label_y + cell_height + 16.0;
        let new_file_rect = Rect::new(new_btn_x, new_btn_y, new_btn_w, btn_h);

        let open_btn_text = "Open File";
        let open_hint_text = "  Cmd+O";
        let open_btn_w = (open_btn_text.len() + open_hint_text.len()) as f32 * cell_size.width + 24.0;
        let open_btn_x = panel_rect.x + (panel_rect.width - open_btn_w) / 2.0;
        let open_btn_y = new_btn_y + btn_h + 8.0;
        let open_file_rect = Rect::new(open_btn_x, open_btn_y, open_btn_w, btn_h);

        Some((new_file_rect, open_file_rect))
    }

    /// Check if a position is on the "New File" button in the empty editor panel.
    pub(crate) fn is_on_new_file_button(&self, pos: tide_core::Vec2) -> bool {
        self.empty_panel_button_rects()
            .is_some_and(|(new_rect, _)| new_rect.contains(pos))
    }

    /// Check if a position is on the "Open File" button in the empty editor panel.
    pub(crate) fn is_on_open_file_button(&self, pos: tide_core::Vec2) -> bool {
        self.empty_panel_button_rects()
            .is_some_and(|(_, open_rect)| open_rect.contains(pos))
    }

    /// Check if a position is on a file finder item. Returns the index into filtered list.
    pub(crate) fn file_finder_item_at(&self, pos: tide_core::Vec2) -> Option<usize> {
        let finder = self.file_finder.as_ref()?;
        let panel_rect = self.editor_panel_rect?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let line_height = cell_size.height * FILE_TREE_LINE_SPACING;

        // Search input area: top of panel
        let input_y = panel_rect.y + PANE_PADDING + 8.0;
        let input_h = cell_size.height + POPUP_INPUT_PADDING;
        let list_top = input_y + input_h + 8.0;

        if pos.y < list_top || pos.x < panel_rect.x || pos.x > panel_rect.x + panel_rect.width {
            return None;
        }

        let rel_y = pos.y - list_top;
        let idx = (rel_y / line_height) as usize + finder.scroll_offset;
        if idx < finder.filtered.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Hit-test the git switcher popup. Returns the filtered index of the item under pos.
    pub(crate) fn git_switcher_item_at(&self, pos: tide_core::Vec2) -> Option<usize> {
        let gs = self.git_switcher.as_ref()?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let logical = self.logical_size();
        let geo = gs.geometry(cell_size.height, logical.width, logical.height);

        if pos.x < geo.popup_x || pos.x > geo.popup_x + geo.popup_w || pos.y < geo.list_top {
            return None;
        }

        let rel_y = pos.y - geo.list_top;
        let vi = (rel_y / geo.line_height) as usize;
        // Don't select items beyond the visible rows (e.g. clicks in the button area below)
        if vi >= geo.max_visible {
            return None;
        }
        let idx = vi + gs.scroll_offset;
        if idx < gs.current_filtered_len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Check if a position is inside the git switcher popup area.
    pub(crate) fn git_switcher_contains(&self, pos: tide_core::Vec2) -> bool {
        if let Some(ref gs) = self.git_switcher {
            if let Some(cs) = self.renderer.as_ref().map(|r| r.cell_size()) {
                let logical = self.logical_size();
                let geo = gs.geometry(cs.height, logical.width, logical.height);
                let popup_rect = Rect::new(geo.popup_x, geo.popup_y, geo.popup_w, geo.popup_h);
                return popup_rect.contains(pos);
            }
        }
        false
    }

    /// Hit-test the git switcher popup for button clicks (both Branches and Worktrees tabs).
    pub(crate) fn git_switcher_button_at(&self, pos: tide_core::Vec2) -> Option<crate::SwitcherButton> {
        let gs = self.git_switcher.as_ref()?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let cell_height = cell_size.height;
        let logical = self.logical_size();
        let geo = gs.geometry(cell_height, logical.width, logical.height);

        // Check per-item buttons
        if pos.y < geo.list_top {
            return None;
        }
        let rel_y = pos.y - geo.list_top;
        let vi = (rel_y / geo.line_height) as usize;
        if vi >= geo.max_visible {
            return None;
        }
        let fi = gs.scroll_offset + vi;
        if fi >= gs.current_filtered_len() {
            return None;
        }

        let y = geo.list_top + vi as f32 * geo.line_height;
        let btn_h = cell_height + 2.0;
        let btn_y = y + (geo.line_height - btn_h) / 2.0;
        if pos.y < btn_y || pos.y > btn_y + btn_h {
            return None;
        }

        let busy = gs.shell_busy;

        // Create row: only Switch and NewPane buttons (no create when busy)
        if gs.is_create_row(fi) {
            if busy { return None; }
            let mut btn_right = geo.popup_x + geo.popup_w - 8.0;

            let pane_label = "Pane";
            let pane_w = pane_label.len() as f32 * cell_size.width + 10.0;
            let pane_x = btn_right - pane_w;
            if pos.x >= pane_x && pos.x <= pane_x + pane_w {
                return Some(crate::SwitcherButton::NewPane(fi));
            }
            btn_right = pane_x - 3.0;

            let switch_label = "Switch";
            let switch_w = switch_label.len() as f32 * cell_size.width + 10.0;
            let switch_x = btn_right - switch_w;
            if pos.x >= switch_x && pos.x <= switch_x + switch_w {
                return Some(crate::SwitcherButton::Switch(fi));
            }
            return None;
        }

        match gs.mode {
            crate::GitSwitcherMode::Branches => {
                let entry_idx = gs.filtered_branches[fi];
                let branch = &gs.branches[entry_idx];
                if branch.is_current {
                    return None;
                }

                let has_wt = gs.worktree_branch_names.contains(&branch.name);
                let mut btn_right = geo.popup_x + geo.popup_w - 8.0;

                // Delete button (×) — only for branches without worktree, hidden when busy
                if !has_wt && !busy {
                    let del_w = cell_size.width + 8.0;
                    let del_x = btn_right - del_w;
                    if pos.x >= del_x && pos.x <= del_x + del_w {
                        return Some(crate::SwitcherButton::Delete(fi));
                    }
                    btn_right = del_x - 3.0;
                }

                // [Pane] button
                let pane_label = "Pane";
                let pane_w = pane_label.len() as f32 * cell_size.width + 10.0;
                let pane_x = btn_right - pane_w;
                if pos.x >= pane_x && pos.x <= pane_x + pane_w {
                    return Some(crate::SwitcherButton::NewPane(fi));
                }

                if !busy {
                    btn_right = pane_x - 3.0;

                    // [Switch] button — hidden when busy
                    let switch_label = "Switch";
                    let switch_w = switch_label.len() as f32 * cell_size.width + 10.0;
                    let switch_x = btn_right - switch_w;
                    if pos.x >= switch_x && pos.x <= switch_x + switch_w {
                        return Some(crate::SwitcherButton::Switch(fi));
                    }
                }
            }
            crate::GitSwitcherMode::Worktrees => {
                let entry_idx = gs.filtered_worktrees[fi];
                let wt = &gs.worktrees[entry_idx];
                if wt.is_current {
                    return None;
                }

                let mut btn_right = geo.popup_x + geo.popup_w - 8.0;

                // Delete button (×) — not for main, hidden when busy
                if !wt.is_main && !busy {
                    let del_w = cell_size.width + 8.0;
                    let del_x = btn_right - del_w;
                    if pos.x >= del_x && pos.x <= del_x + del_w {
                        return Some(crate::SwitcherButton::Delete(fi));
                    }
                    btn_right = del_x - 3.0;
                }

                // [Pane] button
                let pane_label = "Pane";
                let pane_w = pane_label.len() as f32 * cell_size.width + 10.0;
                let pane_x = btn_right - pane_w;
                if pos.x >= pane_x && pos.x <= pane_x + pane_w {
                    return Some(crate::SwitcherButton::NewPane(fi));
                }

                if !busy {
                    btn_right = pane_x - 3.0;

                    // [Switch] button — hidden when busy
                    let switch_label = "Switch";
                    let switch_w = switch_label.len() as f32 * cell_size.width + 10.0;
                    let switch_x = btn_right - switch_w;
                    if pos.x >= switch_x && pos.x <= switch_x + switch_w {
                        return Some(crate::SwitcherButton::Switch(fi));
                    }
                }
            }
        }

        None
    }

    /// Hit-test the file switcher popup. Returns the filtered index of the item under pos.
    pub(crate) fn file_switcher_item_at(&self, pos: tide_core::Vec2) -> Option<usize> {
        let fs = self.file_switcher.as_ref()?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let geo = fs.geometry(cell_size.height);

        if pos.x < geo.popup_x || pos.x > geo.popup_x + geo.popup_w || pos.y < geo.list_top {
            return None;
        }

        let rel_y = pos.y - geo.list_top;
        let idx = (rel_y / geo.line_height) as usize + fs.scroll_offset;
        if idx < fs.filtered.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Compute the bounding rect of the active panel tab (for anchoring popups).
    pub(crate) fn active_panel_tab_rect(&self) -> Option<Rect> {
        let panel_rect = self.editor_panel_rect?;
        let active_id = self.active_editor_tab()?;
        let index = self.active_editor_tabs().iter().position(|&id| id == active_id)?;
        let tab_bar_top = panel_rect.y + PANE_PADDING;
        let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
        let tx = tab_start_x + index as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
        Some(Rect::new(tx, tab_bar_top, PANEL_TAB_WIDTH, PANEL_TAB_HEIGHT))
    }

    /// Check if a position is inside the file finder area (covers the whole editor panel).
    pub(crate) fn file_finder_contains(&self, pos: tide_core::Vec2) -> bool {
        if self.file_finder.is_some() {
            if let Some(panel_rect) = self.editor_panel_rect {
                return panel_rect.contains(pos);
            }
        }
        false
    }

    /// Check if a position is inside the save-as popup area.
    pub(crate) fn save_as_contains(&self, pos: tide_core::Vec2) -> bool {
        if let Some(ref save_as) = self.save_as_input {
            if let (Some(panel_rect), Some(renderer)) = (self.editor_panel_rect, self.renderer.as_ref()) {
                let cell_size = renderer.cell_size();
                let cell_height = cell_size.height;
                let field_h = cell_height + POPUP_INPUT_PADDING;
                let hint_h = cell_height + 8.0;
                let padding = POPUP_TEXT_INSET;
                let popup_w = SAVE_AS_POPUP_W.min(panel_rect.width - 2.0 * PANE_PADDING);
                let popup_h = field_h * 2.0 + POPUP_SEPARATOR + hint_h + 2.0 * padding;
                let popup_x = save_as.anchor_rect.x.clamp(
                    panel_rect.x + PANE_PADDING,
                    panel_rect.x + panel_rect.width - popup_w - PANE_PADDING,
                );
                let popup_y = save_as.anchor_rect.y + save_as.anchor_rect.height + 4.0;
                let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);
                return popup_rect.contains(pos);
            }
        }
        false
    }

    /// Check if a position is inside the file switcher popup area.
    pub(crate) fn file_switcher_contains(&self, pos: tide_core::Vec2) -> bool {
        if let Some(ref fs) = self.file_switcher {
            if let Some(cs) = self.renderer.as_ref().map(|r| r.cell_size()) {
                let geo = fs.geometry(cs.height);
                let popup_rect = Rect::new(geo.popup_x, geo.popup_y, geo.popup_w, geo.popup_h);
                return popup_rect.contains(pos);
            }
        }
        false
    }

    /// Hit-test the context menu. Returns the item index (0=Rename, 1=Delete).
    pub(crate) fn context_menu_item_at(&self, pos: tide_core::Vec2) -> Option<usize> {
        let menu = self.context_menu.as_ref()?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let logical = self.logical_size();
        let rect = menu.geometry(cell_size.height, logical.width, logical.height);

        if !rect.contains(pos) {
            return None;
        }

        let line_height = cell_size.height + POPUP_LINE_EXTRA;
        let rel_y = pos.y - rect.y - 4.0; // 4.0 = top padding
        let idx = (rel_y / line_height) as usize;
        if idx < crate::ContextMenuAction::ALL.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Check if a position is inside the context menu popup area.
    pub(crate) fn context_menu_contains(&self, pos: tide_core::Vec2) -> bool {
        if let Some(ref menu) = self.context_menu {
            if let Some(cs) = self.renderer.as_ref().map(|r| r.cell_size()) {
                let logical = self.logical_size();
                let rect = menu.geometry(cs.height, logical.width, logical.height);
                return rect.contains(pos);
            }
        }
        false
    }

    pub(crate) fn palette(&self) -> &'static ThemePalette {
        if self.dark_mode { &DARK } else { &LIGHT }
    }

    /// Compute the ideal editor panel width based on the number of terminal columns.
    /// 1 pane → half of available width, 2+ panes → one third, clamped to min 150.
    pub(crate) fn auto_editor_panel_width(&self) -> f32 {
        let logical = self.logical_size();
        let sidebar_reserved = if self.show_file_tree { self.file_tree_width } else { 0.0 };
        let available = (logical.width - sidebar_reserved).max(0.0);
        let pane_count = self.layout.pane_ids().len();
        let width = if pane_count <= 1 {
            available / 2.0
        } else {
            available / 3.0
        };
        width.max(150.0)
    }

    pub(crate) fn compute_layout(&mut self) {
        let logical = self.logical_size();
        let top = self.top_inset;
        let pane_ids = self.layout.pane_ids();

        let show_editor_panel = self.show_editor_panel;
        let show_file_tree = self.show_file_tree;

        // Compute how much space is reserved on each side.
        // Sidebar (file tree) and dock (editor panel) can each be on Left or Right.
        // Clamp widths so their total never exceeds the window (leave at least 100px for terminal).
        let max_panels = (logical.width - 100.0).max(0.0);
        let total = (if show_file_tree { self.file_tree_width } else { 0.0 })
            + (if show_editor_panel { self.editor_panel_width } else { 0.0 });
        if total > max_panels && total > 0.0 {
            let scale = max_panels / total;
            if show_file_tree { self.file_tree_width *= scale; }
            if show_editor_panel { self.editor_panel_width *= scale; }
        }
        let sidebar_width = if show_file_tree { self.file_tree_width } else { 0.0 };
        let dock_width = if show_editor_panel { self.editor_panel_width } else { 0.0 };

        let mut left_reserved = 0.0_f32;
        let mut right_reserved = 0.0_f32;

        if show_file_tree {
            match self.sidebar_side {
                LayoutSide::Left => left_reserved += sidebar_width,
                LayoutSide::Right => right_reserved += sidebar_width,
            }
        }
        if show_editor_panel {
            match self.dock_side {
                LayoutSide::Left => left_reserved += dock_width,
                LayoutSide::Right => right_reserved += dock_width,
            }
        }

        // When editor panel is maximized, it fills the full area (excluding file tree on its side)
        if self.editor_panel_maximized && show_editor_panel {
            let ft_reserved = if show_file_tree { sidebar_width } else { 0.0 };
            let ft_on_left = show_file_tree && self.sidebar_side == LayoutSide::Left;
            let panel_x = if ft_on_left { ft_reserved } else { 0.0 };
            let panel_w = (logical.width - ft_reserved).max(100.0);
            self.editor_panel_rect = Some(Rect::new(panel_x, top, panel_w, logical.height - top));
            // File tree rect (still visible during panel maximize)
            if show_file_tree {
                let ft_x = if ft_on_left { 0.0 } else { logical.width - sidebar_width };
                self.file_tree_rect = Some(Rect::new(ft_x, top, sidebar_width - PANE_GAP, logical.height - top));
            } else {
                self.file_tree_rect = None;
            }
            self.pane_area_rect = None;
            self.pane_rects = Vec::new();
            self.visual_pane_rects = Vec::new();
            self.layout_generation += 1;
            self.pane_generations.clear();
            self.chrome_generation += 1;
            self.layout.last_window_size = Some(Size::new(0.0, logical.height - top));
            return;
        }

        let terminal_area = Size::new(
            (logical.width - left_reserved - right_reserved).max(100.0),
            logical.height - top,
        );

        let terminal_offset_x = left_reserved;

        // Compute file_tree_rect and editor_panel_rect based on sides.
        // Rule: sidebar (file tree) is always outermost when both are on the same side.
        //
        // Layout pattern per component (width includes gap budget):
        //   Left side:  outer x=0, inner x=outer_w       (gap is at right end of each rect)
        //   Right side: inner x=W-reserved+GAP, outer x=W-outer_w+GAP  (gap is at left end)
        // Rect width is always component_width - PANE_GAP.
        let both_on_same_side = show_file_tree && show_editor_panel && self.sidebar_side == self.dock_side;

        if show_file_tree {
            let sidebar_x = match self.sidebar_side {
                LayoutSide::Left => 0.0, // always outer
                LayoutSide::Right => {
                    // always outer (at window edge)
                    logical.width - sidebar_width + PANE_GAP
                }
            };
            self.file_tree_rect = Some(Rect::new(
                sidebar_x,
                top,
                sidebar_width - PANE_GAP,
                logical.height - top,
            ));
        } else {
            self.file_tree_rect = None;
        }

        if show_editor_panel {
            let dock_x = match self.dock_side {
                LayoutSide::Left => {
                    if both_on_same_side {
                        sidebar_width // inner: after sidebar
                    } else {
                        0.0 // alone on left
                    }
                }
                LayoutSide::Right => {
                    if both_on_same_side {
                        // inner (closer to terminal)
                        logical.width - right_reserved + PANE_GAP
                    } else {
                        // alone on right (at window edge)
                        logical.width - dock_width + PANE_GAP
                    }
                }
            };
            self.editor_panel_rect = Some(Rect::new(
                dock_x,
                top,
                dock_width - PANE_GAP,
                logical.height - top,
            ));
        } else {
            self.editor_panel_rect = None;
        }

        // Store the pane area rect for root-level drop zone detection
        self.pane_area_rect = Some(Rect::new(terminal_offset_x, top, terminal_area.width, terminal_area.height));

        // Snap ratios to cell boundaries, then recompute with snapped ratios.
        // Skip during active border drags to prevent cumulative drift.
        let is_dragging = self.router.is_dragging_border()
            || self.panel_border_dragging
            || self.file_tree_border_dragging;
        if !is_dragging {
            if let Some(renderer) = &self.renderer {
                let cell_size = renderer.cell_size();
                let decorations = PaneDecorations {
                    gap: PANE_GAP,
                    padding: PANE_PADDING,
                    tab_bar_height: TAB_BAR_HEIGHT,
                };
                self.layout
                    .snap_ratios_to_cells(terminal_area, cell_size, &decorations);
            }
        }

        let mut rects = self.layout.compute(terminal_area, &pane_ids, self.focused);

        // Offset rects to account for file tree panel and titlebar inset
        for (_, rect) in &mut rects {
            rect.x += terminal_offset_x;
            rect.y += top;
        }

        // Stacked mode: single pane fills the terminal area.
        // Safety net: if the stacked pane was removed (e.g. via drag-drop), fall back to Split.
        // The primary close-path handling is in pane_lifecycle.rs.
        if let PaneAreaMode::Stacked(active_id) = self.pane_area_mode {
            if rects.iter().any(|(id, _)| *id == active_id) {
                let full_rect = Rect::new(terminal_offset_x, top, terminal_area.width, terminal_area.height);
                rects = vec![(active_id, full_rect)];
            } else {
                self.pane_area_mode = PaneAreaMode::Split;
            }
        }

        // Force grid rebuild if rects changed
        let rects_changed = rects != self.pane_rects;
        self.pane_rects = rects;

        // Compute visual rects: window edges flush (0px), internal edges share gap
        let logical = self.logical_size();
        let right_edge = terminal_offset_x + terminal_area.width;
        let half = PANE_GAP / 2.0;
        self.visual_pane_rects = self
            .pane_rects
            .iter()
            .map(|&(id, r)| {
                // Window boundary → 0px inset (flush), internal edge → half border width
                let inset_left = if r.x <= terminal_offset_x + 0.5 { 0.0 } else { half };
                let inset_top = if r.y <= top + 0.5 { 0.0 } else { half };
                let inset_right = if r.x + r.width >= right_edge - 0.5 {
                    0.0
                } else {
                    half
                };
                let inset_bottom = if r.y + r.height >= logical.height - 0.5 {
                    0.0
                } else {
                    half
                };
                let vr = Rect::new(
                    r.x + inset_left,
                    r.y + inset_top,
                    (r.width - inset_left - inset_right).max(1.0),
                    (r.height - inset_top - inset_bottom).max(1.0),
                );
                (id, vr)
            })
            .collect();

        // Resize terminal backends to match the actual visible content area.
        // Uses visual rects + PANE_PADDING to match the render inner rect exactly.
        // During border drag or window resize, skip PTY resize to avoid SIGWINCH spam.
        let is_dragging = self.router.is_dragging_border()
            || self.panel_border_dragging
            || self.file_tree_border_dragging
            || self.resize_deferred_at.is_some();
        if !is_dragging {
            let content_top = self.pane_area_mode.content_top();
            if let Some(renderer) = &self.renderer {
                let cell_size = renderer.cell_size();
                for &(id, vr) in &self.visual_pane_rects {
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&id) {
                        let content_rect = Rect::new(
                            vr.x + PANE_PADDING,
                            vr.y + content_top,
                            (vr.width - 2.0 * PANE_PADDING).max(cell_size.width),
                            (vr.height - content_top - PANE_PADDING).max(cell_size.height),
                        );
                        pane.resize_to_rect(content_rect, cell_size);
                    }
                }
            }
        }

        if rects_changed {
            self.layout_generation += 1;
            self.pane_generations.clear();
            self.chrome_generation += 1;
        }

        // Clamp panel tab scroll after layout change (container may have grown)
        self.clamp_panel_tab_scroll();

        // Store window size for layout drag operations
        self.layout.last_window_size = Some(terminal_area);
    }
}
