use tide_core::{Rect, Renderer, TerminalBackend};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;


use super::bar_offset_for;

/// Render cursor, selection highlights, search highlights, URL underlines, and scrollbars
/// for all panes (both tree panes and the active panel editor).
pub(crate) fn render_cursor_and_highlights(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    focused: Option<u64>,
    search_focus: Option<u64>,
    editor_panel_active: Option<u64>,
    editor_panel_rect: Option<Rect>,
) {
    let top_offset = app.pane_area_mode.content_top();

    // Always render cursor (overlay layer) — cursor blinks/moves independently
    for &(id, rect) in visual_pane_rects {
        let pane_bar = bar_offset_for(id, &app.panes, &app.save_confirm);
        let inner = Rect::new(
            rect.x + PANE_PADDING,
            rect.y + top_offset + pane_bar,
            rect.width - 2.0 * PANE_PADDING,
            (rect.height - top_offset - PANE_PADDING - pane_bar).max(1.0),
        );
        match app.panes.get(&id) {
            Some(PaneKind::Terminal(pane)) => {
                // Only render cursor on the focused pane (and hide when search bar is active
                // or IME preedit is composing — preedit overlay replaces the cursor).
                if focused == Some(id) && search_focus != Some(id) && app.ime_preedit.is_empty() {
                    pane.render_cursor(inner, renderer, p.cursor_accent);
                }
                // Render URL underlines when Cmd/Meta is held
                if app.modifiers.meta {
                    pane.render_url_underlines(inner, renderer, p.link_color);
                }
                // Render selection highlight
                if let Some(ref sel) = pane.selection {
                    let cell_size = renderer.cell_size();
                    let (start, end) = if sel.anchor <= sel.end {
                        (sel.anchor, sel.end)
                    } else {
                        (sel.end, sel.anchor)
                    };
                    // Skip rendering if anchor == end (no actual selection)
                    if start != end {
                        let sel_color = p.selection;
                        let grid = pane.backend.grid();
                        let max_rows = (inner.height / cell_size.height).ceil() as usize;
                        let max_cols = (inner.width / cell_size.width).floor() as usize;
                        let visible_rows = (grid.rows as usize).min(max_rows);
                        let visible_cols = (grid.cols as usize).min(max_cols);
                        // Center offset matching terminal grid
                        let actual_w = max_cols as f32 * cell_size.width;
                        let center_x = (inner.width - actual_w) / 2.0;
                        for row in start.0..=end.0.min(visible_rows.saturating_sub(1)) {
                            let col_start = if row == start.0 { start.1 } else { 0 };
                            let col_end = if row == end.0 { end.1 } else { visible_cols };
                            if col_start >= col_end {
                                continue;
                            }
                            let rx = inner.x + center_x + col_start as f32 * cell_size.width;
                            let ry = inner.y + row as f32 * cell_size.height;
                            let rw = (col_end - col_start) as f32 * cell_size.width;
                            renderer.draw_rect(
                                Rect::new(rx, ry, rw, cell_size.height),
                                sel_color,
                            );
                        }
                    }
                }
                // Render terminal search highlights
                if let Some(ref search) = pane.search {
                    if search.visible && !search.input.is_empty() {
                        let cell_size = renderer.cell_size();
                        let history_size = pane.backend.history_size();
                        let display_offset = pane.backend.display_offset();
                        let grid = pane.backend.grid();
                        let screen_rows = grid.rows as usize;
                        // Center offset matching terminal grid
                        let max_cols = (inner.width / cell_size.width).floor() as usize;
                        let actual_w = max_cols as f32 * cell_size.width;
                        let center_x = (inner.width - actual_w) / 2.0;
                        // Visible absolute line range
                        let visible_start = history_size.saturating_sub(display_offset);
                        let visible_end = visible_start + screen_rows;
                        for (mi, m) in search.matches.iter().enumerate() {
                            if m.line < visible_start || m.line >= visible_end {
                                continue;
                            }
                            let visual_row = m.line - visible_start;
                            let rx = inner.x + center_x + m.col as f32 * cell_size.width;
                            let ry = inner.y + visual_row as f32 * cell_size.height;
                            let rw = m.len as f32 * cell_size.width;
                            let color = if search.current == Some(mi) {
                                p.search_current_bg
                            } else {
                                p.search_match_bg
                            };
                            renderer.draw_rect(Rect::new(rx, ry, rw, cell_size.height), color);
                        }
                    }
                }
            }
            Some(PaneKind::Editor(pane)) => {
                if !pane.preview_mode {
                    if focused == Some(id) && search_focus != Some(id) && app.ime_preedit.is_empty() {
                        pane.render_cursor(inner, renderer, p.cursor_accent);
                    }
                    // Render editor selection highlight
                    if let Some(ref sel) = pane.selection {
                        render_editor_selection(pane, inner, renderer, p, sel);
                    }
                    // Render editor search highlights
                    if let Some(ref search) = pane.search {
                        render_editor_search_highlights(pane, inner, renderer, p, search);
                    }
                }
                // Render editor scrollbar with search match markers
                pane.render_scrollbar(inner, renderer, pane.search.as_ref(), p);
            }
            Some(PaneKind::Diff(_)) => {}
            Some(PaneKind::Browser(_)) => {}
            None => {}
        }
    }

    // Render cursor for active panel editor
    if let (Some(active_id), Some(panel_rect)) = (editor_panel_active, editor_panel_rect) {
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&active_id) {
            let bar_offset = bar_offset_for(active_id, &app.panes, &app.save_confirm);
            let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP + bar_offset;
            let inner = Rect::new(
                panel_rect.x + PANE_PADDING,
                content_top,
                panel_rect.width - 2.0 * PANE_PADDING,
                (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING - bar_offset).max(1.0),
            );
            if !pane.preview_mode {
                if focused == Some(active_id) && search_focus != Some(active_id) && app.ime_preedit.is_empty() {
                    pane.render_cursor(inner, renderer, p.cursor_accent);
                }

                // Panel editor selection highlight
                if let Some(ref sel) = pane.selection {
                    render_editor_selection(pane, inner, renderer, p, sel);
                }

                // Panel editor search highlights
                if let Some(ref search) = pane.search {
                    render_editor_search_highlights(pane, inner, renderer, p, search);
                }
            }

            // Render panel editor scrollbar with search match markers
            pane.render_scrollbar(inner, renderer, pane.search.as_ref(), p);
        }
    }
}

