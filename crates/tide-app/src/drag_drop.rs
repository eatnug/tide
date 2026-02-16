use tide_core::{DropZone, PaneId, Rect, SplitDirection, Vec2};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;

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
}

// ──────────────────────────────────────────────
// Drop destination: tree pane or editor panel
// ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum DropDestination {
    TreePane(PaneId, DropZone),
    TreeRoot(DropZone),
    EditorPanel,
}

// ──────────────────────────────────────────────
// Pane drag & drop state machine
// ──────────────────────────────────────────────

pub(crate) enum PaneDragState {
    Idle,
    PendingDrag {
        source_pane: PaneId,
        press_pos: Vec2,
        from_panel: bool,
    },
    Dragging {
        source_pane: PaneId,
        from_panel: bool,
        drop_target: Option<DropDestination>,
    },
}

impl App {
    /// Hit-test whether the position is on a stacked-mode tab. Returns the PaneId of the tab.
    pub(crate) fn stacked_tab_at(&self, pos: Vec2) -> Option<PaneId> {
        let crate::PaneAreaMode::Stacked(_) = self.pane_area_mode else {
            return None;
        };
        let &(_, rect) = self.visual_pane_rects.first()?;
        let tab_bar_top = rect.y + PANE_PADDING;
        if pos.y < tab_bar_top || pos.y > tab_bar_top + PANEL_TAB_HEIGHT {
            return None;
        }
        let pane_ids = self.layout.pane_ids();
        let tab_start_x = rect.x + PANE_PADDING;
        for (i, &tab_id) in pane_ids.iter().enumerate() {
            let tx = tab_start_x + i as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
            if pos.x >= tx && pos.x <= tx + PANEL_TAB_WIDTH {
                return Some(tab_id);
            }
        }
        None
    }

