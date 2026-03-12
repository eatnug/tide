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
mod render_thread;
mod rendering;
mod search;
mod session;
mod settings;
mod theme;
mod ui;
mod ui_state;
mod update;
mod workspace;

#[cfg(test)]
mod behavior_tests;

pub(crate) use ui_state::*;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use tide_core::{Modifiers, PaneId, Rect, Size, TerminalBackend};
use tide_input::Router;
use tide_layout::SplitLayout;
use tide_renderer::WgpuRenderer;
use tide_tree::FsTree;

use drag_drop::{HoverTarget, PaneDragState};
use pane::{PaneKind, TerminalPane};
use theme::*;

pub(crate) use workspace::Workspace;

// ──────────────────────────────────────────────
// App state
// ──────────────────────────────────────────────

struct App {
    pub(crate) device: Option<Arc<wgpu::Device>>,
    pub(crate) queue: Option<Arc<wgpu::Queue>>,
    pub(crate) surface_config: Option<wgpu::SurfaceConfiguration>,
    pub(crate) renderer: Option<WgpuRenderer>,

    // Render thread: owns the wgpu::Surface and handles drawable acquisition,
    // GPU command encoding, submission, and presentation on a dedicated thread.
    // This prevents CAMetalLayer.nextDrawable() from blocking the event loop.
    render_thread: Option<render_thread::RenderThreadHandle>,
    /// Pending surface reconfiguration (sent with the next render job).
    pending_surface_config: Option<wgpu::SurfaceConfiguration>,

    // Panes
    pub(crate) panes: HashMap<PaneId, PaneKind>,
    pub(crate) layout: SplitLayout,
    pub(crate) router: Router,
    pub(crate) focused: Option<PaneId>,

    // File tree (grouped state)
    pub(crate) ft: ui_state::FileTreeModel,

    // Sidebar layout side
    pub(crate) sidebar_side: LayoutSide,
    pub(crate) sidebar_handle_dragging: bool,

    // Window state
    pub(crate) scale_factor: f32,
    pub(crate) window_size: (u32, u32),
    /// Cached cell size from the renderer (font-based constant).
    /// Always available after init_gpu(), even when the renderer is
    /// on the render thread.
    pub(crate) cached_cell_size: tide_core::Size,
    /// Current font size — tracked on the App so font size changes work even
    /// when the renderer is on the render thread.
    pub(crate) current_font_size: f32,
    /// Precomputed cell sizes for font sizes 8..=32 (copied from renderer at init).
    pub(crate) cell_size_table: Vec<tide_core::Size>,
    /// Pending font size to apply to the renderer when it returns from the render thread.
    pub(crate) pending_font_size: Option<f32>,
    pub(crate) modifiers: Modifiers,
    pub(crate) last_cursor_pos: tide_core::Vec2,

    // CWD tracking
    pub(crate) last_cwd: Option<PathBuf>,
    /// Deferred badge check scheduled after PTY output settles.
    /// Event-driven: set ~150ms after last PTY burst so CWD/idle badges
    /// update promptly regardless of whether user or AI agent changed dirs.
    badge_check_at: Option<Instant>,

    // Frame pacing (needs_redraw moved to cache)
    pub(crate) last_frame: Instant,
    /// Last time we checked child process liveness (throttled to ~2s).
    pub(crate) last_child_check: Instant,

    /// Deferred PTY resize after window resize settles (debounce).
    /// While Some, compute_layout skips PTY resize to avoid SIGWINCH spam.
    pub(crate) resize_deferred_at: Option<Instant>,

    // IME composition state (grouped)
    pub(crate) ime: ui_state::ImeState,

    // Computed pane rects: tiling rects (hit-testing/drag) and visual rects (gap-inset, rendering)
    pub(crate) pane_rects: Vec<(PaneId, Rect)>,
    pub(crate) visual_pane_rects: Vec<(PaneId, Rect)>,
    pub(crate) prev_visual_pane_rects: Vec<(PaneId, Rect)>,

    // The overall rect available for pane tiling (excluding file tree and editor panel)
    pub(crate) pane_area_rect: Option<Rect>,

    // Render generation tracking (grouped)
    pub(crate) cache: ui_state::RenderCache,

    // Input latency: skip coalescing after keypress while awaiting PTY response
    pub(crate) input_just_sent: bool,
    pub(crate) input_sent_at: Option<Instant>,
    // Scroll latency: skip coalescing during active scroll gestures
    pub(crate) scroll_at: Option<Instant>,

    // Mouse/drag/scroll interaction (grouped)
    pub(crate) interaction: ui_state::InteractionState,

