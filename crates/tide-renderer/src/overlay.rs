use tide_core::Color;

use crate::vertex::{GlyphVertex, RectVertex};
use crate::WgpuRenderer;

impl WgpuRenderer {
    /// Push a colored quad (two triangles) into the rect batch.
    pub(crate) fn push_rect_quad(&mut self, x: f32, y: f32, w: f32, h: f32, color: Color) {
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
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn push_glyph_quad(
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
    /// Draws: grid rects → chrome rects → overlay rects → grid glyphs → chrome glyphs → overlay glyphs → top rects → top glyphs
    pub fn render_frame(
        &mut self,
        encoder: &mut wgpu::CommandEncoder,
        view: &wgpu::TextureView,
    ) {
        let vb_usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
        let ib_usage = wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST;

        // Update uniform buffer only when screen size changed
        let screen_phys = [
            self.screen_size.width * self.scale_factor,
            self.screen_size.height * self.scale_factor,
        ];
        if screen_phys != self.last_uniform_screen {
            let screen_data = [screen_phys[0], screen_phys[1], 0.0f32, 0.0f32];
            self.queue
                .write_buffer(&self.uniform_buffer, 0, bytemuck::cast_slice(&screen_data));
            self.last_uniform_screen = screen_phys;
        }

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

        // ── Upload chrome layer (only when chrome changed) ──
        if self.chrome_needs_upload {
            if !self.chrome_rect_vertices.is_empty() {
                let vb_bytes = bytemuck::cast_slice(&self.chrome_rect_vertices);
                Self::ensure_buffer_capacity(&self.device, &mut self.chrome_rect_vb, &mut self.chrome_rect_vb_capacity, vb_bytes.len(), vb_usage, "chrome_rect_vb");
                self.queue.write_buffer(&self.chrome_rect_vb, 0, vb_bytes);
                let ib_bytes = bytemuck::cast_slice(&self.chrome_rect_indices);
                Self::ensure_buffer_capacity(&self.device, &mut self.chrome_rect_ib, &mut self.chrome_rect_ib_capacity, ib_bytes.len(), ib_usage, "chrome_rect_ib");
                self.queue.write_buffer(&self.chrome_rect_ib, 0, ib_bytes);
            }
            if !self.chrome_glyph_vertices.is_empty() {
                let vb_bytes = bytemuck::cast_slice(&self.chrome_glyph_vertices);
                Self::ensure_buffer_capacity(&self.device, &mut self.chrome_glyph_vb, &mut self.chrome_glyph_vb_capacity, vb_bytes.len(), vb_usage, "chrome_glyph_vb");
                self.queue.write_buffer(&self.chrome_glyph_vb, 0, vb_bytes);
                let ib_bytes = bytemuck::cast_slice(&self.chrome_glyph_indices);
                Self::ensure_buffer_capacity(&self.device, &mut self.chrome_glyph_ib, &mut self.chrome_glyph_ib_capacity, ib_bytes.len(), ib_usage, "chrome_glyph_ib");
                self.queue.write_buffer(&self.chrome_glyph_ib, 0, ib_bytes);
            }
            self.chrome_needs_upload = false;
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

        // ── Upload top layer (every frame) ──
        let has_top_rects = !self.top_rect_vertices.is_empty();
        let has_top_rounded_rects = !self.top_rounded_rect_vertices.is_empty();
        let has_top_glyphs = !self.top_glyph_vertices.is_empty();

        if has_top_rects {
            let vb_bytes = bytemuck::cast_slice(&self.top_rect_vertices);
            Self::ensure_buffer_capacity(&self.device, &mut self.top_rect_vb, &mut self.top_rect_vb_capacity, vb_bytes.len(), vb_usage, "top_rect_vb");
            self.queue.write_buffer(&self.top_rect_vb, 0, vb_bytes);
            let ib_bytes = bytemuck::cast_slice(&self.top_rect_indices);
            Self::ensure_buffer_capacity(&self.device, &mut self.top_rect_ib, &mut self.top_rect_ib_capacity, ib_bytes.len(), ib_usage, "top_rect_ib");
            self.queue.write_buffer(&self.top_rect_ib, 0, ib_bytes);
        }

        if has_top_rounded_rects {
            let vb_bytes = bytemuck::cast_slice(&self.top_rounded_rect_vertices);
            Self::ensure_buffer_capacity(&self.device, &mut self.top_rounded_rect_vb, &mut self.top_rounded_rect_vb_capacity, vb_bytes.len(), vb_usage, "top_rounded_rect_vb");
            self.queue.write_buffer(&self.top_rounded_rect_vb, 0, vb_bytes);
            let ib_bytes = bytemuck::cast_slice(&self.top_rounded_rect_indices);
            Self::ensure_buffer_capacity(&self.device, &mut self.top_rounded_rect_ib, &mut self.top_rounded_rect_ib_capacity, ib_bytes.len(), ib_usage, "top_rounded_rect_ib");
            self.queue.write_buffer(&self.top_rounded_rect_ib, 0, ib_bytes);
        }

        if has_top_glyphs {
            let vb_bytes = bytemuck::cast_slice(&self.top_glyph_vertices);
            Self::ensure_buffer_capacity(&self.device, &mut self.top_glyph_vb, &mut self.top_glyph_vb_capacity, vb_bytes.len(), vb_usage, "top_glyph_vb");
            self.queue.write_buffer(&self.top_glyph_vb, 0, vb_bytes);
            let ib_bytes = bytemuck::cast_slice(&self.top_glyph_indices);
            Self::ensure_buffer_capacity(&self.device, &mut self.top_glyph_ib, &mut self.top_glyph_ib_capacity, ib_bytes.len(), ib_usage, "top_glyph_ib");
            self.queue.write_buffer(&self.top_glyph_ib, 0, ib_bytes);
        }

        let grid_rect_count = self.grid_rect_indices.len() as u32;
        let grid_glyph_count = self.grid_glyph_indices.len() as u32;
        let chrome_rect_count = self.chrome_rect_indices.len() as u32;
        let chrome_glyph_count = self.chrome_glyph_indices.len() as u32;
        let overlay_rect_count = self.rect_indices.len() as u32;
        let overlay_glyph_count = self.glyph_indices.len() as u32;
        let top_rect_count = self.top_rect_indices.len() as u32;
        let top_rounded_rect_count = self.top_rounded_rect_indices.len() as u32;
        let top_glyph_count = self.top_glyph_indices.len() as u32;

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("main_pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: self.clear_color.r as f64,
                            g: self.clear_color.g as f64,
                            b: self.clear_color.b as f64,
                            a: self.clear_color.a as f64,
                        }),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Draw order: grid rects → chrome rects → overlay rects → grid glyphs → chrome glyphs → overlay glyphs
            pass.set_pipeline(&self.rect_pipeline);
            pass.set_bind_group(0, &self.uniform_bind_group, &[]);

            if grid_rect_count > 0 {
                pass.set_vertex_buffer(0, self.grid_rect_vb.slice(..));
                pass.set_index_buffer(self.grid_rect_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..grid_rect_count, 0, 0..1);
            }

            // Chrome rects use the SDF rounded rect pipeline
            if chrome_rect_count > 0 {
                pass.set_pipeline(&self.chrome_rounded_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.chrome_rect_vb.slice(..));
                pass.set_index_buffer(self.chrome_rect_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..chrome_rect_count, 0, 0..1);
                // Restore rect pipeline for overlay
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
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

            if chrome_glyph_count > 0 {
                pass.set_vertex_buffer(0, self.chrome_glyph_vb.slice(..));
                pass.set_index_buffer(self.chrome_glyph_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..chrome_glyph_count, 0, 0..1);
            }

            if overlay_glyph_count > 0 {
                pass.set_vertex_buffer(0, self.glyph_vb.slice(..));
                pass.set_index_buffer(self.glyph_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..overlay_glyph_count, 0, 0..1);
            }

            // Top layer: rendered absolutely last (opaque UI like search bar)
            // First draw SDF rounded rects (popup backgrounds)
            if top_rounded_rect_count > 0 {
                pass.set_pipeline(&self.chrome_rounded_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.top_rounded_rect_vb.slice(..));
                pass.set_index_buffer(self.top_rounded_rect_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..top_rounded_rect_count, 0, 0..1);
            }
            // Then flat rects (borders, highlights, etc.)
            if top_rect_count > 0 {
                pass.set_pipeline(&self.rect_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_vertex_buffer(0, self.top_rect_vb.slice(..));
                pass.set_index_buffer(self.top_rect_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..top_rect_count, 0, 0..1);
            }

            if top_glyph_count > 0 {
                pass.set_pipeline(&self.glyph_pipeline);
                pass.set_bind_group(0, &self.uniform_bind_group, &[]);
                pass.set_bind_group(1, &self.atlas_bind_group, &[]);
                pass.set_vertex_buffer(0, self.top_glyph_vb.slice(..));
                pass.set_index_buffer(self.top_glyph_ib.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..top_glyph_count, 0, 0..1);
            }
        }
    }
}
