// Tide — GPU terminal emulator with native macOS platform layer.
// Wires all crates together: native window, wgpu surface, renderer, terminal panes,
// layout engine, input router, file tree, and CWD following.

mod action;
mod browser_pane;
mod diff;
mod diff_pane;
mod drag_drop;
mod editor_pane;
mod event_handler;
mod event_loop;
mod file_tree;
mod gpu;
mod header;
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

use tide_core::{Modifiers, PaneId, Rect, Renderer, Size};
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
    pub(crate) window_size: (u32, u32),
    pub(crate) modifiers: Modifiers,
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

    /// Deferred PTY resize after window resize settles (debounce).
    /// While Some, compute_layout skips PTY resize to avoid SIGWINCH spam.
    pub(crate) resize_deferred_at: Option<Instant>,

    // IME composition state (used for rendering preedit overlays)
    pub(crate) ime_composing: bool,
    pub(crate) ime_preedit: String,

    // Tracks which proxy was last focused so we can clear IME state on change
    pub(crate) last_ime_target: Option<u64>,

    // Pending IME proxy view operations (processed in handle_platform_event)
    pub(crate) pending_ime_proxy_creates: Vec<u64>,
    pub(crate) pending_ime_proxy_removes: Vec<u64>,

    // IME cursor area dirty flag: only recompute geometry when cursor may have moved
    pub(crate) ime_cursor_dirty: bool,

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

    // Track dock active tab to force grid reassembly on change
    pub(crate) last_editor_panel_active: Option<PaneId>,

    // Input latency: skip 8ms sleep after keypress while awaiting PTY response
    pub(crate) input_just_sent: bool,
    pub(crate) input_sent_at: Option<Instant>,

    // Pane drag & drop
    pub(crate) pane_drag: PaneDragState,

    // Scroll accumulator for sub-pixel precision (prevents jitter from PixelDelta)
    pub(crate) scroll_accumulator: HashMap<PaneId, f32>,

    // Mouse state for text selection
    pub(crate) mouse_left_pressed: bool,

    // Search focus: which pane's search bar has keyboard focus
    pub(crate) search_focus: Option<PaneId>,

    // Pane area layout mode (Split = tiled 2D, Stacked = dock-like tabs)
    pub(crate) pane_area_mode: PaneAreaMode,

    // Editor panel visibility toggle
    pub(crate) show_editor_panel: bool,

    // Editor panel maximize (temporary full-area display of entire editor panel)
    pub(crate) editor_panel_maximized: bool,

    // Pane area maximize (terminal fills screen minus file tree, hides dock)
    pub(crate) pane_area_maximized: bool,

    // Editor panel (right-side tab panel)
    // NOTE: editor_panel_tabs / editor_panel_active are terminal-bound (TerminalPane.editors / .active_editor).
    // Use active_editor_tabs() / active_editor_tab() accessors.
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

    // Pending terminal close: set when closing a terminal that has dirty editors.
    // After each save-confirm resolution, retries closing the terminal.
    pub(crate) pending_terminal_close: Option<tide_core::PaneId>,

    // File finder state (in-panel file search/open UI)
    pub(crate) file_finder: Option<FileFinderState>,



    // Auto-shown flag: editor panel was auto-shown for an editor; auto-hide when switching
    // to a terminal with no editors.
    pub(crate) editor_panel_auto_shown: bool,

    // Theme mode
    pub(crate) dark_mode: bool,

    // Top inset for macOS transparent titlebar (traffic light area)
    pub(crate) top_inset: f32,
    pub(crate) is_fullscreen: bool,
    pub(crate) is_occluded: bool,

    // Header hit zones (for badge click handling)
    pub(crate) header_hit_zones: Vec<header::HeaderHitZone>,

    // Git switcher popup (integrated branch + worktree)
    pub(crate) git_switcher: Option<GitSwitcherState>,

    // File switcher popup (open files list in editor panel header)
    pub(crate) file_switcher: Option<FileSwitcherState>,

    // Hover target for interactive feedback
    pub(crate) hover_target: Option<HoverTarget>,

    // Context menu (right-click on file tree)
    pub(crate) context_menu: Option<ContextMenuState>,

    // FocusArea: which area currently has keyboard focus
    pub(crate) focus_area: FocusArea,

    // File tree keyboard cursor index (visible entry index)
    pub(crate) file_tree_cursor: usize,

    // File tree inline rename
    pub(crate) file_tree_rename: Option<FileTreeRenameState>,

    // Branch cleanup confirmation (when closing terminal on feature branch)
    pub(crate) branch_cleanup: Option<BranchCleanupState>,

    // Config page overlay
    pub(crate) config_page: Option<ConfigPageState>,

    // Loaded settings
    pub(crate) settings: settings::TideSettings,

    // Git status for file tree entries
    pub(crate) file_tree_git_status: std::collections::HashMap<PathBuf, tide_core::FileGitStatus>,
    pub(crate) file_tree_dir_git_status: std::collections::HashMap<PathBuf, tide_core::FileGitStatus>,
    pub(crate) file_tree_git_root: Option<PathBuf>,

    // File watcher for external change detection in editor panes
    pub(crate) file_watcher: Option<notify::RecommendedWatcher>,
    pub(crate) file_watch_rx: Option<mpsc::Receiver<notify::Result<notify::Event>>>,
    pub(crate) file_watch_dirty: Arc<AtomicBool>,

    // Waker for poking the event loop from background threads (PTY, file watcher)
    pub(crate) event_loop_waker: Option<tide_platform::WakeCallback>,

    // Background git info poller
    pub(crate) git_poll_rx: Option<mpsc::Receiver<std::collections::HashMap<PathBuf, (Option<tide_terminal::git::GitInfo>, usize)>>>,
    pub(crate) git_poll_cwd_tx: Option<mpsc::Sender<Vec<PathBuf>>>,
    pub(crate) git_poll_handle: Option<std::thread::JoinHandle<()>>,
    pub(crate) git_poll_stop: Arc<AtomicBool>,

    // Platform pointers for webview management (macOS)
    pub(crate) content_view_ptr: Option<*mut std::ffi::c_void>,
    pub(crate) window_ptr: Option<*mut std::ffi::c_void>,

    // Window visibility: false until first frame renders (avoids blank window flash)
    pub(crate) window_shown: bool,
}

