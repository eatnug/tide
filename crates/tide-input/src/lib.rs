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

/// Which screen slot the user pressed (Cmd+1/2/3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AreaSlot {
    Slot1,
    Slot2,
    Slot3,
}

/// Global actions triggered by hotkeys or other mechanisms.
#[derive(Debug, Clone, PartialEq)]
pub enum GlobalAction {
    SplitVertical,
    SplitHorizontal,
    SplitVerticalHere,
    SplitHorizontalHere,
    ClosePane,
    FocusArea(AreaSlot),
    Navigate(Direction),
    ToggleZoom,
    DockTabPrev,
    DockTabNext,
    FileFinder,
    Paste,
    Copy,
    ToggleFullscreen,
    Find,
    ToggleTheme,
    FontSizeUp,
    FontSizeDown,
    FontSizeReset,
    NewWindow,
    NewFile,
    OpenConfig,
    OpenBrowser,
}

impl GlobalAction {
    /// Human-readable label for display in the config page.
    pub fn label(&self) -> &'static str {
        match self {
            GlobalAction::SplitVertical => "Split Vertical",
            GlobalAction::SplitHorizontal => "Split Horizontal",
            GlobalAction::SplitVerticalHere => "Split Vertical Here",
            GlobalAction::SplitHorizontalHere => "Split Horizontal Here",
            GlobalAction::ClosePane => "Close Pane",
            GlobalAction::FocusArea(AreaSlot::Slot1) => "Focus Slot 1",
            GlobalAction::FocusArea(AreaSlot::Slot2) => "Focus Slot 2",
            GlobalAction::FocusArea(AreaSlot::Slot3) => "Focus Slot 3",
            GlobalAction::Navigate(Direction::Up) => "Navigate Up",
            GlobalAction::Navigate(Direction::Down) => "Navigate Down",
            GlobalAction::Navigate(Direction::Left) => "Navigate Left",
            GlobalAction::Navigate(Direction::Right) => "Navigate Right",
            GlobalAction::ToggleZoom => "Toggle Zoom",
            GlobalAction::DockTabPrev => "Dock Tab Prev",
            GlobalAction::DockTabNext => "Dock Tab Next",
            GlobalAction::FileFinder => "File Finder",
            GlobalAction::Paste => "Paste",
            GlobalAction::Copy => "Copy",
            GlobalAction::ToggleFullscreen => "Toggle Fullscreen",
            GlobalAction::Find => "Find",
            GlobalAction::ToggleTheme => "Toggle Theme",
            GlobalAction::FontSizeUp => "Font Size Up",
            GlobalAction::FontSizeDown => "Font Size Down",
            GlobalAction::FontSizeReset => "Font Size Reset",
            GlobalAction::NewWindow => "New Window",
            GlobalAction::NewFile => "New File",
            GlobalAction::OpenConfig => "Open Config",
            GlobalAction::OpenBrowser => "Open Browser",
        }
    }

    /// Serialization key for keybinding overrides.
    pub fn action_key(&self) -> &'static str {
        match self {
            GlobalAction::SplitVertical => "SplitVertical",
            GlobalAction::SplitHorizontal => "SplitHorizontal",
            GlobalAction::SplitVerticalHere => "SplitVerticalHere",
            GlobalAction::SplitHorizontalHere => "SplitHorizontalHere",
            GlobalAction::ClosePane => "ClosePane",
            GlobalAction::FocusArea(AreaSlot::Slot1) => "FocusSlot1",
            GlobalAction::FocusArea(AreaSlot::Slot2) => "FocusSlot2",
            GlobalAction::FocusArea(AreaSlot::Slot3) => "FocusSlot3",
            GlobalAction::Navigate(Direction::Up) => "NavigateUp",
            GlobalAction::Navigate(Direction::Down) => "NavigateDown",
            GlobalAction::Navigate(Direction::Left) => "NavigateLeft",
            GlobalAction::Navigate(Direction::Right) => "NavigateRight",
            GlobalAction::ToggleZoom => "ToggleZoom",
            GlobalAction::DockTabPrev => "DockTabPrev",
            GlobalAction::DockTabNext => "DockTabNext",
            GlobalAction::FileFinder => "FileFinder",
            GlobalAction::Paste => "Paste",
            GlobalAction::Copy => "Copy",
            GlobalAction::ToggleFullscreen => "ToggleFullscreen",
            GlobalAction::Find => "Find",
            GlobalAction::ToggleTheme => "ToggleTheme",
            GlobalAction::FontSizeUp => "FontSizeUp",
            GlobalAction::FontSizeDown => "FontSizeDown",
            GlobalAction::FontSizeReset => "FontSizeReset",
            GlobalAction::NewWindow => "NewWindow",
            GlobalAction::NewFile => "NewFile",
            GlobalAction::OpenConfig => "OpenConfig",
            GlobalAction::OpenBrowser => "OpenBrowser",
        }
    }

    /// Parse an action key string back to a GlobalAction.
    pub fn from_action_key(s: &str) -> Option<GlobalAction> {
        match s {
            "SplitVertical" => Some(GlobalAction::SplitVertical),
            "SplitHorizontal" => Some(GlobalAction::SplitHorizontal),
            "SplitVerticalHere" => Some(GlobalAction::SplitVerticalHere),
            "SplitHorizontalHere" => Some(GlobalAction::SplitHorizontalHere),
            "ClosePane" => Some(GlobalAction::ClosePane),
            "FocusSlot1" => Some(GlobalAction::FocusArea(AreaSlot::Slot1)),
            "FocusSlot2" => Some(GlobalAction::FocusArea(AreaSlot::Slot2)),
            "FocusSlot3" => Some(GlobalAction::FocusArea(AreaSlot::Slot3)),
            "NavigateUp" => Some(GlobalAction::Navigate(Direction::Up)),
            "NavigateDown" => Some(GlobalAction::Navigate(Direction::Down)),
            "NavigateLeft" => Some(GlobalAction::Navigate(Direction::Left)),
            "NavigateRight" => Some(GlobalAction::Navigate(Direction::Right)),
            "ToggleZoom" => Some(GlobalAction::ToggleZoom),
            "DockTabPrev" => Some(GlobalAction::DockTabPrev),
            "DockTabNext" => Some(GlobalAction::DockTabNext),
            "FileFinder" => Some(GlobalAction::FileFinder),
            "Paste" => Some(GlobalAction::Paste),
            "Copy" => Some(GlobalAction::Copy),
            "ToggleFullscreen" => Some(GlobalAction::ToggleFullscreen),
            "Find" => Some(GlobalAction::Find),
            "ToggleTheme" => Some(GlobalAction::ToggleTheme),
            "FontSizeUp" => Some(GlobalAction::FontSizeUp),
            "FontSizeDown" => Some(GlobalAction::FontSizeDown),
            "FontSizeReset" => Some(GlobalAction::FontSizeReset),
            "NewWindow" => Some(GlobalAction::NewWindow),
            "NewFile" => Some(GlobalAction::NewFile),
            "OpenConfig" => Some(GlobalAction::OpenConfig),
            "OpenBrowser" => Some(GlobalAction::OpenBrowser),
            _ => None,
        }
    }

    /// All bindable actions for display in the config page.
    pub fn all_actions() -> Vec<GlobalAction> {
        vec![
            GlobalAction::SplitHorizontal,
            GlobalAction::SplitVertical,
            GlobalAction::SplitHorizontalHere,
            GlobalAction::SplitVerticalHere,
            GlobalAction::ClosePane,
            GlobalAction::Navigate(Direction::Up),
            GlobalAction::Navigate(Direction::Down),
            GlobalAction::Navigate(Direction::Left),
            GlobalAction::Navigate(Direction::Right),
            GlobalAction::ToggleZoom,
            GlobalAction::FocusArea(AreaSlot::Slot1),
            GlobalAction::FocusArea(AreaSlot::Slot2),
            GlobalAction::FocusArea(AreaSlot::Slot3),
            GlobalAction::DockTabPrev,
            GlobalAction::DockTabNext,
            GlobalAction::FileFinder,
            GlobalAction::Paste,
            GlobalAction::Copy,
            GlobalAction::Find,
            GlobalAction::ToggleFullscreen,
            GlobalAction::ToggleTheme,
            GlobalAction::FontSizeUp,
            GlobalAction::FontSizeDown,
            GlobalAction::FontSizeReset,
            GlobalAction::NewWindow,
            GlobalAction::NewFile,
            GlobalAction::OpenConfig,
            GlobalAction::OpenBrowser,
        ]
    }
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
// Hotkey and KeybindingMap
// ──────────────────────────────────────────────

