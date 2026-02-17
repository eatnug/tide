// GPU renderer implementation
// Implements tide_core::Renderer using wgpu + cosmic-text

mod atlas;
mod chrome;
mod font;
mod grid;
mod init;
mod overlay;
mod shaders;
mod vertex;

use std::collections::HashMap;
use std::sync::Arc;

use cosmic_text::{FontSystem, SwashCache};
use tide_core::{Color, Rect, Renderer, Size, TextStyle, Vec2};

use atlas::GlyphAtlas;
use grid::PaneGridCache;
use vertex::{ChromeRectVertex, GlyphVertex, RectVertex};

// ──────────────────────────────────────────────
// WgpuRenderer
// ──────────────────────────────────────────────

pub struct WgpuRenderer {
    // GPU pipelines
    pub(crate) rect_pipeline: wgpu::RenderPipeline,
    pub(crate) chrome_rounded_pipeline: wgpu::RenderPipeline,
    pub(crate) glyph_pipeline: wgpu::RenderPipeline,

    // Uniform buffer (screen size)
    pub(crate) uniform_buffer: wgpu::Buffer,
    pub(crate) uniform_bind_group: wgpu::BindGroup,

    // Atlas
    pub(crate) atlas: GlyphAtlas,
    pub(crate) atlas_bind_group: wgpu::BindGroup,

    // Text subsystem
    pub(crate) font_system: FontSystem,
    pub(crate) swash_cache: SwashCache,

    // Per-pane grid caching
    pub(crate) pane_grid_caches: HashMap<u64, PaneGridCache>,
    pub(crate) active_pane_cache: PaneGridCache,
    pub(crate) active_pane_id: Option<u64>,

    // Cached grid layer — only rebuilt when grid content changes
    pub(crate) grid_rect_vertices: Vec<RectVertex>,
    pub(crate) grid_rect_indices: Vec<u32>,
    pub(crate) grid_glyph_vertices: Vec<GlyphVertex>,
    pub(crate) grid_glyph_indices: Vec<u32>,
    pub(crate) grid_needs_upload: bool,

    // Grid GPU buffers
    pub(crate) grid_rect_vb: wgpu::Buffer,
    pub(crate) grid_rect_ib: wgpu::Buffer,
    pub(crate) grid_glyph_vb: wgpu::Buffer,
    pub(crate) grid_glyph_ib: wgpu::Buffer,
    pub(crate) grid_rect_vb_capacity: usize,
    pub(crate) grid_rect_ib_capacity: usize,
    pub(crate) grid_glyph_vb_capacity: usize,
    pub(crate) grid_glyph_ib_capacity: usize,

    // Chrome layer — cached for panel backgrounds and file tree
    pub(crate) chrome_rect_vertices: Vec<ChromeRectVertex>,
    pub(crate) chrome_rect_indices: Vec<u32>,
    pub(crate) chrome_glyph_vertices: Vec<GlyphVertex>,
    pub(crate) chrome_glyph_indices: Vec<u32>,
    pub(crate) chrome_needs_upload: bool,
    pub(crate) chrome_rect_vb: wgpu::Buffer,
    pub(crate) chrome_rect_ib: wgpu::Buffer,
    pub(crate) chrome_glyph_vb: wgpu::Buffer,
    pub(crate) chrome_glyph_ib: wgpu::Buffer,
    pub(crate) chrome_rect_vb_capacity: usize,
    pub(crate) chrome_rect_ib_capacity: usize,
    pub(crate) chrome_glyph_vb_capacity: usize,
    pub(crate) chrome_glyph_ib_capacity: usize,

    // Overlay layer — rebuilt every frame (cursor, preedit)
    pub(crate) rect_vertices: Vec<RectVertex>,
    pub(crate) rect_indices: Vec<u32>,
    pub(crate) glyph_vertices: Vec<GlyphVertex>,
    pub(crate) glyph_indices: Vec<u32>,

    // Overlay GPU buffers
    pub(crate) rect_vb: wgpu::Buffer,
    pub(crate) rect_ib: wgpu::Buffer,
    pub(crate) glyph_vb: wgpu::Buffer,
    pub(crate) glyph_ib: wgpu::Buffer,
    pub(crate) rect_vb_capacity: usize,
    pub(crate) rect_ib_capacity: usize,
    pub(crate) glyph_vb_capacity: usize,
    pub(crate) glyph_ib_capacity: usize,

