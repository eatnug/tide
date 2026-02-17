use unicode_width::UnicodeWidthChar;

use tide_core::{Rect, Renderer, TextStyle, Vec2};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui::file_icon;
use crate::App;

/// Sum of display widths for characters in `s`, treating wide (CJK) chars as 2 columns.
fn visual_width(s: &str) -> usize {
    s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(1)).sum()
}

// ── Shared helper functions ──

/// Draw a rounded popup background with border using SDF.
/// Renders outer rounded rect (border color) then inner rounded rect (fill color).
fn draw_popup_rounded_bg(
    renderer: &mut tide_renderer::WgpuRenderer,
    rect: Rect,
    fill: tide_core::Color,
    border: tide_core::Color,
    radius: f32,
) {
    let bw = POPUP_BORDER_WIDTH;
    // Outer rounded rect (border)
    renderer.draw_top_rounded_rect(rect, border, radius);
    // Inner rounded rect (fill, inset by border width)
    let inner = Rect::new(rect.x + bw, rect.y + bw, rect.width - 2.0 * bw, rect.height - 2.0 * bw);
    renderer.draw_top_rounded_rect(inner, fill, (radius - bw).max(0.0));
}

/// Draw a 1px (or `POPUP_BORDER_WIDTH`) border around `rect`.
fn draw_popup_border(renderer: &mut tide_renderer::WgpuRenderer, rect: Rect, color: tide_core::Color) {
    let bw = POPUP_BORDER_WIDTH;
    renderer.draw_top_rect(Rect::new(rect.x, rect.y, rect.width, bw), color);
    renderer.draw_top_rect(Rect::new(rect.x, rect.y + rect.height - bw, rect.width, bw), color);
    renderer.draw_top_rect(Rect::new(rect.x, rect.y, bw, rect.height), color);
    renderer.draw_top_rect(Rect::new(rect.x + rect.width - bw, rect.y, bw, rect.height), color);
}

/// Draw a cursor beam (vertical line) at the given position.
fn draw_cursor_beam(renderer: &mut tide_renderer::WgpuRenderer, x: f32, y: f32, height: f32, color: tide_core::Color) {
    renderer.draw_top_rect(Rect::new(x, y, CURSOR_BEAM_WIDTH, height), color);
}

/// Create a plain (non-bold) TextStyle with the given foreground color.
fn text_style(color: tide_core::Color) -> TextStyle {
    TextStyle {
        foreground: color,
        background: None,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
    }
}

/// Create a bold TextStyle with the given foreground color.
fn bold_style(color: tide_core::Color) -> TextStyle {
    TextStyle {
        foreground: color,
        background: None,
        bold: true,
        dim: false,
        italic: false,
        underline: false,
    }
}


/// Draw a full-screen dim overlay (scrim) behind floating popups.
fn draw_popup_scrim(renderer: &mut tide_renderer::WgpuRenderer, logical_size: tide_core::Size, color: tide_core::Color) {
    renderer.draw_top_rect(Rect::new(0.0, 0.0, logical_size.width, logical_size.height), color);
}

/// Render all overlay UI elements on the top layer: search bars, notification bars,
/// save-as inline edit, file finder, git switcher, and file switcher.
pub(crate) fn render_overlays(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    editor_panel_active: Option<u64>,
    editor_panel_rect: Option<Rect>,
) {
    render_search_bars(app, renderer, p, visual_pane_rects, editor_panel_active, editor_panel_rect);
    render_notification_bars(app, renderer, p, visual_pane_rects, editor_panel_active, editor_panel_rect);
    render_save_as(app, renderer, p, editor_panel_rect);
    render_file_finder(app, renderer, p, editor_panel_rect);
    render_git_switcher(app, renderer, p);
    render_file_switcher(app, renderer, p);
    render_context_menu(app, renderer, p);
}

