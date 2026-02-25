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
    /// Poll the render thread for completed frames.  Returns the renderer
    /// to `self.renderer` and updates `drawable_wait_us`.
    pub(crate) fn poll_render_result(&mut self) {
        let rt = match self.render_thread.as_ref() {
            Some(rt) => rt,
            None => return,
        };
        let mut surface_lost = false;
        while let Ok(result) = rt.result_rx.try_recv() {
            self.drawable_wait_us = result.drawable_wait_us;
            if result.surface_lost {
                surface_lost = true;
            }
            self.renderer = Some(result.renderer);
        }
        if surface_lost {
            self.reconfigure_surface();
        }
    }

    /// Build all vertex data and send to the render thread for GPU submission.
    /// Returns `true` if the frame was dispatched, `false` if the render
    /// thread is still busy with the previous frame (caller should retry).
    pub(crate) fn render(&mut self) -> bool {
        let t0 = std::time::Instant::now();

        // Try to get the renderer back from the render thread
        self.poll_render_result();

        // If renderer is not available, the render thread is still processing
        // the previous frame.  Skip this frame — it will be retried.
        let mut renderer = match self.renderer.take() {
            Some(r) => r,
            None => return false,
        };

        let logical = self.logical_size();
        // When focus_area is EditorDock, treat the active editor tab as focused
        // so the editor cursor renders (self.focused is still the terminal).
        let focused = if self.focus_area == crate::ui_state::FocusArea::EditorDock {
            self.active_editor_tab().or(self.focused)
        } else {
            self.focused
        };
        let search_focus = self.search_focus;
        let show_file_tree = self.show_file_tree;
        let file_tree_scroll = self.file_tree_scroll;
        let visual_pane_rects = self.visual_pane_rects.clone();
        let editor_panel_rect = self.editor_panel_rect;
        let editor_panel_tabs: Vec<tide_core::PaneId> = self.active_editor_tabs().to_vec();
        let editor_panel_active = self.active_editor_tab();
        let alive_pane_ids: Vec<u64> = self.panes.keys().copied().collect();
        let pane_area_mode = self.pane_area_mode;
        let all_pane_ids = self.layout.pane_ids();
        let empty_panel_btn_rects = self.empty_panel_button_rects();

        let p = self.palette();

        // Keep runtime caches bounded to currently alive panes.
        self.pane_generations.retain(|id, _| self.panes.contains_key(id));
        renderer.retain_pane_caches(&alive_pane_ids);

        // Atlas reset -> all cached UV coords are stale, force full rebuild
        if renderer.atlas_was_reset() {
            self.pane_generations.clear();
            renderer.invalidate_all_pane_caches();
            self.last_chrome_generation = self.chrome_generation.wrapping_sub(1);
        }

        // Layout change -> invalidate only panes whose rects changed
        if self.prev_visual_pane_rects != visual_pane_rects {
            let prev_map: std::collections::HashMap<u64, Rect> =
                self.prev_visual_pane_rects.iter().copied().collect();
            for &(id, rect) in &visual_pane_rects {
                if prev_map.get(&id) != Some(&rect) {
                    self.pane_generations.remove(&id);
                    renderer.remove_pane_cache(id);
                }
            }
            // Also invalidate panes that were removed from the layout
            for &(id, _) in &self.prev_visual_pane_rects {
                if !visual_pane_rects.iter().any(|(vid, _)| *vid == id) {
                    self.pane_generations.remove(&id);
                    renderer.remove_pane_cache(id);
                }
            }
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
                pane_area_mode, &all_pane_ids,
            );

            self.last_chrome_generation = self.chrome_generation;
        }

        let t_chrome = t0.elapsed();

        // Detect dock active tab change → force grid rebuild for new tab
        let dock_active_changed = editor_panel_active != self.last_editor_panel_active;
        if dock_active_changed {
            if let Some(new_active) = editor_panel_active {
                self.pane_generations.remove(&new_active);
            }
            self.last_editor_panel_active = editor_panel_active;
        }

        // Per-pane dirty checking: only rebuild panes whose content changed
        let any_dirty = grid::render_grid(
            self, &mut renderer, &p,
            &visual_pane_rects, editor_panel_active, editor_panel_rect,
        );

        // Assemble all pane caches into the global grid arrays if anything changed,
        // or when dock active tab changed (old grid must be removed from assembly)
        if any_dirty || dock_active_changed {
            let mut order: Vec<u64> = visual_pane_rects.iter().map(|(id, _)| *id).collect();
            if let (Some(active_id), Some(_)) = (editor_panel_active, editor_panel_rect) {
                order.push(active_id);
            }
            renderer.assemble_grid(&order);
        }

        let t_grid = t0.elapsed();

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
            &visual_pane_rects, focused,
        );

        renderer.end_frame();

        let t_build = t0.elapsed();

        // Send the renderer to the render thread for GPU submission.
        // The render thread handles get_current_texture() (which may block
        // on CAMetalLayer.nextDrawable()), command encoding, queue submission,
        // presentation, and device polling — all without blocking this thread.
        if let Some(ref rt) = self.render_thread {
            let config_update = self.pending_surface_config.take();
            let _ = rt.job_tx.send(crate::render_thread::RenderJob {
                renderer,
                config_update,
            });
            // renderer is now on the render thread — self.renderer stays None
            // until poll_render_result() retrieves it.
        }

        log::trace!(
            "frame build: chrome={:.0}us grid={:.0}us total={:.0}us",
            t_chrome.as_micros(),
            t_grid.as_micros(),
            t_build.as_micros(),
        );

        true
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
