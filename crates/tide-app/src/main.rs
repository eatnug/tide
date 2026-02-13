// Tide v0.1 — Integration (Step 3)
// Wires all crates together: winit window, wgpu surface, renderer, terminal panes,
// layout engine, input router, file tree, and CWD following.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key as WinitKey, ModifiersState, NamedKey};
use winit::window::{Window, WindowAttributes, WindowId};

use tide_core::{
    Color, CursorShape, FileTreeSource, InputEvent, Key, LayoutEngine, Modifiers, MouseButton,
    PaneId, Rect, Renderer, Size, SplitDirection, TerminalBackend, TextStyle, Vec2,
};
use tide_input::{Action, Direction, GlobalAction, Router};
use tide_layout::SplitLayout;
use tide_renderer::WgpuRenderer;
use tide_terminal::Terminal;
use tide_tree::FsTree;

// ──────────────────────────────────────────────
// Theme constants
// ──────────────────────────────────────────────

const BG_COLOR: Color = Color::new(0.06, 0.065, 0.09, 1.0);
const BORDER_COLOR: Color = Color::new(0.15, 0.16, 0.22, 1.0);
const FOCUSED_BORDER_COLOR: Color = Color::new(0.25, 0.5, 1.0, 1.0);
const TREE_BG_COLOR: Color = Color::new(0.05, 0.055, 0.08, 1.0);
const TREE_TEXT_COLOR: Color = Color::new(0.72, 0.74, 0.82, 1.0);
const TREE_DIR_COLOR: Color = Color::new(0.35, 0.6, 1.0, 1.0);
const BORDER_WIDTH: f32 = 1.0;
const FILE_TREE_WIDTH: f32 = 220.0;

// ──────────────────────────────────────────────
// TerminalPane
// ──────────────────────────────────────────────

struct TerminalPane {
    #[allow(dead_code)]
    id: PaneId,
    backend: Terminal,
}

impl TerminalPane {
    fn new(id: PaneId, cols: u16, rows: u16) -> Result<Self, Box<dyn std::error::Error>> {
        let backend = Terminal::new(cols, rows)?;
        Ok(Self { id, backend })
    }

    /// Render the grid cells into the cached grid layer.
    fn render_grid(&self, rect: Rect, renderer: &mut WgpuRenderer) {
        let cell_size = renderer.cell_size();

        // Draw pane background into grid layer (so cell BGs draw on top)
        renderer.draw_grid_rect(rect, BG_COLOR);

        let grid = self.backend.grid();
        let offset = Vec2::new(rect.x, rect.y);

        // Clamp to the number of rows/cols that fit within the pane rect
        let max_rows = (rect.height / cell_size.height).ceil() as usize;
        let max_cols = (rect.width / cell_size.width).ceil() as usize;
        let rows = (grid.rows as usize).min(max_rows).min(grid.cells.len());
        let cols = (grid.cols as usize).min(max_cols);

        for row in 0..rows {
            for col in 0..cols {
                if col >= grid.cells[row].len() {
                    break;
                }
                let cell = &grid.cells[row][col];
                if cell.character == '\0'
                    || (cell.character == ' ' && cell.style.background.is_none())
                {
                    continue;
                }
                renderer.draw_grid_cell(cell.character, row, col, cell.style, cell_size, offset);
            }
        }
    }

    /// Render the cursor into the overlay layer (always redrawn).
    fn render_cursor(&self, rect: Rect, renderer: &mut WgpuRenderer) {
        let cell_size = renderer.cell_size();
        let cursor = self.backend.cursor();
        if cursor.visible {
            let cx = rect.x + cursor.col as f32 * cell_size.width;
            let cy = rect.y + cursor.row as f32 * cell_size.height;

            let cursor_color = Color::new(0.25, 0.5, 1.0, 0.9);
            match cursor.shape {
                CursorShape::Block => {
                    renderer.draw_rect(
                        Rect::new(cx, cy, cell_size.width, cell_size.height),
                        cursor_color,
                    );
                }
                CursorShape::Beam => {
                    renderer.draw_rect(Rect::new(cx, cy, 2.0, cell_size.height), cursor_color);
                }
                CursorShape::Underline => {
                    renderer.draw_rect(
                        Rect::new(cx, cy + cell_size.height - 2.0, cell_size.width, 2.0),
                        cursor_color,
                    );
                }
            }
        }
    }