    // Search focus: which pane's search bar has keyboard focus
    pub(crate) search_focus: Option<PaneId>,


    // Modal/popup overlay state (grouped)
    pub(crate) modal: ui_state::ModalStack,

    // Pending terminal close: set when closing a terminal that has dirty editors.
    // After each save-confirm resolution, retries closing the terminal.
    pub(crate) pending_terminal_close: Option<tide_core::PaneId>,

    // Shift+Shift double-tap detection
    pub(crate) last_shift_up: Option<Instant>,
    pub(crate) shift_tap_clean: bool,


    // Theme mode
    pub(crate) dark_mode: bool,

    // Top inset for macOS transparent titlebar (traffic light area)
    pub(crate) top_inset: f32,
    pub(crate) is_fullscreen: bool,
    pub(crate) pending_fullscreen_toggle: bool,
    pub(crate) is_occluded: bool,

    // Header hit zones (for badge click handling)
    pub(crate) header_hit_zones: Vec<header::HeaderHitZone>,

    // FocusArea: which area currently has keyboard focus
    pub(crate) focus_area: FocusArea,

    // Workspace management (grouped)
    pub(crate) ws: ui_state::WorkspaceManager,

    // Loaded settings
    pub(crate) settings: settings::TideSettings,

    // File watcher for external change detection in editor panes
    pub(crate) file_watcher: Option<notify::RecommendedWatcher>,
    pub(crate) file_watch_rx: Option<mpsc::Receiver<notify::Result<notify::Event>>>,
    pub(crate) file_watch_dirty: Arc<AtomicBool>,

    // Waker for poking the event loop from background threads (PTY, file watcher)
    pub(crate) event_loop_waker: Option<tide_platform::WakeCallback>,

    // Background git info poller
    pub(crate) git_poll_rx: Option<mpsc::Receiver<crate::file_tree::GitPollResults>>,
    pub(crate) git_poll_cwd_tx: Option<mpsc::Sender<Vec<PathBuf>>>,
    pub(crate) git_poll_handle: Option<std::thread::JoinHandle<()>>,
    pub(crate) git_poll_stop: Arc<AtomicBool>,
    /// CWD → repo root cache, populated by the git poller (avoids sync git calls)
    pub(crate) cached_repo_roots: std::collections::HashMap<PathBuf, Option<PathBuf>>,

    // Platform pointers for webview management (macOS)
    pub(crate) content_view_ptr: Option<*mut std::ffi::c_void>,
    pub(crate) window_ptr: Option<*mut std::ffi::c_void>,

    // Window visibility: false until first frame renders (avoids blank window flash)
    pub(crate) window_shown: bool,

    // Cursor blink state
    pub(crate) cursor_blink_at: Instant,
    pub(crate) cursor_visible: bool,

    // Event batching: when > 0, suppress rendering until BatchEnd.
    // Used by ImeProxyView to flush deferred IME events atomically.
    pub(crate) batch_depth: u32,

    // GPU backpressure: microseconds spent waiting for drawable in the last render.
    // When high (>4ms), the event loop defers inline rendering to avoid blocking
    // the main thread on CAMetalLayer.nextDrawable() semaphore waits.
    pub(crate) drawable_wait_us: u64,

    // Zoomed pane: when Some, this pane fills the entire pane area (Cmd+Enter toggle)
    pub(crate) zoomed_pane: Option<PaneId>,
}

// Safety: App contains raw pointers (content_view_ptr, window_ptr) and browser
// WebViewHandles that are not inherently Send. These are only used for webview
// management which will be dispatched back to the main thread via WindowCommand.
// All other fields (wgpu resources, channels, atomics) are Send-safe.
unsafe impl Send for App {}

