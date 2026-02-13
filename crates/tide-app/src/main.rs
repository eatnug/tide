// Tide v0.1 — Integration (Step 3)
// Wires all crates together: winit window, wgpu surface, renderer, terminal panes,
// layout engine, input router, file tree, and CWD following.

mod editor_pane;
mod input;
mod pane;
mod theme;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalSize};
use winit::event::{ElementState, Ime, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Window, WindowAttributes, WindowId};

use tide_core::{
    Color, DropZone, FileTreeSource, InputEvent, LayoutEngine, MouseButton,
    PaneId, Rect, Renderer, Size, SplitDirection, TerminalBackend, TextStyle, Vec2,
};
use tide_editor::input::EditorAction;
use tide_input::{Action, Direction, GlobalAction, Router};
use tide_layout::SplitLayout;
use tide_renderer::WgpuRenderer;
use tide_tree::FsTree;

use editor_pane::EditorPane;
use input::{winit_key_to_tide, winit_modifiers_to_tide};
use pane::{PaneKind, TerminalPane};
use theme::*;

// ──────────────────────────────────────────────
// Pane drag & drop state machine
// ──────────────────────────────────────────────

enum PaneDragState {
    Idle,
    PendingDrag {
        source_pane: PaneId,
        press_pos: Vec2,
    },
    Dragging {
        source_pane: PaneId,
        drop_target: Option<(PaneId, DropZone)>,
    },
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
    panes: HashMap<PaneId, PaneKind>,
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

    // Computed pane rects: tiling rects (hit-testing/drag) and visual rects (gap-inset, rendering)
    pane_rects: Vec<(PaneId, Rect)>,
    visual_pane_rects: Vec<(PaneId, Rect)>,

    // Grid generation tracking for vertex caching
    pane_generations: HashMap<PaneId, u64>,
    layout_generation: u64,

    // Chrome generation tracking (borders + file tree)
    chrome_generation: u64,
    last_chrome_generation: u64,

    // Input latency: skip 8ms sleep after keypress while awaiting PTY response
    input_just_sent: bool,
    input_sent_at: Option<Instant>,

    // Adaptive frame pacing: throttle to ~60fps during high throughput
    consecutive_dirty_frames: u32,