/// A hotkey combination: a key plus modifier flags.
#[derive(Debug, Clone, PartialEq)]
pub struct Hotkey {
    pub key: Key,
    pub shift: bool,
    pub ctrl: bool,
    pub meta: bool,
    pub alt: bool,
}

impl Hotkey {
    pub fn new(key: Key, shift: bool, ctrl: bool, meta: bool, alt: bool) -> Self {
        Self { key, shift, ctrl, meta, alt }
    }

    /// Format this hotkey as a human-readable string (e.g. "Cmd+Shift+T").
    pub fn display(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.ctrl { parts.push("Ctrl".to_string()); }
        if self.meta { parts.push("Cmd".to_string()); }
        if self.alt { parts.push("Alt".to_string()); }
        if self.shift { parts.push("Shift".to_string()); }
        parts.push(display_key(&self.key));
        parts.join("+")
    }

    /// Check if this hotkey matches a given key + modifiers.
    pub fn matches(&self, key: &Key, modifiers: &Modifiers) -> bool {
        // For character keys, compare case-insensitively
        let key_matches = match (&self.key, key) {
            (Key::Char(a), Key::Char(b)) => a.to_lowercase().eq(b.to_lowercase()),
            (a, b) => a == b,
        };
        key_matches
            && self.shift == modifiers.shift
            && self.ctrl == modifiers.ctrl
            && self.meta == modifiers.meta
            && self.alt == modifiers.alt
    }

