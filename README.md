# Tide

A terminal that doesn't make you leave.

## What

Working in the terminal, you lose context constantly. Open VS Code to read a file, Finder to browse directories, another window for diffs. One task, three apps.

Tide keeps everything in one screen. A file tree sits next to your terminal. Click a file and the editor opens beside it. The terminal stays right where it is.

Long-term, Tide aims to be an integrated workspace built around the terminal — what [Wave Terminal](https://waveterm.dev) does with web tech, but with native GPU rendering in Rust.

## Core Ideas

- **Don't break context** — view and edit files without leaving the terminal
- **The terminal is the center** — this is not an IDE. Only what the terminal needs, nothing more
- **Native performance** — no Electron, GPU-rendered directly via wgpu

## Features

### Split Panes

Split your terminal horizontally or vertically. Drag borders to resize. Each pane has its own shell, scrollback, and working directory.

Switch to stacked mode to show one pane at a time with a tab bar.

### File Tree

Follows the working directory of the focused terminal. Switch pane focus and the tree updates.

- Real-time filesystem watching
- Git status badges
- Click to open in the editor dock

### Editor Dock

View and edit files alongside your terminal.

- Syntax highlighting
- Search
- Git diff view
- Disk change detection (notifies when files change externally)
- Tabbed file management

### Focus System

Switch between three areas with `Cmd+1/2/3`.

| Key | Area |
|---|---|
| `Cmd+1` | File Tree |
| `Cmd+2` | Pane Area |
| `Cmd+3` | Editor Dock |

Each key cycles through **show + focus → focus → hide**. Use `Cmd+H/J/K/L` to navigate within areas, `Cmd+Enter` for fullscreen zoom.

### Drag & Drop

Drag panes to rearrange layouts. Drop zones for top/bottom/left/right + swap.

### Session Restore

Layout, open tabs, split ratios, and focus state are saved automatically and restored on next launch.

## Keybindings

Customizable via `~/.config/tide/settings.json`.

### Navigation

| Key | Action |
|---|---|
| `Cmd+1` / `2` / `3` | Toggle area |
| `Cmd+H/J/K/L` | Navigate within area |
| `Cmd+Enter` | Toggle zoom |
| `Cmd+I` / `Cmd+O` | Dock tab prev / next |

### Panes

| Key | Action |
|---|---|
| `Cmd+T` | Split horizontal (home) |
| `Cmd+Shift+T` | Split vertical (home) |
| `Cmd+\` | Split horizontal (cwd) |
| `Cmd+Shift+\` | Split vertical (cwd) |
| `Cmd+W` | Close pane |

### General

| Key | Action |
|---|---|
| `Cmd+Shift+O` | File finder |
| `Cmd+F` | Terminal search |
| `Cmd+Shift+D` | Toggle dark / light |
| `Cmd+=` / `Cmd+-` | Font size up / down |
| `Cmd+,` | Settings |

## Tech Stack

| | |
|---|---|
| Language | Rust |
| GPU | wgpu |
| Text | cosmic-text + CoreText fallback |
| Terminal | alacritty_terminal |
| Syntax | syntect |
| Window | tide-platform (native macOS) |
| File watch | notify |

## Architecture

```
tide/
  crates/
    tide-core/        shared types, traits
    tide-renderer/    wgpu GPU rendering
    tide-terminal/    PTY, terminal emulation
    tide-layout/      split pane layout engine
    tide-tree/        file tree
    tide-input/       keybinding, input routing
    tide-editor/      editor, diff viewer
    tide-platform/    native macOS platform layer
    tide-app/         app entry point
```

## Build

```sh
cargo build --release                    # binary
cargo bundle --release -p tide-app       # macOS .app bundle
```

## License

[MIT](LICENSE)
