use tide_core::{Rect, Renderer};

use crate::drag_drop;
use crate::drag_drop::PaneDragState;
use crate::theme::*;
use crate::{App, PaneAreaMode};


/// Render hover highlights (overlay layer) for the currently hovered UI element.
pub(crate) fn render_hover(
    app: &App,
    renderer: &mut tide_renderer::WgpuRenderer,
    p: &ThemePalette,
    logical: tide_core::Size,
    visual_pane_rects: &[(u64, Rect)],
    show_file_tree: bool,
    file_tree_scroll: f32,
    editor_panel_rect: Option<Rect>,
    editor_panel_tabs: &[u64],
    editor_panel_active: Option<u64>,
    empty_panel_btn_rects: Option<(Rect, Rect)>,
) {
    if let Some(ref hover) = app.hover_target {
        // Skip hover rendering during drag
        if matches!(app.pane_drag, PaneDragState::Idle) && !app.panel_border_dragging && !app.file_tree_border_dragging {
            match hover {
                drag_drop::HoverTarget::FileTreeEntry(index) => {
                    if show_file_tree {
                        if let Some(ft_rect) = app.file_tree_rect {
                            let cell_size = renderer.cell_size();
                            let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
                            let y = PANE_PADDING + *index as f32 * line_height - file_tree_scroll;
                            if y + line_height > 0.0 && y < logical.height {
                                let row_rect = Rect::new(ft_rect.x, y, ft_rect.width, line_height);
                                renderer.draw_rect(row_rect, p.hover_file_tree);
                            }
                        }
                    }
                }
                drag_drop::HoverTarget::StackedTab(tab_id) => {
                    // Highlight stacked tab (only inactive tabs, active already has background)
                    if let PaneAreaMode::Stacked(active) = app.pane_area_mode {
                        if active != *tab_id {
                            if let Some(geo) = app.stacked_tab_bar_geometry() {
                                let pane_ids = app.layout.pane_ids();
                                if let Some(idx) = pane_ids.iter().position(|&id| id == *tab_id) {
                                    renderer.draw_rect(geo.tab_rect(idx), p.hover_tab);
                                }
                            }
                        }
                    }
                }
                drag_drop::HoverTarget::StackedTabClose(tab_id) => {
                    if let Some(geo) = app.stacked_tab_bar_geometry() {
                        let pane_ids = app.layout.pane_ids();
                        if let Some(idx) = pane_ids.iter().position(|&id| id == *tab_id) {
                            renderer.draw_rect(geo.close_rect(idx), p.hover_close);
                        }
                    }
                }
                drag_drop::HoverTarget::PaneTabBar(pane_id) => {
                    if let Some(&(_, rect)) = visual_pane_rects.iter().find(|(id, _)| id == pane_id) {
                        let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
                        renderer.draw_rect(tab_rect, p.hover_tab);
                    }
                }
                drag_drop::HoverTarget::PaneTabClose(pane_id) => {
                    if let Some(&(_, rect)) = visual_pane_rects.iter().find(|(id, _)| id == pane_id) {
                        let close_x = rect.x + rect.width - PANE_CLOSE_SIZE - PANE_PADDING;
                        let close_y = rect.y + (TAB_BAR_HEIGHT - PANE_CLOSE_SIZE) / 2.0;
                        let close_rect = Rect::new(close_x, close_y, PANE_CLOSE_SIZE, PANE_CLOSE_SIZE);
                        renderer.draw_rect(close_rect, p.hover_close);
                    }
                }
                drag_drop::HoverTarget::PanelTab(tab_id) => {
                    // Only highlight inactive tabs (active tab already has background)
                    if editor_panel_active != Some(*tab_id) {
                        if let Some(panel_rect) = editor_panel_rect {
                            let tab_bar_top = panel_rect.y + PANE_PADDING;
                            let tab_start_x = panel_rect.x + PANE_PADDING - app.panel_tab_scroll;
                            if let Some(idx) = editor_panel_tabs.iter().position(|&id| id == *tab_id) {
                                let tx = tab_start_x + idx as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
                                let tab_rect = Rect::new(tx, tab_bar_top, PANEL_TAB_WIDTH, PANEL_TAB_HEIGHT);
                                renderer.draw_rect(tab_rect, p.hover_tab);
                            }
                        }
                    }
                }
                drag_drop::HoverTarget::PanelTabClose(tab_id) => {
                    if let Some(panel_rect) = editor_panel_rect {
                        let tab_bar_top = panel_rect.y + PANE_PADDING;
                        let tab_start_x = panel_rect.x + PANE_PADDING - app.panel_tab_scroll;
                        if let Some(idx) = editor_panel_tabs.iter().position(|&id| id == *tab_id) {
                            let tx = tab_start_x + idx as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
                            let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - PANEL_TAB_CLOSE_PADDING;
                            let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - PANEL_TAB_CLOSE_SIZE) / 2.0;
                            let close_rect = Rect::new(close_x, close_y, PANEL_TAB_CLOSE_SIZE, PANEL_TAB_CLOSE_SIZE);
                            renderer.draw_rect(close_rect, p.hover_close);
                        }
                    }
                }
                drag_drop::HoverTarget::SplitBorder(dir) => {
                    // Highlight the border line between adjacent panes
                    for &(id_a, rect_a) in visual_pane_rects {
                        match dir {
                            tide_core::SplitDirection::Horizontal => {
                                let right_edge = rect_a.x + rect_a.width;
                                for &(id_b, rect_b) in visual_pane_rects {
                                    if id_b != id_a && (rect_b.x - right_edge).abs() <= PANE_GAP + 1.0 {
                                        let y = rect_a.y.max(rect_b.y);
                                        let h = (rect_a.y + rect_a.height).min(rect_b.y + rect_b.height) - y;
                                        if h > 0.0 {
                                            let border_rect = Rect::new(right_edge - 1.0, y, rect_b.x - right_edge + 2.0, h);
                                            renderer.draw_rect(border_rect, p.hover_panel_border);
                                        }
                                    }
                                }
                            }
                            tide_core::SplitDirection::Vertical => {
                                let bottom_edge = rect_a.y + rect_a.height;
                                for &(id_b, rect_b) in visual_pane_rects {
                                    if id_b != id_a && (rect_b.y - bottom_edge).abs() <= PANE_GAP + 1.0 {
                                        let x = rect_a.x.max(rect_b.x);
                                        let w = (rect_a.x + rect_a.width).min(rect_b.x + rect_b.width) - x;
                                        if w > 0.0 {
                                            let border_rect = Rect::new(x, bottom_edge - 1.0, w, rect_b.y - bottom_edge + 2.0);
                                            renderer.draw_rect(border_rect, p.hover_panel_border);
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                drag_drop::HoverTarget::FileTreeBorder => {
                    if let Some(ft_rect) = app.file_tree_rect {
                        let border_x = if app.sidebar_side == crate::LayoutSide::Left {
                            ft_rect.x + ft_rect.width
                        } else {
                            ft_rect.x - PANE_GAP
                        };
                        let border_rect = Rect::new(border_x, 0.0, 4.0, logical.height);
                        renderer.draw_rect(border_rect, p.hover_panel_border);
                    }
                }
                drag_drop::HoverTarget::PanelBorder => {
                    if let Some(panel_rect) = editor_panel_rect {
                        let border_x = if app.dock_side == crate::LayoutSide::Right {
                            panel_rect.x - PANE_GAP
                        } else {
                            panel_rect.x + panel_rect.width
                        };
                        let border_rect = Rect::new(border_x, 0.0, 4.0, logical.height);
                        renderer.draw_rect(border_rect, p.hover_panel_border);
                    }
                }
                drag_drop::HoverTarget::EmptyPanelButton => {
                    if let Some((new_rect, _)) = empty_panel_btn_rects {
                        renderer.draw_rect(new_rect, p.hover_tab);
                    }
                }
                drag_drop::HoverTarget::EmptyPanelOpenFile => {
                    if let Some((_, open_rect)) = empty_panel_btn_rects {
                        renderer.draw_rect(open_rect, p.hover_tab);
                    }
                }
                drag_drop::HoverTarget::FileFinderItem(idx) => {
                    if let (Some(ref finder), Some(panel_rect)) = (&app.file_finder, editor_panel_rect) {
                        let cell_size = renderer.cell_size();
                        let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
                        let input_y = panel_rect.y + PANE_PADDING + 8.0;
                        let input_h = cell_size.height + 12.0;
                        let list_top = input_y + input_h + 8.0;
                        let vi = idx.saturating_sub(finder.scroll_offset);
                        let y = list_top + vi as f32 * line_height;
                        let row_rect = Rect::new(
                            panel_rect.x + PANE_PADDING,
                            y,
                            panel_rect.width - 2.0 * PANE_PADDING,
                            line_height,
                        );
                        renderer.draw_rect(row_rect, p.hover_tab);
                    }
                }
                drag_drop::HoverTarget::SidebarHandle => {
                    if let Some(ft_rect) = app.file_tree_rect {
                        // Highlight top edge of file tree panel
                        let handle_rect = Rect::new(ft_rect.x, 0.0, ft_rect.width, PANE_PADDING);
                        renderer.draw_rect(handle_rect, p.hover_panel_border);
                    }
                }
                drag_drop::HoverTarget::DockHandle => {
                    if let Some(panel_rect) = editor_panel_rect {
                        // Highlight top edge of editor panel
                        let handle_rect = Rect::new(panel_rect.x, 0.0, panel_rect.width, PANE_PADDING);
                        renderer.draw_rect(handle_rect, p.hover_panel_border);
                    }
                }
            }
        }
    }
}
