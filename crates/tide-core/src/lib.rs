use std::path::{Path, PathBuf};

// ──────────────────────────────────────────────
// Geometry
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn new(x: f32, y: f32, width: f32, height: f32) -> Self {
        Self { x, y, width, height }
    }

    pub fn contains(&self, point: Vec2) -> bool {
        point.x >= self.x
            && point.x <= self.x + self.width
            && point.y >= self.y
            && point.y <= self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Size {
    pub width: f32,
    pub height: f32,
}

impl Size {
    pub fn new(width: f32, height: f32) -> Self {
        Self { width, height }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

// ──────────────────────────────────────────────
// Identity
// ──────────────────────────────────────────────

pub type PaneId = u64;

// ──────────────────────────────────────────────
// Colors
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub const fn rgb(r: f32, g: f32, b: f32) -> Self {
        Self { r, g, b, a: 1.0 }
    }

    pub const BLACK: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    pub const WHITE: Self = Self::new(1.0, 1.0, 1.0, 1.0);
}

// ──────────────────────────────────────────────
// Text Styling
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextStyle {
    pub foreground: Color,
    pub background: Option<Color>,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
}

impl Default for TextStyle {
    fn default() -> Self {
        Self {
            foreground: Color::WHITE,
            background: None,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
        }
    }
}

// ──────────────────────────────────────────────
// Input
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Key {
    Char(char),
    Enter,
    Backspace,
    Tab,
    Escape,
    Delete,
    Up,
    Down,
    Left,
    Right,
    Home,
    End,
    PageUp,
    PageDown,
    F(u8),
    Insert,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Modifiers {
    pub shift: bool,
    pub ctrl: bool,
    pub alt: bool,
    pub meta: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum InputEvent {
    KeyPress { key: Key, modifiers: Modifiers },
    MouseClick { position: Vec2, button: MouseButton },
    MouseMove { position: Vec2 },
    MouseDrag { position: Vec2, button: MouseButton },
    MouseScroll { delta: f32, position: Vec2 },
    Resize { size: Size },
}

// ──────────────────────────────────────────────
// File types
// ──────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
}

#[derive(Debug, Clone)]
pub struct TreeEntry {
    pub entry: FileEntry,
    pub depth: usize,
    pub is_expanded: bool,
    pub has_children: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FileGitStatus {
    Modified,
    Added,
    Deleted,
    Untracked,
    Conflict,
}

// ──────────────────────────────────────────────
// Terminal types
// ──────────────────────────────────────────────

pub struct TerminalGrid {
    pub cols: u16,
    pub rows: u16,
    pub cells: Vec<Vec<TerminalCell>>,
}

#[derive(Debug, Clone)]
pub struct TerminalCell {
    pub character: char,
    pub style: TextStyle,
}

impl Default for TerminalCell {
    fn default() -> Self {
        Self {
            character: ' ',
            style: TextStyle::default(),
        }
    }
}

pub struct CursorState {
    pub row: u16,
    pub col: u16,
    pub visible: bool,
    pub shape: CursorShape,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorShape {
    Block,
    Beam,
    Underline,
}

// ──────────────────────────────────────────────
// Layout types
// ──────────────────────────────────────────────

/// Decoration sizes needed by the layout engine to snap ratios to cell boundaries.
#[derive(Debug, Clone, Copy)]
pub struct PaneDecorations {
    pub gap: f32,
    pub padding: f32,
    pub tab_bar_height: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SplitDirection {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropZone {
    Top,
    Bottom,
    Left,
    Right,
    Center,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropTarget {
    Pane(PaneId, DropZone),
    Root(DropZone),
}

// ──────────────────────────────────────────────
// Trait: Renderer
// ──────────────────────────────────────────────

/// The renderer draws primitives to the GPU.
/// All visual output goes through this trait.
pub trait Renderer {
    fn begin_frame(&mut self, size: Size);
    fn draw_rect(&mut self, rect: Rect, color: Color);
    fn draw_text(&mut self, text: &str, position: Vec2, style: TextStyle, clip: Rect);
    fn draw_cell(
        &mut self,
        character: char,
        row: usize,
        col: usize,
        style: TextStyle,
        cell_size: Size,
        offset: Vec2,
    );
    fn end_frame(&mut self);
    fn cell_size(&self) -> Size;
}

// ──────────────────────────────────────────────
// Trait: Pane
// ──────────────────────────────────────────────

/// A pane is anything that can render itself into a rectangle
/// and handle input. Terminal panes, file tree, file viewer
/// all implement this trait.
pub trait Pane {
    fn id(&self) -> PaneId;
    fn render(&self, rect: Rect, renderer: &mut dyn Renderer);
    fn handle_input(&mut self, event: InputEvent, rect: Rect) -> bool;
    fn update(&mut self);
}

// ──────────────────────────────────────────────
// Trait: LayoutEngine
// ──────────────────────────────────────────────

/// The layout engine assigns rectangles to panes.
/// It doesn't know what panes contain — just their IDs and the window size.
pub trait LayoutEngine {
    fn compute(
        &self,
        window_size: Size,
        panes: &[PaneId],
        focused: Option<PaneId>,
    ) -> Vec<(PaneId, Rect)>;
    fn drag_border(&mut self, position: Vec2);
    fn split(&mut self, pane: PaneId, direction: SplitDirection) -> PaneId;
    fn remove(&mut self, pane: PaneId);
}

// ──────────────────────────────────────────────
// Trait: TerminalBackend
// ──────────────────────────────────────────────

/// Terminal backend: manages PTY, shell state, and terminal emulation.
pub trait TerminalBackend {
    fn write(&mut self, data: &[u8]);
    fn process(&mut self);
    fn grid(&self) -> &TerminalGrid;
    fn resize(&mut self, cols: u16, rows: u16);
    fn cwd(&self) -> Option<PathBuf>;
    fn cursor(&self) -> CursorState;
}

// ──────────────────────────────────────────────
// Trait: FileTree
// ──────────────────────────────────────────────

/// File tree: reads a directory and provides tree state.
pub trait FileTreeSource {
    fn set_root(&mut self, path: PathBuf);
    fn root(&self) -> &Path;
    fn visible_entries(&self) -> &[TreeEntry];
    fn toggle(&mut self, path: &Path);
    fn refresh(&mut self);
}

// ──────────────────────────────────────────────
// Trait: InputRouter
// ──────────────────────────────────────────────

/// Routes input events to the correct pane based on
/// mouse position and keyboard focus.
pub trait InputRouter {
    fn route(
        &mut self,
        event: InputEvent,
        pane_rects: &[(PaneId, Rect)],
        focused: PaneId,
    ) -> Option<PaneId>;
}
