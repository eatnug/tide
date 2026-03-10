# Core Types — tide-core

**Role**: Shared Kernel. Every crate depends on tide-core for common types and trait contracts.
Changing a type here affects the entire system.

`crates/tide-core/src/lib.rs`

## Value Objects

### Geometry
| Type | Fields | Purpose |
|------|--------|---------|
| `Rect` | `x, y, width, height: f32` | Positioned rectangle. Methods: `contains()`, `clip_to()` |
| `Size` | `width, height: f32` | Dimensions without position |
| `Vec2` | `x, y: f32` | Point or offset |

### Identity
| Type | Definition | Purpose |
|------|-----------|---------|
| `PaneId` | `u64` | Unique pane identity. Allocated by `SplitLayout::alloc_id()` |

### Input
| Type | Description |
|------|-------------|
| `Key` | `Char(char), Enter, Backspace, Tab, Escape, Delete, Up, Down, Left, Right, Home, End, PageUp, PageDown, F(u8), Insert` |
| `Modifiers` | `{ shift, ctrl, alt, meta: bool }` — all default false |
| `MouseButton` | `Left, Right, Middle` |
| `InputEvent` | `KeyPress`, `MouseClick`, `MouseMove`, `MouseDrag`, `MouseScroll`, `Resize` |

### Styling
| Type | Description |
|------|-------------|
| `Color` | `{ r, g, b, a: f32 }` — Constants: `BLACK`, `WHITE` |
| `TextStyle` | `{ foreground: Color, background: Option<Color>, bold, dim, italic, underline: bool }` |

### Terminal
| Type | Description |
|------|-------------|
| `TerminalGrid` | `{ cols, rows: u16, cells: Vec<Vec<TerminalCell>> }` |
| `TerminalCell` | `{ character: char, style: TextStyle }` |
| `CursorState` | `{ row, col: u16, visible: bool, shape: CursorShape }` |
| `CursorShape` | `Block, Beam, Underline` |

### Layout
| Type | Description |
|------|-------------|
| `SplitDirection` | `Horizontal` (top/bottom) or `Vertical` (left/right) |
| `DropZone` | `Top, Bottom, Left, Right, Center` — where a dragged Pane lands |
| `DropTarget` | `Pane(PaneId, DropZone)` or `Root(DropZone)` |
| `PaneDecorations` | `{ gap, padding, tab_bar_height: f32 }` |

### File System
| Type | Description |
|------|-------------|
| `FileEntry` | `{ name: String, path: PathBuf, is_dir: bool }` |
| `TreeEntry` | `{ entry: FileEntry, depth: usize, is_expanded: bool, has_children: bool }` |
| `FileGitStatus` | `Modified, Added, Deleted, Untracked, Conflict` |

## Trait Contracts

These traits define the boundaries between Bounded Contexts.

### Renderer
```rust
trait Renderer {
    fn begin_frame(&mut self, size: Size);
    fn draw_rect(&mut self, rect: Rect, color: Color);
    fn draw_text(&mut self, text: &str, position: Vec2, style: TextStyle, clip: Rect);
    fn draw_cell(&mut self, character: char, row: usize, col: usize,
                 style: TextStyle, cell_size: Size, offset: Vec2);
    fn end_frame(&mut self);
    fn cell_size(&self) -> Size;
}
```

### LayoutEngine
```rust
trait LayoutEngine {
    fn compute(&self, window_size: Size, panes: &[PaneId],
               focused: Option<PaneId>) -> Vec<(PaneId, Rect)>;
    fn drag_border(&mut self, position: Vec2);
    fn split(&mut self, pane: PaneId, direction: SplitDirection) -> PaneId;
    fn remove(&mut self, pane: PaneId);
}
```

### TerminalBackend
```rust
trait TerminalBackend {
    fn write(&mut self, data: &[u8]);
    fn process(&mut self);
    fn grid(&self) -> &TerminalGrid;
    fn resize(&mut self, cols: u16, rows: u16);
    fn cwd(&self) -> Option<PathBuf>;
    fn cursor(&self) -> CursorState;
}
```

### FileTreeSource
```rust
trait FileTreeSource {
    fn set_root(&mut self, path: PathBuf);
    fn root(&self) -> &Path;
    fn visible_entries(&self) -> &[TreeEntry];
    fn toggle(&mut self, path: &Path);
    fn refresh(&mut self);
}
```

### InputRouter
```rust
trait InputRouter {
    fn route(&mut self, event: InputEvent, pane_rects: &[(PaneId, Rect)],
             focused: PaneId) -> Option<PaneId>;
}
```

**Note**: The `InputRouter` trait defines the low-level mouse routing contract. The `Router` struct in `tide-input` implements this trait and additionally provides `Router::process(event, pane_rects) -> Action` — a higher-level method that handles keyboard hotkey matching, mouse routing, and drag border detection. Flow documents refer to `Router.process()` which is the primary entry point used by `tide-app`.
