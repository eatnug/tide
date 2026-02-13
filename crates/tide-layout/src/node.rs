use tide_core::{PaneId, Rect, SplitDirection, Vec2};

// ──────────────────────────────────────────────
// Node: binary tree for layout
// ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub(crate) enum Node {
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
    #[cfg(test)]
    pub(crate) fn contains(&self, pane: PaneId) -> bool {
        match self {
            Node::Leaf(id) => *id == pane,
            Node::Split { left, right, .. } => left.contains(pane) || right.contains(pane),
        }
    }

    /// Collect all leaf PaneIds in this subtree.
    pub(crate) fn pane_ids(&self, out: &mut Vec<PaneId>) {
        match self {
            Node::Leaf(id) => out.push(*id),
            Node::Split { left, right, .. } => {
                left.pane_ids(out);
                right.pane_ids(out);
            }
        }
    }

    /// Traverse the tree and compute the rect for every leaf pane.
    pub(crate) fn compute_rects(&self, rect: Rect, out: &mut Vec<(PaneId, Rect)>) {
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
    pub(crate) fn split_pane(
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
    pub(crate) fn remove_pane(&mut self, target: PaneId) -> Option<Option<Node>> {
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
                            **left = node;
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
                            **right = node;
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
    /// the rect this node occupies.
    pub(crate) fn find_border_at(
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
    pub(crate) fn apply_drag(&mut self, rect: Rect, path: &[bool], position: Vec2, min_ratio: f32) {
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

    /// Replace all occurrences of `from` PaneId with `to` in leaf nodes.
    pub(crate) fn replace_pane_id(&mut self, from: PaneId, to: PaneId) {
        match self {
            Node::Leaf(id) if *id == from => *id = to,
            Node::Leaf(_) => {}
            Node::Split { left, right, .. } => {
                left.replace_pane_id(from, to);
                right.replace_pane_id(from, to);
            }
        }
    }

    /// Swap two pane IDs using a sentinel value for 3-way swap.
    pub(crate) fn swap_panes(&mut self, a: PaneId, b: PaneId) {
        let sentinel = u64::MAX;
        self.replace_pane_id(a, sentinel);
        self.replace_pane_id(b, a);
        self.replace_pane_id(sentinel, b);
    }

    /// Replace the leaf containing `target` with a split containing both
    /// `target` and `new_pane`. `insert_first` controls whether the new pane
    /// goes into the left/top (true) or right/bottom (false) child.
    pub(crate) fn insert_pane_at(
        &mut self,
        target: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
        insert_first: bool,
    ) -> bool {
        match self {
            Node::Leaf(id) if *id == target => {
                let target_node = Node::Leaf(target);
                let new_node = Node::Leaf(new_pane);
                let (left, right) = if insert_first {
                    (new_node, target_node)
                } else {
                    (target_node, new_node)
                };
                *self = Node::Split {
                    direction,
                    ratio: 0.5,
                    left: Box::new(left),
                    right: Box::new(right),
                };
                true
            }
            Node::Leaf(_) => false,
            Node::Split { left, right, .. } => {
                if left.insert_pane_at(target, new_pane, direction, insert_first) {
                    return true;
                }
                right.insert_pane_at(target, new_pane, direction, insert_first)
            }
        }
    }
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Split a rect into two sub-rects based on direction and ratio.
pub(crate) fn split_rect(rect: Rect, direction: SplitDirection, ratio: f32) -> (Rect, Rect) {
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
