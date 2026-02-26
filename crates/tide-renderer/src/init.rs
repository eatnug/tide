use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use tide_core::{Color, Size};

use crate::atlas::GlyphAtlas;
use crate::grid::PaneGridCache;
use crate::msdf::MsdfFontStore;
use crate::shaders::{CHROME_RECT_SHADER, GRID_BG_INSTANCED_SHADER, RECT_SHADER};
use crate::vertex::{ChromeRectVertex, GlyphVertex, GridBgInstance, GridGlyphInstance, RectVertex};
use crate::WgpuRenderer;

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

        // --- Chrome rounded rect pipeline (SDF) ---
        let chrome_rect_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("chrome_rect_shader"),
            source: wgpu::ShaderSource::Wgsl(CHROME_RECT_SHADER.into()),
        });

        let chrome_rect_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("chrome_rect_pipeline_layout"),
                bind_group_layouts: &[&uniform_bind_group_layout],
                push_constant_ranges: &[],
            });

        let chrome_rounded_pipeline =
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("chrome_rounded_pipeline"),
                layout: Some(&chrome_rect_pipeline_layout),
                vertex: wgpu::VertexState {
                    module: &chrome_rect_shader,
                    entry_point: Some("vs_main"),
                    buffers: &[ChromeRectVertex::LAYOUT],
                    compilation_options: Default::default(),
                },
                fragment: Some(wgpu::FragmentState {
                    module: &chrome_rect_shader,
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

        // --- Glyph Atlas (RGBA for MSDF) ---
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

        // --- Glyph pipeline (MSDF) ---
        let glyph_shader_src = crate::shaders::glyph_shader();
        let glyph_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("glyph_shader"),
            source: wgpu::ShaderSource::Wgsl(glyph_shader_src.into()),
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

        // --- Instanced grid bg pipeline ---
        let grid_bg_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid_bg_instanced_shader"),
            source: wgpu::ShaderSource::Wgsl(GRID_BG_INSTANCED_SHADER.into()),
        });

        let grid_bg_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("grid_bg_pipeline_layout"),
                bind_group_layouts: &[&uniform_bind_group_layout],
                push_constant_ranges: &[],
            });

        let grid_bg_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid_bg_pipeline"),
            layout: Some(&grid_bg_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &grid_bg_shader,
                entry_point: Some("vs_main"),
                buffers: &[GridBgInstance::LAYOUT],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &grid_bg_shader,
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

        // --- Instanced grid glyph pipeline (MSDF) ---
        let grid_glyph_shader_src = crate::shaders::grid_glyph_instanced_shader();
        let grid_glyph_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("grid_glyph_instanced_shader"),
            source: wgpu::ShaderSource::Wgsl(grid_glyph_shader_src.into()),
        });

        let grid_glyph_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("grid_glyph_pipeline_layout"),
                bind_group_layouts: &[&uniform_bind_group_layout, &atlas_bind_group_layout],
                push_constant_ranges: &[],
            });

        let grid_glyph_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("grid_glyph_pipeline"),
            layout: Some(&grid_glyph_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &grid_glyph_shader,
                entry_point: Some("vs_main"),
                buffers: &[GridGlyphInstance::LAYOUT],
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &grid_glyph_shader,
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
        let mut font_system = cosmic_text::FontSystem::new();

        // Precompute cell sizes for all font sizes (8..=32) and look up initial
        let cell_size_table = Self::precompute_cell_sizes(&mut font_system, scale_factor);
        let cached_cell_size = cell_size_table[(14 - 8) as usize];

        // --- MSDF font store ---
        // Resolve the exact monospace font that cosmic-text uses, so MSDF rendering
        // matches the cell size measurement pixel-for-pixel.
        let mut msdf_font_store = MsdfFontStore::new();
        let mut mono_em_ascender = 0.8_f32;
        let mut mono_em_descender = 0.2_f32;

        // Shape a test character to discover cosmic-text's resolved font face
        let resolve_face = |font_system: &mut cosmic_text::FontSystem, bold: bool| -> Option<fontdb::ID> {
            use cosmic_text::{
                Attrs, Buffer as CosmicBuffer, Family, Metrics, Shaping,
            };
            let metrics = Metrics::new(14.0, 16.8);
            let mut buffer = CosmicBuffer::new(font_system, metrics);
            let attrs = if bold {
                Attrs::new().family(Family::Monospace).weight(cosmic_text::Weight::BOLD)
            } else {
                Attrs::new().family(Family::Monospace)
            };
            buffer.set_text(font_system, "M", attrs, Shaping::Advanced);
            buffer.shape_until_scroll(font_system, false);
            buffer.layout_runs()
                .next()
                .and_then(|run| run.glyphs.first())
                .map(|g| g.font_id)
        };

        // Register exact font faces for regular and bold
        for bold in [false, true] {
            if let Some(face_id) = resolve_face(&mut font_system, bold) {
                font_system.db().with_face_data(face_id, |data, index| {
                    msdf_font_store.register_font("Monospace", bold, false, data.to_vec(), index);
                });
            }
        }

        // Extract baseline metrics from the regular monospace font
        if let Some((asc, desc)) = msdf_font_store.font_metrics("Monospace", false, false) {
            mono_em_ascender = asc;
            mono_em_descender = desc;
        }

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
            chrome_rounded_pipeline,
            glyph_pipeline,
            grid_bg_pipeline,
            grid_glyph_pipeline,
            uniform_buffer,
            uniform_bind_group,
            atlas,
            atlas_bind_group,
            font_system,
            msdf_font_store,
            // Per-pane grid caching
            pane_grid_caches: HashMap::new(),
            active_pane_cache: PaneGridCache::default(),
            active_pane_id: None,
            // Grid layer (instanced)
            grid_bg_instances: Vec::with_capacity(4096),
            grid_glyph_instances: Vec::with_capacity(8192),
            grid_needs_upload: true,
            grid_bg_inst_buf: create_buf("grid_bg_inst_buf", vb_usage),
            grid_glyph_inst_buf: create_buf("grid_glyph_inst_buf", vb_usage),
            grid_bg_inst_buf_capacity: initial_buf_size as usize,
            grid_glyph_inst_buf_capacity: initial_buf_size as usize,
            // Chrome layer (cached for borders and file tree)
            chrome_rect_vertices: Vec::with_capacity(4096),
            chrome_rect_indices: Vec::with_capacity(6144),
            chrome_glyph_vertices: Vec::with_capacity(8192),
            chrome_glyph_indices: Vec::with_capacity(12288),
            chrome_needs_upload: true,
            chrome_rect_vb: create_buf("chrome_rect_vb", vb_usage),
            chrome_rect_ib: create_buf("chrome_rect_ib", ib_usage),
            chrome_glyph_vb: create_buf("chrome_glyph_vb", vb_usage),
            chrome_glyph_ib: create_buf("chrome_glyph_ib", ib_usage),
            chrome_rect_vb_capacity: initial_buf_size as usize,
            chrome_rect_ib_capacity: initial_buf_size as usize,
            chrome_glyph_vb_capacity: initial_buf_size as usize,
            chrome_glyph_ib_capacity: initial_buf_size as usize,
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
            // Top layer (rendered last — opaque UI like search bar)
            top_rect_vertices: Vec::with_capacity(256),
            top_rect_indices: Vec::with_capacity(384),
            top_rounded_rect_vertices: Vec::with_capacity(256),
            top_rounded_rect_indices: Vec::with_capacity(384),
            top_glyph_vertices: Vec::with_capacity(512),
            top_glyph_indices: Vec::with_capacity(768),
            top_rect_vb: create_buf("top_rect_vb", vb_usage),
            top_rect_ib: create_buf("top_rect_ib", ib_usage),
            top_rounded_rect_vb: create_buf("top_rounded_rect_vb", vb_usage),
            top_rounded_rect_ib: create_buf("top_rounded_rect_ib", ib_usage),
            top_glyph_vb: create_buf("top_glyph_vb", vb_usage),
            top_glyph_ib: create_buf("top_glyph_ib", ib_usage),
            top_rect_vb_capacity: initial_buf_size as usize,
            top_rect_ib_capacity: initial_buf_size as usize,
            top_rounded_rect_vb_capacity: initial_buf_size as usize,
            top_rounded_rect_ib_capacity: initial_buf_size as usize,
            top_glyph_vb_capacity: initial_buf_size as usize,
            top_glyph_ib_capacity: initial_buf_size as usize,
            screen_size: Size::new(800.0, 600.0),
            scale_factor,
            base_font_size: 14.0,
            cached_cell_size,
            cell_size_table,
            mono_em_ascender,
            mono_em_descender,
            surface_format: format,
            clear_color: Color::new(0.02, 0.02, 0.02, 1.0),
            // Incremental grid assembly
            pane_grid_ranges: HashMap::new(),
            last_pane_order: Vec::new(),
            grid_dirty_panes: HashSet::new(),
            grid_partial_uploads: Vec::new(),
            atlas_reset_count: 0,
            last_atlas_reset_count: 0,
            last_uniform_screen: [0.0, 0.0],
            device: Arc::clone(&device),
            queue: Arc::clone(&queue),
        }
    }
}