/// Render selection highlight for an editor pane.
fn render_editor_selection(
    pane: &crate::editor_pane::EditorPane,
    inner: Rect,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    sel: &crate::pane::Selection,
) {
    let cell_size = renderer.cell_size();
    let (start, end) = if sel.anchor <= sel.end {
        (sel.anchor, sel.end)
    } else {
        (sel.end, sel.anchor)
    };
    if start != end {
        let sel_color = p.selection;
        let scroll = pane.editor.scroll_offset();
        let h_scroll = pane.editor.h_scroll_offset();
        let gutter_width = 5.0 * cell_size.width;
        let visible_rows = (inner.height / cell_size.height).ceil() as usize;
        let visible_cols = ((inner.width - gutter_width) / cell_size.width).ceil() as usize;
        for row in start.0..=end.0 {
            if row < scroll || row >= scroll + visible_rows {
                continue;
            }
            let visual_row = row - scroll;
            let col_start = if row == start.0 { start.1 } else { 0 };
            let col_end = if row == end.0 {
                end.1
            } else {
                // Full line width: use char count or visible cols
                let char_count = pane.editor.buffer.line(row).map_or(0, |l| l.chars().count());
                char_count.max(h_scroll + visible_cols)
            };
            if col_start >= col_end {
                continue;
            }
            // Clip to visible horizontal range
            let vis_start = col_start.max(h_scroll).saturating_sub(h_scroll);
            let vis_end = col_end.saturating_sub(h_scroll).min(visible_cols);
            if vis_start >= vis_end {
                continue;
            }
            let rx = inner.x + gutter_width + vis_start as f32 * cell_size.width;
            let ry = inner.y + visual_row as f32 * cell_size.height;
            let rw = (vis_end - vis_start) as f32 * cell_size.width;
            renderer.draw_rect(Rect::new(rx, ry, rw, cell_size.height), sel_color);
        }
    }
}

/// Render search match highlights for an editor pane.
fn render_editor_search_highlights(
    pane: &crate::editor_pane::EditorPane,
    inner: Rect,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    search: &crate::search::SearchState,
) {
    if search.visible && !search.input.is_empty() {
        let cell_size = renderer.cell_size();
        let scroll = pane.editor.scroll_offset();
        let h_scroll = pane.editor.h_scroll_offset();
        let gutter_width = 5.0 * cell_size.width;
        let visible_rows = (inner.height / cell_size.height).ceil() as usize;
        for (mi, m) in search.matches.iter().enumerate() {
            if m.line < scroll || m.line >= scroll + visible_rows {
                continue;
            }
            if m.col + m.len <= h_scroll {
                continue;
            }
            let visual_row = m.line - scroll;
            let visual_col = if m.col >= h_scroll { m.col - h_scroll } else { 0 };
            let draw_len = if m.col >= h_scroll {
                m.len
            } else {
                m.len - (h_scroll - m.col)
            };
            let rx = inner.x + gutter_width + visual_col as f32 * cell_size.width;
            let ry = inner.y + visual_row as f32 * cell_size.height;
            let rw = draw_len as f32 * cell_size.width;
            let color = if search.current == Some(mi) {
                p.search_current_bg
            } else {
                p.search_match_bg
            };
            renderer.draw_rect(Rect::new(rx, ry, rw, cell_size.height), color);
        }
    }
}
