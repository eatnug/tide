<div align="center">

![Tide](assets/icon.png)

# Tide

**A GPU-rendered terminal workspace for macOS**

[![Release](https://img.shields.io/github/v/release/team-attention/tide?style=flat-square&color=blue)](https://github.com/team-attention/tide/releases)
[![License](https://img.shields.io/badge/license-MIT-green?style=flat-square)](LICENSE)
![Platform](https://img.shields.io/badge/platform-macOS-lightgrey?style=flat-square)
![Rust](https://img.shields.io/badge/rust-2021-orange?style=flat-square)

</div>

Everything you need stays in one window. Terminals, files, editor, browser — organize them into workspaces, split them, tab them, zoom into one. Context stays with you however you work.

## Features

- **Workspaces** — group related panes together; switch instantly with `Cmd+1‑9`
- **Split tree with tab groups** — every split leaf is a tab group, so you can tile and tab freely in one unified layout
- **Launcher** — new tabs open a quick picker (`[T]` Terminal, `[E]` New File, `[O]` Open File, `[B]` Browser) instead of defaulting to a terminal
- **File tree** — three modes: hidden, overlay, or pinned alongside your panes; cwd follows the focused terminal or editor automatically
- **Workspace sidebar** — hidden or visible; shows each workspace's name, branch, and working directory at a glance
- **Zoom** — expand any pane to fill the workspace, then snap back
- **GPU rendering** — powered by wgpu for smooth, low-latency output

## UI Model

Tide's layout is a binary split tree where every leaf holds a **tab group** — an ordered list of panes with one active tab. Panes live in a global store and can be moved between tab groups or even across workspaces.

```
App
├── workspaces          (independent layout + focus per workspace)
├── panes               (global store: Terminal, Editor, Browser, Diff, Launcher)
├── sidebar mode        (Hidden / Visible)
├── file tree mode      (Hidden / Overlay / Pinned)
└── zoomed pane

Workspace
├── name
├── split tree          (binary tree of splits and tab groups)
└── focused tab group

Tab Group
├── tabs: [PaneId, ...]
└── active tab index
```

**Design decisions:**

- Workspaces have no cwd of their own — new terminals inherit the cwd of the last focused terminal.
- There is no collapsed/icon-only sidebar; it is either hidden or visible (~180 px).
- The file tree is an independent rounded-rect panel with the same visual weight as any pane, separated by uniform 4 px gaps.
- Every visual region (sidebar, file tree, panes) is a rounded rectangle with identical padding and gap rules — no nested chrome, no hierarchy.

## Keybindings

Customizable via `~/.config/tide/settings.json`.

### Global

| Key | Action |
|---|---|
| `Cmd+1‑9` | Switch workspace |
| `Cmd+Shift+N` | New workspace |
| `Cmd+Shift+W` | Close workspace |
| `Cmd+Enter` | Toggle zoom (expand / collapse focused pane) |
| `Cmd+Shift+O` | File finder (overlay) |
| `Cmd+E` | Toggle file tree |

### Panel focus

| Key | Action |
|---|---|
| `Cmd+H/J/K/L` | Move focus across splits |
| `Cmd+Shift+H/L` | Previous / next tab in group |
| `Cmd+T` | New tab (opens launcher) |
| `Cmd+D` | New vertical split + launcher |
| `Cmd+Shift+D` | New horizontal split + launcher |
| `Cmd+W` | Close current tab |

### Sidebar / File tree focus

| Key | Action |
|---|---|
| `j` / `k` | Navigate up / down |
| `Enter` | Select |
| `Esc` | Return focus to panel (closes overlay if transient) |

## Install

Download the latest `.dmg` from [Releases](https://github.com/team-attention/tide/releases).

## Build from Source

```sh
cargo build --release                    # binary
cargo bundle --release -p tide-app       # macOS .app bundle
./scripts/build-dmg.sh                   # signed + notarized DMG
```

## Roadmap

**Phase 1 — Polish the Core**
Passkey support in browser panel, editor improvements (find & replace, multi-cursor), better terminal search UX, clipboard image support.

**Phase 2 — Document Panel**
A first-class document pane for writing and organizing markdown alongside terminals and code. Internal linking, full-text search, inline images.

**Phase 3 — Extensibility**
Plugin system, custom widgets/blocks, CLI tool to control Tide from the terminal, theming API.

**Phase 4 — Cross-Platform**
Linux (Wayland + X11) and Windows support.

### Non-Goals

- **Full IDE** — no LSP, debugger, or project-level refactoring. Tide is a workspace, not an IDE.
- **App Store distribution** — direct DMG distribution only.

## Design Reference

Mockups live in `ui.pen` (Pencil). Screens include normal mode, zoom mode, file tree overlay, sidebar hidden, and launcher pane.

Inspiration: [cmux](https://www.cmux.dev/) — workspace sidebar + splits + session restore.

## Architecture

```
tide-app          Application entry, event loop, rendering
tide-platform     Native macOS platform layer (NSApplication/NSWindow/NSView)
tide-renderer     wgpu-based GPU renderer
tide-terminal     Terminal emulation (alacritty_terminal backend)
tide-editor       Built-in editor
tide-layout       Pane layout engine
tide-tree         File tree
tide-input        Keybinding & input handling
tide-core         Shared types and utilities
```

## License

[MIT](LICENSE)