    // Top layer — rendered last (above all text), for opaque UI like search bar
    pub(crate) top_rect_vertices: Vec<RectVertex>,
    pub(crate) top_rect_indices: Vec<u32>,
    pub(crate) top_rounded_rect_vertices: Vec<ChromeRectVertex>,
    pub(crate) top_rounded_rect_indices: Vec<u32>,
    pub(crate) top_glyph_vertices: Vec<GlyphVertex>,
    pub(crate) top_glyph_indices: Vec<u32>,
    pub(crate) top_rect_vb: wgpu::Buffer,
    pub(crate) top_rect_ib: wgpu::Buffer,
    pub(crate) top_rounded_rect_vb: wgpu::Buffer,
    pub(crate) top_rounded_rect_ib: wgpu::Buffer,
    pub(crate) top_glyph_vb: wgpu::Buffer,
    pub(crate) top_glyph_ib: wgpu::Buffer,
    pub(crate) top_rect_vb_capacity: usize,
    pub(crate) top_rect_ib_capacity: usize,
    pub(crate) top_rounded_rect_vb_capacity: usize,
    pub(crate) top_rounded_rect_ib_capacity: usize,
    pub(crate) top_glyph_vb_capacity: usize,
    pub(crate) top_glyph_ib_capacity: usize,

    // Current frame state
    pub(crate) screen_size: Size,
    pub(crate) scale_factor: f32,
    pub(crate) base_font_size: f32,

    // Cached cell metrics
    pub(crate) cached_cell_size: Size,

    // Surface format (for potential re-creation)
    #[allow(dead_code)]
    pub(crate) surface_format: wgpu::TextureFormat,

    // Clear color (gap / background)
    pub clear_color: Color,

    // Atlas overflow tracking
    pub(crate) atlas_reset_count: u64,
    pub(crate) last_atlas_reset_count: u64,

    // Cached uniform screen size to avoid redundant writes
    pub(crate) last_uniform_screen: [f32; 2],

    // Store device and queue for uploading glyphs during draw calls
    pub(crate) device: Arc<wgpu::Device>,
    pub(crate) queue: Arc<wgpu::Queue>,
}

// ──────────────────────────────────────────────
// Renderer trait implementation
// ──────────────────────────────────────────────

impl Renderer for WgpuRenderer {
    fn begin_frame(&mut self, size: Size) {
        self.screen_size = size;
        self.rect_vertices.clear();
        self.rect_indices.clear();
        self.glyph_vertices.clear();
        self.glyph_indices.clear();
        self.top_rect_vertices.clear();
        self.top_rect_indices.clear();
        self.top_rounded_rect_vertices.clear();
        self.top_rounded_rect_indices.clear();
        self.top_glyph_vertices.clear();
        self.top_glyph_indices.clear();
    }

    fn draw_rect(&mut self, rect: Rect, color: Color) {
        let x = rect.x * self.scale_factor;
        let y = rect.y * self.scale_factor;
        let w = rect.width * self.scale_factor;
        let h = rect.height * self.scale_factor;
        self.push_rect_quad(x, y, w, h, color);
    }

    fn draw_text(&mut self, text: &str, position: Vec2, style: TextStyle, clip: Rect) {
        let scale = self.scale_factor;
        let cell_w = self.cached_cell_size.width * scale;
        let baseline_y = self.cached_cell_size.height * scale * 0.8; // approximate baseline

        let mut cursor_x = position.x * scale;
        let start_y = position.y * scale;

        // Clip bounds in physical pixels
        let clip_left = clip.x * scale;
        let clip_top = clip.y * scale;
        let clip_right = (clip.x + clip.width) * scale;
        let clip_bottom = (clip.y + clip.height) * scale;

        for ch in text.chars() {
            if ch == ' ' || ch == '\t' {
                let advance = if ch == '\t' { cell_w * 4.0 } else { cell_w };
                cursor_x += advance;
                continue;
            }

            // Draw background if present
            if let Some(bg) = style.background {
                let qx = cursor_x;
                let qy = start_y;
                let qw = cell_w;
                let qh = self.cached_cell_size.height * scale;
                if qx + qw > clip_left && qx < clip_right && qy + qh > clip_top && qy < clip_bottom
                {
                    self.push_rect_quad(qx, qy, qw, qh, bg);
                }
            }

            let region = self.ensure_glyph_cached(ch, style.bold, style.italic);

            if region.width > 0 && region.height > 0 {
                let gx = cursor_x + region.left;
                let gy = start_y + baseline_y - region.top;
                let gw = region.width as f32;
                let gh = region.height as f32;

                // Simple clip check
                if gx + gw > clip_left && gx < clip_right && gy + gh > clip_top && gy < clip_bottom
                {
                    self.push_glyph_quad(
                        gx,
                        gy,
                        gw,
                        gh,
                        region.uv_min,
                        region.uv_max,
                        style.foreground,
                    );
                }
            }

            cursor_x += cell_w;
        }
    }