/// Render search bar UI for panes that have search visible.
fn render_search_bars(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    editor_panel_active: Option<u64>,
    editor_panel_rect: Option<Rect>,
) {
    let search_focus = app.search_focus;
    let cell_size = renderer.cell_size();

    // Helper: render a search bar floating at top-right of a given rect
    let mut search_bars: Vec<(tide_core::PaneId, Rect, String, String, usize, bool)> = Vec::new();
    for &(id, rect) in visual_pane_rects {
        let (query, display, cursor_pos, visible) = match app.panes.get(&id) {
            Some(PaneKind::Terminal(pane)) => match &pane.search {
                Some(s) if s.visible => (s.input.text.clone(), s.current_display(), s.input.cursor, true),
                _ => continue,
            },
            Some(PaneKind::Editor(pane)) => match &pane.search {
                Some(s) if s.visible => (s.input.text.clone(), s.current_display(), s.input.cursor, true),
                _ => continue,
            },
            _ => continue,
        };
        if visible {
            search_bars.push((id, rect, query, display, cursor_pos, search_focus == Some(id)));
        }
    }

    // Also check panel editor
    if let (Some(active_id), Some(panel_rect)) = (editor_panel_active, editor_panel_rect) {
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&active_id) {
            if let Some(ref s) = pane.search {
                if s.visible {
                    search_bars.push((active_id, panel_rect, s.input.text.clone(), s.current_display(), s.input.cursor, search_focus == Some(active_id)));
                }
            }
        }
    }

    for (_id, rect, query, display, cursor_pos, is_focused) in &search_bars {
        let bar_w = SEARCH_BAR_WIDTH.min(rect.width - 16.0);
        if bar_w < 80.0 { continue; } // too narrow to render
        let bar_h = SEARCH_BAR_HEIGHT;
        let bar_x = rect.x + rect.width - bar_w - 8.0;
        let bar_y = rect.y + app.pane_area_mode.content_top() + 4.0;
        let bar_rect = Rect::new(bar_x, bar_y, bar_w, bar_h);

        // Background (top layer — fully opaque, covers text)
        renderer.draw_top_rect(bar_rect, p.search_bar_bg);

        // Border
        draw_popup_border(renderer, bar_rect, p.search_bar_border);

        let text_x = bar_x + 6.0;
        let text_y = bar_y + (bar_h - cell_size.height) / 2.0;
        let ts = text_style(p.search_bar_text);
        let muted_style = text_style(p.tab_text);
        let counter_style = text_style(p.search_bar_counter);

        // Layout: [query text] [counter] [close button]
        let close_area_w = SEARCH_BAR_CLOSE_SIZE;
        let close_x = bar_x + bar_w - close_area_w;
        let counter_w = display.len() as f32 * cell_size.width;
        let counter_x = close_x - counter_w - 4.0;
        let text_clip_w = (counter_x - text_x - 4.0).max(0.0);

        // Query text (top layer) or placeholder
        let text_clip = Rect::new(text_x, bar_y, text_clip_w, bar_h);
        if query.is_empty() {
            renderer.draw_top_text("Search...", Vec2::new(text_x, text_y), muted_style, text_clip);
        } else {
            renderer.draw_top_text(query, Vec2::new(text_x, text_y), ts, text_clip);
        }

        // Text cursor (beam) — only when focused
        if *is_focused {
            let cx = text_x + visual_width(&query[..*cursor_pos]) as f32 * cell_size.width;
            draw_cursor_beam(renderer, cx, text_y, cell_size.height, p.cursor_accent);
        }

        // Counter text
        let counter_clip = Rect::new(counter_x, bar_y, counter_w + 4.0, bar_h);
        renderer.draw_top_text(display, Vec2::new(counter_x, text_y), counter_style, counter_clip);

        // Close button
        let close_icon_x = close_x + (close_area_w - cell_size.width) / 2.0;
        let close_clip = Rect::new(close_x, bar_y, close_area_w, bar_h);
        renderer.draw_top_text("\u{f00d}", Vec2::new(close_icon_x, text_y), counter_style, close_clip);
    }
}

