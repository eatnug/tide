// Layout engine implementation (Stream C)
// Implements tide_core::LayoutEngine with a binary split tree

use tide_core::{LayoutEngine, PaneId, Rect, Size, SplitDirection, Vec2};

// ──────────────────────────────────────────────
// Node: binary tree for layout
// ──────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Node {
    Leaf(PaneId),
    Split {
        direction: SplitDirection,
        ratio: f32,
        left: Box<Node>,
        right: Box<Node>,
    },
}

impl Node {
    /// Returns true if this node (or any descendant) contains the given pane.
    fn contains(&self, pane: PaneId) -> bool {
        match self {
            Node::Leaf(id) => *id == pane,
            Node::Split { left, right, .. } => left.contains(pane) || right.contains(pane),
        }
    }

    /// Collect all leaf PaneIds in this subtree.
    fn pane_ids(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { left, right, .. } => {
                left.pane_ids(out);
                right.pane_ids(out);
            }
        }
    }

    /// Traverse the tree and compute the rect for every leaf pane.
    fn compute_rects(&self, rect: Rect, out: &mut Vec<(PaneId, Rect)>) {
        match self {
            Node::Leaf(id) => {
                out.push((*id, rect));
            }
            Node::Split {
                direction,
                ratio,
                left,
                right,
            } => {
                let (left_rect, right_rect) = split_rect(rect, *direction, *ratio);
                left.compute_rects(left_rect, out);
                right.compute_rects(right_rect, out);
            }
        }
    }

    /// Replace a leaf with a split node containing the original leaf and a new leaf.
    /// Returns the new PaneId if the split was performed, or None if the target was not found.
    fn split_pane(
        &mut self,
        target: PaneId,
        new_id: PaneId,
        direction: SplitDirection,
    ) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                let original = Node::Leaf(target);
                let new_leaf = Node::Leaf(new_id);
                *self = Node::Split {
                    direction,
                    ratio: 0.5,
                    left: Box::new(original),
                    right: Box::new(new_leaf),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { left, right, .. } => {
                if left.split_pane(target, new_id, direction) {
                    return true;
                }
                right.split_pane(target, new_id, direction)
            }
        }
    }

    /// Remove a pane from the tree. Returns:
    /// - Some(Some(node)) if the pane was found and a sibling remains
    /// - Some(None) if the pane was found and this entire node should be removed (leaf case)
    /// - None if the pane was not found in this subtree
    fn remove_pane(&mut self, target: PaneId) -> Option<Option<Node>> {
        match self {
            Node::Leaf(id) if *id == target => {
                // This leaf should be removed; the parent must handle collapsing.
                Some(None)
            }
            Node::Leaf(_) => None,
            Node::Split { left, right, .. } => {
                // Try removing from left child
                if let Some(replacement) = left.remove_pane(target) {
                    return match replacement {
                        Some(node) => {
                            // Left child was restructured
                            *left = Box::new(node);
                            // Keep this split node as-is (child was replaced internally)
                            // Actually we need to signal upward that we handled it.
                            // Return the current node restructured? No:
                            // The remove happened deeper, left was patched. Signal success.
                            Some(Some(self.clone()))
                        }
                        None => {
                            // Left child is gone; replace this split with right child.
                            Some(Some(right.as_ref().clone()))
                        }
                    };
                }
                // Try removing from right child
                if let Some(replacement) = right.remove_pane(target) {
                    return match replacement {
                        Some(node) => {
                            *right = Box::new(node);
                            Some(Some(self.clone()))
                        }
                        None => {
                            // Right child is gone; replace this split with left child.
                            Some(Some(left.as_ref().clone()))
                        }
                    };
                }
                None
            }
        }
    }

    /// Find the split node whose border is closest to the given position, given
    /// the rect this node occupies. Returns a mutable reference path described by
    /// the node path indices, but for simplicity we use a different approach:
    /// we return the best (distance, path) where path is a vec of left/right choices.
    fn find_border_at(
        &self,
        rect: Rect,
        position: Vec2,
        best: &mut Option<(f32, Vec<bool>)>,
        path: &mut Vec<bool>,
    ) {
        if let Node::Split {
            direction,
            ratio,
            left,
            right,
        } = self
        {
            let border_pos = match direction {
                SplitDirection::Horizontal => rect.x + rect.width * ratio,
                SplitDirection::Vertical => rect.y + rect.height * ratio,
            };

            // Compute distance from position to border line
            let dist = match direction {
                SplitDirection::Horizontal => (position.x - border_pos).abs(),
                SplitDirection::Vertical => (position.y - border_pos).abs(),
            };

            // Check that the position is within the perpendicular extent of the border
            let in_range = match direction {
                SplitDirection::Horizontal => {
                    position.y >= rect.y && position.y <= rect.y + rect.height
                }
                SplitDirection::Vertical => {
                    position.x >= rect.x && position.x <= rect.x + rect.width
                }
            };

            if in_range {
                let dominated = match best {
                    Some((best_dist, _)) => dist < *best_dist,
                    None => true,
                };
                if dominated {
                    *best = Some((dist, path.clone()));
                }
            }

            let (left_rect, right_rect) = split_rect(rect, *direction, *ratio);

            path.push(false); // left
            left.find_border_at(left_rect, position, best, path);
            path.pop();

            path.push(true); // right
            right.find_border_at(right_rect, position, best, path);
            path.pop();
        }
    }

    /// Apply a drag operation: follow the path to find the split node, compute
    /// the new ratio based on position and the rect at that level.
    fn apply_drag(&mut self, rect: Rect, path: &[bool], position: Vec2, min_ratio: f32) {
        if let Node::Split {
            direction,
            ratio,
            left,
            right,
        } = self
        {
            if path.is_empty() {
                // This is the target split node. Update its ratio.
                let new_ratio = match direction {
                    SplitDirection::Horizontal => {
                        (position.x - rect.x) / rect.width
                    }
                    SplitDirection::Vertical => {
                        (position.y - rect.y) / rect.height
                    }
                };
                *ratio = new_ratio.clamp(min_ratio, 1.0 - min_ratio);
            } else {
                let (left_rect, right_rect) = split_rect(rect, *direction, *ratio);
                if !path[0] {
                    left.apply_drag(left_rect, &path[1..], position, min_ratio);
                } else {
                    right.apply_drag(right_rect, &path[1..], position, min_ratio);
                }
            }
        }
    }
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Split a rect into two sub-rects based on direction and ratio.
fn split_rect(rect: Rect, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
    match direction {
        SplitDirection::Horizontal => {
            let left_width = rect.width * ratio;
            let right_width = rect.width - left_width;
            (
                Rect::new(rect.x, rect.y, left_width, rect.height),
                Rect::new(rect.x + left_width, rect.y, right_width, rect.height),
            )
        }
        SplitDirection::Vertical => {
            let top_height = rect.height * ratio;
            let bottom_height = rect.height - top_height;
            (
                Rect::new(rect.x, rect.y, rect.width, top_height),
                Rect::new(rect.x, rect.y + top_height, rect.width, bottom_height),
            )
        }
    }
}