    fn draw_cell(
        &mut self,
        character: char,
        row: usize,
        col: usize,
        style: TextStyle,
        cell_size: Size,
        offset: Vec2,
    ) {
        let scale = self.scale_factor;
        let px = (offset.x + col as f32 * cell_size.width) * scale;
        let py = (offset.y + row as f32 * cell_size.height) * scale;
        let cw = cell_size.width * scale;
        let ch = cell_size.height * scale;

        // Draw background
        if let Some(bg) = style.background {
            self.push_rect_quad(px, py, cw, ch, bg);
        }

        // Draw character (skip spaces)
        if character != ' ' && character != '\0' {
            let region = self.ensure_glyph_cached(character, style.bold, style.italic);

            if region.width > 0 && region.height > 0 {
                let baseline_y = ch * 0.8;
                let gx = px + region.left;
                let gy = py + baseline_y - region.top;
                let gw = region.width as f32;
                let gh = region.height as f32;

                self.push_glyph_quad(
                    gx,
                    gy,
                    gw,
                    gh,
                    region.uv_min,
                    region.uv_max,
                    style.foreground,
                );
            }
        }
    }

    fn end_frame(&mut self) {
        // Batching is complete. The caller will invoke render_frame()
        // to submit the draw calls to the GPU.
    }

    fn cell_size(&self) -> Size {
        self.cached_cell_size
    }
}

// ──────────────────────────────────────────────
// Top layer — rendered after ALL other layers (opaque UI)
// ──────────────────────────────────────────────