    // Pane drag & drop
    pane_drag: PaneDragState,
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
            last_cursor_pos: Vec2::new(0.0, 0.0),
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
        self.visual_pane_rects = self
            .pane_rects
            .iter()
            .map(|&(id, r)| {
                // For each edge: if at window boundary → PANE_GAP, else → PANE_GAP/2
                let half = PANE_GAP / 2.0;
                let left = if r.x <= terminal_offset_x + 0.5 { PANE_GAP } else { half };
                let top = if r.y <= 0.5 { PANE_GAP } else { half };
                let right = if r.x + r.width >= terminal_offset_x + terminal_area.width - 0.5 {
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
        let visual_pane_rects = self.visual_pane_rects.clone();

        let renderer = self.renderer.as_mut().unwrap();

        // Atlas reset → all cached UV coords are stale, force full rebuild
        if renderer.atlas_was_reset() {
            self.pane_generations.clear();
            self.last_chrome_generation = self.chrome_generation.wrapping_sub(1);
        }

        renderer.begin_frame(logical);

        // Rebuild chrome layer only when chrome content changed (panel backgrounds, file tree)
        let chrome_dirty = self.chrome_generation != self.last_chrome_generation;
        if chrome_dirty {
            renderer.invalidate_chrome();

            // Draw file tree panel if visible (rounded rect background)
            if show_file_tree {
                let tree_visual_rect = Rect::new(
                    PANE_GAP,
                    PANE_GAP,
                    FILE_TREE_WIDTH - PANE_GAP - PANE_GAP / 2.0,
                    logical.height - PANE_GAP * 2.0,
                );
                renderer.draw_chrome_rounded_rect(tree_visual_rect, TREE_BG, PANE_RADIUS);

                if let Some(tree) = self.file_tree.as_ref() {
                    let cell_size = renderer.cell_size();
                    let line_height = cell_size.height;
                    let indent_width = cell_size.width * 1.5;
                    let left_padding = PANE_GAP + PANE_PADDING;

                    let entries = tree.visible_entries();
                    for (i, entry) in entries.iter().enumerate() {
                        let y = PANE_GAP + PANE_PADDING + i as f32 * line_height - file_tree_scroll;
                        if y + line_height < 0.0 || y > logical.height {
                            continue;
                        }

                        let x = left_padding + entry.depth as f32 * indent_width;

                        // Nerd Font icon
                        let icon = file_icon(&entry.entry.name, entry.entry.is_dir, entry.is_expanded);
                        let icon_color = if entry.entry.is_dir {
                            TREE_DIR_COLOR
                        } else {
                            TREE_ICON_COLOR
                        };

                        // Draw icon
                        let icon_style = TextStyle {
                            foreground: icon_color,
                            background: None,
                            bold: false,
                            italic: false,
                            underline: false,
                        };
                        let icon_str: String = std::iter::once(icon).collect();
                        renderer.draw_chrome_text(
                            &icon_str,
                            Vec2::new(x, y),
                            icon_style,
                            tree_visual_rect,
                        );

                        // Draw name after icon + space
                        let name_x = x + cell_size.width * 2.0;
                        let text_color = if entry.entry.is_dir {
                            TREE_DIR_COLOR
                        } else {
                            TREE_TEXT_COLOR
                        };
                        let name_style = TextStyle {
                            foreground: text_color,
                            background: None,
                            bold: entry.entry.is_dir,
                            italic: false,
                            underline: false,
                        };
                        renderer.draw_chrome_text(
                            &entry.entry.name,
                            Vec2::new(name_x, y),
                            name_style,
                            tree_visual_rect,
                        );
                    }
                }
            }

            // Draw pane backgrounds as rounded rects
            for &(id, rect) in &visual_pane_rects {
                let bg = if focused == Some(id) {
                    PANE_BG_FOCUSED
                } else {
                    PANE_BG
                };
                renderer.draw_chrome_rounded_rect(rect, bg, PANE_RADIUS);
            }

            // Focus accent bar: thin colored bar at the top of the focused pane
            if let Some(fid) = focused {
                if let Some(&(_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == fid) {
                    let bar_h = 2.0;
                    let bar_rect = Rect::new(
                        rect.x + PANE_RADIUS,
                        rect.y,
                        rect.width - PANE_RADIUS * 2.0,
                        bar_h,
                    );
                    renderer.draw_chrome_rect(bar_rect, ACCENT_COLOR);
                }
            }

            // Tab bar text for each pane
            let cell_height = renderer.cell_size().height;
            for &(id, rect) in &visual_pane_rects {
                let title = pane_title(&self.panes, id);
                let text_color = if focused == Some(id) {
                    TAB_BAR_TEXT_FOCUSED
                } else {
                    TAB_BAR_TEXT
                };
                let style = TextStyle {
                    foreground: text_color,
                    background: None,
                    bold: focused == Some(id),
                    italic: false,
                    underline: false,
                };
                let text_y = rect.y + (TAB_BAR_HEIGHT - cell_height) / 2.0;
                renderer.draw_chrome_text(
                    &title,
                    Vec2::new(rect.x + PANE_PADDING + 4.0, text_y),
                    style,
                    Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT),
                );
            }

            self.last_chrome_generation = self.chrome_generation;
        }

        // Check if grid needs rebuild (any pane content or layout changed)
        let mut grid_dirty = false;
        for &(id, _) in &visual_pane_rects {
            let gen = match self.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => pane.backend.grid_generation(),
                Some(PaneKind::Editor(pane)) => pane.generation(),
                None => continue,
            };
            let prev = self.pane_generations.get(&id).copied().unwrap_or(u64::MAX);
            if gen != prev {
                grid_dirty = true;
                break;
            }
        }

        // Rebuild grid layer only when content or layout changed
        if grid_dirty {
            renderer.invalidate_grid();
            for &(id, rect) in &visual_pane_rects {
                let inner = Rect::new(
                    rect.x + PANE_PADDING,
                    rect.y + TAB_BAR_HEIGHT,
                    rect.width - 2.0 * PANE_PADDING,
                    rect.height - TAB_BAR_HEIGHT - PANE_PADDING,
                );
                match self.panes.get(&id) {
                    Some(PaneKind::Terminal(pane)) => {
                        pane.render_grid(inner, renderer);
                        self.pane_generations.insert(id, pane.backend.grid_generation());
                    }
                    Some(PaneKind::Editor(pane)) => {
                        pane.render_grid(inner, renderer);
                        self.pane_generations.insert(id, pane.generation());
                    }
                    None => {}
                }
            }
        }

        // Always render cursor (overlay layer) — cursor blinks/moves independently
        for &(id, rect) in &visual_pane_rects {
            let inner = Rect::new(
                rect.x + PANE_PADDING,
                rect.y + TAB_BAR_HEIGHT,
                rect.width - 2.0 * PANE_PADDING,
                rect.height - TAB_BAR_HEIGHT - PANE_PADDING,
            );
            match self.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => pane.render_cursor(inner, renderer),
                Some(PaneKind::Editor(pane)) => pane.render_cursor(inner, renderer),
                None => {}
            }
        }

        // Render IME preedit overlay (Korean composition in progress) — only for terminal panes
        if !self.ime_preedit.is_empty() {
            if let Some(focused_id) = focused {
                if let Some((_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == focused_id) {
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get(&focused_id) {
                        let cursor = pane.backend.cursor();
                        let cell_size = renderer.cell_size();
                        let inner_offset = Vec2::new(
                            rect.x + PANE_PADDING,
                            rect.y + TAB_BAR_HEIGHT,
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

        // Draw drop preview overlay when dragging a pane
        if let PaneDragState::Dragging { drop_target: Some((target_id, zone)), .. } = &self.pane_drag {
            if let Some(&(_, target_rect)) = visual_pane_rects.iter().find(|(id, _)| *id == *target_id) {
                let preview = compute_preview_rect(target_rect, *zone);
                // Fill
                renderer.draw_rect(preview, DROP_PREVIEW_FILL);
                // Border (4 thin rects)
                let bw = DROP_PREVIEW_BORDER_WIDTH;
                renderer.draw_rect(Rect::new(preview.x, preview.y, preview.width, bw), DROP_PREVIEW_BORDER);
                renderer.draw_rect(Rect::new(preview.x, preview.y + preview.height - bw, preview.width, bw), DROP_PREVIEW_BORDER);
                renderer.draw_rect(Rect::new(preview.x, preview.y, bw, preview.height), DROP_PREVIEW_BORDER);
                renderer.draw_rect(Rect::new(preview.x + preview.width - bw, preview.y, bw, preview.height), DROP_PREVIEW_BORDER);
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
                        if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&focused_id) {
                            pane.backend.write(text.as_bytes());
                            self.input_just_sent = true;
                            self.input_sent_at = Some(Instant::now());
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

                // Cancel pane drag on Escape
                if !matches!(self.pane_drag, PaneDragState::Idle) {
                    if event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) {
                        self.pane_drag = PaneDragState::Idle;
                        return;
                    }
                }

                // During IME composition, only handle non-character keys
                if self.ime_composing
                    && matches!(event.logical_key, winit::keyboard::Key::Character(_))
                {
                    return;
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
                    // Handle pane drag drop on mouse release
                    let drag_state = std::mem::replace(&mut self.pane_drag, PaneDragState::Idle);
                    match drag_state {
                        PaneDragState::Dragging { source_pane, drop_target: Some((target_id, zone)), .. } => {
                            if self.layout.move_pane(source_pane, target_id, zone) {
                                self.chrome_generation += 1;
                                self.compute_layout();
                            }
                            return;
                        }
                        PaneDragState::PendingDrag { source_pane, .. } => {
                            // Click (no drag): just focus the pane
                            if self.focused != Some(source_pane) {
                                self.focused = Some(source_pane);
                                self.router.set_focused(source_pane);
                                self.chrome_generation += 1;
                                self.update_file_tree_cwd();
                            }
                            return;
                        }
                        PaneDragState::Dragging { .. } => {
                            // Drop with no valid target: cancel
                            return;
                        }
                        PaneDragState::Idle => {}
                    }

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

                // Check if click is on a tab bar — initiate pane drag
                if btn == MouseButton::Left {
                    if let Some(pane_id) = self.pane_at_tab_bar(self.last_cursor_pos) {
                        self.pane_drag = PaneDragState::PendingDrag {
                            source_pane: pane_id,
                            press_pos: self.last_cursor_pos,
                        };
                        // Focus the pane immediately
                        if self.focused != Some(pane_id) {
                            self.focused = Some(pane_id);
                            self.router.set_focused(pane_id);
                            self.chrome_generation += 1;
                            self.update_file_tree_cwd();
                        }
                        return;
                    }
                }

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

                // Handle pane drag state machine
                match &self.pane_drag {
                    PaneDragState::PendingDrag { source_pane, press_pos } => {
                        let dx = pos.x - press_pos.x;
                        let dy = pos.y - press_pos.y;
                        if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD {
                            let source = *source_pane;
                            let target = self.compute_drop_target(pos, source);
                            self.pane_drag = PaneDragState::Dragging {
                                source_pane: source,
                                drop_target: target,
                            };
                        }
                        return;
                    }
                    PaneDragState::Dragging { source_pane, .. } => {
                        let source = *source_pane;
                        let target = self.compute_drop_target(pos, source);
                        self.pane_drag = PaneDragState::Dragging {
                            source_pane: source,
                            drop_target: target,
                        };
                        return;
                    }
                    PaneDragState::Idle => {}
                }

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
                    let new_scroll = (self.file_tree_scroll - dy * 10.0).max(0.0);
                    if new_scroll != self.file_tree_scroll {
                        self.file_tree_scroll = new_scroll;
                        self.chrome_generation += 1;
                    }
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
                if let Some(InputEvent::MouseClick { position, .. }) = event {
                    if self.focused != Some(id) {
                        self.focused = Some(id);
                        self.router.set_focused(id);
                        self.chrome_generation += 1;
                        self.update_file_tree_cwd();
                    }

                    // Ctrl+Click / Cmd+Click on terminal → try to open file at click position
                    let mods = winit_modifiers_to_tide(self.modifiers);
                    if mods.ctrl || mods.meta {
                        if let Some(path) = self.extract_file_path_at(id, position) {
                            self.open_editor_pane(path);
                            return;
                        }
                    }
                }

                // Forward keyboard input to the pane
                if let Some(InputEvent::KeyPress { key, modifiers }) = event {
                    match self.panes.get_mut(&id) {
                        Some(PaneKind::Terminal(pane)) => {
                            pane.handle_key(&key, &modifiers);
                            self.input_just_sent = true;
                            self.input_sent_at = Some(Instant::now());
                        }
                        Some(PaneKind::Editor(pane)) => {
                            if let Some(action) = tide_editor::key_to_editor_action(&key, &modifiers) {
                                let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                                let visible_rows = if let Some(cs) = cell_size {
                                    let rect = self.visual_pane_rects.iter()
                                        .find(|(pid, _)| *pid == id)
                                        .map(|(_, r)| r);
                                    rect.map(|r| ((r.height - TAB_BAR_HEIGHT - PANE_PADDING) / cs.height).floor() as usize)
                                        .unwrap_or(30)
                                } else {
                                    30
                                };
                                pane.handle_action(action, visible_rows);
                            }
                        }
                        None => {}
                    }
                }

                // Forward mouse scroll to pane
                if let Some(InputEvent::MouseScroll { delta, .. }) = event {
                    match self.panes.get_mut(&id) {
                        Some(PaneKind::Editor(pane)) => {
                            if delta > 0.0 {
                                pane.handle_action(EditorAction::ScrollUp(delta.abs()), 30);
                            } else {
                                pane.handle_action(EditorAction::ScrollDown(delta.abs()), 30);
                            }
                        }
                        Some(PaneKind::Terminal(pane)) => {
                            // Positive delta = scroll up (into history)
                            pane.scroll_display(delta as i32);
                        }
                        None => {}
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
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
            }
            GlobalAction::SplitHorizontal => {
                if let Some(focused) = self.focused {
                    let new_id = self.layout.split(focused, SplitDirection::Horizontal);
                    self.create_terminal_pane(new_id);
                    self.chrome_generation += 1;
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
                    self.panes.remove(&focused);

                    // Focus the first remaining pane
                    let remaining = self.layout.pane_ids();
                    if let Some(&next) = remaining.first() {
                        self.focused = Some(next);
                        self.router.set_focused(next);
                    } else {
                        self.focused = None;
                    }

                    self.chrome_generation += 1;
                    self.compute_layout();
                    self.update_file_tree_cwd();
                }
            }
            GlobalAction::ToggleFileTree => {
                self.show_file_tree = !self.show_file_tree;
                self.chrome_generation += 1;
                self.compute_layout();
                if self.show_file_tree {
                    self.update_file_tree_cwd();
                }
            }
            GlobalAction::OpenFile => {
                // Open file tree so user can pick a file
                if !self.show_file_tree {
                    self.show_file_tree = true;
                    self.chrome_generation += 1;
                    self.compute_layout();
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
                    if best.is_none_or(|(_, d)| score < d) {
                        best = Some((id, score));
                    }
                }

                if let Some((next_id, _)) = best {
                    self.focused = Some(next_id);
                    self.router.set_focused(next_id);
                    self.chrome_generation += 1;
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
                self.panes.insert(id, PaneKind::Terminal(pane));
            }
            Err(e) => {
                log::error!("Failed to create terminal pane: {}", e);
            }
        }
    }

    /// Open a file in a new editor pane. If already open, focus the existing pane.
    fn open_editor_pane(&mut self, path: PathBuf) {
        // Check if file is already open in an editor pane
        for (&id, pane) in &self.panes {
            if let PaneKind::Editor(editor) = pane {
                if editor.editor.file_path() == Some(path.as_path()) {
                    self.focused = Some(id);
                    self.router.set_focused(id);
                    self.chrome_generation += 1;
                    return;
                }
            }
        }

        // Split the focused pane to create space for the editor
        if let Some(focused) = self.focused {
            let new_id = self.layout.split(focused, SplitDirection::Vertical);
            match EditorPane::open(new_id, &path) {
                Ok(pane) => {
                    self.panes.insert(new_id, PaneKind::Editor(pane));
                    self.focused = Some(new_id);
                    self.router.set_focused(new_id);
                    self.chrome_generation += 1;
                    self.compute_layout();
                }
                Err(e) => {
                    log::error!("Failed to open editor for {:?}: {}", path, e);
                    // Remove the split since we couldn't create the editor
                    self.layout.remove(new_id);
                }
            }
        }
    }

    /// Try to extract a file path from the terminal grid at the given click position.
    /// Scans the clicked row for path-like text and resolves against the terminal's CWD.
    fn extract_file_path_at(&self, pane_id: PaneId, position: Vec2) -> Option<PathBuf> {
        let pane = match self.panes.get(&pane_id) {
            Some(PaneKind::Terminal(p)) => p,
            _ => return None,
        };

        let (_, visual_rect) = self
            .visual_pane_rects
            .iter()
            .find(|(id, _)| *id == pane_id)?;
        let cell_size = self.renderer.as_ref()?.cell_size();

        let inner_x = visual_rect.x + PANE_PADDING;
        let inner_y = visual_rect.y + TAB_BAR_HEIGHT;

        let col = ((position.x - inner_x) / cell_size.width) as usize;
        let row = ((position.y - inner_y) / cell_size.height) as usize;

        let grid = pane.backend.grid();
        if row >= grid.cells.len() {
            return None;
        }
        let line = &grid.cells[row];

        // Build the full text of the row
        let row_text: String = line.iter().map(|c| c.character).collect();
        let row_text = row_text.trim_end();

        if row_text.is_empty() {
            return None;
        }

        // Find the word/path segment under the cursor.
        // Expand left and right from the click column to find path-like characters.
        let chars: Vec<char> = row_text.chars().collect();
        if col >= chars.len() {
            return None;
        }

        let is_path_char = |c: char| -> bool {
            c.is_alphanumeric() || matches!(c, '/' | '\\' | '.' | '-' | '_' | '~')
        };

        let mut start = col;
        while start > 0 && is_path_char(chars[start - 1]) {
            start -= 1;
        }
        let mut end = col;
        while end < chars.len() && is_path_char(chars[end]) {
            end += 1;
        }

        // Also skip trailing colon+number (e.g., "file.rs:42")
        let segment: String = chars[start..end].iter().collect();
        let path_str = segment.split(':').next().unwrap_or(&segment);

        if path_str.is_empty() || !path_str.contains('.') && !path_str.contains('/') {
            return None;
        }

        let path = std::path::Path::new(path_str);

        // If relative, resolve against terminal CWD
        let resolved = if path.is_absolute() {
            path.to_path_buf()
        } else {
            let cwd = pane.backend.detect_cwd_fallback()?;
            cwd.join(path)
        };

        // Only return if the file actually exists
        if resolved.is_file() {
            Some(resolved)
        } else {
            None
        }
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

    fn update_file_tree_cwd(&mut self) {
        if !self.show_file_tree {
            return;
        }

        let cwd = self.focused.and_then(|id| {
            match self.panes.get(&id) {
                Some(PaneKind::Terminal(p)) => p.backend.detect_cwd_fallback(),
                _ => None,
            }
        });

        if let Some(cwd) = cwd {
            if self.last_cwd.as_ref() != Some(&cwd) {
                self.last_cwd = Some(cwd.clone());
                if let Some(tree) = self.file_tree.as_mut() {
                    tree.set_root(cwd);
                }
                self.file_tree_scroll = 0.0;
                self.chrome_generation += 1;
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
        // Account for gap + padding offsets
        let adjusted_y = position.y - PANE_GAP - PANE_PADDING;
        let index = ((adjusted_y + self.file_tree_scroll) / line_height) as usize;

        // Extract click info from file tree (borrow released before open_editor_pane)
        let click_result = if let Some(tree) = self.file_tree.as_mut() {
            let entries = tree.visible_entries();
            if index < entries.len() {
                let entry = entries[index].clone();
                if entry.entry.is_dir {
                    tree.toggle(&entry.entry.path);
                    self.chrome_generation += 1;
                    None
                } else {
                    Some(entry.entry.path.clone())
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(path) = click_result {
            self.open_editor_pane(path);
        }
    }

    /// Hit-test whether the position is within a pane's tab bar area.
    fn pane_at_tab_bar(&self, pos: Vec2) -> Option<PaneId> {
        for &(id, rect) in &self.visual_pane_rects {
            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
            if tab_rect.contains(pos) {
                return Some(id);
            }
        }
        None
    }

    /// Compute the drop target (pane + zone) for a given mouse position during drag.
    fn compute_drop_target(&self, mouse: Vec2, source: PaneId) -> Option<(PaneId, DropZone)> {
        let source_rect = self.visual_pane_rects.iter().find(|(id, _)| *id == source).map(|(_, r)| *r)?;

        for &(id, rect) in &self.visual_pane_rects {
            if id == source {
                continue;
            }
            if !rect.contains(mouse) {
                continue;
            }

            // Compute relative position within the pane rect (0.0 to 1.0)
            let rel_x = (mouse.x - rect.x) / rect.width;
            let rel_y = (mouse.y - rect.y) / rect.height;

            let mut zone = if rel_y < 0.25 {
                DropZone::Top
            } else if rel_y > 0.75 {
                DropZone::Bottom
            } else if rel_x < 0.25 {
                DropZone::Left
            } else if rel_x > 0.75 {
                DropZone::Right
            } else {
                DropZone::Center
            };

            // If the directional zone would result in the same arrangement
            // (source is already on that side of target), show swap instead.
            let src_cx = source_rect.x + source_rect.width / 2.0;
            let src_cy = source_rect.y + source_rect.height / 2.0;
            let tgt_cx = rect.x + rect.width / 2.0;
            let tgt_cy = rect.y + rect.height / 2.0;

            let is_redundant = match zone {
                DropZone::Left => src_cx < tgt_cx,
                DropZone::Right => src_cx > tgt_cx,
                DropZone::Top => src_cy < tgt_cy,
                DropZone::Bottom => src_cy > tgt_cy,
                DropZone::Center => false,
            };
            if is_redundant {
                zone = DropZone::Center;
            }

            return Some((id, zone));
        }
        None
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
// Tab bar title
// ──────────────────────────────────────────────

fn pane_title(panes: &HashMap<PaneId, PaneKind>, id: PaneId) -> String {
    match panes.get(&id) {
        Some(PaneKind::Terminal(pane)) => {
            if let Some(cwd) = pane.backend.detect_cwd_fallback() {
                let components: Vec<_> = cwd.components().collect();
                if components.len() <= 2 {
                    return cwd.display().to_string();
                } else {
                    let last_two: std::path::PathBuf =
                        components[components.len() - 2..].iter().collect();
                    return last_two.display().to_string();
                }
            }
            format!("Terminal {}", id)
        }
        Some(PaneKind::Editor(pane)) => pane.title(),
        None => format!("Pane {}", id),
    }
}

// ──────────────────────────────────────────────
// Drop preview geometry
// ──────────────────────────────────────────────

fn compute_preview_rect(target_rect: Rect, zone: DropZone) -> Rect {
    match zone {
        DropZone::Top => Rect::new(
            target_rect.x,
            target_rect.y,
            target_rect.width,
            target_rect.height / 2.0,
        ),
        DropZone::Bottom => Rect::new(
            target_rect.x,
            target_rect.y + target_rect.height / 2.0,
            target_rect.width,
            target_rect.height / 2.0,
        ),
        DropZone::Left => Rect::new(
            target_rect.x,
            target_rect.y,
            target_rect.width / 2.0,
            target_rect.height,
        ),
        DropZone::Right => Rect::new(
            target_rect.x + target_rect.width / 2.0,
            target_rect.y,
            target_rect.width / 2.0,
            target_rect.height,
        ),
        DropZone::Center => target_rect,
    }
}

// ──────────────────────────────────────────────
// Nerd Font file icons
// ──────────────────────────────────────────────

fn file_icon(name: &str, is_dir: bool, expanded: bool) -> char {
    if is_dir {
        return if expanded { '\u{f07c}' } else { '\u{f07b}' };
    }
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => '\u{e7a8}',
        "toml" => '\u{e615}',
        "md" => '\u{e73e}',
        "json" => '\u{e60b}',
        "yaml" | "yml" => '\u{e615}',
        "js" => '\u{e74e}',
        "ts" => '\u{e628}',
        "tsx" | "jsx" => '\u{e7ba}',
        "py" => '\u{e73c}',
        "go" => '\u{e626}',
        "c" | "h" => '\u{e61e}',
        "cpp" | "hpp" | "cc" => '\u{e61d}',
        "lock" => '\u{f023}',
        "sh" | "bash" | "zsh" => '\u{e795}',
        "git" | "gitignore" => '\u{e702}',
        "css" => '\u{e749}',
        "html" => '\u{e736}',
        "svg" => '\u{e698}',
        "png" | "jpg" | "jpeg" | "gif" | "webp" => '\u{f1c5}',
        "txt" => '\u{f15c}',
        _ => '\u{f15b}',
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
