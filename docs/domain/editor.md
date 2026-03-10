# Editor — tide-editor

**Role**: Text buffer management, cursor navigation, undo/redo, syntax highlighting.
Pure state — no GPU, no PTY, no IO except file save/load.

`crates/tide-editor/src/`

## Aggregate: EditorState

```rust
EditorState {
    buffer: Buffer,               // Text content storage
    cursor: EditorCursor,         // Position + desired column
    highlighter: Highlighter,     // Syntax highlighting (syntect)
    syntax: Option<String>,       // Current syntax name (e.g., "Rust")
    scroll_offset: usize,         // Vertical scroll (in lines)
    h_scroll_offset: usize,       // Horizontal scroll (in chars)
    generation: u64,              // Monotonic counter for cache invalidation
}
```

## Entity: Buffer

```rust
Buffer {
    lines: Vec<String>,           // Text stored as vector of lines
    file_path: Option<PathBuf>,   // None for unsaved buffers
    generation: u64,              // Incremented on every edit
    saved_content: Vec<String>,   // Snapshot at last save point
    undo_stack: Vec<(EditOp, Position)>,  // Undo history + cursor-before
    redo_stack: Vec<(EditOp, Position)>,  // Cleared on every new edit
    trailing_newline: bool,       // Preserve trailing \n
}
```

**Text storage**: Simple `Vec<String>` — not a rope. Cache-friendly for typical file sizes.

**Column indexing**: Byte offset (not char index). `floor_char_boundary()` prevents splitting multi-byte characters.

**Dirty detection**: Content-based comparison (`self.lines != self.saved_content`), not flag-based.

## Value Object: EditorCursor

```rust
EditorCursor {
    position: Position,          // { line: usize, col: usize } — col is byte offset
    desired_col: usize,          // Preserved across up/down through short lines
}
```

**Desired column pattern**: Moving up from a long line to a short line clamps col to line length, but `desired_col` remembers the original column. Moving down to a long line restores it.

## Undo/Redo

8 reversible operations:
```
InsertChar, DeleteChar, Backspace, InsertNewline,
DeleteRange, InsertText, DeleteLine, SwapLines
```

Each entry stores `(EditOp, cursor_position_before)`. Undo restores cursor.
Any new edit clears the redo stack immediately.

## Syntax Highlighting

**Engine**: syntect (Sublime Text grammars), not tree-sitter.

**Incremental strategy**: Checkpoint `(ParseState, HighlightState)` every 256 lines.
On scroll, resume from nearest checkpoint — O(256 + visible_rows) work instead of O(file_length).

## Command: EditorAction (35 variants)

### Text Editing
| Action | Binding | Description |
|--------|---------|-------------|
| `InsertChar(char)` | plain keys | Type a character |
| `Backspace` | Backspace | Delete before cursor |
| `Delete` | Delete | Delete at cursor |
| `Enter` | Enter | New line with auto-indent |
| `Undo` / `Redo` | Cmd+Z / Cmd+Shift+Z | Undo/redo |
| `Save` | Cmd+S | Write to disk |
| `SelectAll` | Cmd+A | Select all text |

### Navigation
| Action | Binding | Description |
|--------|---------|-------------|
| `MoveUp/Down/Left/Right` | Arrow keys | Basic movement |
| `MoveWordLeft/Right` | Alt+Arrow | Word-boundary jump |
| `Home` / `End` | Cmd+Left / Cmd+Right | Line start/end |
| `MoveDocStart/End` | Cmd+Up / Cmd+Down | Document start/end |
| `PageUp` / `PageDown` | PageUp / PageDown | Page scroll |

### Deletion
| Action | Binding | Description |
|--------|---------|-------------|
| `DeleteWordLeft/Right` | Alt+Backspace / Alt+Delete | Delete word |
| `DeleteToLineStart/End` | Cmd+Backspace / Cmd+Delete | Delete to boundary |
| `DeleteLine` | Cmd+Shift+K | Delete entire line |

### Line Manipulation
| Action | Binding | Description |
|--------|---------|-------------|
| `MoveLineUp/Down` | Alt+Up / Alt+Down | Swap lines |
| `Unindent` | Shift+Tab | Remove indentation |

## Key Methods

| Method | Purpose |
|--------|---------|
| `open(path)` | Load file from disk, detect syntax |
| `reload()` | Reload from disk, clamp cursor |
| `handle_action(action)` | Apply EditorAction |
| `insert_text(text)` | Paste block (single undo entry) |
| `visible_highlighted_lines(rows)` | Get syntax-highlighted spans for viewport |
| `ensure_cursor_visible(rows)` | Auto-scroll to keep cursor on screen |
| `matching_bracket()` | Find matching `()[]{}` pair |
| `is_modified()` | `lines != saved_content` |
