// Input router implementation (Stream E)
// Implements tide_core::InputRouter with hit-testing, focus management,
// hotkey interception, and drag routing.

use tide_core::{InputEvent, Key, Modifiers, MouseButton, PaneId, Rect, Vec2};

// ──────────────────────────────────────────────
// Action types
// ──────────────────────────────────────────────

/// Actions the app should handle in response to input.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Route event to a specific pane.
    RouteToPane(PaneId),
    /// A global action was triggered.
    GlobalAction(GlobalAction),
    /// Start or continue dragging a border at the given position.
    DragBorder(Vec2),
    /// No action to take.
    None,
}

/// Global actions triggered by hotkeys or other mechanisms.
#[derive(Debug, Clone, PartialEq)]
pub enum GlobalAction {
    SplitVertical,
    SplitHorizontal,
    ClosePane,
    ToggleFileTree,
    MoveFocus(Direction),
}

/// Cardinal direction for focus movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Up,
    Down,
    Left,
    Right,
}

// ──────────────────────────────────────────────
// Router
// ──────────────────────────────────────────────

/// The input router determines what happens with each input event:
/// which pane it goes to, whether it triggers a global action, or
/// whether it initiates a border drag.
pub struct Router {
    focused: Option<PaneId>,
    hovered: Option<PaneId>,
    dragging_border: bool,
    border_threshold: f32,
}

impl Router {
    /// Create a new Router with default settings.
    pub fn new() -> Self {
        Self {
            focused: None,
            hovered: None,
            dragging_border: false,
            border_threshold: 4.0,
        }
    }

    /// Create a new Router with a custom border detection threshold.
    pub fn with_border_threshold(threshold: f32) -> Self {
        Self {
            focused: None,
            hovered: None,
            dragging_border: false,
            border_threshold: threshold,
        }
    }

    /// Get the currently focused pane, if any.
    pub fn focused(&self) -> Option<PaneId> {
        self.focused
    }

    /// Set the focused pane.
    pub fn set_focused(&mut self, pane: PaneId) {
        self.focused = Some(pane);
    }

    /// Get the currently hovered pane, if any.
    pub fn hovered(&self) -> Option<PaneId> {
        self.hovered
    }

    /// Returns true if a border drag is currently in progress.
    pub fn is_dragging_border(&self) -> bool {
        self.dragging_border
    }

    /// Process an input event and return what action should be taken.
    pub fn process(&mut self, event: InputEvent, pane_rects: &[(PaneId, Rect)]) -> Action {
        match event {
            InputEvent::KeyPress { key, modifiers } => self.process_key(key, modifiers),
            InputEvent::MouseClick {
                position, button, ..
            } => self.process_click(position, button, pane_rects),
            InputEvent::MouseMove { position } => self.process_mouse_move(position, pane_rects),
            InputEvent::MouseDrag {
                position, button, ..
            } => self.process_drag(position, button, pane_rects),
            InputEvent::MouseScroll { position, .. } => {
                // Route scroll events to the pane under the mouse.
                match self.pane_at(position, pane_rects) {
                    Some(id) => Action::RouteToPane(id),
                    None => Action::None,
                }
            }
            InputEvent::Resize { .. } => {
                // Resize events are handled globally by the app, not routed to panes.
                Action::None
            }
        }
    }

    // ── Key processing ──────────────────────────

    fn process_key(&self, key: Key, modifiers: Modifiers) -> Action {
        // Check global hotkeys first. We treat both Ctrl and Meta (Cmd) as
        // the "command" modifier so that hotkeys work on both macOS and Linux.
        if modifiers.ctrl || modifiers.meta {
            if let Some(action) = self.match_hotkey(key, modifiers) {
                return Action::GlobalAction(action);
            }
        }

        // Not a hotkey -- route to the focused pane.
        match self.focused {
            Some(id) => Action::RouteToPane(id),
            None => Action::None,
        }
    }