// ──────────────────────────────────────────────
// SplitLayout
// ──────────────────────────────────────────────

/// Minimum split ratio to prevent panes from becoming too small.
const MIN_RATIO: f32 = 0.1;

/// Border hit-test threshold in pixels.
const BORDER_HIT_THRESHOLD: f32 = 8.0;

pub struct SplitLayout {
    root: Option<Node>,
    next_id: PaneId,
    /// The currently active drag: path to the split node being dragged.
    active_drag: Option<Vec<bool>>,
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

    fn alloc_id(&mut self) -> PaneId {
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
                // This requires knowing the window size. Use last_window_size if available.
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

        // If the pane was not found (or root was None), we still return the id
        // but effectively do nothing. In practice, callers should only split existing panes.
        // Return the allocated ID anyway since the trait requires it.
        new_id
    }

    fn remove(&mut self, pane: PaneId) {
        if let Some(ref mut root) = self.root {
            match root.remove_pane(pane) {
                Some(Some(replacement)) => {
                    *root = replacement;
                }
                Some(None) => {
                    // The root itself was the leaf being removed.
                    self.root = None;
                }
                None => {
                    // Pane not found; do nothing.
                }
            }
        }
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tide_core::{Rect, Size, SplitDirection, Vec2};

    const WINDOW: Size = Size {
        width: 800.0,
        height: 600.0,
    };

    fn approx_eq(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    fn rect_approx_eq(a: &Rect, b: &Rect) -> bool {
        approx_eq(a.x, b.x)
            && approx_eq(a.y, b.y)
            && approx_eq(a.width, b.width)
            && approx_eq(a.height, b.height)
    }

    // ──────────────────────────────────────────
    // Basic construction
    // ──────────────────────────────────────────

    #[test]
    fn test_new_is_empty() {
        let layout = SplitLayout::new();
        let rects = layout.compute(WINDOW, &[], None);
        assert!(rects.is_empty());
    }

    #[test]
    fn test_with_initial_pane() {
        let (layout, pane_id) = SplitLayout::with_initial_pane();
        assert_eq!(pane_id, 1);
        let rects = layout.compute(WINDOW, &[pane_id], None);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].0, pane_id);
    }

