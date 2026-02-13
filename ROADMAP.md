# Tide — Roadmap

## Vision

A native, GPU-rendered app that replaces the terminal. Fast like Ghostty (GPU-rendered everything), interactive like Warp (clickable, block-based), but with file tree and file viewing built in.

Not an IDE. Not a raw terminal. A terminal app that can show you files.

## First Target (v0.1)

**GPU-rendered split terminals + file tree that follows the focused pane's cwd.** That's it. If this works and feels fast, everything else follows.

## Settled Decisions

- **Language:** Rust
- **GPU rendering:** wgpu (abstracts Metal/Vulkan/OpenGL/DX12 per platform)
- **Windowing:** winit (cross-platform window management, input events)
- **Terminal emulation:** alacritty_terminal (VT100/xterm compat, PTY management)
- **Syntax highlighting:** tree-sitter (used by Zed, Neovim, GitHub)
- **Text shaping/layout:** cosmic-text (GPU-compatible text layout and shaping)
- **Text buffer (editor):** ropey (rope data structure, used by Helix editor)
- **Cross-platform from day one**

## Architecture Principles

- **Everything is GPU-rendered.** No falling back to OS text rendering. All text, UI, terminal output goes through the wgpu pipeline.
- **The terminal is the app.** File tree, file viewer, etc. are capabilities of the terminal — not separate panel types. The app IS a terminal that can do more.
- **The terminal is interactive, not a character grid.** Output is block-based (command + result = one block). Blocks are clickable, selectable, scrollable independently. File paths, URLs, and errors in output are detected and interactive.
- **Hit-testing everywhere.** Every rendered element knows if the mouse is over it. Click and drag are first-class interactions, not afterthoughts.
- **Panes render into rectangles.** Each pane is an independent unit that receives a rect and renders into it. The layout engine is a separate system that assigns rects. This boundary must stay clean — it allows the layout algorithm to be changed later without touching rendering code.
- **Layout algorithm is deferred.** Start with simple equal splits. The "focused pane keeps space, unfocused compress" behavior is a future layout algorithm, not a structural requirement. The architecture supports it by keeping panes and layout decoupled.
- **Editor is Warp-level, not VS Code-level.** Syntax highlighting, basic editing, good enough for quick edits. Not an IDE editor.

## Development Strategy: Contracts First, Then Parallel

The codebase is split into independent crates connected by trait contracts. Development order:

1. **Step 1 — Scaffold + Contracts:** Set up workspace, define all traits/interfaces between modules. No implementation yet.
2. **Step 2 — Parallel implementation:** Each crate is implemented independently against the contracts. All streams can run simultaneously.
3. **Step 3 — Integration:** Wire the crates together into the running app.
4. **Repeat** for each milestone.

---

## Step 1: Scaffold and Contracts

Goal: Cargo workspace with all crates, shared types, and trait definitions. Everything compiles. Nothing works yet.

### 1.1 — Project scaffold
- Initialize cargo workspace
- Directory structure:
  ```
  tide/
    Cargo.toml              (workspace root)
    crates/
      tide-core/            (shared types: Rect, Color, Size, PaneId, InputEvent, etc.)
      tide-renderer/        (trait + wgpu implementation: GPU text/rect rendering)
      tide-terminal/        (trait + implementation: PTY, terminal state, cwd tracking)
      tide-layout/          (trait + implementation: rect assignment for panes)
      tide-tree/            (trait + implementation: file tree state and directory reading)
      tide-input/           (trait + implementation: input routing to panes)
      tide-app/             (binary: owns the window, render loop, wires everything together)
  ```
- Each crate depends only on `tide-core` for shared types. No crate depends on another crate's implementation — only on traits.