    /// Match a key + modifiers against the hotkey table.
    /// Returns Some(GlobalAction) if the combination is a known hotkey.
    fn match_hotkey(&self, key: Key, modifiers: Modifiers) -> Option<GlobalAction> {
        match key {
            // Cmd+D / Ctrl+D  -> split vertical
            // Cmd+Shift+D / Ctrl+Shift+D -> split horizontal
            Key::Char('d') | Key::Char('D') => {
                if modifiers.shift {
                    Some(GlobalAction::SplitHorizontal)
                } else {
                    Some(GlobalAction::SplitVertical)
                }
            }
            // Cmd+W / Ctrl+W -> close pane
            Key::Char('w') | Key::Char('W') => Some(GlobalAction::ClosePane),
            // Cmd+B / Ctrl+B -> toggle file tree
            Key::Char('b') | Key::Char('B') => Some(GlobalAction::ToggleFileTree),
            // Cmd+Arrow / Ctrl+Arrow -> move focus
            Key::Up => Some(GlobalAction::MoveFocus(Direction::Up)),
            Key::Down => Some(GlobalAction::MoveFocus(Direction::Down)),
            Key::Left => Some(GlobalAction::MoveFocus(Direction::Left)),
            Key::Right => Some(GlobalAction::MoveFocus(Direction::Right)),
            _ => None,
        }
    }

    // ── Click processing ────────────────────────

    fn process_click(
        &mut self,
        position: Vec2,
        _button: MouseButton,
        pane_rects: &[(PaneId, Rect)],
    ) -> Action {
        // End any ongoing border drag on click.
        self.dragging_border = false;

        // Check if click is near a border first.
        if self.is_near_border(position, pane_rects) {
            self.dragging_border = true;
            return Action::DragBorder(position);
        }

        // Otherwise, hit-test panes.
        match self.pane_at(position, pane_rects) {
            Some(id) => {
                self.focused = Some(id);
                Action::RouteToPane(id)
            }
            None => Action::None,
        }
    }

    // ── Mouse move processing ───────────────────

    fn process_mouse_move(
        &mut self,
        position: Vec2,
        pane_rects: &[(PaneId, Rect)],
    ) -> Action {
        self.hovered = self.pane_at(position, pane_rects);
        Action::None
    }

    // ── Drag processing ─────────────────────────

    fn process_drag(
        &mut self,
        position: Vec2,
        _button: MouseButton,
        pane_rects: &[(PaneId, Rect)],
    ) -> Action {
        // If we are already dragging a border, continue the drag.
        if self.dragging_border {
            return Action::DragBorder(position);
        }

        // If the drag starts near a border, begin a border drag.
        if self.is_near_border(position, pane_rects) {
            self.dragging_border = true;
            return Action::DragBorder(position);
        }

        // Otherwise route the drag to the pane under the mouse.
        match self.pane_at(position, pane_rects) {
            Some(id) => Action::RouteToPane(id),
            None => Action::None,
        }
    }

    // ── Hit testing ─────────────────────────────

    /// Find which pane contains the given point.
    /// If panes overlap, returns the first match (they should not overlap
    /// in a well-formed layout).
    fn pane_at(&self, position: Vec2, pane_rects: &[(PaneId, Rect)]) -> Option<PaneId> {
        for &(id, rect) in pane_rects {
            if rect.contains(position) {
                return Some(id);
            }
        }
        None
    }

    // ── Border detection ────────────────────────

