use tide_core::{FileTreeSource, Rect, Renderer, TextStyle, Vec2};

use crate::drag_drop::HoverTarget;
use crate::header;
use crate::theme::*;
use crate::ui::file_icon;
use crate::ui_state::FocusArea;
use crate::App;

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
    _all_pane_ids: &[u64],
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
        // Centered title: show "Tide" or "Tide · N" when multiple workspaces
        let cs = renderer.cell_size();
        {
            let title_text = if app.workspaces.len() > 1 {
                format!("Tide · {}", app.active_workspace + 1)
            } else {
                "Tide".to_string()
            };
            let title_w = title_text.chars().count() as f32 * cs.width;
            let title_x = (logical.width - title_w) / 2.0;
            let title_y = (app.top_inset - cs.height) / 2.0;
            renderer.draw_chrome_text(
                &title_text,
                Vec2::new(title_x, title_y),
                TextStyle {
                    foreground: p.tab_text,
                    background: None,
                    bold: false, dim: false, italic: false, underline: false,
                },
                tb,
            );
        }
        // Right: titlebar icons
        {
            let _icon_h = 16.0_f32;
            let rect_w = 7.0_f32;
            let gap = 3.0_f32;
            let icon_w = rect_w * 2.0 + gap;
            let icon_x = logical.width - PANE_PADDING - icon_w;

            // Settings gear icon
            {
                let gear_pad = 4.0_f32;
                let gear_icon = "\u{f013}"; // FontAwesome gear
                let gear_w = cs.width + gear_pad * 2.0;
                let gear_h = cs.height + 6.0;
                let gear_x = icon_x - gear_w - 8.0;
                let gear_y = (app.top_inset - gear_h) / 2.0;
                let gear_hovered = matches!(app.hover_target, Some(HoverTarget::TitlebarSettings));
                if gear_hovered {
                    let bg_rect = Rect::new(gear_x, gear_y, gear_w, gear_h);
                    renderer.draw_chrome_rounded_rect(bg_rect, p.badge_bg, 4.0);
                }
                let gear_text_y = gear_y + (gear_h - cs.height) / 2.0;
                let gear_color = if app.config_page.is_some() { p.dock_tab_underline } else { p.tab_text };
                renderer.draw_chrome_text(
                    gear_icon,
                    Vec2::new(gear_x + gear_pad, gear_text_y),
                    TextStyle {
                        foreground: gear_color,
                        background: None,
                        bold: false, dim: false, italic: false, underline: false,
                    },
                    tb,
                );
            }

            // Titlebar toggle buttons: [Sidebar] [PaneArea] [Dock] [gap] [Theme] [Settings] [Swap icon]
            // Positioned right-to-left from the settings icon
            let settings_pad = 4.0_f32;
            let settings_w = cs.width + settings_pad * 2.0;
            let settings_x = icon_x - settings_w - 8.0;

            // Theme toggle icon (between settings and toggle buttons)
            let theme_pad = 4.0_f32;
            let theme_w = cs.width + theme_pad * 2.0;
            let theme_h = cs.height + 6.0;
            let theme_x = settings_x - theme_w - 8.0;
            let theme_y = (app.top_inset - theme_h) / 2.0;
            let theme_hovered = matches!(app.hover_target, Some(HoverTarget::TitlebarTheme));
            if theme_hovered {
                let bg_rect = Rect::new(theme_x, theme_y, theme_w, theme_h);
                renderer.draw_chrome_rounded_rect(bg_rect, p.badge_bg, 4.0);
            }
            let theme_icon = if app.dark_mode { "\u{f186}" } else { "\u{f185}" }; // moon / sun
            let theme_text_y = theme_y + (theme_h - cs.height) / 2.0;
            renderer.draw_chrome_text(
                theme_icon,
                Vec2::new(theme_x + theme_pad, theme_text_y),
                TextStyle {
                    foreground: p.tab_text,
                    background: None,
                    bold: false, dim: false, italic: false, underline: false,
                },
                tb,
            );

            let btn_right = theme_x - TITLEBAR_BUTTON_GAP;
            let tb_clip = Rect::new(0.0, 0.0, logical.width, app.top_inset);

            // Helper: render a titlebar toggle button (icon + ⌘N hint, badge style)
            // Returns the total width consumed
            let render_titlebar_btn = |renderer: &mut tide_renderer::WgpuRenderer,
                                        icon_char: &str,
                                        hint: &str,
                                        hint_char_count: usize,
                                        right_edge: f32,
                                        is_active: bool,
                                        is_hovered: bool| -> f32 {
                let btn_pad_h = 6.0_f32;
                let icon_w_chars = 1;
                let gap_chars = 1; // space between icon and hint
                let total_chars = (icon_w_chars + gap_chars + hint_char_count) as f32;
                let btn_w = total_chars * cs.width + btn_pad_h * 2.0;
                let btn_h = cs.height + 6.0;
                let btn_x = right_edge - btn_w;
                let btn_y = (app.top_inset - btn_h) / 2.0;
                let btn_rect = Rect::new(btn_x, btn_y, btn_w, btn_h);

                // Background
                let bg_color = if is_hovered {
                    p.badge_bg
                } else if is_active {
                    p.badge_bg_unfocused
                } else {
                    tide_core::Color::new(0.0, 0.0, 0.0, 0.0)
                };
                if bg_color.a > 0.0 {
                    renderer.draw_chrome_rounded_rect(btn_rect, bg_color, 4.0);
                }

                // Icon
                let text_y = btn_y + (btn_h - cs.height) / 2.0;
                let icon_color = if is_active { p.dock_tab_underline } else { p.tab_text };
                renderer.draw_chrome_text(
                    icon_char,
                    Vec2::new(btn_x + btn_pad_h, text_y),
                    TextStyle {
                        foreground: icon_color,
                        background: None,
                        bold: false, dim: false, italic: false, underline: false,
                    },
                    tb_clip,
                );

                // Hint text
                let hint_x = btn_x + btn_pad_h + (icon_w_chars + gap_chars) as f32 * cs.width;
                let hint_color = if is_active { p.badge_text } else { p.badge_text_dimmed };
                renderer.draw_chrome_text(
                    hint,
                    Vec2::new(hint_x, text_y),
                    TextStyle {
                        foreground: hint_color,
                        background: None,
                        bold: false, dim: false, italic: false, underline: false,
                    },
                    tb_clip,
                );

                btn_w
            };

            // Render buttons: [Sidebar] [PaneArea] right-to-left
            let areas = app.area_ordering();
            let pane_icon = "\u{f009}"; // grid icon (split)
            let mut cur_right = btn_right;
            for (i, area) in areas.iter().enumerate().rev() {
                let slot = i + 1;
                let hint = format!("\u{2318}{}", slot);
                let (icon, is_active, hover_variant) = match area {
                    FocusArea::FileTree => ("\u{f07b}", app.show_file_tree, HoverTarget::TitlebarFileTree),
                    FocusArea::PaneArea => (pane_icon, app.focus_area == FocusArea::PaneArea, HoverTarget::TitlebarPaneArea),
                };
                let is_hovered = app.hover_target.as_ref() == Some(&hover_variant);
                let w = render_titlebar_btn(
                    renderer, icon, &hint, 2, cur_right, is_active, is_hovered,
                );
                cur_right -= w + TITLEBAR_BUTTON_GAP;
            }
        }
    }

    // Draw workspace sidebar if visible
    if let Some(ws_rect) = app.workspace_sidebar_rect {
        let cs = renderer.cell_size();
        let edge_inset = PANE_CORNER_RADIUS;

        // Sidebar visual rect: inset from top/bottom for corner radius visibility
        let sb_border = Rect::new(
            ws_rect.x,
            ws_rect.y + edge_inset,
            ws_rect.width,
            ws_rect.height - edge_inset * 2.0,
        );

        // Outer rounded rect (border)
        renderer.draw_chrome_rounded_rect(sb_border, p.border_subtle, PANE_CORNER_RADIUS);
        // Inner fill
        let inset = Rect::new(
            sb_border.x + 1.0,
            sb_border.y + 1.0,
            sb_border.width - 2.0,
            sb_border.height - 2.0,
        );
        renderer.draw_chrome_rounded_rect(inset, p.file_tree_bg, (PANE_CORNER_RADIUS - 1.0).max(0.0));

        // Workspace items
        let geo = app.ws_sidebar_geometry().unwrap();
        let content_x = geo.content_x;
        let content_w = geo.content_w;
        let item_gap = geo.item_gap;
        let name_h = cs.height;

        // Collect workspace info: for the active workspace, use live App data;
        // for others, read from the stored workspace vec.
        for i in 0..app.workspaces.len() {
            let is_active = i == app.active_workspace;
            let ws_name = app.workspaces[i].name.clone();

            let item_rect = geo.item_rect(i);

            // Active item: pane-bg background with 1px rounded border
            if is_active {
                // Outer rounded rect = border color
                renderer.draw_chrome_rounded_rect(item_rect, p.border_focused, PANE_CORNER_RADIUS);
                // Inner rounded rect = fill color (inset by 1px)
                let inner = Rect::new(
                    item_rect.x + 1.0,
                    item_rect.y + 1.0,
                    item_rect.width - 2.0,
                    item_rect.height - 2.0,
                );
                renderer.draw_chrome_rounded_rect(inner, p.pane_bg, (PANE_CORNER_RADIUS - 1.0).max(0.0));
            } else {
                // Hover highlight
                if matches!(app.hover_target, Some(HoverTarget::WorkspaceSidebarItem(idx)) if idx == i) {
                    renderer.draw_chrome_rounded_rect(item_rect, p.badge_bg, PANE_CORNER_RADIUS);
                }
            }

            // Name text
            let name_color = if is_active { p.tab_text_focused } else {
                tide_core::Color::new(0.627, 0.627, 0.647, 1.0) // #A0A0A5
            };
            renderer.draw_chrome_text(
                &ws_name,
                Vec2::new(content_x + WS_SIDEBAR_ITEM_PAD_H, item_rect.y + WS_SIDEBAR_ITEM_PAD_V),
                TextStyle {
                    foreground: name_color,
                    background: None,
                    bold: is_active,
                    dim: false, italic: false, underline: false,
                },
                inset,
            );

            // CWD text (second line)
            let cwd_text = if is_active {
                // Use live cwd from the focused terminal
                app.focused_terminal_cwd()
                    .map(|p| crate::ui_state::abbreviate_path(&p))
                    .unwrap_or_default()
            } else {
                String::new()
            };
            if !cwd_text.is_empty() {
                renderer.draw_chrome_text(
                    &cwd_text,
                    Vec2::new(content_x + WS_SIDEBAR_ITEM_PAD_H, item_rect.y + WS_SIDEBAR_ITEM_PAD_V + name_h + WS_SIDEBAR_LINE_GAP),
                    TextStyle {
                        foreground: p.tab_text,
                        background: None,
                        bold: false, dim: false, italic: false, underline: false,
                    },
                    inset,
                );
            }

            // Draw drag drop indicator line before this item (gap == i)
            if let Some((src, press_y, gap)) = app.ws_drag {
                let dragging = (app.last_cursor_pos.y - press_y).abs() > crate::theme::DRAG_THRESHOLD;
                if dragging && gap == i && gap != src && gap != src + 1 {
                    let line_y = item_rect.y - item_gap / 2.0;
                    let line_rect = Rect::new(content_x + 4.0, line_y - 1.0, content_w - 8.0, 2.0);
                    renderer.draw_chrome_rounded_rect(line_rect, p.border_focused, 1.0);
                }
            }
        }

        // Draw drop indicator after the last item (gap == len)
        if let Some((src, press_y, gap)) = app.ws_drag {
            let dragging = (app.last_cursor_pos.y - press_y).abs() > crate::theme::DRAG_THRESHOLD;
            let len = app.workspaces.len();
            if dragging && gap == len && gap != src + 1 {
                let last_bottom = geo.item_rect(len - 1);
                let line_y = last_bottom.y + last_bottom.height + item_gap / 2.0;
                let line_rect = Rect::new(content_x + 4.0, line_y - 1.0, content_w - 8.0, 2.0);
                renderer.draw_chrome_rounded_rect(line_rect, p.border_focused, 1.0);
            }
        }

        // "+ New Workspace" button at bottom
        let btn_h = cs.height + 12.0;
        let btn_y = ws_rect.y + ws_rect.height - edge_inset - btn_h - WS_SIDEBAR_PADDING;
        let btn_rect = Rect::new(content_x, btn_y, content_w, btn_h);

        if matches!(app.hover_target, Some(HoverTarget::WorkspaceSidebarNewBtn)) {
            renderer.draw_chrome_rounded_rect(btn_rect, p.badge_bg, PANE_CORNER_RADIUS);
        }

        let btn_text = "+ New Workspace";
        let btn_text_w = btn_text.len() as f32 * cs.width;
        let btn_text_x = content_x + (content_w - btn_text_w) / 2.0;
        let btn_text_y = btn_y + (btn_h - cs.height) / 2.0;
        renderer.draw_chrome_text(
            btn_text,
            Vec2::new(btn_text_x, btn_text_y),
            TextStyle {
                foreground: p.tab_text,
                background: None,
                bold: false, dim: false, italic: false, underline: false,
            },
            inset,
        );
    }

    // Draw file tree panel if visible (rounded border like panes)
    if show_file_tree {
        let tree_visual_rect = app.file_tree_rect.unwrap_or(Rect::new(
            0.0,
            app.top_inset,
            app.file_tree_width,
            logical.height - app.top_inset,
        ));

        let tree_focused = app.focus_area == FocusArea::FileTree;
        let border_color = if tree_focused { p.border_focused } else { p.border_subtle };
        let top_border = if tree_focused { 2.0 } else { 1.0 };
        let side_border = if tree_focused { 2.0_f32 } else { 1.0_f32 };
        let edge_inset = PANE_CORNER_RADIUS;

        let r_border = Rect::new(
            tree_visual_rect.x,
            tree_visual_rect.y + edge_inset,
            tree_visual_rect.width,
            tree_visual_rect.height - edge_inset * 2.0,
        );

        // Shadow when focused (matches pane style)
        if tree_focused {
            let shadow_color = tide_core::Color::new(0.769, 0.722, 0.651, 0.25);
            renderer.draw_chrome_shadow(r_border, shadow_color, PANE_CORNER_RADIUS, 16.0, -4.0);
        }

        // Outer rounded rect (border)
        renderer.draw_chrome_rounded_rect(r_border, border_color, PANE_CORNER_RADIUS);
        // Inner rounded rect (fill)
        let inset = Rect::new(
            r_border.x + side_border,
            r_border.y + top_border,
            r_border.width - 2.0 * side_border,
            r_border.height - top_border - side_border,
        );
        renderer.draw_chrome_rounded_rect(inset, p.file_tree_bg, (PANE_CORNER_RADIUS - side_border).max(0.0));

        // Shadow tree_visual_rect with inset version so content renders within the border
        let tree_visual_rect = Rect::new(
            tree_visual_rect.x,
            tree_visual_rect.y + edge_inset,
            tree_visual_rect.width,
            tree_visual_rect.height - edge_inset * 2.0,
        );

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

            // File tree header: root directory name
            {
                let header_y = tree_visual_rect.y;
                let header_h = FILE_TREE_HEADER_HEIGHT;
                let header_text_y = header_y + (header_h - cell_size.height) / 2.0;

                // Folder icon
                renderer.draw_chrome_text(
                    "\u{f07b}",
                    Vec2::new(tree_visual_rect.x + left_padding, header_text_y),
                    TextStyle {
                        foreground: p.tree_dir_icon,
                        background: None,
                        bold: false, dim: false, italic: false, underline: false,
                    },
                    tree_text_clip,
                );

                // Directory name (last path component)
                let root_name = tree.root()
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| tree.root().to_string_lossy().to_string());
                renderer.draw_chrome_text(
                    &root_name,
                    Vec2::new(tree_visual_rect.x + left_padding + cell_size.width * 2.0, header_text_y),
                    TextStyle {
                        foreground: p.tab_text_focused,
                        background: None,
                        bold: true, dim: false, italic: false, underline: false,
                    },
                    tree_text_clip,
                );

                // Bottom separator line (accent when focused)
                let sep_color = if tree_focused {
                    let accent = p.dock_tab_underline;
                    tide_core::Color::new(accent.r, accent.g, accent.b, 0.35)
                } else {
                    p.border_subtle
                };
                renderer.draw_chrome_rect(
                    Rect::new(tree_visual_rect.x + PANE_PADDING, header_y + header_h - 1.0, tree_visual_rect.width - PANE_PADDING * 2.0, 1.0),
                    sep_color,
                );
            }

            let entries = tree.visible_entries();
            let text_offset_y = (line_height - cell_size.height) / 2.0;
            for (i, entry) in entries.iter().enumerate() {
                // Skip entries that are being inline-renamed
                if app.file_tree_rename.as_ref().is_some_and(|r| r.entry_index == i) {
                    let y = tree_visual_rect.y + FILE_TREE_HEADER_HEIGHT + i as f32 * line_height - file_tree_scroll;
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

                let y = tree_visual_rect.y + FILE_TREE_HEADER_HEIGHT + i as f32 * line_height - file_tree_scroll;
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
                let cursor_y = tree_visual_rect.y + FILE_TREE_HEADER_HEIGHT + app.file_tree_cursor as f32 * line_height - file_tree_scroll;
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

    // Draw pane backgrounds + borders with rounded corners
    for &(id, rect) in visual_pane_rects {
        // Only show pane focus highlight when focus is in the pane area
        let is_focused = focused == Some(id) && app.focus_area == FocusArea::PaneArea;
        let border_color = if is_focused { p.border_focused } else { p.border_subtle };
        let top_border = if is_focused { 2.0 } else { 1.0 };
        let side_border = if is_focused { 2.0_f32 } else { 1.0_f32 };

        // Focused pane: draw outer glow shadow
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

    // Render per-pane headers (title + badges + close, or tab bar for multi-tab groups)
    let mut all_hit_zones = Vec::new();
    for &(id, rect) in visual_pane_rects {
        let tab_group = app.layout.tab_group_containing(id);
        let is_zoomed = app.zoomed_pane == Some(id);

        // Zoomed pane: tint the tab bar area so it's clearly distinct
        if is_zoomed {
            let tint = tide_core::Color::new(p.badge_git_branch.r, p.badge_git_branch.g, p.badge_git_branch.b, 0.18);
            let tab_bar_rect = Rect::new(
                rect.x + 2.0,
                rect.y + 2.0,
                rect.width - 4.0,
                TAB_BAR_HEIGHT - 2.0,
            );
            renderer.draw_chrome_rect(tab_bar_rect, tint);
        }

        let zones = header::render_pane_header(
            id, rect, &app.panes, focused, tab_group, is_zoomed, p, renderer,
        );
        all_hit_zones.extend(zones);
    }
    app.header_hit_zones = all_hit_zones;

    // Render browser navigation bar for browser panes
    for &(id, rect) in visual_pane_rects {
        if let Some(crate::pane::PaneKind::Browser(bp)) = app.panes.get(&id) {
            render_browser_nav_bar(bp, rect, app, renderer, p);
        }
    }
}

/// Render browser navigation bar (back/forward/refresh + URL bar) inside a browser pane.
fn render_browser_nav_bar(
    bp: &crate::browser_pane::BrowserPane,
    pane_rect: Rect,
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
) {
    use unicode_width::UnicodeWidthChar;

    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let cell_w = cell_size.width;
    let nav_h = (cell_height * 1.5).round();
    let nav_y = pane_rect.y + TAB_BAR_HEIGHT + 2.0;
    let nav_x = pane_rect.x + PANE_PADDING;
    let nav_w = pane_rect.width - PANE_PADDING * 2.0;

    // Nav bar background
    renderer.draw_chrome_rounded_rect(
        Rect::new(nav_x, nav_y, nav_w, nav_h),
        p.panel_tab_bg_active,
        4.0,
    );

    let text_y = nav_y + (nav_h - cell_height) / 2.0;
    let mut cx = nav_x + 8.0;

    // Back button
    let back_color = if bp.can_go_back { p.tab_text_focused } else { p.tab_text };
    renderer.draw_chrome_text(
        "\u{2190}",
        Vec2::new(cx, text_y),
        TextStyle { foreground: back_color, background: None, bold: false, dim: false, italic: false, underline: false },
        Rect::new(cx, nav_y, cell_w * 2.0, nav_h),
    );
    cx += cell_w * 2.0;

    // Forward button
    let fwd_color = if bp.can_go_forward { p.tab_text_focused } else { p.tab_text };
    renderer.draw_chrome_text(
        "\u{2192}",
        Vec2::new(cx, text_y),
        TextStyle { foreground: fwd_color, background: None, bold: false, dim: false, italic: false, underline: false },
        Rect::new(cx, nav_y, cell_w * 2.0, nav_h),
    );
    cx += cell_w * 2.0;

    // Refresh button
    let refresh_icon = if bp.loading { "\u{00d7}" } else { "\u{21bb}" };
    renderer.draw_chrome_text(
        refresh_icon,
        Vec2::new(cx, text_y),
        TextStyle { foreground: p.tab_text_focused, background: None, bold: false, dim: false, italic: false, underline: false },
        Rect::new(cx, nav_y, cell_w * 2.0, nav_h),
    );
    cx += cell_w * 2.0 + 4.0;

    // URL bar
    let url_w = nav_x + nav_w - cx - 8.0;
    if url_w > 40.0 {
        let url_rect = Rect::new(cx, nav_y + 2.0, url_w, nav_h - 4.0);
        let url_bg = if bp.url_input_focused { p.file_tree_bg } else { p.badge_bg };
        renderer.draw_chrome_rounded_rect(url_rect, url_bg, 3.0);

        let str_display_width = |s: &str| -> usize {
            s.chars().map(|c| UnicodeWidthChar::width(c).unwrap_or(1)).sum()
        };

        let max_cols = (url_w / cell_w).floor() as usize;
        if bp.url_input_focused {
            let preedit = &app.ime_preedit;
            let before: String = bp.url_input.chars().take(bp.url_input_cursor).collect();
            let after: String = bp.url_input.chars().skip(bp.url_input_cursor).collect();
            let display = format!("{}{}{}", before, preedit, after);

            let mut truncated = String::new();
            let mut cols = 0;
            for ch in display.chars() {
                let w = UnicodeWidthChar::width(ch).unwrap_or(1);
                if cols + w > max_cols.saturating_sub(1) { break; }
                truncated.push(ch);
                cols += w;
            }

            renderer.draw_chrome_text(
                &truncated,
                Vec2::new(cx + 4.0, text_y),
                TextStyle { foreground: p.tab_text_focused, background: None, bold: false, dim: false, italic: false, underline: false },
                url_rect,
            );

            if !preedit.is_empty() {
                let before_cols = str_display_width(&before) as f32;
                let preedit_cols = str_display_width(preedit) as f32;
                let underline_x = cx + 4.0 + before_cols * cell_w;
                let underline_w = preedit_cols * cell_w;
                renderer.draw_chrome_rect(
                    Rect::new(underline_x, nav_y + nav_h - 4.0, underline_w, 1.0),
                    p.cursor_accent,
                );
            }

            let cursor_cols = str_display_width(&before) + str_display_width(preedit);
            let cursor_x = cx + 4.0 + cursor_cols as f32 * cell_w;
            renderer.draw_chrome_rect(
                Rect::new(cursor_x, nav_y + 4.0, 2.0, nav_h - 8.0),
                p.cursor_accent,
            );
        } else {
            let mut truncated = String::new();
            let mut cols = 0;
            for ch in bp.url.chars() {
                let w = UnicodeWidthChar::width(ch).unwrap_or(1);
                if cols + w > max_cols.saturating_sub(1) { break; }
                truncated.push(ch);
                cols += w;
            }
            renderer.draw_chrome_text(
                &truncated,
                Vec2::new(cx + 4.0, text_y),
                TextStyle { foreground: p.tab_text_focused, background: None, bold: false, dim: false, italic: false, underline: false },
                url_rect,
            );
        }
    }
}
