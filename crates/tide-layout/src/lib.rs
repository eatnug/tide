// Layout engine implementation (Stream C)
// Implements tide_core::LayoutEngine with a binary split tree

mod node;
mod tests;

use tide_core::{DropZone, LayoutEngine, PaneId, Rect, Size, SplitDirection, Vec2};

use node::Node;

// ──────────────────────────────────────────────
// SplitLayout
// ──────────────────────────────────────────────

/// Minimum split ratio to prevent panes from becoming too small.
const MIN_RATIO: f32 = 0.1;

/// Border hit-test threshold in pixels.
const BORDER_HIT_THRESHOLD: f32 = 8.0;

pub struct SplitLayout {
    pub(crate) root: Option<Node>,
    next_id: PaneId,
    /// The currently active drag: path to the split node being dragged.
    pub(crate) active_drag: Option<Vec<bool>>,
    /// The last window size used for drag computation (needed to reconstruct rects during drag).
    pub last_window_size: Option<Size>,
}

impl SplitLayout {
    pub fn new() -> Self {
        Self {
            root: None,
            next_id: 1,
            active_drag: None,
            last_window_size: None,
        }
    }

    /// Create a layout with a single initial pane and return both the layout and the PaneId.
    pub fn with_initial_pane() -> (Self, PaneId) {
        let id: PaneId = 1;
        let layout = Self {
            root: Some(Node::Leaf(id)),
            next_id: 2,
            active_drag: None,
            last_window_size: None,
        };
        (layout, id)
    }

    pub fn alloc_id(&mut self) -> PaneId {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// Begin a drag if the position is near a border. Called externally before drag_border.
    pub fn begin_drag(&mut self, position: Vec2, window_size: Size) {
        if let Some(ref root) = self.root {
            let window_rect = Rect::new(0.0, 0.0, window_size.width, window_size.height);
            let mut best: Option<(f32, Vec<bool>)> = None;
            let mut path = Vec::new();
            root.find_border_at(window_rect, position, &mut best, &mut path);

            if let Some((dist, border_path)) = best {
                if dist <= BORDER_HIT_THRESHOLD {
                    self.active_drag = Some(border_path);
                    self.last_window_size = Some(window_size);
                }
            }
        }
    }

    /// End the current drag.
    pub fn end_drag(&mut self) {
        self.active_drag = None;
    }

    /// Get all pane IDs in the layout.
    pub fn pane_ids(&self) -> Vec<PaneId> {
        let mut ids = Vec::new();
        if let Some(ref root) = self.root {
            root.pane_ids(&mut ids);
        }
        ids
    }

    /// Insert a new pane next to an existing target pane in the split tree.
    /// Used when moving panes from the editor panel into the tree.
    pub fn insert_pane(
        &mut self,
        target: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
        insert_first: bool,
    ) -> bool {
        if let Some(ref mut root) = self.root {
            root.insert_pane_at(target, new_pane, direction, insert_first)
        } else {
            // Tree is empty — make this the root
            self.root = Some(Node::Leaf(new_pane));
            true
        }
    }

    /// Move `source` pane relative to `target` pane based on the drop zone.
    /// Center = swap the two panes. Directional = remove source, insert next to target.
    /// Returns true if the operation succeeded.
    pub fn move_pane(&mut self, source: PaneId, target: PaneId, zone: DropZone) -> bool {
        if source == target {
            return false;
        }
        let root = match self.root.as_mut() {
            Some(r) => r,
            None => return false,
        };

        if zone == DropZone::Center {
            root.swap_panes(source, target);
            return true;
        }

        // Directional move: remove source from tree, then insert next to target.
        match root.remove_pane(source) {
            Some(Some(replacement)) => {
                *root = replacement;
            }
            Some(None) => {
                // Source was the only pane — can't move it.
                self.root = Some(Node::Leaf(source));
                return false;
            }
            None => return false,
        }

        let root = self.root.as_mut().unwrap();
        let (direction, insert_first) = match zone {
            DropZone::Top => (SplitDirection::Vertical, true),
            DropZone::Bottom => (SplitDirection::Vertical, false),
            DropZone::Left => (SplitDirection::Horizontal, true),
            DropZone::Right => (SplitDirection::Horizontal, false),
            DropZone::Center => unreachable!(),
        };

        root.insert_pane_at(target, source, direction, insert_first)
    }
}

impl Default for SplitLayout {
    fn default() -> Self {
        Self::new()
    }
}

impl LayoutEngine for SplitLayout {
    fn compute(
        &self,
        window_size: Size,
        _panes: &[PaneId],
        _focused: Option<PaneId>,
    ) -> Vec<(PaneId, Rect)> {
        let mut result = Vec::new();
        if let Some(ref root) = self.root {
            let window_rect = Rect::new(0.0, 0.0, window_size.width, window_size.height);
            root.compute_rects(window_rect, &mut result);
        }
        result
    }

    fn drag_border(&mut self, position: Vec2) {
        // If there is an active drag, apply it.
        let drag_path = match self.active_drag {
            Some(ref p) => p.clone(),
            None => {
                // Auto-detect: find the closest border to the position and drag it.
                if let (Some(ref root), Some(ws)) = (&self.root, self.last_window_size) {
                    let window_rect = Rect::new(0.0, 0.0, ws.width, ws.height);
                    let mut best: Option<(f32, Vec<bool>)> = None;
                    let mut path = Vec::new();
                    root.find_border_at(window_rect, position, &mut best, &mut path);

                    if let Some((_dist, border_path)) = best {
                        self.active_drag = Some(border_path.clone());
                        border_path
                    } else {
                        return;
                    }
                } else {
                    return;
                }
            }
        };

        if let (Some(ref mut root), Some(ws)) = (&mut self.root, self.last_window_size) {
            let window_rect = Rect::new(0.0, 0.0, ws.width, ws.height);
            root.apply_drag(window_rect, &drag_path, position, MIN_RATIO);
        }
    }

    fn split(&mut self, pane: PaneId, direction: SplitDirection) -> PaneId {
        let new_id = self.alloc_id();

        if let Some(ref mut root) = self.root {
            if root.split_pane(pane, new_id, direction) {
                return new_id;
            }
        }

        new_id
    }

    fn remove(&mut self, pane: PaneId) {
        if let Some(ref mut root) = self.root {
            match root.remove_pane(pane) {
                Some(Some(replacement)) => {
                    *root = replacement;
                }
                Some(None) => {
                    self.root = None;
                }
                None => {}
            }
        }
    }
}
