use tide_core::{DropZone, PaneId, Rect, Renderer, SplitDirection, Vec2};

use crate::theme::*;
use crate::ui::{dock_tab_x, dock_tabs_total_width, panel_tab_title, stacked_tab_width};
use crate::{App, PaneAreaMode};

/// Threshold for outer zone detection (0–12% of pane extent).
const OUTER_ZONE_THRESHOLD: f32 = 0.12;

// ──────────────────────────────────────────────
// Hover target: tracks which interactive element the mouse is over
// ──────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub(crate) enum HoverTarget {
    FileTreeEntry(usize),
    FileTreeBorder,
    SplitBorder(SplitDirection),
    PaneTabBar(PaneId),
    PaneTabClose(PaneId),
    PanelTab(PaneId),
    PanelTabClose(PaneId),
    StackedTab(PaneId),
    StackedTabClose(PaneId),
    PanelBorder,
    EmptyPanelButton,
    EmptyPanelOpenFile,
    FileFinderItem(usize),
    SidebarHandle,
    DockHandle,
    TitlebarSwap,
    TitlebarSettings,
    TitlebarTheme,
    TitlebarFileTree,
    TitlebarPaneArea,
    TitlebarDock,
    PaneModeToggle,
    PaneMaximize(PaneId),
    PaneAreaMaximize,
    DockMaximize,
    DockPreviewToggle,
    BrowserBack,
    BrowserForward,
    BrowserRefresh,
    BrowserUrlBar,
}

// ──────────────────────────────────────────────
// Drop destination: tree pane or editor panel
// ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum DropDestination {
    TreePane(PaneId, DropZone),
    TreeRoot(DropZone),
}

// ──────────────────────────────────────────────
// Pane drag & drop state machine
// ──────────────────────────────────────────────

pub(crate) enum PaneDragState {
    Idle,
    PendingDrag {
        source_pane: PaneId,
        press_pos: Vec2,
    },
    Dragging {
        source_pane: PaneId,
        drop_target: Option<DropDestination>,
    },
}