/// Render notification bars (conflict / save confirm) for all editor panes.
fn render_notification_bars(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    editor_panel_active: Option<u64>,
    editor_panel_rect: Option<Rect>,
) {
    let cell_size = renderer.cell_size();

    // Collect all panes that need notification bars
    let mut bar_panes: Vec<(tide_core::PaneId, Rect)> = Vec::new();

    // Panel editor
    if let (Some(active_id), Some(panel_rect)) = (editor_panel_active, editor_panel_rect) {
        let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
        let bar_x = panel_rect.x + PANE_PADDING;
        let bar_w = panel_rect.width - 2.0 * PANE_PADDING;
        bar_panes.push((active_id, Rect::new(bar_x, content_top, bar_w, CONFLICT_BAR_HEIGHT)));
    }

    // Left-side panes
    let content_top_off = app.pane_area_mode.content_top();
    for &(id, rect) in visual_pane_rects {
        let content_top = rect.y + content_top_off;
        let bar_x = rect.x + PANE_PADDING;
        let bar_w = rect.width - 2.0 * PANE_PADDING;
        bar_panes.push((id, Rect::new(bar_x, content_top, bar_w, CONFLICT_BAR_HEIGHT)));
    }

    for (pane_id, bar_rect) in bar_panes {
        // Check for save confirm bar first
        if let Some(ref sc) = app.save_confirm {
            if sc.pane_id == pane_id {
                // Render save confirm bar
                renderer.draw_top_rect(bar_rect, p.conflict_bar_bg);
                let text_y = bar_rect.y + (CONFLICT_BAR_HEIGHT - cell_size.height) / 2.0;
                let ts = text_style(p.conflict_bar_text);
                renderer.draw_top_text("Unsaved changes", Vec2::new(bar_rect.x + 8.0, text_y), ts, bar_rect);

                let btn_style = bold_style(p.conflict_bar_btn_text);
                let btn_pad = 8.0;
                let btn_h = CONFLICT_BAR_HEIGHT - 6.0;
                let btn_y = bar_rect.y + 3.0;

                // Cancel button (rightmost)
                let cancel_text = "Cancel";
                let cancel_w = cancel_text.len() as f32 * cell_size.width + btn_pad * 2.0;
                let cancel_x = bar_rect.x + bar_rect.width - cancel_w - 4.0;
                let cancel_rect = Rect::new(cancel_x, btn_y, cancel_w, btn_h);
                renderer.draw_top_rect(cancel_rect, p.conflict_bar_btn);
                renderer.draw_top_text(cancel_text, Vec2::new(cancel_x + btn_pad, text_y), btn_style, cancel_rect);

                // Don't Save button
                let dont_save_text = "Don't Save";
                let dont_save_w = dont_save_text.len() as f32 * cell_size.width + btn_pad * 2.0;
                let dont_save_x = cancel_x - dont_save_w - 4.0;
                let dont_save_rect = Rect::new(dont_save_x, btn_y, dont_save_w, btn_h);
                renderer.draw_top_rect(dont_save_rect, p.conflict_bar_btn);
                renderer.draw_top_text(dont_save_text, Vec2::new(dont_save_x + btn_pad, text_y), btn_style, dont_save_rect);

                // Save button
                let save_text = "Save";
                let save_w = save_text.len() as f32 * cell_size.width + btn_pad * 2.0;
                let save_x = dont_save_x - save_w - 4.0;
                let save_rect = Rect::new(save_x, btn_y, save_w, btn_h);
                renderer.draw_top_rect(save_rect, p.conflict_bar_btn);
                renderer.draw_top_text(save_text, Vec2::new(save_x + btn_pad, text_y), btn_style, save_rect);

                continue; // Don't also show conflict bar
            }
        }

        // Notification bar (diff mode or file deleted)
        if let Some(PaneKind::Editor(pane)) = app.panes.get(&pane_id) {
            if pane.needs_notification_bar() {
                renderer.draw_top_rect(bar_rect, p.conflict_bar_bg);
                let text_y = bar_rect.y + (CONFLICT_BAR_HEIGHT - cell_size.height) / 2.0;
                let ts = text_style(p.conflict_bar_text);
                let msg = if pane.file_deleted {
                    "File deleted on disk"
                } else {
                    "Comparing with disk"
                };
                renderer.draw_top_text(msg, Vec2::new(bar_rect.x + 8.0, text_y), ts, bar_rect);

                let btn_style = bold_style(p.conflict_bar_btn_text);
                let btn_pad = 8.0;
                let btn_h = CONFLICT_BAR_HEIGHT - 6.0;
                let btn_y = bar_rect.y + 3.0;

                // Overwrite button (rightmost)
                let overwrite_text = "Overwrite";
                let overwrite_w = overwrite_text.len() as f32 * cell_size.width + btn_pad * 2.0;
                let overwrite_x = bar_rect.x + bar_rect.width - overwrite_w - 4.0;
                let overwrite_rect = Rect::new(overwrite_x, btn_y, overwrite_w, btn_h);
                renderer.draw_top_rect(overwrite_rect, p.conflict_bar_btn);
                renderer.draw_top_text(overwrite_text, Vec2::new(overwrite_x + btn_pad, text_y), btn_style, overwrite_rect);

                // Reload button (diff mode only, not for deleted files)
                if pane.diff_mode && !pane.file_deleted {
                    let reload_text = "Reload";
                    let reload_w = reload_text.len() as f32 * cell_size.width + btn_pad * 2.0;
                    let reload_x = overwrite_x - reload_w - 4.0;
                    let reload_rect = Rect::new(reload_x, btn_y, reload_w, btn_h);
                    renderer.draw_top_rect(reload_rect, p.conflict_bar_btn);
                    renderer.draw_top_text(reload_text, Vec2::new(reload_x + btn_pad, text_y), btn_style, reload_rect);
                }
            }
        }
    }
}

