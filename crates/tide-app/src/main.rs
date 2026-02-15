// Tide v0.1 — Integration (Step 3)
// Wires all crates together: winit window, wgpu surface, renderer, terminal panes,
// layout engine, input router, file tree, and CWD following.

mod action;
mod diff;
mod drag_drop;
mod editor_pane;
mod event_handler;
mod file_tree;
mod gpu;
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

    // Window state
    pub(crate) scale_factor: f32,
    pub(crate) window_size: PhysicalSize<u32>,
    pub(crate) modifiers: ModifiersState,
    pub(crate) last_cursor_pos: tide_core::Vec2,

    // CWD tracking
    pub(crate) last_cwd: Option<PathBuf>,
    pub(crate) last_cwd_check: Instant,

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

    // Hover target for interactive feedback
    pub(crate) hover_target: Option<HoverTarget>,

    // File watcher for external change detection in editor panes
    pub(crate) file_watcher: Option<notify::RecommendedWatcher>,
    pub(crate) file_watch_rx: Option<mpsc::Receiver<notify::Result<notify::Event>>>,
    pub(crate) file_watch_dirty: Arc<AtomicBool>,

    // Event loop proxy for waking the loop from background threads (PTY, file watcher)
    pub(crate) event_loop_proxy: Option<EventLoopProxy<()>>,
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
            scale_factor: 1.0,
            window_size: PhysicalSize::new(1200, 800),
            modifiers: ModifiersState::empty(),
            last_cursor_pos: tide_core::Vec2::new(0.0, 0.0),
            last_cwd: None,
            last_cwd_check: Instant::now(),
            needs_redraw: true,
            last_frame: Instant::now(),
            ime_active: false,
            ime_composing: false,
            ime_preedit: String::new(),
            pending_hangul_initial: None,
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
            hover_target: None,
            file_watcher: None,
            file_watch_rx: None,
            file_watch_dirty: Arc::new(AtomicBool::new(false)),
            event_loop_proxy: None,
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

    pub(crate) fn palette(&self) -> &'static ThemePalette {
        if self.dark_mode { &DARK } else { &LIGHT }
    }

    /// Compute the ideal editor panel width based on the number of terminal columns.
    /// 1 pane → half of available width, 2+ panes → one third, clamped to min 150.
    pub(crate) fn auto_editor_panel_width(&self) -> f32 {
        let logical = self.logical_size();
        let left_reserved = if self.show_file_tree { self.file_tree_width } else { 0.0 };
        let available = (logical.width - left_reserved).max(0.0);
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

        // When editor panel is maximized, it fills the full area (excluding file tree)
        if self.editor_panel_maximized && show_editor_panel {
            let left_reserved = if self.show_file_tree { self.file_tree_width } else { 0.0 };
            self.editor_panel_rect = Some(Rect::new(
                left_reserved,
                0.0,
                (logical.width - left_reserved).max(100.0),
                logical.height,
            ));
            self.pane_area_rect = None;
            self.pane_rects = Vec::new();
            self.visual_pane_rects = Vec::new();
            self.layout_generation += 1;
            self.pane_generations.clear();
            self.chrome_generation += 1;
            self.layout.last_window_size = Some(Size::new(0.0, logical.height));
            return;
        }

        // Reserve space for file tree (left) and editor panel (right)
        let left_reserved = if self.show_file_tree { self.file_tree_width } else { 0.0 };
        let right_reserved = if show_editor_panel { self.editor_panel_width } else { 0.0 };

        let terminal_area = Size::new(
            (logical.width - left_reserved - right_reserved).max(100.0),
            logical.height,
        );

        let terminal_offset_x = left_reserved;

        // Compute editor panel rect (edge-to-edge, gap provided by clear color)
        if show_editor_panel {
            let panel_x = terminal_offset_x + terminal_area.width;
            self.editor_panel_rect = Some(Rect::new(
                panel_x + PANE_GAP,
                0.0,
                self.editor_panel_width - PANE_GAP,
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
            self.file_tree_scroll = self.file_tree_scroll_target;
            self.chrome_generation += 1;
        }

        let pt_diff = self.panel_tab_scroll_target - self.panel_tab_scroll;
        if pt_diff.abs() > SCROLL_SNAP {
            self.panel_tab_scroll += pt_diff * SCROLL_LERP;
            self.chrome_generation += 1;
            self.needs_redraw = true;
        } else if pt_diff.abs() > 0.0 {
            self.panel_tab_scroll = self.panel_tab_scroll_target;
            self.chrome_generation += 1;
        }

        // Periodic CWD check (every 500ms)
        if self.last_cwd_check.elapsed() > Duration::from_millis(500) {
            self.last_cwd_check = Instant::now();
            self.update_file_tree_cwd();
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

        // Try restoring from saved session; fall back to fresh pane
        let restored = if let Some(session) = saved_session {
            self.restore_from_session(session)
        } else {
            false
        };
        if !restored {
            self.create_initial_pane();
        }
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
                if panel_rect.contains(self.last_cursor_pos) && !near_border {
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

        // Handle pane tab bar close button clicks
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
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.show_file_tree && self.last_cursor_pos.x < self.file_tree_width - 5.0 {
                self.handle_file_tree_click(self.last_cursor_pos);
                return;
            }
        }

        self.handle_window_event(event);
        self.needs_redraw = true;
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Check if any terminal has new PTY output (cheap atomic load)
        for pane in self.panes.values() {
            if let PaneKind::Terminal(terminal) = pane {
                if terminal.backend.has_new_output() {
                    self.needs_redraw = true;
                    self.input_just_sent = false;
                    self.input_sent_at = None;
                    break;
                }
            }
        }

        // Check if file watcher has pending events
        if self.file_watch_dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.needs_redraw = true;
        }

        if self.needs_redraw {
            // Throttle to ~120fps max to prevent GPU staging buffer accumulation.
            // Without this, continuous PTY output causes an unthrottled render loop.
            let min_frame_time = Duration::from_micros(8_333); // ~120fps
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
            // Truly idle: sleep until PTY waker or user input wakes us
            self.consecutive_dirty_frames = 0;
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
