use tide_core::{Rect, Renderer};

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
) -> bool {
    let top_offset = TAB_BAR_HEIGHT;

    // Set side-by-side mode on diff panes
    for &(id, _) in visual_pane_rects {
        if let Some(PaneKind::Diff(dp)) = app.panes.get_mut(&id) {
            dp.side_by_side = true;
        }
    }

    // Pre-compute preview caches for editor panes in preview mode
    for &(id, rect) in visual_pane_rects {
        if let Some(PaneKind::Editor(pane)) = app.panes.get_mut(&id) {
            if pane.preview_mode {
                let cell_w = renderer.cell_size().width;
                // Reserve scrollbar width so wrapping matches the visible content area
                let wrap_width = ((rect.width - 2.0 * PANE_PADDING - SCROLLBAR_WIDTH) / cell_w).floor() as usize;
                pane.ensure_preview_cache(wrap_width, app.dark_mode);
            }
        }
    }

    // Determine which pane is the effective IME target for preedit shift
    let ime_target_id = app.focused;

    let mut any_dirty = false;
    for &(id, rect) in visual_pane_rects {
        let gen = match app.panes.get(&id) {
            Some(PaneKind::Terminal(pane)) => pane.backend.grid_generation(),
            Some(PaneKind::Editor(pane)) => pane.generation(),
            Some(PaneKind::Diff(dp)) => dp.generation(),
            Some(PaneKind::Browser(_)) => continue, // webview renders natively
            Some(PaneKind::Launcher(_)) => 0, // static content, always render on first check
            None => continue,
        };
        let prev = app.pane_generations.get(&id).copied().unwrap_or(u64::MAX);
        if gen != prev {
            any_dirty = true;
            let pane_bar = bar_offset_for(id, &app.panes, &app.save_confirm);
            let inner = Rect::new(
                rect.x + PANE_PADDING,
                rect.y + top_offset + pane_bar,
                rect.width - 2.0 * PANE_PADDING,
                (rect.height - top_offset - PANE_PADDING - pane_bar).max(1.0),
            );
            renderer.begin_pane_grid(id);
            match app.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => {
                    pane.render_grid(inner, renderer);
                    app.pane_generations.insert(id, pane.backend.grid_generation());
                }
                Some(PaneKind::Editor(pane)) => {
                    let preedit = if ime_target_id == Some(id) { &app.ime_preedit } else { "" };
                    pane.render_grid_full(inner, renderer, p.gutter_text, p.gutter_active_text,
                        Some(p.diff_added_bg), Some(p.diff_removed_bg),
                        Some(p.diff_added_gutter), Some(p.diff_removed_gutter),
                        preedit, p.current_line_bg, p.indent_guide);
                    app.pane_generations.insert(id, pane.generation());
                }
                Some(PaneKind::Diff(dp)) => {
                    dp.render_grid(inner, renderer, p.tab_text_focused, p.tab_text,
                        p.diff_added_bg, p.diff_removed_bg,
                        p.diff_added_gutter, p.diff_removed_gutter,
                        p.border_subtle);
                    app.pane_generations.insert(id, dp.generation());
                }
                Some(PaneKind::Browser(_)) => {} // webview renders natively
                Some(PaneKind::Launcher(_launcher_id)) => {
                    // Render launcher type-selection UI
                    let cs = renderer.cell_size();
                    let lines: [(&str, tide_core::Color); 4] = [
                        ("\u{f120}  [T]  Terminal", p.tab_text_focused),
                        ("\u{f15c}  [E]  New File", p.tab_text),
                        ("\u{f07c}  [O]  Open File", p.tab_text),
                        ("\u{f268}  [B]  Browser", p.tab_text),
                    ];
                    let line_h = cs.height * 1.8;
                    let block_h = lines.len() as f32 * line_h;
                    let start_y = inner.y + (inner.height - block_h) / 2.0;
                    for (i, (text, color)) in lines.iter().enumerate() {
                        let text_w = text.chars().count() as f32 * cs.width;
                        let x = inner.x + (inner.width - text_w) / 2.0;
                        let y = start_y + i as f32 * line_h;
                        renderer.draw_text(
                            text,
                            tide_core::Vec2::new(x, y),
                            tide_core::TextStyle {
                                foreground: *color,
                                background: None,
                                bold: i == 0,
                                dim: false,
                                italic: false,
                                underline: false,
                            },
                            inner,
                        );
                    }
                    // Don't cache generation — Launcher is static/cheap, always
                    // re-render to avoid stale-cache issues after atlas resets.
                }
                None => {}
            }
            renderer.end_pane_grid();
        }
    }

    any_dirty
}
