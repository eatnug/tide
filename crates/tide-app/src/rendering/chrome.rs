use tide_core::{FileTreeSource, Rect, Renderer, TextStyle, Vec2};

use crate::drag_drop::HoverTarget;
use crate::header;
use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui::{file_icon, panel_tab_title, stacked_tab_width};
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
            let icon_h = 16.0_f32;
            let rect_w = 7.0_f32;
            let gap = 3.0_f32;
            let icon_w = rect_w * 2.0 + gap;
            let icon_x = logical.width - PANE_PADDING - icon_w;
            let icon_y = (app.top_inset - icon_h) / 2.0;
            // Hover background for swap icon
            let swap_hovered = matches!(app.hover_target, Some(HoverTarget::TitlebarSwap));
            if swap_hovered {
                let bg_pad = 4.0_f32;
                let bg_rect = Rect::new(icon_x - bg_pad, icon_y - bg_pad, icon_w + bg_pad * 2.0, icon_h + bg_pad * 2.0);
                renderer.draw_chrome_rounded_rect(bg_rect, p.badge_bg, 4.0);
            }
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

            // Titlebar toggle buttons: [Sidebar] [PaneArea] [Dock] [gap] [Swap icon]
            // Positioned right-to-left from the swap icon
            // Hints are dynamic based on current layout slot assignment
            let btn_right = icon_x - 4.0 - TITLEBAR_BUTTON_GAP;
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

            // Render buttons based on area_ordering: icons swap with layout, numbers stay fixed
            // Buttons are rendered right-to-left: slot 3 (rightmost), slot 2 (middle), slot 1 (leftmost)
            let areas = app.area_ordering();
            let pane_icon = if matches!(app.pane_area_mode, PaneAreaMode::Stacked(_)) {
                "\u{f24d}" // clone/stack icon
            } else {
                "\u{f009}" // grid icon (split)
            };
            let mut cur_right = btn_right;
            for (i, area) in areas.iter().enumerate().rev() {
                let slot = i + 1;
                let hint = format!("\u{2318}{}", slot);
                let (icon, is_active, hover_variant) = match area {
                    FocusArea::FileTree => ("\u{f07b}", app.show_file_tree, HoverTarget::TitlebarFileTree),
                    FocusArea::PaneArea => (pane_icon, app.focus_area == FocusArea::PaneArea, HoverTarget::TitlebarPaneArea),
                    FocusArea::EditorDock => ("\u{f15c}", app.show_editor_panel, HoverTarget::TitlebarDock),
                };
                let is_hovered = app.hover_target.as_ref() == Some(&hover_variant);
                let w = render_titlebar_btn(
                    renderer, icon, &hint, 2, cur_right, is_active, is_hovered,
                );
                cur_right -= w + TITLEBAR_BUTTON_GAP;
            }
        }
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
        let side_border = 1.0_f32;
        let edge_inset = PANE_CORNER_RADIUS;

        // Detect which window edge the file tree touches; extend border past it
        let ft_at_left = tree_visual_rect.x < 1.0;
        let ft_at_right = (tree_visual_rect.x + tree_visual_rect.width - logical.width).abs() < 1.0;
        let r_border = if ft_at_left {
            Rect::new(
                tree_visual_rect.x - PANE_CORNER_RADIUS,
                tree_visual_rect.y + edge_inset,
                tree_visual_rect.width + PANE_CORNER_RADIUS,
                tree_visual_rect.height - edge_inset * 2.0,
            )
        } else if ft_at_right {
            Rect::new(
                tree_visual_rect.x,
                tree_visual_rect.y + edge_inset,
                tree_visual_rect.width + PANE_CORNER_RADIUS,
                tree_visual_rect.height - edge_inset * 2.0,
            )
        } else {
            // Not at window edge (shouldn't happen for file tree, but handle gracefully)
            Rect::new(
                tree_visual_rect.x,
                tree_visual_rect.y + edge_inset,
                tree_visual_rect.width,
                tree_visual_rect.height - edge_inset * 2.0,
            )
        };

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

    // Draw editor panel if visible (rounded border like panes)
    if let Some(panel_rect) = editor_panel_rect {
        let dock_focused = app.focus_area == FocusArea::EditorDock;
        let border_color = if dock_focused { p.border_focused } else { p.border_subtle };
        let top_border = if dock_focused { 2.0 } else { 1.0 };
        let side_border = 1.0_f32;

        // Inset top/bottom to align with pane visual rects; extend border past window edge
        let edge_inset = PANE_CORNER_RADIUS;
        let dock_at_right = (panel_rect.x + panel_rect.width - logical.width).abs() < 1.0;
        let dock_at_left = panel_rect.x < 1.0;
        let r_border = if dock_at_right {
            Rect::new(
                panel_rect.x,
                panel_rect.y + edge_inset,
                panel_rect.width + PANE_CORNER_RADIUS,
                panel_rect.height - edge_inset * 2.0,
            )
        } else if dock_at_left {
            Rect::new(
                panel_rect.x - PANE_CORNER_RADIUS,
                panel_rect.y + edge_inset,
                panel_rect.width + PANE_CORNER_RADIUS,
                panel_rect.height - edge_inset * 2.0,
            )
        } else {
            // Dock not at window edge (inner panel when both on same side)
            Rect::new(
                panel_rect.x,
                panel_rect.y + edge_inset,
                panel_rect.width,
                panel_rect.height - edge_inset * 2.0,
            )
        };

        // Shadow when focused (matches pane style)
        if dock_focused {
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

        // Shadow panel_rect with inset version so content renders within the border
        let panel_rect = Rect::new(
            panel_rect.x,
            panel_rect.y + edge_inset,
            panel_rect.width,
            panel_rect.height - edge_inset * 2.0,
        );

        if !editor_panel_tabs.is_empty() {
            let cell_size = renderer.cell_size();
            let cell_height = cell_size.height;
            let cell_w = cell_size.width;
            let tab_bar_top = panel_rect.y;
            let tab_bar_clip = Rect::new(
                panel_rect.x + PANE_PADDING,
                tab_bar_top,
                panel_rect.width - PANE_PADDING * 2.0,
                PANEL_TAB_HEIGHT,
            );

            // Bottom separator (inset from edges to avoid rounded corners)
            let dock_focused = app.focus_area == FocusArea::EditorDock;
            let tab_sep_color = if dock_focused {
                let accent = p.dock_tab_underline;
                tide_core::Color::new(accent.r, accent.g, accent.b, 0.35)
            } else {
                p.border_subtle
            };
            renderer.draw_chrome_rect(
                Rect::new(panel_rect.x + PANE_PADDING, tab_bar_top + PANEL_TAB_HEIGHT - 1.0, panel_rect.width - PANE_PADDING * 2.0, 1.0),
                tab_sep_color,
            );

            // Right side controls: [maximize] [close] (matching stacked mode layout)
            let text_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
            let content_right = panel_rect.x + panel_rect.width - PANE_PADDING;

            // Close button (far right)
            let close_w = cell_w + BADGE_PADDING_H * 2.0;
            let close_x = content_right - close_w;
            let is_close_hovered = matches!(app.hover_target, Some(HoverTarget::PanelTabClose(_)));
            {
                let close_style = TextStyle {
                    foreground: if is_close_hovered { p.tab_text_focused } else { p.close_icon },
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

            // Maximize button (left of close)
            let badge_gap = 6.0_f32;
            let max_icon = if app.editor_panel_maximized { "\u{f066}" } else { "\u{f065}" };
            let max_w = cell_w + BADGE_PADDING_H * 2.0;
            let max_x = close_x - badge_gap - max_w;
            let max_hovered = matches!(app.hover_target, Some(HoverTarget::DockMaximize));
            if max_hovered {
                renderer.draw_chrome_rounded_rect(
                    Rect::new(max_x, text_y - 1.0, max_w, cell_height + 2.0),
                    p.badge_bg,
                    3.0,
                );
            }
            renderer.draw_chrome_text(
                max_icon,
                Vec2::new(max_x + BADGE_PADDING_H, text_y),
                TextStyle {
                    foreground: if max_hovered { p.tab_text_focused } else { p.close_icon },
                    background: None,
                    bold: false, dim: false, italic: false, underline: false,
                },
                tab_bar_clip,
            );

            // Markdown preview toggle badge (left of maximize button)
            let mut tabs_stop = max_x - 12.0;
            if let Some(active_id) = editor_panel_active {
                if let Some(PaneKind::Editor(ep)) = app.panes.get(&active_id) {
                    if ep.is_markdown() && !ep.diff_mode {
                        let preview_text = if ep.preview_mode { "edit" } else { "preview" };
                        let badge_w = preview_text.len() as f32 * cell_w + BADGE_PADDING_H * 2.0;
                        let badge_x = tabs_stop - badge_w;
                        let dock_focused = app.focus_area == FocusArea::EditorDock;
                        let badge_color = if dock_focused { p.badge_text } else { p.tab_text };
                        let badge_bg = if dock_focused { p.badge_bg } else { p.badge_bg_unfocused };
                        crate::header::render_dock_preview_badge(
                            renderer, badge_x, text_y, badge_w, cell_height,
                            preview_text, badge_color, badge_bg,
                        );
                        tabs_stop = badge_x - BADGE_GAP;
                    }
                }
            }

            // Variable-width tabs (matching stacked mode style)
            let mut tx = panel_rect.x + PANE_PADDING - app.panel_tab_scroll;
            for &tab_id in editor_panel_tabs.iter() {
                let title = panel_tab_title(&app.panes, tab_id);
                let tab_w = stacked_tab_width(&title, cell_w);

                // Don't overlap with maximize control
                if tx + tab_w > tabs_stop {
                    break;
                }

                // Skip tabs outside visible area
                if tx + tab_w < panel_rect.x || tx > panel_rect.x + panel_rect.width {
                    tx += tab_w;
                    continue;
                }

                let is_active = editor_panel_active == Some(tab_id);
                let is_modified = app.panes.get(&tab_id)
                    .and_then(|pk| if let PaneKind::Editor(ep) = pk { Some(ep.editor.is_modified()) } else { None })
                    .unwrap_or(false);

                // Active tab: underline only (matching stacked mode)
                if is_active {
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
                    Vec2::new(tx + STACKED_TAB_PAD, text_y),
                    TextStyle {
                        foreground: text_color,
                        background: None,
                        bold: is_active,
                        dim: false, italic: false, underline: false,
                    },
                    tab_bar_clip,
                );

                tx += tab_w;
            }

            // Browser navigation bar: render when active tab is a Browser pane
            if let Some(active_id) = editor_panel_active {
                if let Some(PaneKind::Browser(bp)) = app.panes.get(&active_id) {
                    let cell_size = renderer.cell_size();
                    let cell_height = cell_size.height;
                    let cell_w = cell_size.width;
                    let nav_h = (cell_height * 1.5).round();
                    let nav_y = panel_rect.y + PANEL_TAB_HEIGHT + 2.0;
                    let nav_x = panel_rect.x + PANE_PADDING;
                    let nav_w = panel_rect.width - PANE_PADDING * 2.0;

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
                    let back_text = "\u{2190}"; // ←
                    renderer.draw_chrome_text(
                        back_text,
                        Vec2::new(cx, text_y),
                        TextStyle { foreground: back_color, background: None, bold: false, dim: false, italic: false, underline: false },
                        Rect::new(cx, nav_y, cell_w * 2.0, nav_h),
                    );
                    cx += cell_w * 2.0;

                    // Forward button
                    let fwd_color = if bp.can_go_forward { p.tab_text_focused } else { p.tab_text };
                    let fwd_text = "\u{2192}"; // →
                    renderer.draw_chrome_text(
                        fwd_text,
                        Vec2::new(cx, text_y),
                        TextStyle { foreground: fwd_color, background: None, bold: false, dim: false, italic: false, underline: false },
                        Rect::new(cx, nav_y, cell_w * 2.0, nav_h),
                    );
                    cx += cell_w * 2.0;

                    // Refresh button
                    let refresh_icon = if bp.loading { "\u{00d7}" } else { "\u{21bb}" }; // × or ↻
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

                        // URL text
                        let url_display = if bp.url_input_focused {
                            &bp.url_input
                        } else {
                            &bp.url
                        };
                        let max_chars = (url_w / cell_w).floor() as usize;
                        let truncated: String = url_display.chars().take(max_chars.saturating_sub(1)).collect();
                        renderer.draw_chrome_text(
                            &truncated,
                            Vec2::new(cx + 4.0, text_y),
                            TextStyle { foreground: p.tab_text_focused, background: None, bold: false, dim: false, italic: false, underline: false },
                            url_rect,
                        );

                        // URL input cursor
                        if bp.url_input_focused {
                            let cursor_x = cx + 4.0 + bp.url_input_cursor.min(max_chars) as f32 * cell_w;
                            renderer.draw_chrome_rect(
                                Rect::new(cursor_x, nav_y + 4.0, 2.0, nav_h - 8.0),
                                p.cursor_accent,
                            );
                        }
                    }
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

        // (Border already drawn above with rounded rect)
    }

    // Draw pane backgrounds + borders with rounded corners
    for &(id, rect) in visual_pane_rects {
        // Only show pane focus highlight when focus is in the pane area
        let is_focused = focused == Some(id) && app.focus_area == FocusArea::PaneArea;
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
        let zones = render_stacked_tab_bar(
            app, renderer, p,
            visual_pane_rects, all_pane_ids,
            stacked_active, focused,
        );
        all_hit_zones.extend(zones);
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
) -> Vec<header::HeaderHitZone> {
    let mut zones = Vec::new();
    let Some(&(_, rect)) = visual_pane_rects.first() else {
        return zones;
    };
    let cell_size = renderer.cell_size();
    let cell_height = cell_size.height;
    let cell_w = cell_size.width;
    let header_top = rect.y;
    let header_h = TAB_BAR_HEIGHT;
    let text_y = header_top + (header_h - cell_height) / 2.0;
    let header_clip = Rect::new(rect.x, header_top, rect.width, header_h);

    // Right side: [mode toggle] [maximize] [close]
    let content_right = rect.x + rect.width - PANE_PADDING;
    let close_w = cell_w + BADGE_PADDING_H * 2.0;
    let close_x = content_right - close_w;
    let badge_gap = 6.0_f32;
    let badge_pad = 6.0_f32;
    let badge_h = cell_height + 4.0;

    // Maximize button (expand/compress icon)
    let max_icon = if app.pane_area_maximized { "\u{f066}" } else { "\u{f065}" };
    let max_badge_w = cell_w + badge_pad * 2.0;
    let max_badge_x = close_x - badge_gap - max_badge_w;
    let max_badge_y = header_top + (header_h - badge_h) / 2.0;
    let max_badge_rect = Rect::new(max_badge_x, max_badge_y, max_badge_w, badge_h);
    let max_hovered = matches!(app.hover_target, Some(HoverTarget::PaneAreaMaximize));
    let max_bg = if max_hovered { p.badge_bg } else { p.badge_bg_unfocused };
    renderer.draw_chrome_rounded_rect(max_badge_rect, max_bg, 4.0);
    let max_text_y = max_badge_y + (badge_h - cell_height) / 2.0;
    renderer.draw_chrome_text(
        max_icon,
        Vec2::new(max_badge_x + badge_pad, max_text_y),
        TextStyle {
            foreground: p.badge_text,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        },
        header_clip,
    );

    // Mode toggle badge: grid icon + ⌘N hint (to switch to Split)
    let mode_icon = "\u{f009}"; // grid icon
    let pane_slot = app.slot_number_for_area(FocusArea::PaneArea);
    let mode_hint = format!("\u{2318}{}", pane_slot);
    let mode_hint_len = 2;
    let mode_badge_chars = (1 + 1 + mode_hint_len) as f32;
    let mode_badge_w = mode_badge_chars * cell_w + badge_pad * 2.0;
    let mode_badge_x = max_badge_x - badge_gap - mode_badge_w;
    let mode_badge_y = header_top + (header_h - badge_h) / 2.0;
    let mode_badge_rect = Rect::new(mode_badge_x, mode_badge_y, mode_badge_w, badge_h);

    let mode_hovered = matches!(app.hover_target, Some(HoverTarget::PaneModeToggle));
    let mode_bg = if mode_hovered { p.badge_bg } else { p.badge_bg_unfocused };
    renderer.draw_chrome_rounded_rect(mode_badge_rect, mode_bg, 4.0);

    let mode_text_y = mode_badge_y + (badge_h - cell_height) / 2.0;
    renderer.draw_chrome_text(
        mode_icon,
        Vec2::new(mode_badge_x + badge_pad, mode_text_y),
        TextStyle {
            foreground: p.badge_text,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        },
        header_clip,
    );
    renderer.draw_chrome_text(
        &mode_hint,
        Vec2::new(mode_badge_x + badge_pad + 2.0 * cell_w, mode_text_y),
        TextStyle {
            foreground: p.badge_text_dimmed,
            background: None,
            bold: false, dim: false, italic: false, underline: false,
        },
        header_clip,
    );

    // Close button
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

    // Git badges for the active terminal pane (between tabs and controls)
    let mut tabs_stop = mode_badge_x - badge_gap;
    if let Some(PaneKind::Terminal(pane)) = app.panes.get(&stacked_active) {
        if let Some(ref git) = pane.git_info {
            // Git status badge (green tinted, only for active pane)
            if git.status.changed_files > 0 {
                let stat_text = format!(
                    "{} +{} -{}",
                    git.status.changed_files, git.status.additions, git.status.deletions
                );
                let stat_bg = tide_core::Color::new(p.git_added.r, p.git_added.g, p.git_added.b, 0.094);
                let stat_w = stat_text.len() as f32 * cell_w + BADGE_PADDING_H * 2.0;
                let stat_x = tabs_stop - stat_w;
                if stat_x > rect.x + PANE_PADDING + 60.0 {
                    header::render_badge_colored(renderer, stat_x, text_y, stat_w, cell_height, &stat_text, p.git_added, stat_bg, BADGE_RADIUS);
                    zones.push(header::HeaderHitZone {
                        pane_id: stacked_active,
                        rect: Rect::new(stat_x, rect.y, stat_w, TAB_BAR_HEIGHT),
                        action: header::HeaderHitAction::GitStatus,
                    });
                    tabs_stop = stat_x - badge_gap;
                }
            }

            // Git branch badge
            let branch_display = format!("\u{e0a0} {}", git.branch);
            let branch_color = p.badge_git_branch;
            let branch_badge_bg = p.badge_bg;
            let branch_w = branch_display.chars().count() as f32 * cell_w + BADGE_PADDING_H * 2.0;
            let branch_x = tabs_stop - branch_w;
            if branch_x > rect.x + PANE_PADDING + 60.0 {
                header::render_badge_colored(renderer, branch_x, text_y, branch_w, cell_height, &branch_display, branch_color, branch_badge_bg, BADGE_RADIUS);
                zones.push(header::HeaderHitZone {
                    pane_id: stacked_active,
                    rect: Rect::new(branch_x, rect.y, branch_w, TAB_BAR_HEIGHT),
                    action: header::HeaderHitAction::GitBranch,
                });
                tabs_stop = branch_x - badge_gap;
            }
        }
    }

    // Left side: inline pane tabs (variable width)
    let mut tx = rect.x + PANE_PADDING;
    for &tab_id in all_pane_ids.iter() {
        let title = crate::ui::pane_title(&app.panes, tab_id);
        let tab_w = stacked_tab_width(&title, cell_w);

        if tx + tab_w > tabs_stop - 12.0 {
            break; // don't overlap with badges/controls
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
    zones
}