    /// Check if a point is near any pane border. A "border" is the boundary
    /// between two adjacent panes. We detect this by checking if the point
    /// is within `border_threshold` pixels of any edge of any pane rect,
    /// but only on edges that are *shared* with another pane (i.e., not on
    /// the window boundary).
    ///
    /// For simplicity, we check if the point is within threshold of any
    /// pane edge, and that it is also near (within threshold) of another
    /// pane's opposing edge. This ensures we only detect internal borders.
    fn is_near_border(&self, position: Vec2, pane_rects: &[(PaneId, Rect)]) -> bool {
        let t = self.border_threshold;

        for &(id_a, rect_a) in pane_rects {
            // Check right edge of rect_a
            let right_edge = rect_a.x + rect_a.width;
            if (position.x - right_edge).abs() <= t
                && position.y >= rect_a.y
                && position.y <= rect_a.y + rect_a.height
            {
                // See if another pane's left edge is adjacent.
                for &(id_b, rect_b) in pane_rects {
                    if id_b != id_a
                        && (rect_b.x - right_edge).abs() <= t * 2.0
                        && position.y >= rect_b.y
                        && position.y <= rect_b.y + rect_b.height
                    {
                        return true;
                    }
                }
            }

            // Check bottom edge of rect_a
            let bottom_edge = rect_a.y + rect_a.height;
            if (position.y - bottom_edge).abs() <= t
                && position.x >= rect_a.x
                && position.x <= rect_a.x + rect_a.width
            {
                // See if another pane's top edge is adjacent.
                for &(id_b, rect_b) in pane_rects {
                    if id_b != id_a
                        && (rect_b.y - bottom_edge).abs() <= t * 2.0
                        && position.x >= rect_b.x
                        && position.x <= rect_b.x + rect_b.width
                    {
                        return true;
                    }
                }
            }
        }

        false
    }
}

impl Default for Router {
    fn default() -> Self {
        Self::new()
    }
}

// ──────────────────────────────────────────────
// Trait implementation: tide_core::InputRouter
// ──────────────────────────────────────────────

impl tide_core::InputRouter for Router {
    fn route(
        &mut self,
        event: InputEvent,
        pane_rects: &[(PaneId, Rect)],
        focused: PaneId,
    ) -> Option<PaneId> {
        // Update our internal focus state from the authoritative source.
        self.focused = Some(focused);

        match event {
            InputEvent::KeyPress { .. } => {
                // Keyboard events go to the focused pane, unless a global
                // hotkey intercepts them.
                let action = self.process(event, pane_rects);
                match action {
                    Action::RouteToPane(id) => Some(id),
                    // Global actions are not routed to any pane.
                    Action::GlobalAction(_) => None,
                    _ => Some(focused),
                }
            }
            InputEvent::MouseClick { position, .. } => {
                // Click: route to the pane under the click, also
                // updating focus.
                match self.pane_at(position, pane_rects) {
                    Some(id) => {
                        self.focused = Some(id);
                        Some(id)
                    }
                    None => None,
                }
            }
            InputEvent::MouseMove { position } => {
                self.hovered = self.pane_at(position, pane_rects);
                // Mouse move is informational; no pane "consumes" it via routing.
                self.hovered
            }
            InputEvent::MouseDrag { position, .. } => self.pane_at(position, pane_rects),
            InputEvent::MouseScroll { position, .. } => self.pane_at(position, pane_rects),
            InputEvent::Resize { .. } => None,
        }
    }
}

