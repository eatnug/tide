// Layout computation and geometry utility methods extracted from main.rs

use tide_core::{LayoutEngine, PaneDecorations, Rect, Renderer, Size, SplitDirection};

use crate::drag_drop::HoverTarget;
use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui_state::LayoutSide;
use crate::App;

impl App {
    pub(crate) fn update_cursor_icon(&self) {
        use winit::window::CursorIcon;
        let icon = match &self.hover_target {
            Some(HoverTarget::FileTreeEntry(_))
            | Some(HoverTarget::PaneTabBar(_))
            | Some(HoverTarget::PaneTabClose(_))
            | Some(HoverTarget::PanelTab(_))
            | Some(HoverTarget::PanelTabClose(_))
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
        if !self.editor_panel_tabs.is_empty() || self.file_finder.is_some() {
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
        let input_h = cell_size.height + 12.0;
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
        let line_height = cell_size.height + 4.0;
        let tab_h = cell_size.height + 8.0;

        let logical = self.logical_size();
        let popup_w = 320.0_f32;
        let popup_x = gs.anchor_rect.x.min(logical.width - popup_w - 4.0).max(0.0);
        let popup_y = gs.anchor_rect.y + gs.anchor_rect.height + 4.0;
        let input_h = cell_size.height + 10.0;
        let list_top = popup_y + input_h + tab_h;

        if pos.x < popup_x || pos.x > popup_x + popup_w || pos.y < list_top {
            return None;
        }

        let rel_y = pos.y - list_top;
        let idx = (rel_y / line_height) as usize + gs.scroll_offset;
        if idx < gs.current_filtered_len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Check if a position is inside the git switcher popup area.
    pub(crate) fn git_switcher_contains(&self, pos: tide_core::Vec2) -> bool {
        if let Some(ref gs) = self.git_switcher {
            let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
            if let Some(cs) = cell_size {
                let logical = self.logical_size();
                let popup_w = 320.0_f32;
                let popup_x = gs.anchor_rect.x.min(logical.width - popup_w - 4.0).max(0.0);
                let popup_y = gs.anchor_rect.y + gs.anchor_rect.height + 4.0;
                let line_height = cs.height + 4.0;
                let tab_h = cs.height + 8.0;
                let input_h = cs.height + 10.0;
                let current_len = gs.current_filtered_len();
                let max_visible = 10.min(current_len);
                let has_new_wt_btn = gs.mode == crate::GitSwitcherMode::Worktrees;
                let new_wt_btn_h = if has_new_wt_btn { line_height + 4.0 } else { 0.0 };
                let popup_h = input_h + tab_h + max_visible as f32 * line_height + new_wt_btn_h + 12.0;
                let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);
                return popup_rect.contains(pos);
            }
        }
        false
    }

    /// Hit-test the git switcher popup for button clicks (worktree mode only).
    /// Returns (filtered_index, button_type).
    pub(crate) fn git_switcher_button_at(&self, pos: tide_core::Vec2) -> Option<crate::WorktreeButton> {
        let gs = self.git_switcher.as_ref()?;
        if gs.mode != crate::GitSwitcherMode::Worktrees {
            return None;
        }
        let cell_size = self.renderer.as_ref()?.cell_size();
        let cell_height = cell_size.height;
        let line_height = cell_height + 4.0;
        let tab_h = cell_height + 8.0;

        let logical = self.logical_size();
        let popup_w = 320.0_f32;
        let popup_x = gs.anchor_rect.x.min(logical.width - popup_w - 4.0).max(0.0);
        let popup_y = gs.anchor_rect.y + gs.anchor_rect.height + 4.0;
        let input_h = cell_height + 10.0;
        let list_top = popup_y + input_h + tab_h;
        let max_visible = 10.min(gs.filtered_worktrees.len());

        // Check [+ New Worktree] button
        let new_wt_y = list_top + max_visible as f32 * line_height + 4.0;
        let new_wt_label = "+ New Worktree";
        let new_wt_w = new_wt_label.len() as f32 * cell_size.width + 16.0;
        let new_wt_x = popup_x + (popup_w - new_wt_w) / 2.0;
        let new_wt_h = cell_height + 4.0;
        if pos.x >= new_wt_x && pos.x <= new_wt_x + new_wt_w
            && pos.y >= new_wt_y && pos.y <= new_wt_y + new_wt_h
        {
            return Some(crate::WorktreeButton::NewWorktree);
        }

        // Check per-item buttons
        if pos.y < list_top {
            return None;
        }
        let rel_y = pos.y - list_top;
        let vi = (rel_y / line_height) as usize;
        if vi >= max_visible {
            return None;
        }
        let fi = gs.scroll_offset + vi;
        if fi >= gs.filtered_worktrees.len() {
            return None;
        }
        let entry_idx = gs.filtered_worktrees[fi];
        let wt = &gs.worktrees[entry_idx];
        if wt.is_current {
            return None;
        }

        let y = list_top + vi as f32 * line_height;
        let btn_h = cell_height + 2.0;
        let btn_y = y + (line_height - btn_h) / 2.0;
        if pos.y < btn_y || pos.y > btn_y + btn_h {
            return None;
        }

        let mut btn_right = popup_x + popup_w - 8.0;

        // Delete button (×)
        if !wt.is_main {
            let del_w = cell_size.width + 8.0;
            let del_x = btn_right - del_w;
            if pos.x >= del_x && pos.x <= del_x + del_w {
                return Some(crate::WorktreeButton::Delete(fi));
            }
            btn_right = del_x - 3.0;
        }

        // [New Pane] button
        let pane_label = "Pane";
        let pane_w = pane_label.len() as f32 * cell_size.width + 10.0;
        let pane_x = btn_right - pane_w;
        if pos.x >= pane_x && pos.x <= pane_x + pane_w {
            return Some(crate::WorktreeButton::NewPane(fi));
        }
        btn_right = pane_x - 3.0;

        // [Switch] button
        let switch_label = "Switch";
        let switch_w = switch_label.len() as f32 * cell_size.width + 10.0;
        let switch_x = btn_right - switch_w;
        if pos.x >= switch_x && pos.x <= switch_x + switch_w {
            return Some(crate::WorktreeButton::Switch(fi));
        }

        None
    }

    /// Hit-test the file switcher popup. Returns the filtered index of the item under pos.
    pub(crate) fn file_switcher_item_at(&self, pos: tide_core::Vec2) -> Option<usize> {
        let fs = self.file_switcher.as_ref()?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let line_height = cell_size.height + 4.0;

        let popup_w = 260.0_f32;
        let popup_x = fs.anchor_rect.x;
        let popup_y = fs.anchor_rect.y + fs.anchor_rect.height + 4.0;
        let input_h = cell_size.height + 10.0;
        let list_top = popup_y + input_h;

        if pos.x < popup_x || pos.x > popup_x + popup_w || pos.y < list_top {
            return None;
        }

        let rel_y = pos.y - list_top;
        let idx = (rel_y / line_height) as usize + fs.scroll_offset;
        if idx < fs.filtered.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Check if a position is inside the file switcher popup area.
    pub(crate) fn file_switcher_contains(&self, pos: tide_core::Vec2) -> bool {
        if let Some(ref fs) = self.file_switcher {
            let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
            if let Some(cs) = cell_size {
                let popup_w = 260.0_f32;
                let popup_x = fs.anchor_rect.x;
                let popup_y = fs.anchor_rect.y + fs.anchor_rect.height + 4.0;
                let line_height = cs.height + 4.0;
                let input_h = cs.height + 10.0;
                let max_visible = 10.min(fs.filtered.len());
                let popup_h = input_h + max_visible as f32 * line_height + 8.0;
                let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);
                return popup_rect.contains(pos);
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
            self.editor_panel_rect = Some(Rect::new(panel_x, 0.0, panel_w, logical.height));
            // File tree rect (still visible during panel maximize)
            if show_file_tree {
                let ft_x = if ft_on_left { 0.0 } else { logical.width - sidebar_width };
                self.file_tree_rect = Some(Rect::new(ft_x, 0.0, sidebar_width - PANE_GAP, logical.height));
            } else {
                self.file_tree_rect = None;
            }
            self.pane_area_rect = None;
            self.pane_rects = Vec::new();
            self.visual_pane_rects = Vec::new();
            self.layout_generation += 1;
            self.pane_generations.clear();
            self.chrome_generation += 1;
            self.layout.last_window_size = Some(Size::new(0.0, logical.height));
            return;
        }

        let terminal_area = Size::new(
            (logical.width - left_reserved - right_reserved).max(100.0),
            logical.height,
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
                0.0,
                sidebar_width - PANE_GAP,
                logical.height,
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
                0.0,
                dock_width - PANE_GAP,
                logical.height,
            ));
        } else {
            self.editor_panel_rect = None;
        }

        // Store the pane area rect for root-level drop zone detection
        self.pane_area_rect = Some(Rect::new(terminal_offset_x, 0.0, terminal_area.width, terminal_area.height));

        // First compute to establish initial rects
        let _initial_rects = self.layout.compute(terminal_area, &pane_ids, self.focused);

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

        // Offset rects to account for file tree panel
        for (_, rect) in &mut rects {
            rect.x += terminal_offset_x;
        }

        // If a pane is maximized, override rects to show only that pane filling the terminal area
        if let Some(max_id) = self.maximized_pane {
            if rects.iter().any(|(id, _)| *id == max_id) {
                let full_rect = Rect::new(terminal_offset_x, 0.0, terminal_area.width, terminal_area.height);
                rects = vec![(max_id, full_rect)];
            } else {
                // Maximized pane no longer exists in layout — clear maximize
                self.maximized_pane = None;
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
                let left = if r.x <= terminal_offset_x + 0.5 { 0.0 } else { half };
                let top = if r.y <= 0.5 { 0.0 } else { half };
                let right = if r.x + r.width >= right_edge - 0.5 {
                    0.0
                } else {
                    half
                };
                let bottom = if r.y + r.height >= logical.height - 0.5 {
                    0.0
                } else {
                    half
                };
                let vr = Rect::new(
                    r.x + left,
                    r.y + top,
                    (r.width - left - right).max(1.0),
                    (r.height - top - bottom).max(1.0),
                );
                (id, vr)
            })
            .collect();

        // Resize terminal backends to match the actual visible content area.
        // Uses visual rects + PANE_PADDING to match the render inner rect exactly.
        // During border drag, skip PTY resize to avoid SIGWINCH spam.
        let is_dragging = self.router.is_dragging_border()
            || self.panel_border_dragging
            || self.file_tree_border_dragging;
        if !is_dragging {
            if let Some(renderer) = &self.renderer {
                let cell_size = renderer.cell_size();
                for &(id, vr) in &self.visual_pane_rects {
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&id) {
                        let content_rect = Rect::new(
                            vr.x + PANE_PADDING,
                            vr.y + TAB_BAR_HEIGHT,
                            (vr.width - 2.0 * PANE_PADDING).max(cell_size.width),
                            (vr.height - TAB_BAR_HEIGHT - PANE_PADDING).max(cell_size.height),
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