impl App {
    fn new() -> Self {
        Self {
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
            window_size: (1200, 800),
            modifiers: Modifiers::default(),
            last_cursor_pos: tide_core::Vec2::new(0.0, 0.0),
            last_cwd: None,
            badge_check_at: None,
            needs_redraw: true,
            last_frame: Instant::now(),
            resize_deferred_at: None,
            ime_composing: false,
            ime_preedit: String::new(),
            last_ime_target: None,
            pending_ime_proxy_creates: Vec::new(),
            pending_ime_proxy_removes: Vec::new(),
            ime_cursor_dirty: true,
            pane_rects: Vec::new(),
            visual_pane_rects: Vec::new(),
            prev_visual_pane_rects: Vec::new(),
            pane_area_rect: None,
            pane_generations: HashMap::new(),
            layout_generation: 0,
            chrome_generation: 0,
            last_chrome_generation: u64::MAX,
            last_editor_panel_active: None,
            input_just_sent: false,
            input_sent_at: None,
            pane_drag: PaneDragState::Idle,
            scroll_accumulator: HashMap::new(),
            mouse_left_pressed: false,
            search_focus: None,
            pane_area_mode: PaneAreaMode::default(),
            show_editor_panel: false,
            editor_panel_maximized: false,
            pane_area_maximized: false,
            editor_panel_rect: None,
            editor_panel_width: EDITOR_PANEL_WIDTH,
            panel_border_dragging: false,
            editor_panel_width_manual: false,
            panel_tab_scroll: 0.0,
            panel_tab_scroll_target: 0.0,
            save_as_input: None,
            save_confirm: None,
            pending_terminal_close: None,
            file_finder: None,
            editor_panel_auto_shown: false,
            dark_mode: true,
            top_inset: if cfg!(target_os = "macos") { TITLEBAR_HEIGHT } else { 0.0 },
            is_fullscreen: false,
            is_occluded: false,
            header_hit_zones: Vec::new(),
            git_switcher: None,
            file_switcher: None,
            hover_target: None,
            context_menu: None,
            focus_area: FocusArea::PaneArea,
            file_tree_cursor: 0,
            file_tree_rename: None,
            branch_cleanup: None,
            config_page: None,
            settings: settings::load_settings(),
            file_tree_git_status: std::collections::HashMap::new(),
            file_tree_dir_git_status: std::collections::HashMap::new(),
            file_tree_git_root: None,
            file_watcher: None,
            file_watch_rx: None,
            file_watch_dirty: Arc::new(AtomicBool::new(false)),
            event_loop_waker: None,
            git_poll_rx: None,
            git_poll_cwd_tx: None,
            git_poll_handle: None,
            git_poll_stop: Arc::new(AtomicBool::new(false)),
            content_view_ptr: None,
            window_ptr: None,
            window_shown: false,
        }
    }

    // ── Terminal-bound editor dock accessors ──

