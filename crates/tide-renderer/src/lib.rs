// GPU renderer implementation (Stream A)
// Implements tide_core::Renderer using wgpu + cosmic-text

use std::collections::HashMap;
use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use cosmic_text::{
    Attrs, Buffer as CosmicBuffer, Family, FontSystem, Metrics, Shaping, SwashCache,
};
use tide_core::{Color, Rect, Renderer, Size, TextStyle, Vec2};
use wgpu::util::DeviceExt;

// ──────────────────────────────────────────────
// WGSL Shaders
// ──────────────────────────────────────────────

const RECT_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) color: vec4<f32>,
};

struct Uniforms {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    // Convert pixel coords to NDC: x: [0, width] -> [-1, 1], y: [0, height] -> [1, -1]
    let ndc_x = (in.position.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (in.position.y / uniforms.screen_size.y) * 2.0;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    return in.color;
}
"#;

const GLYPH_SHADER: &str = r#"
struct VertexInput {
    @location(0) position: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) color: vec4<f32>,
};

struct VertexOutput {
    @builtin(position) clip_position: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

struct Uniforms {
    screen_size: vec2<f32>,
};

@group(0) @binding(0)
var<uniform> uniforms: Uniforms;

@group(1) @binding(0)
var atlas_texture: texture_2d<f32>;
@group(1) @binding(1)
var atlas_sampler: sampler;

@vertex
fn vs_main(in: VertexInput) -> VertexOutput {
    var out: VertexOutput;
    let ndc_x = (in.position.x / uniforms.screen_size.x) * 2.0 - 1.0;
    let ndc_y = 1.0 - (in.position.y / uniforms.screen_size.y) * 2.0;
    out.clip_position = vec4<f32>(ndc_x, ndc_y, 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    return out;
}

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    let alpha = textureSample(atlas_texture, atlas_sampler, in.uv).r;
    return vec4<f32>(in.color.rgb, in.color.a * alpha);
}
"#;

// ──────────────────────────────────────────────
// Vertex types
// ──────────────────────────────────────────────

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct RectVertex {
    position: [f32; 2],
    color: [f32; 4],
}

impl RectVertex {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<RectVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x4,
            },
        ],
    };
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
struct GlyphVertex {
    position: [f32; 2],
    uv: [f32; 2],
    color: [f32; 4],
}

impl GlyphVertex {
    const LAYOUT: wgpu::VertexBufferLayout<'static> = wgpu::VertexBufferLayout {
        array_stride: std::mem::size_of::<GlyphVertex>() as wgpu::BufferAddress,
        step_mode: wgpu::VertexStepMode::Vertex,
        attributes: &[
            wgpu::VertexAttribute {
                offset: 0,
                shader_location: 0,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 8,
                shader_location: 1,
                format: wgpu::VertexFormat::Float32x2,
            },
            wgpu::VertexAttribute {
                offset: 16,
                shader_location: 2,
                format: wgpu::VertexFormat::Float32x4,
            },
        ],
    };
}

// ──────────────────────────────────────────────
// Glyph Atlas
// ──────────────────────────────────────────────

/// Region in the atlas texture for a single glyph
#[derive(Debug, Clone, Copy)]
struct AtlasRegion {
    /// UV coords in [0,1] range
    uv_min: [f32; 2],
    uv_max: [f32; 2],
    /// Pixel size of the glyph image
    width: u32,
    height: u32,
    /// Offset from the baseline/origin
    left: f32,
    top: f32,
}

/// Key for glyph cache lookup
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct GlyphCacheKey {
    character: char,
    bold: bool,
    italic: bool,
}

const ATLAS_SIZE: u32 = 1024;

struct GlyphAtlas {
    texture: wgpu::Texture,
    texture_view: wgpu::TextureView,
    /// Current packing cursor
    cursor_x: u32,
    cursor_y: u32,
    row_height: u32,
    /// Map from glyph key to atlas region
    cache: HashMap<GlyphCacheKey, AtlasRegion>,
}

