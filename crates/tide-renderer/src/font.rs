use cosmic_text::{
    Attrs, Buffer as CosmicBuffer, Family, FontSystem, Metrics, Shaping,
};
use tide_core::Size;

use crate::atlas::{AtlasRegion, GlyphCacheKey};
use crate::WgpuRenderer;

impl WgpuRenderer {
    pub(crate) fn compute_cell_size(font_system: &mut FontSystem, scale_factor: f32) -> Size {
        let font_size = 14.0 * scale_factor;
        let line_height = (font_size * 1.2).ceil();
        let metrics = Metrics::new(font_size, line_height);

        // Create a buffer to measure a single character
        let mut buffer = CosmicBuffer::new(font_system, metrics);
        buffer.set_text(
            font_system,
            "M",
            Attrs::new().family(Family::Monospace),
            Shaping::Advanced,
        );
        buffer.shape_until_scroll(font_system, false);

        // Get the advance width from layout
        let cell_width = buffer
            .layout_runs()
            .next()
            .and_then(|run| run.glyphs.first())
            .map(|g| g.w)
            .unwrap_or(font_size * 0.6);

        Size::new(cell_width / scale_factor, line_height / scale_factor)
    }

    /// Rasterize and cache a glyph, returning its atlas region.
    pub(crate) fn ensure_glyph_cached(&mut self, character: char, bold: bool, italic: bool) -> AtlasRegion {
        let key = GlyphCacheKey {
            character,
            bold,
            italic,
        };

        if let Some(region) = self.atlas.cache.get(&key) {
            return *region;
        }

        let font_size = 14.0 * self.scale_factor;
        let line_height = (font_size * 1.2).ceil();
        let metrics = Metrics::new(font_size, line_height);

        // Build attrs
        let mut attrs = Attrs::new().family(Family::Monospace);
        if bold {
            attrs = attrs.weight(cosmic_text::Weight::BOLD);
        }
        if italic {
            attrs = attrs.style(cosmic_text::Style::Italic);
        }

        // Shape the character
        let mut buffer = CosmicBuffer::new(&mut self.font_system, metrics);
        let text = character.to_string();
        buffer.set_text(&mut self.font_system, &text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        // Try to rasterize using swash
        let mut region = AtlasRegion {
            uv_min: [0.0, 0.0],
            uv_max: [0.0, 0.0],
            width: 0,
            height: 0,
            left: 0.0,
            top: 0.0,
        };

        if let Some(run) = buffer.layout_runs().next() {
            if let Some(glyph) = run.glyphs.first() {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                if let Some(image) = self
                    .swash_cache
                    .get_image(&mut self.font_system, physical.cache_key)
                {
                    let width = image.placement.width;
                    let height = image.placement.height;
                    let left = image.placement.left as f32;
                    let top = image.placement.top as f32;

                    if width > 0 && height > 0 {
                        // Convert to single-channel alpha if needed
                        let alpha_data: Vec<u8> = match image.content {
                            cosmic_text::SwashContent::Mask => image.data.clone(),
                            cosmic_text::SwashContent::Color => {
                                // RGBA -> take alpha channel
                                image.data.chunks(4).map(|c| c.get(3).copied().unwrap_or(255)).collect()
                            }
                            cosmic_text::SwashContent::SubpixelMask => {
                                // RGB subpixel -> average as grayscale
                                image.data.chunks(3).map(|c| {
                                    let r = c.first().copied().unwrap_or(0) as u16;
                                    let g = c.get(1).copied().unwrap_or(0) as u16;
                                    let b = c.get(2).copied().unwrap_or(0) as u16;
                                    ((r + g + b) / 3) as u8
                                }).collect()
                            }
                        };

                        let cache_len_before = self.atlas.cache.len();
                        region = self.atlas.upload_glyph(
                            &self.queue,
                            width,
                            height,
                            left,
                            top,
                            &alpha_data,
                        );
                        // Detect atlas reset: cache was cleared during upload
                        if self.atlas.cache.is_empty() && cache_len_before > 0 {
                            self.atlas_reset_count += 1;
                            self.grid_needs_upload = true;
                            self.chrome_needs_upload = true;
                        }
                    }
                }
            }
        }

        self.atlas.cache.insert(key, region);
        region
    }
}