    /// Hit-test whether the position is on a stacked-mode tab's close button.
    pub(crate) fn stacked_tab_close_at(&self, pos: Vec2) -> Option<PaneId> {
        let crate::PaneAreaMode::Stacked(_) = self.pane_area_mode else {
            return None;
        };
        let &(_, rect) = self.visual_pane_rects.first()?;
        let tab_bar_top = rect.y + PANE_PADDING;
        if pos.y < tab_bar_top || pos.y > tab_bar_top + PANEL_TAB_HEIGHT {
            return None;
        }
        let pane_ids = self.layout.pane_ids();
        let tab_start_x = rect.x + PANE_PADDING;
        for (i, &tab_id) in pane_ids.iter().enumerate() {
            let tx = tab_start_x + i as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
            let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 4.0;
            let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - PANEL_TAB_CLOSE_SIZE) / 2.0;
            if pos.x >= close_x
                && pos.x <= close_x + PANEL_TAB_CLOSE_SIZE
                && pos.y >= close_y
                && pos.y <= close_y + PANEL_TAB_CLOSE_SIZE
            {
                return Some(tab_id);
            }
        }
        None
    }

    /// Hit-test whether the position is within a pane's tab bar area (split tree panes).
    /// Returns None in stacked mode (stacked has its own tab bar).
    pub(crate) fn pane_at_tab_bar(&self, pos: Vec2) -> Option<PaneId> {
        if matches!(self.pane_area_mode, crate::PaneAreaMode::Stacked(_)) {
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
        if matches!(self.pane_area_mode, crate::PaneAreaMode::Stacked(_)) {
            return None;
        }
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if !tab_rect.contains(pos) {
                continue;
            }
            let close_x = rect.x + rect.width - PANE_CLOSE_SIZE - PANE_PADDING;
            let close_y = rect.y + (TAB_BAR_HEIGHT - PANE_CLOSE_SIZE) / 2.0;
            if pos.x >= close_x
                && pos.x <= close_x + PANE_CLOSE_SIZE
                && pos.y >= close_y
                && pos.y <= close_y + PANE_CLOSE_SIZE
            {
                return Some(id);
            }
        }
        None
    }

    /// Hit-test whether the position is on a panel tab. Returns the PaneId of the tab.
    pub(crate) fn panel_tab_at(&self, pos: Vec2) -> Option<PaneId> {
        let panel_rect = self.editor_panel_rect.as_ref()?;
        // Tab bar area is the top PANEL_TAB_HEIGHT of the panel
        let tab_bar_top = panel_rect.y + PANE_PADDING;
        if pos.y < tab_bar_top || pos.y > tab_bar_top + PANEL_TAB_HEIGHT {
            return None;
        }

        let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
        for (i, &tab_id) in self.editor_panel_tabs.iter().enumerate() {
            let tx = tab_start_x + i as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
            if pos.x >= tx && pos.x <= tx + PANEL_TAB_WIDTH {
                // Only match if within panel bounds
                if pos.x >= panel_rect.x && pos.x <= panel_rect.x + panel_rect.width {
                    return Some(tab_id);
                }
            }
        }
        None
    }

    /// Check if a click position is on the close button of a panel tab.
    /// Returns the tab's PaneId if clicking the close "x".
    pub(crate) fn panel_tab_close_at(&self, pos: Vec2) -> Option<PaneId> {
        let panel_rect = self.editor_panel_rect.as_ref()?;
        let tab_bar_top = panel_rect.y + PANE_PADDING;
        if pos.y < tab_bar_top || pos.y > tab_bar_top + PANEL_TAB_HEIGHT {
            return None;
        }

        let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
        for (i, &tab_id) in self.editor_panel_tabs.iter().enumerate() {
            let tx = tab_start_x + i as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
            // Close button is on the right edge of the tab
            let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 4.0;
            let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - PANEL_TAB_CLOSE_SIZE) / 2.0;
            if pos.x >= close_x
                && pos.x <= close_x + PANEL_TAB_CLOSE_SIZE
                && pos.y >= close_y
                && pos.y <= close_y + PANEL_TAB_CLOSE_SIZE
            {
                if pos.x >= panel_rect.x && pos.x <= panel_rect.x + panel_rect.width {
                    return Some(tab_id);
                }
            }
        }
        None
    }

    /// Check if the cursor is in the panel tab bar area.
    pub(crate) fn is_over_panel_tab_bar(&self, pos: Vec2) -> bool {
        if let Some(ref panel_rect) = self.editor_panel_rect {
            let tab_bar_top = panel_rect.y + PANE_PADDING;
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
        if let Some(ref panel_rect) = self.editor_panel_rect {
            let total_width = self.editor_panel_tabs.len() as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP) - PANEL_TAB_GAP;
            let visible_width = panel_rect.width - 2.0 * PANE_PADDING;
            let max_scroll = (total_width - visible_width).max(0.0);
            self.panel_tab_scroll_target = self.panel_tab_scroll_target.clamp(0.0, max_scroll);
            self.panel_tab_scroll = self.panel_tab_scroll.clamp(0.0, max_scroll);
        }
    }

    /// Auto-scroll to make the active panel tab visible.
    pub(crate) fn scroll_to_active_panel_tab(&mut self) {
        if let (Some(active), Some(ref panel_rect)) = (self.editor_panel_active, self.editor_panel_rect) {
            if let Some(idx) = self.editor_panel_tabs.iter().position(|&id| id == active) {
                let tab_left = idx as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
                let tab_right = tab_left + PANEL_TAB_WIDTH;
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
    /// Checks editor panel first, then falls back to tree pane targets.
    pub(crate) fn compute_drop_destination(
        &self,
        mouse: Vec2,
        source: PaneId,
        from_panel: bool,
    ) -> Option<DropDestination> {
        // Check panel rect first (only if source is an editor pane and from tree)
        if !from_panel {
            if let Some(ref panel_rect) = self.editor_panel_rect {
                if panel_rect.contains(mouse) {
                    // Only accept editor panes, reject terminals
                    if matches!(self.panes.get(&source), Some(PaneKind::Editor(_))) {
                        // Reject if this is the last tree pane
                        if self.layout.pane_ids().len() > 1 {
                            return Some(DropDestination::EditorPanel);
                        }
                    }
                    return None;
                }
            }
        }
        // Even if from_panel and hovering panel area, show no target (can't drop back on self)
        if from_panel {
            if let Some(ref panel_rect) = self.editor_panel_rect {
                if panel_rect.contains(mouse) {
                    return None;
                }
            }
        }

        // Fall back to tree pane drop targets
        self.compute_tree_drop_target(mouse, source, from_panel)
    }

    /// Compute tree pane drop target (pane + zone) for drag.
    /// Uses tiling rects for hit-testing so the gap between panes is a valid drop area.
    fn compute_tree_drop_target(
        &self,
        mouse: Vec2,
        source: PaneId,
        from_panel: bool,
    ) -> Option<DropDestination> {
        // Use tiling rects for source redundancy check (tiling rects touch area boundary)
        let source_tiling = if from_panel {
            None
        } else {
            self.pane_rects
                .iter()
                .find(|(id, _)| *id == source)
                .map(|(_, r)| *r)
        };

        // Iterate tiling rects for hit-testing (covers gap areas between panes)
        for &(id, tiling_rect) in &self.pane_rects {
            if !from_panel && id == source {
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

            let mut zone = if rel_y < 0.25 {
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

            // Disallow swap (Center) when dragging from editor panel onto a terminal pane
            if from_panel && zone == DropZone::Center {
                if matches!(self.panes.get(&id), Some(PaneKind::Terminal(_))) {
                    // Fall back to the closest directional zone
                    let dx = rel_x - 0.5;
                    let dy = rel_y - 0.5;
                    zone = if dx.abs() > dy.abs() {
                        if dx > 0.0 { DropZone::Right } else { DropZone::Left }
                    } else {
                        if dy > 0.0 { DropZone::Bottom } else { DropZone::Top }
                    };
                }
            }

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
                            false // panel sources never touch tree boundaries
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
