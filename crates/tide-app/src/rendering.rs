use tide_core::{Color, FileTreeSource, Rect, Renderer, TerminalBackend, TextStyle, Vec2};

use crate::drag_drop;
use crate::drag_drop::{DropDestination, PaneDragState};
use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui::{file_icon, pane_title, panel_tab_title};
use crate::App;

impl App {
    pub(crate) fn render(&mut self) {
        let surface = match self.surface.as_ref() {
            Some(s) => s,
            None => return,
        };

        let output = match surface.get_current_texture() {
            Ok(t) => t,
            Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                self.reconfigure_surface();
                return;
            }
            Err(e) => {
                log::error!("Surface error: {}", e);
                return;
            }
        };

        let view = output
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());

        let logical = self.logical_size();
        let focused = self.focused;
        let search_focus = self.search_focus;
        let show_file_tree = self.show_file_tree;
        let file_tree_scroll = self.file_tree_scroll;
        let visual_pane_rects = self.visual_pane_rects.clone();
        let editor_panel_rect = self.editor_panel_rect;
        let editor_panel_tabs = self.editor_panel_tabs.clone();
        let editor_panel_active = self.editor_panel_active;

        let renderer = self.renderer.as_mut().unwrap();

        // Atlas reset → all cached UV coords are stale, force full rebuild
        if renderer.atlas_was_reset() {
            self.pane_generations.clear();
            renderer.invalidate_all_pane_caches();
            self.last_chrome_generation = self.chrome_generation.wrapping_sub(1);
        }

        // Layout change → invalidate all pane caches (positions changed)
        if self.prev_visual_pane_rects != visual_pane_rects {
            self.pane_generations.clear();
            renderer.invalidate_all_pane_caches();
            self.prev_visual_pane_rects = visual_pane_rects.clone();
        }

        renderer.begin_frame(logical);

        // Rebuild chrome layer only when chrome content changed (panel backgrounds, file tree)
        let chrome_dirty = self.chrome_generation != self.last_chrome_generation;
        if chrome_dirty {
            renderer.invalidate_chrome();

            // Draw file tree panel if visible (flat, edge-to-edge)
            if show_file_tree {
                let tree_visual_rect = Rect::new(
                    0.0,
                    0.0,
                    FILE_TREE_WIDTH - BORDER_WIDTH,
                    logical.height,
                );
                renderer.draw_chrome_rect(tree_visual_rect, SURFACE_BG);

                if let Some(tree) = self.file_tree.as_ref() {
                    let cell_size = renderer.cell_size();
                    let line_height = cell_size.height;
                    let indent_width = cell_size.width * 1.5;
                    let left_padding = PANE_PADDING;

                    let entries = tree.visible_entries();
                    for (i, entry) in entries.iter().enumerate() {
                        let y = PANE_PADDING + i as f32 * line_height - file_tree_scroll;
                        if y + line_height < 0.0 || y > logical.height {
                            continue;
                        }

                        let x = left_padding + entry.depth as f32 * indent_width;

                        // Nerd Font icon
                        let icon = file_icon(&entry.entry.name, entry.entry.is_dir, entry.is_expanded);
                        let icon_color = if entry.entry.is_dir {
                            TREE_DIR_COLOR
                        } else {
                            TREE_ICON_COLOR
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
                            Vec2::new(x, y),
                            icon_style,
                            tree_visual_rect,
                        );

                        // Draw name after icon + space
                        let name_x = x + cell_size.width * 2.0;
                        let text_color = if entry.entry.is_dir {
                            TREE_DIR_COLOR
                        } else {
                            TREE_TEXT_COLOR
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
                            Vec2::new(name_x, y),
                            name_style,
                            tree_visual_rect,
                        );
                    }
                }
            }

            // Draw editor panel if visible (flat, border provided by clear color)
            if let Some(panel_rect) = editor_panel_rect {
                renderer.draw_chrome_rect(panel_rect, SURFACE_BG);

                let cell_size = renderer.cell_size();
                let cell_height = cell_size.height;
                let tab_bar_top = panel_rect.y + PANE_PADDING;
                let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
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
                        renderer.draw_chrome_rounded_rect(tab_bg_rect, PANEL_TAB_BG_ACTIVE, 4.0);
                    }

                    // Tab title — clip to both tab bounds and panel bounds
                    let title = panel_tab_title(&self.panes, tab_id);
                    let text_color = if is_active && focused == Some(tab_id) {
                        TAB_BAR_TEXT_FOCUSED
                    } else if is_active {
                        TREE_TEXT_COLOR
                    } else {
                        TAB_BAR_TEXT
                    };
                    let style = TextStyle {
                        foreground: text_color,
                        background: None,
                        bold: is_active,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    let text_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
                    let title_clip_w = (PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 8.0)
                        .min((tab_bar_clip.x + tab_bar_clip.width - tx).max(0.0));
                    let clip_x = tx.max(tab_bar_clip.x);
                    let clip = Rect::new(clip_x, tab_bar_top, title_clip_w.max(0.0), PANEL_TAB_HEIGHT);
                    renderer.draw_chrome_text(
                        &title,
                        Vec2::new(tx + 6.0, text_y),
                        style,
                        clip,
                    );

                    // Close "x" button
                    let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 4.0;
                    let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
                    // Only draw close button if it's within visible area
                    if close_x + PANEL_TAB_CLOSE_SIZE > tab_bar_clip.x
                        && close_x < tab_bar_clip.x + tab_bar_clip.width
                    {
                        let close_style = TextStyle {
                            foreground: TAB_BAR_TEXT,
                            background: None,
                            bold: false,
                            dim: false,
                            italic: false,
                            underline: false,
                        };
                        let close_clip = Rect::new(close_x, tab_bar_top, PANEL_TAB_CLOSE_SIZE + 4.0, PANEL_TAB_HEIGHT);
                        renderer.draw_chrome_text(
                            "\u{f00d}",  // Nerd Font close icon
                            Vec2::new(close_x, close_y),
                            close_style,
                            close_clip,
                        );
                    }
                }

                // Accent border if panel's active pane is focused (all 4 sides)
                if let Some(active) = editor_panel_active {
                    if focused == Some(active) {
                        let bw = BORDER_WIDTH;
                        let r = panel_rect;
                        renderer.draw_chrome_rect(Rect::new(r.x, r.y, r.width, bw), BORDER_FOCUSED);
                        renderer.draw_chrome_rect(Rect::new(r.x, r.y + r.height - bw, r.width, bw), BORDER_FOCUSED);
                        renderer.draw_chrome_rect(Rect::new(r.x, r.y, bw, r.height), BORDER_FOCUSED);
                        renderer.draw_chrome_rect(Rect::new(r.x + r.width - bw, r.y, bw, r.height), BORDER_FOCUSED);
                    }
                }
            }

            // Draw pane backgrounds (flat, unified surface color)
            for &(_id, rect) in &visual_pane_rects {
                renderer.draw_chrome_rect(rect, SURFACE_BG);
            }

            // Accent border on focused pane (all 4 sides)
            if let Some(fid) = focused {
                if let Some(&(_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == fid) {
                    let bw = BORDER_WIDTH;
                    renderer.draw_chrome_rect(Rect::new(rect.x, rect.y, rect.width, bw), BORDER_FOCUSED);
                    renderer.draw_chrome_rect(Rect::new(rect.x, rect.y + rect.height - bw, rect.width, bw), BORDER_FOCUSED);
                    renderer.draw_chrome_rect(Rect::new(rect.x, rect.y, bw, rect.height), BORDER_FOCUSED);
                    renderer.draw_chrome_rect(Rect::new(rect.x + rect.width - bw, rect.y, bw, rect.height), BORDER_FOCUSED);
                }
            }

            // Tab bar text for each pane
            let cell_height = renderer.cell_size().height;
            for &(id, rect) in &visual_pane_rects {
                let title = pane_title(&self.panes, id);
                let text_color = if focused == Some(id) {
                    TAB_BAR_TEXT_FOCUSED
                } else {
                    TAB_BAR_TEXT
                };
                let style = TextStyle {
                    foreground: text_color,
                    background: None,
                    bold: focused == Some(id),
                    dim: false,
                    italic: false,
                    underline: false,
                };
                let text_y = rect.y + (TAB_BAR_HEIGHT - cell_height) / 2.0;
                renderer.draw_chrome_text(
                    &title,
                    Vec2::new(rect.x + PANE_PADDING + 4.0, text_y),
                    style,
                    Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT),
                );
            }

            self.last_chrome_generation = self.chrome_generation;
        }

        // Per-pane dirty checking: only rebuild panes whose content changed
        let mut any_dirty = false;
        for &(id, rect) in &visual_pane_rects {
            let gen = match self.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => pane.backend.grid_generation(),
                Some(PaneKind::Editor(pane)) => pane.generation(),
                None => continue,
            };
            let prev = self.pane_generations.get(&id).copied().unwrap_or(u64::MAX);
            if gen != prev {
                any_dirty = true;
                let inner = Rect::new(
                    rect.x + PANE_PADDING,
                    rect.y + TAB_BAR_HEIGHT,
                    rect.width - 2.0 * PANE_PADDING,
                    rect.height - TAB_BAR_HEIGHT - PANE_PADDING,
                );
                renderer.begin_pane_grid(id);
                match self.panes.get(&id) {
                    Some(PaneKind::Terminal(pane)) => {
                        pane.render_grid(inner, renderer);
                        self.pane_generations.insert(id, pane.backend.grid_generation());
                    }
                    Some(PaneKind::Editor(pane)) => {
                        pane.render_grid(inner, renderer);
                        self.pane_generations.insert(id, pane.generation());
                    }
                    None => {}
                }
                renderer.end_pane_grid();
            }
        }

        // Also check active panel editor pane
        if let (Some(active_id), Some(panel_rect)) = (editor_panel_active, editor_panel_rect) {
            if let Some(PaneKind::Editor(pane)) = self.panes.get(&active_id) {
                let gen = pane.generation();
                let prev = self.pane_generations.get(&active_id).copied().unwrap_or(u64::MAX);
                if gen != prev {
                    any_dirty = true;
                    let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                    let inner = Rect::new(
                        panel_rect.x + PANE_PADDING,
                        content_top,
                        panel_rect.width - 2.0 * PANE_PADDING,
                        (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0),
                    );
                    renderer.begin_pane_grid(active_id);
                    pane.render_grid(inner, renderer);
                    renderer.end_pane_grid();
                    self.pane_generations.insert(active_id, pane.generation());
                }
            }
        }

        // Assemble all pane caches into the global grid arrays if anything changed
        if any_dirty {
            let mut order: Vec<u64> = visual_pane_rects.iter().map(|(id, _)| *id).collect();
            if let Some(active_id) = editor_panel_active {
                order.push(active_id);
            }
            renderer.assemble_grid(&order);
        }

        // Always render cursor (overlay layer) — cursor blinks/moves independently
        for &(id, rect) in &visual_pane_rects {
            let inner = Rect::new(
                rect.x + PANE_PADDING,
                rect.y + TAB_BAR_HEIGHT,
                rect.width - 2.0 * PANE_PADDING,
                rect.height - TAB_BAR_HEIGHT - PANE_PADDING,
            );
            match self.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => {
                    // Hide cursor when search bar is focused on this pane
                    if search_focus != Some(id) {
                        pane.render_cursor(inner, renderer);
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
                            let sel_color = Color::new(0.35, 0.58, 1.0, 0.25);
                            let grid = pane.backend.grid();
                            let max_rows = (inner.height / cell_size.height).ceil() as usize;
                            let max_cols = (inner.width / cell_size.width).ceil() as usize;
                            let visible_rows = (grid.rows as usize).min(max_rows);
                            let visible_cols = (grid.cols as usize).min(max_cols);
                            for row in start.0..=end.0.min(visible_rows.saturating_sub(1)) {
                                let col_start = if row == start.0 { start.1 } else { 0 };
                                let col_end = if row == end.0 { end.1 } else { visible_cols };
                                if col_start >= col_end {
                                    continue;
                                }
                                let rx = inner.x + col_start as f32 * cell_size.width;
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
                        if search.visible && !search.query.is_empty() {
                            let cell_size = renderer.cell_size();
                            let history_size = pane.backend.history_size();
                            let display_offset = pane.backend.display_offset();
                            let grid = pane.backend.grid();
                            let screen_rows = grid.rows as usize;
                            // Visible absolute line range
                            let visible_start = history_size.saturating_sub(display_offset);
                            let visible_end = visible_start + screen_rows;
                            for (mi, m) in search.matches.iter().enumerate() {
                                if m.line < visible_start || m.line >= visible_end {
                                    continue;
                                }
                                let visual_row = m.line - visible_start;
                                let rx = inner.x + m.col as f32 * cell_size.width;
                                let ry = inner.y + visual_row as f32 * cell_size.height;
                                let rw = m.len as f32 * cell_size.width;
                                let color = if search.current == Some(mi) {
                                    SEARCH_CURRENT_BG
                                } else {
                                    SEARCH_MATCH_BG
                                };
                                renderer.draw_rect(Rect::new(rx, ry, rw, cell_size.height), color);
                            }
                        }
                    }
                }
                Some(PaneKind::Editor(pane)) => {
                    if search_focus != Some(id) {
                        pane.render_cursor(inner, renderer);
                    }
                    // Render editor selection highlight
                    if let Some(ref sel) = pane.selection {
                        let cell_size = renderer.cell_size();
                        let (start, end) = if sel.anchor <= sel.end {
                            (sel.anchor, sel.end)
                        } else {
                            (sel.end, sel.anchor)
                        };
                        if start != end {
                            let sel_color = Color::new(0.35, 0.58, 1.0, 0.25);
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
                                    // Full line width: use buffer line length or visible cols
                                    let line_len = pane.editor.buffer.line(row).map_or(0, |l| l.len());
                                    line_len.max(h_scroll + visible_cols)
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
                    // Render editor search highlights
                    if let Some(ref search) = pane.search {
                        if search.visible && !search.query.is_empty() {
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
                                    SEARCH_CURRENT_BG
                                } else {
                                    SEARCH_MATCH_BG
                                };
                                renderer.draw_rect(Rect::new(rx, ry, rw, cell_size.height), color);
                            }
                        }
                    }
                    // Render editor scrollbar with search match markers
                    pane.render_scrollbar(inner, renderer, pane.search.as_ref());
                }
                None => {}
            }
        }

        // Render cursor for active panel editor
        if let (Some(active_id), Some(panel_rect)) = (editor_panel_active, editor_panel_rect) {
            if let Some(PaneKind::Editor(pane)) = self.panes.get(&active_id) {
                let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                let inner = Rect::new(
                    panel_rect.x + PANE_PADDING,
                    content_top,
                    panel_rect.width - 2.0 * PANE_PADDING,
                    (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0),
                );
                if search_focus != Some(active_id) {
                    pane.render_cursor(inner, renderer);
                }

                // Panel editor selection highlight
                if let Some(ref sel) = pane.selection {
                    let cell_size = renderer.cell_size();
                    let (start, end) = if sel.anchor <= sel.end {
                        (sel.anchor, sel.end)
                    } else {
                        (sel.end, sel.anchor)
                    };
                    if start != end {
                        let sel_color = Color::new(0.35, 0.58, 1.0, 0.25);
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
                                let line_len = pane.editor.buffer.line(row).map_or(0, |l| l.len());
                                line_len.max(h_scroll + visible_cols)
                            };
                            if col_start >= col_end {
                                continue;
                            }
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

                // Panel editor search highlights
                if let Some(ref search) = pane.search {
                    if search.visible && !search.query.is_empty() {
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
                                SEARCH_CURRENT_BG
                            } else {
                                SEARCH_MATCH_BG
                            };
                            renderer.draw_rect(Rect::new(rx, ry, rw, cell_size.height), color);
                        }
                    }
                }
                // Render panel editor scrollbar with search match markers
                pane.render_scrollbar(inner, renderer, pane.search.as_ref());
            }
        }

        // Render hover highlights (overlay layer)
        if let Some(ref hover) = self.hover_target {
            // Skip hover rendering during drag
            if matches!(self.pane_drag, PaneDragState::Idle) && !self.panel_border_dragging {
                match hover {
                    drag_drop::HoverTarget::FileTreeEntry(index) => {
                        if show_file_tree {
                            let cell_size = renderer.cell_size();
                            let line_height = cell_size.height;
                            let y = PANE_PADDING + *index as f32 * line_height - file_tree_scroll;
                            if y + line_height > 0.0 && y < logical.height {
                                let row_rect = Rect::new(0.0, y, FILE_TREE_WIDTH - BORDER_WIDTH, line_height);
                                renderer.draw_rect(row_rect, HOVER_FILE_TREE);
                            }
                        }
                    }
                    drag_drop::HoverTarget::PaneTabBar(pane_id) => {
                        if let Some(&(_, rect)) = visual_pane_rects.iter().find(|(id, _)| id == pane_id) {
                            let tab_rect = Rect::new(rect.x, rect.y, rect.width, TAB_BAR_HEIGHT);
                            renderer.draw_rect(tab_rect, HOVER_TAB);
                        }
                    }
                    drag_drop::HoverTarget::PanelTab(tab_id) => {
                        // Only highlight inactive tabs (active tab already has background)
                        if editor_panel_active != Some(*tab_id) {
                            if let Some(panel_rect) = editor_panel_rect {
                                let tab_bar_top = panel_rect.y + PANE_PADDING;
                                let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
                                if let Some(idx) = editor_panel_tabs.iter().position(|&id| id == *tab_id) {
                                    let tx = tab_start_x + idx as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
                                    let tab_rect = Rect::new(tx, tab_bar_top, PANEL_TAB_WIDTH, PANEL_TAB_HEIGHT);
                                    renderer.draw_rect(tab_rect, HOVER_TAB);
                                }
                            }
                        }
                    }
                    drag_drop::HoverTarget::PanelTabClose(tab_id) => {
                        if let Some(panel_rect) = editor_panel_rect {
                            let tab_bar_top = panel_rect.y + PANE_PADDING;
                            let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
                            if let Some(idx) = editor_panel_tabs.iter().position(|&id| id == *tab_id) {
                                let tx = tab_start_x + idx as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
                                let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 4.0;
                                let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - PANEL_TAB_CLOSE_SIZE) / 2.0;
                                let close_rect = Rect::new(close_x, close_y, PANEL_TAB_CLOSE_SIZE, PANEL_TAB_CLOSE_SIZE);
                                renderer.draw_rect(close_rect, HOVER_CLOSE_BUTTON);
                            }
                        }
                    }
                    drag_drop::HoverTarget::PanelBorder => {
                        if let Some(panel_rect) = editor_panel_rect {
                            let border_x = panel_rect.x - 2.0;
                            let border_rect = Rect::new(border_x, 0.0, 4.0, logical.height);
                            renderer.draw_rect(border_rect, HOVER_PANEL_BORDER);
                        }
                    }
                }
            }
        }

        // Render search bar UI for panes that have search visible
        {
            let search_focus = self.search_focus;
            let cell_size = renderer.cell_size();

            // Helper: render a search bar floating at top-right of a given rect
            let mut search_bars: Vec<(tide_core::PaneId, Rect, String, String, usize, bool)> = Vec::new();
            for &(id, rect) in &visual_pane_rects {
                let (query, display, cursor_pos, visible) = match self.panes.get(&id) {
                    Some(PaneKind::Terminal(pane)) => match &pane.search {
                        Some(s) if s.visible => (s.query.clone(), s.current_display(), s.cursor, true),
                        _ => continue,
                    },
                    Some(PaneKind::Editor(pane)) => match &pane.search {
                        Some(s) if s.visible => (s.query.clone(), s.current_display(), s.cursor, true),
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
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&active_id) {
                    if let Some(ref s) = pane.search {
                        if s.visible {
                            search_bars.push((active_id, panel_rect, s.query.clone(), s.current_display(), s.cursor, search_focus == Some(active_id)));
                        }
                    }
                }
            }

            for (_id, rect, query, display, cursor_pos, is_focused) in &search_bars {
                let bar_w = SEARCH_BAR_WIDTH;
                let bar_h = SEARCH_BAR_HEIGHT;
                let bar_x = rect.x + rect.width - bar_w - 8.0;
                let bar_y = rect.y + TAB_BAR_HEIGHT + 4.0;
                let bar_rect = Rect::new(bar_x, bar_y, bar_w, bar_h);

                // Background (top layer — fully opaque, covers text)
                renderer.draw_top_rect(bar_rect, SEARCH_BAR_BG);

                // Border (only when focused)
                if *is_focused {
                    let bw = 1.0;
                    renderer.draw_top_rect(Rect::new(bar_x, bar_y, bar_w, bw), SEARCH_BAR_BORDER);
                    renderer.draw_top_rect(Rect::new(bar_x, bar_y + bar_h - bw, bar_w, bw), SEARCH_BAR_BORDER);
                    renderer.draw_top_rect(Rect::new(bar_x, bar_y, bw, bar_h), SEARCH_BAR_BORDER);
                    renderer.draw_top_rect(Rect::new(bar_x + bar_w - bw, bar_y, bw, bar_h), SEARCH_BAR_BORDER);
                }

                let text_x = bar_x + 6.0;
                let text_y = bar_y + (bar_h - cell_size.height) / 2.0;
                let text_style = TextStyle {
                    foreground: SEARCH_BAR_TEXT,
                    background: None,
                    bold: false,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                let counter_style = TextStyle {
                    foreground: SEARCH_BAR_COUNTER,
                    background: None,
                    bold: false,
                    dim: false,
                    italic: false,
                    underline: false,
                };

                // Layout: [query text] [counter] [close button]
                let close_area_w = SEARCH_BAR_CLOSE_SIZE;
                let close_x = bar_x + bar_w - close_area_w;
                let counter_w = display.len() as f32 * cell_size.width;
                let counter_x = close_x - counter_w - 4.0;
                let text_clip_w = (counter_x - text_x - 4.0).max(0.0);

                // Query text (top layer)
                let text_clip = Rect::new(text_x, bar_y, text_clip_w, bar_h);
                renderer.draw_top_text(query, Vec2::new(text_x, text_y), text_style, text_clip);

                // Text cursor (beam) — only when focused
                if *is_focused {
                    let cursor_char_offset = query[..*cursor_pos].chars().count();
                    let cx = text_x + cursor_char_offset as f32 * cell_size.width;
                    let cursor_color = Color::new(0.35, 0.58, 1.0, 0.9);
                    renderer.draw_top_rect(Rect::new(cx, text_y, 1.5, cell_size.height), cursor_color);
                }

                // Counter text
                let counter_clip = Rect::new(counter_x, bar_y, counter_w + 4.0, bar_h);
                renderer.draw_top_text(display, Vec2::new(counter_x, text_y), counter_style, counter_clip);

                // Close button "×"
                let close_icon_x = close_x + (close_area_w - cell_size.width) / 2.0;
                let close_clip = Rect::new(close_x, bar_y, close_area_w, bar_h);
                renderer.draw_top_text("\u{f00d}", Vec2::new(close_icon_x, text_y), counter_style, close_clip);
            }
        }

        // Render IME preedit overlay (Korean composition in progress) — only for terminal panes
        if !self.ime_preedit.is_empty() {
            if let Some(focused_id) = focused {
                if let Some((_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == focused_id) {
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get(&focused_id) {
                        let cursor = pane.backend.cursor();
                        let cell_size = renderer.cell_size();
                        let inner_offset = Vec2::new(
                            rect.x + PANE_PADDING,
                            rect.y + TAB_BAR_HEIGHT,
                        );
                        let cx = inner_offset.x + cursor.col as f32 * cell_size.width;
                        let cy = inner_offset.y + cursor.row as f32 * cell_size.height;

                        // Draw preedit background
                        let preedit_chars: Vec<char> = self.ime_preedit.chars().collect();
                        let pw = preedit_chars.len().max(1) as f32 * cell_size.width;
                        let preedit_bg = Color::new(0.18, 0.22, 0.38, 1.0);
                        renderer.draw_rect(
                            Rect::new(cx, cy, pw, cell_size.height),
                            preedit_bg,
                        );

                        // Draw each preedit character
                        let preedit_style = TextStyle {
                            foreground: Color::new(0.95, 0.96, 1.0, 1.0),
                            background: None,
                            bold: false,
                            dim: false,
                            italic: false,
                            underline: true,
                        };
                        for (i, &ch) in preedit_chars.iter().enumerate() {
                            renderer.draw_cell(
                                ch,
                                cursor.row as usize,
                                cursor.col as usize + i,
                                preedit_style,
                                cell_size,
                                inner_offset,
                            );
                        }
                    }
                }
            }
        }

        // Draw drop preview overlay when dragging a pane
        if let PaneDragState::Dragging {
            source_pane,
            from_panel,
            drop_target: Some(ref dest),
        } = &self.pane_drag {
            match dest {
                DropDestination::TreeRoot(zone) | DropDestination::TreePane(_, zone) => {
                    let is_swap = *zone == tide_core::DropZone::Center;

                    if is_swap {
                        // Swap preview: border-only outline around target's visual rect
                        if let DropDestination::TreePane(target_id, _) = dest {
                            if let Some(&(_, target_rect)) = visual_pane_rects.iter().find(|(id, _)| *id == *target_id) {
                                Self::draw_swap_preview(renderer, target_rect);
                            }
                        }
                    } else {
                        // Use simulate_drop for accurate preview
                        let source_in_tree = !from_panel;
                        let target_id = match dest {
                            DropDestination::TreePane(tid, _) => Some(*tid),
                            _ => None,
                        };
                        if let Some(pane_area) = self.pane_area_rect {
                            let pane_area_size = tide_core::Size::new(pane_area.width, pane_area.height);
                            if let Some(preview_rect) = self.layout.simulate_drop(
                                *source_pane, target_id, *zone, source_in_tree, pane_area_size,
                            ) {
                                // Offset from layout space to screen space
                                let screen_rect = Rect::new(
                                    preview_rect.x + pane_area.x,
                                    preview_rect.y + pane_area.y,
                                    preview_rect.width,
                                    preview_rect.height,
                                );
                                Self::draw_insert_preview(renderer, screen_rect);
                            }
                        }
                    }
                }
                DropDestination::EditorPanel => {
                    if let Some(panel_rect) = editor_panel_rect {
                        Self::draw_insert_preview(renderer, panel_rect);
                    }
                }
            }
        }

        renderer.end_frame();

        let device = self.device.as_ref().unwrap();
        let queue = self.queue.as_ref().unwrap();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("render_encoder"),
        });

        renderer.render_frame(&mut encoder, &view);

        queue.submit(std::iter::once(encoder.finish()));
        output.present();
    }

    /// Insert preview: semi-transparent fill + thin border.
    fn draw_insert_preview(renderer: &mut tide_renderer::WgpuRenderer, preview: Rect) {
        renderer.draw_rect(preview, DROP_PREVIEW_FILL);
        let bw = DROP_PREVIEW_BORDER_WIDTH;
        renderer.draw_rect(Rect::new(preview.x, preview.y, preview.width, bw), DROP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x, preview.y + preview.height - bw, preview.width, bw), DROP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x, preview.y, bw, preview.height), DROP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x + preview.width - bw, preview.y, bw, preview.height), DROP_PREVIEW_BORDER);
    }

    /// Swap preview: thick border only, no fill — visually distinct from insert.
    fn draw_swap_preview(renderer: &mut tide_renderer::WgpuRenderer, preview: Rect) {
        let bw = SWAP_PREVIEW_BORDER_WIDTH;
        renderer.draw_rect(Rect::new(preview.x, preview.y, preview.width, bw), SWAP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x, preview.y + preview.height - bw, preview.width, bw), SWAP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x, preview.y, bw, preview.height), SWAP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x + preview.width - bw, preview.y, bw, preview.height), SWAP_PREVIEW_BORDER);
    }
}
