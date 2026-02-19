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
    _logical: tide_core::Size,
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
                            let y = ft_rect.y + FILE_TREE_HEADER_HEIGHT + *index as f32 * line_height - file_tree_scroll;
                            if y + line_height > ft_rect.y && y < ft_rect.y + ft_rect.height {
                                let row_rect = Rect::new(ft_rect.x, y, ft_rect.width, line_height);
                                renderer.draw_rect(row_rect, p.hover_file_tree);
                            }
                        }
                    }
                }
                drag_drop::HoverTarget::StackedTab(tab_id) => {
                    // Highlight stacked inline tab (only inactive tabs)
                    if let PaneAreaMode::Stacked(active) = app.pane_area_mode {
                        if active != *tab_id {
                            if let Some(&(_, rect)) = app.visual_pane_rects.first() {
                                let cell_w = renderer.cell_size().width;
                                let pane_ids = app.layout.pane_ids();
                                let mut tx = rect.x + PANE_PADDING;
                                for &pid in pane_ids.iter() {
                                    let title = crate::ui::pane_title(&app.panes, pid);
                                    let tab_w = crate::ui::stacked_tab_width(&title, cell_w);
                                    if pid == *tab_id {
                                        renderer.draw_rect(
                                            Rect::new(tx, rect.y, tab_w, TAB_BAR_HEIGHT),
                                            p.hover_tab,
                                        );
                                        break;
                                    }
                                    tx += tab_w;
                                }
                            }
                        }
                    }
                }
                drag_drop::HoverTarget::StackedTabClose(_tab_id) => {
                    // Single close button on header right
                    if let Some(&(_, rect)) = app.visual_pane_rects.first() {
                        let cell_w = renderer.cell_size().width;
                        let cell_h = renderer.cell_size().height;
                        let content_right = rect.x + rect.width - PANE_PADDING;
                        let close_w = cell_w + BADGE_PADDING_H * 2.0;
                        let close_x = content_right - close_w;
                        let close_y = rect.y + (TAB_BAR_HEIGHT - cell_h - 2.0) / 2.0;
                        renderer.draw_rect(
                            Rect::new(close_x, close_y, close_w, cell_h + 2.0),
                            p.hover_close,
                        );
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
                        let cell_w = renderer.cell_size().width;
                        let grid_cols = ((rect.width - 2.0 * PANE_PADDING) / cell_w).floor();
                        let grid_right = rect.x + PANE_PADDING + grid_cols * cell_w;
                        let close_w = cell_w + BADGE_PADDING_H * 2.0;
                        let close_x = grid_right - close_w;
                        let close_y = rect.y + (TAB_BAR_HEIGHT - renderer.cell_size().height - 2.0) / 2.0;
                        let close_rect = Rect::new(close_x, close_y, close_w, renderer.cell_size().height + 2.0);
                        renderer.draw_rect(close_rect, p.hover_close);
                    }
                }
                drag_drop::HoverTarget::PanelTab(tab_id) => {
                    // Only highlight inactive dock tabs (active already has bg)
                    if editor_panel_active != Some(*tab_id) {
                        if let Some(panel_rect) = editor_panel_rect {
                            let cell_w = renderer.cell_size().width;
                            let tab_bar_top = panel_rect.y;
                            let mut tx = panel_rect.x - app.panel_tab_scroll;
                            for &tid in editor_panel_tabs.iter() {
                                let title = crate::ui::panel_tab_title(&app.panes, tid);
                                let tab_w = crate::ui::dock_tab_width(&title, cell_w);
                                if tid == *tab_id {
                                    renderer.draw_rect(
                                        Rect::new(tx, tab_bar_top, tab_w, PANEL_TAB_HEIGHT),
                                        p.hover_tab,
                                    );
                                    break;
                                }
                                tx += tab_w;
                            }
                        }
                    }
                }
                drag_drop::HoverTarget::PanelTabClose(tab_id) => {
                    if let Some(panel_rect) = editor_panel_rect {
                        let cell_w = renderer.cell_size().width;
                        let cell_h = renderer.cell_size().height;
                        let tab_bar_top = panel_rect.y;
                        let mut tx = panel_rect.x - app.panel_tab_scroll;
                        for &tid in editor_panel_tabs.iter() {
                            let title = crate::ui::panel_tab_title(&app.panes, tid);
                            let tab_w = crate::ui::dock_tab_width(&title, cell_w);
                            if tid == *tab_id {
                                let icon_x = tx + DOCK_TAB_PAD + title.chars().count() as f32 * cell_w + DOCK_TAB_GAP;
                                let icon_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_h) / 2.0;
                                renderer.draw_rect(
                                    Rect::new(icon_x, icon_y, cell_w, cell_h),
                                    p.hover_close,
                                );
                                break;
                            }
                            tx += tab_w;
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
                        let border_rect = Rect::new(border_x, ft_rect.y, 4.0, ft_rect.height);
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
                        let border_rect = Rect::new(border_x, panel_rect.y, 4.0, panel_rect.height);
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
                        let handle_rect = Rect::new(ft_rect.x, ft_rect.y, ft_rect.width, PANE_PADDING);
                        renderer.draw_rect(handle_rect, p.hover_panel_border);
                    }
                }
                drag_drop::HoverTarget::DockHandle => {
                    if let Some(panel_rect) = editor_panel_rect {
                        // Highlight top edge of editor panel
                        let handle_rect = Rect::new(panel_rect.x, panel_rect.y, panel_rect.width, PANE_PADDING);
                        renderer.draw_rect(handle_rect, p.hover_panel_border);
                    }
                }
                drag_drop::HoverTarget::TitlebarSwap => {
                    // Hover is rendered via chrome.rs (badge_bg on swap icon)
                    // No additional overlay needed since chrome already handles it
                }
                drag_drop::HoverTarget::TitlebarFileTree => {
                    // Hover is rendered via chrome.rs (badge_bg on sidebar button)
                }
                drag_drop::HoverTarget::TitlebarPaneArea => {
                    // Hover is rendered via chrome.rs (badge_bg on pane area button)
                }
                drag_drop::HoverTarget::TitlebarDock => {
                    // Hover is rendered via chrome.rs (badge_bg on dock button)
                }
                drag_drop::HoverTarget::PaneModeToggle => {
                    // Hover is rendered via chrome.rs (badge_bg on mode toggle)
                }
                drag_drop::HoverTarget::PaneMaximize(pane_id) => {
                    // Highlight maximize icon on split pane header
                    if let Some(&(_, rect)) = visual_pane_rects.iter().find(|(id, _)| id == pane_id) {
                        let cell_w = renderer.cell_size().width;
                        let cell_h = renderer.cell_size().height;
                        let grid_cols = ((rect.width - 2.0 * PANE_PADDING) / cell_w).floor();
                        let grid_right = rect.x + PANE_PADDING + grid_cols * cell_w;
                        let close_w = cell_w + BADGE_PADDING_H * 2.0;
                        let close_x = grid_right - close_w;
                        let max_w = cell_w + BADGE_PADDING_H * 2.0;
                        let max_x = close_x - BADGE_GAP - max_w;
                        let max_y = rect.y + (TAB_BAR_HEIGHT - cell_h - 2.0) / 2.0;
                        renderer.draw_rect(
                            Rect::new(max_x, max_y, max_w, cell_h + 2.0),
                            p.hover_tab,
                        );
                    }
                }
                drag_drop::HoverTarget::PaneAreaMaximize => {
                    // Hover is rendered via chrome.rs (bg on stacked maximize badge)
                }
                drag_drop::HoverTarget::DockMaximize => {
                    // Hover is rendered via chrome.rs (bg on maximize icon)
                }
            }
        }
    }
}
