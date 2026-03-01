use unicode_width::UnicodeWidthChar;

use tide_core::{Color, Rect, Renderer, TextStyle, Vec2};

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
    render_file_finder(app, renderer, p);
    render_git_switcher(app, renderer, p);
    render_file_switcher(app, renderer, p);
    render_context_menu(app, renderer, p);
    render_config_page(app, renderer, p);
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
        // Check for branch cleanup bar
        if let Some(ref bc) = app.branch_cleanup {
            if bc.pane_id == pane_id {
                renderer.draw_top_rect(bar_rect, p.conflict_bar_bg);
                let text_y = bar_rect.y + (CONFLICT_BAR_HEIGHT - cell_size.height) / 2.0;
                let ts = text_style(p.conflict_bar_text);
                let msg = if bc.worktree_path.is_some() {
                    format!("Delete worktree + branch '{}'?", bc.branch)
                } else {
                    format!("Delete branch '{}'?", bc.branch)
                };
                renderer.draw_top_text(&msg, Vec2::new(bar_rect.x + 8.0, text_y), ts, bar_rect);

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

                // Keep button
                let keep_text = "Keep";
                let keep_w = keep_text.len() as f32 * cell_size.width + btn_pad * 2.0;
                let keep_x = cancel_x - keep_w - 4.0;
                let keep_rect = Rect::new(keep_x, btn_y, keep_w, btn_h);
                renderer.draw_top_rect(keep_rect, p.conflict_bar_btn);
                renderer.draw_top_text(keep_text, Vec2::new(keep_x + btn_pad, text_y), btn_style, keep_rect);

                // Delete button (destructive, leftmost of buttons)
                let delete_text = "Delete";
                let delete_w = delete_text.len() as f32 * cell_size.width + btn_pad * 2.0;
                let delete_x = keep_x - delete_w - 4.0;
                let delete_rect = Rect::new(delete_x, btn_y, delete_w, btn_h);
                let delete_bg = Color::new(0.6, 0.2, 0.2, 1.0);
                renderer.draw_top_rect(delete_rect, delete_bg);
                renderer.draw_top_text(delete_text, Vec2::new(delete_x + btn_pad, text_y), btn_style, delete_rect);

                continue;
            }
        }

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
) {
    let finder = match app.file_finder {
        Some(ref f) => f,
        None => return,
    };

    // Dim overlay (scrim)
    draw_popup_scrim(renderer, app.logical_size(), p.popup_scrim);

    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let logical = app.logical_size();
    let geo = finder.geometry(cell_height, logical.width, logical.height);

    let line_height = geo.line_height;
    let popup_w = geo.popup_w;
    let popup_x = geo.popup_x;
    let popup_y = geo.popup_y;
    let popup_h = geo.popup_h;
    let input_h = geo.input_h;
    let max_visible = geo.max_visible;
    let indent_width = cell_size.width * 1.5;

    let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

    // Shadow
    let shadow_color = Color::new(0.0, 0.0, 0.0, 0.25);
    renderer.draw_top_shadow(popup_rect, shadow_color, 8.0, 40.0, 0.0);

    // Background + border (rounded)
    draw_popup_rounded_bg(renderer, popup_rect, p.popup_bg, p.popup_border, POPUP_CORNER_RADIUS);

    let ts = text_style(p.tab_text_focused);
    let muted_style = text_style(p.tab_text);
    let item_pad = 12.0_f32;

    // Search input — with search icon
    let input_y = popup_y + 2.0;
    let input_clip = Rect::new(popup_x + item_pad, input_y, popup_w - 2.0 * item_pad, input_h);
    let text_y = input_y + (input_h - cell_height) / 2.0;
    let icon_x = popup_x + item_pad;
    let icon_style = text_style(p.tab_text);

    // Search icon
    renderer.draw_top_text(
        "\u{f002} ",
        Vec2::new(icon_x, text_y),
        icon_style,
        input_clip,
    );

    let text_x = icon_x + 2.0 * cell_size.width;
    let text_clip = Rect::new(text_x, input_y, popup_w - item_pad - 2.0 * cell_size.width, input_h);

    if finder.input.is_empty() {
        renderer.draw_top_text(
            "Search files...",
            Vec2::new(text_x, text_y),
            muted_style,
            text_clip,
        );
    } else {
        renderer.draw_top_text(
            &finder.input.text,
            Vec2::new(text_x, text_y),
            ts,
            text_clip,
        );
    }

    // Match count
    let count_text = format!("{}/{}", finder.filtered.len(), finder.entries.len());
    let count_w = count_text.len() as f32 * cell_size.width;
    let count_x = popup_x + popup_w - count_w - item_pad;
    renderer.draw_top_text(
        &count_text,
        Vec2::new(count_x, text_y),
        muted_style,
        input_clip,
    );

    // Cursor beam
    let cx = text_x + visual_width(&finder.input.text[..finder.input.cursor]) as f32 * cell_size.width;
    draw_cursor_beam(renderer, cx, text_y, cell_height, p.cursor_accent);

    // Separator line below input
    let sep_y = input_y + input_h;
    let sep_rect = Rect::new(popup_x + POPUP_SEPARATOR_INSET, sep_y, popup_w - 2.0 * POPUP_SEPARATOR_INSET, POPUP_SEPARATOR);
    renderer.draw_top_rect(sep_rect, p.popup_border);

    // File list
    let list_top = geo.list_top;
    let list_clip = Rect::new(
        popup_x + item_pad,
        list_top,
        popup_w - 2.0 * item_pad,
        max_visible as f32 * line_height,
    );

    for vi in 0..max_visible {
        let fi = finder.scroll_offset + vi;
        if fi >= finder.filtered.len() {
            break;
        }
        let entry_idx = finder.filtered[fi];
        let rel_path = &finder.entries[entry_idx];
        let y = list_top + vi as f32 * line_height;

        // Selected item highlight
        if fi == finder.selected {
            let sel_rect = Rect::new(
                popup_x + 2.0,
                y,
                popup_w - 4.0,
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
        let icon_x = popup_x + item_pad + 4.0;
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

    // Shadow (behind background) — always dark for depth regardless of theme
    let shadow_color = Color::new(0.0, 0.0, 0.0, 0.25);
    renderer.draw_top_shadow(popup_rect, shadow_color, 8.0, 40.0, 0.0);

    // Background + border (rounded)
    draw_popup_rounded_bg(renderer, popup_rect, p.popup_bg, p.popup_border, POPUP_CORNER_RADIUS);

    let ts = text_style(p.tab_text_focused);
    let muted_style = text_style(p.tab_text);
    let item_pad = 12.0_f32;

    // Search input — with search icon and bottom border
    let input_y = popup_y + 2.0;
    let input_clip = Rect::new(popup_x + item_pad, input_y, popup_w - 2.0 * item_pad, input_h);
    let text_y = input_y + (input_h - cell_height) / 2.0;
    let icon_x = popup_x + item_pad;
    let icon_gray = p.tab_text;
    let icon_style = text_style(icon_gray);
    renderer.draw_top_text("\u{f002}", Vec2::new(icon_x, text_y), icon_style, input_clip);
    let text_x = icon_x + cell_size.width + 6.0; // after icon + gap
    let placeholder = match gs.mode {
        crate::GitSwitcherMode::Branches => "Search branches...",
        crate::GitSwitcherMode::Worktrees => "Search worktrees...",
    };
    let placeholder_color = p.badge_text_dimmed;
    let placeholder_style = text_style(placeholder_color);
    if gs.input.is_empty() {
        renderer.draw_top_text(placeholder, Vec2::new(text_x, text_y), placeholder_style, input_clip);
    } else {
        renderer.draw_top_text(&gs.input.text, Vec2::new(text_x, text_y), ts, input_clip);
    }
    // Cursor beam
    let cx = text_x + visual_width(&gs.input.text[..gs.input.cursor]) as f32 * cell_size.width;
    draw_cursor_beam(renderer, cx, text_y, cell_height, p.cursor_accent);
    // Bottom border of search bar
    let sep_color = p.popup_border;
    renderer.draw_top_rect(Rect::new(popup_x, input_y + input_h - 1.0, popup_w, 1.0), sep_color);

    // Tab bar — two full-width centered tabs
    let tab_y = input_y + input_h;
    let tab_sep_y = tab_y + tab_h;
    // Full-width separator (1px)
    renderer.draw_top_rect(Rect::new(popup_x, tab_sep_y, popup_w, 1.0), sep_color);

    let branches_label = "Branches";
    let worktrees_label = "Worktrees";
    let half_w = popup_w / 2.0;
    let tab_text_y = tab_y + (tab_h - cell_height) / 2.0;

    // Active tab underline (2px, accent color, centered under active tab)
    let active_tab_x = match gs.mode {
        crate::GitSwitcherMode::Branches => popup_x,
        crate::GitSwitcherMode::Worktrees => popup_x + half_w,
    };
    renderer.draw_top_rect(
        Rect::new(active_tab_x, tab_sep_y - 2.0, half_w, 2.0),
        p.dock_tab_underline,
    );

    let tab_active_color = p.tab_text_focused;
    let tab_inactive_color = p.tab_text;
    let branches_style = TextStyle {
        foreground: if gs.mode == crate::GitSwitcherMode::Branches { tab_active_color } else { tab_inactive_color },
        background: None,
        bold: gs.mode == crate::GitSwitcherMode::Branches,
        dim: false,
        italic: false,
        underline: false,
    };
    let worktrees_style = TextStyle {
        foreground: if gs.mode == crate::GitSwitcherMode::Worktrees { tab_active_color } else { tab_inactive_color },
        background: None,
        bold: gs.mode == crate::GitSwitcherMode::Worktrees,
        dim: false,
        italic: false,
        underline: false,
    };
    let tab_clip = Rect::new(popup_x, tab_y, popup_w, tab_h);
    // Center each label in its half
    let branches_text_w = branches_label.len() as f32 * cell_size.width;
    let worktrees_text_w = worktrees_label.len() as f32 * cell_size.width;
    let branches_text_x = popup_x + (half_w - branches_text_w) / 2.0;
    let worktrees_text_x = popup_x + half_w + (half_w - worktrees_text_w) / 2.0;
    renderer.draw_top_text(branches_label, Vec2::new(branches_text_x, tab_text_y), branches_style, tab_clip);
    renderer.draw_top_text(worktrees_label, Vec2::new(worktrees_text_x, tab_text_y), worktrees_style, tab_clip);

    // List area (with 4px top padding per Pen design)
    let list_top = tab_sep_y + 4.0;
    let list_clip = Rect::new(popup_x, list_top, popup_w, max_visible as f32 * line_height + new_wt_btn_h);

    // Compute button zone width so we can clip text before it
    let btn_pad_h = 10.0_f32;
    let new_pane_btn_w = "New Pane".len() as f32 * cell_size.width + btn_pad_h * 2.0;
    let switch_btn_w = "Switch".len() as f32 * cell_size.width + btn_pad_h * 2.0;
    let delete_btn_w = cell_size.width + btn_pad_h * 2.0; // trash icon only
    let gap = 8.0_f32; // flex gap between items (matches Pen)
    let is_worktree_mode = gs.mode == crate::GitSwitcherMode::Worktrees;
    let busy = gs.shell_busy;
    let buttons_zone_w = if is_worktree_mode {
        new_pane_btn_w + if !busy { gap + delete_btn_w } else { 0.0 }
    } else {
        new_pane_btn_w + gap + switch_btn_w + if !busy { gap + delete_btn_w } else { 0.0 }
    };

    // Branch item style constants
    let accent_color = p.dock_tab_underline; // #C4B8A6
    let text_gray = p.tab_text_focused;
    let hint_bar_border = p.popup_border;
    let hint_text_color = p.tab_text;
    let badge_bg_color = Color::new(accent_color.r, accent_color.g, accent_color.b, 0.094);
    let switch_btn_bg = accent_color;
    // Button text must always be dark (readable on accent bg in both modes)
    let switch_btn_text_color = Color::new(0.05, 0.05, 0.05, 1.0);
    let new_pane_border_color = Color::new(p.tab_text.r, p.tab_text.g, p.tab_text.b, 0.3);

    // Delete button style constants
    let delete_border_color = Color::new(0.6, 0.2, 0.2, 1.0); // red-tinted border
    let delete_icon_color = Color::new(0.8, 0.3, 0.3, 1.0); // red-tinted icon

    let delete_confirm_idx = gs.delete_confirm;

    // Helper: render action buttons, right-aligned in row.
    // For branches: [Delete] [Switch (filled)] [New Pane (outlined)].
    // For worktrees: [Delete] [New Pane (filled, primary action)].
    // When `busy` is true, Delete and Switch are hidden.
    // `show_delete` controls whether the delete button is shown (hidden for main worktree).
    // When `fi` matches `delete_confirm`, delete button shows "Delete?" filled red.
    let render_action_buttons = |renderer: &mut tide_renderer::WgpuRenderer,
                                  y: f32, _item_y: f32, show_delete: bool, fi: usize| {
        let confirming = delete_confirm_idx == Some(fi);
        let btn_h = cell_height + 4.0; // taller buttons for 36px rows
        let btn_y = y + (line_height - btn_h) / 2.0;
        let btn_radius = 4.0_f32;
        let btn_right = popup_x + popup_w - item_pad;
        let btn_text_y = btn_y + (btn_h - cell_height) / 2.0;

        if is_worktree_mode {
            // Worktrees: single "New Pane" button (filled, primary action)
            let label = "New Pane";
            let w = label.len() as f32 * cell_size.width + btn_pad_h * 2.0;
            let x = btn_right - w;
            renderer.draw_top_rounded_rect(
                Rect::new(x, btn_y, w, btn_h),
                switch_btn_bg,
                btn_radius,
            );
            let style = TextStyle {
                foreground: switch_btn_text_color,
                background: None,
                bold: true,
                dim: false,
                italic: false,
                underline: false,
            };
            renderer.draw_top_text(label, Vec2::new(x + btn_pad_h, btn_text_y), style, list_clip);

            // Delete button — outlined red (hidden when busy or main worktree)
            if !busy && show_delete {
                if confirming {
                    // Confirmation state: filled red "Delete?" button
                    let del_label = "Delete?";
                    let del_w = del_label.len() as f32 * cell_size.width + btn_pad_h * 2.0;
                    let del_x = x - gap - del_w;
                    renderer.draw_top_rounded_rect(
                        Rect::new(del_x, btn_y, del_w, btn_h),
                        delete_border_color,
                        btn_radius,
                    );
                    let del_style = TextStyle {
                        foreground: Color::new(1.0, 1.0, 1.0, 1.0),
                        background: None,
                        bold: true, dim: false, italic: false, underline: false,
                    };
                    renderer.draw_top_text(del_label, Vec2::new(del_x + btn_pad_h, btn_text_y), del_style, list_clip);
                } else {
                    let del_w = cell_size.width + btn_pad_h * 2.0;
                    let del_x = x - gap - del_w;
                    renderer.draw_top_rounded_rect(
                        Rect::new(del_x, btn_y, del_w, btn_h),
                        delete_border_color,
                        btn_radius,
                    );
                    renderer.draw_top_rounded_rect(
                        Rect::new(del_x + 1.0, btn_y + 1.0, del_w - 2.0, btn_h - 2.0),
                        p.popup_bg,
                        (btn_radius - 1.0).max(0.0),
                    );
                    let del_style = text_style(delete_icon_color);
                    renderer.draw_top_text("\u{f1f8}", Vec2::new(del_x + btn_pad_h, btn_text_y), del_style, list_clip);
                }
            }
        } else {
            // Branches: "New Pane" (outlined) + "Switch" (filled) + Delete (outlined red)
            let mut cur_right = btn_right;

            // "New Pane" button — outlined
            let new_pane_label = "New Pane";
            let new_pane_w = new_pane_label.len() as f32 * cell_size.width + btn_pad_h * 2.0;
            let new_pane_x = cur_right - new_pane_w;
            renderer.draw_top_rounded_rect(
                Rect::new(new_pane_x, btn_y, new_pane_w, btn_h),
                new_pane_border_color,
                btn_radius,
            );
            renderer.draw_top_rounded_rect(
                Rect::new(new_pane_x + 1.0, btn_y + 1.0, new_pane_w - 2.0, btn_h - 2.0),
                p.popup_bg,
                (btn_radius - 1.0).max(0.0),
            );
            let new_pane_style = text_style(text_gray);
            renderer.draw_top_text(new_pane_label, Vec2::new(new_pane_x + btn_pad_h, btn_text_y), new_pane_style, list_clip);
            cur_right = new_pane_x - gap;

            if !busy {
                // "Switch" button — filled accent
                let switch_label = "Switch";
                let switch_w = switch_label.len() as f32 * cell_size.width + btn_pad_h * 2.0;
                let switch_x = cur_right - switch_w;
                renderer.draw_top_rounded_rect(
                    Rect::new(switch_x, btn_y, switch_w, btn_h),
                    switch_btn_bg,
                    btn_radius,
                );
                let switch_style = TextStyle {
                    foreground: switch_btn_text_color,
                    background: None,
                    bold: true,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                renderer.draw_top_text(switch_label, Vec2::new(switch_x + btn_pad_h, btn_text_y), switch_style, list_clip);
                cur_right = switch_x - gap;

                // Delete button — outlined red
                if show_delete {
                    if confirming {
                        let del_label = "Delete?";
                        let del_w = del_label.len() as f32 * cell_size.width + btn_pad_h * 2.0;
                        let del_x = cur_right - del_w;
                        renderer.draw_top_rounded_rect(
                            Rect::new(del_x, btn_y, del_w, btn_h),
                            delete_border_color,
                            btn_radius,
                        );
                        let del_style = TextStyle {
                            foreground: Color::new(1.0, 1.0, 1.0, 1.0),
                            background: None,
                            bold: true, dim: false, italic: false, underline: false,
                        };
                        renderer.draw_top_text(del_label, Vec2::new(del_x + btn_pad_h, btn_text_y), del_style, list_clip);
                    } else {
                        let del_w = cell_size.width + btn_pad_h * 2.0;
                        let del_x = cur_right - del_w;
                        renderer.draw_top_rounded_rect(
                            Rect::new(del_x, btn_y, del_w, btn_h),
                            delete_border_color,
                            btn_radius,
                        );
                        renderer.draw_top_rounded_rect(
                            Rect::new(del_x + 1.0, btn_y + 1.0, del_w - 2.0, btn_h - 2.0),
                            p.popup_bg,
                            (btn_radius - 1.0).max(0.0),
                        );
                        let del_style = text_style(delete_icon_color);
                        renderer.draw_top_text("\u{f1f8}", Vec2::new(del_x + btn_pad_h, btn_text_y), del_style, list_clip);
                    }
                }
            }
        }
    };

    let base_len = gs.base_filtered_len();

    match gs.mode {
        crate::GitSwitcherMode::Branches => {
            for vi in 0..max_visible {
                let fi = gs.scroll_offset + vi;
                if fi >= base_len {
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

                let item_x = popup_x + item_pad;
                let item_y = y + (line_height - cell_height) / 2.0;

                // Git-branch icon
                let icon_color = if branch.is_current { accent_color } else { icon_gray };
                let branch_icon_style = text_style(icon_color);
                renderer.draw_top_text("\u{e0a0}", Vec2::new(item_x, item_y), branch_icon_style, list_clip);
                let name_x = item_x + cell_size.width + 6.0; // icon width + gap

                if branch.is_current {
                    // Current branch: accent icon, white text, subtle bg tint, "current" badge
                    // Subtle accent bg tint on entire row
                    let current_row_bg = Color::new(0.769, 0.722, 0.651, 0.031); // #C4B8A608
                    renderer.draw_top_rect(
                        Rect::new(popup_x, y, popup_w, line_height),
                        current_row_bg,
                    );
                    let name_style = TextStyle {
                        foreground: p.tab_text_focused,
                        background: None,
                        bold: fi == gs.selected,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_top_text(&branch.name, Vec2::new(name_x, item_y), name_style, list_clip);

                    // "current" badge
                    let badge_label = "current";
                    let badge_w = badge_label.len() as f32 * cell_size.width + 8.0;
                    let badge_h = cell_height;
                    let badge_x = name_x + (branch.name.len() as f32 + 1.0) * cell_size.width;
                    let badge_y = y + (line_height - badge_h) / 2.0;
                    renderer.draw_top_rounded_rect(
                        Rect::new(badge_x, badge_y, badge_w, badge_h),
                        badge_bg_color,
                        4.0,
                    );
                    let badge_style = TextStyle {
                        foreground: accent_color,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_top_text(badge_label, Vec2::new(badge_x + 4.0, item_y), badge_style, list_clip);
                } else {
                    // Non-current branch: gray icon, gray text, action buttons
                    // Clip text before buttons zone
                    let icon_zone = cell_size.width + 6.0;
                    let text_clip_w = popup_w - item_pad * 2.0 - icon_zone - buttons_zone_w - 8.0;
                    let text_clip = Rect::new(name_x, y, text_clip_w.max(0.0), line_height);
                    let name_style = TextStyle {
                        foreground: text_gray,
                        background: None,
                        bold: fi == gs.selected,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_top_text(&branch.name, Vec2::new(name_x, item_y), name_style, text_clip);

                    render_action_buttons(renderer, y, item_y, true, fi);
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

                let item_x = popup_x + item_pad;
                let item_y = y + (line_height - cell_height) / 2.0;

                let name = wt.branch.as_deref().unwrap_or("(detached)");

                // Git-branch icon
                let wt_icon_color = if wt.is_current { p.badge_git_worktree } else { icon_gray };
                let wt_icon_style = text_style(wt_icon_color);
                renderer.draw_top_text("\u{e0a0}", Vec2::new(item_x, item_y), wt_icon_style, list_clip);
                let name_x = item_x + cell_size.width + 6.0;

                if wt.is_current {
                    // Current worktree: accent icon, white text, subtle bg tint, "current" badge
                    let current_row_bg = Color::new(0.769, 0.722, 0.651, 0.031); // #C4B8A608
                    renderer.draw_top_rect(
                        Rect::new(popup_x, y, popup_w, line_height),
                        current_row_bg,
                    );
                    let name_style = TextStyle {
                        foreground: p.tab_text_focused,
                        background: None,
                        bold: fi == gs.selected,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_top_text(name, Vec2::new(name_x, item_y), name_style, list_clip);

                    // "current" badge
                    let badge_label = "current";
                    let badge_w = badge_label.len() as f32 * cell_size.width + 8.0;
                    let badge_h = cell_height;
                    let badge_x = name_x + (name.len() as f32 + 1.0) * cell_size.width;
                    let badge_y = y + (line_height - badge_h) / 2.0;
                    renderer.draw_top_rounded_rect(
                        Rect::new(badge_x, badge_y, badge_w, badge_h),
                        badge_bg_color,
                        4.0,
                    );
                    let badge_style = TextStyle {
                        foreground: accent_color,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_top_text(badge_label, Vec2::new(badge_x + 4.0, item_y), badge_style, list_clip);
                } else {
                    // Non-current worktree: gray icon, gray text, path, action buttons
                    // Clip text before buttons zone
                    let icon_zone = cell_size.width + 6.0;
                    let text_clip_w = popup_w - item_pad * 2.0 - icon_zone - buttons_zone_w - 8.0;
                    let text_clip = Rect::new(name_x, y, text_clip_w.max(0.0), line_height);
                    let name_style = TextStyle {
                        foreground: text_gray,
                        background: None,
                        bold: fi == gs.selected,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_top_text(name, Vec2::new(name_x, item_y), name_style, text_clip);

                    // Abbreviated path
                    let path_display = abbreviate_path(&wt.path);
                    let path_x = name_x + (name.len() as f32 + 1.0) * cell_size.width;
                    renderer.draw_top_text(&path_display, Vec2::new(path_x, item_y), muted_style, text_clip);

                    render_action_buttons(renderer, y, item_y, !wt.is_main, fi);
                }
            }
        }
    }

    // Create row: rendered after normal items if visible (hidden when busy)
    if gs.has_create_row() && !busy {
        let create_fi = base_len;
        if create_fi >= gs.scroll_offset && create_fi < gs.scroll_offset + max_visible {
            let vi = create_fi - gs.scroll_offset;
            let y = list_top + vi as f32 * line_height;
            let item_x = popup_x + item_pad;
            let item_y = y + (line_height - cell_height) / 2.0;

            if create_fi == gs.selected {
                renderer.draw_top_rect(
                    Rect::new(popup_x + POPUP_SELECTED_INSET, y, popup_w - 2.0 * POPUP_SELECTED_INSET, line_height),
                    p.popup_selected,
                );
            }

            let plus_style = bold_style(accent_color);
            renderer.draw_top_text("+", Vec2::new(item_x, item_y), plus_style, list_clip);

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

            render_action_buttons(renderer, y, item_y, false, usize::MAX);  // no delete for create row
        }
    }

    // Hint bar at bottom
    let hint_bar_h = 28.0_f32;
    let hint_bar_y = popup_y + popup_h - hint_bar_h;
    // Top border of hint bar
    renderer.draw_top_rect(Rect::new(popup_x, hint_bar_y, popup_w, 1.0), hint_bar_border);
    // Hint text centered
    let hint_text = if is_worktree_mode {
        "\u{21B5} new pane  \u{2318}\u{232B} delete  esc close"
    } else {
        "\u{21B5} switch  \u{2318}\u{21B5} new pane  \u{2318}\u{232B} delete  esc close"
    };
    let hint_text_w = hint_text.len() as f32 * cell_size.width;
    let hint_text_x = popup_x + (popup_w - hint_text_w) / 2.0;
    let hint_text_y = hint_bar_y + (hint_bar_h - cell_height) / 2.0;
    let hint_style = TextStyle {
        foreground: hint_text_color,
        background: None,
        bold: false,
        dim: false,
        italic: false,
        underline: false,
    };
    let hint_clip = Rect::new(popup_x, hint_bar_y, popup_w, hint_bar_h);
    renderer.draw_top_text(hint_text, Vec2::new(hint_text_x, hint_text_y), hint_style, hint_clip);
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
    let actions = menu.items();
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

/// Render the config page overlay (settings modal).
fn render_config_page(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
) {
    let page = match app.config_page {
        Some(ref page) => page,
        None => return,
    };

    use crate::ui_state::ConfigSection;

    // Dim overlay (scrim)
    draw_popup_scrim(renderer, app.logical_size(), p.popup_scrim);

    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let logical = app.logical_size();

    // Popup dimensions
    let popup_w = CONFIG_PAGE_W.min(logical.width - 80.0).max(300.0);
    let popup_h = CONFIG_PAGE_MAX_H.min(logical.height - 80.0).max(200.0);
    let popup_x = (logical.width - popup_w) / 2.0;
    let popup_y = (logical.height - popup_h) / 2.0;
    let popup_rect = Rect::new(popup_x, popup_y, popup_w, popup_h);

    // Shadow — always dark for depth
    let shadow_color = Color::new(0.0, 0.0, 0.0, 0.25);
    renderer.draw_top_shadow(popup_rect, shadow_color, 8.0, 40.0, 0.0);

    // Background + border (rounded)
    draw_popup_rounded_bg(renderer, popup_rect, p.popup_bg, p.popup_border, POPUP_CORNER_RADIUS);

    let item_pad = 12.0_f32;
    let sep_color = p.popup_border;
    let accent_color = p.dock_tab_underline;
    let tab_active_color = p.tab_text_focused;
    let tab_inactive_color = p.tab_text;
    let hint_text_color = p.badge_text_dimmed;

    // ── Title bar ──
    let title_h = CONFIG_PAGE_TITLE_H;
    let title_y = popup_y + 2.0;
    let title_text_y = title_y + (title_h - cell_height) / 2.0;
    let title_style = bold_style(p.tab_text_focused);
    let title_clip = Rect::new(popup_x + item_pad, title_y, popup_w - 2.0 * item_pad, title_h);
    renderer.draw_top_text("Settings", Vec2::new(popup_x + item_pad, title_text_y), title_style, title_clip);
    renderer.draw_top_rect(Rect::new(popup_x, title_y + title_h, popup_w, 1.0), sep_color);

    // ── Tab bar ──
    let tab_h = CONFIG_PAGE_TAB_H;
    let tab_y = title_y + title_h + 1.0;
    let tab_text_y = tab_y + (tab_h - cell_height) / 2.0;
    let half_w = popup_w / 2.0;

    let keybindings_label = "Keybindings";
    let worktree_label = "Worktree";

    let kb_active = page.section == ConfigSection::Keybindings;
    let kb_style = TextStyle {
        foreground: if kb_active { tab_active_color } else { tab_inactive_color },
        background: None,
        bold: kb_active,
        dim: false,
        italic: false,
        underline: false,
    };
    let wt_style = TextStyle {
        foreground: if !kb_active { tab_active_color } else { tab_inactive_color },
        background: None,
        bold: !kb_active,
        dim: false,
        italic: false,
        underline: false,
    };
    let tab_clip = Rect::new(popup_x, tab_y, popup_w, tab_h);
    let kb_text_w = keybindings_label.len() as f32 * cell_size.width;
    let wt_text_w = worktree_label.len() as f32 * cell_size.width;
    let kb_text_x = popup_x + (half_w - kb_text_w) / 2.0;
    let wt_text_x = popup_x + half_w + (half_w - wt_text_w) / 2.0;
    renderer.draw_top_text(keybindings_label, Vec2::new(kb_text_x, tab_text_y), kb_style, tab_clip);
    renderer.draw_top_text(worktree_label, Vec2::new(wt_text_x, tab_text_y), wt_style, tab_clip);

    // Active tab underline
    let active_tab_x = if kb_active { popup_x } else { popup_x + half_w };
    renderer.draw_top_rect(Rect::new(active_tab_x, tab_y + tab_h - 2.0, half_w, 2.0), accent_color);
    renderer.draw_top_rect(Rect::new(popup_x, tab_y + tab_h, popup_w, 1.0), sep_color);

    // ── Content area ──
    let content_top = tab_y + tab_h + 1.0;
    let hint_bar_h = CONFIG_PAGE_HINT_BAR_H;
    let content_bottom = popup_y + popup_h - hint_bar_h;
    let line_height = 32.0_f32.max(cell_height + POPUP_LINE_EXTRA);

    match page.section {
        ConfigSection::Keybindings => {
            let max_visible = ((content_bottom - content_top) / line_height).floor() as usize;
            let list_clip = Rect::new(popup_x, content_top, popup_w, content_bottom - content_top);

            for vi in 0..max_visible {
                let fi = page.scroll_offset + vi;
                if fi >= page.bindings.len() {
                    break;
                }
                let (ref action, ref hotkey) = page.bindings[fi];
                let y = content_top + vi as f32 * line_height;
                if y + line_height > content_bottom {
                    break;
                }

                // Selected highlight
                if fi == page.selected {
                    renderer.draw_top_rect(
                        Rect::new(popup_x + POPUP_SELECTED_INSET, y, popup_w - 2.0 * POPUP_SELECTED_INSET, line_height),
                        p.popup_selected,
                    );
                }

                let item_y = y + (line_height - cell_height) / 2.0;

                // Action label
                let label = action.label();
                let label_color = if fi == page.selected { p.tab_text_focused } else { tab_active_color };
                let label_style = TextStyle {
                    foreground: label_color,
                    background: None,
                    bold: fi == page.selected,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                let label_clip = Rect::new(popup_x + item_pad, y, popup_w * 0.55, line_height);
                renderer.draw_top_text(label, Vec2::new(popup_x + item_pad, item_y), label_style, label_clip);

                // Recording state or hotkey display
                let is_recording = page.recording.as_ref().map_or(false, |r| r.action_index == fi);
                let hotkey_x = popup_x + popup_w * 0.55;
                let hotkey_clip = Rect::new(hotkey_x, y, popup_w * 0.35, line_height);

                if is_recording {
                    let recording_style = TextStyle {
                        foreground: accent_color,
                        background: None,
                        bold: true,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_top_text("Press key...", Vec2::new(hotkey_x, item_y), recording_style, hotkey_clip);
                } else {
                    let display = hotkey.display();
                    let hotkey_color = if fi == page.selected { p.tab_text_focused } else { tab_inactive_color };
                    let hotkey_style = text_style(hotkey_color);
                    renderer.draw_top_text(&display, Vec2::new(hotkey_x, item_y), hotkey_style, hotkey_clip);
                }

                // Edit indicator
                if fi == page.selected && !is_recording {
                    let edit_label = "\u{f044}"; // pencil icon
                    let edit_x = popup_x + popup_w - item_pad - cell_size.width;
                    let edit_style = text_style(tab_inactive_color);
                    renderer.draw_top_text(edit_label, Vec2::new(edit_x, item_y), edit_style, list_clip);
                }
            }
        }
        ConfigSection::Worktree => {
            let input_h = cell_height + POPUP_INPUT_PADDING;
            let selected_field = page.selected_field;
            let selected_border = accent_color;

            // ── Base dir pattern ──
            let y = content_top + 8.0;
            let item_y = y + (line_height - cell_height) / 2.0;

            // Label
            let label_style = bold_style(tab_active_color);
            renderer.draw_top_text("Base dir pattern:", Vec2::new(popup_x + item_pad, item_y), label_style,
                Rect::new(popup_x, y, popup_w, line_height));

            // Input field
            let wt_input_y = y + line_height + 4.0;
            let wt_input_rect = Rect::new(popup_x + item_pad, wt_input_y, popup_w - 2.0 * item_pad, input_h);
            renderer.draw_top_rect(wt_input_rect, if page.worktree_editing { p.popup_selected } else { p.surface_bg });
            let wt_border = if selected_field == 0 && !page.worktree_editing && !page.copy_files_editing { selected_border } else { p.popup_border };
            draw_popup_border(renderer, wt_input_rect, wt_border);

            let text_x = popup_x + item_pad + POPUP_TEXT_INSET;
            let text_y = wt_input_y + (input_h - cell_height) / 2.0;
            let text_clip = Rect::new(text_x, wt_input_y, popup_w - 2.0 * item_pad - 2.0 * POPUP_TEXT_INSET, input_h);

            if page.worktree_input.is_empty() && !page.worktree_editing {
                let placeholder = "{repo_root}.worktree/{branch}";
                let muted_style = text_style(tab_inactive_color);
                renderer.draw_top_text(placeholder, Vec2::new(text_x, text_y), muted_style, text_clip);
            } else {
                let ts = text_style(p.tab_text_focused);
                renderer.draw_top_text(&page.worktree_input.text, Vec2::new(text_x, text_y), ts, text_clip);
            }

            // Cursor beam when editing
            if page.worktree_editing {
                let cx = text_x + visual_width(&page.worktree_input.text[..page.worktree_input.cursor]) as f32 * cell_size.width;
                draw_cursor_beam(renderer, cx, text_y, cell_height, p.cursor_accent);
            }

            // Help text
            let help_y = wt_input_y + input_h + 8.0;
            let help_text = "Variables: {repo_root}, {branch}";
            let help_style = text_style(hint_text_color);
            renderer.draw_top_text(help_text, Vec2::new(popup_x + item_pad, help_y),
                help_style, Rect::new(popup_x, help_y, popup_w, cell_height + 4.0));

            // ── Copy files ──
            let cf_label_y = help_y + cell_height + 12.0;
            let cf_label_item_y = cf_label_y + (line_height - cell_height) / 2.0;
            renderer.draw_top_text("Copy files:", Vec2::new(popup_x + item_pad, cf_label_item_y), label_style,
                Rect::new(popup_x, cf_label_y, popup_w, line_height));

            let cf_input_y = cf_label_y + line_height + 4.0;
            let cf_input_rect = Rect::new(popup_x + item_pad, cf_input_y, popup_w - 2.0 * item_pad, input_h);
            renderer.draw_top_rect(cf_input_rect, if page.copy_files_editing { p.popup_selected } else { p.surface_bg });
            let cf_border = if selected_field == 1 && !page.worktree_editing && !page.copy_files_editing { selected_border } else { p.popup_border };
            draw_popup_border(renderer, cf_input_rect, cf_border);

            let cf_text_x = popup_x + item_pad + POPUP_TEXT_INSET;
            let cf_text_y = cf_input_y + (input_h - cell_height) / 2.0;
            let cf_text_clip = Rect::new(cf_text_x, cf_input_y, popup_w - 2.0 * item_pad - 2.0 * POPUP_TEXT_INSET, input_h);

            if page.copy_files_input.is_empty() && !page.copy_files_editing {
                let cf_placeholder = ".env, .vscode/settings.json";
                let muted_style = text_style(tab_inactive_color);
                renderer.draw_top_text(cf_placeholder, Vec2::new(cf_text_x, cf_text_y), muted_style, cf_text_clip);
            } else {
                let ts = text_style(p.tab_text_focused);
                renderer.draw_top_text(&page.copy_files_input.text, Vec2::new(cf_text_x, cf_text_y), ts, cf_text_clip);
            }

            // Cursor beam when editing
            if page.copy_files_editing {
                let cx = cf_text_x + visual_width(&page.copy_files_input.text[..page.copy_files_input.cursor]) as f32 * cell_size.width;
                draw_cursor_beam(renderer, cx, cf_text_y, cell_height, p.cursor_accent);
            }

            // Help text for copy files
            let cf_help_y = cf_input_y + input_h + 8.0;
            let cf_help_text = "Comma-separated relative paths to copy into new worktrees";
            renderer.draw_top_text(cf_help_text, Vec2::new(popup_x + item_pad, cf_help_y),
                help_style, Rect::new(popup_x, cf_help_y, popup_w, cell_height + 4.0));
        }
    }

    // ── Hint bar at bottom ──
    let hint_bar_y = popup_y + popup_h - hint_bar_h;
    renderer.draw_top_rect(Rect::new(popup_x, hint_bar_y, popup_w, 1.0), sep_color);
    let hint_text = match page.section {
        ConfigSection::Keybindings => {
            if page.recording.is_some() {
                "Press key combo  Esc cancel"
            } else {
                "Esc close  Tab section  \u{21B5} rebind  Bksp reset"
            }
        }
        ConfigSection::Worktree => {
            if page.worktree_editing || page.copy_files_editing {
                "\u{21B5} done  Esc cancel"
            } else {
                "Esc close  Tab section  \u{2191}\u{2193} select  \u{21B5} edit"
            }
        }
    };
    let hint_text_w = hint_text.len() as f32 * cell_size.width;
    let hint_text_x = popup_x + (popup_w - hint_text_w) / 2.0;
    let hint_text_y = hint_bar_y + (hint_bar_h - cell_height) / 2.0;
    let hint_style = text_style(hint_text_color);
    let hint_clip = Rect::new(popup_x, hint_bar_y, popup_w, hint_bar_h);
    renderer.draw_top_text(hint_text, Vec2::new(hint_text_x, hint_text_y), hint_style, hint_clip);
}