### 1.2 — Shared types (`tide-core`)
```rust
// Geometry
pub struct Rect { pub x: f32, pub y: f32, pub width: f32, pub height: f32 }
pub struct Size { pub width: f32, pub height: f32 }
pub struct Vec2 { pub x: f32, pub y: f32 }

// Identity
pub type PaneId = u64;

// Colors
pub struct Color { pub r: f32, pub g: f32, pub b: f32, pub a: f32 }

// Text styling
pub struct TextStyle {
    pub foreground: Color,
    pub background: Option<Color>,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
}

// Input events (abstracted from winit)
pub enum InputEvent {
    KeyPress { key: Key, modifiers: Modifiers },
    MouseClick { position: Vec2, button: MouseButton },
    MouseMove { position: Vec2 },
    MouseDrag { position: Vec2, button: MouseButton },
    MouseScroll { delta: f32, position: Vec2 },
    Resize { size: Size },
}

// File tree entries
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}
```

### 1.3 — Renderer contract (`tide-renderer`)
```rust
/// The renderer draws primitives to the GPU.
/// All visual output goes through this trait.
pub trait Renderer {
    /// Begin a new frame
    fn begin_frame(&mut self, size: Size);

    /// Draw a filled rectangle
    fn draw_rect(&mut self, rect: Rect, color: Color);

    /// Draw text at a position within a clipping rect
    fn draw_text(
        &mut self,
        text: &str,
        position: Vec2,
        style: TextStyle,
        clip: Rect,
    );

    /// Draw a single terminal cell (character + attributes)
    fn draw_cell(
        &mut self,
        character: char,
        row: usize,
        col: usize,
        style: TextStyle,
        cell_size: Size,
        offset: Vec2,
    );

    /// Submit the frame for presentation
    fn end_frame(&mut self);

    /// Returns the size of a single monospace cell
    fn cell_size(&self) -> Size;
}
```

### 1.4 — Pane contract (in `tide-core`)
```rust
/// A pane is anything that can render itself into a rectangle
/// and handle input. Terminal panes, file tree, file viewer
/// all implement this trait.
pub trait Pane {
    fn id(&self) -> PaneId;

    /// Render this pane into the given rect
    fn render(&self, rect: Rect, renderer: &mut dyn Renderer);

    /// Handle an input event. Returns true if consumed.
    fn handle_input(&mut self, event: InputEvent, rect: Rect) -> bool;

    /// Tick/update (called each frame for animations, PTY reads, etc.)
    fn update(&mut self);
}
```

### 1.5 — Layout contract (`tide-layout`)
```rust
/// The layout engine assigns rectangles to panes.
/// It doesn't know what panes contain — just their IDs and the window size.
pub trait LayoutEngine {
    /// Given the available space, pane IDs, and which is focused,
    /// return a rect for each pane.
    fn compute(
        &self,
        window_size: Size,
        panes: &[PaneId],
        focused: Option<PaneId>,
    ) -> Vec<(PaneId, Rect)>;

    /// Notify the layout that a pane border is being dragged
    fn drag_border(&mut self, position: Vec2);

    /// Add a split (horizontal or vertical) relative to a pane
    fn split(&mut self, pane: PaneId, direction: SplitDirection) -> PaneId;

    /// Remove a pane from the layout
    fn remove(&mut self, pane: PaneId);
}

pub enum SplitDirection { Horizontal, Vertical }
```

### 1.6 — Terminal contract (`tide-terminal`)
```rust
/// Terminal backend: manages PTY, shell state, and terminal emulation.
/// Separate from rendering — this is pure terminal logic.
pub trait TerminalBackend {
    /// Write user input to the PTY
    fn write(&mut self, data: &[u8]);

    /// Process any pending PTY output, update internal state
    fn process(&mut self);

    /// Get the terminal grid (rows x cols of cells)
    fn grid(&self) -> &TerminalGrid;

    /// Resize the terminal
    fn resize(&mut self, cols: u16, rows: u16);

    /// Get the current working directory (if detectable)
    fn cwd(&self) -> Option<PathBuf>;

    /// Get cursor position and style
    fn cursor(&self) -> CursorState;
}

pub struct TerminalGrid {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Vec<TerminalCell>>,
}

pub struct TerminalCell {
    pub character: char,
    pub style: TextStyle,
}

pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
    pub shape: CursorShape,
}

pub enum CursorShape { Block, Beam, Underline }
```