    /// Serialization key for the key field.
    pub fn key_name(&self) -> String {
        match &self.key {
            Key::Char(c) => c.to_uppercase().to_string(),
            Key::Enter => "Enter".to_string(),
            Key::Escape => "Escape".to_string(),
            Key::Backspace => "Backspace".to_string(),
            Key::Tab => "Tab".to_string(),
            Key::Up => "Up".to_string(),
            Key::Down => "Down".to_string(),
            Key::Left => "Left".to_string(),
            Key::Right => "Right".to_string(),
            Key::Delete => "Delete".to_string(),
            Key::Home => "Home".to_string(),
            Key::End => "End".to_string(),
            Key::PageUp => "PageUp".to_string(),
            Key::PageDown => "PageDown".to_string(),
            _ => format!("{:?}", self.key),
        }
    }

    /// Parse a key name string back to a Key.
    pub fn key_from_name(s: &str) -> Option<Key> {
        match s {
            "Enter" => Some(Key::Enter),
            "Escape" => Some(Key::Escape),
            "Backspace" => Some(Key::Backspace),
            "Tab" => Some(Key::Tab),
            "Up" => Some(Key::Up),
            "Down" => Some(Key::Down),
            "Left" => Some(Key::Left),
            "Right" => Some(Key::Right),
            "Delete" => Some(Key::Delete),
            "Home" => Some(Key::Home),
            "End" => Some(Key::End),
            "PageUp" => Some(Key::PageUp),
            "PageDown" => Some(Key::PageDown),
            _ => {
                let mut chars = s.chars();
                let c = chars.next()?;
                if chars.next().is_none() {
                    Some(Key::Char(c.to_lowercase().next().unwrap_or(c)))
                } else {
                    None
                }
            }
        }
    }
}

