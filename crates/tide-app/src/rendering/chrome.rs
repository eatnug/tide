use tide_core::{FileTreeSource, Rect, Renderer, TextStyle, Vec2};

use crate::drag_drop::HoverTarget;
use crate::header;
use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui::{dock_tab_width, file_icon, panel_tab_title, stacked_tab_width};
use crate::ui_state::FocusArea;
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

    // Draw titlebar background, border, and title (macOS transparent titlebar)
    if app.top_inset > 0.0 {
        let tb = Rect::new(0.0, 0.0, logical.width, app.top_inset);
        renderer.draw_chrome_rect(tb, p.file_tree_bg);
        // Bottom border
        renderer.draw_chrome_rect(
            Rect::new(0.0, app.top_inset - BORDER_WIDTH, logical.width, BORDER_WIDTH),
            p.border_subtle,
        );
        // Centered "tide" title
        let cs = renderer.cell_size();
        let title_text = "Tide";
        let title_w = title_text.len() as f32 * cs.width;
        let title_x = (logical.width - title_w) / 2.0;
        let title_y = (app.top_inset - cs.height) / 2.0;
        renderer.draw_chrome_text(
            title_text,
            Vec2::new(title_x, title_y),
            TextStyle {
                foreground: p.tab_text,
                background: None,
                bold: false, dim: false, italic: false, underline: false,
            },
            tb,
        );
        // Right: dock position indicator (two vertical rectangles, filled side = dock side)
        {
            let icon_h = 12.0_f32;
            let rect_w = 5.0_f32;
            let gap = 2.0_f32;
            let icon_w = rect_w * 2.0 + gap;
            let icon_x = logical.width - PANE_PADDING - icon_w;
            let icon_y = (app.top_inset - icon_h) / 2.0;
            let left_rect = Rect::new(icon_x, icon_y, rect_w, icon_h);
            let right_rect = Rect::new(icon_x + rect_w + gap, icon_y, rect_w, icon_h);
            let fill_color = p.tab_text;
            let outline_color = tide_core::Color::new(p.tab_text.r, p.tab_text.g, p.tab_text.b, 0.4);
            let bw = 1.0_f32;
            let (filled, outlined) = if app.dock_side == crate::LayoutSide::Right {
                (right_rect, left_rect)
            } else {
                (left_rect, right_rect)
            };
            // Filled rectangle
            renderer.draw_chrome_rect(filled, fill_color);
            // Outlined rectangle (4 border edges)
            let o = outlined;
            renderer.draw_chrome_rect(Rect::new(o.x, o.y, o.width, bw), outline_color);
            renderer.draw_chrome_rect(Rect::new(o.x, o.y + o.height - bw, o.width, bw), outline_color);
            renderer.draw_chrome_rect(Rect::new(o.x, o.y, bw, o.height), outline_color);
            renderer.draw_chrome_rect(Rect::new(o.x + o.width - bw, o.y, bw, o.height), outline_color);
        }
    }

    // Draw file tree panel if visible (flat, edge-to-edge)
    if show_file_tree {
        let tree_visual_rect = app.file_tree_rect.unwrap_or(Rect::new(
            0.0,
            app.top_inset,
            app.file_tree_width,
            logical.height - app.top_inset,
        ));
        renderer.draw_chrome_rect(tree_visual_rect, p.file_tree_bg);

        // Right edge border for file tree
        {
            let r = tree_visual_rect;
            let tree_focused = app.focus_area == FocusArea::FileTree;
            if tree_focused {
                // Focused: warm accent edge (2px) + subtle inner glow (4px fade)
                let accent = p.dock_tab_underline;
                let glow = tide_core::Color::new(accent.r, accent.g, accent.b, 0.10);
                renderer.draw_chrome_rect(Rect::new(r.x + r.width - 2.0, r.y, 2.0, r.height), accent);
                renderer.draw_chrome_rect(Rect::new(r.x + r.width - 6.0, r.y, 4.0, r.height), glow);
            } else {
                renderer.draw_chrome_rect(Rect::new(r.x + r.width - BORDER_WIDTH, r.y, BORDER_WIDTH, r.height), p.border_subtle);
            }
        }

        if let Some(tree) = app.file_tree.as_ref() {
            let cell_size = renderer.cell_size();
            let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
            let indent_width = cell_size.width * 1.5;
            let left_padding = PANE_PADDING;

            // Text clip rect: inset with padding on both sides (matches left_padding)
            let tree_text_clip = Rect::new(
                tree_visual_rect.x,
                tree_visual_rect.y,
                tree_visual_rect.width - PANE_PADDING,
                tree_visual_rect.height,
            );

            let entries = tree.visible_entries();
            let text_offset_y = (line_height - cell_size.height) / 2.0;
            for (i, entry) in entries.iter().enumerate() {
                // Skip entries that are being inline-renamed
                if app.file_tree_rename.as_ref().is_some_and(|r| r.entry_index == i) {
                    let y = tree_visual_rect.y + PANE_PADDING + i as f32 * line_height - file_tree_scroll;
                    if y + line_height < tree_visual_rect.y || y > tree_visual_rect.y + tree_visual_rect.height {
                        continue;
                    }
                    let text_y = y + text_offset_y;
                    let x = tree_visual_rect.x + left_padding + entry.depth as f32 * indent_width;

                    // Draw icon normally
                    let icon = file_icon(&entry.entry.name, entry.entry.is_dir, entry.is_expanded);
                    let icon_style = TextStyle {
                        foreground: p.tree_icon,
                        background: None,
                        bold: false, dim: false, italic: false, underline: false,
                    };
                    let icon_str: String = std::iter::once(icon).collect();
                    renderer.draw_chrome_text(&icon_str, Vec2::new(x, text_y), icon_style, tree_text_clip);

                    // Draw inline rename input
                    let name_x = x + cell_size.width * 2.0;
                    let rename = app.file_tree_rename.as_ref().unwrap();
                    let input_w = tree_visual_rect.x + tree_visual_rect.width - name_x - PANE_PADDING;
                    let input_rect = Rect::new(name_x - 2.0, y, input_w + 2.0, line_height);
                    renderer.draw_chrome_rect(input_rect, p.popup_bg);
                    // Border
                    renderer.draw_chrome_rect(Rect::new(input_rect.x, input_rect.y, input_rect.width, 1.0), p.popup_border);
                    renderer.draw_chrome_rect(Rect::new(input_rect.x, input_rect.y + input_rect.height - 1.0, input_rect.width, 1.0), p.popup_border);
                    renderer.draw_chrome_rect(Rect::new(input_rect.x, input_rect.y, 1.0, input_rect.height), p.popup_border);
                    renderer.draw_chrome_rect(Rect::new(input_rect.x + input_rect.width - 1.0, input_rect.y, 1.0, input_rect.height), p.popup_border);
                    // Text
                    let ts = TextStyle {
                        foreground: p.tab_text_focused,
                        background: None,
                        bold: false, dim: false, italic: false, underline: false,
                    };
                    renderer.draw_chrome_text(&rename.input.text, Vec2::new(name_x, text_y), ts, tree_text_clip);
                    // Cursor beam
                    let cursor_x = name_x + unicode_width::UnicodeWidthStr::width(&rename.input.text[..rename.input.cursor]) as f32 * cell_size.width;
                    renderer.draw_chrome_rect(Rect::new(cursor_x, text_y, 1.5, cell_size.height), p.cursor_accent);
                    continue;
                }

                let y = tree_visual_rect.y + PANE_PADDING + i as f32 * line_height - file_tree_scroll;
                if y + line_height < tree_visual_rect.y || y > tree_visual_rect.y + tree_visual_rect.height {
                    continue;
                }

                let text_y = y + text_offset_y;
                let x = tree_visual_rect.x + left_padding + entry.depth as f32 * indent_width;

                // Expanded directory: draw row background (per Tide.pen)
                if entry.entry.is_dir && entry.is_expanded {
                    let row_rect = Rect::new(
                        tree_visual_rect.x + left_padding / 2.0,
                        y,
                        tree_visual_rect.width - left_padding,
                        line_height,
                    );
                    renderer.draw_chrome_rounded_rect(row_rect, p.tree_row_active, FILE_TREE_ROW_RADIUS);
                }

                // Look up git status for this entry (O(1) via pre-computed cache)
                let git_color = if entry.entry.is_dir {
                    app.file_tree_dir_git_status.get(&entry.entry.path).copied()
                } else {
                    app.file_tree_git_status.get(&entry.entry.path).copied()
                };

                let status_color = git_color.and_then(|gs| match gs {
                    tide_core::FileGitStatus::Modified => Some(p.git_modified),
                    tide_core::FileGitStatus::Added | tide_core::FileGitStatus::Untracked => Some(p.git_added),
                    tide_core::FileGitStatus::Conflict => Some(p.git_conflict),
                    tide_core::FileGitStatus::Deleted => None, // deleted files won't appear in tree
                });

                // Git status badge letter (right-aligned)
                let status_badge = git_color.and_then(|gs| match gs {
                    tide_core::FileGitStatus::Modified => Some("M"),
                    tide_core::FileGitStatus::Added | tide_core::FileGitStatus::Untracked => Some("U"),
                    tide_core::FileGitStatus::Conflict => Some("!"),
                    tide_core::FileGitStatus::Deleted => None,
                });

                // Icon — directories always keep standard icon color (per Tide.pen)
                let icon = file_icon(&entry.entry.name, entry.entry.is_dir, entry.is_expanded);
                let icon_color = if entry.entry.is_dir {
                    p.tree_dir_icon
                } else if let Some(sc) = status_color {
                    sc
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
                    tree_text_clip,
                );

                // Draw name after icon + space
                let name_x = x + cell_size.width * 2.0;
                let is_expanded_dir = entry.entry.is_dir && entry.is_expanded;
                let text_color = if let Some(sc) = status_color {
                    sc
                } else if is_expanded_dir {
                    p.tab_text_focused
                } else if entry.entry.is_dir {
                    p.tree_dir
                } else {
                    p.tree_text
                };
                let name_style = TextStyle {
                    foreground: text_color,
                    background: None,
                    bold: is_expanded_dir,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                renderer.draw_chrome_text(
                    &entry.entry.name,
                    Vec2::new(name_x, text_y),
                    name_style,
                    tree_text_clip,
                );

                // Draw git status badge ("M", "A", "?", "!") right-aligned
                if let Some(badge) = status_badge {
                    let badge_x = tree_visual_rect.x + tree_visual_rect.width - PANE_PADDING - cell_size.width;
                    let badge_style = TextStyle {
                        foreground: status_color.unwrap_or(p.tree_text),
                        background: None,
                        bold: true, dim: false, italic: false, underline: false,
                    };
                    renderer.draw_chrome_text(badge, Vec2::new(badge_x, text_y), badge_style, tree_text_clip);
                }
            }

            // File tree keyboard cursor highlight (when focus_area == FileTree)
            if app.focus_area == FocusArea::FileTree && app.file_tree_cursor < entries.len() {
                let cursor_y = tree_visual_rect.y + PANE_PADDING + app.file_tree_cursor as f32 * line_height - file_tree_scroll;
                if cursor_y + line_height > tree_visual_rect.y && cursor_y < tree_visual_rect.y + tree_visual_rect.height {
                    let row_rect = Rect::new(
                        tree_visual_rect.x + left_padding / 2.0,
                        cursor_y,
                        tree_visual_rect.width - left_padding,
                        line_height,
                    );
                    // Warm accent row highlight (more visible than hover)
                    let accent = p.dock_tab_underline;
                    let row_bg = tide_core::Color::new(accent.r, accent.g, accent.b, 0.12);
                    renderer.draw_chrome_rounded_rect(row_rect, row_bg, FILE_TREE_ROW_RADIUS);
                    // Left accent bar on cursor row
                    renderer.draw_chrome_rect(
                        Rect::new(tree_visual_rect.x + 2.0, cursor_y + 2.0, 2.0, line_height - 4.0),
                        accent,
                    );
                }
            }
        }
    }

    // Draw editor panel if visible (flat, border provided by clear color)
    if let Some(panel_rect) = editor_panel_rect {
        renderer.draw_chrome_rect(panel_rect, p.file_tree_bg);

        if !editor_panel_tabs.is_empty() {
            let cell_size = renderer.cell_size();
            let cell_height = cell_size.height;
            let cell_w = cell_size.width;
            let tab_bar_top = panel_rect.y;
            let tab_bar_clip = Rect::new(
                panel_rect.x,
                tab_bar_top,
                panel_rect.width,
                PANEL_TAB_HEIGHT,
            );

            // Tab bar background + bottom border (highlight when dock focused)
            let dock_focused = app.focus_area == FocusArea::EditorDock;
            if dock_focused {
                let accent = p.dock_tab_underline;
                let tab_bar_bg = tide_core::Color::new(accent.r, accent.g, accent.b, 0.06);
                renderer.draw_chrome_rect(
                    Rect::new(panel_rect.x, tab_bar_top, panel_rect.width, PANEL_TAB_HEIGHT),
                    tab_bar_bg,
                );
            }
            renderer.draw_chrome_rect(
                Rect::new(panel_rect.x, tab_bar_top + PANEL_TAB_HEIGHT - 1.0, panel_rect.width, 1.0),
                p.border_subtle,
            );

            // Variable-width tabs (per Tide.pen dock tab bar)
            let mut tx = panel_rect.x - app.panel_tab_scroll;
            for &tab_id in editor_panel_tabs.iter() {
                let title = panel_tab_title(&app.panes, tab_id);
                let tab_w = dock_tab_width(&title, cell_w);

                // Skip tabs outside visible area
                if tx + tab_w < panel_rect.x || tx > panel_rect.x + panel_rect.width {
                    tx += tab_w;
                    continue;
                }

                let is_active = editor_panel_active == Some(tab_id);
                let is_modified = app.panes.get(&tab_id)
                    .and_then(|pk| if let PaneKind::Editor(ep) = pk { Some(ep.editor.is_modified()) } else { None })
                    .unwrap_or(false);

                // Active tab: bg + underline
                if is_active {
                    renderer.draw_chrome_rect(
                        Rect::new(tx, tab_bar_top, tab_w, PANEL_TAB_HEIGHT),
                        p.pane_bg,
                    );
                    renderer.draw_chrome_rect(
                        Rect::new(tx, tab_bar_top + PANEL_TAB_HEIGHT - 2.0, tab_w, 2.0),
                        p.dock_tab_underline,
                    );
                }

                // Tab title
                let text_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
                let text_color = if is_active && is_modified {
                    p.editor_modified
                } else if is_active {
                    p.tab_text_focused
                } else {
                    p.tab_text
                };
                renderer.draw_chrome_text(
                    &title,
                    Vec2::new(tx + DOCK_TAB_PAD, text_y),
                    TextStyle {
                        foreground: text_color,
                        background: None,
                        bold: is_active,
                        dim: false, italic: false, underline: false,
                    },
                    tab_bar_clip,
                );

                // Close icon or modified dot
                let icon_x = tx + DOCK_TAB_PAD + title.chars().count() as f32 * cell_w + DOCK_TAB_GAP;
                let is_close_hovered = matches!(app.hover_target, Some(HoverTarget::PanelTabClose(hid)) if hid == tab_id);

                if is_modified && is_active && !is_close_hovered {
                    // Modified dot (6x6 circle, accent color)
                    let dot_y = text_y + (cell_height - DOCK_TAB_DOT_SIZE) / 2.0;
                    renderer.draw_chrome_rounded_rect(
                        Rect::new(icon_x, dot_y, DOCK_TAB_DOT_SIZE, DOCK_TAB_DOT_SIZE),
                        p.dock_tab_underline,
                        3.0,
                    );
                } else {
                    // Close icon
                    let icon_color = if is_close_hovered { p.tab_text_focused } else { p.close_icon };
                    renderer.draw_chrome_text(
                        "\u{f00d}",
                        Vec2::new(icon_x, text_y),
                        TextStyle {
                            foreground: icon_color,
                            background: None,
                            bold: false, dim: false, italic: false, underline: false,
                        },
                        tab_bar_clip,
                    );
                }

                tx += tab_w;
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
            let hint_text = "  Cmd+Shift+N";
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

        // Left edge border for editor panel
        {
            let r = panel_rect;
            let dock_focused = app.focus_area == FocusArea::EditorDock;
            if dock_focused {
                // Focused: warm accent edge (2px) + subtle inner glow (4px fade)
                let accent = p.dock_tab_underline;
                let glow = tide_core::Color::new(accent.r, accent.g, accent.b, 0.10);
                renderer.draw_chrome_rect(Rect::new(r.x, r.y, 2.0, r.height), accent);
                renderer.draw_chrome_rect(Rect::new(r.x + 2.0, r.y, 4.0, r.height), glow);
            } else {
                renderer.draw_chrome_rect(Rect::new(r.x, r.y, BORDER_WIDTH, r.height), p.border_subtle);
            }
        }
    }

    // Draw pane backgrounds + borders with rounded corners
    for &(id, rect) in visual_pane_rects {
        let is_focused = focused == Some(id);
        let border_color = if is_focused { p.border_focused } else { p.border_subtle };
        let top_border = if is_focused { 2.0 } else { 1.0 };
        let side_border = 1.0_f32;

        // Focused pane: draw outer glow shadow (per Tide.pen: blur=12, spread=-4, #C4B8A622)
        if is_focused {
            let shadow_color = tide_core::Color::new(0.769, 0.722, 0.651, 0.25);
            renderer.draw_chrome_shadow(rect, shadow_color, PANE_CORNER_RADIUS, 16.0, -4.0);
        }

        // Outer rounded rect (border color)
        renderer.draw_chrome_rounded_rect(rect, border_color, PANE_CORNER_RADIUS);
        // Inner rounded rect (pane fill, inset by border widths)
        let inset = Rect::new(
            rect.x + side_border,
            rect.y + top_border,
            rect.width - 2.0 * side_border,
            rect.height - top_border - side_border,
        );
        renderer.draw_chrome_rounded_rect(inset, p.pane_bg, (PANE_CORNER_RADIUS - side_border).max(0.0));
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

}

/// Render inline text tabs for stacked pane mode (per Tide.pen maximize mode header).
/// Tabs are variable-width inline text with underline for active tab.
/// Close button is on the far right of the header (not per-tab).
fn render_stacked_tab_bar(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    all_pane_ids: &[u64],
    stacked_active: u64,
    _focused: Option<u64>,
) {
    let Some(&(_, rect)) = visual_pane_rects.first() else {
        return;
    };
    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let cell_w = cell_size.width;
    let header_top = rect.y;
    let header_h = TAB_BAR_HEIGHT;
    let text_y = header_top + (header_h - cell_height) / 2.0;
    let header_clip = Rect::new(rect.x, header_top, rect.width, header_h);

    // Right side: close button
    let content_right = rect.x + rect.width - PANE_PADDING;
    let close_w = cell_w + BADGE_PADDING_H * 2.0;
    let close_x = content_right - close_w;
    {
        let close_style = TextStyle {
            foreground: p.close_icon,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        };
        renderer.draw_chrome_text(
            "\u{f00d}",
            Vec2::new(close_x + BADGE_PADDING_H, text_y),
            close_style,
            Rect::new(close_x, text_y - 1.0, close_w, cell_height + 2.0),
        );
    }

    // Left side: inline pane tabs (variable width)
    let mut tx = rect.x + PANE_PADDING;
    for &tab_id in all_pane_ids.iter() {
        let title = crate::ui::pane_title(&app.panes, tab_id);
        let tab_w = stacked_tab_width(&title, cell_w);

        if tx + tab_w > close_x - 12.0 {
            break; // don't overlap with right-side controls
        }

        let is_active = stacked_active == tab_id;

        // Active tab: underline
        if is_active {
            renderer.draw_chrome_rect(
                Rect::new(tx, header_top + header_h - 2.0, tab_w, 2.0),
                p.dock_tab_underline,
            );
        }

        let text_color = if is_active {
            p.tab_text_focused
        } else {
            p.tab_text
        };
        renderer.draw_chrome_text(
            &title,
            Vec2::new(tx + STACKED_TAB_PAD, text_y),
            TextStyle {
                foreground: text_color,
                background: None,
                bold: is_active,
                dim: false, italic: false, underline: false,
            },
            header_clip,
        );

        tx += tab_w;
    }
}
