use tide_core::{FileTreeSource, Rect, Renderer, TextStyle, Vec2};

use crate::drag_drop::HoverTarget;
use crate::header;
use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui::{file_icon, panel_tab_title};
use crate::{App, PaneAreaMode};

/// Render the chrome layer: file tree panel, editor panel + tabs, pane backgrounds,
/// focused borders, headers, and grip dots.
///
/// This is called only when `chrome_dirty` is true (i.e. chrome_generation changed).
pub(crate) fn render_chrome(
    app: &mut App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    logical: tide_core::Size,
    focused: Option<u64>,
    show_file_tree: bool,
    file_tree_scroll: f32,
    visual_pane_rects: &[(u64, Rect)],
    editor_panel_rect: Option<Rect>,
    editor_panel_tabs: &[u64],
    editor_panel_active: Option<u64>,
    pane_area_mode: PaneAreaMode,
    all_pane_ids: &[u64],
) {
    renderer.invalidate_chrome();

    // Draw file tree panel if visible (flat, edge-to-edge)
    if show_file_tree {
        let tree_visual_rect = app.file_tree_rect.unwrap_or(Rect::new(
            0.0,
            app.top_inset,
            app.file_tree_width - PANE_GAP,
            logical.height - app.top_inset,
        ));
        renderer.draw_chrome_rect(tree_visual_rect, p.file_tree_bg);

        if let Some(tree) = app.file_tree.as_ref() {
            let cell_size = renderer.cell_size();
            let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
            let indent_width = cell_size.width * 1.5;
            let left_padding = PANE_PADDING;

            let entries = tree.visible_entries();
            let text_offset_y = (line_height - cell_size.height) / 2.0;
            for (i, entry) in entries.iter().enumerate() {
                let y = tree_visual_rect.y + PANE_PADDING + i as f32 * line_height - file_tree_scroll;
                if y + line_height < tree_visual_rect.y || y > tree_visual_rect.y + tree_visual_rect.height {
                    continue;
                }

                let text_y = y + text_offset_y;
                let x = tree_visual_rect.x + left_padding + entry.depth as f32 * indent_width;

                // Nerd Font icon
                let icon = file_icon(&entry.entry.name, entry.entry.is_dir, entry.is_expanded);
                let icon_color = if entry.entry.is_dir {
                    p.tree_dir
                } else {
                    p.tree_icon
                };

                // Draw icon
                let icon_style = TextStyle {
                    foreground: icon_color,
                    background: None,
                    bold: false,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                let icon_str: String = std::iter::once(icon).collect();
                renderer.draw_chrome_text(
                    &icon_str,
                    Vec2::new(x, text_y),
                    icon_style,
                    tree_visual_rect,
                );

                // Draw name after icon + space
                let name_x = x + cell_size.width * 2.0;
                let text_color = if entry.entry.is_dir {
                    p.tree_dir
                } else {
                    p.tree_text
                };
                let name_style = TextStyle {
                    foreground: text_color,
                    background: None,
                    bold: entry.entry.is_dir,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                renderer.draw_chrome_text(
                    &entry.entry.name,
                    Vec2::new(name_x, text_y),
                    name_style,
                    tree_visual_rect,
                );
            }
        }
    }

    // Draw editor panel if visible (flat, border provided by clear color)
    if let Some(panel_rect) = editor_panel_rect {
        renderer.draw_chrome_rect(panel_rect, p.surface_bg);

        if !editor_panel_tabs.is_empty() {
            let cell_size = renderer.cell_size();
            let cell_height = cell_size.height;
            let tab_bar_top = panel_rect.y + PANE_PADDING;
            let tab_start_x = panel_rect.x + PANE_PADDING - app.panel_tab_scroll;
            let tab_bar_clip = Rect::new(
                panel_rect.x + PANE_PADDING,
                tab_bar_top,
                panel_rect.width - 2.0 * PANE_PADDING,
                PANEL_TAB_HEIGHT,
            );

            // Draw horizontal tab bar (with scroll offset)
            for (i, &tab_id) in editor_panel_tabs.iter().enumerate() {
                let tx = tab_start_x + i as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);

                // Skip tabs entirely outside visible area
                if tx + PANEL_TAB_WIDTH < tab_bar_clip.x || tx > tab_bar_clip.x + tab_bar_clip.width {
                    continue;
                }

                let is_active = editor_panel_active == Some(tab_id);

                // Tab background
                if is_active {
                    let tab_bg_rect = Rect::new(tx, tab_bar_top, PANEL_TAB_WIDTH, PANEL_TAB_HEIGHT);
                    renderer.draw_chrome_rounded_rect(tab_bg_rect, p.panel_tab_bg_active, 4.0);
                }

                // Tab title — clip to both tab bounds and panel bounds
                let text_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
                let title_clip_w = (PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - PANEL_TAB_CLOSE_PADDING - PANEL_TAB_TEXT_INSET - 2.0)
                    .min((tab_bar_clip.x + tab_bar_clip.width - tx).max(0.0));
                let clip_x = tx.max(tab_bar_clip.x);
                let clip = Rect::new(clip_x, tab_bar_top, title_clip_w.max(0.0), PANEL_TAB_HEIGHT);

                let title = panel_tab_title(&app.panes, tab_id);
                let text_color = if is_active && focused == Some(tab_id) {
                    p.tab_text_focused
                } else if is_active {
                    p.tree_text
                } else {
                    p.tab_text
                };
                let style = TextStyle {
                    foreground: text_color,
                    background: None,
                    bold: is_active,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                renderer.draw_chrome_text(
                    &title,
                    Vec2::new(tx + PANEL_TAB_TEXT_INSET, text_y),
                    style,
                    clip,
                );

                // Close / modified indicator button
                let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - PANEL_TAB_CLOSE_PADDING;
                let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - PANEL_TAB_CLOSE_SIZE) / 2.0;
                // Only draw close button if it's within visible area
                if close_x + PANEL_TAB_CLOSE_SIZE > tab_bar_clip.x
                    && close_x < tab_bar_clip.x + tab_bar_clip.width
                {
                    let is_modified = app.panes.get(&tab_id)
                        .and_then(|pk| if let PaneKind::Editor(ep) = pk { Some(ep.editor.is_modified()) } else { None })
                        .unwrap_or(false);
                    let is_close_hovered = matches!(app.hover_target, Some(HoverTarget::PanelTabClose(hid)) if hid == tab_id);
                    let (icon, icon_color) = if is_modified && !is_close_hovered {
                        ("\u{f111}", p.editor_modified)  // in modified color
                    } else {
                        ("\u{f00d}", p.tab_text)  // in normal color
                    };
                    let close_style = TextStyle {
                        foreground: icon_color,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    let close_clip = Rect::new(close_x, tab_bar_top, PANEL_TAB_CLOSE_SIZE + PANEL_TAB_CLOSE_PADDING, PANEL_TAB_HEIGHT);
                    renderer.draw_chrome_text(
                        icon,
                        Vec2::new(close_x, close_y),
                        close_style,
                        close_clip,
                    );
                }
            }

        } else if app.file_finder.is_none() {
            // Empty state: "No files open" + "New File" + "Open File" buttons
            let cell_size = renderer.cell_size();
            let cell_height = cell_size.height;

            // "No files open" text at ~38% height
            let label = "No files open";
            let label_w = label.len() as f32 * cell_size.width;
            let label_x = panel_rect.x + (panel_rect.width - label_w) / 2.0;
            let label_y = panel_rect.y + panel_rect.height * 0.38;
            let muted_style = TextStyle {
                foreground: p.tab_text,
                background: None,
                bold: false,
                dim: false,
                italic: false,
                underline: false,
            };
            renderer.draw_chrome_text(
                label,
                Vec2::new(label_x, label_y),
                muted_style,
                panel_rect,
            );

            // "New File" button
            let btn_text = "New File";
            let hint_text = "  Cmd+Shift+E";
            let btn_w = (btn_text.len() + hint_text.len()) as f32 * cell_size.width + 24.0;
            let btn_h = cell_height + 12.0;
            let btn_x = panel_rect.x + (panel_rect.width - btn_w) / 2.0;
            let btn_y = label_y + cell_height + 16.0;
            let btn_rect = Rect::new(btn_x, btn_y, btn_w, btn_h);
            renderer.draw_chrome_rounded_rect(btn_rect, p.panel_tab_bg_active, 4.0);

            let btn_text_y = btn_y + (btn_h - cell_height) / 2.0;
            let btn_style = TextStyle {
                foreground: p.tab_text_focused,
                background: None,
                bold: true,
                dim: false,
                italic: false,
                underline: false,
            };
            renderer.draw_chrome_text(
                btn_text,
                Vec2::new(btn_x + 12.0, btn_text_y),
                btn_style,
                btn_rect,
            );
            let hint_x = btn_x + 12.0 + btn_text.len() as f32 * cell_size.width;
            renderer.draw_chrome_text(
                hint_text,
                Vec2::new(hint_x, btn_text_y),
                muted_style,
                btn_rect,
            );

            // "Open File" button
            let open_btn_text = "Open File";
            let open_hint_text = "  Cmd+O";
            let open_btn_w = (open_btn_text.len() + open_hint_text.len()) as f32 * cell_size.width + 24.0;
            let open_btn_x = panel_rect.x + (panel_rect.width - open_btn_w) / 2.0;
            let open_btn_y = btn_y + btn_h + 8.0;
            let open_btn_rect = Rect::new(open_btn_x, open_btn_y, open_btn_w, btn_h);
            renderer.draw_chrome_rounded_rect(open_btn_rect, p.panel_tab_bg_active, 4.0);

            let open_btn_text_y = open_btn_y + (btn_h - cell_height) / 2.0;
            renderer.draw_chrome_text(
                open_btn_text,
                Vec2::new(open_btn_x + 12.0, open_btn_text_y),
                btn_style,
                open_btn_rect,
            );
            let open_hint_x = open_btn_x + 12.0 + open_btn_text.len() as f32 * cell_size.width;
            renderer.draw_chrome_text(
                open_hint_text,
                Vec2::new(open_hint_x, open_btn_text_y),
                muted_style,
                open_btn_rect,
            );
        }

        // Accent border around focused panel
        if let Some(active) = editor_panel_active {
            if focused == Some(active) {
                let r = panel_rect;
                // top
                renderer.draw_chrome_rect(Rect::new(r.x, r.y, r.width, BORDER_WIDTH), p.border_focused);
                // bottom
                renderer.draw_chrome_rect(Rect::new(r.x, r.y + r.height - BORDER_WIDTH, r.width, BORDER_WIDTH), p.border_focused);
                // left
                renderer.draw_chrome_rect(Rect::new(r.x, r.y, BORDER_WIDTH, r.height), p.border_focused);
                // right
                renderer.draw_chrome_rect(Rect::new(r.x + r.width - BORDER_WIDTH, r.y, BORDER_WIDTH, r.height), p.border_focused);
            }
        }
    }

    // Draw pane backgrounds (flat, unified surface color)
    for &(_id, rect) in visual_pane_rects {
        renderer.draw_chrome_rect(rect, p.surface_bg);
    }

    // Accent border around focused pane
    if let Some(fid) = focused {
        if let Some(&(_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == fid) {
            // top
            renderer.draw_chrome_rect(Rect::new(rect.x, rect.y, rect.width, BORDER_WIDTH), p.border_focused);
            // bottom
            renderer.draw_chrome_rect(Rect::new(rect.x, rect.y + rect.height - BORDER_WIDTH, rect.width, BORDER_WIDTH), p.border_focused);
            // left
            renderer.draw_chrome_rect(Rect::new(rect.x, rect.y, BORDER_WIDTH, rect.height), p.border_focused);
            // right
            renderer.draw_chrome_rect(Rect::new(rect.x + rect.width - BORDER_WIDTH, rect.y, BORDER_WIDTH, rect.height), p.border_focused);
        }
    }

    // Stacked mode: render dock-style tab bar; Split mode: render per-pane headers
    let mut all_hit_zones = Vec::new();
    if let PaneAreaMode::Stacked(stacked_active) = pane_area_mode {
        render_stacked_tab_bar(
            app, renderer, p,
            visual_pane_rects, all_pane_ids,
            stacked_active, focused,
        );
    } else {
        // Split mode: Header (title + badges + close) for each pane
        for &(id, rect) in visual_pane_rects {
            let zones = header::render_pane_header(
                id, rect, &app.panes, focused, p, renderer,
            );
            all_hit_zones.extend(zones);
        }
    }
    app.header_hit_zones = all_hit_zones;

    // Render grip handle dots at the top center of sidebar and dock panels
    {
        let dot_size = 2.0_f32;
        let dot_gap = 3.0_f32;
        let dot_count = 3;
        let total_w = dot_count as f32 * dot_size + (dot_count - 1) as f32 * dot_gap;

        // Sidebar grip dots (top center of file tree)
        if show_file_tree {
            if let Some(ft_rect) = app.file_tree_rect {
                let dot_y = ft_rect.y + (PANE_PADDING - dot_size) / 2.0;
                let center_x = ft_rect.x + ft_rect.width / 2.0;
                for i in 0..dot_count {
                    let dx = center_x - total_w / 2.0 + i as f32 * (dot_size + dot_gap);
                    renderer.draw_chrome_rounded_rect(
                        Rect::new(dx, dot_y, dot_size, dot_size),
                        p.handle_dots,
                        1.0,
                    );
                }
            }
        }

        // Dock grip dots (top center of editor panel)
        if let Some(panel_rect) = editor_panel_rect {
            let dot_y = panel_rect.y + (PANE_PADDING - dot_size) / 2.0;
            let center_x = panel_rect.x + panel_rect.width / 2.0;
            for i in 0..dot_count {
                let dx = center_x - total_w / 2.0 + i as f32 * (dot_size + dot_gap);
                renderer.draw_chrome_rounded_rect(
                    Rect::new(dx, dot_y, dot_size, dot_size),
                    p.handle_dots,
                    1.0,
                );
            }
        }
    }
}

/// Render the horizontal tab bar for stacked pane mode.
fn render_stacked_tab_bar(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    all_pane_ids: &[u64],
    stacked_active: u64,
    focused: Option<u64>,
) {
    let Some(&(_, rect)) = visual_pane_rects.first() else {
        return;
    };
    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let tab_bar_top = rect.y + PANE_PADDING;
    let tab_start_x = rect.x + PANE_PADDING;
    let tab_bar_clip = Rect::new(
        rect.x + PANE_PADDING,
        tab_bar_top,
        rect.width - 2.0 * PANE_PADDING,
        PANEL_TAB_HEIGHT,
    );

    for (i, &tab_id) in all_pane_ids.iter().enumerate() {
        let tx = tab_start_x + i as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);

        // Skip tabs entirely outside visible area
        if tx + PANEL_TAB_WIDTH < tab_bar_clip.x || tx > tab_bar_clip.x + tab_bar_clip.width {
            continue;
        }

        let is_active = stacked_active == tab_id;

        // Tab background
        if is_active {
            let tab_bg_rect = Rect::new(tx, tab_bar_top, PANEL_TAB_WIDTH, PANEL_TAB_HEIGHT);
            renderer.draw_chrome_rounded_rect(tab_bg_rect, p.panel_tab_bg_active, 4.0);
        }

        // Tab title — clip to leave room for close button
        let text_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
        let title_clip_w = (PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - PANEL_TAB_CLOSE_PADDING - PANEL_TAB_TEXT_INSET - 2.0)
            .min((tab_bar_clip.x + tab_bar_clip.width - tx).max(0.0));
        let clip_x = tx.max(tab_bar_clip.x);
        let clip = Rect::new(clip_x, tab_bar_top, title_clip_w.max(0.0), PANEL_TAB_HEIGHT);

        let title = panel_tab_title(&app.panes, tab_id);
        let text_color = if is_active && focused == Some(tab_id) {
            p.tab_text_focused
        } else if is_active {
            p.tree_text
        } else {
            p.tab_text
        };
        let style = TextStyle {
            foreground: text_color,
            background: None,
            bold: is_active,
            dim: false,
            italic: false,
            underline: false,
        };
        renderer.draw_chrome_text(
            &title,
            Vec2::new(tx + PANEL_TAB_TEXT_INSET, text_y),
            style,
            clip,
        );

        // Close button — use PANEL_TAB_CLOSE_SIZE for y to match hit-test geometry
        let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - PANEL_TAB_CLOSE_PADDING;
        let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - PANEL_TAB_CLOSE_SIZE) / 2.0;
        if close_x + PANEL_TAB_CLOSE_SIZE > tab_bar_clip.x
            && close_x < tab_bar_clip.x + tab_bar_clip.width
        {
            let is_close_hovered = matches!(app.hover_target, Some(HoverTarget::StackedTabClose(hid)) if hid == tab_id);
            let icon_color = if is_close_hovered { p.tab_text_focused } else { p.tab_text };
            let close_style = TextStyle {
                foreground: icon_color,
                background: None,
                bold: false,
                dim: false,
                italic: false,
                underline: false,
            };
            let close_clip = Rect::new(close_x, tab_bar_top, PANEL_TAB_CLOSE_SIZE + PANEL_TAB_CLOSE_PADDING, PANEL_TAB_HEIGHT);
            renderer.draw_chrome_text(
                "\u{f00d}",
                Vec2::new(close_x, close_y),
                close_style,
                close_clip,
            );
        }
    }
}