/// Format a Key as a short display string.
///
/// Returns a `String` to support arbitrary `Key::Char` values.
pub fn display_key(key: &Key) -> String {
    match key {
        Key::Char('\\') | Key::Char('|') => "\\".to_string(),
        Key::Char('+') | Key::Char('=') => "+".to_string(),
        Key::Char('-') | Key::Char('_') => "-".to_string(),
        Key::Char('!') => "1".to_string(),
        Key::Char('@') => "2".to_string(),
        Key::Char('#') => "3".to_string(),
        Key::Char(c) => c.to_uppercase().to_string(),
        Key::Enter => "Enter".to_string(),
        Key::Escape => "Esc".to_string(),
        Key::Up => "\u{2191}".to_string(),
        Key::Down => "\u{2193}".to_string(),
        Key::Left => "\u{2190}".to_string(),
        Key::Right => "\u{2192}".to_string(),
        Key::Backspace => "Bksp".to_string(),
        Key::Tab => "Tab".to_string(),
        Key::Delete => "Del".to_string(),
        Key::Home => "Home".to_string(),
        Key::End => "End".to_string(),
        Key::PageUp => "PgUp".to_string(),
        Key::PageDown => "PgDn".to_string(),
        _ => "?".to_string(),
    }
}

/// A user-customizable keybinding map. Overrides the hardcoded hotkey table.
pub struct KeybindingMap {
    pub bindings: Vec<(Hotkey, GlobalAction)>,
}

impl KeybindingMap {
    /// Build the default keybinding map from the hardcoded hotkey table.
    pub fn default_bindings() -> Vec<(Hotkey, GlobalAction)> {
        vec![
            (Hotkey::new(Key::Char('t'), false, false, true, false), GlobalAction::SplitHorizontal),
            (Hotkey::new(Key::Char('t'), true, false, true, false), GlobalAction::SplitVertical),
            (Hotkey::new(Key::Char('\\'), false, false, true, false), GlobalAction::SplitHorizontalHere),
            (Hotkey::new(Key::Char('\\'), true, false, true, false), GlobalAction::SplitVerticalHere),
            (Hotkey::new(Key::Char('w'), false, false, true, false), GlobalAction::ClosePane),
            (Hotkey::new(Key::Char('v'), false, false, true, false), GlobalAction::Paste),
            (Hotkey::new(Key::Char('c'), false, false, true, false), GlobalAction::Copy),
            (Hotkey::new(Key::Char('f'), false, true, true, false), GlobalAction::ToggleFullscreen),
            (Hotkey::new(Key::Char('f'), false, false, true, false), GlobalAction::Find),
            (Hotkey::new(Key::Enter, false, false, true, false), GlobalAction::ToggleZoom),
            (Hotkey::new(Key::Char('d'), true, false, true, false), GlobalAction::ToggleTheme),
            (Hotkey::new(Key::Char('1'), false, false, true, false), GlobalAction::FocusArea(AreaSlot::Slot1)),
            (Hotkey::new(Key::Char('2'), false, false, true, false), GlobalAction::FocusArea(AreaSlot::Slot2)),
            (Hotkey::new(Key::Char('3'), false, false, true, false), GlobalAction::FocusArea(AreaSlot::Slot3)),
            (Hotkey::new(Key::Up, false, false, true, false), GlobalAction::Navigate(Direction::Up)),
            (Hotkey::new(Key::Down, false, false, true, false), GlobalAction::Navigate(Direction::Down)),
            (Hotkey::new(Key::Left, false, false, true, false), GlobalAction::Navigate(Direction::Left)),
            (Hotkey::new(Key::Right, false, false, true, false), GlobalAction::Navigate(Direction::Right)),
            (Hotkey::new(Key::Char('h'), false, false, true, false), GlobalAction::Navigate(Direction::Left)),
            (Hotkey::new(Key::Char('j'), false, false, true, false), GlobalAction::Navigate(Direction::Down)),
            (Hotkey::new(Key::Char('k'), false, false, true, false), GlobalAction::Navigate(Direction::Up)),
            (Hotkey::new(Key::Char('l'), false, false, true, false), GlobalAction::Navigate(Direction::Right)),
            (Hotkey::new(Key::Char('i'), false, false, true, false), GlobalAction::DockTabPrev),
            (Hotkey::new(Key::Char('o'), false, false, true, false), GlobalAction::DockTabNext),
            (Hotkey::new(Key::Char('o'), true, false, true, false), GlobalAction::FileFinder),
            (Hotkey::new(Key::Char('n'), false, false, true, false), GlobalAction::NewWindow),
            (Hotkey::new(Key::Char('n'), true, false, true, false), GlobalAction::NewFile),
            (Hotkey::new(Key::Char('+'), false, false, true, false), GlobalAction::FontSizeUp),
            (Hotkey::new(Key::Char('='), false, false, true, false), GlobalAction::FontSizeUp),
            (Hotkey::new(Key::Char('-'), false, false, true, false), GlobalAction::FontSizeDown),
            (Hotkey::new(Key::Char('0'), false, false, true, false), GlobalAction::FontSizeReset),
            (Hotkey::new(Key::Char(','), false, false, true, false), GlobalAction::OpenConfig),
            (Hotkey::new(Key::Char('b'), true, false, true, false), GlobalAction::OpenBrowser),
        ]
    }