// ──────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tide_core::{InputEvent, Key, Modifiers, MouseButton, Rect, Size, Vec2};

    /// Helper: creates a set of two side-by-side pane rects.
    ///
    /// ```text
    /// ┌─────────┬─────────┐
    /// │  Pane 1  │  Pane 2  │
    /// │  (0,0)   │ (200,0)  │
    /// │  200x400 │  200x400 │
    /// └─────────┴─────────┘
    /// ```
    fn two_panes_horizontal() -> Vec<(PaneId, Rect)> {
        vec![
            (1, Rect::new(0.0, 0.0, 200.0, 400.0)),
            (2, Rect::new(200.0, 0.0, 200.0, 400.0)),
        ]
    }

    /// Helper: creates a set of two vertically stacked pane rects.
    ///
    /// ```text
    /// ┌──────────┐
    /// │  Pane 1   │
    /// │  (0,0)    │
    /// │  400x200  │
    /// ├──────────┤
    /// │  Pane 2   │
    /// │  (0,200)  │
    /// │  400x200  │
    /// └──────────┘
    /// ```
    fn two_panes_vertical() -> Vec<(PaneId, Rect)> {
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

        // Click in pane 1.
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

        // First click in pane 1.
        let event1 = InputEvent::MouseClick {
            position: Vec2::new(100.0, 200.0),
            button: MouseButton::Left,
        };
        router.process(event1, &panes);
        assert_eq!(router.focused(), Some(1));

        // Then click in pane 2.
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

        // Click outside all panes.
        let event = InputEvent::MouseClick {
            position: Vec2::new(500.0, 500.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::None);
        // Focus unchanged.
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
    fn ctrl_d_triggers_split_vertical() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('d'),
            modifiers: ctrl(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::SplitVertical));
    }

    #[test]
    fn meta_d_triggers_split_vertical() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('d'),
            modifiers: meta(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::SplitVertical));
    }

    #[test]
    fn ctrl_shift_d_triggers_split_horizontal() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('d'),
            modifiers: ctrl_shift(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::SplitHorizontal));
    }

    #[test]
    fn meta_shift_d_triggers_split_horizontal() {
        let mut router = Router::new();
        router.set_focused(1);
        let panes = two_panes_horizontal();

        let event = InputEvent::KeyPress {
            key: Key::Char('D'),
            modifiers: meta_shift(),
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::GlobalAction(GlobalAction::SplitHorizontal));
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
            key: Key::Char('d'),
            modifiers: ctrl(),
        };
        let action = router.process(event, &panes);

        // Should be a global action, NOT RouteToPane.
        match action {
            Action::GlobalAction(_) => {} // correct
            other => panic!("Expected GlobalAction, got {:?}", other),
        }
    }

    // ── Mouse hit-testing tests ─────────────────

    #[test]
    fn mouse_click_routes_to_pane_containing_mouse() {
        let mut router = Router::new();
        let panes = two_panes_horizontal();

        // Click inside pane 2.
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

        // Move into pane 1.
        let event1 = InputEvent::MouseMove {
            position: Vec2::new(50.0, 50.0),
        };
        router.process(event1, &panes);
        assert_eq!(router.hovered(), Some(1));

        // Move into pane 2.
        let event2 = InputEvent::MouseMove {
            position: Vec2::new(300.0, 50.0),
        };
        router.process(event2, &panes);
        assert_eq!(router.hovered(), Some(2));

        // Move outside.
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
        // The border between pane 1 and pane 2 is at x=200.
        // Click at x=200 (right on the border).
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
        // The border between pane 1 and pane 2 is at y=200.
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

        // Click well inside pane 1 (far from border at x=200).
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

        // Start a click on the border.
        let click = InputEvent::MouseClick {
            position: Vec2::new(200.0, 200.0),
            button: MouseButton::Left,
        };
        router.process(click, &panes);
        assert!(router.is_dragging_border());

        // Continue dragging.
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

        // Drag inside pane 1, far from any border.
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

        // Start border drag.
        let click_border = InputEvent::MouseClick {
            position: Vec2::new(200.0, 200.0),
            button: MouseButton::Left,
        };
        router.process(click_border, &panes);
        assert!(router.is_dragging_border());

        // Click inside pane 1 (not on border).
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
        // A single pane: its right edge at x=200 is the window edge, not
        // a border between panes.
        let panes = vec![(1, Rect::new(0.0, 0.0, 200.0, 400.0))];

        let event = InputEvent::MouseClick {
            position: Vec2::new(200.0, 200.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        // Should route to the pane (it's on the edge of the pane rect),
        // not detect a border drag.
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
            key: Key::Char('d'),
            modifiers: ctrl(),
        };
        let result = router.route(event, &panes, 1);

        // Global hotkey is not routed to any pane.
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

        // Click in pane 2, even though pane 1 was focused.
        assert_eq!(result, Some(2));
        // Focus should have switched.
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

        // Scroll is over pane 1.
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
        let panes: Vec<(PaneId, Rect)> = vec![];

        let event = InputEvent::MouseClick {
            position: Vec2::new(100.0, 100.0),
            button: MouseButton::Left,
        };
        let action = router.process(event, &panes);

        assert_eq!(action, Action::None);
    }

    #[test]
    fn border_threshold_respected() {
        // Use a larger threshold to verify it's configurable.
        let mut router = Router::with_border_threshold(10.0);
        let panes = two_panes_horizontal();

        // 8 pixels from border (within 10px threshold).
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

        // 20 pixels from border (well outside 4px threshold).
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
