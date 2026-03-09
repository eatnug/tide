use tide_core::{DropZone, PaneId, Rect, SplitDirection, Vec2};

use crate::theme::*;
use crate::App;

/// Threshold for outer zone detection (0–12% of pane extent).
const OUTER_ZONE_THRESHOLD: f32 = 0.12;

// ──────────────────────────────────────────────
// Workspace sidebar item geometry (shared layout computation)
// ──────────────────────────────────────────────

/// Precomputed layout geometry for workspace sidebar items.
pub(crate) struct WsSidebarGeometry {
    pub content_x: f32,
    pub content_w: f32,
    pub start_y: f32,
    pub item_h: f32,
    pub item_gap: f32,
}

impl WsSidebarGeometry {
    /// Get the rect of the nth workspace sidebar item.
    pub fn item_rect(&self, idx: usize) -> Rect {
        let y = self.start_y + idx as f32 * (self.item_h + self.item_gap);
        Rect::new(self.content_x, y, self.content_w, self.item_h)
    }
}

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
    PaneMaximize(PaneId),
    FileFinderItem(usize),
    SidebarHandle,
    TitlebarSwap,
    TitlebarSettings,
    TitlebarTheme,
    TitlebarFileTree,
    TitlebarPaneArea,
    BrowserBack,
    BrowserForward,
    BrowserRefresh,
    BrowserUrlBar,
    EditorScrollbar(PaneId),
    WorkspaceSidebarItem(usize),
    WorkspaceSidebarNewBtn,
}

impl HoverTarget {
    /// Returns true if this hover target's visual feedback is rendered in the chrome layer.
    /// When transitioning to/from these targets, chrome_generation must be bumped.
    pub(crate) fn affects_chrome(&self) -> bool {
        matches!(
            self,
            HoverTarget::TitlebarSwap
                | HoverTarget::TitlebarSettings
                | HoverTarget::TitlebarTheme
                | HoverTarget::TitlebarFileTree
                | HoverTarget::TitlebarPaneArea
                | HoverTarget::BrowserBack
                | HoverTarget::BrowserForward
                | HoverTarget::BrowserRefresh
                | HoverTarget::PaneTabClose(_)
                | HoverTarget::WorkspaceSidebarItem(_)
                | HoverTarget::WorkspaceSidebarNewBtn
        )
    }
}

// ──────────────────────────────────────────────
// Drop destination: tree pane
// ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum DropDestination {
    TreePane(PaneId, DropZone),
    TreeRoot(DropZone),
    Workspace(usize),
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
    /// Hit-test whether the position is within a pane's tab bar area.
    pub(crate) fn pane_at_tab_bar(&self, pos: Vec2) -> Option<PaneId> {
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if tab_rect.contains(pos) {
                return Some(id);
            }
        }
        None
    }

    /// Hit-test whether the position is on a pane tab bar close button.
    pub(crate) fn pane_tab_close_at(&self, pos: Vec2) -> Option<PaneId> {
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if !tab_rect.contains(pos) {
                continue;
            }
            // Close badge is the rightmost badge, grid-aligned
            let cell_w = self.cell_size().width;
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
    pub(crate) fn pane_maximize_at(&self, pos: Vec2) -> Option<PaneId> {
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if !tab_rect.contains(pos) {
                continue;
            }
            let cell_w = self.cell_size().width;
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

    /// Compute the drop destination for a given mouse position during drag.
    pub(crate) fn compute_drop_destination(
        &self,
        mouse: Vec2,
        source: PaneId,
    ) -> Option<DropDestination> {
        // Check workspace sidebar first — allow cross-workspace pane moves
        if let Some(idx) = self.workspace_sidebar_item_at_pos(mouse) {
            if idx != self.active_workspace {
                return Some(DropDestination::Workspace(idx));
            }
        }
        self.compute_tree_drop_target(mouse, source)
    }

    /// Compute workspace sidebar item layout geometry.
    pub(crate) fn ws_sidebar_geometry(&self) -> Option<WsSidebarGeometry> {
        let ws_rect = self.workspace_sidebar_rect?;
        let cs = self.cell_size();
        let name_h = cs.height;
        let sub_h = cs.height * WS_SIDEBAR_SUB_SCALE;
        Some(WsSidebarGeometry {
            content_x: ws_rect.x + WS_SIDEBAR_PADDING,
            content_w: ws_rect.width - WS_SIDEBAR_PADDING * 2.0,
            start_y: ws_rect.y + PANE_CORNER_RADIUS + WS_SIDEBAR_PADDING,
            item_h: WS_SIDEBAR_ITEM_PAD_V * 2.0 + name_h + WS_SIDEBAR_LINE_GAP + sub_h,
            item_gap: WS_SIDEBAR_ITEM_GAP,
        })
    }

    /// Hit-test workspace sidebar items. Returns the 0-based workspace index if hit.
    fn workspace_sidebar_item_at_pos(&self, pos: Vec2) -> Option<usize> {
        if !self.show_workspace_sidebar {
            return None;
        }
        let geo = self.ws_sidebar_geometry()?;
        for i in 0..self.workspaces.len() {
            if geo.item_rect(i).contains(pos) {
                return Some(i);
            }
        }
        None
    }

    /// Get the visual rect of a workspace sidebar item (for rendering drag highlights).
    pub(crate) fn workspace_sidebar_item_rect(&self, idx: usize) -> Option<Rect> {
        let geo = self.ws_sidebar_geometry()?;
        if idx < self.workspaces.len() {
            Some(geo.item_rect(idx))
        } else {
            None
        }
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