    /// Create a new KeybindingMap with default bindings.
    pub fn new() -> Self {
        Self {
            bindings: Self::default_bindings(),
        }
    }

    /// Apply user overrides on top of the default bindings.
    pub fn with_overrides(overrides: Vec<(Hotkey, GlobalAction)>) -> Self {
        let mut bindings = Self::default_bindings();
        for (hotkey, action) in overrides {
            // Remove any existing binding for this action
            bindings.retain(|(_, a)| a.action_key() != action.action_key());
            bindings.push((hotkey, action));
        }
        Self { bindings }
    }

    /// Look up a key + modifiers in the binding table. First match wins.
    pub fn lookup(&self, key: &Key, modifiers: &Modifiers) -> Option<GlobalAction> {
        for (hotkey, action) in &self.bindings {
            if hotkey.matches(key, modifiers) {
                return Some(action.clone());
            }
        }
        None
    }

    /// Get the first hotkey bound to a given action.
    pub fn hotkey_for(&self, action: &GlobalAction) -> Option<&Hotkey> {
        self.bindings.iter()
            .find(|(_, a)| a.action_key() == action.action_key())
            .map(|(h, _)| h)
    }
}

impl Default for KeybindingMap {
    fn default() -> Self {
        Self::new()
    }
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
    pub keybinding_map: Option<KeybindingMap>,
}

impl Router {
    /// Create a new Router with default settings.
    pub fn new() -> Self {
        Self {
            focused: None,
            hovered: None,
            dragging_border: false,
            border_threshold: 4.0,
            keybinding_map: None,
        }
    }