    fn handle_key(&mut self, key: &Key, modifiers: &Modifiers) {
        let bytes = Terminal::key_to_bytes(key, modifiers);
        if !bytes.is_empty() {
            self.backend.write(&bytes);
        }
    }

    fn resize_to_rect(&mut self, rect: Rect, cell_size: Size) {
        let cols = (rect.width / cell_size.width).max(1.0) as u16;
        let rows = (rect.height / cell_size.height).max(1.0) as u16;
        self.backend.resize(cols, rows);
    }
}

// ──────────────────────────────────────────────
// App state
// ──────────────────────────────────────────────

struct App {
    window: Option<Arc<Window>>,
    surface: Option<wgpu::Surface<'static>>,
    device: Option<Arc<wgpu::Device>>,
    queue: Option<Arc<wgpu::Queue>>,
    surface_config: Option<wgpu::SurfaceConfiguration>,
    renderer: Option<WgpuRenderer>,

    // Panes
    terminal_panes: HashMap<PaneId, TerminalPane>,
    layout: SplitLayout,
    router: Router,
    focused: Option<PaneId>,

    // File tree
    file_tree: Option<FsTree>,
    show_file_tree: bool,
    file_tree_scroll: f32,

    // Window state
    scale_factor: f32,
    window_size: PhysicalSize<u32>,
    modifiers: ModifiersState,
    last_cursor_pos: Vec2,

    // CWD tracking
    last_cwd: Option<PathBuf>,
    last_cwd_check: Instant,

    // Frame pacing
    needs_redraw: bool,
    last_frame: Instant,

    // IME composition state
    ime_composing: bool,
    ime_preedit: String,

    // Computed pane rects (cached after layout computation)
    pane_rects: Vec<(PaneId, Rect)>,

