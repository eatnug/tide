use unicode_width::UnicodeWidthChar;

use tide_core::{Color, Rect, TextStyle, Vec2};

use crate::vertex::{ChromeRectVertex, GlyphVertex};
use crate::WgpuRenderer;

impl WgpuRenderer {
    /// Draw a sharp rect into the cached chrome layer (radius = 0).
    pub fn draw_chrome_rect(&mut self, rect: Rect, color: Color) {
        self.draw_chrome_rounded_rect(rect, color, 0.0);
    }

    /// Draw a rounded rect into the cached chrome layer (SDF-based AA).
    pub fn draw_chrome_rounded_rect(&mut self, rect: Rect, color: Color, radius: f32) {
        let s = self.scale_factor;
        let x = rect.x * s;
        let y = rect.y * s;
        let w = rect.width * s;
        let h = rect.height * s;
        let r = radius * s;

        // Expand quad by 1px for AA bleed
        let expand = 1.0_f32;
        let qx = x - expand;
        let qy = y - expand;
        let qw = w + expand * 2.0;
        let qh = h + expand * 2.0;

        let center = [x + w * 0.5, y + h * 0.5];
        let half = [w * 0.5, h * 0.5];
        let c = [color.r, color.g, color.b, color.a];

        let base = self.chrome_rect_vertices.len() as u32;
        let vert = |px: f32, py: f32| ChromeRectVertex {
            position: [px, py],
            color: c,
            rect_center: center,
            rect_half: half,
            corner_radius: r,
            shadow_blur: 0.0,
        };
        self.chrome_rect_vertices.push(vert(qx, qy));
        self.chrome_rect_vertices.push(vert(qx + qw, qy));
        self.chrome_rect_vertices.push(vert(qx + qw, qy + qh));
        self.chrome_rect_vertices.push(vert(qx, qy + qh));
        self.chrome_rect_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Draw a soft outer shadow for a rounded rect (SDF-based blur).
    /// `blur` = blur radius in logical pixels, `spread` = expand/shrink of shadow shape.
    pub fn draw_chrome_shadow(&mut self, rect: Rect, color: Color, radius: f32, blur: f32, spread: f32) {
        let s = self.scale_factor;
        // Shadow shape = rect adjusted by spread (negative = smaller)
        let sx = (rect.x - spread) * s;
        let sy = (rect.y - spread) * s;
        let sw = (rect.width + spread * 2.0) * s;
        let sh = (rect.height + spread * 2.0) * s;
        let sr = radius * s;
        let sb = blur * s;

        // Expand quad by blur radius so the soft falloff is fully visible
        let expand = sb + 2.0;
        let qx = sx - expand;
        let qy = sy - expand;
        let qw = sw + expand * 2.0;
        let qh = sh + expand * 2.0;

        let center = [sx + sw * 0.5, sy + sh * 0.5];
        let half = [sw * 0.5, sh * 0.5];
        let c = [color.r, color.g, color.b, color.a];

        let base = self.chrome_rect_vertices.len() as u32;
        let vert = |px: f32, py: f32| ChromeRectVertex {
            position: [px, py],
            color: c,
            rect_center: center,
            rect_half: half,
            corner_radius: sr,
            shadow_blur: sb,
        };
        self.chrome_rect_vertices.push(vert(qx, qy));
        self.chrome_rect_vertices.push(vert(qx + qw, qy));
        self.chrome_rect_vertices.push(vert(qx + qw, qy + qh));
        self.chrome_rect_vertices.push(vert(qx, qy + qh));
        self.chrome_rect_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Draw text into the cached chrome layer.
    pub fn draw_chrome_text(&mut self, text: &str, position: Vec2, style: TextStyle, clip: Rect) {
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
            let char_cells = ch.width().unwrap_or(1) as f32;

            if ch == ' ' || ch == '\t' {
                let advance = if ch == '\t' { cell_w * 4.0 } else { cell_w };
                cursor_x += advance;
                continue;
            }

            if let Some(bg) = style.background {
                let qx = cursor_x;
                let qy = start_y;
                let qw = cell_w * char_cells;
                let qh = self.cached_cell_size.height * scale;
                if qx + qw > clip_left && qx < clip_right && qy + qh > clip_top && qy < clip_bottom {
                    let base = self.chrome_rect_vertices.len() as u32;
                    let c = [bg.r, bg.g, bg.b, bg.a];
                    let center = [qx + qw * 0.5, qy + qh * 0.5];
                    let half = [qw * 0.5, qh * 0.5];
                    let vert = |px: f32, py: f32| ChromeRectVertex {
                        position: [px, py],
                        color: c,
                        rect_center: center,
                        rect_half: half,
                        corner_radius: 0.0,
                        shadow_blur: 0.0,
                    };
                    self.chrome_rect_vertices.push(vert(qx, qy));
                    self.chrome_rect_vertices.push(vert(qx + qw, qy));
                    self.chrome_rect_vertices.push(vert(qx + qw, qy + qh));
                    self.chrome_rect_vertices.push(vert(qx, qy + qh));
                    self.chrome_rect_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                }
            }

            let region = self.ensure_glyph_cached(ch, style.bold, style.italic);

            if region.width > 0 && region.height > 0 {
                let gx = cursor_x + region.left;
                let gy = start_y + baseline_y - region.top;
                let gw = region.width as f32;
                let gh = region.height as f32;

                if gx + gw > clip_left && gx + gw <= clip_right && gy + gh > clip_top && gy < clip_bottom {
                    let base = self.chrome_glyph_vertices.len() as u32;
                    let c = [style.foreground.r, style.foreground.g, style.foreground.b, style.foreground.a];
                    self.chrome_glyph_vertices.push(GlyphVertex { position: [gx, gy], uv: [region.uv_min[0], region.uv_min[1]], color: c });
                    self.chrome_glyph_vertices.push(GlyphVertex { position: [gx + gw, gy], uv: [region.uv_max[0], region.uv_min[1]], color: c });
                    self.chrome_glyph_vertices.push(GlyphVertex { position: [gx + gw, gy + gh], uv: [region.uv_max[0], region.uv_max[1]], color: c });
                    self.chrome_glyph_vertices.push(GlyphVertex { position: [gx, gy + gh], uv: [region.uv_min[0], region.uv_max[1]], color: c });
                    self.chrome_glyph_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
                }
            }

            cursor_x += cell_w * char_cells;
        }
    }

    /// Signal that chrome content has changed and needs a full rebuild.
    pub fn invalidate_chrome(&mut self) {
        self.chrome_rect_vertices.clear();
        self.chrome_rect_indices.clear();
        self.chrome_glyph_vertices.clear();
        self.chrome_glyph_indices.clear();
        self.chrome_needs_upload = true;
    }
}
