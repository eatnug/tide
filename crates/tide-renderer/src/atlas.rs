// Glyph atlas: texture packing and cache for MSDF glyphs.

use std::collections::HashMap;

/// Region in the atlas texture for a single glyph
#[derive(Debug, Clone, Copy)]
pub struct AtlasRegion {
    /// UV coords in [0,1] range
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Glyph metrics in em-relative units.
    /// Multiply by (font_size * scale_factor) to get physical pixels.
    pub em_left: f32,
    pub em_top: f32,
    pub em_width: f32,
    pub em_height: f32,
}

impl AtlasRegion {
    pub fn is_empty(&self) -> bool {
        self.em_width <= 0.0 || self.em_height <= 0.0
    }
}

/// Key for glyph cache lookup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphCacheKey {
    pub character: char,
    pub bold: bool,
    pub italic: bool,
}

pub const ATLAS_SIZE: u32 = 4096;

pub struct GlyphAtlas {
    pub texture: wgpu::Texture,
    pub texture_view: wgpu::TextureView,
    /// Current packing cursor
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    /// Map from glyph key to atlas region
    pub cache: HashMap<GlyphCacheKey, AtlasRegion>,
}

impl GlyphAtlas {
    pub fn new(device: &wgpu::Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("glyph_atlas"),
            size: wgpu::Extent3d {
                width: ATLAS_SIZE,
                height: ATLAS_SIZE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let texture_view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        Self {
            texture,
            texture_view,
            cursor_x: 0,
            cursor_y: 0,
            row_height: 0,
            cache: HashMap::new(),
        }
    }

    /// Clear the atlas cache, allowing it to be repacked from scratch.
    pub fn reset(&mut self) {
        let count = self.cache.len();
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.row_height = 0;
        self.cache.clear();
        log::warn!("Glyph atlas full: cleared {count} cached glyphs");
    }

    /// Upload an MSDF glyph (RGBA data) into the atlas, returning the region.
    pub fn upload_glyph(
        &mut self,
        queue: &wgpu::Queue,
        texel_width: u32,
        texel_height: u32,
        em_left: f32,
        em_top: f32,
        em_width: f32,
        em_height: f32,
        rgba_data: &[u8],
    ) -> AtlasRegion {
        if texel_width == 0 || texel_height == 0 {
            return AtlasRegion {
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                em_left,
                em_top,
                em_width: 0.0,
                em_height: 0.0,
            };
        }

        // Move to next row if needed
        if self.cursor_x + texel_width > ATLAS_SIZE {
            self.cursor_x = 0;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }

        // If we've run out of space, reset and retry
        if self.cursor_y + texel_height > ATLAS_SIZE {
            self.reset();
            if self.cursor_x + texel_width > ATLAS_SIZE {
                self.cursor_x = 0;
                self.cursor_y += self.row_height + 1;
                self.row_height = 0;
            }
            if self.cursor_y + texel_height > ATLAS_SIZE {
                log::error!("Single glyph exceeds atlas size");
                return AtlasRegion {
                    uv_min: [0.0, 0.0],
                    uv_max: [0.0, 0.0],
                    em_left,
                    em_top,
                    em_width: 0.0,
                    em_height: 0.0,
                };
            }
        }

        let x = self.cursor_x;
        let y = self.cursor_y;

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d { x, y, z: 0 },
                aspect: wgpu::TextureAspect::All,
            },
            rgba_data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(texel_width * 4), // RGBA = 4 bytes per texel
                rows_per_image: Some(texel_height),
            },
            wgpu::Extent3d {
                width: texel_width,
                height: texel_height,
                depth_or_array_layers: 1,
            },
        );

        let uv_min = [
            x as f32 / ATLAS_SIZE as f32,
            y as f32 / ATLAS_SIZE as f32,
        ];
        let uv_max = [
            (x + texel_width) as f32 / ATLAS_SIZE as f32,
            (y + texel_height) as f32 / ATLAS_SIZE as f32,
        ];

        self.cursor_x += texel_width + 1;
        if texel_height > self.row_height {
            self.row_height = texel_height;
        }

        AtlasRegion {
            uv_min,
            uv_max,
            em_left,
            em_top,
            em_width,
            em_height,
        }
    }
}
