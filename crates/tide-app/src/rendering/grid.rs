use tide_core::Rect;

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;


use super::bar_offset_for;

/// Perform per-pane dirty checking and rebuild grid caches for panes whose content changed.
/// Returns `true` if any pane was dirty (so the grid needs reassembly).
pub(crate) fn render_grid(
    app: &mut App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    editor_panel_active: Option<u64>,
    editor_panel_rect: Option<Rect>,
) -> bool {
    let mut any_dirty = false;
    for &(id, rect) in visual_pane_rects {
        let gen = match app.panes.get(&id) {
            Some(PaneKind::Terminal(pane)) => pane.backend.grid_generation(),
            Some(PaneKind::Editor(pane)) => pane.generation(),
            Some(PaneKind::Diff(dp)) => dp.generation(),
            None => continue,
        };
        let prev = app.pane_generations.get(&id).copied().unwrap_or(u64::MAX);
        if gen != prev {
            any_dirty = true;
            let pane_bar = bar_offset_for(id, &app.panes, &app.save_confirm);
            let inner = Rect::new(
                rect.x + PANE_PADDING,
                rect.y + TAB_BAR_HEIGHT + pane_bar,
                rect.width - 2.0 * PANE_PADDING,
                (rect.height - TAB_BAR_HEIGHT - PANE_PADDING - pane_bar).max(1.0),
            );
            renderer.begin_pane_grid(id);
            match app.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => {
                    pane.render_grid(inner, renderer);
                    app.pane_generations.insert(id, pane.backend.grid_generation());
                }
                Some(PaneKind::Editor(pane)) => {
                    pane.render_grid_full(inner, renderer, p.gutter_text, p.gutter_active_text,
                        Some(p.diff_added_bg), Some(p.diff_removed_bg),
                        Some(p.diff_added_gutter), Some(p.diff_removed_gutter));
                    app.pane_generations.insert(id, pane.generation());
                }
                Some(PaneKind::Diff(dp)) => {
                    dp.render_grid(inner, renderer, p.tab_text_focused, p.tab_text,
                        p.diff_added_bg, p.diff_removed_bg,
                        p.diff_added_gutter, p.diff_removed_gutter);
                    app.pane_generations.insert(id, dp.generation());
                }
                None => {}
            }
            renderer.end_pane_grid();
        }
    }

    // Also check active panel pane (editor or diff)
    if let (Some(active_id), Some(panel_rect)) = (editor_panel_active, editor_panel_rect) {
        let pane_gen = match app.panes.get(&active_id) {
            Some(PaneKind::Editor(pane)) => Some(pane.generation()),
            Some(PaneKind::Diff(dp)) => Some(dp.generation()),
            _ => None,
        };
        if let Some(gen) = pane_gen {
            let prev = app.pane_generations.get(&active_id).copied().unwrap_or(u64::MAX);
            if gen != prev {
                any_dirty = true;
                let bar_offset = bar_offset_for(active_id, &app.panes, &app.save_confirm);
                let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP + bar_offset;
                let inner = Rect::new(
                    panel_rect.x + PANE_PADDING,
                    content_top,
                    panel_rect.width - 2.0 * PANE_PADDING,
                    (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING - bar_offset).max(1.0),
                );
                renderer.begin_pane_grid(active_id);
                match app.panes.get(&active_id) {
                    Some(PaneKind::Editor(pane)) => {
                        pane.render_grid_full(inner, renderer, p.gutter_text, p.gutter_active_text,
                            Some(p.diff_added_bg), Some(p.diff_removed_bg),
                            Some(p.diff_added_gutter), Some(p.diff_removed_gutter));
                    }
                    Some(PaneKind::Diff(dp)) => {
                        dp.render_grid(inner, renderer, p.tab_text_focused, p.tab_text,
                            p.diff_added_bg, p.diff_removed_bg,
                            p.diff_added_gutter, p.diff_removed_gutter);
                    }
                    _ => {}
                }
                renderer.end_pane_grid();
                app.pane_generations.insert(active_id, gen);
            }
        }
    }

    any_dirty
}
