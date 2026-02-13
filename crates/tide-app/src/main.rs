// Tide v0.1 — Integration (Step 3)
// Wires all crates together: winit window, wgpu surface, renderer, terminal panes,
// layout engine, input router, file tree, and CWD following.

mod action;
mod drag_drop;
mod editor_pane;
mod event_handler;
mod file_tree;
mod gpu;
mod input;
mod pane;
mod rendering;
mod theme;
mod ui;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, MouseButton as WinitMouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId};

use tide_core::{LayoutEngine, PaneId, Rect, Renderer, Size, TerminalBackend};
use tide_input::Router;
use tide_layout::SplitLayout;
use tide_renderer::WgpuRenderer;
use tide_tree::FsTree;

use drag_drop::PaneDragState;
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
    pub(crate) ime_composing: bool,
    pub(crate) ime_preedit: String,

    // Computed pane rects: tiling rects (hit-testing/drag) and visual rects (gap-inset, rendering)
    pub(crate) pane_rects: Vec<(PaneId, Rect)>,
    pub(crate) visual_pane_rects: Vec<(PaneId, Rect)>,

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

    // Editor panel (right-side tab panel)
    pub(crate) editor_panel_tabs: Vec<tide_core::PaneId>,
    pub(crate) editor_panel_active: Option<tide_core::PaneId>,
    pub(crate) editor_panel_rect: Option<Rect>,
    pub(crate) editor_panel_width: f32,
    pub(crate) panel_border_dragging: bool,
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
            scale_factor: 1.0,
            window_size: PhysicalSize::new(1200, 800),
            modifiers: ModifiersState::empty(),
            last_cursor_pos: tide_core::Vec2::new(0.0, 0.0),
            last_cwd: None,
            last_cwd_check: Instant::now(),
            needs_redraw: true,
            last_frame: Instant::now(),
            ime_composing: false,
            ime_preedit: String::new(),
            pane_rects: Vec::new(),
            visual_pane_rects: Vec::new(),
            pane_generations: HashMap::new(),
            layout_generation: 0,
            chrome_generation: 0,
            last_chrome_generation: u64::MAX,
            input_just_sent: false,
            input_sent_at: None,
            consecutive_dirty_frames: 0,
            pane_drag: PaneDragState::Idle,
            editor_panel_tabs: Vec::new(),
            editor_panel_active: None,
            editor_panel_rect: None,
            editor_panel_width: EDITOR_PANEL_WIDTH,
            panel_border_dragging: false,
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

        let show_editor_panel = !self.editor_panel_tabs.is_empty();

        // Reserve space for file tree (left) and editor panel (right)
        let left_reserved = if self.show_file_tree { FILE_TREE_WIDTH } else { 0.0 };
        let right_reserved = if show_editor_panel { self.editor_panel_width } else { 0.0 };

        let terminal_area = Size::new(
            (logical.width - left_reserved - right_reserved).max(100.0),
            logical.height,
        );

        let terminal_offset_x = left_reserved;

        // Compute editor panel rect
        if show_editor_panel {
            let panel_x = terminal_offset_x + terminal_area.width;
            self.editor_panel_rect = Some(Rect::new(
                panel_x + PANE_GAP / 2.0,
                PANE_GAP,
                self.editor_panel_width - PANE_GAP / 2.0 - PANE_GAP,
                logical.height - PANE_GAP * 2.0,
            ));
        } else {
            self.editor_panel_rect = None;
        }

        let mut rects = self.layout.compute(terminal_area, &pane_ids, self.focused);

        // Offset rects to account for file tree panel
        for (_, rect) in &mut rects {
            rect.x += terminal_offset_x;
        }

        // Resize terminal backends to match their rects (minus tab bar height)
        // During border drag, skip PTY resize to avoid SIGWINCH spam
        // (shell redraws prompt on every resize, flooding the terminal)
        let is_dragging = self.router.is_dragging_border();
        if !is_dragging {
            if let Some(renderer) = &self.renderer {
                let cell_size = renderer.cell_size();
                for &(id, rect) in &rects {
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&id) {
                        let content_rect = Rect::new(
                            rect.x,
                            rect.y,
                            rect.width,
                            (rect.height - TAB_BAR_HEIGHT).max(cell_size.height),
                        );
                        pane.resize_to_rect(content_rect, cell_size);
                    }
                }
            }
        }

        // Force grid rebuild if rects changed
        let rects_changed = rects != self.pane_rects;
        self.pane_rects = rects;

        // Compute visual rects with gap insets for rendering
        let logical = self.logical_size();
        let right_edge = terminal_offset_x + terminal_area.width;
        self.visual_pane_rects = self
            .pane_rects
            .iter()
            .map(|&(id, r)| {
                // For each edge: if at window boundary → PANE_GAP, else → PANE_GAP/2
                let half = PANE_GAP / 2.0;
                let left = if r.x <= terminal_offset_x + 0.5 { PANE_GAP } else { half };
                let top = if r.y <= 0.5 { PANE_GAP } else { half };
                let right = if r.x + r.width >= right_edge - 0.5 {
                    PANE_GAP
                } else {
                    half
                };
                let bottom = if r.y + r.height >= logical.height - 0.5 {
                    PANE_GAP
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

        if rects_changed {
            self.layout_generation += 1;
            self.pane_generations.clear();
            self.chrome_generation += 1;
        }

        // Store window size for layout drag operations
        self.layout.last_window_size = Some(terminal_area);
    }

    fn update(&mut self) {
        // Process PTY output for terminal panes only
        for pane in self.panes.values_mut() {
            if let PaneKind::Terminal(terminal) = pane {
                terminal.backend.process();
            }
        }

        // Poll file tree events
        if let Some(tree) = self.file_tree.as_mut() {
            let had_changes = tree.poll_events();
            if had_changes {
                self.chrome_generation += 1;
            }
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

        let attrs = WindowAttributes::default()
            .with_title("Tide")
            .with_inner_size(LogicalSize::new(1200.0, 800.0))
            .with_min_inner_size(LogicalSize::new(400.0, 300.0));

        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        window.set_ime_allowed(true);

        self.window = Some(window);
        self.init_gpu();
        self.create_initial_pane();
        self.compute_layout();
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
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
                if panel_rect.contains(self.last_cursor_pos) {
                    // Tab close button → handle here
                    if let Some(tab_id) = self.panel_tab_close_at(self.last_cursor_pos) {
                        self.close_editor_panel_tab(tab_id);
                        self.needs_redraw = true;
                        return;
                    }
                    // Tab click → let flow to handle_window_event for drag initiation
                    if self.panel_tab_at(self.last_cursor_pos).is_some() {
                        // fall through
                    } else {
                        // Content area click → focus + cursor
                        self.handle_editor_panel_click(self.last_cursor_pos);
                        self.needs_redraw = true;
                        return;
                    }
                }
            }
        }

        // Handle file tree clicks before general routing
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.show_file_tree && self.last_cursor_pos.x < FILE_TREE_WIDTH {
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

        if self.needs_redraw {
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
            // Adaptive frame pacing: 16ms (~60fps) during high throughput, 8ms otherwise
            let wait_ms = if self.consecutive_dirty_frames > 10 { 16 } else { 8 };
            self.consecutive_dirty_frames = 0;
            event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(wait_ms)));
        }
    }
}

// ──────────────────────────────────────────────
// Entry point
// ──────────────────────────────────────────────

fn main() {
    env_logger::init();

    let event_loop = EventLoop::new().expect("create event loop");
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::new();
    event_loop.run_app(&mut app).expect("run event loop");
}