### 1.7 — File tree contract (`tide-tree`)
```rust
/// File tree: reads a directory and provides tree state.
/// Rendering is done by the pane that wraps this.
pub trait FileTree {
    /// Set the root directory
    fn set_root(&mut self, path: PathBuf);

    /// Get the current root
    fn root(&self) -> &Path;

    /// Get visible entries (respecting expand/collapse state)
    fn visible_entries(&self) -> &[TreeEntry];

    /// Toggle expand/collapse of a directory
    fn toggle(&mut self, path: &Path);

    /// Refresh the tree (re-read from filesystem)
    fn refresh(&mut self);
}

pub struct TreeEntry {
    pub entry: FileEntry,
    pub depth: usize,
    pub is_expanded: bool,
    pub has_children: bool,
}
```

### 1.8 — Input router contract (`tide-input`)
```rust
/// Routes raw winit events to the correct pane based on
/// mouse position and keyboard focus.
pub trait InputRouter {
    /// Route an input event. Returns which pane consumed it (if any).
    fn route(
        &mut self,
        event: InputEvent,
        pane_rects: &[(PaneId, Rect)],
        focused: PaneId,
    ) -> Option<PaneId>;
}
```

**After Step 1: `cargo build` compiles. All traits defined. All crates exist. Zero functionality. Total parallelism unlocked.**

---

## Step 2: Parallel Implementation Streams

After contracts are defined, these 5 streams can run **simultaneously**. Each stream works within its own crate, depending only on `tide-core` traits.

### Stream A: GPU Renderer (`tide-renderer`)

Implement the `Renderer` trait using wgpu + cosmic-text.

**A.1 — wgpu initialization**
- Create wgpu instance, surface, device, queue
- Configure swap chain
- Render loop: clear screen with background color
- Handle resize

**A.2 — Glyph atlas**
- Load a monospace font with cosmic-text
- Rasterize glyphs to a texture atlas
- Cache glyphs (HashMap<GlyphKey, AtlasRegion>)
- Handle atlas overflow (grow or LRU evict)

**A.3 — Text rendering pipeline**
- Vertex shader + fragment shader for textured quads
- `draw_text()`: shape text with cosmic-text, look up glyphs in atlas, emit quads
- `draw_cell()`: fast path for single-character terminal cells
- Clipping support (scissor rects per pane)

**A.4 — Rect rendering pipeline**
- Simple colored quad shader
- `draw_rect()`: used for backgrounds, borders, selection highlights, cursor
- Batch quads for efficiency

**A.5 — Frame management**
- `begin_frame()` / `end_frame()` batching
- Sort draw calls (rects first, then text on top)
- Measure and log: frame time, draw call count, glyph cache hit rate
- HiDPI / Retina: handle scale factor from winit

**Test without integration:** Standalone binary that opens a window, creates the wgpu renderer, and draws hardcoded colored text. Visual verification.

---

### Stream B: Terminal Backend (`tide-terminal`)

Implement the `TerminalBackend` trait using alacritty_terminal.

**B.1 — PTY spawning**
- Spawn a shell process ($SHELL or fallback)
- Create PTY pair (master/slave)
- Use alacritty_terminal's PTY abstraction
- Handle shell exit

**B.2 — Terminal emulation**
- Feed PTY output to alacritty_terminal's parser
- Read terminal grid state (cells, attributes, colors)
- Map alacritty_terminal's cell format to our `TerminalCell`
- Support: ANSI 256 colors, truecolor, bold, italic, underline, inverse

**B.3 — Input handling**
- Convert our `InputEvent::KeyPress` to bytes for the PTY
- Handle special keys (arrow keys, function keys, etc.)
- Handle modifier keys (Ctrl+C, Ctrl+D, etc.)

**B.4 — Terminal state**
- Scrollback buffer (configurable size)
- Alternate screen buffer (vim, less, htop)
- Terminal resize → update PTY dimensions + reflow
- Cursor state tracking

**B.5 — CWD detection**
- Parse OSC 7 sequences (shell reports cwd)
- Fallback: read /proc/{pid}/cwd on Linux, lsof on macOS
- Expose via `cwd() -> Option<PathBuf>`

**Test without integration:** Standalone binary that creates a `TerminalBackend`, spawns a shell, pipes stdin/stdout to it in raw text mode (no GPU). Verify shell works, commands execute, cwd updates.