impl WgpuRenderer {
    /// Draw a rounded rect in the top layer (SDF-based AA, rendered after all text).
    pub fn draw_top_rounded_rect(&mut self, rect: Rect, color: Color, radius: f32) {
        let s = self.scale_factor;
        let x = rect.x * s;
        let y = rect.y * s;
        let w = rect.width * s;
        let h = rect.height * s;
        let r = radius * s;

        let expand = 1.0_f32;
        let qx = x - expand;
        let qy = y - expand;
        let qw = w + expand * 2.0;
        let qh = h + expand * 2.0;

        let center = [x + w * 0.5, y + h * 0.5];
        let half = [w * 0.5, h * 0.5];
        let c = [color.r, color.g, color.b, color.a];

        let base = self.top_rounded_rect_vertices.len() as u32;
        let vert = |px: f32, py: f32| ChromeRectVertex {
            position: [px, py],
            color: c,
            rect_center: center,
            rect_half: half,
            corner_radius: r,
            _pad: 0.0,
        };
        self.top_rounded_rect_vertices.push(vert(qx, qy));
        self.top_rounded_rect_vertices.push(vert(qx + qw, qy));
        self.top_rounded_rect_vertices.push(vert(qx + qw, qy + qh));
        self.top_rounded_rect_vertices.push(vert(qx, qy + qh));
        self.top_rounded_rect_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Draw a rect in the top layer (rendered after all text).
    pub fn draw_top_rect(&mut self, rect: Rect, color: Color) {
        let x = rect.x * self.scale_factor;
        let y = rect.y * self.scale_factor;
        let w = rect.width * self.scale_factor;
        let h = rect.height * self.scale_factor;

        let base = self.top_rect_vertices.len() as u32;
        let c = [color.r, color.g, color.b, color.a];
        self.top_rect_vertices.push(RectVertex { position: [x, y], color: c });
        self.top_rect_vertices.push(RectVertex { position: [x + w, y], color: c });
        self.top_rect_vertices.push(RectVertex { position: [x + w, y + h], color: c });
        self.top_rect_vertices.push(RectVertex { position: [x, y + h], color: c });
        self.top_rect_indices.push(base);
        self.top_rect_indices.push(base + 1);
        self.top_rect_indices.push(base + 2);
        self.top_rect_indices.push(base);
        self.top_rect_indices.push(base + 2);
        self.top_rect_indices.push(base + 3);
    }

    /// Draw a single glyph in the top layer (rendered after all other layers).
    /// Used for rendering inverse cursor characters on top of the cursor rect.
    pub fn draw_top_glyph(&mut self, ch: char, position: Vec2, color: Color, bold: bool, italic: bool) {
        let scale = self.scale_factor;
        let baseline_y = self.cached_cell_size.height * scale * 0.8;

        let start_x = position.x * scale;
        let start_y = position.y * scale;

        if ch == ' ' || ch == '\0' {
            return;
        }

        let region = self.ensure_glyph_cached(ch, bold, italic);

        if region.width > 0 && region.height > 0 {
            let gx = start_x + region.left;
            let gy = start_y + baseline_y - region.top;
            let gw = region.width as f32;
            let gh = region.height as f32;

            let base = self.top_glyph_vertices.len() as u32;
            let c = [color.r, color.g, color.b, color.a];
            self.top_glyph_vertices.push(GlyphVertex { position: [gx, gy], uv: [region.uv_min[0], region.uv_min[1]], color: c });
            self.top_glyph_vertices.push(GlyphVertex { position: [gx + gw, gy], uv: [region.uv_max[0], region.uv_min[1]], color: c });
            self.top_glyph_vertices.push(GlyphVertex { position: [gx + gw, gy + gh], uv: [region.uv_max[0], region.uv_max[1]], color: c });
            self.top_glyph_vertices.push(GlyphVertex { position: [gx, gy + gh], uv: [region.uv_min[0], region.uv_max[1]], color: c });
            self.top_glyph_indices.push(base);
            self.top_glyph_indices.push(base + 1);
            self.top_glyph_indices.push(base + 2);
            self.top_glyph_indices.push(base);
            self.top_glyph_indices.push(base + 2);
            self.top_glyph_indices.push(base + 3);
        }
    }

    /// Draw text in the top layer (rendered after all text).
    pub fn draw_top_text(&mut self, text: &str, position: Vec2, style: TextStyle, clip: Rect) {
        let scale = self.scale_factor;
        let cell_w = self.cached_cell_size.width * scale;
        let baseline_y = self.cached_cell_size.height * scale * 0.8;

        let mut cursor_x = position.x * scale;
        let start_y = position.y * scale;

        let clip_left = clip.x * scale;
        let clip_top = clip.y * scale;
        let clip_right = (clip.x + clip.width) * scale;
        let clip_bottom = (clip.y + clip.height) * scale;

        for ch in text.chars() {
            if ch == ' ' || ch == '\t' {
                let advance = if ch == '\t' { cell_w * 4.0 } else { cell_w };
                cursor_x += advance;
                continue;
            }

            if let Some(bg) = style.background {
                let qx = cursor_x;
                let qy = start_y;
                let qw = cell_w;
                let qh = self.cached_cell_size.height * scale;
                if qx + qw > clip_left && qx < clip_right && qy + qh > clip_top && qy < clip_bottom {
                    // Push into top rect arrays
                    let base = self.top_rect_vertices.len() as u32;
                    let c = [bg.r, bg.g, bg.b, bg.a];
                    self.top_rect_vertices.push(RectVertex { position: [qx, qy], color: c });
                    self.top_rect_vertices.push(RectVertex { position: [qx + qw, qy], color: c });
                    self.top_rect_vertices.push(RectVertex { position: [qx + qw, qy + qh], color: c });
                    self.top_rect_vertices.push(RectVertex { position: [qx, qy + qh], color: c });
                    self.top_rect_indices.push(base);
                    self.top_rect_indices.push(base + 1);
                    self.top_rect_indices.push(base + 2);
                    self.top_rect_indices.push(base);
                    self.top_rect_indices.push(base + 2);
                    self.top_rect_indices.push(base + 3);
                }
            }

            let region = self.ensure_glyph_cached(ch, style.bold, style.italic);

            if region.width > 0 && region.height > 0 {
                let gx = cursor_x + region.left;
                let gy = start_y + baseline_y - region.top;
                let gw = region.width as f32;
                let gh = region.height as f32;

                if gx + gw > clip_left && gx < clip_right && gy + gh > clip_top && gy < clip_bottom {
                    let base = self.top_glyph_vertices.len() as u32;
                    let c = [style.foreground.r, style.foreground.g, style.foreground.b, style.foreground.a];
                    self.top_glyph_vertices.push(GlyphVertex { position: [gx, gy], uv: [region.uv_min[0], region.uv_min[1]], color: c });
                    self.top_glyph_vertices.push(GlyphVertex { position: [gx + gw, gy], uv: [region.uv_max[0], region.uv_min[1]], color: c });
                    self.top_glyph_vertices.push(GlyphVertex { position: [gx + gw, gy + gh], uv: [region.uv_max[0], region.uv_max[1]], color: c });
                    self.top_glyph_vertices.push(GlyphVertex { position: [gx, gy + gh], uv: [region.uv_min[0], region.uv_max[1]], color: c });
                    self.top_glyph_indices.push(base);
                    self.top_glyph_indices.push(base + 1);
                    self.top_glyph_indices.push(base + 2);
                    self.top_glyph_indices.push(base);
                    self.top_glyph_indices.push(base + 2);
                    self.top_glyph_indices.push(base + 3);
                }
            }

            cursor_x += cell_w;
        }
    }
}
