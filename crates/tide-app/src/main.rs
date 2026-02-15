// Tide v0.1 — Integration (Step 3)
// Wires all crates together: winit window, wgpu surface, renderer, terminal panes,
// layout engine, input router, file tree, and CWD following.

mod action;
mod diff;
mod diff_pane;
mod drag_drop;
mod editor_pane;
mod event_handler;
mod file_tree;
mod gpu;
mod header;
mod input;
mod pane;
mod rendering;
mod search;
mod session;
mod theme;
mod ui;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use notify::{self, Watcher};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton as WinitMouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId};

use tide_core::{LayoutEngine, PaneDecorations, PaneId, Rect, Renderer, Size, SplitDirection, TerminalBackend};
use tide_input::Router;
use tide_layout::SplitLayout;
use tide_renderer::WgpuRenderer;
use tide_tree::FsTree;

use drag_drop::{HoverTarget, PaneDragState};
use pane::{PaneKind, TerminalPane};
use theme::*;
use theme::{ThemePalette, DARK, LIGHT};

// ──────────────────────────────────────────────
// Layout side: which edge a sidebar/dock component is on
// ──────────────────────────────────────────────

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum LayoutSide {
    Left,
    Right,
}

// ──────────────────────────────────────────────
// Save-as input state (inline filename entry for untitled files)
// ──────────────────────────────────────────────

pub(crate) struct SaveAsInput {
    pub pane_id: PaneId,
    pub query: String,
    pub cursor: usize,
}

