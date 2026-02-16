// Tide v0.1 — Integration (Step 3)
// Wires all crates together: winit window, wgpu surface, renderer, terminal panes,
// layout engine, input router, file tree, and CWD following.

mod action;
mod diff;
mod diff_pane;
mod drag_drop;
mod editor_pane;
mod event_handler;
mod event_loop;
mod file_tree;
mod gpu;
mod header;
mod input;
mod layout_compute;
mod pane;
mod rendering;
mod search;
mod session;
mod settings;
mod theme;
mod ui;
mod ui_state;
mod update;

pub(crate) use ui_state::*;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use winit::dpi::PhysicalSize;
use winit::event_loop::{ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::ModifiersState;
use winit::window::Window;

use tide_core::{PaneId, Rect, Renderer, Size};
use tide_input::Router;
use tide_layout::SplitLayout;
use tide_renderer::WgpuRenderer;
use tide_tree::FsTree;

use drag_drop::{HoverTarget, PaneDragState};
use pane::{PaneKind, TerminalPane};
use theme::*;

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

    // Git switcher popup (integrated branch + worktree)
    pub(crate) git_switcher: Option<GitSwitcherState>,

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
    pub(crate) git_poll_rx: Option<mpsc::Receiver<std::collections::HashMap<PathBuf, (Option<tide_terminal::git::GitInfo>, usize)>>>,
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
            git_switcher: None,
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

    pub(crate) fn logical_size(&self) -> Size {
        Size::new(
            self.window_size.width as f32 / self.scale_factor,
            self.window_size.height as f32 / self.scale_factor,
        )
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