    // Grid generation tracking for vertex caching
    pane_generations: HashMap<PaneId, u64>,
    layout_generation: u64,
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
            terminal_panes: HashMap::new(),
            layout: SplitLayout::new(),
            router: Router::new(),
            focused: None,
            file_tree: None,
            show_file_tree: false,
            file_tree_scroll: 0.0,
            scale_factor: 1.0,
            window_size: PhysicalSize::new(1200, 800),
            modifiers: ModifiersState::empty(),
            last_cursor_pos: Vec2::new(0.0, 0.0),
            last_cwd: None,
            last_cwd_check: Instant::now(),
            needs_redraw: true,
            last_frame: Instant::now(),
            ime_composing: false,
            ime_preedit: String::new(),
            pane_rects: Vec::new(),
            pane_generations: HashMap::new(),
            layout_generation: 0,
        }
    }

    fn init_gpu(&mut self) {
        let window = self.window.as_ref().unwrap().clone();
        self.scale_factor = window.scale_factor() as f32;
        self.window_size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });

        let surface = instance.create_surface(window).expect("create surface");

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .expect("no suitable GPU adapter found");

        let (device, queue) = pollster::block_on(adapter.request_device(
            &wgpu::DeviceDescriptor {
                label: Some("tide_device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                memory_hints: Default::default(),
            },
            None,
        ))
        .expect("failed to create device");

        let device = Arc::new(device);
        let queue = Arc::new(queue);

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| !f.is_srgb())
            .copied()
            .unwrap_or(caps.formats[0]);

        // Prefer Mailbox (low latency, no tearing) > Fifo (vsync fallback)
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else {
            wgpu::PresentMode::Fifo
        };

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: self.window_size.width,
            height: self.window_size.height,
            present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let renderer = WgpuRenderer::new(
            Arc::clone(&device),
            Arc::clone(&queue),
            format,
            self.scale_factor,
        );

        self.surface = Some(surface);
        self.device = Some(device);
        self.queue = Some(queue);
        self.surface_config = Some(config);
        self.renderer = Some(renderer);
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
                self.terminal_panes.insert(pane_id, pane);
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

        // Reserve space for file tree if visible
        let terminal_area = if self.show_file_tree {
            Size::new(
                (logical.width - FILE_TREE_WIDTH).max(100.0),
                logical.height,
            )
        } else {
            logical
        };

        let terminal_offset_x = if self.show_file_tree {
            FILE_TREE_WIDTH
        } else {
            0.0
        };

        let mut rects = self.layout.compute(terminal_area, &pane_ids, self.focused);

        // Offset rects to account for file tree panel
        for (_, rect) in &mut rects {
            rect.x += terminal_offset_x;
        }

        // Resize terminal backends to match their rects
        // During border drag, skip PTY resize to avoid SIGWINCH spam
        // (shell redraws prompt on every resize, flooding the terminal)
        let is_dragging = self.router.is_dragging_border();
        if !is_dragging {
            if let Some(renderer) = &self.renderer {
                let cell_size = renderer.cell_size();
                for &(id, rect) in &rects {
                    if let Some(pane) = self.terminal_panes.get_mut(&id) {
                        pane.resize_to_rect(rect, cell_size);
                    }
                }
            }
        }

        // Force grid rebuild if rects changed
        let rects_changed = rects != self.pane_rects;
        self.pane_rects = rects;

        if rects_changed {
            self.layout_generation += 1;
            self.pane_generations.clear();
        }

        // Store window size for layout drag operations
        self.layout.last_window_size = Some(terminal_area);
    }

    fn render(&mut self) {
        let surface = match self.surface.as_ref() {
            Some(s) => s,
            None => return,
        };

        let output = match surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.reconfigure_surface();
                return;
            }
            Err(e) => {
                log::error!("Surface error: {}", e);
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let logical = self.logical_size();
        let focused = self.focused;
        let show_file_tree = self.show_file_tree;
        let file_tree_scroll = self.file_tree_scroll;
        let pane_rects = self.pane_rects.clone();

        let renderer = self.renderer.as_mut().unwrap();

        renderer.begin_frame(logical);

        // Draw file tree panel if visible
        if show_file_tree {
            if let Some(tree) = self.file_tree.as_ref() {
                let panel_rect = Rect::new(0.0, 0.0, FILE_TREE_WIDTH, logical.height);

                renderer.draw_rect(panel_rect, TREE_BG_COLOR);
                renderer.draw_rect(
                    Rect::new(FILE_TREE_WIDTH - BORDER_WIDTH, 0.0, BORDER_WIDTH, logical.height),
                    BORDER_COLOR,
                );

                let cell_size = renderer.cell_size();
                let line_height = cell_size.height;
                let indent_width = cell_size.width * 1.5;
                let left_padding = 4.0;

                let entries = tree.visible_entries();
                for (i, entry) in entries.iter().enumerate() {
                    let y = i as f32 * line_height - file_tree_scroll;
                    if y + line_height < 0.0 || y > logical.height {
                        continue;
                    }

                    let x = left_padding + entry.depth as f32 * indent_width;

                    let prefix = if entry.entry.is_dir {
                        if entry.is_expanded { "v " } else { "> " }
                    } else {
                        "  "
                    };

                    let text_color = if entry.entry.is_dir {
                        TREE_DIR_COLOR
                    } else {
                        TREE_TEXT_COLOR
                    };

                    let style = TextStyle {
                        foreground: text_color,
                        background: None,
                        bold: entry.entry.is_dir,
                        italic: false,
                        underline: false,
                    };

                    let display_text = format!("{}{}", prefix, entry.entry.name);
                    renderer.draw_text(&display_text, Vec2::new(x, y), style, panel_rect);
                }
            }
        }

        // Draw pane borders and backgrounds
        for &(id, rect) in &pane_rects {
            let is_focused = focused == Some(id);
            let border_color = if is_focused {
                FOCUSED_BORDER_COLOR
            } else {
                BORDER_COLOR
            };

            renderer.draw_rect(
                Rect::new(rect.x, rect.y, rect.width, BORDER_WIDTH),
                border_color,
            );
            renderer.draw_rect(
                Rect::new(
                    rect.x,
                    rect.y + rect.height - BORDER_WIDTH,
                    rect.width,
                    BORDER_WIDTH,
                ),
                border_color,
            );
            renderer.draw_rect(
                Rect::new(rect.x, rect.y, BORDER_WIDTH, rect.height),
                border_color,
            );
            renderer.draw_rect(
                Rect::new(
                    rect.x + rect.width - BORDER_WIDTH,
                    rect.y,
                    BORDER_WIDTH,
                    rect.height,
                ),
                border_color,
            );
        }

        // Check if grid needs rebuild (any pane content or layout changed)
        let mut grid_dirty = false;
        for &(id, _) in &pane_rects {
            if let Some(pane) = self.terminal_panes.get(&id) {
                let gen = pane.backend.grid_generation();
                let prev = self.pane_generations.get(&id).copied().unwrap_or(u64::MAX);
                if gen != prev {
                    grid_dirty = true;
                    break;
                }
            }
        }

        // Rebuild grid layer only when content or layout changed
        if grid_dirty {
            renderer.invalidate_grid();
            for &(id, rect) in &pane_rects {
                if let Some(pane) = self.terminal_panes.get(&id) {
                    let inner = Rect::new(
                        rect.x + BORDER_WIDTH,
                        rect.y + BORDER_WIDTH,
                        rect.width - 2.0 * BORDER_WIDTH,
                        rect.height - 2.0 * BORDER_WIDTH,
                    );
                    pane.render_grid(inner, renderer);
                    self.pane_generations.insert(id, pane.backend.grid_generation());
                }
            }
        }

        // Always render cursor (overlay layer) — cursor blinks/moves independently
        for &(id, rect) in &pane_rects {
            if let Some(pane) = self.terminal_panes.get(&id) {
                let inner = Rect::new(
                    rect.x + BORDER_WIDTH,
                    rect.y + BORDER_WIDTH,
                    rect.width - 2.0 * BORDER_WIDTH,
                    rect.height - 2.0 * BORDER_WIDTH,
                );
                pane.render_cursor(inner, renderer);
            }
        }

        // Render IME preedit overlay (Korean composition in progress)
        if !self.ime_preedit.is_empty() {
            if let Some(focused_id) = focused {
                if let Some((_, rect)) = pane_rects.iter().find(|(id, _)| *id == focused_id) {
                    if let Some(pane) = self.terminal_panes.get(&focused_id) {
                        let cursor = pane.backend.cursor();
                        let cell_size = renderer.cell_size();
                        let inner_offset = Vec2::new(
                            rect.x + BORDER_WIDTH,
                            rect.y + BORDER_WIDTH,
                        );
                        let cx = inner_offset.x + cursor.col as f32 * cell_size.width;
                        let cy = inner_offset.y + cursor.row as f32 * cell_size.height;

                        // Draw preedit background
                        let preedit_chars: Vec<char> = self.ime_preedit.chars().collect();
                        let pw = preedit_chars.len().max(1) as f32 * cell_size.width;
                        let preedit_bg = Color::new(0.18, 0.22, 0.38, 1.0);
                        renderer.draw_rect(
                            Rect::new(cx, cy, pw, cell_size.height),
                            preedit_bg,
                        );

                        // Draw each preedit character
                        let preedit_style = TextStyle {
                            foreground: Color::new(0.95, 0.96, 1.0, 1.0),
                            background: None,
                            bold: false,
                            italic: false,
                            underline: true,
                        };
                        for (i, &ch) in preedit_chars.iter().enumerate() {
                            renderer.draw_cell(
                                ch,
                                cursor.row as usize,
                                cursor.col as usize + i,
                                preedit_style,
                                cell_size,
                                inner_offset,
                            );
                        }
                    }
                }
            }
        }

        renderer.end_frame();

        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render_encoder"),
        });

        renderer.render_frame(&mut encoder, &view);

        queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                std::process::exit(0);
            }
            WindowEvent::Resized(new_size) => {
                self.window_size = new_size;
                self.reconfigure_surface();
                self.compute_layout();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor as f32;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Commit(text) => {
                    // IME composed text (Korean, CJK, etc.) → write directly to terminal
                    if let Some(focused_id) = self.focused {
                        if let Some(pane) = self.terminal_panes.get_mut(&focused_id) {
                            pane.backend.write(text.as_bytes());
                        }
                    }
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                }
                Ime::Preedit(text, _) => {
                    self.ime_composing = !text.is_empty();
                    self.ime_preedit = text;
                }
                _ => {}
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                // During IME composition, only handle non-character keys
                if self.ime_composing {
                    if matches!(event.logical_key, WinitKey::Character(_)) {
                        return;
                    }
                }

                if let Some(key) = winit_key_to_tide(&event.logical_key) {
                    let modifiers = winit_modifiers_to_tide(self.modifiers);
                    let input = InputEvent::KeyPress { key, modifiers };

                    let action = self.router.process(input, &self.pane_rects);
                    self.handle_action(action, Some(input));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state != ElementState::Pressed {
                    let was_dragging = self.router.is_dragging_border();
                    // End drag on mouse release
                    self.layout.end_drag();
                    self.router.end_drag();
                    // Apply final PTY resize now that drag is over
                    if was_dragging {
                        self.compute_layout();
                    }
                    return;
                }

                let btn = match button {
                    WinitMouseButton::Left => MouseButton::Left,
                    WinitMouseButton::Right => MouseButton::Right,
                    WinitMouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };

                let input = InputEvent::MouseClick {
                    position: self.last_cursor_pos,
                    button: btn,
                };

                let action = self.router.process(input, &self.pane_rects);
                self.handle_action(action, Some(input));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pos = Vec2::new(
                    position.x as f32 / self.scale_factor,
                    position.y as f32 / self.scale_factor,
                );
                self.last_cursor_pos = pos;

                if self.router.is_dragging_border() {
                    // Adjust position for file tree offset
                    let drag_pos = if self.show_file_tree {
                        Vec2::new(pos.x - FILE_TREE_WIDTH, pos.y)
                    } else {
                        pos
                    };
                    self.layout.drag_border(drag_pos);
                    self.compute_layout();
                } else {
                    let input = InputEvent::MouseMove { position: pos };
                    let _ = self.router.process(input, &self.pane_rects);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 3.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 10.0,
                };

                // Check if scrolling over the file tree
                if self.show_file_tree && self.last_cursor_pos.x < FILE_TREE_WIDTH {
                    self.file_tree_scroll = (self.file_tree_scroll - dy * 10.0).max(0.0);
                } else {
                    let input = InputEvent::MouseScroll {
                        delta: dy,
                        position: self.last_cursor_pos,
                    };
                    let action = self.router.process(input, &self.pane_rects);
                    self.handle_action(action, Some(input));
                }
            }
            WindowEvent::RedrawRequested => {
                self.update();
                self.render();
                self.needs_redraw = false;
                self.last_frame = Instant::now();
            }
            _ => {}
        }
    }

    fn handle_action(&mut self, action: Action, event: Option<InputEvent>) {
        match action {
            Action::RouteToPane(id) => {
                // Update focus
                if let Some(InputEvent::MouseClick { .. }) = event {
                    self.focused = Some(id);
                    self.router.set_focused(id);
                    self.update_file_tree_cwd();
                }

                // Forward keyboard input to terminal
                if let Some(InputEvent::KeyPress { key, modifiers }) = event {
                    if let Some(pane) = self.terminal_panes.get_mut(&id) {
                        pane.handle_key(&key, &modifiers);
                    }
                }
            }
            Action::GlobalAction(global) => {
                self.handle_global_action(global);
            }
            Action::DragBorder(pos) => {
                let drag_pos = if self.show_file_tree {
                    Vec2::new(pos.x - FILE_TREE_WIDTH, pos.y)
                } else {
                    pos
                };
                let terminal_area = if self.show_file_tree {
                    Size::new(
                        (self.logical_size().width - FILE_TREE_WIDTH).max(100.0),
                        self.logical_size().height,
                    )
                } else {
                    self.logical_size()
                };
                self.layout.begin_drag(drag_pos, terminal_area);
                self.layout.drag_border(drag_pos);
                self.compute_layout();
            }
            Action::None => {}
        }
    }

    fn handle_global_action(&mut self, action: GlobalAction) {
        match action {
            GlobalAction::SplitVertical => {
                if let Some(focused) = self.focused {
                    let new_id = self.layout.split(focused, SplitDirection::Vertical);
                    self.create_terminal_pane(new_id);
                    self.compute_layout();
                }
            }
            GlobalAction::SplitHorizontal => {
                if let Some(focused) = self.focused {
                    let new_id = self.layout.split(focused, SplitDirection::Horizontal);
                    self.create_terminal_pane(new_id);
                    self.compute_layout();
                }
            }
            GlobalAction::ClosePane => {
                if let Some(focused) = self.focused {
                    let remaining = self.layout.pane_ids();
                    if remaining.len() <= 1 {
                        // Don't close the last pane — exit the app instead
                        std::process::exit(0);
                    }

                    self.layout.remove(focused);
                    self.terminal_panes.remove(&focused);

                    // Focus the first remaining pane
                    let remaining = self.layout.pane_ids();
                    if let Some(&next) = remaining.first() {
                        self.focused = Some(next);
                        self.router.set_focused(next);
                    } else {
                        self.focused = None;
                    }

                    self.compute_layout();
                    self.update_file_tree_cwd();
                }
            }
            GlobalAction::ToggleFileTree => {
                self.show_file_tree = !self.show_file_tree;
                self.compute_layout();
                if self.show_file_tree {
                    self.update_file_tree_cwd();
                }
            }
            GlobalAction::MoveFocus(direction) => {
                if self.pane_rects.len() < 2 {
                    return;
                }
                let current_id = match self.focused {
                    Some(id) => id,
                    None => return,
                };
                let current_rect = match self.pane_rects.iter().find(|(id, _)| *id == current_id) {
                    Some((_, r)) => *r,
                    None => return,
                };
                let cx = current_rect.x + current_rect.width / 2.0;
                let cy = current_rect.y + current_rect.height / 2.0;

                // Find the closest pane in the given direction.
                // For Left/Right: prefer panes that vertically overlap, rank by horizontal distance.
                // For Up/Down: prefer panes that horizontally overlap, rank by vertical distance.
                let mut best: Option<(PaneId, f32)> = None;
                for &(id, rect) in &self.pane_rects {
                    if id == current_id {
                        continue;
                    }
                    let ox = rect.x + rect.width / 2.0;
                    let oy = rect.y + rect.height / 2.0;
                    let dx = ox - cx;
                    let dy = oy - cy;

                    let (valid, overlaps, dist) = match direction {
                        Direction::Left => (
                            dx < -1.0,
                            rect.y < current_rect.y + current_rect.height && rect.y + rect.height > current_rect.y,
                            dx.abs(),
                        ),
                        Direction::Right => (
                            dx > 1.0,
                            rect.y < current_rect.y + current_rect.height && rect.y + rect.height > current_rect.y,
                            dx.abs(),
                        ),
                        Direction::Up => (
                            dy < -1.0,
                            rect.x < current_rect.x + current_rect.width && rect.x + rect.width > current_rect.x,
                            dy.abs(),
                        ),
                        Direction::Down => (
                            dy > 1.0,
                            rect.x < current_rect.x + current_rect.width && rect.x + rect.width > current_rect.x,
                            dy.abs(),
                        ),
                    };

                    if !valid {
                        continue;
                    }

                    // Prefer overlapping panes; among those, pick the closest on the primary axis
                    let score = if overlaps { dist } else { dist + 100000.0 };
                    if best.map_or(true, |(_, d)| score < d) {
                        best = Some((id, score));
                    }
                }

                if let Some((next_id, _)) = best {
                    self.focused = Some(next_id);
                    self.router.set_focused(next_id);
                    self.update_file_tree_cwd();
                }
            }
        }
    }

    fn create_terminal_pane(&mut self, id: PaneId) {
        let cell_size = self.renderer.as_ref().unwrap().cell_size();
        let logical = self.logical_size();
        let cols = (logical.width / 2.0 / cell_size.width).max(1.0) as u16;
        let rows = (logical.height / cell_size.height).max(1.0) as u16;

        match TerminalPane::new(id, cols, rows) {
            Ok(pane) => {
                self.terminal_panes.insert(id, pane);
            }
            Err(e) => {
                log::error!("Failed to create terminal pane: {}", e);
            }
        }
    }

    fn update(&mut self) {
        // Process PTY output for all terminals
        for pane in self.terminal_panes.values_mut() {
            pane.backend.process();
        }

        // Poll file tree events
        if let Some(tree) = self.file_tree.as_mut() {
            tree.poll_events();
        }

        // Periodic CWD check (every 500ms)
        if self.last_cwd_check.elapsed() > Duration::from_millis(500) {
            self.last_cwd_check = Instant::now();
            self.update_file_tree_cwd();
        }
    }

    fn update_file_tree_cwd(&mut self) {
        if !self.show_file_tree {
            return;
        }

        let cwd = self.focused.and_then(|id| {
            self.terminal_panes
                .get(&id)
                .and_then(|p| p.backend.detect_cwd_fallback())
        });

        if let Some(cwd) = cwd {
            if self.last_cwd.as_ref() != Some(&cwd) {
                self.last_cwd = Some(cwd.clone());
                if let Some(tree) = self.file_tree.as_mut() {
                    tree.set_root(cwd);
                }
                self.file_tree_scroll = 0.0;
            }
        }
    }

    fn handle_file_tree_click(&mut self, position: Vec2) {
        if !self.show_file_tree || position.x >= FILE_TREE_WIDTH {
            return;
        }

        let cell_size = match self.renderer.as_ref() {
            Some(r) => r.cell_size(),
            None => return,
        };

        let line_height = cell_size.height;
        let index = ((position.y + self.file_tree_scroll) / line_height) as usize;

        if let Some(tree) = self.file_tree.as_mut() {
            let entries = tree.visible_entries();
            if index < entries.len() {
                let entry = entries[index].clone();
                if entry.entry.is_dir {
                    tree.toggle(&entry.entry.path);
                }
            }
        }
    }

    fn reconfigure_surface(&mut self) {
        if let (Some(surface), Some(device), Some(config)) = (
            self.surface.as_ref(),
            self.device.as_ref(),
            self.surface_config.as_mut(),
        ) {
            config.width = self.window_size.width.max(1);
            config.height = self.window_size.height.max(1);
            surface.configure(device, config);
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
        for pane in self.terminal_panes.values() {
            if pane.backend.has_new_output() {
                self.needs_redraw = true;
                break;
            }
        }

        if self.needs_redraw {
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        } else {
            // Nothing changed — sleep until next event or 8ms timeout
            event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(8)));
        }
    }
}

