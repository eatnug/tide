use tide_core::{DropZone, PaneId, Rect, Vec2};

use crate::theme::*;
use crate::App;

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
        drop_target: Option<(PaneId, DropZone)>,
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

    /// Compute the drop target (pane + zone) for a given mouse position during drag.
    pub(crate) fn compute_drop_target(&self, mouse: Vec2, source: PaneId) -> Option<(PaneId, DropZone)> {
        let source_rect = self.visual_pane_rects.iter().find(|(id, _)| *id == source).map(|(_, r)| *r)?;

        for &(id, rect) in &self.visual_pane_rects {
            if id == source {
                continue;
            }
            if !rect.contains(mouse) {
                continue;
            }

            // Compute relative position within the pane rect (0.0 to 1.0)
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

            // If the directional zone would result in the same arrangement
            // (source is already on that side of target), show swap instead.
            let src_cx = source_rect.x + source_rect.width / 2.0;
            let src_cy = source_rect.y + source_rect.height / 2.0;
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

            return Some((id, zone));
        }
        None
    }
}
