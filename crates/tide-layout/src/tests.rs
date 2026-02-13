#[cfg(test)]
mod tests {
    use crate::SplitLayout;
    use tide_core::{LayoutEngine, Rect, Size, SplitDirection, Vec2};

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
}