    // ──────────────────────────────────────────
    // Single pane fills entire window
    // ──────────────────────────────────────────

    #[test]
    fn test_single_pane_fills_window() {
        let (layout, pane_id) = SplitLayout::with_initial_pane();
        let rects = layout.compute(WINDOW, &[pane_id], Some(pane_id));
        assert_eq!(rects.len(), 1);
        let (id, rect) = &rects[0];
        assert_eq!(*id, pane_id);
        assert!(rect_approx_eq(rect, &Rect::new(0.0, 0.0, 800.0, 600.0)));
    }

    // ──────────────────────────────────────────
    // Horizontal split divides width
    // ──────────────────────────────────────────

    #[test]
    fn test_horizontal_split_divides_width() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        assert_eq!(rects.len(), 2);

        // Left pane (original)
        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(
            &left.1,
            &Rect::new(0.0, 0.0, 400.0, 600.0)
        ));

        // Right pane (new)
        let right = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(rect_approx_eq(
            &right.1,
            &Rect::new(400.0, 0.0, 400.0, 600.0)
        ));
    }

    // ──────────────────────────────────────────
    // Vertical split divides height
    // ──────────────────────────────────────────

    #[test]
    fn test_vertical_split_divides_height() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Vertical);

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        assert_eq!(rects.len(), 2);

        // Top pane (original)
        let top = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(
            &top.1,
            &Rect::new(0.0, 0.0, 800.0, 300.0)
        ));

        // Bottom pane (new)
        let bottom = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(rect_approx_eq(
            &bottom.1,
            &Rect::new(0.0, 300.0, 800.0, 300.0)
        ));
    }

    // ──────────────────────────────────────────
    // Nested splits
    // ──────────────────────────────────────────

    #[test]
    fn test_nested_splits() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        // Split horizontally: left=pane1, right=pane2
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        // Split pane2 vertically: top=pane2, bottom=pane3
        let pane3 = layout.split(pane2, SplitDirection::Vertical);

        let rects = layout.compute(WINDOW, &[pane1, pane2, pane3], None);
        assert_eq!(rects.len(), 3);

        // pane1 is the left half
        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(
            &r1.1,
            &Rect::new(0.0, 0.0, 400.0, 600.0)
        ));

        // pane2 is the top-right quarter
        let r2 = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(rect_approx_eq(
            &r2.1,
            &Rect::new(400.0, 0.0, 400.0, 300.0)
        ));

        // pane3 is the bottom-right quarter
        let r3 = rects.iter().find(|(id, _)| *id == pane3).unwrap();
        assert!(rect_approx_eq(
            &r3.1,
            &Rect::new(400.0, 300.0, 400.0, 300.0)
        ));
    }

    #[test]
    fn test_deeply_nested_splits() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane1, SplitDirection::Vertical);
        let pane4 = layout.split(pane2, SplitDirection::Vertical);

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);

        // Left half is split vertically: pane1 top-left, pane3 bottom-left
        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(
            &r1.1,
            &Rect::new(0.0, 0.0, 400.0, 300.0)
        ));
        let r3 = rects.iter().find(|(id, _)| *id == pane3).unwrap();
        assert!(rect_approx_eq(
            &r3.1,
            &Rect::new(0.0, 300.0, 400.0, 300.0)
        ));

        // Right half is split vertically: pane2 top-right, pane4 bottom-right
        let r2 = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(rect_approx_eq(
            &r2.1,
            &Rect::new(400.0, 0.0, 400.0, 300.0)
        ));
        let r4 = rects.iter().find(|(id, _)| *id == pane4).unwrap();
        assert!(rect_approx_eq(
            &r4.1,
            &Rect::new(400.0, 300.0, 400.0, 300.0)
        ));
    }

    // ──────────────────────────────────────────
    // Remove pane collapses the split
    // ──────────────────────────────────────────

    #[test]
    fn test_remove_pane_collapses_split() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        // Remove pane2, layout should collapse back to just pane1
        layout.remove(pane2);
        let rects = layout.compute(WINDOW, &[pane1], None);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].0, pane1);
        assert!(rect_approx_eq(
            &rects[0].1,
            &Rect::new(0.0, 0.0, 800.0, 600.0)
        ));
    }

    #[test]
    fn test_remove_left_pane_collapses_to_right() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        // Remove the left pane (pane1), should collapse to pane2 taking full window
        layout.remove(pane1);
        let rects = layout.compute(WINDOW, &[pane2], None);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].0, pane2);
        assert!(rect_approx_eq(
            &rects[0].1,
            &Rect::new(0.0, 0.0, 800.0, 600.0)
        ));
    }

    #[test]
    fn test_remove_from_nested() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane2, SplitDirection::Vertical);

        // Remove pane3, pane2 should take its full right half again
        layout.remove(pane3);
        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        assert_eq!(rects.len(), 2);

        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(
            &r1.1,
            &Rect::new(0.0, 0.0, 400.0, 600.0)
        ));

        let r2 = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(rect_approx_eq(
            &r2.1,
            &Rect::new(400.0, 0.0, 400.0, 600.0)
        ));
    }

    #[test]
    fn test_remove_last_pane() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        layout.remove(pane1);
        let rects = layout.compute(WINDOW, &[], None);
        assert!(rects.is_empty());
    }

    #[test]
    fn test_remove_nonexistent_pane() {
        let (mut layout, _pane1) = SplitLayout::with_initial_pane();
        // Removing a non-existent pane should not crash or change anything
        layout.remove(999);
        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 1);
    }

    // ──────────────────────────────────────────
    // No gaps, no overlaps (rects tile the window)
    // ──────────────────────────────────────────

    #[test]
    fn test_no_gaps_no_overlaps_two_panes() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let rects = layout.compute(WINDOW, &[pane1, pane2], None);

        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_no_gaps_no_overlaps_four_panes() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let _pane3 = layout.split(pane1, SplitDirection::Vertical);
        let _pane4 = layout.split(pane2, SplitDirection::Vertical);

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_no_gaps_no_overlaps_many_splits() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane2, SplitDirection::Vertical);
        let _pane4 = layout.split(pane3, SplitDirection::Horizontal);
        let _pane5 = layout.split(pane1, SplitDirection::Vertical);

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 5);
        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    /// Check that the total area of all rects equals the window area
    /// and that no two rects overlap (they may share edges but not interior).
    fn assert_no_gaps_no_overlaps(rects: &[(PaneId, Rect)], window: Size) {
        let window_area = window.width * window.height;

        let total_area: f32 = rects.iter().map(|(_, r)| r.width * r.height).sum();
        assert!(
            approx_eq(total_area, window_area),
            "Total area {total_area} != window area {window_area}"
        );

        // Check no interior overlap between any pair
        for i in 0..rects.len() {
            for j in (i + 1)..rects.len() {
                let a = &rects[i].1;
                let b = &rects[j].1;
                let overlap_x = (a.x.max(b.x) - (a.x + a.width).min(b.x + b.width)).min(0.0);
                let overlap_y = (a.y.max(b.y) - (a.y + a.height).min(b.y + b.height)).min(0.0);
                let overlap_area = overlap_x * overlap_y;
                assert!(
                    overlap_area < 0.01,
                    "Rects {:?} and {:?} overlap with area {overlap_area}",
                    rects[i],
                    rects[j]
                );
            }
        }

        // Check that all rects are within the window bounds
        for (id, r) in rects {
            assert!(
                r.x >= -0.01 && r.y >= -0.01,
                "Pane {id} has negative position: {:?}",
                r
            );
            assert!(
                r.x + r.width <= window.width + 0.01
                    && r.y + r.height <= window.height + 0.01,
                "Pane {id} exceeds window bounds: {:?}",
                r
            );
        }
    }

    // ──────────────────────────────────────────
    // Border drag changes ratio
    // ──────────────────────────────────────────

    #[test]
    fn test_border_drag_changes_ratio_horizontal() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        // Set up for dragging: begin drag near the border at x=400
        layout.begin_drag(Vec2::new(400.0, 300.0), WINDOW);
        assert!(layout.active_drag.is_some());

        // Drag border to x=600 (75% of width)
        layout.drag_border(Vec2::new(600.0, 300.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let right = rects.iter().find(|(id, _)| *id == pane2).unwrap();

        assert!(
            approx_eq(left.1.width, 600.0),
            "Expected left width ~600, got {}",
            left.1.width
        );
        assert!(
            approx_eq(right.1.width, 200.0),
            "Expected right width ~200, got {}",
            right.1.width
        );
        assert!(approx_eq(right.1.x, 600.0));

        // Verify tiling is still correct
        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_border_drag_changes_ratio_vertical() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Vertical);

        // Begin drag near the border at y=300
        layout.begin_drag(Vec2::new(400.0, 300.0), WINDOW);
        assert!(layout.active_drag.is_some());

        // Drag border to y=150 (25% of height)
        layout.drag_border(Vec2::new(400.0, 150.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let top = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let bottom = rects.iter().find(|(id, _)| *id == pane2).unwrap();

        assert!(
            approx_eq(top.1.height, 150.0),
            "Expected top height ~150, got {}",
            top.1.height
        );
        assert!(
            approx_eq(bottom.1.height, 450.0),
            "Expected bottom height ~450, got {}",
            bottom.1.height
        );

        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_border_drag_clamps_min_ratio() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.begin_drag(Vec2::new(400.0, 300.0), WINDOW);

        // Try to drag all the way to the left edge (ratio ~0)
        layout.drag_border(Vec2::new(0.0, 300.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();

        // The left pane width should be at least MIN_RATIO * 800 = 80
        assert!(
            left.1.width >= 800.0 * MIN_RATIO - 0.01,
            "Left width {} is less than minimum {}",
            left.1.width,
            800.0 * MIN_RATIO
        );
    }

    #[test]
    fn test_border_drag_clamps_max_ratio() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.begin_drag(Vec2::new(400.0, 300.0), WINDOW);

        // Try to drag all the way to the right edge (ratio ~1)
        layout.drag_border(Vec2::new(800.0, 300.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let right = rects.iter().find(|(id, _)| *id == pane2).unwrap();

        // The right pane width should be at least MIN_RATIO * 800 = 80
        assert!(
            right.1.width >= 800.0 * MIN_RATIO - 0.01,
            "Right width {} is less than minimum {}",
            right.1.width,
            800.0 * MIN_RATIO
        );
    }

    #[test]
    fn test_begin_drag_miss() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let _pane2 = layout.split(pane1, SplitDirection::Horizontal);

        // Click far from the border (border is at x=400, click at x=100)
        layout.begin_drag(Vec2::new(100.0, 300.0), WINDOW);
        assert!(layout.active_drag.is_none());
    }

    // ──────────────────────────────────────────
    // Border drag on nested layout
    // ──────────────────────────────────────────

    #[test]
    fn test_border_drag_nested() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane2, SplitDirection::Vertical);

        // The inner border (between pane2 and pane3) is at y=300 within the right half (x=400..800)
        // Click near that inner border
        layout.begin_drag(Vec2::new(600.0, 300.0), WINDOW);
        assert!(layout.active_drag.is_some());

        // Drag the inner border down to y=450 (75% of 600 height)
        layout.drag_border(Vec2::new(600.0, 450.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[], None);

        // pane1 should be unaffected (left half)
        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(
            &r1.1,
            &Rect::new(0.0, 0.0, 400.0, 600.0)
        ));

        // pane2 should now be taller (top-right, 75% of height)
        let r2 = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(approx_eq(r2.1.height, 450.0), "got {}", r2.1.height);

        // pane3 should be shorter (bottom-right, 25% of height)
        let r3 = rects.iter().find(|(id, _)| *id == pane3).unwrap();
        assert!(approx_eq(r3.1.height, 150.0), "got {}", r3.1.height);

        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    // ──────────────────────────────────────────
    // PaneId generation
    // ──────────────────────────────────────────

    #[test]
    fn test_pane_ids_are_unique() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane2, SplitDirection::Vertical);
        let pane4 = layout.split(pane1, SplitDirection::Vertical);

        let mut ids = vec![pane1, pane2, pane3, pane4];
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 4, "All pane IDs must be unique");
    }

    #[test]
    fn test_pane_ids_list() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane2, SplitDirection::Vertical);

        let mut ids = layout.pane_ids();
        ids.sort();
        let mut expected = vec![pane1, pane2, pane3];
        expected.sort();
        assert_eq!(ids, expected);
    }

    // ──────────────────────────────────────────
    // Edge cases
    // ──────────────────────────────────────────

    #[test]
    fn test_split_nonexistent_pane() {
        let (mut layout, _pane1) = SplitLayout::with_initial_pane();
        // Splitting a non-existent pane should still return a new ID
        // but not modify the tree
        let new_id = layout.split(999, SplitDirection::Horizontal);
        assert!(new_id > 0);
        // Tree should still have just one pane
        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 1);
    }

    #[test]
    fn test_remove_and_resplit() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        // Remove pane2
        layout.remove(pane2);
        assert_eq!(layout.pane_ids().len(), 1);

        // Split pane1 again
        let pane3 = layout.split(pane1, SplitDirection::Vertical);
        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 2);

        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let r3 = rects.iter().find(|(id, _)| *id == pane3).unwrap();
        assert!(approx_eq(r1.1.height, 300.0));
        assert!(approx_eq(r3.1.height, 300.0));
        assert!(approx_eq(r1.1.width, 800.0));
        assert!(approx_eq(r3.1.width, 800.0));
    }

    #[test]
    fn test_different_window_sizes() {
        let (layout, pane1) = SplitLayout::with_initial_pane();
        let small = Size::new(100.0, 50.0);
        let rects = layout.compute(small, &[pane1], None);
        assert!(rect_approx_eq(
            &rects[0].1,
            &Rect::new(0.0, 0.0, 100.0, 50.0)
        ));

        let large = Size::new(3840.0, 2160.0);
        let rects = layout.compute(large, &[pane1], None);
        assert!(rect_approx_eq(
            &rects[0].1,
            &Rect::new(0.0, 0.0, 3840.0, 2160.0)
        ));
    }

    #[test]
    fn test_drag_border_without_begin_uses_autodetect() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        // Set last_window_size manually to enable auto-detect in drag_border
        layout.last_window_size = Some(WINDOW);

        // Drag near the border (auto-detect should find it)
        layout.drag_border(Vec2::new(600.0, 300.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        // The ratio should have changed towards 0.75
        assert!(
            approx_eq(left.1.width, 600.0),
            "Expected ~600, got {}",
            left.1.width
        );
    }

    // ──────────────────────────────────────────
    // Contains helper
    // ──────────────────────────────────────────

    #[test]
    fn test_node_contains() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        if let Some(ref root) = layout.root {
            assert!(root.contains(pane1));
            assert!(root.contains(pane2));
            assert!(!root.contains(999));
        }
    }

    // ──────────────────────────────────────────
    // Default trait
    // ──────────────────────────────────────────

    #[test]
    fn test_default() {
        let layout = SplitLayout::default();
        assert!(layout.root.is_none());
        let rects = layout.compute(WINDOW, &[], None);
        assert!(rects.is_empty());
    }
}