impl SaveAsInput {
    pub fn new(pane_id: PaneId) -> Self {
        Self {
            pane_id,
            query: String::new(),
            cursor: 0,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.query.len() {
            let next = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
            self.query.drain(self.cursor..next);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            self.cursor = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
        }
    }
}

// ──────────────────────────────────────────────
// Save confirm state (inline bar when closing dirty editors)
// ──────────────────────────────────────────────

pub(crate) struct SaveConfirmState {
    pub pane_id: PaneId,
}

// ──────────────────────────────────────────────
// File finder state (in-panel file search/open UI)
// ──────────────────────────────────────────────

pub(crate) struct FileFinderState {
    pub query: String,
    pub cursor: usize,
    pub base_dir: PathBuf,
    pub entries: Vec<PathBuf>,          // all files (relative to base_dir)
    pub filtered: Vec<usize>,           // indices into entries
    pub selected: usize,                // index into filtered
    pub scroll_offset: usize,           // scroll offset in filtered list
}

impl FileFinderState {
    pub fn new(base_dir: PathBuf, entries: Vec<PathBuf>) -> Self {
        let filtered: Vec<usize> = (0..entries.len()).collect();
        Self {
            query: String::new(),
            cursor: 0,
            base_dir,
            entries,
            filtered,
            selected: 0,
            scroll_offset: 0,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.filter();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
            self.filter();
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.query.len() {
            let next = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
            self.query.drain(self.cursor..next);
            self.filter();
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            self.cursor = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn select_down(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn selected_path(&self) -> Option<PathBuf> {
        let idx = *self.filtered.get(self.selected)?;
        let rel = self.entries.get(idx)?;
        Some(self.base_dir.join(rel))
    }

    fn filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let query_lower = self.query.to_lowercase();
            self.filtered = self.entries.iter().enumerate()
                .filter(|(_, path)| {
                    let name = path.to_string_lossy().to_lowercase();
                    name.contains(&query_lower)
                })
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

// ──────────────────────────────────────────────
// Branch switcher popup state
// ──────────────────────────────────────────────

pub(crate) struct BranchSwitcherState {
    pub pane_id: PaneId,
    pub query: String,
    pub cursor: usize,
    pub branches: Vec<tide_terminal::git::BranchInfo>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub anchor_rect: Rect,
}

impl BranchSwitcherState {
    pub fn new(pane_id: PaneId, branches: Vec<tide_terminal::git::BranchInfo>, anchor_rect: Rect) -> Self {
        let filtered: Vec<usize> = (0..branches.len()).collect();
        Self {
            pane_id,
            query: String::new(),
            cursor: 0,
            branches,
            filtered,
            selected: 0,
            scroll_offset: 0,
            anchor_rect,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.filter();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
            self.filter();
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn select_down(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn selected_branch(&self) -> Option<&tide_terminal::git::BranchInfo> {
        let idx = *self.filtered.get(self.selected)?;
        self.branches.get(idx)
    }

    fn filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.branches.len()).collect();
        } else {
            let query_lower = self.query.to_lowercase();
            self.filtered = self.branches.iter().enumerate()
                .filter(|(_, b)| b.name.to_lowercase().contains(&query_lower))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

// ──────────────────────────────────────────────
// File switcher popup (open files list for editor panel)
// ──────────────────────────────────────────────

pub(crate) struct FileSwitcherEntry {
    pub pane_id: PaneId,
    pub name: String,
    pub is_active: bool,
}

pub(crate) struct FileSwitcherState {
    pub query: String,
    pub cursor: usize,
    pub entries: Vec<FileSwitcherEntry>,
    pub filtered: Vec<usize>,
    pub selected: usize,
    pub scroll_offset: usize,
    pub anchor_rect: tide_core::Rect,
}

impl FileSwitcherState {
    pub fn new(entries: Vec<FileSwitcherEntry>, anchor_rect: tide_core::Rect) -> Self {
        let filtered: Vec<usize> = (0..entries.len()).collect();
        // Pre-select the active entry
        let selected = entries.iter().position(|e| e.is_active).unwrap_or(0);
        Self {
            query: String::new(),
            cursor: 0,
            entries,
            filtered,
            selected,
            scroll_offset: 0,
            anchor_rect,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
        self.filter();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
            self.filter();
        }
    }

    pub fn select_up(&mut self) {
        if self.selected > 0 {
            self.selected -= 1;
            if self.selected < self.scroll_offset {
                self.scroll_offset = self.selected;
            }
        }
    }

    pub fn select_down(&mut self) {
        if !self.filtered.is_empty() && self.selected + 1 < self.filtered.len() {
            self.selected += 1;
        }
    }

    pub fn selected_entry(&self) -> Option<&FileSwitcherEntry> {
        let idx = *self.filtered.get(self.selected)?;
        self.entries.get(idx)
    }

    fn filter(&mut self) {
        if self.query.is_empty() {
            self.filtered = (0..self.entries.len()).collect();
        } else {
            let query_lower = self.query.to_lowercase();
            self.filtered = self.entries.iter().enumerate()
                .filter(|(_, e)| e.name.to_lowercase().contains(&query_lower))
                .map(|(i, _)| i)
                .collect();
        }
        self.selected = 0;
        self.scroll_offset = 0;
    }
}

// ──────────────────────────────────────────────
// App state
// ──────────────────────────────────────────────

struct App {
    pub(crate) window: Option<Arc<Window>>,
    pub(crate) surface: Option<wgpu::Surface<'static>>,
    pub(crate) device: Option<Arc<wgpu::Device>>,
    pub(crate) queue: Option<Arc<wgpu::Queue>>,
    pub(crate) surface_config: Option<wgpu::SurfaceConfiguration>,
    pub(crate) renderer: Option<WgpuRenderer>,

    // Panes
    pub(crate) panes: HashMap<PaneId, PaneKind>,
    pub(crate) layout: SplitLayout,
    pub(crate) router: Router,
    pub(crate) focused: Option<PaneId>,

    // File tree
    pub(crate) file_tree: Option<FsTree>,
    pub(crate) show_file_tree: bool,
    pub(crate) file_tree_scroll: f32,
    pub(crate) file_tree_scroll_target: f32,
    pub(crate) file_tree_width: f32,
    pub(crate) file_tree_border_dragging: bool,
    pub(crate) file_tree_rect: Option<Rect>,

    // Sidebar/dock layout sides
    pub(crate) sidebar_side: LayoutSide,
    pub(crate) dock_side: LayoutSide,
    pub(crate) sidebar_handle_dragging: bool,
    pub(crate) dock_handle_dragging: bool,
    /// Preview state during handle drag: target side for the dragged component
    pub(crate) handle_drag_preview: Option<LayoutSide>,

    // Window state
    pub(crate) scale_factor: f32,
    pub(crate) window_size: PhysicalSize<u32>,
    pub(crate) modifiers: ModifiersState,
    pub(crate) last_cursor_pos: tide_core::Vec2,

    // CWD tracking
    pub(crate) last_cwd: Option<PathBuf>,
    /// Deferred badge check scheduled after PTY output settles.
    /// Event-driven: set ~150ms after last PTY burst so CWD/idle badges
    /// update promptly regardless of whether user or AI agent changed dirs.
    badge_check_at: Option<Instant>,

    // Frame pacing
    pub(crate) needs_redraw: bool,
    pub(crate) last_frame: Instant,

    // IME composition state
    pub(crate) ime_active: bool,
    pub(crate) ime_composing: bool,
    pub(crate) ime_preedit: String,
    /// First hangul character typed before IME was active (macOS sends
    /// KeyboardInput before Ime::Enabled on language switch).  Stored here
    /// so we can combine it with the first Preedit/Commit the IME produces.
    pub(crate) pending_hangul_initial: Option<char>,
    /// Preedit text saved when composition is cleared by Preedit("").
    /// If the next Ime::Commit doesn't contain this text, it was dropped
    /// by the IME (e.g. pressing ? during Korean composition) and must be
    /// prepended to the committed output.
    pub(crate) ime_dropped_preedit: Option<String>,
    /// Physical key of the last Pressed event that had text (event.text.is_some()).
    /// Used to prevent the Released event handler from duplicating characters
    /// that were already processed by the Pressed handler.
    pub(crate) last_pressed_with_text: Option<winit::keyboard::PhysicalKey>,

    // Computed pane rects: tiling rects (hit-testing/drag) and visual rects (gap-inset, rendering)
    pub(crate) pane_rects: Vec<(PaneId, Rect)>,
    pub(crate) visual_pane_rects: Vec<(PaneId, Rect)>,
    pub(crate) prev_visual_pane_rects: Vec<(PaneId, Rect)>,

    // The overall rect available for pane tiling (excluding file tree and editor panel)
    pub(crate) pane_area_rect: Option<Rect>,

    // Grid generation tracking for vertex caching
    pub(crate) pane_generations: HashMap<PaneId, u64>,
    pub(crate) layout_generation: u64,

    // Chrome generation tracking (borders + file tree)
    pub(crate) chrome_generation: u64,
    pub(crate) last_chrome_generation: u64,

    // Input latency: skip 8ms sleep after keypress while awaiting PTY response
    pub(crate) input_just_sent: bool,
    pub(crate) input_sent_at: Option<Instant>,

    // Adaptive frame pacing: throttle to ~60fps during high throughput
    pub(crate) consecutive_dirty_frames: u32,

    // Pane drag & drop
    pub(crate) pane_drag: PaneDragState,

    // Scroll accumulator for sub-pixel precision (prevents jitter from PixelDelta)
    pub(crate) scroll_accumulator: HashMap<PaneId, f32>,

    // Mouse state for text selection
    pub(crate) mouse_left_pressed: bool,

    // Search focus: which pane's search bar has keyboard focus
    pub(crate) search_focus: Option<PaneId>,

    // Pane maximize (temporary full-area display of a single pane)
    pub(crate) maximized_pane: Option<PaneId>,

    // Editor panel visibility toggle
    pub(crate) show_editor_panel: bool,

    // Editor panel maximize (temporary full-area display of entire editor panel)
    pub(crate) editor_panel_maximized: bool,

    // Editor panel (right-side tab panel)
    pub(crate) editor_panel_tabs: Vec<tide_core::PaneId>,
    pub(crate) editor_panel_active: Option<tide_core::PaneId>,
    pub(crate) editor_panel_rect: Option<Rect>,
    pub(crate) editor_panel_width: f32,
    pub(crate) panel_border_dragging: bool,
    pub(crate) editor_panel_width_manual: bool,
    pub(crate) panel_tab_scroll: f32,
    pub(crate) panel_tab_scroll_target: f32,

    // Save-as input (inline filename entry for untitled files)
    pub(crate) save_as_input: Option<SaveAsInput>,

    // Save confirm state (inline bar when closing dirty editors)
    pub(crate) save_confirm: Option<SaveConfirmState>,

    // File finder state (in-panel file search/open UI)
    pub(crate) file_finder: Option<FileFinderState>,

    // Placeholder PaneId for empty editor panel focus (not in panes or tabs)
    pub(crate) editor_panel_placeholder: Option<tide_core::PaneId>,

    // Theme mode
    pub(crate) dark_mode: bool,

    // Header hit zones (for badge click handling)
    pub(crate) header_hit_zones: Vec<header::HeaderHitZone>,

    // Branch switcher popup
    pub(crate) branch_switcher: Option<BranchSwitcherState>,

    // File switcher popup (open files list in editor panel header)
    pub(crate) file_switcher: Option<FileSwitcherState>,

    // Hover target for interactive feedback
    pub(crate) hover_target: Option<HoverTarget>,

    // File watcher for external change detection in editor panes
    pub(crate) file_watcher: Option<notify::RecommendedWatcher>,
    pub(crate) file_watch_rx: Option<mpsc::Receiver<notify::Result<notify::Event>>>,
    pub(crate) file_watch_dirty: Arc<AtomicBool>,

    // Event loop proxy for waking the loop from background threads (PTY, file watcher)
    pub(crate) event_loop_proxy: Option<EventLoopProxy<()>>,

    // Background git info poller
    pub(crate) git_poll_rx: Option<mpsc::Receiver<std::collections::HashMap<PathBuf, Option<tide_terminal::git::GitInfo>>>>,
    pub(crate) git_poll_cwd_tx: Option<mpsc::Sender<Vec<PathBuf>>>,
    pub(crate) git_poll_handle: Option<std::thread::JoinHandle<()>>,
    pub(crate) git_poll_stop: Arc<AtomicBool>,
}

impl App {
    fn new() -> Self {
        Self {
            window: None,
            surface: None,
            device: None,
            queue: None,
            surface_config: None,
            renderer: None,
            panes: HashMap::new(),
            layout: SplitLayout::new(),
            router: Router::new(),
            focused: None,
            file_tree: None,
            show_file_tree: false,
            file_tree_scroll: 0.0,
            file_tree_scroll_target: 0.0,
            file_tree_width: FILE_TREE_WIDTH,
            file_tree_border_dragging: false,
            file_tree_rect: None,
            sidebar_side: LayoutSide::Left,
            dock_side: LayoutSide::Right,
            sidebar_handle_dragging: false,
            dock_handle_dragging: false,
            handle_drag_preview: None,
            scale_factor: 1.0,
            window_size: PhysicalSize::new(1200, 800),
            modifiers: ModifiersState::empty(),
            last_cursor_pos: tide_core::Vec2::new(0.0, 0.0),
            last_cwd: None,
            badge_check_at: None,
            needs_redraw: true,
            last_frame: Instant::now(),
            ime_active: false,
            ime_composing: false,
            ime_preedit: String::new(),
            pending_hangul_initial: None,
            ime_dropped_preedit: None,
            last_pressed_with_text: None,
            pane_rects: Vec::new(),
            visual_pane_rects: Vec::new(),
            prev_visual_pane_rects: Vec::new(),
            pane_area_rect: None,
            pane_generations: HashMap::new(),
            layout_generation: 0,
            chrome_generation: 0,
            last_chrome_generation: u64::MAX,
            input_just_sent: false,
            input_sent_at: None,
            consecutive_dirty_frames: 0,
            pane_drag: PaneDragState::Idle,
            scroll_accumulator: HashMap::new(),
            mouse_left_pressed: false,
            search_focus: None,
            maximized_pane: None,
            show_editor_panel: false,
            editor_panel_maximized: false,
            editor_panel_tabs: Vec::new(),
            editor_panel_active: None,
            editor_panel_rect: None,
            editor_panel_width: EDITOR_PANEL_WIDTH,
            panel_border_dragging: false,
            editor_panel_width_manual: false,
            panel_tab_scroll: 0.0,
            panel_tab_scroll_target: 0.0,
            save_as_input: None,
            save_confirm: None,
            file_finder: None,
            editor_panel_placeholder: None,
            dark_mode: true,
            header_hit_zones: Vec::new(),
            branch_switcher: None,
            file_switcher: None,
            hover_target: None,
            file_watcher: None,
            file_watch_rx: None,
            file_watch_dirty: Arc::new(AtomicBool::new(false)),
            event_loop_proxy: None,
            git_poll_rx: None,
            git_poll_cwd_tx: None,
            git_poll_handle: None,
            git_poll_stop: Arc::new(AtomicBool::new(false)),
        }
    }

    fn update_cursor_icon(&self) {
        use winit::window::CursorIcon;
        let icon = match &self.hover_target {
            Some(HoverTarget::FileTreeEntry(_))
            | Some(HoverTarget::PaneTabBar(_))
            | Some(HoverTarget::PaneTabClose(_))
            | Some(HoverTarget::PanelTab(_))
            | Some(HoverTarget::PanelTabClose(_))
            | Some(HoverTarget::EmptyPanelButton)
            | Some(HoverTarget::EmptyPanelOpenFile)
            | Some(HoverTarget::FileFinderItem(_)) => CursorIcon::Pointer,
            Some(HoverTarget::SidebarHandle)
            | Some(HoverTarget::DockHandle) => CursorIcon::Grab,
            Some(HoverTarget::FileTreeBorder) => CursorIcon::ColResize,
            Some(HoverTarget::PanelBorder) => CursorIcon::ColResize,
            Some(HoverTarget::SplitBorder(SplitDirection::Horizontal)) => CursorIcon::ColResize,
            Some(HoverTarget::SplitBorder(SplitDirection::Vertical)) => CursorIcon::RowResize,
            None => CursorIcon::Default,
        };
        if let Some(window) = &self.window {
            window.set_cursor(icon);
        }
    }

    /// Compute the geometry for buttons in the empty editor panel.
    /// Returns (new_file_rect, open_file_rect) or None if not applicable.
    fn empty_panel_button_rects(&self) -> Option<(Rect, Rect)> {
        if !self.editor_panel_tabs.is_empty() || self.file_finder.is_some() {
            return None;
        }
        let panel_rect = self.editor_panel_rect?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let cell_height = cell_size.height;
        let label_y = panel_rect.y + panel_rect.height * 0.38;

        let new_btn_text = "New File";
        let new_hint_text = "  Cmd+Shift+E";
        let new_btn_w = (new_btn_text.len() + new_hint_text.len()) as f32 * cell_size.width + 24.0;
        let btn_h = cell_height + 12.0;
        let new_btn_x = panel_rect.x + (panel_rect.width - new_btn_w) / 2.0;
        let new_btn_y = label_y + cell_height + 16.0;
        let new_file_rect = Rect::new(new_btn_x, new_btn_y, new_btn_w, btn_h);

        let open_btn_text = "Open File";
        let open_hint_text = "  Cmd+O";
        let open_btn_w = (open_btn_text.len() + open_hint_text.len()) as f32 * cell_size.width + 24.0;
        let open_btn_x = panel_rect.x + (panel_rect.width - open_btn_w) / 2.0;
        let open_btn_y = new_btn_y + btn_h + 8.0;
        let open_file_rect = Rect::new(open_btn_x, open_btn_y, open_btn_w, btn_h);

        Some((new_file_rect, open_file_rect))
    }

    /// Check if a position is on the "New File" button in the empty editor panel.
    pub(crate) fn is_on_new_file_button(&self, pos: tide_core::Vec2) -> bool {
        self.empty_panel_button_rects()
            .is_some_and(|(new_rect, _)| new_rect.contains(pos))
    }

    /// Check if a position is on the "Open File" button in the empty editor panel.
    pub(crate) fn is_on_open_file_button(&self, pos: tide_core::Vec2) -> bool {
        self.empty_panel_button_rects()
            .is_some_and(|(_, open_rect)| open_rect.contains(pos))
    }

    /// Check if a position is on a file finder item. Returns the index into filtered list.
    pub(crate) fn file_finder_item_at(&self, pos: tide_core::Vec2) -> Option<usize> {
        let finder = self.file_finder.as_ref()?;
        let panel_rect = self.editor_panel_rect?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let line_height = cell_size.height * FILE_TREE_LINE_SPACING;

        // Search input area: top of panel
        let input_y = panel_rect.y + PANE_PADDING + 8.0;
        let input_h = cell_size.height + 12.0;
        let list_top = input_y + input_h + 8.0;

        if pos.y < list_top || pos.x < panel_rect.x || pos.x > panel_rect.x + panel_rect.width {
            return None;
        }

        let rel_y = pos.y - list_top;
        let idx = (rel_y / line_height) as usize + finder.scroll_offset;
        if idx < finder.filtered.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Hit-test the branch switcher popup. Returns the filtered index of the item under pos.
    pub(crate) fn branch_switcher_item_at(&self, pos: tide_core::Vec2) -> Option<usize> {
        let bs = self.branch_switcher.as_ref()?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let line_height = cell_size.height + 4.0;

        // Popup geometry (must match rendering)
        let popup_w = 260.0_f32;
        let popup_x = bs.anchor_rect.x;
        let popup_y = bs.anchor_rect.y + bs.anchor_rect.height + 4.0;
        let input_h = cell_size.height + 10.0;
        let list_top = popup_y + input_h;

        if pos.x < popup_x || pos.x > popup_x + popup_w || pos.y < list_top {
            return None;
        }

        let rel_y = pos.y - list_top;
        let idx = (rel_y / line_height) as usize + bs.scroll_offset;
        if idx < bs.filtered.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Hit-test the file switcher popup. Returns the filtered index of the item under pos.
    pub(crate) fn file_switcher_item_at(&self, pos: tide_core::Vec2) -> Option<usize> {
        let fs = self.file_switcher.as_ref()?;
        let cell_size = self.renderer.as_ref()?.cell_size();
        let line_height = cell_size.height + 4.0;

        let popup_w = 260.0_f32;
        let popup_x = fs.anchor_rect.x;
        let popup_y = fs.anchor_rect.y + fs.anchor_rect.height + 4.0;
        let input_h = cell_size.height + 10.0;
        let list_top = popup_y + input_h;

        if pos.x < popup_x || pos.x > popup_x + popup_w || pos.y < list_top {
            return None;
        }

        let rel_y = pos.y - list_top;
        let idx = (rel_y / line_height) as usize + fs.scroll_offset;
        if idx < fs.filtered.len() {
            Some(idx)
        } else {
            None
        }
    }

    /// Check if a position is inside the file switcher popup area.
    pub(crate) fn file_switcher_contains(&self, pos: tide_core::Vec2) -> bool {
        if let Some(ref fs) = self.file_switcher {
            let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
            if let Some(cs) = cell_size {
                let popup_w = 260.0_f32;
                let popup_x = fs.anchor_rect.x;
                let popup_y = fs.anchor_rect.y + fs.anchor_rect.height + 4.0;
                let line_height = cs.height + 4.0;
                let input_h = cs.height + 10.0;
                let max_visible = 10.min(fs.filtered.len());
                let popup_h = input_h + max_visible as f32 * line_height + 8.0;
                let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);
                return popup_rect.contains(pos);
            }
        }
        false
    }

    /// Check if a position is inside the branch switcher popup area.
    pub(crate) fn branch_switcher_contains(&self, pos: tide_core::Vec2) -> bool {
        if let Some(ref bs) = self.branch_switcher {
            let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
            if let Some(cs) = cell_size {
                let popup_w = 260.0_f32;
                let popup_x = bs.anchor_rect.x;
                let popup_y = bs.anchor_rect.y + bs.anchor_rect.height + 4.0;
                let line_height = cs.height + 4.0;
                let input_h = cs.height + 10.0;
                let max_visible = 10.min(bs.filtered.len());
                let popup_h = input_h + max_visible as f32 * line_height + 8.0;
                let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);
                return popup_rect.contains(pos);
            }
        }
        false
    }

    pub(crate) fn palette(&self) -> &'static ThemePalette {
        if self.dark_mode { &DARK } else { &LIGHT }
    }

    /// Compute the ideal editor panel width based on the number of terminal columns.
    /// 1 pane → half of available width, 2+ panes → one third, clamped to min 150.
    pub(crate) fn auto_editor_panel_width(&self) -> f32 {
        let logical = self.logical_size();
        let sidebar_reserved = if self.show_file_tree { self.file_tree_width } else { 0.0 };
        let available = (logical.width - sidebar_reserved).max(0.0);
        let pane_count = self.layout.pane_ids().len();
        let width = if pane_count <= 1 {
            available / 2.0
        } else {
            available / 3.0
        };
        width.max(150.0)
    }

    /// Get or allocate a placeholder PaneId for the empty editor panel.
    /// Used only for focus tracking and maximize — never added to panes or tabs.
    pub(crate) fn get_or_alloc_placeholder(&mut self) -> PaneId {
        if let Some(id) = self.editor_panel_placeholder {
            id
        } else {
            let id = self.layout.alloc_id();
            self.editor_panel_placeholder = Some(id);
            id
        }
    }

    /// Install an event-loop waker on a terminal pane so the PTY thread
    /// can wake us from `ControlFlow::Wait` when new output arrives.
    fn install_pty_waker(&self, pane: &TerminalPane) {
        if let Some(proxy) = &self.event_loop_proxy {
            let proxy = proxy.clone();
            pane.backend.set_waker(Box::new(move || {
                let _ = proxy.send_event(());
            }));
        }
    }

    fn create_initial_pane(&mut self) {
        let (layout, pane_id) = SplitLayout::with_initial_pane();
        self.layout = layout;

        let cell_size = self.renderer.as_ref().unwrap().cell_size();
        let logical_w = self.window_size.width as f32 / self.scale_factor;
        let logical_h = self.window_size.height as f32 / self.scale_factor;

        let cols = (logical_w / cell_size.width).max(1.0) as u16;
        let rows = (logical_h / cell_size.height).max(1.0) as u16;

        match TerminalPane::new(pane_id, cols, rows) {
            Ok(pane) => {
                self.install_pty_waker(&pane);
                self.panes.insert(pane_id, PaneKind::Terminal(pane));
                self.focused = Some(pane_id);
                self.router.set_focused(pane_id);
            }
            Err(e) => {
                log::error!("Failed to create terminal pane: {}", e);
            }
        }

        // Initialize file tree with CWD
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/"));
        let tree = FsTree::new(cwd.clone());
        self.file_tree = Some(tree);
        self.last_cwd = Some(cwd);
    }

    fn logical_size(&self) -> Size {
        Size::new(
            self.window_size.width as f32 / self.scale_factor,
            self.window_size.height as f32 / self.scale_factor,
        )
    }

    fn compute_layout(&mut self) {
        let logical = self.logical_size();
        let pane_ids = self.layout.pane_ids();

        let show_editor_panel = self.show_editor_panel;
        let show_file_tree = self.show_file_tree;

        // Compute how much space is reserved on each side.
        // Sidebar (file tree) and dock (editor panel) can each be on Left or Right.
        // Clamp widths so their total never exceeds the window (leave at least 100px for terminal).
        let max_panels = (logical.width - 100.0).max(0.0);
        let total = (if show_file_tree { self.file_tree_width } else { 0.0 })
            + (if show_editor_panel { self.editor_panel_width } else { 0.0 });
        if total > max_panels && total > 0.0 {
            let scale = max_panels / total;
            if show_file_tree { self.file_tree_width *= scale; }
            if show_editor_panel { self.editor_panel_width *= scale; }
        }
        let sidebar_width = if show_file_tree { self.file_tree_width } else { 0.0 };
        let dock_width = if show_editor_panel { self.editor_panel_width } else { 0.0 };

        let mut left_reserved = 0.0_f32;
        let mut right_reserved = 0.0_f32;

        if show_file_tree {
            match self.sidebar_side {
                LayoutSide::Left => left_reserved += sidebar_width,
                LayoutSide::Right => right_reserved += sidebar_width,
            }
        }
        if show_editor_panel {
            match self.dock_side {
                LayoutSide::Left => left_reserved += dock_width,
                LayoutSide::Right => right_reserved += dock_width,
            }
        }

        // When editor panel is maximized, it fills the full area (excluding file tree on its side)
        if self.editor_panel_maximized && show_editor_panel {
            let ft_reserved = if show_file_tree { sidebar_width } else { 0.0 };
            let ft_on_left = show_file_tree && self.sidebar_side == LayoutSide::Left;
            let panel_x = if ft_on_left { ft_reserved } else { 0.0 };
            let panel_w = (logical.width - ft_reserved).max(100.0);
            self.editor_panel_rect = Some(Rect::new(panel_x, 0.0, panel_w, logical.height));
            // File tree rect (still visible during panel maximize)
            if show_file_tree {
                let ft_x = if ft_on_left { 0.0 } else { logical.width - sidebar_width };
                self.file_tree_rect = Some(Rect::new(ft_x, 0.0, sidebar_width - PANE_GAP, logical.height));
            } else {
                self.file_tree_rect = None;
            }
            self.pane_area_rect = None;
            self.pane_rects = Vec::new();
            self.visual_pane_rects = Vec::new();
            self.layout_generation += 1;
            self.pane_generations.clear();
            self.chrome_generation += 1;
            self.layout.last_window_size = Some(Size::new(0.0, logical.height));
            return;
        }

        let terminal_area = Size::new(
            (logical.width - left_reserved - right_reserved).max(100.0),
            logical.height,
        );

        let terminal_offset_x = left_reserved;

        // Compute file_tree_rect and editor_panel_rect based on sides.
        // Rule: sidebar (file tree) is always outermost when both are on the same side.
        //
        // Layout pattern per component (width includes gap budget):
        //   Left side:  outer x=0, inner x=outer_w       (gap is at right end of each rect)
        //   Right side: inner x=W-reserved+GAP, outer x=W-outer_w+GAP  (gap is at left end)
        // Rect width is always component_width - PANE_GAP.
        let both_on_same_side = show_file_tree && show_editor_panel && self.sidebar_side == self.dock_side;

        if show_file_tree {
            let sidebar_x = match self.sidebar_side {
                LayoutSide::Left => 0.0, // always outer
                LayoutSide::Right => {
                    // always outer (at window edge)
                    logical.width - sidebar_width + PANE_GAP
                }
            };
            self.file_tree_rect = Some(Rect::new(
                sidebar_x,
                0.0,
                sidebar_width - PANE_GAP,
                logical.height,
            ));
        } else {
            self.file_tree_rect = None;
        }

        if show_editor_panel {
            let dock_x = match self.dock_side {
                LayoutSide::Left => {
                    if both_on_same_side {
                        sidebar_width // inner: after sidebar
                    } else {
                        0.0 // alone on left
                    }
                }
                LayoutSide::Right => {
                    if both_on_same_side {
                        // inner (closer to terminal)
                        logical.width - right_reserved + PANE_GAP
                    } else {
                        // alone on right (at window edge)
                        logical.width - dock_width + PANE_GAP
                    }
                }
            };
            self.editor_panel_rect = Some(Rect::new(
                dock_x,
                0.0,
                dock_width - PANE_GAP,
                logical.height,
            ));
        } else {
            self.editor_panel_rect = None;
        }

        // Store the pane area rect for root-level drop zone detection
        self.pane_area_rect = Some(Rect::new(terminal_offset_x, 0.0, terminal_area.width, terminal_area.height));

        // First compute to establish initial rects
        let _initial_rects = self.layout.compute(terminal_area, &pane_ids, self.focused);

        // Snap ratios to cell boundaries, then recompute with snapped ratios.
        // Skip during active border drags to prevent cumulative drift.
        let is_dragging = self.router.is_dragging_border()
            || self.panel_border_dragging
            || self.file_tree_border_dragging;
        if !is_dragging {
            if let Some(renderer) = &self.renderer {
                let cell_size = renderer.cell_size();
                let decorations = PaneDecorations {
                    gap: PANE_GAP,
                    padding: PANE_PADDING,
                    tab_bar_height: TAB_BAR_HEIGHT,
                };
                self.layout
                    .snap_ratios_to_cells(terminal_area, cell_size, &decorations);
            }
        }

        let mut rects = self.layout.compute(terminal_area, &pane_ids, self.focused);

        // Offset rects to account for file tree panel
        for (_, rect) in &mut rects {
            rect.x += terminal_offset_x;
        }

        // If a pane is maximized, override rects to show only that pane filling the terminal area
        if let Some(max_id) = self.maximized_pane {
            if rects.iter().any(|(id, _)| *id == max_id) {
                let full_rect = Rect::new(terminal_offset_x, 0.0, terminal_area.width, terminal_area.height);
                rects = vec![(max_id, full_rect)];
            } else {
                // Maximized pane no longer exists in layout — clear maximize
                self.maximized_pane = None;
            }
        }

        // Force grid rebuild if rects changed
        let rects_changed = rects != self.pane_rects;
        self.pane_rects = rects;

        // Compute visual rects: window edges flush (0px), internal edges share gap
        let logical = self.logical_size();
        let right_edge = terminal_offset_x + terminal_area.width;
        let half = PANE_GAP / 2.0;
        self.visual_pane_rects = self
            .pane_rects
            .iter()
            .map(|&(id, r)| {
                // Window boundary → 0px inset (flush), internal edge → half border width
                let left = if r.x <= terminal_offset_x + 0.5 { 0.0 } else { half };
                let top = if r.y <= 0.5 { 0.0 } else { half };
                let right = if r.x + r.width >= right_edge - 0.5 {
                    0.0
                } else {
                    half
                };
                let bottom = if r.y + r.height >= logical.height - 0.5 {
                    0.0
                } else {
                    half
                };
                let vr = Rect::new(
                    r.x + left,
                    r.y + top,
                    (r.width - left - right).max(1.0),
                    (r.height - top - bottom).max(1.0),
                );
                (id, vr)
            })
            .collect();

        // Resize terminal backends to match the actual visible content area.
        // Uses visual rects + PANE_PADDING to match the render inner rect exactly.
        // During border drag, skip PTY resize to avoid SIGWINCH spam.
        let is_dragging = self.router.is_dragging_border()
            || self.panel_border_dragging
            || self.file_tree_border_dragging;
        if !is_dragging {
            if let Some(renderer) = &self.renderer {
                let cell_size = renderer.cell_size();
                for &(id, vr) in &self.visual_pane_rects {
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&id) {
                        let content_rect = Rect::new(
                            vr.x + PANE_PADDING,
                            vr.y + TAB_BAR_HEIGHT,
                            (vr.width - 2.0 * PANE_PADDING).max(cell_size.width),
                            (vr.height - TAB_BAR_HEIGHT - PANE_PADDING).max(cell_size.height),
                        );
                        pane.resize_to_rect(content_rect, cell_size);
                    }
                }
            }
        }

        if rects_changed {
            self.layout_generation += 1;
            self.pane_generations.clear();
            self.chrome_generation += 1;
        }

        // Clamp panel tab scroll after layout change (container may have grown)
        self.clamp_panel_tab_scroll();

        // Store window size for layout drag operations
        self.layout.last_window_size = Some(terminal_area);
    }

    /// Ensure the file watcher is initialized. Returns true if watcher is available.
    fn ensure_file_watcher(&mut self) -> bool {
        if self.file_watcher.is_some() {
            return true;
        }
        let (tx, rx) = mpsc::channel();
        let proxy = self.event_loop_proxy.clone();
        let dirty_flag = self.file_watch_dirty.clone();
        match notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
            dirty_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            // Wake the event loop so file changes are processed immediately
            if let Some(ref p) = proxy {
                let _ = p.send_event(());
            }
        }) {
            Ok(watcher) => {
                self.file_watcher = Some(watcher);
                self.file_watch_rx = Some(rx);
                true
            }
            Err(e) => {
                log::error!("Failed to create file watcher: {}", e);
                false
            }
        }
    }

    /// Start watching a file path for changes.
    fn watch_file(&mut self, path: &std::path::Path) {
        if !self.ensure_file_watcher() {
            return;
        }
        if let Some(watcher) = self.file_watcher.as_mut() {
            if let Err(e) = watcher.watch(path, notify::RecursiveMode::NonRecursive) {
                log::error!("Failed to watch {:?}: {}", path, e);
            }
        }
    }

    /// Stop watching a file path.
    fn unwatch_file(&mut self, path: &std::path::Path) {
        if let Some(watcher) = self.file_watcher.as_mut() {
            let _ = watcher.unwatch(path);
        }
    }

    fn update(&mut self) {
        // Process PTY output for terminal panes only
        for pane in self.panes.values_mut() {
            if let PaneKind::Terminal(terminal) = pane {
                if terminal.cursor_suppress > 0 {
                    terminal.cursor_suppress -= 1;
                    self.needs_redraw = true;
                }
                let old_gen = terminal.backend.grid_generation();
                terminal.backend.process();
                // Re-execute search when terminal output changes
                if terminal.backend.grid_generation() != old_gen {
                    if let Some(ref mut s) = terminal.search {
                        if !s.query.is_empty() {
                            search::execute_search_terminal(s, &terminal.backend);
                        }
                    }
                }
            }
        }

        // Poll file tree events
        if let Some(tree) = self.file_tree.as_mut() {
            let had_changes = tree.poll_events();
            if had_changes {
                self.chrome_generation += 1;
            }
        }

        // Poll editor file watch events
        if let Some(rx) = self.file_watch_rx.as_ref() {
            let mut changed_paths: Vec<PathBuf> = Vec::new();
            let mut removed_paths: Vec<PathBuf> = Vec::new();
            while let Ok(event_result) = rx.try_recv() {
                if let Ok(event) = event_result {
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            for path in event.paths {
                                if !changed_paths.contains(&path) {
                                    changed_paths.push(path);
                                }
                            }
                        }
                        EventKind::Remove(_) => {
                            for path in event.paths {
                                if !removed_paths.contains(&path) {
                                    removed_paths.push(path);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }
            for changed_path in &changed_paths {
                // Find editor panes with this file path
                let matching_ids: Vec<tide_core::PaneId> = self.panes.iter()
                    .filter_map(|(&id, pane)| {
                        if let PaneKind::Editor(editor) = pane {
                            if editor.editor.file_path() == Some(changed_path.as_path()) {
                                return Some(id);
                            }
                        }
                        None
                    })
                    .collect();

                // Check if the file actually exists (macOS FSEvents may report
                // Modify events for deleted files)
                let file_exists = changed_path.exists();

                for id in matching_ids {
                    if let Some(PaneKind::Editor(editor_pane)) = self.panes.get_mut(&id) {
                        if !file_exists {
                            // File doesn't exist — treat as deletion
                            if !editor_pane.editor.is_modified() {
                                // Buffer clean → will be closed below via removed_paths
                                // Add to removed_paths to avoid duplication
                                if !removed_paths.contains(changed_path) {
                                    removed_paths.push(changed_path.clone());
                                }
                            } else {
                                editor_pane.disk_changed = true;
                                editor_pane.file_deleted = true;
                                // Exit diff mode — disk content is stale
                                editor_pane.diff_mode = false;
                                editor_pane.disk_content = None;
                            }
                        } else {
                            // File was recreated or modified
                            editor_pane.file_deleted = false;
                            editor_pane.diff_mode = false;
                            editor_pane.disk_content = None;
                            if !editor_pane.editor.is_modified() {
                                // Buffer clean → auto-reload silently
                                if let Err(e) = editor_pane.editor.reload() {
                                    log::error!("Failed to reload {:?}: {}", changed_path, e);
                                }
                                editor_pane.disk_changed = false;
                            } else {
                                // Buffer dirty → mark disk changed, let user decide
                                editor_pane.disk_changed = true;
                            }
                        }
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&id);
                    }
                }
            }

            // Handle removed files: close clean tabs, mark dirty tabs
            let mut tabs_to_close: Vec<tide_core::PaneId> = Vec::new();
            for removed_path in &removed_paths {
                let matching_ids: Vec<tide_core::PaneId> = self.panes.iter()
                    .filter_map(|(&id, pane)| {
                        if let PaneKind::Editor(editor) = pane {
                            if editor.editor.file_path() == Some(removed_path.as_path()) {
                                return Some(id);
                            }
                        }
                        None
                    })
                    .collect();

                for id in matching_ids {
                    if let Some(PaneKind::Editor(editor_pane)) = self.panes.get_mut(&id) {
                        if !editor_pane.editor.is_modified() {
                            // Buffer clean → close the tab
                            tabs_to_close.push(id);
                        } else {
                            // Buffer dirty → mark as deleted and disk changed
                            editor_pane.disk_changed = true;
                            editor_pane.file_deleted = true;
                            // Exit diff mode — disk content is stale
                            editor_pane.diff_mode = false;
                            editor_pane.disk_content = None;
                            self.chrome_generation += 1;
                            self.pane_generations.remove(&id);
                        }
                    }
                }
            }
            for tab_id in tabs_to_close {
                self.close_editor_panel_tab(tab_id);
            }
        }

        // Smooth scroll animation
        const SCROLL_LERP: f32 = 0.45;
        const SCROLL_SNAP: f32 = 0.5;

        let ft_diff = self.file_tree_scroll_target - self.file_tree_scroll;
        if ft_diff.abs() > SCROLL_SNAP {
            self.file_tree_scroll += ft_diff * SCROLL_LERP;
            self.chrome_generation += 1;
            self.needs_redraw = true;
        } else if ft_diff.abs() > 0.0 {
            // Final snap (< 0.5px) — set position but skip chrome rebuild.
            // Next natural chrome rebuild will use the correct final value.
            self.file_tree_scroll = self.file_tree_scroll_target;
        }

        let pt_diff = self.panel_tab_scroll_target - self.panel_tab_scroll;
        if pt_diff.abs() > SCROLL_SNAP {
            self.panel_tab_scroll += pt_diff * SCROLL_LERP;
            self.chrome_generation += 1;
            self.needs_redraw = true;
        } else if pt_diff.abs() > 0.0 {
            self.panel_tab_scroll = self.panel_tab_scroll_target;
        }

        // Consume git info from background poller (non-blocking).
        // CWD/idle badge checks are event-driven (see badge_check_at in about_to_wait).
        // Git info still needs periodic consumption since it arrives asynchronously.
        self.update_terminal_badges();

        // Start git poller if not yet running
        if self.git_poll_handle.is_none() {
            self.start_git_poller();
        }
    }
}

// ──────────────────────────────────────────────
// ApplicationHandler implementation
// ──────────────────────────────────────────────

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Try loading a saved session to restore window size
        let saved_session = session::load_session();
        let (win_w, win_h) = saved_session
            .as_ref()
            .map(|s| (s.window_width as f64, s.window_height as f64))
            .unwrap_or((1200.0, 800.0));

        let attrs = WindowAttributes::default()
            .with_title("Tide")
            .with_inner_size(LogicalSize::new(win_w, win_h))
            .with_min_inner_size(LogicalSize::new(400.0, 300.0));

        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        window.set_ime_allowed(true);

        self.window = Some(window);
        self.init_gpu();

        // Crash recovery: if the running marker exists, the previous session
        // ended abnormally → restore everything.  Otherwise only restore prefs.
        let is_crash = session::is_crash_recovery();
        let restored = match saved_session {
            Some(session) if is_crash => self.restore_from_session(session),
            Some(ref session) => {
                self.restore_preferences(session);
                true
            }
            None => false,
        };
        if !restored {
            self.create_initial_pane();
        }
        session::create_running_marker();
        self.compute_layout();
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Handle RedrawRequested directly — must NOT fall through to the
        // unconditional `needs_redraw = true` at the end of this function,
        // otherwise every completed frame immediately requests another frame,
        // creating an infinite render loop that leaks GPU staging memory.
        if matches!(event, WindowEvent::RedrawRequested) {
            self.update();
            self.render();
            self.needs_redraw = false;
            self.last_frame = Instant::now();
            return;
        }

        // Handle search bar clicks before anything else
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.check_search_bar_click() {
                self.needs_redraw = true;
                return;
            }
        }

        // Handle empty panel "New File" / "Open File" button clicks
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.is_on_new_file_button(self.last_cursor_pos) {
                self.new_editor_pane();
                self.needs_redraw = true;
                return;
            }
            if self.is_on_open_file_button(self.last_cursor_pos) {
                self.open_file_finder();
                self.needs_redraw = true;
                return;
            }
        }

        // Handle file finder item click
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            // Branch switcher popup click handling
            if self.branch_switcher.is_some() {
                if let Some(idx) = self.branch_switcher_item_at(self.last_cursor_pos) {
                    let selected = self.branch_switcher.as_ref()
                        .and_then(|bs| {
                            let entry_idx = *bs.filtered.get(idx)?;
                            Some((bs.pane_id, bs.branches.get(entry_idx)?.name.clone()))
                        });
                    self.branch_switcher = None;
                    if let Some((pane_id, branch_name)) = selected {
                        if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&pane_id) {
                            let cmd = format!("git checkout {}\n", branch_name);
                            pane.backend.write(cmd.as_bytes());
                        }
                    }
                    self.needs_redraw = true;
                    return;
                } else if !self.branch_switcher_contains(self.last_cursor_pos) {
                    // Click outside popup → close it
                    self.branch_switcher = None;
                    self.needs_redraw = true;
                    return;
                }
            }

            // File switcher popup click handling
            if self.file_switcher.is_some() {
                if let Some(idx) = self.file_switcher_item_at(self.last_cursor_pos) {
                    let selected_pane_id = self.file_switcher.as_ref()
                        .and_then(|fs| {
                            let entry_idx = *fs.filtered.get(idx)?;
                            Some(fs.entries.get(entry_idx)?.pane_id)
                        });
                    self.file_switcher = None;
                    if let Some(pane_id) = selected_pane_id {
                        self.editor_panel_active = Some(pane_id);
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&pane_id);
                    }
                    self.needs_redraw = true;
                    return;
                } else if !self.file_switcher_contains(self.last_cursor_pos) {
                    self.file_switcher = None;
                    self.needs_redraw = true;
                    return;
                }
            }

            if let Some(idx) = self.file_finder_item_at(self.last_cursor_pos) {
                if let Some(ref finder) = self.file_finder {
                    if let Some(&entry_idx) = finder.filtered.get(idx) {
                        let path = finder.base_dir.join(&finder.entries[entry_idx]);
                        self.close_file_finder();
                        self.open_editor_pane(path);
                        self.needs_redraw = true;
                        return;
                    }
                }
            }
        }

