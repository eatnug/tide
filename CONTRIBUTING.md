# Contributing to Tide

Thanks for your interest in contributing to Tide!

## Getting Started

### Prerequisites

- Rust (stable, edition 2021)
- macOS 13.0+ (native platform layer currently macOS-only)
- Xcode Command Line Tools

### Building

```sh
# Debug build
cargo build

# Release build
cargo build --release

# macOS .app bundle
cargo bundle --release -p tide-app
```

### Running

```sh
cargo run --release -p tide-app
```

### Tests

```sh
cargo test --workspace
```

## Architecture

Tide is split into independent crates with clear boundaries:

| Crate | Role |
|---|---|
| `tide-core` | Shared types, traits (no logic) |
| `tide-renderer` | GPU rendering via wgpu + cosmic-text |
| `tide-terminal` | PTY management, alacritty_terminal backend |
| `tide-layout` | Split pane layout tree |
| `tide-tree` | File tree with git status, filesystem watching |
| `tide-input` | Keybinding map, input routing |
| `tide-editor` | Text editor, syntax highlighting, diff viewer |
| `tide-platform` | Native macOS windowing (NSWindow, NSView, IME) |
| `tide-app` | Application entry point, event loop, UI composition |

### Platform Layer

The platform layer (`tide-platform`) uses `objc2` for native macOS interop instead of cross-platform abstractions like winit. This gives us direct control over IME handling (critical for CJK input), window lifecycle, and event routing.

All Objective-C → Rust callbacks are wrapped in `catch_unwind` to prevent panics from crossing the FFI boundary.

### Rendering

GPU rendering uses a multi-layer architecture:
1. **Grid layer** — terminal cell content (cached, rebuilt on content change)
2. **Chrome layer** — UI elements like file tree, tab bars, headers (cached)
3. **Overlay layer** — cursor, IME preedit, selection (rebuilt every frame)
4. **Top layer** — search bar, popups (above all text)

## Pull Requests

1. Fork the repo and create a branch from `main`
2. Make your changes
3. Run `cargo test --workspace` and `cargo clippy --workspace`
4. Open a PR with a clear description of what changed and why

## Code Style

- Follow existing patterns in the codebase
- Use `pub(crate)` for internal APIs, minimize `pub` exports
- Prefer `if let` / `match` over `.unwrap()` in production code
- Keep `unsafe` blocks minimal and document safety invariants

## License

By contributing, you agree that your contributions will be licensed under the [MIT License](LICENSE).
