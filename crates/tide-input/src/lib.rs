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
    OpenFile,
    MoveFocus(Direction),
    Paste,
    Copy,
    ToggleFullscreen,
    Find,
    ToggleMaximizePane,
    ToggleEditorPanel,
    NewEditorFile,
    ToggleTheme,
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

    /// End border drag state (call on mouse release).
    pub fn end_drag(&mut self) {
        self.dragging_border = false;
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
        // Check global hotkeys.  On macOS, Cmd (Meta) is the app-level
        // modifier; plain Ctrl must pass through to the terminal (Ctrl+C,
        // Ctrl+W, etc.).  On Linux (no Meta key), Ctrl+Shift serves as
        // the hotkey modifier (e.g. Ctrl+Shift+C for copy).
        if modifiers.meta || (modifiers.ctrl && modifiers.shift) {
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
            // Cmd+\ / Ctrl+\  -> split right
            // Cmd+Shift+\ / Ctrl+Shift+\ -> split bottom
            Key::Char('\\') | Key::Char('|') => {
                if modifiers.shift {
                    Some(GlobalAction::SplitVertical)
                } else {
                    Some(GlobalAction::SplitHorizontal)
                }
            }
            // Cmd+W / Ctrl+W -> close pane
            Key::Char('w') | Key::Char('W') => Some(GlobalAction::ClosePane),
            // Cmd+B / Ctrl+B -> toggle file tree
            Key::Char('b') | Key::Char('B') => Some(GlobalAction::ToggleFileTree),
            // Cmd+O / Ctrl+O -> open file (show file tree)
            Key::Char('o') | Key::Char('O') => Some(GlobalAction::OpenFile),
            // Cmd+V (macOS) / Ctrl+Shift+V (Linux) -> paste
            Key::Char('v') | Key::Char('V') => {
                if modifiers.meta {
                    Some(GlobalAction::Paste)
                } else if modifiers.ctrl && modifiers.shift {
                    Some(GlobalAction::Paste)
                } else {
                    None // Ctrl+V → terminal 0x16
                }
            }
            // Cmd+C (macOS) / Ctrl+Shift+C (Linux) -> copy
            Key::Char('c') | Key::Char('C') => {
                if modifiers.meta {
                    Some(GlobalAction::Copy)
                } else if modifiers.ctrl && modifiers.shift {
                    Some(GlobalAction::Copy)
                } else {
                    None // Ctrl+C → terminal SIGINT
                }
            }
            // Cmd+Ctrl+F -> toggle fullscreen, Cmd+F / Ctrl+F -> find
            Key::Char('f') | Key::Char('F') => {
                if modifiers.meta && modifiers.ctrl {
                    Some(GlobalAction::ToggleFullscreen)
                } else if modifiers.meta || modifiers.ctrl {
                    Some(GlobalAction::Find)
                } else {
                    None
                }
            }
            // Cmd+Enter / Ctrl+Enter -> toggle maximize pane
            Key::Enter => Some(GlobalAction::ToggleMaximizePane),
            // Cmd+Shift+T / Ctrl+Shift+T -> toggle dark/light theme
            Key::Char('t') | Key::Char('T') => {
                if modifiers.shift {
                    return Some(GlobalAction::ToggleTheme);
                }
                None
            }
            // Cmd+Shift+E -> new editor file, Cmd+E / Ctrl+E -> toggle editor panel
            Key::Char('e') | Key::Char('E') => {
                if modifiers.shift {
                    Some(GlobalAction::NewEditorFile)
                } else {
                    Some(GlobalAction::ToggleEditorPanel)
                }
            }
            // Cmd+Arrow -> move focus (Ctrl+Arrow is reserved for terminal
            // word navigation, so only match on Meta/Cmd)
            Key::Up if modifiers.meta => Some(GlobalAction::MoveFocus(Direction::Up)),
            Key::Down if modifiers.meta => Some(GlobalAction::MoveFocus(Direction::Down)),
            Key::Left if modifiers.meta => Some(GlobalAction::MoveFocus(Direction::Left)),
            Key::Right if modifiers.meta => Some(GlobalAction::MoveFocus(Direction::Right)),
            // Cmd+HJKL / Ctrl+HJKL -> vim-style focus navigation
            Key::Char('h') | Key::Char('H') => Some(GlobalAction::MoveFocus(Direction::Left)),
            Key::Char('j') | Key::Char('J') => Some(GlobalAction::MoveFocus(Direction::Down)),
            Key::Char('k') | Key::Char('K') => Some(GlobalAction::MoveFocus(Direction::Up)),
            Key::Char('l') | Key::Char('L') => Some(GlobalAction::MoveFocus(Direction::Right)),
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

mod tests;