// ──────────────────────────────────────────────
// Key conversion utilities
// ──────────────────────────────────────────────

fn winit_key_to_tide(key: &WinitKey) -> Option<Key> {
    match key {
        WinitKey::Named(named) => match named {
            NamedKey::Enter => Some(Key::Enter),
            NamedKey::Backspace => Some(Key::Backspace),
            NamedKey::Tab => Some(Key::Tab),
            NamedKey::Escape => Some(Key::Escape),
            NamedKey::Delete => Some(Key::Delete),
            NamedKey::ArrowUp => Some(Key::Up),
            NamedKey::ArrowDown => Some(Key::Down),
            NamedKey::ArrowLeft => Some(Key::Left),
            NamedKey::ArrowRight => Some(Key::Right),
            NamedKey::Home => Some(Key::Home),
            NamedKey::End => Some(Key::End),
            NamedKey::PageUp => Some(Key::PageUp),
            NamedKey::PageDown => Some(Key::PageDown),
            NamedKey::Insert => Some(Key::Insert),
            NamedKey::F1 => Some(Key::F(1)),
            NamedKey::F2 => Some(Key::F(2)),
            NamedKey::F3 => Some(Key::F(3)),
            NamedKey::F4 => Some(Key::F(4)),
            NamedKey::F5 => Some(Key::F(5)),
            NamedKey::F6 => Some(Key::F(6)),
            NamedKey::F7 => Some(Key::F(7)),
            NamedKey::F8 => Some(Key::F(8)),
            NamedKey::F9 => Some(Key::F(9)),
            NamedKey::F10 => Some(Key::F(10)),
            NamedKey::F11 => Some(Key::F(11)),
            NamedKey::F12 => Some(Key::F(12)),
            NamedKey::Space => Some(Key::Char(' ')),
            _ => None,
        },
        WinitKey::Character(s) => {
            let mut chars = s.chars();
            if let Some(c) = chars.next() {
                if chars.next().is_none() {
                    Some(Key::Char(c))
                } else {
                    None
                }
            } else {
                None
            }
        }
        _ => None,
    }
}

fn winit_modifiers_to_tide(modifiers: ModifiersState) -> Modifiers {
    Modifiers {
        shift: modifiers.shift_key(),
        ctrl: modifiers.control_key(),
        alt: modifiers.alt_key(),
        meta: modifiers.super_key(),
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
