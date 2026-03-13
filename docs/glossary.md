# Glossary — Ubiquitous Language

Terms used consistently across the Tide codebase. When adding new code, use these terms exactly.

## Entities (have identity)

| Term | Type | Location | Description |
|------|------|----------|-------------|
| **Pane** | `PaneKind` | `tide-app/pane.rs` | A content container identified by `PaneId`. Can be Terminal, Editor, Diff, Browser, or Launcher. |
| **PaneId** | `u64` | `tide-core` | Unique identity of a pane. Allocated incrementally by `SplitLayout::alloc_id()`. |
| **Workspace** | `Workspace` | `tide-app/workspace.rs` | An isolated set of panes + layout + focus. Only one is active at a time. |
| **TabGroup** | `TabGroup` | `tide-layout/tab_group.rs` | Multiple panes stacked in one layout slot. Only the active tab renders. |
| **Terminal** | `Terminal` | `tide-terminal` | A PTY backend instance. Owns the shell process and grid state. |
| **EditorState** | `EditorState` | `tide-editor` | A text buffer with cursor, undo stack, and syntax highlighting. |
| **LspClient** | `LspClient` | `tide-lsp` | Manages communication with one language server process via JSON-RPC over stdio. |
| **LspManager** | `LspManager` | `tide-app` | Owns all LspClient instances (one per language). Orchestrates start/stop and request routing. |
| **CompletionPopup** | `CompletionState` | `tide-app` | Per-EditorPane inline autocomplete dropdown. NOT part of ModalStack — coexists with typing. |

## Value Objects (identity-less, compared by value)

| Term | Type | Location | Description |
|------|------|----------|-------------|
| **Rect** | `Rect` | `tide-core` | `{x, y, width, height}` — a positioned rectangle. |
| **Size** | `Size` | `tide-core` | `{width, height}` — dimensions without position. |
| **Key** | `Key` | `tide-core` | A keyboard key (`Char('a')`, `Enter`, `F(1)`, etc.). |
| **Modifiers** | `Modifiers` | `tide-core` | `{shift, ctrl, alt, meta}` — modifier key state. |
| **Hotkey** | `Hotkey` | `tide-input` | A `Key` + `Modifiers` combination that maps to a `GlobalAction`. |
| **Color** | `Color` | `tide-core` | RGBA float color. |
| **TextStyle** | `TextStyle` | `tide-core` | Bold/dim/italic/underline + fg/bg color. |
| **TerminalCell** | `TerminalCell` | `tide-core` | One character + its `TextStyle`. |
| **TerminalGrid** | `TerminalGrid` | `tide-core` | 2D array of `TerminalCell` — the terminal's visible content. |
| **CursorState** | `CursorState` | `tide-core` | Position + visibility + shape of a terminal cursor. |
| **DropTarget** | `DropTarget` | `tide-core` | Where a dragged pane can land: `Pane(id, zone)` or `Root(zone)`. |

## Aggregates (consistency boundaries)

| Term | Root Entity | Description |
|------|-------------|-------------|
| **App** | `App` | The top-level aggregate. Owns all panes, layout, focus, modals, workspaces. All mutations go through App methods. |
| **SplitLayout** | `SplitLayout` | The binary tree of splits and tab groups. Enforces layout invariants (min ratio, tree balance). |
| **WorkspaceManager** | `WorkspaceManager` | Owns the workspace list and active index. Swaps layout/panes/focus on switch. |
| **ModalStack** | `ModalStack` | Mutually-exclusive popups. At most one modal is open at a time. |

## Domain Events (things that happened)

| Term | Type | Location | Description |
|------|------|----------|-------------|
| **PlatformEvent** | `PlatformEvent` | `tide-platform` | Raw OS event: key press, mouse click, resize, IME commit, etc. |
| **InputEvent** | `InputEvent` | `tide-core` | Normalized input: `KeyPress`, `MouseClick`, `MouseScroll`, `Resize`. |

## Commands (intent to mutate)

| Term | Type | Location | Description |
|------|------|----------|-------------|
| **GlobalAction** | `GlobalAction` | `tide-input` | A user-intent command: `SplitVertical`, `ClosePane`, `Navigate(Up)`, `ToggleZoom`, etc. 31 variants. |
| **Action** | `Action` | `tide-input` | Routing decision: `RouteToPane(id)`, `GlobalAction(...)`, `DragBorder(pos)`, or `None`. |
| **EditorAction** | `EditorAction` | `tide-editor` | Editor-specific command: `InsertChar`, `Backspace`, `Save`, `Undo`, etc. |
| **WindowCommand** | `WindowCommand` | `tide-platform` | App→window command: `RequestRedraw`, `SetFullscreen`, `CreateImeProxy`, etc. |

## Domain Concepts

| Term | Type | Description |
|------|------|-------------|
| **FocusArea** | `FocusArea` | Which region has keyboard focus: `FileTree` or `PaneArea`. |
| **AreaSlot** | `AreaSlot` | Positional slot (`Slot1`/`Slot2`/`Slot3`) for Cmd+1/2/3 focus cycling. |
| **Direction** | `Direction` | `Up`/`Down`/`Left`/`Right` for pane navigation. |
| **SplitDirection** | `SplitDirection` | `Horizontal` (top/bottom) or `Vertical` (left/right) split. |
| **DropZone** | `DropZone` | Which edge of a pane to drop on: `Top`/`Bottom`/`Left`/`Right`/`Center`. |
| **PaneKind** | enum | The 5 content types: `Terminal`, `Editor`, `Diff`, `Browser`, `Launcher`. |
| **CursorShape** | enum | Terminal cursor appearance: `Block`, `Beam`, `Underline`. |
| **CompletionItem** | struct | A single completion suggestion: label, kind, insertText, sortText. |
| **Generation** | `u64` | Monotonic counter for cache invalidation. Incremented on state change. |
| **Ratio** | `f32` | Split position (0.0–1.0). Clamped to [0.1, 0.9] minimum. |
| **Cell Size** | `Size` | Pixel dimensions of one terminal character cell (font-dependent). |

## Infrastructure Concepts

| Term | Description |
|------|-------------|
| **PTY** | Pseudo-terminal. The OS mechanism connecting Tide to a shell process. |
| **Sync Thread** | Background thread that copies terminal grid data, converts colors, and diffs changes. |
| **Render Thread** | Dedicated background thread (`render_thread.rs`) for GPU drawable acquisition and frame submission. Decouples CAMetalLayer blocking from the main App thread. |
| **IME Proxy** | Per-pane `NSTextInputClient` view for Input Method Editor composition. |
| **Glyph Atlas** | GPU texture cache of rendered font glyphs (MSDF format). |
| **Dirty Tracking** | Generation-based system to skip re-rendering unchanged panes/chrome. |