        // Handle editor panel clicks before general routing
        // Tab clicks flow through to handle_window_event for drag support.
        // Only intercept: close buttons and content area clicks.
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if let Some(ref panel_rect) = self.editor_panel_rect {
                let near_border = (self.last_cursor_pos.x - panel_rect.x).abs() < 5.0;
                let in_handle_strip = self.last_cursor_pos.y < PANE_PADDING;
                if panel_rect.contains(self.last_cursor_pos) && !near_border && !in_handle_strip {
                    // Tab close button → handle here
                    if let Some(tab_id) = self.panel_tab_close_at(self.last_cursor_pos) {
                        self.close_editor_panel_tab(tab_id);
                        self.needs_redraw = true;
                        return;
                    }
                    // Tab click → cancel save confirm and let flow for drag initiation
                    if self.panel_tab_at(self.last_cursor_pos).is_some() {
                        self.cancel_save_confirm();
                        // fall through
                    } else if self.handle_notification_bar_click(self.last_cursor_pos) {
                        // Conflict bar button was clicked
                        return;
                    } else {
                        // Content area click → focus + cursor + start selection drag
                        self.mouse_left_pressed = true;
                        self.handle_editor_panel_click(self.last_cursor_pos);
                        self.needs_redraw = true;
                        return;
                    }
                }
            }
        }

