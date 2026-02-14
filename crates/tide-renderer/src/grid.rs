use tide_core::{Color, Rect, Size, TextStyle, Vec2};

use crate::vertex::{GlyphVertex, RectVertex};
use crate::WgpuRenderer;

#[derive(Default)]
pub struct PaneGridCache {
    pub rect_vertices: Vec<RectVertex>,
    pub rect_indices: Vec<u32>,
    pub glyph_vertices: Vec<GlyphVertex>,
    pub glyph_indices: Vec<u32>,
}

impl PaneGridCache {
    fn clear(&mut self) {
        self.rect_vertices.clear();
        self.rect_indices.clear();
        self.glyph_vertices.clear();
        self.glyph_indices.clear();
    }
}

impl WgpuRenderer {
    // ── Per-pane cache API ──────────────────────────────────

    /// Begin recording grid draw calls for a specific pane.
    pub fn begin_pane_grid(&mut self, pane_id: u64) {
        self.active_pane_cache.clear();
        self.active_pane_id = Some(pane_id);
    }

    /// Finish recording and store the pane's cache.
    pub fn end_pane_grid(&mut self) {
        if let Some(id) = self.active_pane_id.take() {
            let mut cache = self.pane_grid_caches.remove(&id).unwrap_or_default();
            std::mem::swap(&mut cache, &mut self.active_pane_cache);
            self.pane_grid_caches.insert(id, cache);
        }
    }

    /// Remove a pane's cached vertices (call when pane is closed).
    pub fn remove_pane_cache(&mut self, pane_id: u64) {
        self.pane_grid_caches.remove(&pane_id);
    }

    /// Invalidate all per-pane caches (atlas reset, scale change, etc.).
    pub fn invalidate_all_pane_caches(&mut self) {
        self.pane_grid_caches.clear();
    }

    /// Assemble all per-pane caches into the global grid arrays, in the given order.
    /// Also removes caches for panes not in the order list.
    pub fn assemble_grid(&mut self, pane_order: &[u64]) {
        self.grid_rect_vertices.clear();
        self.grid_rect_indices.clear();
        self.grid_glyph_vertices.clear();
        self.grid_glyph_indices.clear();

        for &id in pane_order {
            if let Some(cache) = self.pane_grid_caches.get(&id) {
                let rect_base = self.grid_rect_vertices.len() as u32;
                self.grid_rect_vertices.extend_from_slice(&cache.rect_vertices);
                self.grid_rect_indices.extend(cache.rect_indices.iter().map(|i| i + rect_base));

                let glyph_base = self.grid_glyph_vertices.len() as u32;
                self.grid_glyph_vertices.extend_from_slice(&cache.glyph_vertices);
                self.grid_glyph_indices.extend(cache.glyph_indices.iter().map(|i| i + glyph_base));
            }
        }

        // Remove stale caches for panes no longer in the order
        self.pane_grid_caches.retain(|id, _| pane_order.contains(id));

        self.grid_needs_upload = true;
    }

    // ── Existing API ────────────────────────────────────────

    /// Draw a rect into the cached grid layer (or active pane cache).
    pub fn draw_grid_rect(&mut self, rect: Rect, color: Color) {
        let x = rect.x * self.scale_factor;
        let y = rect.y * self.scale_factor;
        let w = rect.width * self.scale_factor;
        let h = rect.height * self.scale_factor;
        let c = [color.r, color.g, color.b, color.a];

        let (rv, ri) = if self.active_pane_id.is_some() {
            (&mut self.active_pane_cache.rect_vertices, &mut self.active_pane_cache.rect_indices)
        } else {
            (&mut self.grid_rect_vertices, &mut self.grid_rect_indices)
        };

        let base = rv.len() as u32;
        rv.push(RectVertex { position: [x, y], color: c });
        rv.push(RectVertex { position: [x + w, y], color: c });
        rv.push(RectVertex { position: [x + w, y + h], color: c });
        rv.push(RectVertex { position: [x, y + h], color: c });
        ri.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
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

    /// Draw a cell into the cached grid layer (or active pane cache).
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

        // Cache glyph first (needs &mut self for font system)
        let glyph_region = if character != ' ' && character != '\0' {
            let region = self.ensure_glyph_cached(character, style.bold, style.italic);
            if region.width > 0 && region.height > 0 {
                Some(region)
            } else {
                None
            }
        } else {
            None
        };

        // Determine target arrays
        let (rv, ri, gv, gi) = if self.active_pane_id.is_some() {
            (
                &mut self.active_pane_cache.rect_vertices,
                &mut self.active_pane_cache.rect_indices,
                &mut self.active_pane_cache.glyph_vertices,
                &mut self.active_pane_cache.glyph_indices,
            )
        } else {
            (
                &mut self.grid_rect_vertices,
                &mut self.grid_rect_indices,
                &mut self.grid_glyph_vertices,
                &mut self.grid_glyph_indices,
            )
        };

        // Draw background
        if let Some(bg) = style.background {
            let base = rv.len() as u32;
            let c = [bg.r, bg.g, bg.b, bg.a];
            rv.push(RectVertex { position: [px, py], color: c });
            rv.push(RectVertex { position: [px + cw, py], color: c });
            rv.push(RectVertex { position: [px + cw, py + ch], color: c });
            rv.push(RectVertex { position: [px, py + ch], color: c });
            ri.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }

        // Draw character
        if let Some(region) = glyph_region {
            let baseline_y = ch * 0.8;
            let gx = px + region.left;
            let gy = py + baseline_y - region.top;
            let gw = region.width as f32;
            let gh = region.height as f32;
            let c = [style.foreground.r, style.foreground.g, style.foreground.b, style.foreground.a];

            let base = gv.len() as u32;
            gv.push(GlyphVertex { position: [gx, gy], uv: [region.uv_min[0], region.uv_min[1]], color: c });
            gv.push(GlyphVertex { position: [gx + gw, gy], uv: [region.uv_max[0], region.uv_min[1]], color: c });
            gv.push(GlyphVertex { position: [gx + gw, gy + gh], uv: [region.uv_max[0], region.uv_max[1]], color: c });
            gv.push(GlyphVertex { position: [gx, gy + gh], uv: [region.uv_min[0], region.uv_max[1]], color: c });
            gi.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
        }
    }
}