/// Render save-as floating popup overlay on the top layer.
fn render_save_as(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    editor_panel_rect: Option<Rect>,
) {
    let save_as = match app.save_as_input {
        Some(ref s) => s,
        None => return,
    };
    let panel_rect = match editor_panel_rect {
        Some(r) => r,
        None => return,
    };

    // Dim overlay (scrim)
    draw_popup_scrim(renderer, app.logical_size(), p.popup_scrim);

    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let field_h = cell_height + POPUP_INPUT_PADDING;
    let hint_h = cell_height + 8.0;
    let padding = POPUP_TEXT_INSET;

    // Popup dimensions — anchored below the active panel tab
    let popup_w = SAVE_AS_POPUP_W.min(panel_rect.width - 2.0 * PANE_PADDING);
    let popup_h = field_h * 2.0 + POPUP_SEPARATOR + hint_h + 2.0 * padding;
    let popup_x = save_as.anchor_rect.x.clamp(
        panel_rect.x + PANE_PADDING,
        panel_rect.x + panel_rect.width - popup_w - PANE_PADDING,
    );
    let popup_y = save_as.anchor_rect.y + save_as.anchor_rect.height + 4.0;
    let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

    // Background + border (rounded)
    draw_popup_rounded_bg(renderer, popup_rect, p.popup_bg, p.popup_border, POPUP_CORNER_RADIUS);

    let ts = text_style(p.tab_text_focused);
    let label_style = bold_style(p.tab_text);
    let muted_style = text_style(p.tab_text);

    let label_w = 5.0 * cell_size.width + 8.0; // "Dir " or "Name" + padding
    let content_x = popup_x + padding + label_w;
    let content_w = popup_w - 2.0 * padding - label_w;

    let is_dir_active = save_as.active_field == crate::SaveAsField::Directory;

    // ── Directory field ──
    let dir_y = popup_y + padding;
    let dir_rect = Rect::new(popup_x + padding, dir_y, popup_w - 2.0 * padding, field_h);
    if is_dir_active {
        renderer.draw_top_rect(dir_rect, p.popup_selected);
    }
    let dir_text_y = dir_y + (field_h - cell_height) / 2.0;
    renderer.draw_top_text("Dir", Vec2::new(popup_x + padding + 4.0, dir_text_y), label_style, dir_rect);
    let dir_clip = Rect::new(content_x, dir_y, content_w, field_h);
    renderer.draw_top_text(&save_as.directory.text, Vec2::new(content_x, dir_text_y), ts, dir_clip);
    if is_dir_active {
        let cx = content_x + visual_width(&save_as.directory.text[..save_as.directory.cursor]) as f32 * cell_size.width;
        draw_cursor_beam(renderer, cx, dir_text_y, cell_height, p.cursor_accent);
    }

    // Separator
    let sep_y = dir_y + field_h;
    renderer.draw_top_rect(Rect::new(popup_x + POPUP_SEPARATOR_INSET, sep_y, popup_w - 2.0 * POPUP_SEPARATOR_INSET, POPUP_SEPARATOR), p.popup_border);

    // ── Filename field ──
    let name_y = sep_y + POPUP_SEPARATOR;
    let name_rect = Rect::new(popup_x + padding, name_y, popup_w - 2.0 * padding, field_h);
    if !is_dir_active {
        renderer.draw_top_rect(name_rect, p.popup_selected);
    }
    let name_text_y = name_y + (field_h - cell_height) / 2.0;
    renderer.draw_top_text("Name", Vec2::new(popup_x + padding + 4.0, name_text_y), label_style, name_rect);
    let name_clip = Rect::new(content_x, name_y, content_w, field_h);
    renderer.draw_top_text(&save_as.filename.text, Vec2::new(content_x, name_text_y), ts, name_clip);
    if !is_dir_active {
        let cx = content_x + visual_width(&save_as.filename.text[..save_as.filename.cursor]) as f32 * cell_size.width;
        draw_cursor_beam(renderer, cx, name_text_y, cell_height, p.cursor_accent);
    }

    // ── Hint bar ──
    let hint_y = name_y + field_h;
    let hint_text_y = hint_y + (hint_h - cell_height) / 2.0;
    let hint = "Enter save   Tab switch   Esc cancel";
    let hint_w_px = hint.len() as f32 * cell_size.width;
    let hint_x = popup_x + (popup_w - hint_w_px) / 2.0;
    let hint_clip = Rect::new(popup_x + padding, hint_y, popup_w - 2.0 * padding, hint_h);
    renderer.draw_top_text(hint, Vec2::new(hint_x, hint_text_y), muted_style, hint_clip);
}

