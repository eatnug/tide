use unicode_width::UnicodeWidthChar;

use tide_core::{Rect, Renderer, TerminalBackend, TextStyle, Vec2};

use crate::drag_drop::{DropDestination, PaneDragState};
use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;


/// Render IME preedit overlay (Korean composition in progress) for terminal panes,
/// drag-drop preview overlays, and handle drag preview.
pub(crate) fn render_ime_and_drop_preview(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    focused: Option<u64>,
) {
    // Render IME preedit overlay for terminal and editor panes
    if !app.ime_preedit.is_empty() {
        // Determine effective target: dock editor when focus_area is EditorDock
        let effective_id = if app.focus_area == crate::ui_state::FocusArea::EditorDock {
            app.active_editor_tab().or(focused)
        } else {
            focused
        };
        if let Some(target_id) = effective_id {
            // Try editor pane first (both tree editors and panel editors)
            let is_editor = matches!(app.panes.get(&target_id), Some(PaneKind::Editor(_)));
            if is_editor {
                render_editor_ime_preedit(app, renderer, p, visual_pane_rects, target_id);
            } else if let Some((_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == target_id) {
                if let Some(PaneKind::Terminal(pane)) = app.panes.get(&target_id) {
                    let cursor = pane.backend.cursor();
                    let cell_size = renderer.cell_size();
                    let inner_w = rect.width - 2.0 * PANE_PADDING;
                    let max_cols = (inner_w / cell_size.width).floor() as usize;
                    let actual_w = max_cols as f32 * cell_size.width;
                    let center_x = (inner_w - actual_w) / 2.0;
                    let ime_top = app.pane_area_mode.content_top();
                    let inner_offset = Vec2::new(
                        rect.x + PANE_PADDING + center_x,
                        rect.y + ime_top,
                    );
                    let cx = inner_offset.x + cursor.col as f32 * cell_size.width;
                    let cy = inner_offset.y + cursor.row as f32 * cell_size.height;

                    // Draw preedit background
                    let preedit_chars: Vec<char> = app.ime_preedit.chars().collect();
                    let pw = preedit_chars.iter()
                        .map(|c| UnicodeWidthChar::width(*c).unwrap_or(1))
                        .sum::<usize>()
                        .max(1) as f32 * cell_size.width;
                    renderer.draw_rect(
                        Rect::new(cx, cy, pw, cell_size.height),
                        p.ime_preedit_bg,
                    );

                    // Draw each preedit character
                    let preedit_style = TextStyle {
                        foreground: p.ime_preedit_fg,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: true,
                    };
                    let mut col_offset = 0usize;
                    for &ch in preedit_chars.iter() {
                        renderer.draw_cell(
                            ch,
                            cursor.row as usize,
                            cursor.col as usize + col_offset,
                            preedit_style,
                            cell_size,
                            inner_offset,
                        );
                        col_offset += UnicodeWidthChar::width(ch).unwrap_or(1);
                    }
                }
            }
        }
    }

    // Draw drop preview overlay when dragging a pane (tree-to-tree only)
    if let PaneDragState::Dragging {
        source_pane,
        drop_target: Some(ref dest),
    } = &app.pane_drag {
        match dest {
            DropDestination::TreeRoot(zone) | DropDestination::TreePane(_, zone) => {
                let is_swap = *zone == tide_core::DropZone::Center;

                if is_swap {
                    // Swap preview: border-only outline around target's visual rect
                    if let DropDestination::TreePane(target_id, _) = dest {
                        if let Some(&(_, target_rect)) = visual_pane_rects.iter().find(|(id, _)| *id == *target_id) {
                            App::draw_swap_preview(renderer, target_rect, p);
                        }
                    }
                } else {
                    // Use simulate_drop for accurate preview
                    let target_id = match dest {
                        DropDestination::TreePane(tid, _) => Some(*tid),
                        _ => None,
                    };
                    if let Some(pane_area) = app.pane_area_rect {
                        let pane_area_size = tide_core::Size::new(pane_area.width, pane_area.height);
                        if let Some(preview_rect) = app.layout.simulate_drop(
                            *source_pane, target_id, *zone, true, pane_area_size,
                        ) {
                            // Offset from layout space to screen space
                            let screen_rect = Rect::new(
                                preview_rect.x + pane_area.x,
                                preview_rect.y + pane_area.y,
                                preview_rect.width,
                                preview_rect.height,
                            );
                            App::draw_insert_preview(renderer, screen_rect, p);
                        }
                    }
                }
            }
        }
    }

    // Draw handle drag drop preview
    // Sidebar is always outermost: sidebar at edge, dock inside.
    if let Some(target_side) = app.handle_drag_preview {
        let win_w = app.window_size.0 as f32 / app.scale_factor;
        let win_h = app.window_size.1 as f32 / app.scale_factor;
        let is_sidebar = app.sidebar_handle_dragging;
        let my_width = if is_sidebar { app.file_tree_width } else { app.editor_panel_width };
        let other_visible = if is_sidebar { app.show_editor_panel } else { app.show_file_tree };
        let other_side = if is_sidebar { app.dock_side } else { app.sidebar_side };
        let other_width = if is_sidebar { app.editor_panel_width } else { app.file_tree_width };

        let both_same = other_visible && target_side == other_side;
        // Sidebar is always outer; dock is always inner when on same side
        let i_am_inner = if is_sidebar { false } else { both_same };

        let preview_x = match target_side {
            crate::LayoutSide::Left => {
                if i_am_inner { other_width } else { 0.0 }
            }
            crate::LayoutSide::Right => {
                if i_am_inner {
                    win_w - other_width - my_width
                } else {
                    win_w - my_width
                }
            }
        };
        let preview_rect = Rect::new(preview_x, 0.0, my_width, win_h);
        App::draw_insert_preview(renderer, preview_rect, p);
    }
}

/// Render IME preedit overlay for an editor pane (tree editor or panel editor).
fn render_editor_ime_preedit(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    visual_pane_rects: &[(u64, Rect)],
    target_id: u64,
) {
    let pane = match app.panes.get(&target_id) {
        Some(PaneKind::Editor(pane)) => pane,
        _ => return,
    };
    let cell_size = renderer.cell_size();
    let pos = pane.editor.cursor_position();
    let scroll = pane.editor.scroll_offset();
    let h_scroll = pane.editor.h_scroll_offset();

    if pos.line < scroll {
        return;
    }
    let visual_row = pos.line - scroll;

    // Convert byte offset to char index
    let cursor_char_col = if let Some(line_text) = pane.editor.buffer.line(pos.line) {
        let byte_col = pos.col.min(line_text.len());
        line_text[..byte_col].chars().count()
    } else {
        0
    };
    if cursor_char_col < h_scroll {
        return;
    }
    let visual_col_offset = if let Some(line_text) = pane.editor.buffer.line(pos.line) {
        line_text.chars()
            .skip(h_scroll)
            .take(cursor_char_col - h_scroll)
            .map(|c| UnicodeWidthChar::width(c).unwrap_or(1))
            .sum::<usize>()
    } else {
        cursor_char_col - h_scroll
    };
    let gutter_cells = crate::editor_pane::GUTTER_WIDTH_CELLS;

    // Determine the rect for this editor (tree pane or panel editor)
    let (inner_x, inner_y) = if let Some((_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == target_id) {
        let top_offset = app.pane_area_mode.content_top();
        (rect.x + PANE_PADDING, rect.y + top_offset)
    } else if let Some(panel_rect) = app.editor_panel_rect {
        let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
        (panel_rect.x + PANE_PADDING, content_top)
    } else {
        return;
    };

    let gutter_width = gutter_cells as f32 * cell_size.width;
    let cx = inner_x + gutter_width + visual_col_offset as f32 * cell_size.width;
    let cy = inner_y + visual_row as f32 * cell_size.height;

    // Draw preedit background
    let preedit_chars: Vec<char> = app.ime_preedit.chars().collect();
    let pw = preedit_chars.iter()
        .map(|c| UnicodeWidthChar::width(*c).unwrap_or(1))
        .sum::<usize>()
        .max(1) as f32 * cell_size.width;
    renderer.draw_top_rect(
        Rect::new(cx, cy, pw, cell_size.height),
        p.ime_preedit_bg,
    );

    // Draw each preedit character in the top layer (above preedit bg)
    let mut col_offset = 0usize;
    for &ch in preedit_chars.iter() {
        let char_x = cx + col_offset as f32 * cell_size.width;
        renderer.draw_top_glyph(ch, Vec2::new(char_x, cy), p.ime_preedit_fg, false, false);
        col_offset += UnicodeWidthChar::width(ch).unwrap_or(1);
    }
}