---

### Stream C: Layout Engine (`tide-layout`)

Implement the `LayoutEngine` trait.

**C.1 — Layout tree data structure**
- Binary tree: internal nodes are splits (H/V), leaves are panes
- Each node stores split ratio (default 0.5)
- Tree operations: insert split, remove pane, find pane

**C.2 — Rect computation**
- Traverse tree, divide available space at each split
- Respect split ratio and direction
- Output: Vec<(PaneId, Rect)>

**C.3 — Split and remove**
- `split()`: replace a leaf with a split node containing the original pane and a new pane
- `remove()`: collapse a split node when one child is removed
- Generate new PaneIds

**C.4 — Border dragging**
- Hit-test borders (thin region between adjacent panes)
- `drag_border()`: update split ratio based on mouse position
- Clamp ratio to prevent panes from becoming too small

**Test without integration:** Unit tests. Given N panes and a window size, verify computed rects tile the window correctly. No gaps, no overlaps. Test split, remove, drag.

---

### Stream D: File Tree (`tide-tree`)

Implement the `FileTree` trait.

**D.1 — Directory reading**
- Read directory contents (std::fs::read_dir)
- Sort: directories first, then files, alphabetical
- Create `FileEntry` structs
- Handle permission errors, symlinks

**D.2 — Tree state**
- Track which directories are expanded (HashSet<PathBuf>)
- `visible_entries()`: depth-first traversal, skipping collapsed directories
- Lazy loading: only read a directory when first expanded

**D.3 — Expand/collapse**
- `toggle()`: flip expanded state, load children if needed
- `set_root()`: reset tree state, load new root directory

**D.4 — File system watching**
- Watch root directory for changes (notify crate — widely used, cross-platform)
- Auto-refresh when files are added/removed/renamed
- Debounce rapid changes

**Test without integration:** Unit tests + standalone CLI that prints the tree for a given directory. Expand/collapse via stdin commands.

---

### Stream E: Input Router (`tide-input`)

Implement the `InputRouter` trait.

**E.1 — Hit testing**
- Given mouse position and pane rects, determine which pane the mouse is over
- Point-in-rect test for clicks
- Track which pane the mouse is currently in (for hover state)

**E.2 — Keyboard routing**
- Keyboard events go to the focused pane
- Global hotkeys (split pane, close pane, toggle file tree) are intercepted before routing
- Define a hotkey table: Vec<(KeyCombo, Action)>

**E.3 — Focus management**
- Click a pane → that pane becomes focused
- Keyboard shortcut to move focus (Cmd/Ctrl+Arrow or similar)
- Track focused PaneId, notify layout engine on change

**E.4 — Drag routing**
- Mouse drag on a pane border → route to layout engine (border resize)
- Mouse drag inside a pane → route to the pane (text selection)
- Distinguish by hit-test: border region vs. pane interior

**Test without integration:** Unit tests. Given mock pane rects and input events, verify correct routing.

---

## Step 3: Integration (v0.1)

Goal: Wire all streams together into a running app. All crates are implemented and tested independently — now connect them.

### 3.1 — App shell (`tide-app`)
- Create winit window + event loop
- Initialize wgpu renderer (Stream A)
- Create one terminal pane (Stream B)
- Create layout engine (Stream C)
- Create input router (Stream E)
- Render loop:
  1. Poll terminal for PTY output → `terminal.process()`
  2. Layout engine computes rects
  3. For each pane: `pane.render(rect, renderer)`
  4. Submit frame

### 3.2 — TerminalPane (wraps TerminalBackend + Renderer)
- Implements `Pane` trait
- `render()`: reads terminal grid, calls `renderer.draw_cell()` for each cell
- `handle_input()`: forwards key events to terminal backend
- `update()`: calls `terminal.process()` to consume PTY output

### 3.3 — Multi-pane
- Wire split/close hotkeys to layout engine
- Each split creates a new TerminalPane with its own shell
- Focus tracking: click or keyboard shortcut changes focused pane
- Visual focus indicator: highlighted border on focused pane