        // Handle notification bar clicks on left-side panes
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            // Check left-side pane notification bars (not inside panel)
            let in_panel = self.editor_panel_rect.is_some_and(|pr| pr.contains(self.last_cursor_pos));
            if !in_panel && self.handle_notification_bar_click(self.last_cursor_pos) {
                return;
            }
        }

        // Handle header badge clicks (close, git branch, git status)
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.check_header_click() {
                return;
            }
        }

        // Handle pane tab bar close button clicks (fallback for header)
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if let Some(pane_id) = self.pane_tab_close_at(self.last_cursor_pos) {
                self.close_specific_pane(pane_id);
                self.needs_redraw = true;
                return;
            }
        }

        // Handle file tree clicks before general routing
        // (skip the top handle strip so drag-to-move can work)
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.show_file_tree {
                if let Some(ft_rect) = self.file_tree_rect {
                    let pos = self.last_cursor_pos;
                    if pos.x >= ft_rect.x && pos.x < ft_rect.x + ft_rect.width && pos.y >= PANE_PADDING {
                        self.handle_file_tree_click(pos);
                        return;
                    }
                }
            }
        }

        // Check if this event can skip a redraw (idle cursor hover, modifier change)
        let skip_redraw = match &event {
            WindowEvent::CursorMoved { .. } => {
                !self.mouse_left_pressed
                    && !self.file_tree_border_dragging
                    && !self.panel_border_dragging
                    && !self.sidebar_handle_dragging
                    && !self.dock_handle_dragging
                    && !self.router.is_dragging_border()
                    && matches!(self.pane_drag, PaneDragState::Idle)
            }
            WindowEvent::ModifiersChanged(_) => true,
            _ => false,
        };

        self.handle_window_event(event);

        if !skip_redraw {
            self.needs_redraw = true;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Check if any terminal has new PTY output (cheap atomic load)
        let mut had_pty_output = false;
        for pane in self.panes.values() {
            if let PaneKind::Terminal(terminal) = pane {
                if terminal.backend.has_new_output() {
                    self.needs_redraw = true;
                    self.input_just_sent = false;
                    self.input_sent_at = None;
                    had_pty_output = true;
                    break;
                }
            }
        }

        // PTY just produced output → schedule a deferred badge check.
        // After a cd (by user or AI agent), the shell emits a new prompt.
        // We check CWD/idle 150ms after the last output burst settles.
        if had_pty_output {
            self.badge_check_at = Some(Instant::now() + Duration::from_millis(150));
        }

        // Check if file watcher has pending events
        if self.file_watch_dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.needs_redraw = true;
        }

        // Consume git poller results (woken via EventLoopProxy::send_event)
        if self.consume_git_poll_results() {
            self.chrome_generation += 1;
            self.needs_redraw = true;
        }

        if self.needs_redraw {
            // Adaptive frame throttling: start at ~120fps, drop to ~30fps during
            // sustained PTY output (e.g. long-running commands) to save battery/GPU.
            let min_frame_time = if self.consecutive_dirty_frames > 60 {
                Duration::from_micros(33_333) // ~30fps after 0.5s of continuous output
            } else {
                Duration::from_micros(8_333) // ~120fps normal
            };
            let elapsed = self.last_frame.elapsed();
            if elapsed < min_frame_time {
                event_loop.set_control_flow(ControlFlow::wait_duration(min_frame_time - elapsed));
                return;
            }
            self.consecutive_dirty_frames += 1;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        } else if self.input_just_sent {
            // User just typed — reset adaptive throttle so next PTY burst starts at 120fps
            self.consecutive_dirty_frames = 0;
            // Poll aggressively while awaiting PTY response after keypress
            // 50ms safety timeout: stop polling if PTY hasn't responded
            if self.input_sent_at.is_some_and(|t| t.elapsed() > Duration::from_millis(50)) {
                self.input_just_sent = false;
                self.input_sent_at = None;
                event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(8)));
            } else {
                event_loop.set_control_flow(ControlFlow::Poll);
            }
        } else {
            // Idle — check if a deferred badge update is due
            self.consecutive_dirty_frames = 0;

            if let Some(check_at) = self.badge_check_at {
                let now = Instant::now();
                if now >= check_at {
                    // PTY output settled — run CWD/idle badge check now
                    self.badge_check_at = None;
                    self.update_file_tree_cwd();
                    self.update_terminal_badges();

                    // Send CWDs to git poller so git badges update too
                    if let Some(ref tx) = self.git_poll_cwd_tx {
                        let mut cwds: Vec<PathBuf> = Vec::new();
                        for pane in self.panes.values() {
                            if let PaneKind::Terminal(p) = pane {
                                if let Some(ref cwd) = p.cwd {
                                    if !cwds.contains(cwd) {
                                        cwds.push(cwd.clone());
                                    }
                                }
                            }
                        }
                        let _ = tx.send(cwds);
                    }

                    // If badge changed, request a frame
                    if self.needs_redraw {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                } else {
                    // Not yet — sleep until the scheduled check time
                    event_loop.set_control_flow(ControlFlow::WaitUntil(check_at));
                    return;
                }
            }

            // Truly idle: sleep until PTY waker or user input wakes us
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}

// ──────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────

fn main() {
    env_logger::init();

    #[cfg(target_os = "macos")]
    let event_loop = {
        use winit::platform::macos::EventLoopBuilderExtMacOS;
        EventLoop::builder()
            .with_default_menu(false)
            .build()
            .expect("create event loop")
    };
    #[cfg(not(target_os = "macos"))]
    let event_loop = EventLoop::new().expect("create event loop");
    let proxy = event_loop.create_proxy();
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut app = App::new();
    app.event_loop_proxy = Some(proxy);
    event_loop.run_app(&mut app).expect("run event loop");
}
