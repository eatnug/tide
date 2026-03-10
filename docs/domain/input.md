# Input Routing — tide-input

**Role**: Resolves keyboard/mouse events into domain commands (GlobalAction or RouteToPane).
Knows nothing about Pane content — only which Pane is focused and what keys were pressed.

`crates/tide-input/src/lib.rs`

## Aggregate: Router

```rust
Router {
    focused: Option<PaneId>,            // Currently focused pane
    hovered: Option<PaneId>,            // Mouse hover target
    dragging_border: bool,              // Border drag in progress
    border_threshold: f32,              // Hit-test threshold (default 4.0px)
    keybinding_map: Option<KeybindingMap>, // User-customizable keybindings
}
```

### InputEvent → Action Flow

```
InputEvent
    │
    ├── KeyPress { key, modifiers }
    │     │
    │     ├── Has Cmd/Ctrl modifier? → keybinding_map.lookup(key, mods)
    │     │     ├── Found → Action::GlobalAction(action)
    │     │     └── Not found → Action::RouteToPane(focused)
    │     │
    │     └── No command modifier → Action::RouteToPane(focused)
    │
    ├── MouseClick { position, button }
    │     ├── Near split border? → Action::DragBorder
    │     └── On a pane? → Action::RouteToPane(pane_at_position)
    │
    ├── MouseDrag { position }
    │     ├── Dragging border? → Action::DragBorder(position)
    │     └── Otherwise → Action::RouteToPane(pane_under_cursor)
    │
    ├── MouseScroll { delta, position }
    │     └── Action::RouteToPane(pane_at_position)
    │
    └── MouseMove / Resize → Action::None
```

## Value Object: Hotkey

```rust
Hotkey {
    key: Key,       // Base key
    shift: bool,    // Shift modifier
    ctrl: bool,     // Ctrl modifier
    meta: bool,     // Cmd modifier (macOS primary)
    alt: bool,      // Alt modifier
}
```

**Matching**: Character keys are case-insensitive. All modifiers must match exactly.

## Value Object: KeybindingMap

```rust
KeybindingMap {
    bindings: Vec<(Hotkey, GlobalAction)>,
}
```

- 31 default bindings hardcoded
- `with_overrides(user_bindings)` layers user customization on top
- `lookup(key, modifiers)` → first match wins

## Command: GlobalAction (35 variants)

### Pane Operations
| Action | Default Binding | Description |
|--------|----------------|-------------|
| `SplitVertical` | Cmd+Shift+T | Split pane left/right |
| `SplitHorizontal` | Cmd+\ | Split pane top/bottom |
| `ClosePane` | Cmd+W | Close focused pane |
| `ToggleZoom` | Cmd+Enter | Zoom/unzoom focused pane |
| `Navigate(Direction)` | Cmd+HJKL | Move focus between panes |
| `TabPrev` / `TabNext` | Cmd+I / Cmd+O | Cycle tabs in TabGroup |
| `NewTab` | Cmd+T | New tab (Launcher) |
| `NewFile` | — | New empty editor |

### Global UI
| Action | Default Binding | Description |
|--------|----------------|-------------|
| `FocusArea(Slot)` | Cmd+1/2/3 | Toggle focus between areas |
| `FileFinder` | Cmd+Shift+O | Open file finder modal |
| `Find` | Cmd+F | Search in pane |
| `Paste` / `Copy` | Cmd+V / Cmd+C | Clipboard |
| `OpenConfig` | Cmd+, | Settings page |
| `ToggleTheme` | Cmd+Shift+D | Dark/light mode |
| `FontSizeUp/Down/Reset` | Cmd+=/Cmd+-/Cmd+0 | Font size |
| `ToggleFullscreen` | Cmd+Ctrl+F | Fullscreen |
| `NewWindow` | Cmd+N | New window |
| `OpenBrowser` | Cmd+Shift+B | Browser pane |
| `BrowserBack/Forward` | Cmd+Shift+[/] | Browser navigation |
| `ScrollHalfPageUp/Down` | Cmd+U / Cmd+D | Half-page scroll |

### Workspace
| Action | Default Binding | Description |
|--------|----------------|-------------|
| `WorkspacePrev/Next` | Cmd+[ / Cmd+] | Cycle workspaces |
| `NewWorkspace` | Cmd+Shift+N | Create workspace |
| `CloseWorkspace` | Cmd+Shift+W | Close workspace |

### Sidebar
| Action | Default Binding | Description |
|--------|----------------|-------------|
| `ToggleFileTree` | Cmd+E | Show/hide file tree |
| `ToggleWorkspaceSidebar` | Cmd+B | Show/hide workspace list |

## Command: Action (routing decision)

```rust
enum Action {
    GlobalAction(GlobalAction),     // System-wide command
    RouteToPane(PaneId),            // Send input to specific pane
    DragBorder(Vec2),               // Split border being dragged
    None,                           // No action
}
```
