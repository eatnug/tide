# Flow: Pane Lifecycle

Create, split, tab, drag, and close Panes.

## Participants

| Context | Role |
|---------|------|
| `tide-app` | Orchestrates lifecycle, maintains panes HashMap |
| `tide-layout` | Manages binary split tree and TabGroups |
| `tide-terminal` | Creates PTY for Terminal panes |
| `tide-editor` | Creates buffer for Editor panes |

## Create: New Tab (Cmd+T)

```
GlobalAction::NewTab
    │
    ▼
new_terminal_tab()
    │
    ├── focused pane exists? → YES
    │   ├── alloc PaneId via layout.alloc_id()
    │   ├── Create Launcher(new_id)
    │   ├── Insert into app.panes HashMap
    │   ├── layout.add_tab(focused_id, new_id)  ← same TabGroup
    │   ├── Set focused = new_id
    │   └── invalidate_chrome()
    │
    └── focused pane exists? → NO
        └── do nothing
```

## Create: Split (Cmd+Shift+T)

```
GlobalAction::SplitVertical
    │
    ▼
split_with_launcher(Vertical)
    │
    ├── Unzoom if zoomed
    ├── new_id = layout.split(focused_id, Vertical)
    │   └── Binary tree: Leaf → Split { left: original, right: new_leaf }
    ├── Create Launcher(new_id)
    ├── Insert into app.panes HashMap
    ├── Set focused = new_id
    └── invalidate_chrome()
```

## Resolve: Launcher → Concrete Pane

```
User presses T/E/O/B in Launcher
    │
    ▼
resolve_launcher(launcher_id, choice)
    │
    ├── T (Terminal): spawn PTY → replace PaneKind::Launcher with PaneKind::Terminal
    ├── E (NewFile): create empty EditorState → PaneKind::Editor
    ├── O (OpenFile): open file finder modal → user picks file → PaneKind::Editor
    └── B (Browser): create WebView → PaneKind::Browser
```

## Open File (from file tree or file finder)

```
open_editor_pane(path)
    │
    ├── File already open in some tab?
    │   └── YES → set_active_tab(existing_id), focus it, RETURN
    │
    └── NO → Create new EditorPane
        ├── alloc PaneId
        ├── EditorState::open(path)
        ├── Insert into app.panes
        ├── layout.add_tab(focused_id, new_id)
        ├── Set focused = new_id
        └── Start file watcher on path
```

## Close Pane (Cmd+W)

```
close_specific_pane(id)
    │
    ├── Is Editor + dirty + has file_path?
    │   └── YES → Show save_confirm modal, RETURN
    │
    ├── Is Editor + dirty + no file_path (untitled)?
    │   └── Close immediately (no prompt)
    │
    └── force_close_editor_panel_tab(id)
        ├── Remove from app.panes HashMap
        ├── layout.remove(id)
        │   ├── Remove from TabGroup
        │   ├── If TabGroup empty → remove Leaf
        │   └── If parent Split has one child → collapse
        ├── Remove file watcher
        ├── Remove IME proxy
        │
        ├── Any panes left?
        │   └── YES → focus adjacent pane
        │
        └── No panes left?
            └── Is this the last workspace?
                ├── YES → Create Launcher (app must have at least one pane)
                └── NO → Close workspace, switch to adjacent
```

## Drag & Drop

```
Mouse down on tab bar
    │
    ▼
PaneDragState::PendingDrag { source, press_pos }
    │
    ├── Mouse moves > threshold?
    │   └── PaneDragState::Dragging { source, drop_target }
    │       │
    │       ├── Mouse over pane → compute DropZone (Top/Bottom/Left/Right/Center)
    │       │   └── layout.simulate_drop() → preview Rect
    │       │
    │       ├── Mouse over workspace sidebar → highlight workspace
    │       │
    │       └── Mouse released
    │           ├── On pane zone → layout.move_pane(source, target, zone)
    │           ├── On workspace → move_pane_to_workspace(source, ws_idx)
    │           └── On root zone → layout.move_pane_to_root(source, zone)
    │
    └── Mouse released before threshold → just a click (focus tab)
```

## Invariants to Maintain

After ANY lifecycle operation:
1. **PaneId sync**: Every id in `layout.pane_ids()` exists in `app.panes`, and vice versa
2. **Focus valid**: `app.focused` is either `None` or a valid key in `app.panes`
3. **At least one pane**: App always has at least one pane (Launcher if needed)

## Related Behavior Tests

```
mod pane_lifecycle:
  - new_editor_pane_adds_a_tab_to_focused_panes_group
  - splitting_creates_a_new_pane_in_the_layout
  - splitting_unzooms_the_focused_pane
  - resolving_launcher_as_new_file_creates_editor
  - closing_an_editor_pane_moves_focus_to_another_pane
  - closing_a_dirty_editor_with_file_shows_save_confirm
  - closing_a_dirty_untitled_editor_does_not_show_save_confirm
  - opening_same_file_twice_activates_existing_tab_instead
```
