use tide_core::{PaneDecorations, PaneId, Rect, Size, SplitDirection, Vec2};

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

    /// Count the number of leaf panes reachable through consecutive same-direction splits.
    /// A node with a different split direction or a leaf counts as 1.
    pub(crate) fn count_chain_leaves(&self, dir: SplitDirection) -> usize {
        match self {
            Node::Leaf(_) => 1,
            Node::Split { direction, left, right, .. } if *direction == dir => {
                left.count_chain_leaves(dir) + right.count_chain_leaves(dir)
            }
            _ => 1,
        }
    }

    /// Replace a leaf with a split node containing the original leaf and a new leaf.
    /// When the new split has the same direction as a parent split, ratios are
    /// adjusted so all leaves in the same-direction chain get equal space.
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
            Node::Split { direction: dir, ratio, left, right, .. } => {
                if left.split_pane(target, new_id, direction) {
                    if *dir == direction {
                        let n_left = left.count_chain_leaves(*dir);
                        let n_right = right.count_chain_leaves(*dir);
                        *ratio = n_left as f32 / (n_left + n_right) as f32;
                    }
                    return true;
                }
                if right.split_pane(target, new_id, direction) {
                    if *dir == direction {
                        let n_left = left.count_chain_leaves(*dir);
                        let n_right = right.count_chain_leaves(*dir);
                        *ratio = n_left as f32 / (n_left + n_right) as f32;
                    }
                    return true;
                }
                false
            }
        }
    }

    /// Remove a pane from the tree. Returns:
    /// - Some(Some(node)) if the pane was found and a sibling remains
    /// - Some(None) if the pane was found and this entire node should be removed (leaf case)
    /// - None if the pane was not found in this subtree
    ///
    /// When removal changes the same-direction chain leaf count at a split node,
    /// the ratio is re-equalized so columns/rows stay balanced.
    pub(crate) fn remove_pane(&mut self, target: PaneId) -> Option<Option<Node>> {
        match self {
            Node::Leaf(id) if *id == target => {
                // This leaf should be removed; the parent must handle collapsing.
                Some(None)
            }
            Node::Leaf(_) => None,
            Node::Split { direction, ratio, left, right } => {
                let dir = *direction;

                // Try removing from left child
                let left_old = left.count_chain_leaves(dir);
                if let Some(replacement) = left.remove_pane(target) {
                    return match replacement {
                        Some(node) => {
                            **left = node;
                            if left.count_chain_leaves(dir) != left_old {
                                let nl = left.count_chain_leaves(dir);
                                let nr = right.count_chain_leaves(dir);
                                *ratio = nl as f32 / (nl + nr) as f32;
                            }
                            Some(Some(self.clone()))
                        }
                        None => {
                            // Left child is gone; replace this split with right child.
                            Some(Some(right.as_ref().clone()))
                        }
                    };
                }
                // Try removing from right child
                let right_old = right.count_chain_leaves(dir);
                if let Some(replacement) = right.remove_pane(target) {
                    return match replacement {
                        Some(node) => {
                            **right = node;
                            if right.count_chain_leaves(dir) != right_old {
                                let nl = left.count_chain_leaves(dir);
                                let nr = right.count_chain_leaves(dir);
                                *ratio = nl as f32 / (nl + nr) as f32;
                            }
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

    /// Recursively snap split ratios so that the left/top child's content area
    /// aligns to a whole number of cells.
    ///
    /// For each split node the algorithm:
    /// 1. Computes the left child's tiling rect from the current ratio
    /// 2. Derives the content width/height (subtracting gap + padding)
    /// 3. Rounds to the nearest cell boundary
    /// 4. Adjusts the ratio accordingly, clamped to dynamic min/max
    /// 5. Recurses into both children
    pub(crate) fn snap_ratios(
        &mut self,
        rect: Rect,
        cell_size: Size,
        decorations: &PaneDecorations,
    ) {
        if let Node::Split {
            direction,
            ratio,
            left,
            right,
        } = self
        {
            let half_gap = decorations.gap / 2.0;

            match direction {
                SplitDirection::Horizontal => {
                    let total = rect.width;
                    if total < 1.0 || cell_size.width < 1.0 {
                        return;
                    }
                    // Left child tiling width
                    let left_tiling_w = total * *ratio;
                    // Content width: tiling_width - interior gap/2 - padding*2
                    let content_w = left_tiling_w - half_gap - 2.0 * decorations.padding;
                    if content_w > 0.0 {
                        let snapped_w = (content_w / cell_size.width).round() * cell_size.width;
                        let new_tiling_w = snapped_w + half_gap + 2.0 * decorations.padding;
                        let new_ratio = new_tiling_w / total;
                        let min_r = min_ratio_for_direction(
                            rect,
                            cell_size,
                            decorations,
                            SplitDirection::Horizontal,
                        );
                        *ratio = new_ratio.clamp(min_r, 1.0 - min_r);
                    }
                }
                SplitDirection::Vertical => {
                    let total = rect.height;
                    if total < 1.0 || cell_size.height < 1.0 {
                        return;
                    }
                    let left_tiling_h = total * *ratio;
                    // Content height: tiling_height - interior gap/2 - tab_bar - padding
                    let content_h =
                        left_tiling_h - half_gap - decorations.tab_bar_height - decorations.padding;
                    if content_h > 0.0 {
                        let snapped_h = (content_h / cell_size.height).round() * cell_size.height;
                        let new_tiling_h =
                            snapped_h + half_gap + decorations.tab_bar_height + decorations.padding;
                        let new_ratio = new_tiling_h / total;
                        let min_r = min_ratio_for_direction(
                            rect,
                            cell_size,
                            decorations,
                            SplitDirection::Vertical,
                        );
                        *ratio = new_ratio.clamp(min_r, 1.0 - min_r);
                    }
                }
            }

            let (left_rect, right_rect) = split_rect(rect, *direction, *ratio);
            left.snap_ratios(left_rect, cell_size, decorations);
            right.snap_ratios(right_rect, cell_size, decorations);
        }
    }

    /// Replace the leaf containing `target` with a split containing both
    /// `target` and `new_pane`. `insert_first` controls whether the new pane
    /// goes into the left/top (true) or right/bottom (false) child.
    /// When the new split has the same direction as a parent split, ratios are
    /// adjusted so all leaves in the same-direction chain get equal space.
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
            Node::Split { direction: dir, ratio, left, right, .. } => {
                if left.insert_pane_at(target, new_pane, direction, insert_first) {
                    if *dir == direction {
                        let n_left = left.count_chain_leaves(*dir);
                        let n_right = right.count_chain_leaves(*dir);
                        *ratio = n_left as f32 / (n_left + n_right) as f32;
                    }
                    return true;
                }
                if right.insert_pane_at(target, new_pane, direction, insert_first) {
                    if *dir == direction {
                        let n_left = left.count_chain_leaves(*dir);
                        let n_right = right.count_chain_leaves(*dir);
                        *ratio = n_left as f32 / (n_left + n_right) as f32;
                    }
                    return true;
                }
                false
            }
        }
    }
}

// ──────────────────────────────────────────────
// Tree reconstruction from rects
// ──────────────────────────────────────────────

/// Try to find a clean split along the given axis.
/// Returns (left_group, right_group, ratio) if a clean partition exists.
fn try_split(
    pane_rects: &[(PaneId, Rect)],
    direction: SplitDirection,
) -> Option<(Vec<(PaneId, Rect)>, Vec<(PaneId, Rect)>, f32)> {
    if pane_rects.len() < 2 {
        return None;
    }

    // Compute bounding box
    let mut min_x = f32::MAX;
    let mut min_y = f32::MAX;
    let mut max_x = f32::MIN;
    let mut max_y = f32::MIN;
    for (_, r) in pane_rects {
        min_x = min_x.min(r.x);
        min_y = min_y.min(r.y);
        max_x = max_x.max(r.x + r.width);
        max_y = max_y.max(r.y + r.height);
    }

    // Collect candidate split positions (right/bottom edges of panes, excluding bbox edge)
    let mut candidates: Vec<f32> = Vec::new();
    let eps = 0.5;

    match direction {
        SplitDirection::Horizontal => {
            for (_, r) in pane_rects {
                let edge = r.x + r.width;
                if (edge - max_x).abs() > eps {
                    // Not the bounding box edge
                    if !candidates.iter().any(|&c| (c - edge).abs() < eps) {
                        candidates.push(edge);
                    }
                }
            }
        }
        SplitDirection::Vertical => {
            for (_, r) in pane_rects {
                let edge = r.y + r.height;
                if (edge - max_y).abs() > eps {
                    if !candidates.iter().any(|&c| (c - edge).abs() < eps) {
                        candidates.push(edge);
                    }
                }
            }
        }
    }

    // Try each candidate: partition panes into left/right (or top/bottom)
    for split_pos in candidates {
        let mut left_group = Vec::new();
        let mut right_group = Vec::new();
        let mut clean = true;

        for &(id, r) in pane_rects {
            match direction {
                SplitDirection::Horizontal => {
                    let pane_left = r.x;
                    let pane_right = r.x + r.width;
                    if pane_right <= split_pos + eps {
                        left_group.push((id, r));
                    } else if pane_left >= split_pos - eps {
                        right_group.push((id, r));
                    } else {
                        // Pane straddles the split
                        clean = false;
                        break;
                    }
                }
                SplitDirection::Vertical => {
                    let pane_top = r.y;
                    let pane_bottom = r.y + r.height;
                    if pane_bottom <= split_pos + eps {
                        left_group.push((id, r));
                    } else if pane_top >= split_pos - eps {
                        right_group.push((id, r));
                    } else {
                        clean = false;
                        break;
                    }
                }
            }
        }

        if clean && !left_group.is_empty() && !right_group.is_empty() {
            // Compute ratio from original bounding box sizes
            let ratio = match direction {
                SplitDirection::Horizontal => (split_pos - min_x) / (max_x - min_x),
                SplitDirection::Vertical => (split_pos - min_y) / (max_y - min_y),
            };
            return Some((left_group, right_group, ratio));
        }
    }

    None
}

/// Build a tree from (PaneId, Rect) pairs, preferring splits along the primary direction.
/// Used for tree restructuring during drag-and-drop moves.
pub(crate) fn build_tree_from_rects(
    pane_rects: &[(PaneId, Rect)],
    primary: SplitDirection,
) -> Option<Node> {
    match pane_rects.len() {
        0 => None,
        1 => Some(Node::Leaf(pane_rects[0].0)),
        _ => {
            let secondary = match primary {
                SplitDirection::Horizontal => SplitDirection::Vertical,
                SplitDirection::Vertical => SplitDirection::Horizontal,
            };

            // Try primary axis first, then secondary
            let (direction, left_group, right_group, ratio) =
                if let Some((l, r, rat)) = try_split(pane_rects, primary) {
                    (primary, l, r, rat)
                } else if let Some((l, r, rat)) = try_split(pane_rects, secondary) {
                    (secondary, l, r, rat)
                } else {
                    // Fallback below
                    return {
                        let mut node = Node::Leaf(pane_rects[0].0);
                        for &(id, _) in &pane_rects[1..] {
                            node = Node::Split {
                                direction: primary,
                                ratio: 0.5,
                                left: Box::new(node),
                                right: Box::new(Node::Leaf(id)),
                            };
                        }
                        Some(node)
                    };
                };

            let left = build_tree_from_rects(&left_group, primary)?;
            let right = build_tree_from_rects(&right_group, primary)?;
            Some(Node::Split {
                direction,
                ratio,
                left: Box::new(left),
                right: Box::new(right),
            })
        }
    }
}

// ──────────────────────────────────────────────
// Helpers
// ──────────────────────────────────────────────

/// Minimum number of columns/rows a pane must contain.
const MIN_COLS: f32 = 4.0;
const MIN_ROWS: f32 = 2.0;

/// Compute the minimum ratio for a split so that neither child is smaller than
/// MIN_COLS/MIN_ROWS cells (accounting for decorations).
pub(crate) fn min_ratio_for_direction(
    rect: Rect,
    cell_size: Size,
    decorations: &PaneDecorations,
    direction: SplitDirection,
) -> f32 {
    let half_gap = decorations.gap / 2.0;
    match direction {
        SplitDirection::Horizontal => {
            if rect.width < 1.0 {
                return 0.1;
            }
            let min_tiling_w = MIN_COLS * cell_size.width + half_gap + 2.0 * decorations.padding;
            (min_tiling_w / rect.width).clamp(0.05, 0.45)
        }
        SplitDirection::Vertical => {
            if rect.height < 1.0 {
                return 0.1;
            }
            let min_tiling_h =
                MIN_ROWS * cell_size.height + half_gap + decorations.tab_bar_height + decorations.padding;
            (min_tiling_h / rect.height).clamp(0.05, 0.45)
        }
    }
}

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