impl GlyphAtlas {
    fn new(device: &wgpu::Device) -> Self {
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

    /// Upload a glyph bitmap into the atlas, returning the region.
    fn upload_glyph(
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

        // If we've run out of space, just return empty (in production, grow or use multiple atlases)
        if self.cursor_y + height > ATLAS_SIZE {
            return AtlasRegion {
                uv_min: [0.0, 0.0],
                uv_max: [0.0, 0.0],
                width: 0,
                height: 0,
                left,
                top,
            };
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

// ──────────────────────────────────────────────
// WgpuRenderer
// ──────────────────────────────────────────────

pub struct WgpuRenderer {
    // GPU pipelines
    rect_pipeline: wgpu::RenderPipeline,
    glyph_pipeline: wgpu::RenderPipeline,

    // Uniform buffer (screen size)
    uniform_buffer: wgpu::Buffer,
    uniform_bind_group: wgpu::BindGroup,

    // Atlas
    atlas: GlyphAtlas,
    atlas_bind_group: wgpu::BindGroup,

    // Text subsystem
    font_system: FontSystem,
    swash_cache: SwashCache,

    // Cached grid layer — only rebuilt when grid content changes
    grid_rect_vertices: Vec<RectVertex>,
    grid_rect_indices: Vec<u32>,
    grid_glyph_vertices: Vec<GlyphVertex>,
    grid_glyph_indices: Vec<u32>,
    grid_needs_upload: bool,

    // Grid GPU buffers
    grid_rect_vb: wgpu::Buffer,
    grid_rect_ib: wgpu::Buffer,
    grid_glyph_vb: wgpu::Buffer,
    grid_glyph_ib: wgpu::Buffer,
    grid_rect_vb_capacity: usize,
    grid_rect_ib_capacity: usize,
    grid_glyph_vb_capacity: usize,
    grid_glyph_ib_capacity: usize,

    // Overlay layer — rebuilt every frame (borders, cursor, file tree, preedit)
    rect_vertices: Vec<RectVertex>,
    rect_indices: Vec<u32>,
    glyph_vertices: Vec<GlyphVertex>,
    glyph_indices: Vec<u32>,

    // Overlay GPU buffers
    rect_vb: wgpu::Buffer,
    rect_ib: wgpu::Buffer,
    glyph_vb: wgpu::Buffer,
    glyph_ib: wgpu::Buffer,
    rect_vb_capacity: usize,
    rect_ib_capacity: usize,
    glyph_vb_capacity: usize,
    glyph_ib_capacity: usize,

    // Current frame state
    screen_size: Size,
    scale_factor: f32,

    // Cached cell metrics
    cached_cell_size: Size,

    // Surface format (for potential re-creation)
    #[allow(dead_code)]
    surface_format: wgpu::TextureFormat,

    // Store device and queue for uploading glyphs during draw calls
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
}

impl WgpuRenderer {
    pub fn new(
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
        format: wgpu::TextureFormat,
        scale_factor: f32,
    ) -> Self {
        // --- Uniform buffer ---
        let uniform_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("uniform_buffer"),
            size: 16, // vec2<f32> padded to 16 bytes
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // --- Uniform bind group layout ---
        let uniform_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("uniform_bgl"),
                entries: &[wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                }],
            });

        let uniform_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("uniform_bg"),
            layout: &uniform_bind_group_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: uniform_buffer.as_entire_binding(),
            }],
        });

        // --- Rect pipeline ---
        let rect_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("rect_shader"),
            source: wgpu::ShaderSource::Wgsl(RECT_SHADER.into()),
        });

        let rect_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("rect_pipeline_layout"),
                bind_group_layouts: &[&uniform_bind_group_layout],
                push_constant_ranges: &[],
            });

        let rect_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("rect_pipeline"),
            layout: Some(&rect_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &rect_shader,
                entry_point: Some("vs_main"),
                buffers: &[RectVertex::LAYOUT],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &rect_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- Glyph Atlas ---
        let atlas = GlyphAtlas::new(&device);

        let atlas_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("atlas_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });

        let atlas_bind_group_layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
                label: Some("atlas_bgl"),
                entries: &[
                    wgpu::BindGroupLayoutEntry {
                        binding: 0,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Texture {
                            sample_type: wgpu::TextureSampleType::Float { filterable: true },
                            view_dimension: wgpu::TextureViewDimension::D2,
                            multisampled: false,
                        },
                        count: None,
                    },
                    wgpu::BindGroupLayoutEntry {
                        binding: 1,
                        visibility: wgpu::ShaderStages::FRAGMENT,
                        ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                        count: None,
                    },
                ],
            });

        let atlas_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("atlas_bg"),
            layout: &atlas_bind_group_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&atlas.texture_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&atlas_sampler),
                },
            ],
        });

        // --- Glyph pipeline ---
        let glyph_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_shader"),
            source: wgpu::ShaderSource::Wgsl(GLYPH_SHADER.into()),
        });

        let glyph_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("glyph_pipeline_layout"),
                bind_group_layouts: &[&uniform_bind_group_layout, &atlas_bind_group_layout],
                push_constant_ranges: &[],
            });

        let glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("glyph_pipeline"),
            layout: Some(&glyph_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &glyph_shader,
                entry_point: Some("vs_main"),
                buffers: &[GlyphVertex::LAYOUT],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &glyph_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                strip_index_format: None,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                polygon_mode: wgpu::PolygonMode::Fill,
                unclipped_depth: false,
                conservative: false,
            },
            depth_stencil: None,
            multisample: wgpu::MultisampleState::default(),
            multiview: None,
            cache: None,
        });

        // --- Font system ---
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

        // Compute cell size from the monospace font metrics
        let cached_cell_size = Self::compute_cell_size(&mut font_system, scale_factor);

        // Pre-allocate GPU buffers (64KB initial, will grow as needed)
        let initial_buf_size: u64 = 64 * 1024;
        let create_buf = |label: &str, usage| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: initial_buf_size,
                usage,
                mapped_at_creation: false,
            })
        };
        let vb_usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
        let ib_usage = wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST;

        Self {
            rect_pipeline,
            glyph_pipeline,
            uniform_buffer,
            uniform_bind_group,
            atlas,
            atlas_bind_group,
            font_system,
            swash_cache,
            // Grid layer (cached)
            grid_rect_vertices: Vec::with_capacity(8192),
            grid_rect_indices: Vec::with_capacity(12288),
            grid_glyph_vertices: Vec::with_capacity(16384),
            grid_glyph_indices: Vec::with_capacity(24576),
            grid_needs_upload: true,
            grid_rect_vb: create_buf("grid_rect_vb", vb_usage),
            grid_rect_ib: create_buf("grid_rect_ib", ib_usage),
            grid_glyph_vb: create_buf("grid_glyph_vb", vb_usage),
            grid_glyph_ib: create_buf("grid_glyph_ib", ib_usage),
            grid_rect_vb_capacity: initial_buf_size as usize,
            grid_rect_ib_capacity: initial_buf_size as usize,
            grid_glyph_vb_capacity: initial_buf_size as usize,
            grid_glyph_ib_capacity: initial_buf_size as usize,
            // Overlay layer (rebuilt every frame)
            rect_vertices: Vec::with_capacity(4096),
            rect_indices: Vec::with_capacity(6144),
            glyph_vertices: Vec::with_capacity(8192),
            glyph_indices: Vec::with_capacity(12288),
            rect_vb: create_buf("rect_vb", vb_usage),
            rect_ib: create_buf("rect_ib", ib_usage),
            glyph_vb: create_buf("glyph_vb", vb_usage),
            glyph_ib: create_buf("glyph_ib", ib_usage),
            rect_vb_capacity: initial_buf_size as usize,
            rect_ib_capacity: initial_buf_size as usize,
            glyph_vb_capacity: initial_buf_size as usize,
            glyph_ib_capacity: initial_buf_size as usize,
            screen_size: Size::new(800.0, 600.0),
            scale_factor,
            cached_cell_size,
            surface_format: format,
            device: Arc::clone(&device),
            queue: Arc::clone(&queue),
        }
    }

    fn compute_cell_size(font_system: &mut FontSystem, scale_factor: f32) -> Size {
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
    fn ensure_glyph_cached(&mut self, character: char, bold: bool, italic: bool) -> AtlasRegion {
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
                                    let r = c.get(0).copied().unwrap_or(0) as u16;
                                    let g = c.get(1).copied().unwrap_or(0) as u16;
                                    let b = c.get(2).copied().unwrap_or(0) as u16;
                                    ((r + g + b) / 3) as u8
                                }).collect()
                            }
                        };

                        region = self.atlas.upload_glyph(
                            &self.queue,
                            width,
                            height,
                            left,
                            top,
                            &alpha_data,
                        );
                    }
                }
            }
        }

        self.atlas.cache.insert(key, region);
        region
    }

    // ── Grid layer methods (cached) ────────────────

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

    // ── Overlay layer methods (rebuilt every frame) ──

    /// Push a colored quad (two triangles) into the rect batch.
    fn push_rect_quad(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
        let base = self.rect_vertices.len() as u32;
        let c = [color.r, color.g, color.b, color.a];

        self.rect_vertices.push(RectVertex {
            position: [x, y],
            color: c,
        });
        self.rect_vertices.push(RectVertex {
            position: [x + w, y],
            color: c,
        });
        self.rect_vertices.push(RectVertex {
            position: [x + w, y + h],
            color: c,
        });
        self.rect_vertices.push(RectVertex {
            position: [x, y + h],
            color: c,
        });

        self.rect_indices.push(base);
        self.rect_indices.push(base + 1);
        self.rect_indices.push(base + 2);
        self.rect_indices.push(base);
        self.rect_indices.push(base + 2);
        self.rect_indices.push(base + 3);
    }

    /// Push a textured glyph quad into the glyph batch.
    fn push_glyph_quad(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        uv_min: [f32; 2],
        uv_max: [f32; 2],
        color: Color,
    ) {
        let base = self.glyph_vertices.len() as u32;
        let c = [color.r, color.g, color.b, color.a];

        self.glyph_vertices.push(GlyphVertex {
            position: [x, y],
            uv: [uv_min[0], uv_min[1]],
            color: c,
        });
        self.glyph_vertices.push(GlyphVertex {
            position: [x + w, y],
            uv: [uv_max[0], uv_min[1]],
            color: c,
        });
        self.glyph_vertices.push(GlyphVertex {
            position: [x + w, y + h],
            uv: [uv_max[0], uv_max[1]],
            color: c,
        });
        self.glyph_vertices.push(GlyphVertex {
            position: [x, y + h],
            uv: [uv_min[0], uv_max[1]],
            color: c,
        });

        self.glyph_indices.push(base);
        self.glyph_indices.push(base + 1);
        self.glyph_indices.push(base + 2);
        self.glyph_indices.push(base);
        self.glyph_indices.push(base + 2);
        self.glyph_indices.push(base + 3);
    }

    /// Ensure a GPU buffer is large enough; grow if needed.
    fn ensure_buffer_capacity(
        device: &wgpu::Device,
        buf: &mut wgpu::Buffer,
        capacity: &mut usize,
        needed: usize,
        usage: wgpu::BufferUsages,
        label: &str,
    ) {
        if needed > *capacity {
            let new_cap = needed.next_power_of_two().max(64 * 1024);
            *buf = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: new_cap as u64,
                usage,
                mapped_at_creation: false,
            });
            *capacity = new_cap;
        }
    }

    /// Submit batched draw calls to a render pass.
    /// Draws: grid rects → overlay rects → grid glyphs → overlay glyphs
    pub fn render_frame(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let vb_usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
        let ib_usage = wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST;

        // Update uniform buffer with current screen size
        let screen_data = [
            self.screen_size.width * self.scale_factor,
            self.screen_size.height * self.scale_factor,
            0.0f32, 0.0f32,
        ];
        self.queue
            .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&screen_data));

        // ── Upload grid layer (only when content changed) ──
        if self.grid_needs_upload {
            if !self.grid_rect_vertices.is_empty() {
                let vb_bytes = bytemuck::cast_slice(&self.grid_rect_vertices);
                Self::ensure_buffer_capacity(&self.device, &mut self.grid_rect_vb, &mut self.grid_rect_vb_capacity, vb_bytes.len(), vb_usage, "grid_rect_vb");
                self.queue.write_buffer(&self.grid_rect_vb, 0, vb_bytes);
                let ib_bytes = bytemuck::cast_slice(&self.grid_rect_indices);
                Self::ensure_buffer_capacity(&self.device, &mut self.grid_rect_ib, &mut self.grid_rect_ib_capacity, ib_bytes.len(), ib_usage, "grid_rect_ib");
                self.queue.write_buffer(&self.grid_rect_ib, 0, ib_bytes);
            }
            if !self.grid_glyph_vertices.is_empty() {
                let vb_bytes = bytemuck::cast_slice(&self.grid_glyph_vertices);
                Self::ensure_buffer_capacity(&self.device, &mut self.grid_glyph_vb, &mut self.grid_glyph_vb_capacity, vb_bytes.len(), vb_usage, "grid_glyph_vb");
                self.queue.write_buffer(&self.grid_glyph_vb, 0, vb_bytes);
                let ib_bytes = bytemuck::cast_slice(&self.grid_glyph_indices);
                Self::ensure_buffer_capacity(&self.device, &mut self.grid_glyph_ib, &mut self.grid_glyph_ib_capacity, ib_bytes.len(), ib_usage, "grid_glyph_ib");
                self.queue.write_buffer(&self.grid_glyph_ib, 0, ib_bytes);
            }
            self.grid_needs_upload = false;
        }

        // ── Upload overlay layer (every frame) ──
        let has_overlay_rects = !self.rect_vertices.is_empty();
        let has_overlay_glyphs = !self.glyph_vertices.is_empty();

        if has_overlay_rects {
            let vb_bytes = bytemuck::cast_slice(&self.rect_vertices);
            Self::ensure_buffer_capacity(&self.device, &mut self.rect_vb, &mut self.rect_vb_capacity, vb_bytes.len(), vb_usage, "rect_vb");
            self.queue.write_buffer(&self.rect_vb, 0, vb_bytes);
            let ib_bytes = bytemuck::cast_slice(&self.rect_indices);
            Self::ensure_buffer_capacity(&self.device, &mut self.rect_ib, &mut self.rect_ib_capacity, ib_bytes.len(), ib_usage, "rect_ib");
            self.queue.write_buffer(&self.rect_ib, 0, ib_bytes);
        }

        if has_overlay_glyphs {
            let vb_bytes = bytemuck::cast_slice(&self.glyph_vertices);
            Self::ensure_buffer_capacity(&self.device, &mut self.glyph_vb, &mut self.glyph_vb_capacity, vb_bytes.len(), vb_usage, "glyph_vb");
            self.queue.write_buffer(&self.glyph_vb, 0, vb_bytes);
            let ib_bytes = bytemuck::cast_slice(&self.glyph_indices);
            Self::ensure_buffer_capacity(&self.device, &mut self.glyph_ib, &mut self.glyph_ib_capacity, ib_bytes.len(), ib_usage, "glyph_ib");
            self.queue.write_buffer(&self.glyph_ib, 0, ib_bytes);
        }

        let grid_rect_count = self.grid_rect_indices.len() as u32;
        let grid_glyph_count = self.grid_glyph_indices.len() as u32;
        let overlay_rect_count = self.rect_indices.len() as u32;
        let overlay_glyph_count = self.glyph_indices.len() as u32;

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color { r: 0.0, g: 0.0, b: 0.0, a: 1.0 }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw order: grid rects → overlay rects → grid glyphs → overlay glyphs
            pass.set_pipeline(&self.rect_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);

            if grid_rect_count > 0 {
                pass.set_vertex_buffer(0, self.grid_rect_vb.slice(..));
                pass.set_index_buffer(self.grid_rect_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..grid_rect_count, 0, 0..1);
            }

            if overlay_rect_count > 0 {
                pass.set_vertex_buffer(0, self.rect_vb.slice(..));
                pass.set_index_buffer(self.rect_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..overlay_rect_count, 0, 0..1);
            }

            pass.set_pipeline(&self.glyph_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);
            pass.set_bind_group(1, &self.atlas_bind_group, &[]);

            if grid_glyph_count > 0 {
                pass.set_vertex_buffer(0, self.grid_glyph_vb.slice(..));
                pass.set_index_buffer(self.grid_glyph_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..grid_glyph_count, 0, 0..1);
            }

            if overlay_glyph_count > 0 {
                pass.set_vertex_buffer(0, self.glyph_vb.slice(..));
                pass.set_index_buffer(self.glyph_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..overlay_glyph_count, 0, 0..1);
            }
        }
    }
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
