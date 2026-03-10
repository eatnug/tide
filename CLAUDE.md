# Tide — Project Rules

## Domain Language (Required)

All code, commits, PRs, and discussions MUST use the terms defined in `docs/glossary.md`.
Before writing code or describing changes, check the glossary. If a term doesn't exist, add it.

Key terms to always use precisely:
- **Pane** (not "panel", "tab", "window") — a content container with a PaneId
- **PaneKind** — the 5 types: Terminal, Editor, Diff, Browser, Launcher
- **Workspace** — an isolated set of panes + layout + focus (not "tab group", "session")
- **TabGroup** — multiple panes stacked in one layout slot (not "workspace")
- **FocusArea** — FileTree or PaneArea (not "focus mode", "focus zone")
- **SplitLayout** — the binary tree of splits (not "grid", "tiling")
- **ModalStack** — mutually-exclusive popups (not "dialog", "overlay")
- **GlobalAction** — a user-intent command from keybinding (not "event", "message")
- **Generation** — monotonic counter for cache invalidation (not "version", "revision")

## Bounded Contexts (Crates)

Each crate is a bounded context. Know which one you're touching:

| Crate | Responsibility | Key Entities |
|-------|---------------|--------------|
| `tide-core` | Shared types & traits | PaneId, Rect, Key, TerminalGrid |
| `tide-layout` | Binary split tree | SplitLayout, TabGroup |
| `tide-terminal` | PTY & grid sync | Terminal, GridSyncer |
| `tide-editor` | Text buffer & cursor | EditorState |
| `tide-input` | Keybinding resolution | Router, Hotkey, GlobalAction |
| `tide-tree` | Filesystem & git status | FsTree |
| `tide-platform` | Native macOS windowing | PlatformEvent, PlatformWindow |
| `tide-renderer` | GPU rendering pipeline | WgpuRenderer, GlyphAtlas |
| `tide-app` | Orchestrator | App, WorkspaceManager, ModalStack |

## Behavior-First Development

When adding a new feature or fixing a bug:

1. **Write behavior tests first** in `crates/tide-app/src/behavior_tests.rs`
2. Test name = natural language sentence: `fn closing_last_pane_in_workspace_shows_launcher()`
3. Place test in the correct domain module (see existing modules in the file)
4. Then implement until tests pass

See `docs/testing/behavior-tests.md` for the full guide.

## Commit Messages

Format: `<verb> <what> in <bounded-context>`

```
Add pane drag preview in tide-app
Fix TabGroup active index after close in tide-layout
Extract GridSyncer dirty tracking in tide-terminal
```

- Verb: Add (new feature), Fix (bug), Extract (refactor), Remove, Update
- What: Use domain terms from glossary
- Bounded context: Which crate is primarily affected

## PR Description

Follow the template in `.github/PULL_REQUEST_TEMPLATE.md`. Must include:
- Which Bounded Context(s) are touched
- Which Entities/Aggregates are modified
- Which Invariants are preserved or changed
- Which behavior tests were added

## Architecture Invariants

These must NEVER be violated:

1. **PaneId sync**: Every PaneId in SplitLayout MUST exist in App.panes HashMap, and vice versa
2. **Single active workspace**: Only the active Workspace is loaded into App fields; others are cold-stored in WorkspaceManager
3. **Modal exclusivity**: At most one modal in ModalStack can be open at a time
4. **Input routing priority**: Modal → FocusArea → Router → TextInput (never skip a level)
5. **Generation monotonicity**: chrome_generation and pane_generations only increase, never decrease or reset **within a workspace session**. Exception: pane_generations is cleared on workspace switch (entirely new pane set)
6. **IME proxy lifecycle**: Every pane with keyboard focus must have an active IME proxy; proxy must be synced on every event

## File Structure

- `docs/glossary.md` — Single source of truth for all domain terms
- `docs/context-map.md` — How bounded contexts relate
- `docs/domain/*.md` — Per-context deep dives
- `docs/flows/*.md` — Cross-cutting use case flows
- `docs/testing/behavior-tests.md` — How to write behavior tests
- `crates/tide-app/src/behavior_tests.rs` — Living specification (117+ tests)
