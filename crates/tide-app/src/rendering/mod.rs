mod chrome;
mod cursor;
mod grid;
mod hover;
mod ime;
mod overlays;

use tide_core::{Rect, Renderer};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;


/// Compute the bar offset for a pane. Returns CONFLICT_BAR_HEIGHT if a notification bar
/// (conflict or save confirm) is visible, else 0.
pub(super) fn bar_offset_for(
    pane_id: tide_core::PaneId,
    panes: &std::collections::HashMap<tide_core::PaneId, PaneKind>,
    save_confirm: &Option<crate::SaveConfirmState>,
) -> f32 {
    if let Some(ref sc) = save_confirm {
        if sc.pane_id == pane_id {
            return CONFLICT_BAR_HEIGHT;
        }
    }
    if let Some(PaneKind::Editor(pane)) = panes.get(&pane_id) {
        if pane.needs_notification_bar() {
            return CONFLICT_BAR_HEIGHT;
        }
    }
    0.0
}

impl App {
    pub(crate) fn render(&mut self) {
        let surface = match self.surface.as_ref() {
            Some(s) => s,
            None => return,
        };

        let output = match surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.reconfigure_surface();
                return;
            }
            Err(e) => {
                log::error!("Surface error: {}", e);
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let logical = self.logical_size();
        let focused = self.focused;
        let search_focus = self.search_focus;
        let show_file_tree = self.show_file_tree;
        let file_tree_scroll = self.file_tree_scroll;
        let visual_pane_rects = self.visual_pane_rects.clone();
        let editor_panel_rect = self.editor_panel_rect;
        let editor_panel_tabs = self.editor_panel_tabs.clone();
        let editor_panel_active = self.editor_panel_active;
        let alive_pane_ids: Vec<u64> = self.panes.keys().copied().collect();
        let empty_panel_btn_rects = self.empty_panel_button_rects();

        let p = self.palette();

        // Temporarily take the renderer out of self so we can pass both
        // &mut App and &mut WgpuRenderer to sub-module functions.
        let mut renderer = self.renderer.take().unwrap();

        // Keep runtime caches bounded to currently alive panes.
        self.pane_generations.retain(|id, _| self.panes.contains_key(id));
        renderer.retain_pane_caches(&alive_pane_ids);

        // Atlas reset -> all cached UV coords are stale, force full rebuild
        if renderer.atlas_was_reset() {
            self.pane_generations.clear();
            renderer.invalidate_all_pane_caches();
            self.last_chrome_generation = self.chrome_generation.wrapping_sub(1);
        }

        // Layout change -> invalidate all pane caches (positions changed)
        if self.prev_visual_pane_rects != visual_pane_rects {
            self.pane_generations.clear();
            renderer.invalidate_all_pane_caches();
            self.prev_visual_pane_rects = visual_pane_rects.clone();
        }

        renderer.begin_frame(logical);

        // Rebuild chrome layer only when chrome content changed (panel backgrounds, file tree)
        let chrome_dirty = self.chrome_generation != self.last_chrome_generation;
        if chrome_dirty {
            chrome::render_chrome(
                self, &mut renderer, &p, logical,
                focused, show_file_tree, file_tree_scroll,
                &visual_pane_rects, editor_panel_rect,
                &editor_panel_tabs, editor_panel_active,
            );

            self.last_chrome_generation = self.chrome_generation;
        }

        // Per-pane dirty checking: only rebuild panes whose content changed
        let any_dirty = grid::render_grid(
            self, &mut renderer, &p,
            &visual_pane_rects, editor_panel_active, editor_panel_rect,
        );

        // Assemble all pane caches into the global grid arrays if anything changed
        if any_dirty {
            let mut order: Vec<u64> = visual_pane_rects.iter().map(|(id, _)| *id).collect();
            if let Some(active_id) = editor_panel_active {
                order.push(active_id);
            }
            renderer.assemble_grid(&order);
        }

        // Always render cursor (overlay layer) — cursor blinks/moves independently
        cursor::render_cursor_and_highlights(
            self, &mut renderer, &p,
            &visual_pane_rects, focused, search_focus,
            editor_panel_active, editor_panel_rect,
        );

        // Render hover highlights (overlay layer)
        hover::render_hover(
            self, &mut renderer, &p, logical,
            &visual_pane_rects, show_file_tree, file_tree_scroll,
            editor_panel_rect, &editor_panel_tabs, editor_panel_active,
            empty_panel_btn_rects,
        );

        // Render overlay UI elements (search bars, notification bars, save-as, file finder,
        // branch switcher, file switcher)
        overlays::render_overlays(
            self, &mut renderer, &p,
            &visual_pane_rects, editor_panel_active, editor_panel_rect,
        );

        // Render IME preedit overlay and drag-drop preview
        ime::render_ime_and_drop_preview(
            self, &mut renderer, &p,
            &visual_pane_rects, focused, editor_panel_rect,
        );

        renderer.end_frame();

        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render_encoder"),
        });

        renderer.render_frame(&mut encoder, &view);

        queue.submit(std::iter::once(encoder.finish()));
        output.present();

        // Reclaim completed GPU staging buffers to prevent memory accumulation.
        // Without this, write_buffer() staging allocations are never freed on macOS Metal.
        device.poll(wgpu::Maintain::Poll);

        // Put renderer back
        self.renderer = Some(renderer);
    }

    /// Insert preview: semi-transparent fill + thin border.
    fn draw_insert_preview(renderer: &mut tide_renderer::WgpuRenderer, preview: Rect, p: &ThemePalette) {
        renderer.draw_rect(preview, p.drop_fill);
        let bw = DROP_PREVIEW_BORDER_WIDTH;
        renderer.draw_rect(Rect::new(preview.x, preview.y, preview.width, bw), p.drop_border);
        renderer.draw_rect(Rect::new(preview.x, preview.y + preview.height - bw, preview.width, bw), p.drop_border);
        renderer.draw_rect(Rect::new(preview.x, preview.y, bw, preview.height), p.drop_border);
        renderer.draw_rect(Rect::new(preview.x + preview.width - bw, preview.y, bw, preview.height), p.drop_border);
    }

    /// Swap preview: thick border only, no fill — visually distinct from insert.
    fn draw_swap_preview(renderer: &mut tide_renderer::WgpuRenderer, preview: Rect, p: &ThemePalette) {
        let bw = SWAP_PREVIEW_BORDER_WIDTH;
        renderer.draw_rect(Rect::new(preview.x, preview.y, preview.width, bw), p.swap_border);
        renderer.draw_rect(Rect::new(preview.x, preview.y + preview.height - bw, preview.width, bw), p.swap_border);
        renderer.draw_rect(Rect::new(preview.x, preview.y, bw, preview.height), p.swap_border);
        renderer.draw_rect(Rect::new(preview.x + preview.width - bw, preview.y, bw, preview.height), p.swap_border);
    }
}