### 3.4 — File tree pane
- Create a `FileTreePane` that implements `Pane`
- Wraps the `FileTree` (Stream D)
- `render()`: draws tree entries as text rows with indentation
- `handle_input()`: click to expand/collapse, click file to (future: open viewer)
- Toggle with Cmd/Ctrl+B

### 3.5 — CWD following
- On focus change: read `terminal.cwd()` from the newly focused terminal
- Call `file_tree.set_root(cwd)` to update the tree
- Debounce: don't thrash the tree if focus changes rapidly

### 3.6 — Polish for v0.1
- Pane borders rendered cleanly
- Focused pane has distinct border color
- File tree scroll works
- Terminal scroll works (scrollback)
- Clipboard: copy/paste in terminal
- Graceful handling of shell exit (pane shows "exited" or closes)
- Window title shows focused pane's cwd

**Milestone: v0.1 — Multiple GPU-rendered terminals with a file tree that follows focus. Usable daily.**

---

## Post v0.1 Phases (sequential, build on v0.1)

### Phase 4: Interactive Terminal (block-based output)
- Shell integration scripts (OSC 133 for command boundaries)
- Block-based output rendering (command + output as a selectable unit)
- Clickable file paths, URLs, error locations in terminal output
- Hit-test infrastructure for interactive elements

### Phase 5: File Viewer
- Open files from file tree or terminal output clicks
- Syntax highlighting with tree-sitter
- Line numbers, smooth scrolling, find in file
- Viewer opens within the focused terminal's context

### Phase 6: Text Editing (Warp-level)
- Text buffer with ropey
- Basic cursor movement, insert, delete, selection
- Undo/redo, save
- Good enough for quick edits, not an IDE

### Phase 7: Markdown Rendering
- pulldown-cmark parser
- Rendered view: headings, lists, code blocks, links
- Source/rendered toggle

### Phase 8: Configuration and Theming
- TOML config file, hot reload
- Color themes (dark/light), user-creatable
- Keybinding customization
- Session persistence (restore panes on restart)

### Phase 9: Polish and Ship
- Performance profiling (60fps target, <5ms input latency)
- Platform packaging (DMG, AppImage, .deb)
- Shell integration installer
- Documentation

---

## Parallel Execution Map

```
Step 1: Scaffold + Contracts          (sequential, one session)
         │
         ├── Stream A: GPU Renderer    ─── can start immediately
         ├── Stream B: Terminal Backend ─── can start immediately
         ├── Stream C: Layout Engine    ─── can start immediately
         ├── Stream D: File Tree        ─── can start immediately
         └── Stream E: Input Router     ─── can start immediately
                    │
                    ▼
Step 3: Integration                    (after all streams done)
         │
         ▼
    v0.1 shipped
         │
         ├── Phase 4: Block-based terminal
         ├── Phase 5: File viewer
         ├── Phase 6: Editing
         ├── Phase 7: Markdown
         ├── Phase 8: Config/themes
         └── Phase 9: Ship
```

**Maximum parallelism: 5 streams after Step 1.**

---

## How to Use This Roadmap

### Step 1 (contracts):
> "We're building Tide. Do Step 1 from ROADMAP.md — set up the cargo workspace and define all trait contracts. No implementations yet."

### Step 2 (parallel streams):
Run 5 Claude Code sessions simultaneously, one per stream:
> "We're building Tide. Implement Stream A (GPU Renderer) from ROADMAP.md. The contracts are already defined in the crates. Implement the Renderer trait in tide-renderer using wgpu + cosmic-text."

> "We're building Tide. Implement Stream B (Terminal Backend) from ROADMAP.md. Implement the TerminalBackend trait in tide-terminal using alacritty_terminal."

> "We're building Tide. Implement Stream C (Layout Engine) from ROADMAP.md. Implement the LayoutEngine trait in tide-layout."

> "We're building Tide. Implement Stream D (File Tree) from ROADMAP.md. Implement the FileTree trait in tide-tree using the notify crate for fs watching."

> "We're building Tide. Implement Stream E (Input Router) from ROADMAP.md. Implement the InputRouter trait in tide-input."

### Step 3 (integration):
> "We're building Tide. All stream implementations are done. Do Step 3 from ROADMAP.md — wire everything together in tide-app."
