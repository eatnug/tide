use tide_core::{DropZone, PaneId, Rect, Vec2};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;

// ──────────────────────────────────────────────
// Drop destination: tree pane or editor panel
// ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum DropDestination {
    TreePane(PaneId, DropZone),
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
    /// Hit-test whether the position is within a pane's tab bar area (split tree panes).
    pub(crate) fn pane_at_tab_bar(&self, pos: Vec2) -> Option<PaneId> {
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if tab_rect.contains(pos) {
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

        let tab_start_x = panel_rect.x + PANE_PADDING;
        for (i, &tab_id) in self.editor_panel_tabs.iter().enumerate() {
            let tx = tab_start_x + i as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
            if pos.x >= tx && pos.x <= tx + PANEL_TAB_WIDTH {
                return Some(tab_id);
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

        let tab_start_x = panel_rect.x + PANE_PADDING;
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
                return Some(tab_id);
            }
        }
        None
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
    fn compute_tree_drop_target(
        &self,
        mouse: Vec2,
        source: PaneId,
        from_panel: bool,
    ) -> Option<DropDestination> {
        // For panel sources, we don't have a source_rect in the tree — use a dummy
        let source_rect = if from_panel {
            None
        } else {
            self.visual_pane_rects
                .iter()
                .find(|(id, _)| *id == source)
                .map(|(_, r)| *r)
        };

        for &(id, rect) in &self.visual_pane_rects {
            if !from_panel && id == source {
                continue;
            }
            if !rect.contains(mouse) {
                continue;
            }

            let rel_x = (mouse.x - rect.x) / rect.width;
            let rel_y = (mouse.y - rect.y) / rect.height;

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

            // If source is in tree, check for redundant placement
            if let Some(src_rect) = source_rect {
                let src_cx = src_rect.x + src_rect.width / 2.0;
                let src_cy = src_rect.y + src_rect.height / 2.0;
                let tgt_cx = rect.x + rect.width / 2.0;
                let tgt_cy = rect.y + rect.height / 2.0;

                let is_redundant = match zone {
                    DropZone::Left => src_cx < tgt_cx,
                    DropZone::Right => src_cx > tgt_cx,
                    DropZone::Top => src_cy < tgt_cy,
                    DropZone::Bottom => src_cy > tgt_cy,
                    DropZone::Center => false,
                };
                if is_redundant {
                    zone = DropZone::Center;
                }
            }

            return Some(DropDestination::TreePane(id, zone));
        }
        None
    }
}