impl App {
    /// Hit-test whether the position is on a stacked-mode inline tab.
    /// Returns the PaneId of the tab. Tabs are variable-width inline text.
    pub(crate) fn stacked_tab_at(&self, pos: Vec2) -> Option<PaneId> {
        if !matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
            return None;
        }
        let &(_, rect) = self.visual_pane_rects.first()?;
        let header_top = rect.y;
        if pos.y < header_top || pos.y > header_top + TAB_BAR_HEIGHT {
            return None;
        }
        let cell_w = self.renderer.as_ref()?.cell_size().width;
        let pane_ids = self.layout.pane_ids();
        let mut tx = rect.x + PANE_PADDING;
        for &tab_id in pane_ids.iter() {
            let title = crate::ui::pane_title(&self.panes, tab_id);
            let tab_w = stacked_tab_width(&title, cell_w);
            if pos.x >= tx && pos.x <= tx + tab_w {
                return Some(tab_id);
            }
            tx += tab_w;
        }
        None
    }

    /// Hit-test stacked-mode close button (single button on header right).
    /// Returns the active pane id since it's a single close button for the header.
    pub(crate) fn stacked_tab_close_at(&self, pos: Vec2) -> Option<PaneId> {
        if let PaneAreaMode::Stacked(active) = self.pane_area_mode {
            let &(_, rect) = self.visual_pane_rects.first()?;
            let cell_size = self.renderer.as_ref()?.cell_size();
            let header_top = rect.y;
            if pos.y < header_top || pos.y > header_top + TAB_BAR_HEIGHT {
                return None;
            }
            let content_right = rect.x + rect.width - PANE_PADDING;
            let close_w = cell_size.width + BADGE_PADDING_H * 2.0;
            let close_x = content_right - close_w;
            if pos.x >= close_x && pos.x <= close_x + close_w {
                return Some(active);
            }
        }
        None
    }

    /// Hit-test whether the position is within a pane's tab bar area (split tree panes).
    /// Returns None in stacked mode (stacked has its own tab bar).
    pub(crate) fn pane_at_tab_bar(&self, pos: Vec2) -> Option<PaneId> {
        if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
            return None;
        }
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if tab_rect.contains(pos) {
                return Some(id);
            }
        }
        None
    }

    /// Hit-test whether the position is on a pane tab bar close button.
    /// Returns None in stacked mode (stacked has its own close buttons).
    pub(crate) fn pane_tab_close_at(&self, pos: Vec2) -> Option<PaneId> {
        if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
            return None;
        }
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if !tab_rect.contains(pos) {
                continue;
            }
            // Close badge is the rightmost badge, grid-aligned
            let cell_w = self.renderer.as_ref().map(|r| r.cell_size().width).unwrap_or(8.0);
            let grid_cols = ((rect.width - 2.0 * PANE_PADDING) / cell_w).floor();
            let grid_right = rect.x + PANE_PADDING + grid_cols * cell_w;
            let close_w = cell_w + BADGE_PADDING_H * 2.0;
            let close_x = grid_right - close_w;
            let close_y = rect.y + (TAB_BAR_HEIGHT - PANE_CLOSE_SIZE) / 2.0;
            if pos.x >= close_x
                && pos.x <= close_x + close_w
                && pos.y >= close_y
                && pos.y <= close_y + PANE_CLOSE_SIZE
            {
                return Some(id);
            }
        }
        None
    }

    /// Hit-test whether the position is on a pane header maximize button.
    /// Returns None in stacked mode.
    pub(crate) fn pane_maximize_at(&self, pos: Vec2) -> Option<PaneId> {
        if matches!(self.pane_area_mode, PaneAreaMode::Stacked(_)) {
            return None;
        }
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if !tab_rect.contains(pos) {
                continue;
            }
            let cell_w = self.renderer.as_ref().map(|r| r.cell_size().width).unwrap_or(8.0);
            let grid_cols = ((rect.width - 2.0 * PANE_PADDING) / cell_w).floor();
            let grid_right = rect.x + PANE_PADDING + grid_cols * cell_w;
            let close_w = cell_w + BADGE_PADDING_H * 2.0;
            let close_x = grid_right - close_w;
            let max_w = cell_w + BADGE_PADDING_H * 2.0;
            let max_x = close_x - BADGE_GAP - max_w;
            let max_y = rect.y;
            if pos.x >= max_x && pos.x <= max_x + max_w
                && pos.y >= max_y && pos.y <= max_y + TAB_BAR_HEIGHT
            {
                return Some(id);
            }
        }
        None
    }

    /// Hit-test whether the position is on a panel tab (variable-width dock tabs).
    pub(crate) fn panel_tab_at(&self, pos: Vec2) -> Option<PaneId> {
        let panel_rect = self.editor_panel_rect.as_ref()?;
        let tab_bar_top = panel_rect.y + PANE_CORNER_RADIUS;
        if pos.y < tab_bar_top || pos.y > tab_bar_top + PANEL_TAB_HEIGHT {
            return None;
        }
        let cell_w = self.renderer.as_ref()?.cell_size().width;
        let tabs = self.active_editor_tabs();
        let mut tx = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
        for &tab_id in tabs.iter() {
            let title = panel_tab_title(&self.panes, tab_id);
            let tab_w = stacked_tab_width(&title, cell_w);
            if pos.x >= tx && pos.x <= tx + tab_w {
                if pos.x >= panel_rect.x && pos.x <= panel_rect.x + panel_rect.width {
                    return Some(tab_id);
                }
            }
            tx += tab_w;
        }
        None
    }

    /// Hit-test the close button in the dock header (far right, closes active tab).
    pub(crate) fn panel_tab_close_at(&self, pos: Vec2) -> Option<PaneId> {
        let panel_rect = self.editor_panel_rect.as_ref()?;
        let active = self.active_editor_tab()?;
        let renderer = self.renderer.as_ref()?;
        let cell_w = renderer.cell_size().width;
        let tab_bar_top = panel_rect.y + PANE_CORNER_RADIUS;

        // Close button is at the far right of the dock header (matching stacked mode)
        let close_w = cell_w + BADGE_PADDING_H * 2.0;
        let close_x = panel_rect.x + panel_rect.width - PANE_PADDING - close_w;
        let close_y = tab_bar_top;
        if pos.x >= close_x && pos.x <= close_x + close_w
            && pos.y >= close_y && pos.y <= close_y + PANEL_TAB_HEIGHT
        {
            return Some(active);
        }
        None
    }

    /// Check if the cursor is in the panel tab bar area.
    pub(crate) fn is_over_panel_tab_bar(&self, pos: Vec2) -> bool {
        if let Some(ref panel_rect) = self.editor_panel_rect {
            let tab_bar_top = panel_rect.y + PANE_CORNER_RADIUS;
            pos.x >= panel_rect.x
                && pos.x <= panel_rect.x + panel_rect.width
                && pos.y >= tab_bar_top
                && pos.y <= tab_bar_top + PANEL_TAB_HEIGHT
        } else {
            false
        }
    }

    /// Clamp the panel tab scroll to valid range.
    pub(crate) fn clamp_panel_tab_scroll(&mut self) {
        if let (Some(ref panel_rect), Some(renderer)) = (self.editor_panel_rect, self.renderer.as_ref()) {
            let cell_w = renderer.cell_size().width;
            let tabs = self.active_editor_tabs();
            let total_width = dock_tabs_total_width(&self.panes, &tabs, cell_w);
            let visible_width = panel_rect.width - 2.0 * PANE_PADDING;
            let max_scroll = (total_width - visible_width).max(0.0);
            self.panel_tab_scroll_target = self.panel_tab_scroll_target.clamp(0.0, max_scroll);
            self.panel_tab_scroll = self.panel_tab_scroll.clamp(0.0, max_scroll);
        }
    }

    /// Auto-scroll to make the active panel tab visible.
    pub(crate) fn scroll_to_active_panel_tab(&mut self) {
        if let (Some(active), Some(ref panel_rect), Some(renderer)) =
            (self.active_editor_tab(), self.editor_panel_rect, self.renderer.as_ref())
        {
            let cell_w = renderer.cell_size().width;
            let tabs = self.active_editor_tabs();
            if let Some(idx) = tabs.iter().position(|&id| id == active) {
                let tab_left = dock_tab_x(&self.panes, &tabs, idx, cell_w);
                let title = panel_tab_title(&self.panes, active);
                let tab_right = tab_left + stacked_tab_width(&title, cell_w);
                let visible_width = panel_rect.width - 2.0 * PANE_PADDING;

                if tab_left < self.panel_tab_scroll_target {
                    self.panel_tab_scroll_target = tab_left;
                } else if tab_right > self.panel_tab_scroll_target + visible_width {
                    self.panel_tab_scroll_target = tab_right - visible_width;
                }
                self.clamp_panel_tab_scroll();
            }
        }
    }

    /// Compute the drop destination for a given mouse position during drag.
    pub(crate) fn compute_drop_destination(
        &self,
        mouse: Vec2,
        source: PaneId,
    ) -> Option<DropDestination> {
        // Hovering over editor panel → no drop target (drag is tree-only)
        if let Some(ref panel_rect) = self.editor_panel_rect {
            if panel_rect.contains(mouse) {
                return None;
            }
        }

        self.compute_tree_drop_target(mouse, source)
    }

    /// Compute tree pane drop target (pane + zone) for drag.
    /// Uses tiling rects for hit-testing so the gap between panes is a valid drop area.
    fn compute_tree_drop_target(
        &self,
        mouse: Vec2,
        source: PaneId,
    ) -> Option<DropDestination> {
        let source_tiling = self.pane_rects
            .iter()
            .find(|(id, _)| *id == source)
            .map(|(_, r)| *r);

        // Iterate tiling rects for hit-testing (covers gap areas between panes)
        for &(id, tiling_rect) in &self.pane_rects {
            if id == source {
                continue;
            }
            if !tiling_rect.contains(mouse) {
                continue;
            }

            // Use visual rect for zone computation: rel coords outside [0,1] = in gap → edge zone
            let visual_rect = self.visual_pane_rects
                .iter()
                .find(|(vid, _)| *vid == id)
                .map(|(_, r)| *r)
                .unwrap_or(tiling_rect);

            let rel_x = (mouse.x - visual_rect.x) / visual_rect.width;
            let rel_y = (mouse.y - visual_rect.y) / visual_rect.height;

            let zone = if rel_y < 0.25 {
                DropZone::Top
            } else if rel_y > 0.75 {
                DropZone::Bottom
            } else if rel_x < 0.25 {
                DropZone::Left
            } else if rel_x > 0.75 {
                DropZone::Right
            } else {
                DropZone::Center
            };

            // Check for outer zone: if the zone is directional and the target pane's
            // tiling rect edge touches the pane_area_rect boundary, AND the relative
            // position is within the outer threshold → Root-level drop.
            if zone != DropZone::Center {
                if let Some(area) = self.pane_area_rect {
                    let touches_boundary = match zone {
                        DropZone::Top => tiling_rect.y <= area.y + 0.5,
                        DropZone::Bottom => tiling_rect.y + tiling_rect.height >= area.y + area.height - 0.5,
                        DropZone::Left => tiling_rect.x <= area.x + 0.5,
                        DropZone::Right => tiling_rect.x + tiling_rect.width >= area.x + area.width - 0.5,
                        DropZone::Center => false,
                    };

                    let is_outer = match zone {
                        DropZone::Top => rel_y < OUTER_ZONE_THRESHOLD,
                        DropZone::Bottom => rel_y > (1.0 - OUTER_ZONE_THRESHOLD),
                        DropZone::Left => rel_x < OUTER_ZONE_THRESHOLD,
                        DropZone::Right => rel_x > (1.0 - OUTER_ZONE_THRESHOLD),
                        DropZone::Center => false,
                    };

                    if touches_boundary && is_outer {
                        // Redundancy check: root-level drop is redundant only if the source
                        // already spans the full perpendicular extent on that edge.
                        let source_redundant = if let Some(src_rect) = source_tiling {
                            match zone {
                                DropZone::Top => {
                                    src_rect.y <= area.y + 0.5
                                        && src_rect.x <= area.x + 0.5
                                        && src_rect.x + src_rect.width >= area.x + area.width - 0.5
                                }
                                DropZone::Bottom => {
                                    src_rect.y + src_rect.height >= area.y + area.height - 0.5
                                        && src_rect.x <= area.x + 0.5
                                        && src_rect.x + src_rect.width >= area.x + area.width - 0.5
                                }
                                DropZone::Left => {
                                    src_rect.x <= area.x + 0.5
                                        && src_rect.y <= area.y + 0.5
                                        && src_rect.y + src_rect.height >= area.y + area.height - 0.5
                                }
                                DropZone::Right => {
                                    src_rect.x + src_rect.width >= area.x + area.width - 0.5
                                        && src_rect.y <= area.y + 0.5
                                        && src_rect.y + src_rect.height >= area.y + area.height - 0.5
                                }
                                DropZone::Center => false,
                            }
                        } else {
                            false
                        };

                        if !source_redundant {
                            return Some(DropDestination::TreeRoot(zone));
                        }
                    }
                }
            }

            return Some(DropDestination::TreePane(id, zone));
        }
        None
    }
}