impl App {
    fn new() -> Self {
        Self {
            device: None,
            queue: None,
            surface_config: None,
            renderer: None,
            render_thread: None,
            pending_surface_config: None,
            panes: HashMap::new(),
            layout: SplitLayout::new(),
            router: Router::new(),
            focused: None,
            ft: ui_state::FileTreeModel::new(FILE_TREE_WIDTH),
            sidebar_side: LayoutSide::Left,
            sidebar_handle_dragging: false,
            scale_factor: 1.0,
            window_size: (1200, 800),
            cached_cell_size: tide_core::Size::new(0.0, 0.0),
            current_font_size: 14.0,
            cell_size_table: Vec::new(),
            pending_font_size: None,
            modifiers: Modifiers::default(),
            last_cursor_pos: tide_core::Vec2::new(0.0, 0.0),
            last_cwd: None,
            badge_check_at: None,
            last_frame: Instant::now(),
            last_child_check: Instant::now(),
            resize_deferred_at: None,
            ime: ui_state::ImeState::new(),
            pane_rects: Vec::new(),
            visual_pane_rects: Vec::new(),
            prev_visual_pane_rects: Vec::new(),
            pane_area_rect: None,
            cache: ui_state::RenderCache::new(),
            input_just_sent: false,
            input_sent_at: None,
            scroll_at: None,
            interaction: ui_state::InteractionState::new(),
            search_focus: None,
            modal: ui_state::ModalStack::new(),
            pending_terminal_close: None,
            last_shift_up: None,
            shift_tap_clean: false,
            dark_mode: true,
            top_inset: if cfg!(target_os = "macos") { TITLEBAR_HEIGHT } else { 0.0 },
            is_fullscreen: false,
            pending_fullscreen_toggle: false,
            is_occluded: false,
            header_hit_zones: Vec::new(),
            focus_area: FocusArea::PaneArea,
            ws: ui_state::WorkspaceManager::new(),
            settings: settings::load_settings(),
            file_watcher: None,
            file_watch_rx: None,
            file_watch_dirty: Arc::new(AtomicBool::new(false)),
            event_loop_waker: None,
            git_poll_rx: None,
            git_poll_cwd_tx: None,
            git_poll_handle: None,
            git_poll_stop: Arc::new(AtomicBool::new(false)),
            cached_repo_roots: std::collections::HashMap::new(),
            content_view_ptr: None,
            window_ptr: None,
            window_shown: false,
            cursor_blink_at: Instant::now(),
            cursor_visible: true,
            batch_depth: 0,
            drawable_wait_us: 0,
            zoomed_pane: None,
        }
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

    /// Create the initial terminal pane. If `early_terminal` is provided, reuse it
    /// (pre-spawned before GPU init so the shell loads in parallel). Otherwise
    /// spawn a fresh PTY.
    fn create_initial_pane(&mut self, early_terminal: Option<tide_terminal::Terminal>) {
        let (layout, pane_id) = SplitLayout::with_initial_pane();
        self.layout = layout;

        let cell_size = self.cell_size();
        let logical_w = self.window_size.0 as f32 / self.scale_factor;
        let logical_h = self.window_size.1 as f32 / self.scale_factor;

        let cols = if cell_size.width > 0.0 {
            ((logical_w / cell_size.width).max(1.0).min(1000.0)) as u16
        } else {
            80
        };
        let rows = if cell_size.height > 0.0 {
            ((logical_h / cell_size.height).max(1.0).min(500.0)) as u16
        } else {
            24
        };

        let result = if let Some(mut terminal) = early_terminal {
            // Resize pre-spawned terminal to actual dimensions
            terminal.resize(cols, rows);
            Ok(TerminalPane::with_terminal(pane_id, terminal))
        } else {
            TerminalPane::with_cwd(pane_id, cols, rows, None, self.dark_mode)
        };

        match result {
            Ok(pane) => {
                self.install_pty_waker(&pane);
                self.panes.insert(pane_id, PaneKind::Terminal(pane));
                self.ime.pending_creates.push(pane_id);
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
        self.ft.tree = Some(tree);
        self.last_cwd = Some(cwd);

        // Create the first workspace (placeholder — active data lives on App fields)
        self.ws.workspaces.push(Workspace {
            name: "Workspace 1".to_string(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        self.ws.active = 0;
    }

    pub(crate) fn logical_size(&self) -> Size {
        Size::new(
            self.window_size.0 as f32 / self.scale_factor,
            self.window_size.1 as f32 / self.scale_factor,
        )
    }

    /// Look up the precomputed cell size for a given font size.
    fn lookup_cell_size(&self, font_size: f32) -> tide_core::Size {
        let idx = (font_size.round() as u32).saturating_sub(8) as usize;
        self.cell_size_table.get(idx).copied()
            .unwrap_or(self.cached_cell_size)
    }

    /// Return the cached cell size. Always available after init_gpu(),
    /// even when the renderer is temporarily on the render thread.
    pub(crate) fn cell_size(&self) -> Size {
        self.cached_cell_size
    }

    /// Apply a font size change. Works whether or not the renderer is
    /// currently available (on the render thread).  When the renderer is
    /// away, the change is queued in `pending_font_size` and applied when
    /// the renderer returns via `flush_pending_font_size`.
    pub(crate) fn apply_font_size(&mut self, size: f32) {
        let size = size.clamp(8.0, 32.0);
        if (size - self.current_font_size).abs() < 0.01 {
            return;
        }
        self.current_font_size = size;
        self.cached_cell_size = self.lookup_cell_size(size);

        if let Some(renderer) = &mut self.renderer {
            renderer.set_font_size(size);
        } else {
            self.pending_font_size = Some(size);
        }

        self.cache.pane_generations.clear();
        self.cache.invalidate_chrome();
        self.cache.layout_generation = self.cache.layout_generation.wrapping_add(1);
        self.compute_layout();
    }

    /// Apply any queued font size change to the renderer after it returns
    /// from the render thread.
    pub(crate) fn flush_pending_font_size(&mut self) {
        if let Some(size) = self.pending_font_size.take() {
            if let Some(renderer) = &mut self.renderer {
                renderer.set_font_size(size);
            }
        }
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

    // ── Channels ──────────────────────────────────────────────────────
    // event channel: main thread → app thread (platform events + wake signals)
    // command channel: app thread → main thread (window mutations)
    let (event_tx, event_rx) = std::sync::mpsc::channel::<event_loop::AppEvent>();
    let (cmd_tx, cmd_rx) = std::sync::mpsc::channel::<tide_platform::WindowCommand>();

    // ── Wakers ────────────────────────────────────────────────────────
    // Main thread waker: posts NSEvent + triggerRedraw to wake the main run loop
    // and cause the callback to fire (which drains window commands).
    let main_waker = tide_platform::macos::MacosApp::create_waker();

    // Combined waker for background threads (PTY, file watcher, render thread):
    // wakes both the app thread (via event channel) and the main thread (via NSEvent).
    let waker_tx = std::sync::Arc::new(std::sync::Mutex::new(event_tx.clone()));
    let combined_waker: tide_platform::WakeCallback = std::sync::Arc::new({
        let main_waker = main_waker.clone();
        let waker_tx = waker_tx.clone();
        move || {
            let _ = waker_tx.lock().unwrap().send(event_loop::AppEvent::Wake);
            main_waker();
        }
    });

    // ── WindowProxy ──────────────────────────────────────────────────
    // App thread uses this to send commands back to the main thread.
    let window_proxy = tide_platform::WindowProxy::new(cmd_tx, main_waker.clone());

    // ── App setup ────────────────────────────────────────────────────
    let mut app = App::new();
    app.event_loop_waker = Some(combined_waker);

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

    // ── Phase 1 handoff state ────────────────────────────────────────
    // Shared between the main thread callback and Phase 1 initialization.
    // After Phase 1, the App + event_rx + proxy are moved to the app thread.
    let init_state = std::sync::Arc::new(std::sync::Mutex::new(Some((
        app,
        event_rx,
        window_proxy.clone(),
    ))));
    let init_state_cb = init_state.clone();
    let initialized = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let initialized_cb = initialized.clone();

    // ── Run the macOS event loop ─────────────────────────────────────
    // Phase 1: first event triggers GPU init on main thread, then spawns app thread.
    // Phase 2: all subsequent events are forwarded to the app thread.
    tide_platform::macos::MacosApp::run(
        config,
        Box::new(move |event, window| {
            // Phase 1: one-time initialization (main thread)
            if !initialized_cb.load(std::sync::atomic::Ordering::Acquire) {
                if let Some((mut app, rx, proxy)) = init_state_cb.lock().unwrap().take() {
                    // GPU init, session restore, pane creation (needs real window)
                    app.init_phase1(window);

                    // Sync IME proxies using WindowProxy (commands go to cmd_tx)
                    app.sync_ime_proxies(&proxy);
                    app.compute_layout();

                    // Drain any window commands generated during init
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        tide_platform::execute_window_command(window, cmd);
                    }

                    // Spawn the app thread
                    std::thread::Builder::new()
                        .name("app-thread".into())
                        .spawn(move || {
                            app.app_thread_run(rx, proxy);
                        })
                        .expect("failed to spawn app thread");

                    initialized_cb.store(true, std::sync::atomic::Ordering::Release);
                }
                return;
            }

            // Phase 2: drain commands FIRST so IME proxy focus etc. execute
            // before macOS dispatches the next event to first responder.
            while let Ok(cmd) = cmd_rx.try_recv() {
                tide_platform::execute_window_command(window, cmd);
            }
            // Forward event to app thread
            if !matches!(event, tide_platform::PlatformEvent::RedrawRequested) {
                let _ = event_tx.send(event_loop::AppEvent::Platform(event));
            }
        }),
    );
}
