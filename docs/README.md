# Tide System Documentation

A DDD-style living specification of the Tide terminal emulator.

## How to Read This

Each document maps to a **Bounded Context** — a self-contained area of the system with its own language and rules. Start with the glossary, then explore the context that interests you.

## Documents

### Foundation
- **[Glossary](glossary.md)** — Ubiquitous language. Every domain term defined in one place.
- **[Context Map](context-map.md)** — How the 8 bounded contexts relate to each other.

### Bounded Contexts (by crate)
- **[Core Types](domain/core-types.md)** — Value Objects and trait contracts shared across all contexts. (`tide-core`)
- **[Layout](domain/layout.md)** — Binary split tree, tab groups, pane arrangement. (`tide-layout`)
- **[Terminal](domain/terminal.md)** — PTY management, grid synchronization, threading. (`tide-terminal`)
- **[Editor](domain/editor.md)** — Text buffer, cursor, syntax highlighting, undo. (`tide-editor`)
- **[Input Routing](domain/input.md)** — Keybinding resolution, hotkey matching, action dispatch. (`tide-input`)
- **[File Tree](domain/file-tree.md)** — Filesystem watching, directory traversal, git status. (`tide-tree`)
- **[Platform](domain/platform.md)** — Native macOS windowing, IME, event sourcing. (`tide-platform`)
- **[Renderer](domain/renderer.md)** — GPU pipeline, glyph atlas, dirty tracking. (`tide-renderer`)

### Application Layer
- **[App Orchestrator](domain/app.md)** — The App aggregate: sub-modules, state management, the update/render loop. (`tide-app`)

### Domain Flows (Use Cases)
- **[Keystroke → Action](flows/keystroke-to-action.md)** — How a keypress becomes a system mutation.
- **[Pane Lifecycle](flows/pane-lifecycle.md)** — Create, split, tab, drag, close panes.
- **[Terminal Output](flows/terminal-output.md)** — PTY bytes → grid cells → GPU pixels.
- **[Workspace Switch](flows/workspace-switch.md)** — Save/load/switch workspace state.
- **[Modal Interactions](flows/modal-interactions.md)** — File finder, git switcher, save confirm, context menu.

### Living Tests
- **[Behavior Test Guide](testing/behavior-tests.md)** — How to read and write behavioral tests as specification.
