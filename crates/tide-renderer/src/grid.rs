use tide_core::{Color, Rect, Size, TextStyle, Vec2};

use crate::vertex::{GlyphVertex, RectVertex};
use crate::WgpuRenderer;

impl WgpuRenderer {
    /// Draw a rect into the cached grid layer.
    pub fn draw_grid_rect(&mut self, rect: Rect, color: Color) {
        let x = rect.x * self.scale_factor;
        let y = rect.y * self.scale_factor;
        let w = rect.width * self.scale_factor;
        let h = rect.height * self.scale_factor;
        let base = self.grid_rect_vertices.len() as u32;
        let c = [color.r, color.g, color.b, color.a];
        self.grid_rect_vertices.push(RectVertex { position: [x, y], color: c });
        self.grid_rect_vertices.push(RectVertex { position: [x + w, y], color: c });
        self.grid_rect_vertices.push(RectVertex { position: [x + w, y + h], color: c });
        self.grid_rect_vertices.push(RectVertex { position: [x, y + h], color: c });
        self.grid_rect_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }

    /// Check if the atlas was reset since last check (all UV coords are stale).
    pub fn atlas_was_reset(&mut self) -> bool {
        let prev = self.last_atlas_reset_count;
        self.last_atlas_reset_count = self.atlas_reset_count;
        prev != self.atlas_reset_count
    }

    /// Signal that the grid content has changed and needs a full rebuild.
    pub fn invalidate_grid(&mut self) {
        self.grid_rect_vertices.clear();
        self.grid_rect_indices.clear();
        self.grid_glyph_vertices.clear();
        self.grid_glyph_indices.clear();
        self.grid_needs_upload = true;
    }

    /// Draw a cell into the cached grid layer.
    pub fn draw_grid_cell(
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

        // Draw background into grid layer
        if let Some(bg) = style.background {
            let base = self.grid_rect_vertices.len() as u32;
            let c = [bg.r, bg.g, bg.b, bg.a];
            self.grid_rect_vertices.push(RectVertex { position: [px, py], color: c });
            self.grid_rect_vertices.push(RectVertex { position: [px + cw, py], color: c });
            self.grid_rect_vertices.push(RectVertex { position: [px + cw, py + ch], color: c });
            self.grid_rect_vertices.push(RectVertex { position: [px, py + ch], color: c });
            self.grid_rect_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }

        // Draw character into grid layer
        if character != ' ' && character != '\0' {
            let region = self.ensure_glyph_cached(character, style.bold, style.italic);
            if region.width > 0 && region.height > 0 {
                let baseline_y = ch * 0.8;
                let gx = px + region.left;
                let gy = py + baseline_y - region.top;
                let gw = region.width as f32;
                let gh = region.height as f32;
                let c = [style.foreground.r, style.foreground.g, style.foreground.b, style.foreground.a];

                let base = self.grid_glyph_vertices.len() as u32;
                self.grid_glyph_vertices.push(GlyphVertex { position: [gx, gy], uv: [region.uv_min[0], region.uv_min[1]], color: c });
                self.grid_glyph_vertices.push(GlyphVertex { position: [gx + gw, gy], uv: [region.uv_max[0], region.uv_min[1]], color: c });
                self.grid_glyph_vertices.push(GlyphVertex { position: [gx + gw, gy + gh], uv: [region.uv_max[0], region.uv_max[1]], color: c });
                self.grid_glyph_vertices.push(GlyphVertex { position: [gx, gy + gh], uv: [region.uv_min[0], region.uv_max[1]], color: c });
                self.grid_glyph_indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
            }
        }
    }
}
