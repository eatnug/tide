#[cfg(test)]
mod tests {
    use crate::SplitLayout;
    use tide_core::{LayoutEngine, PaneDecorations, Rect, Size, SplitDirection, Vec2};

    const WINDOW: Size = Size {
        width: 800.0,
        height: 600.0,
    };

    /// Minimum split ratio (mirrors the constant in the main module).
    const MIN_RATIO: f32 = 0.1;

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
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane2, SplitDirection::Vertical);

        let rects = layout.compute(WINDOW, &[pane1, pane2, pane3], None);
        assert_eq!(rects.len(), 3);

        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(&r1.1, &Rect::new(0.0, 0.0, 400.0, 600.0)));

        let r2 = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(rect_approx_eq(&r2.1, &Rect::new(400.0, 0.0, 400.0, 300.0)));

        let r3 = rects.iter().find(|(id, _)| *id == pane3).unwrap();
        assert!(rect_approx_eq(&r3.1, &Rect::new(400.0, 300.0, 400.0, 300.0)));
    }

    #[test]
    fn test_deeply_nested_splits() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane1, SplitDirection::Vertical);
        let pane4 = layout.split(pane2, SplitDirection::Vertical);

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);

        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(&r1.1, &Rect::new(0.0, 0.0, 400.0, 300.0)));
        let r3 = rects.iter().find(|(id, _)| *id == pane3).unwrap();
        assert!(rect_approx_eq(&r3.1, &Rect::new(0.0, 300.0, 400.0, 300.0)));

        let r2 = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(rect_approx_eq(&r2.1, &Rect::new(400.0, 0.0, 400.0, 300.0)));
        let r4 = rects.iter().find(|(id, _)| *id == pane4).unwrap();
        assert!(rect_approx_eq(&r4.1, &Rect::new(400.0, 300.0, 400.0, 300.0)));
    }

    // ──────────────────────────────────────────
    // Remove pane collapses the split
    // ──────────────────────────────────────────

    #[test]
    fn test_remove_pane_collapses_split() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.remove(pane2);
        let rects = layout.compute(WINDOW, &[pane1], None);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].0, pane1);
        assert!(rect_approx_eq(&rects[0].1, &Rect::new(0.0, 0.0, 800.0, 600.0)));
    }

    #[test]
    fn test_remove_left_pane_collapses_to_right() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.remove(pane1);
        let rects = layout.compute(WINDOW, &[pane2], None);
        assert_eq!(rects.len(), 1);
        assert_eq!(rects[0].0, pane2);
        assert!(rect_approx_eq(&rects[0].1, &Rect::new(0.0, 0.0, 800.0, 600.0)));
    }

    #[test]
    fn test_remove_from_nested() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane2, SplitDirection::Vertical);

        layout.remove(pane3);
        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        assert_eq!(rects.len(), 2);

        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(&r1.1, &Rect::new(0.0, 0.0, 400.0, 600.0)));

        let r2 = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(rect_approx_eq(&r2.1, &Rect::new(400.0, 0.0, 400.0, 600.0)));
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

    fn assert_no_gaps_no_overlaps(rects: &[(tide_core::PaneId, Rect)], window: Size) {
        let window_area = window.width * window.height;

        let total_area: f32 = rects.iter().map(|(_, r)| r.width * r.height).sum();
        assert!(
            approx_eq(total_area, window_area),
            "Total area {total_area} != window area {window_area}"
        );

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

        layout.begin_drag(Vec2::new(400.0, 300.0), WINDOW);
        assert!(layout.active_drag.is_some());

        layout.drag_border(Vec2::new(600.0, 300.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let right = rects.iter().find(|(id, _)| *id == pane2).unwrap();

        assert!(approx_eq(left.1.width, 600.0), "Expected left width ~600, got {}", left.1.width);
        assert!(approx_eq(right.1.width, 200.0), "Expected right width ~200, got {}", right.1.width);
        assert!(approx_eq(right.1.x, 600.0));

        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_border_drag_changes_ratio_vertical() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Vertical);

        layout.begin_drag(Vec2::new(400.0, 300.0), WINDOW);
        assert!(layout.active_drag.is_some());

        layout.drag_border(Vec2::new(400.0, 150.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let top = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let bottom = rects.iter().find(|(id, _)| *id == pane2).unwrap();

        assert!(approx_eq(top.1.height, 150.0), "Expected top height ~150, got {}", top.1.height);
        assert!(approx_eq(bottom.1.height, 450.0), "Expected bottom height ~450, got {}", bottom.1.height);

        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_border_drag_clamps_min_ratio() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.begin_drag(Vec2::new(400.0, 300.0), WINDOW);
        layout.drag_border(Vec2::new(0.0, 300.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();

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
        layout.drag_border(Vec2::new(800.0, 300.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let right = rects.iter().find(|(id, _)| *id == pane2).unwrap();

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

        layout.begin_drag(Vec2::new(600.0, 300.0), WINDOW);
        assert!(layout.active_drag.is_some());

        layout.drag_border(Vec2::new(600.0, 450.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[], None);

        let r1 = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(rect_approx_eq(&r1.1, &Rect::new(0.0, 0.0, 400.0, 600.0)));

        let r2 = rects.iter().find(|(id, _)| *id == pane2).unwrap();
        assert!(approx_eq(r2.1.height, 450.0), "got {}", r2.1.height);

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
        let new_id = layout.split(999, SplitDirection::Horizontal);
        assert!(new_id > 0);
        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 1);
    }

    #[test]
    fn test_remove_and_resplit() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.remove(pane2);
        assert_eq!(layout.pane_ids().len(), 1);

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
        assert!(rect_approx_eq(&rects[0].1, &Rect::new(0.0, 0.0, 100.0, 50.0)));

        let large = Size::new(3840.0, 2160.0);
        let rects = layout.compute(large, &[pane1], None);
        assert!(rect_approx_eq(&rects[0].1, &Rect::new(0.0, 0.0, 3840.0, 2160.0)));
    }

    #[test]
    fn test_drag_border_without_begin_uses_autodetect() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.last_window_size = Some(WINDOW);

        layout.drag_border(Vec2::new(600.0, 300.0));
        layout.end_drag();

        let rects = layout.compute(WINDOW, &[pane1, pane2], None);
        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        assert!(approx_eq(left.1.width, 600.0), "Expected ~600, got {}", left.1.width);
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

    // ──────────────────────────────────────────
    // move_pane_to_root
    // ──────────────────────────────────────────

    #[test]
    fn test_move_pane_to_root_left_creates_horizontal_split() {
        // V(A, V(B, C)) → move A to root-left → H(A, V(B, C))
        let (mut layout, pane_a) = SplitLayout::with_initial_pane();
        let pane_b = layout.split(pane_a, SplitDirection::Vertical);
        let pane_c = layout.split(pane_b, SplitDirection::Vertical);

        assert!(layout.move_pane_to_root(pane_a, tide_core::DropZone::Left));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 3);

        // A should be on the left half
        let ra = rects.iter().find(|(id, _)| *id == pane_a).unwrap();
        assert!(approx_eq(ra.1.x, 0.0));
        assert!(approx_eq(ra.1.width, 400.0));
        assert!(approx_eq(ra.1.height, 600.0));

        // B and C should share the right half vertically
        let rb = rects.iter().find(|(id, _)| *id == pane_b).unwrap();
        let rc = rects.iter().find(|(id, _)| *id == pane_c).unwrap();
        assert!(approx_eq(rb.1.x, 400.0));
        assert!(approx_eq(rc.1.x, 400.0));
        assert!(approx_eq(rb.1.width, 400.0));
        assert!(approx_eq(rc.1.width, 400.0));

        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_move_pane_to_root_integrity() {
        // V(A, V(B, C)) → move A to root-right → H(V(B,C), A)
        let (mut layout, pane_a) = SplitLayout::with_initial_pane();
        let pane_b = layout.split(pane_a, SplitDirection::Vertical);
        let _pane_c = layout.split(pane_b, SplitDirection::Vertical);

        assert!(layout.move_pane_to_root(pane_a, tide_core::DropZone::Right));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 3);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // A should be on the right half
        let ra = rects.iter().find(|(id, _)| *id == pane_a).unwrap();
        assert!(approx_eq(ra.1.x, 400.0));
        assert!(approx_eq(ra.1.width, 400.0));
    }

    #[test]
    fn test_move_pane_to_root_single_pane_noop() {
        let (mut layout, pane_a) = SplitLayout::with_initial_pane();

        // Single pane: root move should be a no-op
        assert!(!layout.move_pane_to_root(pane_a, tide_core::DropZone::Left));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 1);
        assert!(rect_approx_eq(&rects[0].1, &Rect::new(0.0, 0.0, 800.0, 600.0)));
    }

    #[test]
    fn test_move_pane_to_root_two_panes() {
        // H(A, B) → move B to root-top → V(B, A)
        let (mut layout, pane_a) = SplitLayout::with_initial_pane();
        let pane_b = layout.split(pane_a, SplitDirection::Horizontal);

        assert!(layout.move_pane_to_root(pane_b, tide_core::DropZone::Top));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 2);

        // B should be on top
        let rb = rects.iter().find(|(id, _)| *id == pane_b).unwrap();
        assert!(approx_eq(rb.1.y, 0.0));
        assert!(approx_eq(rb.1.height, 300.0));
        assert!(approx_eq(rb.1.width, 800.0));

        // A should be on bottom
        let ra = rects.iter().find(|(id, _)| *id == pane_a).unwrap();
        assert!(approx_eq(ra.1.y, 300.0));
        assert!(approx_eq(ra.1.height, 300.0));
        assert!(approx_eq(ra.1.width, 800.0));

        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_move_pane_to_root_bottom() {
        // V(A, B) → move A to root-bottom → V(B, A)
        let (mut layout, pane_a) = SplitLayout::with_initial_pane();
        let pane_b = layout.split(pane_a, SplitDirection::Vertical);

        assert!(layout.move_pane_to_root(pane_a, tide_core::DropZone::Bottom));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 2);

        // B should be on top, A on bottom
        let rb = rects.iter().find(|(id, _)| *id == pane_b).unwrap();
        assert!(approx_eq(rb.1.y, 0.0));
        let ra = rects.iter().find(|(id, _)| *id == pane_a).unwrap();
        assert!(approx_eq(ra.1.y, 300.0));

        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_move_pane_to_root_center_returns_false() {
        let (mut layout, pane_a) = SplitLayout::with_initial_pane();
        let _pane_b = layout.split(pane_a, SplitDirection::Horizontal);

        assert!(!layout.move_pane_to_root(pane_a, tide_core::DropZone::Center));
    }

    // ──────────────────────────────────────────
    // Cell-aligned snap
    // ──────────────────────────────────────────

    const CELL: Size = Size {
        width: 8.0,
        height: 16.0,
    };

    const DECORATIONS: PaneDecorations = PaneDecorations {
        gap: 4.0,
        padding: 6.0,
        tab_bar_height: 30.0,
    };

    /// Helper: compute content width from a visual pane rect.
    /// content_w = tiling_width - gap/2 - 2*padding (interior pane assumption)
    fn content_width_from_tiling(tiling_w: f32) -> f32 {
        tiling_w - DECORATIONS.gap / 2.0 - 2.0 * DECORATIONS.padding
    }

    fn content_height_from_tiling(tiling_h: f32) -> f32 {
        tiling_h - DECORATIONS.gap / 2.0 - DECORATIONS.tab_bar_height - DECORATIONS.padding
    }

    #[test]
    fn test_snap_horizontal_split_aligns_to_cells() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        // Snap ratios
        layout.snap_ratios_to_cells(WINDOW, CELL, &DECORATIONS);
        let rects = layout.compute(WINDOW, &[pane1, pane2], None);

        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let cw = content_width_from_tiling(left.1.width);
        let cols = cw / CELL.width;
        assert!(
            (cols - cols.round()).abs() < 0.01,
            "Left content width {} does not align to cell width {}: cols = {}",
            cw,
            CELL.width,
            cols
        );
    }

    #[test]
    fn test_snap_vertical_split_aligns_to_cells() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Vertical);

        layout.snap_ratios_to_cells(WINDOW, CELL, &DECORATIONS);
        let rects = layout.compute(WINDOW, &[pane1, pane2], None);

        let top = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let ch = content_height_from_tiling(top.1.height);
        let rows = ch / CELL.height;
        assert!(
            (rows - rows.round()).abs() < 0.01,
            "Top content height {} does not align to cell height {}: rows = {}",
            ch,
            CELL.height,
            rows
        );
    }

    #[test]
    fn test_snap_50_50_split_equal_cols() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.snap_ratios_to_cells(WINDOW, CELL, &DECORATIONS);
        let rects = layout.compute(WINDOW, &[pane1, pane2], None);

        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let right = rects.iter().find(|(id, _)| *id == pane2).unwrap();

        let left_cols = (content_width_from_tiling(left.1.width) / CELL.width).round() as i32;
        let right_cols = (content_width_from_tiling(right.1.width) / CELL.width).round() as i32;

        assert!(
            (left_cols - right_cols).abs() <= 1,
            "50:50 split should have equal cols (±1): left={}, right={}",
            left_cols,
            right_cols
        );
    }

    #[test]
    fn test_snap_preserves_tiling() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        layout.snap_ratios_to_cells(WINDOW, CELL, &DECORATIONS);
        let rects = layout.compute(WINDOW, &[pane1, pane2], None);

        // Rects should still tile the window (no gaps, no overlaps)
        assert_no_gaps_no_overlaps(&rects, WINDOW);
    }

    #[test]
    fn test_snap_nested_splits() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);
        let pane3 = layout.split(pane2, SplitDirection::Vertical);

        layout.snap_ratios_to_cells(WINDOW, CELL, &DECORATIONS);
        let rects = layout.compute(WINDOW, &[pane1, pane2, pane3], None);

        assert_eq!(rects.len(), 3);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // Check that left pane content is cell-aligned
        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let cw = content_width_from_tiling(left.1.width);
        let cols = cw / CELL.width;
        assert!(
            (cols - cols.round()).abs() < 0.01,
            "Nested: left content width not cell-aligned: cols = {}",
            cols
        );
    }

    #[test]
    fn test_snap_single_pane_is_noop() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();

        layout.snap_ratios_to_cells(WINDOW, CELL, &DECORATIONS);
        let rects = layout.compute(WINDOW, &[pane1], None);

        assert_eq!(rects.len(), 1);
        assert!(rect_approx_eq(
            &rects[0].1,
            &Rect::new(0.0, 0.0, 800.0, 600.0)
        ));
    }

    #[test]
    fn test_snap_respects_min_pane_size() {
        let (mut layout, pane1) = SplitLayout::with_initial_pane();
        let pane2 = layout.split(pane1, SplitDirection::Horizontal);

        // Drag border to extreme left
        layout.last_window_size = Some(WINDOW);
        layout.begin_drag(Vec2::new(400.0, 300.0), WINDOW);
        layout.drag_border(Vec2::new(10.0, 300.0));
        layout.end_drag();

        layout.snap_ratios_to_cells(WINDOW, CELL, &DECORATIONS);
        let rects = layout.compute(WINDOW, &[pane1, pane2], None);

        let left = rects.iter().find(|(id, _)| *id == pane1).unwrap();
        let cw = content_width_from_tiling(left.1.width);
        let cols = cw / CELL.width;

        // Should have at least MIN_COLS (4) columns
        assert!(
            cols >= 3.5,
            "Left pane too small after snap: cols = {}",
            cols
        );
    }

    // ──────────────────────────────────────────
    // Helper: 4-quadrant layout for restructure tests
    // ──────────────────────────────────────────

    /// Build a 4-quadrant layout: H(0.5, V(0.5, 2, 3), V(0.5, 1, 4))
    ///
    /// Pane positions in 800x600:
    ///   2(0,0,400,300)   | 1(400,0,400,300)
    ///   3(0,300,400,300)  | 4(400,300,400,300)
    fn make_quadrant_layout() -> SplitLayout {
        use crate::node::Node;

        let root = Node::Split {
            direction: SplitDirection::Horizontal,
            ratio: 0.5,
            left: Box::new(Node::Split {
                direction: SplitDirection::Vertical,
                ratio: 0.5,
                left: Box::new(Node::Leaf(2)),
                right: Box::new(Node::Leaf(3)),
            }),
            right: Box::new(Node::Split {
                direction: SplitDirection::Vertical,
                ratio: 0.5,
                left: Box::new(Node::Leaf(1)),
                right: Box::new(Node::Leaf(4)),
            }),
        };

        SplitLayout {
            root: Some(root),
            next_id: 5,
            active_drag: None,
            last_window_size: None,
        }
    }

    // ──────────────────────────────────────────
    // Restructure: move pane 4 to root-left (Case 1)
    // Expected: 4 | V(2,3) | 1 — 3 equal columns
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_move_to_root_left() {
        let mut layout = make_quadrant_layout();
        assert!(layout.restructure_move_to_root(4, tide_core::DropZone::Left, WINDOW));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // Pane 4 should be on the far left, full height
        let r4 = rects.iter().find(|(id, _)| *id == 4).unwrap();
        assert!(approx_eq(r4.1.x, 0.0), "pane 4 x: {}", r4.1.x);
        assert!(approx_eq(r4.1.height, 600.0), "pane 4 height: {}", r4.1.height);

        // Panes 2 and 3 should be stacked vertically in the middle column
        let r2 = rects.iter().find(|(id, _)| *id == 2).unwrap();
        let r3 = rects.iter().find(|(id, _)| *id == 3).unwrap();
        assert!(approx_eq(r2.1.x, r3.1.x), "2 and 3 should share x");
        assert!(r2.1.y < r3.1.y, "2 should be above 3");

        // Pane 1 should be on the far right, full height
        let r1 = rects.iter().find(|(id, _)| *id == 1).unwrap();
        assert!(approx_eq(r1.1.height, 600.0), "pane 1 height: {}", r1.1.height);
        assert!(r1.1.x > r2.1.x, "pane 1 should be right of pane 2");
    }

    // ──────────────────────────────────────────
    // Restructure: move pane 4 to pane 2 left (Case 2)
    // Expected: H(4,2) over 3 | 1 (left column restructured)
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_move_pane_to_pane2_left() {
        let mut layout = make_quadrant_layout();
        assert!(layout.restructure_move_pane(4, 2, tide_core::DropZone::Left, WINDOW));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // Pane 4 should be to the left of pane 2
        let r4 = rects.iter().find(|(id, _)| *id == 4).unwrap();
        let r2 = rects.iter().find(|(id, _)| *id == 2).unwrap();
        assert!(r4.1.x < r2.1.x, "pane 4 should be left of pane 2");
        assert!(approx_eq(r4.1.y, r2.1.y), "pane 4 and 2 should share top y");
    }

    // ──────────────────────────────────────────
    // Restructure: move pane 4 to pane 1 left (Case 3)
    // Expected: V(2,3) | 4 | 1 — 3 equal columns
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_move_pane4_to_pane1_left() {
        let mut layout = make_quadrant_layout();
        assert!(layout.restructure_move_pane(4, 1, tide_core::DropZone::Left, WINDOW));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // Pane 4 should be between V(2,3) and pane 1
        let r4 = rects.iter().find(|(id, _)| *id == 4).unwrap();
        let r1 = rects.iter().find(|(id, _)| *id == 1).unwrap();
        let r2 = rects.iter().find(|(id, _)| *id == 2).unwrap();
        assert!(r4.1.x > r2.1.x, "pane 4 should be right of pane 2");
        assert!(r4.1.x < r1.1.x, "pane 4 should be left of pane 1");
        // Pane 4 should be full height (it's a standalone column)
        assert!(approx_eq(r4.1.height, 600.0), "pane 4 height: {}", r4.1.height);
    }

    // ──────────────────────────────────────────
    // Restructure: move pane 4 to pane 1 right (Case 4)
    // Expected: V(2,3) | 1 | 4 — 3 equal columns
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_move_pane4_to_pane1_right() {
        let mut layout = make_quadrant_layout();
        assert!(layout.restructure_move_pane(4, 1, tide_core::DropZone::Right, WINDOW));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // Pane 4 should be to the right of pane 1
        let r4 = rects.iter().find(|(id, _)| *id == 4).unwrap();
        let r1 = rects.iter().find(|(id, _)| *id == 1).unwrap();
        let r2 = rects.iter().find(|(id, _)| *id == 2).unwrap();
        assert!(r4.1.x > r1.1.x, "pane 4 should be right of pane 1");
        assert!(r1.1.x > r2.1.x, "pane 1 should be right of pane 2");
        assert!(approx_eq(r4.1.height, 600.0), "pane 4 height: {}", r4.1.height);
    }

    // ──────────────────────────────────────────
    // Restructure: move pane 4 to root-top (Case 5)
    // Expected: 4 over H(2,1) over 3 — 3 equal rows
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_move_to_root_top() {
        let mut layout = make_quadrant_layout();
        assert!(layout.restructure_move_to_root(4, tide_core::DropZone::Top, WINDOW));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // Pane 4 should be on top, full width
        let r4 = rects.iter().find(|(id, _)| *id == 4).unwrap();
        assert!(approx_eq(r4.1.y, 0.0), "pane 4 y: {}", r4.1.y);
        assert!(approx_eq(r4.1.width, 800.0), "pane 4 width: {}", r4.1.width);

        // Panes 2 and 1 should be in the middle row, side by side
        let r2 = rects.iter().find(|(id, _)| *id == 2).unwrap();
        let r1 = rects.iter().find(|(id, _)| *id == 1).unwrap();
        assert!(approx_eq(r2.1.y, r1.1.y), "2 and 1 should share y");
        assert!(r2.1.y > r4.1.y, "2 should be below 4");

        // Pane 3 should be on the bottom, full width
        let r3 = rects.iter().find(|(id, _)| *id == 3).unwrap();
        assert!(approx_eq(r3.1.width, 800.0), "pane 3 width: {}", r3.1.width);
        assert!(r3.1.y > r2.1.y, "3 should be below 2");
    }

    // ──────────────────────────────────────────
    // Restructure: move pane 4 to root-bottom (Case 6)
    // Expected: H(2,1) over 3 over 4 — 3 equal rows
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_move_to_root_bottom() {
        let mut layout = make_quadrant_layout();
        assert!(layout.restructure_move_to_root(4, tide_core::DropZone::Bottom, WINDOW));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // Panes 2 and 1 should be in the top row, side by side
        let r2 = rects.iter().find(|(id, _)| *id == 2).unwrap();
        let r1 = rects.iter().find(|(id, _)| *id == 1).unwrap();
        assert!(approx_eq(r2.1.y, 0.0) || approx_eq(r1.1.y, 0.0), "top row at y=0");
        assert!(approx_eq(r2.1.y, r1.1.y), "2 and 1 should share y");

        // Pane 3 should be in the middle row, full width
        let r3 = rects.iter().find(|(id, _)| *id == 3).unwrap();
        assert!(r3.1.y > r2.1.y, "3 should be below 2");
        assert!(approx_eq(r3.1.width, 800.0), "pane 3 width: {}", r3.1.width);

        // Pane 4 should be on the bottom, full width
        let r4 = rects.iter().find(|(id, _)| *id == 4).unwrap();
        assert!(r4.1.y > r3.1.y, "4 should be below 3");
        assert!(approx_eq(r4.1.width, 800.0), "pane 4 width: {}", r4.1.width);
    }

    // ──────────────────────────────────────────
    // Restructure: move pane 4 to pane 3 top (Case 7)
    // Expected: H(2,1) over 4 over 3 — 3 equal rows (approx)
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_move_pane4_to_pane3_top() {
        let mut layout = make_quadrant_layout();
        assert!(layout.restructure_move_pane(4, 3, tide_core::DropZone::Top, WINDOW));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // Pane 4 should be above pane 3
        let r4 = rects.iter().find(|(id, _)| *id == 4).unwrap();
        let r3 = rects.iter().find(|(id, _)| *id == 3).unwrap();
        assert!(r4.1.y < r3.1.y, "pane 4 should be above pane 3");

        // Panes 2 and 1 should be in the top row
        let r2 = rects.iter().find(|(id, _)| *id == 2).unwrap();
        let r1 = rects.iter().find(|(id, _)| *id == 1).unwrap();
        assert!(approx_eq(r2.1.y, r1.1.y), "2 and 1 should share y");
    }

    // ──────────────────────────────────────────
    // Restructure: swap (Center zone)
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_swap_center() {
        let mut layout = make_quadrant_layout();
        let rects_before = layout.compute(WINDOW, &[], None);
        let r4_before = rects_before.iter().find(|(id, _)| *id == 4).unwrap().1;
        let r1_before = rects_before.iter().find(|(id, _)| *id == 1).unwrap().1;

        assert!(layout.restructure_move_pane(4, 1, tide_core::DropZone::Center, WINDOW));

        let rects_after = layout.compute(WINDOW, &[], None);
        assert_eq!(rects_after.len(), 4);
        assert_no_gaps_no_overlaps(&rects_after, WINDOW);

        // After swap: pane 4 should be where pane 1 was, and vice versa
        let r4_after = rects_after.iter().find(|(id, _)| *id == 4).unwrap().1;
        let r1_after = rects_after.iter().find(|(id, _)| *id == 1).unwrap().1;
        assert!(rect_approx_eq(&r4_after, &r1_before), "pane 4 should be at pane 1's old position");
        assert!(rect_approx_eq(&r1_after, &r4_before), "pane 1 should be at pane 4's old position");
    }

    // ──────────────────────────────────────────
    // Restructure: edge cases
    // ──────────────────────────────────────────

    #[test]
    fn test_restructure_two_pane_move() {
        // H(A, B) → restructure move B to root-left → H(B, A)
        let (mut layout, pane_a) = SplitLayout::with_initial_pane();
        let pane_b = layout.split(pane_a, SplitDirection::Horizontal);

        assert!(layout.restructure_move_to_root(pane_b, tide_core::DropZone::Left, WINDOW));

        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 2);
        assert_no_gaps_no_overlaps(&rects, WINDOW);

        // B should be on the left
        let rb = rects.iter().find(|(id, _)| *id == pane_b).unwrap();
        assert!(approx_eq(rb.1.x, 0.0));
        assert!(approx_eq(rb.1.width, 400.0));
    }

    #[test]
    fn test_restructure_single_pane_noop() {
        let (mut layout, pane_a) = SplitLayout::with_initial_pane();
        assert!(!layout.restructure_move_to_root(pane_a, tide_core::DropZone::Left, WINDOW));

        // Should still have the single pane
        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 1);
    }

    #[test]
    fn test_restructure_same_pane_noop() {
        let mut layout = make_quadrant_layout();
        assert!(!layout.restructure_move_pane(4, 4, tide_core::DropZone::Left, WINDOW));

        // Layout unchanged
        let rects = layout.compute(WINDOW, &[], None);
        assert_eq!(rects.len(), 4);
    }

    // ──────────────────────────────────────────
    // simulate_drop uses restructure
    // ──────────────────────────────────────────

    #[test]
    fn test_simulate_drop_restructure_root() {
        let layout = make_quadrant_layout();

        // Simulate moving pane 4 to root-left (source_in_tree = true)
        let preview = layout.simulate_drop(4, None, tide_core::DropZone::Left, true, WINDOW);
        assert!(preview.is_some(), "simulate_drop should return a rect");
        let r = preview.unwrap();
        // Pane 4 should be on the far left
        assert!(approx_eq(r.x, 0.0), "preview x: {}", r.x);
        assert!(approx_eq(r.height, 600.0), "preview height: {}", r.height);
    }

    #[test]
    fn test_simulate_drop_restructure_pane() {
        let layout = make_quadrant_layout();

        // Simulate moving pane 4 to pane 1's left (source_in_tree = true)
        let preview = layout.simulate_drop(4, Some(1), tide_core::DropZone::Left, true, WINDOW);
        assert!(preview.is_some(), "simulate_drop should return a rect");
        let r = preview.unwrap();
        // Pane 4 should be full height (standalone column)
        assert!(approx_eq(r.height, 600.0), "preview height: {}", r.height);
    }
}
