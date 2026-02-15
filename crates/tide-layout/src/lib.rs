// Layout engine implementation (Stream C)
// Implements tide_core::LayoutEngine with a binary split tree

mod node;
mod tests;

use tide_core::{DropZone, LayoutEngine, PaneDecorations, PaneId, Rect, Size, SplitDirection, Vec2};

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

    /// Equalize the root split's ratio based on same-direction chain leaf counts.
    fn equalize_root_chain(&mut self) {
        if let Some(Node::Split { direction, ratio, left, right, .. }) = &mut self.root {
            let dir = *direction;
            let n_left = left.count_chain_leaves(dir);
            let n_right = right.count_chain_leaves(dir);
            *ratio = n_left as f32 / (n_left + n_right) as f32;
        }
    }

    /// Snap all split ratios so that pane content areas align to cell boundaries.
    /// Call this after `compute()` but before using the resulting rects for rendering.
    /// The caller should call `compute()` again after snapping.
    pub fn snap_ratios_to_cells(
        &mut self,
        window_size: Size,
        cell_size: tide_core::Size,
        decorations: &PaneDecorations,
    ) {
        if let Some(ref mut root) = self.root {
            let rect = Rect::new(0.0, 0.0, window_size.width, window_size.height);
            root.snap_ratios(rect, cell_size, decorations);
        }
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

    /// Insert a new pane at the root level, wrapping the existing tree.
    /// Used when moving a pane from the editor panel to the tree root.
    pub fn insert_at_root(&mut self, new_pane: PaneId, zone: DropZone) -> bool {
        if zone == DropZone::Center {
            return false;
        }

        let (direction, insert_first) = match zone {
            DropZone::Top => (SplitDirection::Vertical, true),
            DropZone::Bottom => (SplitDirection::Vertical, false),
            DropZone::Left => (SplitDirection::Horizontal, true),
            DropZone::Right => (SplitDirection::Horizontal, false),
            DropZone::Center => unreachable!(),
        };

        let new_node = Node::Leaf(new_pane);

        match self.root.take() {
            Some(existing) => {
                let (left, right) = if insert_first {
                    (new_node, existing)
                } else {
                    (existing, new_node)
                };
                self.root = Some(Node::Split {
                    direction,
                    ratio: 0.5,
                    left: Box::new(left),
                    right: Box::new(right),
                });
                // Equalize same-direction chain at root
                self.equalize_root_chain();
            }
            None => {
                self.root = Some(new_node);
            }
        }

        true
    }

    /// Move `source` pane to the root level based on the drop zone.
    /// Removes source from the tree, then wraps the remaining root in a new split.
    /// Returns true if the operation succeeded.
    pub fn move_pane_to_root(&mut self, source: PaneId, zone: DropZone) -> bool {
        if zone == DropZone::Center {
            return false;
        }
        let root = match self.root.as_mut() {
            Some(r) => r,
            None => return false,
        };

        // Remove source from tree
        match root.remove_pane(source) {
            Some(Some(replacement)) => {
                *root = replacement;
            }
            Some(None) => {
                // Source was the only pane — can't do root-level move.
                self.root = Some(Node::Leaf(source));
                return false;
            }
            None => return false,
        }

        // Wrap remaining root with source at the specified edge
        let remaining = self.root.take().unwrap();
        let (direction, insert_first) = match zone {
            DropZone::Top => (SplitDirection::Vertical, true),
            DropZone::Bottom => (SplitDirection::Vertical, false),
            DropZone::Left => (SplitDirection::Horizontal, true),
            DropZone::Right => (SplitDirection::Horizontal, false),
            DropZone::Center => unreachable!(),
        };

        let source_node = Node::Leaf(source);
        let (left, right) = if insert_first {
            (source_node, remaining)
        } else {
            (remaining, source_node)
        };

        self.root = Some(Node::Split {
            direction,
            ratio: 0.5,
            left: Box::new(left),
            right: Box::new(right),
        });
        self.equalize_root_chain();

        true
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

    /// Move `source` pane to the root level with tree restructuring.
    /// Uses original rects (before removal) to rebuild remaining panes into a
    /// direction-appropriate tree, then inserts source at root.
    pub fn restructure_move_to_root(&mut self, source: PaneId, zone: DropZone, window_size: Size) -> bool {
        if zone == DropZone::Center {
            return false;
        }
        if self.root.is_none() {
            return false;
        }

        // 1. Compute original rects before any modifications
        let original_rects = self.compute(window_size, &[], None);

        // Check source exists and there are other panes
        let remaining: Vec<(PaneId, Rect)> = original_rects
            .into_iter()
            .filter(|(id, _)| *id != source)
            .collect();
        if remaining.is_empty() {
            return false;
        }

        // 2. Determine primary direction from drop zone
        let primary = match zone {
            DropZone::Left | DropZone::Right => SplitDirection::Horizontal,
            DropZone::Top | DropZone::Bottom => SplitDirection::Vertical,
            DropZone::Center => unreachable!(),
        };

        // 3. Rebuild remaining panes into a new tree using original rects
        let new_root = match node::build_tree_from_rects(&remaining, primary) {
            Some(tree) => tree,
            None => return false,
        };
        self.root = Some(new_root);

        // 4. Insert source at root (handles equalization)
        self.insert_at_root(source, zone)
    }

    /// Move `source` pane relative to `target` pane with tree restructuring.
    /// Center zone = swap (no restructuring). Directional = rebuild remaining tree
    /// from original rects, then insert source next to target.
    pub fn restructure_move_pane(&mut self, source: PaneId, target: PaneId, zone: DropZone, window_size: Size) -> bool {
        if source == target {
            return false;
        }
        if self.root.is_none() {
            return false;
        }

        // Center zone: just swap, no restructuring needed
        if zone == DropZone::Center {
            if let Some(ref mut root) = self.root {
                root.swap_panes(source, target);
                return true;
            }
            return false;
        }

        // 1. Compute original rects before any modifications
        let original_rects = self.compute(window_size, &[], None);

        let remaining: Vec<(PaneId, Rect)> = original_rects
            .into_iter()
            .filter(|(id, _)| *id != source)
            .collect();
        if remaining.is_empty() {
            return false;
        }

        // 2. Determine primary direction from drop zone
        let primary = match zone {
            DropZone::Left | DropZone::Right => SplitDirection::Horizontal,
            DropZone::Top | DropZone::Bottom => SplitDirection::Vertical,
            DropZone::Center => unreachable!(),
        };

        // 3. Rebuild remaining panes into a new tree using original rects
        let new_root = match node::build_tree_from_rects(&remaining, primary) {
            Some(tree) => tree,
            None => return false,
        };
        self.root = Some(new_root);

        // 4. Insert source next to target (handles equalization)
        let (direction, insert_first) = match zone {
            DropZone::Top => (SplitDirection::Vertical, true),
            DropZone::Bottom => (SplitDirection::Vertical, false),
            DropZone::Left => (SplitDirection::Horizontal, true),
            DropZone::Right => (SplitDirection::Horizontal, false),
            DropZone::Center => unreachable!(),
        };

        if let Some(ref mut root) = self.root {
            root.insert_pane_at(target, source, direction, insert_first)
        } else {
            false
        }
    }

    /// Simulate a drop operation and return the resulting tiling rect for the source pane.
    /// Used for accurate drop preview that accounts for equalization.
    pub fn simulate_drop(
        &self,
        source: PaneId,
        target: Option<PaneId>,
        zone: DropZone,
        source_in_tree: bool,
        window_size: Size,
    ) -> Option<Rect> {
        let mut sim = SplitLayout {
            root: self.root.clone(),
            next_id: self.next_id,
            active_drag: None,
            last_window_size: None,
        };

        match target {
            None => {
                // Root-level drop
                if source_in_tree {
                    if !sim.restructure_move_to_root(source, zone, window_size) {
                        return None;
                    }
                } else if !sim.insert_at_root(source, zone) {
                    return None;
                }
            }
            Some(target_id) => {
                if source_in_tree {
                    if !sim.restructure_move_pane(source, target_id, zone, window_size) {
                        return None;
                    }
                } else {
                    let (direction, insert_first) = match zone {
                        DropZone::Top => (SplitDirection::Vertical, true),
                        DropZone::Bottom => (SplitDirection::Vertical, false),
                        DropZone::Left => (SplitDirection::Horizontal, true),
                        DropZone::Right => (SplitDirection::Horizontal, false),
                        DropZone::Center => (SplitDirection::Horizontal, false),
                    };
                    sim.insert_pane(target_id, source, direction, insert_first);
                }
            }
        }

        let rects = sim.compute(window_size, &[], None);
        rects.into_iter().find(|(id, _)| *id == source).map(|(_, r)| r)
    }
}

// ──────────────────────────────────────────────
// LayoutSnapshot: public tree representation for serialization
// ──────────────────────────────────────────────

/// A public, clonable representation of the layout tree.
/// Used by tide-app for session persistence without exposing `Node`.
#[derive(Debug, Clone)]
pub enum LayoutSnapshot {
    Leaf(PaneId),
    Split {
        direction: SplitDirection,
        ratio: f32,
        left: Box<LayoutSnapshot>,
        right: Box<LayoutSnapshot>,
    },
}

impl SplitLayout {
    /// Capture the current layout tree as a `LayoutSnapshot`.
    pub fn snapshot(&self) -> Option<LayoutSnapshot> {
        self.root.as_ref().map(Self::node_to_snapshot)
    }

    fn node_to_snapshot(node: &Node) -> LayoutSnapshot {
        match node {
            Node::Leaf(id) => LayoutSnapshot::Leaf(*id),
            Node::Split { direction, ratio, left, right } => LayoutSnapshot::Split {
                direction: *direction,
                ratio: *ratio,
                left: Box::new(Self::node_to_snapshot(left)),
                right: Box::new(Self::node_to_snapshot(right)),
            },
        }
    }

    /// Reconstruct a `SplitLayout` from a `LayoutSnapshot`.
    /// The `next_id` is set to one past the maximum PaneId found.
    pub fn from_snapshot(snap: LayoutSnapshot) -> Self {
        let max_id = Self::max_id_in_snapshot(&snap);
        Self {
            root: Some(Self::snapshot_to_node(&snap)),
            next_id: max_id + 1,
            active_drag: None,
            last_window_size: None,
        }
    }

    fn snapshot_to_node(snap: &LayoutSnapshot) -> Node {
        match snap {
            LayoutSnapshot::Leaf(id) => Node::Leaf(*id),
            LayoutSnapshot::Split { direction, ratio, left, right } => Node::Split {
                direction: *direction,
                ratio: *ratio,
                left: Box::new(Self::snapshot_to_node(left)),
                right: Box::new(Self::snapshot_to_node(right)),
            },
        }
    }

    fn max_id_in_snapshot(snap: &LayoutSnapshot) -> PaneId {
        match snap {
            LayoutSnapshot::Leaf(id) => *id,
            LayoutSnapshot::Split { left, right, .. } => {
                Self::max_id_in_snapshot(left).max(Self::max_id_in_snapshot(right))
            }
        }
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