/// Render file finder UI on top layer (visible regardless of tab state).
fn render_file_finder(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    editor_panel_rect: Option<Rect>,
) {
    if let (Some(ref finder), Some(panel_rect)) = (&app.file_finder, editor_panel_rect) {
        // Dim overlay (scrim)
        draw_popup_scrim(renderer, app.logical_size(), p.popup_scrim);

        let cell_size = renderer.cell_size();
        let cell_height = cell_size.height;
        let line_height = cell_height * FILE_TREE_LINE_SPACING;
        let indent_width = cell_size.width * 1.5;

        // Full panel background to cover editor content below
        renderer.draw_top_rect(panel_rect, p.surface_bg);

        let muted_style = text_style(p.tab_text);

        // Search input bar
        let input_x = panel_rect.x + PANE_PADDING;
        let input_y = panel_rect.y + PANE_PADDING + 8.0;
        let input_w = panel_rect.width - 2.0 * PANE_PADDING;
        let input_h = cell_height + POPUP_INPUT_PADDING;
        let input_rect = Rect::new(input_x, input_y, input_w, input_h);
        renderer.draw_top_rect(input_rect, p.popup_bg);
        draw_popup_border(renderer, input_rect, p.popup_border);

        // Search icon + query text
        let query_x = input_x + POPUP_TEXT_INSET;
        let query_y = input_y + (input_h - cell_height) / 2.0;
        let search_icon = "\u{f002} ";
        let icon_style = text_style(p.tab_text);
        renderer.draw_top_text(
            search_icon,
            Vec2::new(query_x, query_y),
            icon_style,
            input_rect,
        );
        let text_x = query_x + 2.0 * cell_size.width;
        let ts = text_style(p.tab_text_focused);
        let text_clip = Rect::new(text_x, input_y, input_w - POPUP_TEXT_INSET - 2.0 * cell_size.width, input_h);
        if finder.input.is_empty() {
            renderer.draw_top_text(
                "Search files...",
                Vec2::new(text_x, query_y),
                muted_style,
                text_clip,
            );
        } else {
            renderer.draw_top_text(
                &finder.input.text,
                Vec2::new(text_x, query_y),
                ts,
                text_clip,
            );
        }

        // Match count
        let count_text = format!("{}/{}", finder.filtered.len(), finder.entries.len());
        let count_w = count_text.len() as f32 * cell_size.width;
        let count_x = input_x + input_w - count_w - POPUP_TEXT_INSET;
        renderer.draw_top_text(
            &count_text,
            Vec2::new(count_x, query_y),
            muted_style,
            input_rect,
        );

        // Cursor beam
        let cx = text_x + visual_width(&finder.input.text[..finder.input.cursor]) as f32 * cell_size.width;
        draw_cursor_beam(renderer, cx, query_y, cell_height, p.cursor_accent);

        // File list
        let list_top = input_y + input_h + 8.0;
        let list_bottom = panel_rect.y + panel_rect.height - PANE_PADDING;
        let visible_rows = ((list_bottom - list_top) / line_height).floor() as usize;
        let list_clip = Rect::new(
            panel_rect.x + PANE_PADDING,
            list_top,
            panel_rect.width - 2.0 * PANE_PADDING,
            list_bottom - list_top,
        );

        for vi in 0..visible_rows {
            let fi = finder.scroll_offset + vi;
            if fi >= finder.filtered.len() {
                break;
            }
            let entry_idx = finder.filtered[fi];
            let rel_path = &finder.entries[entry_idx];
            let y = list_top + vi as f32 * line_height;
            if y + line_height > list_bottom {
                break;
            }

            // Selected item highlight
            if fi == finder.selected {
                let sel_rect = Rect::new(
                    panel_rect.x + PANE_PADDING,
                    y,
                    panel_rect.width - 2.0 * PANE_PADDING,
                    line_height,
                );
                renderer.draw_top_rect(sel_rect, p.popup_selected);
            }

            // File icon
            let text_offset_y = (line_height - cell_height) / 2.0;
            let file_name = rel_path.file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_default();
            let icon = file_icon(&file_name, false, false);
            let icon_style = text_style(p.tree_icon);
            let icon_x = panel_rect.x + PANE_PADDING + 4.0;
            let icon_str: String = std::iter::once(icon).collect();
            renderer.draw_top_text(
                &icon_str,
                Vec2::new(icon_x, y + text_offset_y),
                icon_style,
                list_clip,
            );

            // File path
            let path_x = icon_x + indent_width + 4.0;
            let display_path = rel_path.to_string_lossy();
            let path_color = if fi == finder.selected {
                p.tab_text_focused
            } else {
                p.tree_text
            };
            let path_style = TextStyle {
                foreground: path_color,
                background: None,
                bold: fi == finder.selected,
                dim: false,
                italic: false,
                underline: false,
            };
            renderer.draw_top_text(
                &display_path,
                Vec2::new(path_x, y + text_offset_y),
                path_style,
                list_clip,
            );
        }
    }
}