    /// ID of the terminal whose editors are currently shown in the dock.
    /// Priority: focused terminal → terminal owning focused editor → first layout terminal.
    pub(crate) fn focused_terminal_id(&self) -> Option<PaneId> {
        let focused = self.focused?;
        if matches!(self.panes.get(&focused), Some(PaneKind::Terminal(_))) {
            return Some(focused);
        }
        if let Some(owner) = self.terminal_owning(focused) {
            return Some(owner);
        }
        // Fallback: first terminal in layout order
        self.layout.pane_ids().into_iter()
            .find(|&id| matches!(self.panes.get(&id), Some(PaneKind::Terminal(_))))
    }

    /// Reverse lookup: which terminal owns the given editor/diff pane?
    pub(crate) fn terminal_owning(&self, editor_id: PaneId) -> Option<PaneId> {
        for (&id, pane) in &self.panes {
            if let PaneKind::Terminal(tp) = pane {
                if tp.editors.contains(&editor_id) {
                    return Some(id);
                }
            }
        }
        None
    }

    /// The editor tab list visible in the dock (from the focused terminal).
    pub(crate) fn active_editor_tabs(&self) -> &[PaneId] {
        if let Some(tid) = self.focused_terminal_id() {
            if let Some(PaneKind::Terminal(tp)) = self.panes.get(&tid) {
                return &tp.editors;
            }
        }
        &[]
    }

    /// The currently active editor tab in the dock.
    pub(crate) fn active_editor_tab(&self) -> Option<PaneId> {
        let tid = self.focused_terminal_id()?;
        match self.panes.get(&tid) {
            Some(PaneKind::Terminal(tp)) => tp.active_editor,
            _ => None,
        }
    }

    /// Check whether a pane lives in any terminal's editor dock.
    pub(crate) fn is_dock_editor(&self, pane_id: PaneId) -> bool {
        self.terminal_owning(pane_id).is_some()
    }

    // ── Helpers ──

    /// Install an event-loop waker on a terminal pane so the PTY thread
    /// can wake us from sleep when new output arrives.
    fn install_pty_waker(&self, pane: &TerminalPane) {
        if let Some(ref waker) = self.event_loop_waker {
            let w = waker.clone();
            pane.backend.set_waker(Box::new(move || w()));
        }
    }

    fn create_initial_pane(&mut self) {
        let (layout, pane_id) = SplitLayout::with_initial_pane();
        self.layout = layout;

        let cell_size = match self.renderer.as_ref() {
            Some(r) => r.cell_size(),
            None => {
                log::error!("create_initial_pane called before renderer initialized");
                return;
            }
        };
        let logical_w = self.window_size.0 as f32 / self.scale_factor;
        let logical_h = self.window_size.1 as f32 / self.scale_factor;

        let cols = (logical_w / cell_size.width).max(1.0) as u16;
        let rows = (logical_h / cell_size.height).max(1.0) as u16;

        match TerminalPane::new(pane_id, cols, rows) {
            Ok(pane) => {
                self.install_pty_waker(&pane);
                self.panes.insert(pane_id, PaneKind::Terminal(pane));
                self.pending_ime_proxy_creates.push(pane_id);
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
            self.window_size.0 as f32 / self.scale_factor,
            self.window_size.1 as f32 / self.scale_factor,
        )
    }
}

// ──────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────

fn main() {
    // Enable backtraces for panic diagnostics
    std::env::set_var("RUST_BACKTRACE", "1");

    // Install a custom panic hook that logs to stderr before the default handler.
    // This ensures we capture the panic message even when catch_unwind absorbs it.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        eprintln!("[tide] PANIC: {info}");
        default_hook(info);
    }));

    env_logger::init();

    let mut app = App::new();
    app.event_loop_waker = Some(tide_platform::macos::MacosApp::create_waker());

    // Initialize keybinding map from saved settings
    if !app.settings.keybindings.is_empty() {
        let map = settings::build_keybinding_map(&app.settings);
        app.router.keybinding_map = Some(map);
    }

    // Try loading a saved session to restore window size
    let saved_session = session::load_session();
    let (win_w, win_h) = saved_session
        .as_ref()
        .map(|s| (s.window_width as f64, s.window_height as f64))
        .unwrap_or((960.0, 640.0));

    let config = tide_platform::WindowConfig {
        title: "Tide".to_string(),
        width: win_w,
        height: win_h,
        min_width: 400.0,
        min_height: 300.0,
        transparent_titlebar: true,
    };

    tide_platform::macos::MacosApp::run(
        config,
        Box::new(move |event, window| {
            app.handle_platform_event(event, window);
        }),
    );
}
