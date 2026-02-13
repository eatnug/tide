#[cfg(test)]
mod tests {
    use crate::{Action, Direction, GlobalAction, Router};
    use tide_core::{InputEvent, Key, Modifiers, MouseButton, Rect, Size, Vec2};

    /// Helper: creates a set of two side-by-side pane rects.
    fn two_panes_horizontal() -> Vec<(tide_core::PaneId, Rect)> {
        vec![
            (1, Rect::new(0.0, 0.0, 200.0, 400.0)),
            (2, Rect::new(200.0, 0.0, 200.0, 400.0)),
        ]
    }

    /// Helper: creates a set of two vertically stacked pane rects.
    fn two_panes_vertical() -> Vec<(tide_core::PaneId, Rect)> {
        vec![
            (1, Rect::new(0.0, 0.0, 400.0, 200.0)),
            (2, Rect::new(0.0, 200.0, 400.0, 200.0)),
        ]
    }

    fn no_modifiers() -> Modifiers {
        Modifiers::default()
    }

    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Default::default()
        }
    }

    fn meta() -> Modifiers {
        Modifiers {
            meta: true,
            ..Default::default()
        }
    }

    fn ctrl_shift() -> Modifiers {
        Modifiers {
            ctrl: true,
            shift: true,
            ..Default::default()
        }
    }

    fn meta_shift() -> Modifiers {
        Modifiers {
            meta: true,
            shift: true,
            ..Default::default()
        }
    }

    // ── Focus management tests ──────────────────

    #[test]
    fn click_in_pane_a_focuses_pane_a() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseClick {
            position: Vec2::new(100.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::RouteToPane(1));
        assert_eq!(router.focused(), Some(1));
    }

    #[test]
    fn click_in_pane_b_switches_focus() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event1 = InputEvent::MouseClick {
            position: Vec2::new(100.0, 200.0),
            button: MouseButton::Left,
        };
        router.process(event1, &panes);
        assert_eq!(router.focused(), Some(1));

        let event2 = InputEvent::MouseClick {
            position: Vec2::new(300.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event2, &panes);

        assert_eq!(action, Action::RouteToPane(2));
        assert_eq!(router.focused(), Some(2));
    }

    #[test]
    fn click_outside_panes_does_not_change_focus() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseClick {
            position: Vec2::new(500.0, 500.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::None);
        assert_eq!(router.focused(), Some(1));
    }

    // ── Keyboard routing tests ──────────────────

    #[test]
    fn keyboard_event_routes_to_focused_pane() {
        let mut router = Router::new();
        router.set_focused(2);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('a'),
            modifiers: no_modifiers(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::RouteToPane(2));
    }

    #[test]
    fn keyboard_event_with_no_focus_returns_none() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('a'),
            modifiers: no_modifiers(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::None);
    }

    // ── Hotkey interception tests ───────────────

    #[test]
    fn ctrl_backslash_triggers_split_right() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('\\'),
            modifiers: ctrl(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::SplitHorizontal));
    }

    #[test]
    fn meta_backslash_triggers_split_right() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('\\'),
            modifiers: meta(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::SplitHorizontal));
    }

    #[test]
    fn ctrl_shift_backslash_triggers_split_bottom() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('|'),
            modifiers: ctrl_shift(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::SplitVertical));
    }

    #[test]
    fn meta_shift_backslash_triggers_split_bottom() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('|'),
            modifiers: meta_shift(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::SplitVertical));
    }

    #[test]
    fn ctrl_w_triggers_close_pane() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('w'),
            modifiers: ctrl(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::ClosePane));
    }

    #[test]
    fn meta_w_triggers_close_pane() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('w'),
            modifiers: meta(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::ClosePane));
    }

    #[test]
    fn ctrl_b_triggers_toggle_file_tree() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('b'),
            modifiers: ctrl(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::ToggleFileTree));
    }

    #[test]
    fn ctrl_arrow_triggers_move_focus() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let cases = [
            (Key::Up, Direction::Up),
            (Key::Down, Direction::Down),
            (Key::Left, Direction::Left),
            (Key::Right, Direction::Right),
        ];

        for (key, expected_dir) in cases {
            let event = InputEvent::KeyPress {
                key,
                modifiers: ctrl(),
            };
            let action = router.process(event, &panes);
            assert_eq!(
                action,
                Action::GlobalAction(GlobalAction::MoveFocus(expected_dir))
            );
        }
    }

    #[test]
    fn meta_arrow_triggers_move_focus() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Right,
            modifiers: meta(),
        };
        let action = router.process(event, &panes);

        assert_eq!(
            action,
            Action::GlobalAction(GlobalAction::MoveFocus(Direction::Right))
        );
    }

    #[test]
    fn hotkey_is_not_routed_to_pane() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('\\'),
            modifiers: ctrl(),
        };
        let action = router.process(event, &panes);

        match action {
            Action::GlobalAction(_) => {}
            other => panic!("Expected GlobalAction, got {:?}", other),
        }
    }

    // ── Mouse hit-testing tests ─────────────────

    #[test]
    fn mouse_click_routes_to_pane_containing_mouse() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseClick {
            position: Vec2::new(350.0, 100.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::RouteToPane(2));
    }

    #[test]
    fn mouse_move_updates_hovered_pane() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event1 = InputEvent::MouseMove {
            position: Vec2::new(50.0, 50.0),
        };
        router.process(event1, &panes);
        assert_eq!(router.hovered(), Some(1));

        let event2 = InputEvent::MouseMove {
            position: Vec2::new(300.0, 50.0),
        };
        router.process(event2, &panes);
        assert_eq!(router.hovered(), Some(2));

        let event3 = InputEvent::MouseMove {
            position: Vec2::new(500.0, 50.0),
        };
        router.process(event3, &panes);
        assert_eq!(router.hovered(), None);
    }

    #[test]
    fn scroll_routes_to_pane_under_mouse() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseScroll {
            delta: -1.0,
            position: Vec2::new(300.0, 200.0),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::RouteToPane(2));
    }

    // ── Border detection and drag tests ─────────

    #[test]
    fn mouse_near_vertical_border_detected_as_border_drag() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();
        let event = InputEvent::MouseClick {
            position: Vec2::new(200.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::DragBorder(Vec2::new(200.0, 200.0)));
        assert!(router.is_dragging_border());
    }

    #[test]
    fn mouse_near_horizontal_border_detected_as_border_drag() {
        let mut router = Router::new();
        let panes = two_panes_vertical();
        let event = InputEvent::MouseClick {
            position: Vec2::new(200.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::DragBorder(Vec2::new(200.0, 200.0)));
        assert!(router.is_dragging_border());
    }

    #[test]
    fn mouse_not_near_border_routes_to_pane() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseClick {
            position: Vec2::new(50.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::RouteToPane(1));
        assert!(!router.is_dragging_border());
    }

    #[test]
    fn drag_on_border_continues_border_drag() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let click = InputEvent::MouseClick {
            position: Vec2::new(200.0, 200.0),
            button: MouseButton::Left,
        };
        router.process(click, &panes);
        assert!(router.is_dragging_border());

        let drag = InputEvent::MouseDrag {
            position: Vec2::new(210.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(drag, &panes);

        assert_eq!(action, Action::DragBorder(Vec2::new(210.0, 200.0)));
    }

    #[test]
    fn drag_inside_pane_routes_to_pane() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let drag = InputEvent::MouseDrag {
            position: Vec2::new(50.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(drag, &panes);

        assert_eq!(action, Action::RouteToPane(1));
        assert!(!router.is_dragging_border());
    }

    #[test]
    fn click_after_border_drag_ends_drag_state() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let click_border = InputEvent::MouseClick {
            position: Vec2::new(200.0, 200.0),
            button: MouseButton::Left,
        };
        router.process(click_border, &panes);
        assert!(router.is_dragging_border());

        let click_pane = InputEvent::MouseClick {
            position: Vec2::new(50.0, 200.0),
            button: MouseButton::Left,
        };
        router.process(click_pane, &panes);
        assert!(!router.is_dragging_border());
    }

    #[test]
    fn border_only_detected_between_adjacent_panes() {
        let mut router = Router::new();
        let panes = vec![(1, Rect::new(0.0, 0.0, 200.0, 400.0))];

        let event = InputEvent::MouseClick {
            position: Vec2::new(200.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::RouteToPane(1));
        assert!(!router.is_dragging_border());
    }

    // ── Trait implementation tests ───────────────

    #[test]
    fn trait_route_keyboard_to_focused() {
        use tide_core::InputRouter as _;

        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('x'),
            modifiers: no_modifiers(),
        };
        let result = router.route(event, &panes, 2);

        assert_eq!(result, Some(2));
    }

    #[test]
    fn trait_route_hotkey_returns_none() {
        use tide_core::InputRouter as _;

        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('\\'),
            modifiers: ctrl(),
        };
        let result = router.route(event, &panes, 1);

        assert_eq!(result, None);
    }

    #[test]
    fn trait_route_click_to_correct_pane() {
        use tide_core::InputRouter as _;

        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseClick {
            position: Vec2::new(300.0, 200.0),
            button: MouseButton::Left,
        };
        let result = router.route(event, &panes, 1);

        assert_eq!(result, Some(2));
        assert_eq!(router.focused(), Some(2));
    }

    #[test]
    fn trait_route_scroll_to_pane_under_mouse() {
        use tide_core::InputRouter as _;

        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseScroll {
            delta: 1.0,
            position: Vec2::new(100.0, 200.0),
        };
        let result = router.route(event, &panes, 2);

        assert_eq!(result, Some(1));
    }

    #[test]
    fn trait_route_resize_returns_none() {
        use tide_core::InputRouter as _;

        let mut router = Router::new();
        let panes = two_panes_horizontal();

        let event = InputEvent::Resize {
            size: Size::new(800.0, 600.0),
        };
        let result = router.route(event, &panes, 1);

        assert_eq!(result, None);
    }

    // ── Edge case tests ─────────────────────────

    #[test]
    fn empty_pane_rects() {
        let mut router = Router::new();
        let panes: Vec<(tide_core::PaneId, Rect)> = vec![];

        let event = InputEvent::MouseClick {
            position: Vec2::new(100.0, 100.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::None);
    }

    #[test]
    fn border_threshold_respected() {
        let mut router = Router::with_border_threshold(10.0);
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseClick {
            position: Vec2::new(192.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::DragBorder(Vec2::new(192.0, 200.0)));
    }

    #[test]
    fn border_threshold_too_far() {
        let mut router = Router::with_border_threshold(4.0);
        let panes = two_panes_horizontal();

        let event = InputEvent::MouseClick {
            position: Vec2::new(180.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::RouteToPane(1));
        assert!(!router.is_dragging_border());
    }

    #[test]
    fn set_focused_and_get_focused() {
        let mut router = Router::new();
        assert_eq!(router.focused(), None);

        router.set_focused(42);
        assert_eq!(router.focused(), Some(42));

        router.set_focused(7);
        assert_eq!(router.focused(), Some(7));
    }

    #[test]
    fn default_trait() {
        let router = Router::default();
        assert_eq!(router.focused(), None);
        assert_eq!(router.hovered(), None);
        assert!(!router.is_dragging_border());
    }
}
