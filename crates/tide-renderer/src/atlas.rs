// Glyph atlas: texture packing and cache for rasterized glyphs.

use std::collections::HashMap;

/// Region in the atlas texture for a single glyph
#[derive(Debug, Clone, Copy)]
pub struct AtlasRegion {
    /// UV coords in [0,1] range
    pub uv_min: [f32; 2],
    pub uv_max: [f32; 2],
    /// Pixel size of the glyph image
    pub width: u32,
    pub height: u32,
    /// Offset from the baseline/origin
    pub left: f32,
    pub top: f32,
}

/// Key for glyph cache lookup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GlyphCacheKey {
    pub character: char,
    pub bold: bool,
    pub italic: bool,
}

pub const ATLAS_SIZE: u32 = 8192;

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
            format: wgpu::TextureFormat::R8Unorm,
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

    /// Upload a glyph bitmap into the atlas, returning the region.
    pub fn upload_glyph(
        &mut self,
        queue: &wgpu::Queue,
        width: u32,
        height: u32,
        left: f32,
        top: f32,
        data: &[u8],
    ) -> AtlasRegion {
        if width == 0 || height == 0 {
            return AtlasRegion {
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                width: 0,
                height: 0,
                left,
                top,
            };
        }

        // Move to next row if needed
        if self.cursor_x + width > ATLAS_SIZE {
            self.cursor_x = 0;
            self.cursor_y += self.row_height + 1;
            self.row_height = 0;
        }

        // If we've run out of space, reset and retry
        if self.cursor_y + height > ATLAS_SIZE {
            self.reset();
            // Re-check row wrap after reset
            if self.cursor_x + width > ATLAS_SIZE {
                self.cursor_x = 0;
                self.cursor_y += self.row_height + 1;
                self.row_height = 0;
            }
            // If a single glyph exceeds the entire atlas, give up
            if self.cursor_y + height > ATLAS_SIZE {
                log::error!("Single glyph exceeds atlas size");
                return AtlasRegion {
                    uv_min: [0.0, 0.0],
                    uv_max: [0.0, 0.0],
                    width: 0,
                    height: 0,
                    left,
                    top,
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
            data,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(width),
                rows_per_image: Some(height),
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );

        let uv_min = [x as f32 / ATLAS_SIZE as f32, y as f32 / ATLAS_SIZE as f32];
        let uv_max = [
            (x + width) as f32 / ATLAS_SIZE as f32,
            (y + height) as f32 / ATLAS_SIZE as f32,
        ];

        self.cursor_x += width + 1;
        if height > self.row_height {
            self.row_height = height;
        }

        AtlasRegion {
            uv_min,
            uv_max,
            width,
            height,
            left,
            top,
        }
    }
}
