# Flow: Workspace Switch

How the active Workspace is saved, swapped, and loaded.

## Participants

| Context | Role |
|---------|------|
| `tide-app` | WorkspaceManager owns the workspace list |
| `tide-layout` | SplitLayout is saved/restored via LayoutSnapshot |
| `tide-platform` | IME proxies and WebView frames are synced |

## Core Pattern: Swap

The active Workspace's state lives directly in App fields (for fast access).
Inactive Workspaces are cold-stored in `WorkspaceManager.workspaces[i]`.

Switching = **save current → update index → load target**.

## Sequence: Cmd+] (WorkspaceNext)

```
GlobalAction::WorkspaceNext
    │
    ▼
switch_workspace(next_idx)
    │
    ▼
┌─ Step 1: Hide current WebViews ────────────────┐
│ For each Browser pane in current workspace:      │
│   Hide WKWebView frame (off-screen)             │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ Step 2: save_active_workspace() ───────────────┐
│ workspace[active].layout = app.layout.snapshot() │
│ workspace[active].panes  = app.panes.drain()     │
│ workspace[active].focused = app.focused           │
│ workspace[active].focus_area = app.focus_area     │
│ workspace[active].zoomed = app.zoomed_pane        │
│ workspace[active].search = app.search_focus       │
│                                                  │
│ After: app.panes is EMPTY, app.layout is EMPTY   │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ Step 3: Update index ──────────────────────────┐
│ ws.active = next_idx                             │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ Step 4: load_active_workspace() ───────────────┐
│ app.layout = from_snapshot(workspace[active].layout) │
│ app.panes  = workspace[active].panes.drain()     │
│ app.focused = workspace[active].focused           │
│ app.focus_area = workspace[active].focus_area     │
│ app.zoomed_pane = workspace[active].zoomed        │
│ app.search_focus = workspace[active].search       │
│                                                  │
│ After: workspace[active] is EMPTY (data in App)  │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ Step 5: Full invalidation ─────────────────────┐
│ cache.pane_generations.clear()                   │
│   → Forces full redraw of all panes             │
│ compute_layout()                                 │
│   → Recompute Rects for new layout              │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ Step 6: Restore WebViews ──────────────────────┐
│ sync_browser_webview_frames()                    │
│   → Position WKWebViews to their new Rects      │
└──────────────────────────────────────────────────┘
```

## Cross-Workspace Pane Drag

```
User drags pane tab to workspace sidebar item
    │
    ▼
move_pane_to_workspace(pane_id, target_ws_idx)
    │
    ├── Remove pane from current layout + panes
    ├── save_active_workspace()
    ├── ws.active = target_ws_idx
    ├── load_active_workspace()
    ├── Insert pane into loaded panes
    ├── Add pane to loaded layout
    ├── Focus the dragged pane
    └── Full invalidation + compute_layout()
```

## Workspace Entity

```rust
Workspace {
    name: String,
    layout: Option<LayoutSnapshot>,   // None when active (data in App)
    panes: HashMap<PaneId, PaneKind>, // Empty when active (data in App)
    focused: Option<PaneId>,
    focus_area: FocusArea,
    zoomed: Option<PaneId>,
    search: Option<PaneId>,
}
```

## Invariants

1. **Only one active**: Exactly one workspace has its data in App fields; all others in cold storage
2. **No shared PaneIds**: A PaneId belongs to exactly one workspace
3. **Full invalidation on switch**: All pane_generations cleared to force complete redraw
4. **IME proxy sync**: After load, IME proxies must be recreated for new panes

## Related Behavior Tests

```
mod workspace_behavior:
  - switching_workspace_preserves_layout
  - switching_workspace_restores_focus
  - new_workspace_starts_with_launcher
  - closing_last_workspace_creates_new_one
  - workspace_prev_next_wraps_around
  - moving_pane_to_workspace_removes_from_source
```