    /// Create a new Router with a custom border detection threshold.
    pub fn with_border_threshold(threshold: f32) -> Self {
        Self {
            focused: None,
            hovered: None,
            dragging_border: false,
            border_threshold: threshold,
            keybinding_map: None,
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
        // When a custom keybinding map exists, use it exclusively so that
        // removed/rebound bindings don't fall through to the hardcoded table.
        if let Some(ref map) = self.keybinding_map {
            return map.lookup(&key, &modifiers);
        }

        match key {
            // Cmd+T -> split horizontal (home), Cmd+Shift+T -> split vertical (home)
            Key::Char('t') | Key::Char('T') => {
                if modifiers.shift {
                    Some(GlobalAction::SplitVertical)
                } else {
                    Some(GlobalAction::SplitHorizontal)
                }
            }
            // Cmd+\ -> split horizontal (cwd), Cmd+Shift+\ -> split vertical (cwd)
            Key::Char('\\') | Key::Char('|') => {
                if modifiers.shift {
                    Some(GlobalAction::SplitVerticalHere)
                } else {
                    Some(GlobalAction::SplitHorizontalHere)
                }
            }
            // Cmd+W / Ctrl+W -> close pane
            Key::Char('w') | Key::Char('W') => Some(GlobalAction::ClosePane),
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
            // Cmd+Enter / Ctrl+Enter -> toggle zoom
            Key::Enter => Some(GlobalAction::ToggleZoom),
            // Cmd+Shift+D -> toggle dark/light theme
            Key::Char('d') | Key::Char('D') => {
                if modifiers.shift {
                    Some(GlobalAction::ToggleTheme)
                } else {
                    None
                }
            }
            // Cmd+1 -> FocusArea(Slot1)
            Key::Char('1') | Key::Char('!') => Some(GlobalAction::FocusArea(AreaSlot::Slot1)),
            // Cmd+2 -> FocusArea(Slot2)
            Key::Char('2') | Key::Char('@') => Some(GlobalAction::FocusArea(AreaSlot::Slot2)),
            // Cmd+3 -> FocusArea(Slot3)
            Key::Char('3') | Key::Char('#') => Some(GlobalAction::FocusArea(AreaSlot::Slot3)),
            // Cmd+Arrow -> Navigate
            Key::Up if modifiers.meta => Some(GlobalAction::Navigate(Direction::Up)),
            Key::Down if modifiers.meta => Some(GlobalAction::Navigate(Direction::Down)),
            Key::Left if modifiers.meta => Some(GlobalAction::Navigate(Direction::Left)),
            Key::Right if modifiers.meta => Some(GlobalAction::Navigate(Direction::Right)),
            // Cmd+HJKL -> Navigate
            Key::Char('h') | Key::Char('H') => Some(GlobalAction::Navigate(Direction::Left)),
            Key::Char('j') | Key::Char('J') => Some(GlobalAction::Navigate(Direction::Down)),
            Key::Char('k') | Key::Char('K') => Some(GlobalAction::Navigate(Direction::Up)),
            Key::Char('l') | Key::Char('L') => Some(GlobalAction::Navigate(Direction::Right)),
            // Cmd+I -> dock tab prev
            Key::Char('i') | Key::Char('I') => Some(GlobalAction::DockTabPrev),
            // Cmd+O -> dock tab next
            Key::Char('o') | Key::Char('O') => {
                if modifiers.shift {
                    Some(GlobalAction::FileFinder)
                } else {
                    Some(GlobalAction::DockTabNext)
                }
            }
            // Cmd+N -> new window, Cmd+Shift+N -> new file
            Key::Char('n') | Key::Char('N') => {
                if modifiers.shift {
                    Some(GlobalAction::NewFile)
                } else {
                    Some(GlobalAction::NewWindow)
                }
            }
            // Cmd+= / Cmd++ -> font size up, Cmd+- -> font size down, Cmd+0 -> reset
            Key::Char('+') | Key::Char('=') => Some(GlobalAction::FontSizeUp),
            Key::Char('-') | Key::Char('_') => Some(GlobalAction::FontSizeDown),
            Key::Char('0') => Some(GlobalAction::FontSizeReset),
            // Cmd+, -> open config
            Key::Char(',') => Some(GlobalAction::OpenConfig),
            // Cmd+Shift+B -> open browser
            Key::Char('b') | Key::Char('B') => {
                if modifiers.shift {
                    Some(GlobalAction::OpenBrowser)
                } else {
                    None
                }
            }
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