/// Render git switcher popup overlay (integrated branch + worktree popup).
fn render_git_switcher(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
) {
    let gs = match app.git_switcher {
        Some(ref gs) => gs,
        None => return,
    };

    // Dim overlay (scrim)
    draw_popup_scrim(renderer, app.logical_size(), p.popup_scrim);

    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let logical = app.logical_size();
    let geo = gs.geometry(cell_height, logical.width, logical.height);

    let line_height = geo.line_height;
    let popup_w = geo.popup_w;
    let popup_x = geo.popup_x;
    let popup_y = geo.popup_y;
    let popup_h = geo.popup_h;
    let input_h = geo.input_h;
    let tab_h = geo.tab_h;
    let max_visible = geo.max_visible;
    let new_wt_btn_h = geo.new_wt_btn_h;

    let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

    // Background + border (rounded)
    draw_popup_rounded_bg(renderer, popup_rect, p.popup_bg, p.popup_border, POPUP_CORNER_RADIUS);

    let ts = text_style(p.tab_text_focused);
    let muted_style = text_style(p.tab_text);

    // Search input
    let input_y = popup_y + 2.0;
    let input_clip = Rect::new(popup_x + POPUP_TEXT_INSET, input_y, popup_w - 2.0 * POPUP_TEXT_INSET, input_h);
    let text_y = input_y + (input_h - cell_height) / 2.0;
    let text_x = popup_x + POPUP_TEXT_INSET;
    let placeholder = match gs.mode {
        crate::GitSwitcherMode::Branches => "Filter branches...",
        crate::GitSwitcherMode::Worktrees => "Filter worktrees...",
    };
    if gs.input.is_empty() {
        renderer.draw_top_text(placeholder, Vec2::new(text_x, text_y), muted_style, input_clip);
    } else {
        renderer.draw_top_text(&gs.input.text, Vec2::new(text_x, text_y), ts, input_clip);
    }
    // Cursor beam
    let cx = text_x + visual_width(&gs.input.text[..gs.input.cursor]) as f32 * cell_size.width;
    draw_cursor_beam(renderer, cx, text_y, cell_height, p.cursor_accent);

    // Tab bar
    let tab_y = input_y + input_h;
    let tab_sep_y = tab_y + tab_h;
    renderer.draw_top_rect(Rect::new(popup_x + POPUP_SEPARATOR_INSET, tab_sep_y, popup_w - 2.0 * POPUP_SEPARATOR_INSET, POPUP_SEPARATOR), p.popup_border);

    let branches_label = "Branches";
    let worktrees_label = "Worktrees";
    let tab_pad = 12.0;
    let branches_w = branches_label.len() as f32 * cell_size.width + tab_pad * 2.0;
    let worktrees_w = worktrees_label.len() as f32 * cell_size.width + tab_pad * 2.0;
    let branches_x = popup_x + POPUP_TEXT_INSET;
    let worktrees_x = branches_x + branches_w + 4.0;
    let tab_text_y = tab_y + (tab_h - cell_height) / 2.0;

    // Active tab underline
    let active_tab_x = match gs.mode {
        crate::GitSwitcherMode::Branches => branches_x,
        crate::GitSwitcherMode::Worktrees => worktrees_x,
    };
    let active_tab_w = match gs.mode {
        crate::GitSwitcherMode::Branches => branches_w,
        crate::GitSwitcherMode::Worktrees => worktrees_w,
    };
    renderer.draw_top_rect(
        Rect::new(active_tab_x, tab_sep_y - 2.0, active_tab_w, 2.0),
        p.tab_text_focused,
    );

    let branches_style = TextStyle {
        foreground: if gs.mode == crate::GitSwitcherMode::Branches { p.tab_text_focused } else { p.tab_text },
        background: None,
        bold: gs.mode == crate::GitSwitcherMode::Branches,
        dim: false,
        italic: false,
        underline: false,
    };
    let worktrees_style = TextStyle {
        foreground: if gs.mode == crate::GitSwitcherMode::Worktrees { p.tab_text_focused } else { p.tab_text },
        background: None,
        bold: gs.mode == crate::GitSwitcherMode::Worktrees,
        dim: false,
        italic: false,
        underline: false,
    };
    let tab_clip = Rect::new(popup_x, tab_y, popup_w, tab_h);
    renderer.draw_top_text(branches_label, Vec2::new(branches_x + tab_pad, tab_text_y), branches_style, tab_clip);
    renderer.draw_top_text(worktrees_label, Vec2::new(worktrees_x + tab_pad, tab_text_y), worktrees_style, tab_clip);

    // Tab hint
    let hint = "Tab";
    let hint_w = hint.len() as f32 * cell_size.width;
    let hint_x = popup_x + popup_w - hint_w - 12.0;
    renderer.draw_top_text(hint, Vec2::new(hint_x, tab_text_y), muted_style, tab_clip);

    // List area
    let list_top = tab_sep_y + 2.0;
    let list_clip = Rect::new(popup_x, list_top, popup_w, max_visible as f32 * line_height + new_wt_btn_h);

    let btn_style = text_style(p.badge_text);

    let busy = gs.shell_busy;

    // Helper closure: render [Switch] [Pane] buttons (and optionally [×] for worktrees)
    // Returns nothing; just draws. `show_delete` controls the × button.
    // When `busy` is true, only [Pane] is shown (Switch/Delete hidden).
    let render_action_buttons = |renderer: &mut tide_renderer::WgpuRenderer,
                                  y: f32, item_y: f32, show_delete: bool| {
        let btn_h = cell_height + 2.0;
        let btn_y = y + (line_height - btn_h) / 2.0;
        let mut btn_right = popup_x + popup_w - POPUP_TEXT_INSET;

        if show_delete && !busy {
            let del_w = cell_size.width + 8.0;
            let del_x = btn_right - del_w;
            renderer.draw_top_rect(Rect::new(del_x, btn_y, del_w, btn_h), p.badge_bg);
            let del_style = text_style(p.badge_git_deletions);
            renderer.draw_top_text("\u{f00d}", Vec2::new(del_x + 4.0, item_y), del_style, list_clip);
            btn_right = del_x - 3.0;
        }

        // [Pane]
        let pane_label = "Pane";
        let pane_w = pane_label.len() as f32 * cell_size.width + 10.0;
        let pane_x = btn_right - pane_w;
        renderer.draw_top_rect(Rect::new(pane_x, btn_y, pane_w, btn_h), p.badge_bg);
        renderer.draw_top_text(pane_label, Vec2::new(pane_x + 5.0, item_y), btn_style, list_clip);

        if !busy {
            btn_right = pane_x - 3.0;

            // [Switch]
            let switch_label = "Switch";
            let switch_w = switch_label.len() as f32 * cell_size.width + 10.0;
            let switch_x = btn_right - switch_w;
            renderer.draw_top_rect(Rect::new(switch_x, btn_y, switch_w, btn_h), p.badge_bg);
            renderer.draw_top_text(switch_label, Vec2::new(switch_x + 5.0, item_y), btn_style, list_clip);
        }
    };

    let base_len = gs.base_filtered_len();

    match gs.mode {
        crate::GitSwitcherMode::Branches => {
            for vi in 0..max_visible {
                let fi = gs.scroll_offset + vi;
                if fi >= base_len {
                    // Might be the create row — handled below
                    break;
                }
                let entry_idx = gs.filtered_branches[fi];
                let branch = &gs.branches[entry_idx];
                let y = list_top + vi as f32 * line_height;

                // Selected highlight
                if fi == gs.selected {
                    renderer.draw_top_rect(
                        Rect::new(popup_x + POPUP_SELECTED_INSET, y, popup_w - 2.0 * POPUP_SELECTED_INSET, line_height),
                        p.popup_selected,
                    );
                }

                let item_x = popup_x + POPUP_TEXT_INSET;
                let item_y = y + (line_height - cell_height) / 2.0;

                // Current branch checkmark
                if branch.is_current {
                    let check_style = bold_style(p.badge_git_branch);
                    renderer.draw_top_text("\u{f00c}", Vec2::new(item_x, item_y), check_style, list_clip);
                }

                // Worktree indicator (tree icon if branch has a worktree)
                let has_wt = gs.worktree_branch_names.contains(&branch.name);
                if has_wt {
                    let wt_icon_x = item_x + 1.5 * cell_size.width;
                    let wt_style = text_style(p.badge_git_worktree);
                    renderer.draw_top_text("\u{f1bb}", Vec2::new(wt_icon_x, item_y), wt_style, list_clip);
                }

                // Branch name
                let name_x = item_x + 3.5 * cell_size.width;
                let name_color = if branch.is_current {
                    p.badge_git_branch
                } else {
                    p.tab_text_focused
                };
                let name_style = TextStyle {
                    foreground: name_color,
                    background: None,
                    bold: fi == gs.selected,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                renderer.draw_top_text(&branch.name, Vec2::new(name_x, item_y), name_style, list_clip);

                // Action buttons — not for current branch
                if !branch.is_current {
                    let show_delete = !has_wt;
                    render_action_buttons(renderer, y, item_y, show_delete);
                }
            }
        }
        crate::GitSwitcherMode::Worktrees => {
            for vi in 0..max_visible {
                let fi = gs.scroll_offset + vi;
                if fi >= base_len {
                    break;
                }
                let entry_idx = gs.filtered_worktrees[fi];
                let wt = &gs.worktrees[entry_idx];
                let y = list_top + vi as f32 * line_height;

                // Selected highlight
                if fi == gs.selected {
                    renderer.draw_top_rect(
                        Rect::new(popup_x + POPUP_SELECTED_INSET, y, popup_w - 2.0 * POPUP_SELECTED_INSET, line_height),
                        p.popup_selected,
                    );
                }

                let item_x = popup_x + POPUP_TEXT_INSET;
                let item_y = y + (line_height - cell_height) / 2.0;

                // Current worktree checkmark
                if wt.is_current {
                    let check_style = bold_style(p.badge_git_worktree);
                    renderer.draw_top_text("\u{f00c}", Vec2::new(item_x, item_y), check_style, list_clip);
                }

                // Branch name or "(detached)"
                let name = wt.branch.as_deref().unwrap_or("(detached)");
                let name_x = item_x + 2.0 * cell_size.width;
                let name_style = TextStyle {
                    foreground: if wt.is_current { p.badge_git_worktree } else { p.tab_text_focused },
                    background: None,
                    bold: fi == gs.selected,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                renderer.draw_top_text(name, Vec2::new(name_x, item_y), name_style, list_clip);

                // Abbreviated path
                let path_display = abbreviate_path(&wt.path);
                let path_x = name_x + (name.len() as f32 + 1.0) * cell_size.width;
                renderer.draw_top_text(&path_display, Vec2::new(path_x, item_y), muted_style, list_clip);

                // Action buttons — not for current worktree
                if !wt.is_current {
                    let show_delete = !wt.is_main;
                    render_action_buttons(renderer, y, item_y, show_delete);
                }
            }
        }
    }

    // Create row: rendered after normal items if visible (hidden when busy)
    if gs.has_create_row() && !busy {
        let create_fi = base_len;
        // Check if create row is within the visible window
        if create_fi >= gs.scroll_offset && create_fi < gs.scroll_offset + max_visible {
            let vi = create_fi - gs.scroll_offset;
            let y = list_top + vi as f32 * line_height;
            let item_x = popup_x + POPUP_TEXT_INSET;
            let item_y = y + (line_height - cell_height) / 2.0;

            // Selected highlight
            if create_fi == gs.selected {
                renderer.draw_top_rect(
                    Rect::new(popup_x + POPUP_SELECTED_INSET, y, popup_w - 2.0 * POPUP_SELECTED_INSET, line_height),
                    p.popup_selected,
                );
            }

            // "+" icon
            let plus_style = bold_style(p.badge_git_branch);
            renderer.draw_top_text("+", Vec2::new(item_x, item_y), plus_style, list_clip);

            // Query text as the name
            let name_x = item_x + 2.0 * cell_size.width;
            let create_name_style = TextStyle {
                foreground: p.tab_text_focused,
                background: None,
                bold: create_fi == gs.selected,
                dim: false,
                italic: false,
                underline: false,
            };
            renderer.draw_top_text(gs.input.text.trim(), Vec2::new(name_x, item_y), create_name_style, list_clip);

            // [Switch] [Pane] buttons (no Delete)
            render_action_buttons(renderer, y, item_y, false);
        }
    }
}

// Re-use abbreviate_path from ui_state
use crate::ui_state::abbreviate_path;

/// Render context menu popup (right-click on file tree).
fn render_context_menu(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
) {
    let menu = match app.context_menu {
        Some(ref m) => m,
        None => return,
    };

    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let logical = app.logical_size();
    let rect = menu.geometry(cell_height, logical.width, logical.height);
    let line_height = cell_height + POPUP_LINE_EXTRA;

    // Background + border (rounded)
    draw_popup_rounded_bg(renderer, rect, p.popup_bg, p.popup_border, POPUP_CORNER_RADIUS);

    // Items
    let actions = crate::ContextMenuAction::ALL;
    for (i, action) in actions.iter().enumerate() {
        let y = rect.y + 4.0 + i as f32 * line_height;

        // Selected highlight
        if i == menu.selected {
            let sel_rect = Rect::new(
                rect.x + POPUP_SELECTED_INSET,
                y,
                rect.width - 2.0 * POPUP_SELECTED_INSET,
                line_height,
            );
            renderer.draw_top_rect(sel_rect, p.popup_selected);
        }

        let item_x = rect.x + POPUP_TEXT_INSET;
        let item_y = y + (line_height - cell_height) / 2.0;
        let item_clip = Rect::new(rect.x, y, rect.width, line_height);

        // Icon
        let icon_style = text_style(p.tree_icon);
        renderer.draw_top_text(action.icon(), Vec2::new(item_x, item_y), icon_style, item_clip);

        // Label
        let label_x = item_x + 2.5 * cell_size.width;
        let label_color = if i == menu.selected { p.tab_text_focused } else { p.tree_text };
        let label_style = TextStyle {
            foreground: label_color,
            background: None,
            bold: i == menu.selected,
            dim: false,
            italic: false,
            underline: false,
        };
        renderer.draw_top_text(action.label(), Vec2::new(label_x, item_y), label_style, item_clip);
    }
}

/// Render file switcher popup overlay.
fn render_file_switcher(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
) {
    if let Some(ref fs) = app.file_switcher {
        // Dim overlay (scrim)
        draw_popup_scrim(renderer, app.logical_size(), p.popup_scrim);

        let cell_size = renderer.cell_size();
        let cell_height = cell_size.height;
        let geo = fs.geometry(cell_height);

        let popup_x = geo.popup_x;
        let popup_y = geo.popup_y;
        let popup_w = geo.popup_w;
        let popup_h = geo.popup_h;
        let input_h = geo.input_h;
        let line_height = geo.line_height;
        let max_visible = geo.max_visible;

        let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

        // Background + border (rounded)
        draw_popup_rounded_bg(renderer, popup_rect, p.popup_bg, p.popup_border, POPUP_CORNER_RADIUS);

        // Search input
        let input_y = popup_y + 2.0;
        let input_clip = Rect::new(popup_x + POPUP_TEXT_INSET, input_y, popup_w - 2.0 * POPUP_TEXT_INSET, input_h);
        let ts = text_style(p.tab_text_focused);
        let muted_style = text_style(p.tab_text);
        let text_y = input_y + (input_h - cell_height) / 2.0;
        let text_x = popup_x + POPUP_TEXT_INSET;
        if fs.input.is_empty() {
            renderer.draw_top_text(
                "Switch to file...",
                Vec2::new(text_x, text_y),
                muted_style,
                input_clip,
            );
        } else {
            renderer.draw_top_text(
                &fs.input.text,
                Vec2::new(text_x, text_y),
                ts,
                input_clip,
            );
        }
        // Cursor beam
        let cx = text_x + visual_width(&fs.input.text[..fs.input.cursor]) as f32 * cell_size.width;
        draw_cursor_beam(renderer, cx, text_y, cell_height, p.cursor_accent);

        // Separator line
        let sep_y = input_y + input_h;
        renderer.draw_top_rect(Rect::new(popup_x + POPUP_SEPARATOR_INSET, sep_y, popup_w - 2.0 * POPUP_SEPARATOR_INSET, POPUP_SEPARATOR), p.popup_border);

        // File list
        let list_top = geo.list_top;
        let list_clip = Rect::new(popup_x, list_top, popup_w, max_visible as f32 * line_height);
        for vi in 0..max_visible {
            let fi = fs.scroll_offset + vi;
            if fi >= fs.filtered.len() {
                break;
            }
            let entry_idx = fs.filtered[fi];
            let entry = &fs.entries[entry_idx];
            let y = list_top + vi as f32 * line_height;

            // Selected highlight
            if fi == fs.selected {
                renderer.draw_top_rect(
                    Rect::new(popup_x + POPUP_SELECTED_INSET, y, popup_w - 2.0 * POPUP_SELECTED_INSET, line_height),
                    p.popup_selected,
                );
            }

            let item_x = popup_x + POPUP_TEXT_INSET;
            let item_y = y + (line_height - cell_height) / 2.0;

            // File icon + name
            let icon = crate::ui::file_icon(&entry.name, false, false);
            let display = format!("{} {}", icon, entry.name);
            let item_style = TextStyle {
                foreground: if entry.is_active { p.tab_text_focused } else { p.tab_text },
                background: None,
                bold: fi == fs.selected || entry.is_active,
                dim: false,
                italic: false,
                underline: false,
            };
            renderer.draw_top_text(
                &display,
                Vec2::new(item_x, item_y),
                item_style,
                list_clip,
            );
        }
    }
}
