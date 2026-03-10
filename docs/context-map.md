# Context Map

How Tide's 8 bounded contexts (crates) relate to each other.

```
┌─────────────────────────────────────────────────────────┐
│                    tide-app (Orchestrator)               │
│                                                         │
│  Owns: App aggregate, PaneKind, WorkspaceManager,       │
│        ModalStack, RenderCache, InteractionState,       │
│        FileTreeModel, ImeState                          │
│                                                         │
│  Consumes ALL other contexts                            │
└────┬──────┬──────┬──────┬──────┬──────┬──────┬─────────┘
     │      │      │      │      │      │      │
     ▼      ▼      ▼      ▼      ▼      ▼      ▼
┌────────┐┌──────┐┌──────┐┌──────┐┌─────┐┌──────┐┌──────────┐
│ input  ││layout││termi-││editor││tree ││render││ platform │
│        ││      ││nal   ││      ││     ││er    ││          │
│Router  ││Split-││Termi-││Edit- ││FsT- ││Wgpu-││Platform- │
│Hotkey  ││Layout││nal   ││orSta-││ree  ││Rende-││Event     │
│Global- ││Tab-  ││Grid- ││te    ││     ││rer   ││Platform- │
│Action  ││Group ││Syncer││      ││     ││      ││Window    │
└───┬────┘└──┬───┘└──┬───┘└──┬───┘└──┬──┘└──┬───┘└────┬─────┘
    │        │       │       │       │      │         │
    ▼        ▼       ▼       ▼       ▼      ▼         ▼
┌─────────────────────────────────────────────────────────┐
│                 tide-core (Shared Kernel)                │
│                                                         │
│  Types: PaneId, Rect, Size, Key, Modifiers, Color,     │
│         TerminalGrid, TerminalCell, CursorState         │
│  Traits: Renderer, Pane, LayoutEngine, TerminalBackend, │
│          FileTreeSource, InputRouter                    │
└─────────────────────────────────────────────────────────┘
```

## Relationships

### Shared Kernel: `tide-core`
All crates depend on `tide-core` for common types and trait definitions. This is the shared vocabulary — changing a type here affects everything.

### Upstream/Downstream

| Upstream (provides) | Downstream (consumes) | Relationship |
|---------------------|----------------------|--------------|
| `tide-core` | All crates | **Shared Kernel** — common types and traits |
| `tide-platform` | `tide-app` | **Anti-Corruption Layer** — translates native macOS events into domain events |
| `tide-input` | `tide-app` | **Conformist** — app conforms to Action/GlobalAction vocabulary |
| `tide-layout` | `tide-app` | **Conformist** — app uses SplitLayout API directly |
| `tide-terminal` | `tide-app` | **Open Host Service** — Terminal exposes grid snapshots via trait |
| `tide-editor` | `tide-app` | **Open Host Service** — EditorState exposes buffer/cursor via methods |
| `tide-tree` | `tide-app` | **Open Host Service** — FsTree exposes visible entries via trait |
| `tide-renderer` | `tide-app` | **Open Host Service** — WgpuRenderer implements Renderer trait |

### Key Integration Points

1. **Platform → App**: `PlatformEvent` is the only way outside world enters the system
2. **App → Input**: `Router.process(InputEvent)` returns `Action`
3. **App → Layout**: `SplitLayout.compute()` returns `Vec<(PaneId, Rect)>`
4. **App → Terminal**: `Terminal.process()` consumes PTY output; `Terminal.grid()` reads state
5. **App → Renderer**: `WgpuRenderer.begin_frame()` / `draw_*()` / `end_frame()`
6. **App → Platform**: `WindowCommand` channel for redraw requests, IME proxy management

## Invariants Across Contexts

- A `PaneId` is unique within a `WorkspaceManager` — no two panes share the same ID
- `SplitLayout` and `App.panes` HashMap must stay in sync — every ID in layout exists in the map
- Only the **active workspace** is loaded into App fields; others are stored in `WorkspaceManager.workspaces`
- `ModalStack` allows at most one open modal; `is_any_open()` gates input routing
