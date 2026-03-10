# App Orchestrator — tide-app

**Role**: The Application Service that coordinates all Bounded Contexts.
All user input enters through App, and all state mutations happen through App methods.

## Aggregate: App

`crates/tide-app/src/main.rs`

App is the system's root Aggregate. State is partitioned into 6 sub-modules:

```
App
├── panes: HashMap<PaneId, PaneKind>    ← all Pane entities
├── layout: SplitLayout                  ← arrangement Aggregate
├── focused: Option<PaneId>              ← current focus
├── focus_area: FocusArea                ← FileTree | PaneArea
│
├── ime: ImeState                        ← IME composition state
├── modal: ModalStack                    ← popups (mutually exclusive)
├── cache: RenderCache                   ← render cache + Generation tracking
├── interaction: InteractionState        ← mouse/drag/scroll
├── ft: FileTreeModel                    ← file tree + git status
└── ws: WorkspaceManager                 ← workspace list
```

## Sub-Modules

### ImeState (`ui_state.rs`)
CJK input method composition state. Manages per-Pane IME proxy lifecycle.

| Field | Type | Description |
|-------|------|-------------|
| `composing` | `bool` | Whether IME composition is active |
| `preedit` | `String` | Uncommitted text |
| `pending_creates` | `Vec<u64>` | IME proxies awaiting creation |
| `pending_removes` | `Vec<u64>` | IME proxies awaiting removal |

### ModalStack (`ui_state.rs`)
**Invariant: At most one open at a time.** When `is_any_open()` is true, input routes to the modal.

| Modal | Trigger | Purpose |
|-------|---------|---------|
| `file_finder` | Shift+Shift | File search |
| `git_switcher` | Cmd+G | Branch/worktree switch |
| `config_page` | Cmd+, | Settings overlay |
| `save_confirm` | Closing dirty editor | Save confirmation |
| `save_as_input` | Cmd+Shift+S | Save as dialog |
| `context_menu` | Right-click | Context menu |
| `file_tree_rename` | R key (file tree) | Inline rename |
| `branch_cleanup` | Branch delete | Delete confirmation |

### RenderCache (`ui_state.rs`)
Generation-based dirty tracking. Minimizes GPU re-rendering.

```
invalidate_chrome() → chrome_generation += 1
invalidate_pane(id) → pane_generations[id] += 1
needs_redraw = true → GPU work on next frame
```

### InteractionState (`ui_state.rs`)
Mouse interaction state machine.

```
PaneDragState: Idle → PendingDrag → Dragging
                         (threshold)    (drop target computation)
```

### FileTreeModel (`ui_state.rs`)
File tree + git status cache. CWD tracking → sticky git root.

### WorkspaceManager (`workspace.rs`)
**Core pattern: Swap**
```
switch_workspace(idx):
  1. save_active_workspace()   ← App fields → Workspace[active]
  2. ws.active = idx
  3. load_active_workspace()   ← Workspace[idx] → App fields
  4. Clear all pane_generations (full redraw)
```

## Event Flow

```
PlatformEvent (from OS)
    │
    ▼
handle_platform_event()
    │
    ├── KeyDown → handle_key_down()
    │               │
    │               ├── Modal open? → modal consumes it
    │               ├── FocusArea == FileTree? → file tree key handling
    │               ├── Router.process() → Action
    │               │     ├── GlobalAction → handle_action()
    │               │     ├── RouteToPane → send_text_to_target()
    │               │     └── None → ignored
    │               └── Plain text? → send_text_to_target()
    │
    ├── MouseDown → handle_mouse_down()
    │               ├── hit test: which Pane/tab/button?
    │               ├── tab bar → focus or start drag
    │               └── Pane area → focus + selection start
    │
    ├── ImeCommit → handle_ime_commit()
    │               └── send_text_to_target()
    │
    └── Resized → reconfigure_surface() + compute_layout()

    ▼
update()  (every frame)
    ├── Terminal.process() — consume PTY output
    ├── File watcher — editor reload
    ├── Git poller — file tree status refresh
    └── Animations (scroll, cursor blink)

    ▼
render()  (when needs_redraw == true)
    └── Submit GPU frame
```

## Input Routing Priority

This order **must never be skipped** (Invariant):

```
1. config_page    (highest — blocks all input)
2. context_menu   (ESC to dismiss)
3. save_confirm   (Y/N/ESC)
4. save_as_input  (text input + ESC)
5. file_finder    (text input + arrows + ESC)
6. git_switcher   (text input + arrows + ESC)
7. file_tree_rename (text input + ESC)
8. FocusArea dispatch (FileTree or PaneArea)
9. Router.process() → GlobalAction
10. Text input → send_text_to_target()
```

## Key Methods

| Method | File | Role |
|--------|------|------|
| `handle_key_down()` | `event_handler/keyboard.rs` | Key event routing entry point |
| `handle_action()` | `action/mod.rs` | GlobalAction dispatch |
| `handle_focus_area()` | `action/mod.rs` | FocusArea 3-state toggle |
| `focus_terminal()` | `action/mod.rs` | Pane focus + Generation update |
| `new_editor_pane()` | `action/pane_lifecycle.rs` | Create editor tab |
| `split_with_launcher()` | `action/pane_lifecycle.rs` | Split Pane |
| `close_specific_pane()` | `action/pane_lifecycle.rs` | Close Pane (may trigger modal) |
| `switch_workspace()` | `workspace.rs` | Workspace switch (swap pattern) |
| `compute_layout()` | `layout_compute.rs` | Window size → Pane Rect calculation |
| `update()` | `update.rs` | Per-frame state update |

## Invariants

1. **PaneId sync**: `layout.pane_ids()` ⊆ `panes.keys()` ∧ `panes.keys()` ⊆ `layout.pane_ids()`
2. **Modal exclusivity**: At most 1 field in `modal` is `Some`
3. **Input routing order**: The priority chain above is never skipped
4. **Generation monotonicity**: `chrome_generation` and `pane_generations[id]` only increase within a workspace session. Exception: `pane_generations` is cleared on workspace switch (entirely new pane set loaded)
5. **Workspace isolation**: Inactive Workspace Panes are NOT in App.panes
6. **IME proxy sync**: The focused Pane always has an active IME proxy
