use tide_core::{FileTreeSource, Rect, Renderer, TerminalBackend, TextStyle, Vec2};

use crate::drag_drop;
use crate::drag_drop::{DropDestination, HoverTarget, PaneDragState};
use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui::{file_icon, pane_title, panel_tab_title};
use crate::App;

/// Compute the bar offset for a pane. Returns CONFLICT_BAR_HEIGHT if a notification bar
/// (conflict or save confirm) is visible, else 0.
fn bar_offset_for(
    pane_id: tide_core::PaneId,
    panes: &std::collections::HashMap<tide_core::PaneId, PaneKind>,
    save_confirm: &Option<crate::SaveConfirmState>,
) -> f32 {
    if let Some(ref sc) = save_confirm {
        if sc.pane_id == pane_id {
            return CONFLICT_BAR_HEIGHT;
        }
    }
    if let Some(PaneKind::Editor(pane)) = panes.get(&pane_id) {
        if pane.needs_notification_bar() {
            return CONFLICT_BAR_HEIGHT;
        }
    }
    0.0
}

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
        let alive_pane_ids: Vec<u64> = self.panes.keys().copied().collect();
        let empty_panel_btn_rects = self.empty_panel_button_rects();

        let p = self.palette();

        let renderer = self.renderer.as_mut().unwrap();

        // Keep runtime caches bounded to currently alive panes.
        self.pane_generations.retain(|id, _| self.panes.contains_key(id));
        renderer.retain_pane_caches(&alive_pane_ids);

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
                    self.file_tree_width - PANE_GAP,
                    logical.height,
                );
                renderer.draw_chrome_rect(tree_visual_rect, p.file_tree_bg);

                if let Some(tree) = self.file_tree.as_ref() {
                    let cell_size = renderer.cell_size();
                    let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
                    let indent_width = cell_size.width * 1.5;
                    let left_padding = PANE_PADDING;

                    let entries = tree.visible_entries();
                    let text_offset_y = (line_height - cell_size.height) / 2.0;
                    for (i, entry) in entries.iter().enumerate() {
                        let y = PANE_PADDING + i as f32 * line_height - file_tree_scroll;
                        if y + line_height < 0.0 || y > logical.height {
                            continue;
                        }

                        let text_y = y + text_offset_y;
                        let x = left_padding + entry.depth as f32 * indent_width;

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
                            renderer.draw_chrome_rounded_rect(tab_bg_rect, p.panel_tab_bg_active, 4.0);
                        }

                        // Tab title — clip to both tab bounds and panel bounds
                        let text_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
                        let title_clip_w = (PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 14.0)
                            .min((tab_bar_clip.x + tab_bar_clip.width - tx).max(0.0));
                        let clip_x = tx.max(tab_bar_clip.x);
                        let clip = Rect::new(clip_x, tab_bar_top, title_clip_w.max(0.0), PANEL_TAB_HEIGHT);

                        let title = panel_tab_title(&self.panes, tab_id);
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
                            Vec2::new(tx + 12.0, text_y),
                            style,
                            clip,
                        );

                        // Close / modified indicator button
                        let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 4.0;
                        let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
                        // Only draw close button if it's within visible area
                        if close_x + PANEL_TAB_CLOSE_SIZE > tab_bar_clip.x
                            && close_x < tab_bar_clip.x + tab_bar_clip.width
                        {
                            let is_modified = self.panes.get(&tab_id)
                                .and_then(|pk| if let PaneKind::Editor(ep) = pk { Some(ep.editor.is_modified()) } else { None })
                                .unwrap_or(false);
                            let is_close_hovered = matches!(self.hover_target, Some(HoverTarget::PanelTabClose(hid)) if hid == tab_id);
                            let (icon, icon_color) = if is_modified && !is_close_hovered {
                                ("\u{f111}", p.editor_modified)  // ● in modified color
                            } else {
                                ("\u{f00d}", p.tab_text)  // ✕ in normal color
                            };
                            let close_style = TextStyle {
                                foreground: icon_color,
                                background: None,
                                bold: false,
                                dim: false,
                                italic: false,
                                underline: false,
                            };
                            let close_clip = Rect::new(close_x, tab_bar_top, PANEL_TAB_CLOSE_SIZE + 4.0, PANEL_TAB_HEIGHT);
                            renderer.draw_chrome_text(
                                icon,
                                Vec2::new(close_x, close_y),
                                close_style,
                                close_clip,
                            );
                        }
                    }

                } else if let Some(ref finder) = self.file_finder {
                    // File finder UI: search input + file list
                    let cell_size = renderer.cell_size();
                    let cell_height = cell_size.height;
                    let line_height = cell_height * FILE_TREE_LINE_SPACING;
                    let indent_width = cell_size.width * 1.5;

                    let muted_style = TextStyle {
                        foreground: p.tab_text,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: false,
                    };

                    // Search input bar
                    let input_x = panel_rect.x + PANE_PADDING;
                    let input_y = panel_rect.y + PANE_PADDING + 8.0;
                    let input_w = panel_rect.width - 2.0 * PANE_PADDING;
                    let input_h = cell_height + 12.0;
                    let input_rect = Rect::new(input_x, input_y, input_w, input_h);
                    renderer.draw_chrome_rounded_rect(input_rect, p.panel_tab_bg_active, 4.0);

                    // Search icon + query text
                    let query_x = input_x + 8.0;
                    let query_y = input_y + (input_h - cell_height) / 2.0;
                    let search_icon = "\u{f002} "; //
                    let icon_style = TextStyle {
                        foreground: p.tab_text,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_chrome_text(
                        search_icon,
                        Vec2::new(query_x, query_y),
                        icon_style,
                        input_rect,
                    );
                    let text_x = query_x + 2.0 * cell_size.width;
                    let text_style = TextStyle {
                        foreground: p.tab_text_focused,
                        background: None,
                        bold: false,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    let text_clip = Rect::new(text_x, input_y, input_w - 8.0 - 2.0 * cell_size.width, input_h);
                    if finder.query.is_empty() {
                        renderer.draw_chrome_text(
                            "Search files...",
                            Vec2::new(text_x, query_y),
                            muted_style,
                            text_clip,
                        );
                    } else {
                        renderer.draw_chrome_text(
                            &finder.query,
                            Vec2::new(text_x, query_y),
                            text_style,
                            text_clip,
                        );
                    }

                    // Match count
                    let count_text = format!("{}/{}", finder.filtered.len(), finder.entries.len());
                    let count_w = count_text.len() as f32 * cell_size.width;
                    let count_x = input_x + input_w - count_w - 8.0;
                    renderer.draw_chrome_text(
                        &count_text,
                        Vec2::new(count_x, query_y),
                        muted_style,
                        input_rect,
                    );

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
                            renderer.draw_chrome_rounded_rect(sel_rect, p.panel_tab_bg_active, 2.0);
                        }

                        // File icon
                        let text_offset_y = (line_height - cell_height) / 2.0;
                        let file_name = rel_path.file_name()
                            .map(|n| n.to_string_lossy().to_string())
                            .unwrap_or_default();
                        let icon = file_icon(&file_name, false, false);
                        let icon_style = TextStyle {
                            foreground: p.tree_icon,
                            background: None,
                            bold: false,
                            dim: false,
                            italic: false,
                            underline: false,
                        };
                        let icon_x = panel_rect.x + PANE_PADDING + 4.0;
                        let icon_str: String = std::iter::once(icon).collect();
                        renderer.draw_chrome_text(
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
                        renderer.draw_chrome_text(
                            &display_path,
                            Vec2::new(path_x, y + text_offset_y),
                            path_style,
                            list_clip,
                        );
                    }
                } else {
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
            for &(_id, rect) in &visual_pane_rects {
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

            // Tab bar text + close button for each pane
            let cell_height = renderer.cell_size().height;
            for &(id, rect) in &visual_pane_rects {
                let title = pane_title(&self.panes, id);
                let text_color = if focused == Some(id) {
                    p.tab_text_focused
                } else {
                    p.tab_text
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
                let title_clip_w = rect.width - PANE_CLOSE_SIZE - PANE_PADDING * 2.0 - 4.0;
                renderer.draw_chrome_text(
                    &title,
                    Vec2::new(rect.x + PANE_PADDING + 4.0, text_y),
                    style,
                    Rect::new(rect.x, rect.y, title_clip_w.max(0.0) + PANE_PADDING + 4.0, TAB_BAR_HEIGHT),
                );

                // Close / modified indicator button
                let close_x = rect.x + rect.width - PANE_CLOSE_SIZE - PANE_PADDING;
                let close_y = rect.y + (TAB_BAR_HEIGHT - cell_height) / 2.0;
                let is_modified = match self.panes.get(&id) {
                    Some(PaneKind::Editor(ep)) => ep.editor.is_modified(),
                    _ => false,
                };
                let is_close_hovered = matches!(self.hover_target, Some(HoverTarget::PaneTabClose(hid)) if hid == id);
                let (icon, icon_color) = if is_modified && !is_close_hovered {
                    ("\u{f111}", p.editor_modified)
                } else {
                    ("\u{f00d}", p.tab_text)
                };
                let close_style = TextStyle {
                    foreground: icon_color,
                    background: None,
                    bold: false,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                renderer.draw_chrome_text(
                    icon,
                    Vec2::new(close_x, close_y),
                    close_style,
                    Rect::new(close_x, rect.y, PANE_CLOSE_SIZE + PANE_PADDING, TAB_BAR_HEIGHT),
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
                let pane_bar = bar_offset_for(id, &self.panes, &self.save_confirm);
                let inner = Rect::new(
                    rect.x + PANE_PADDING,
                    rect.y + TAB_BAR_HEIGHT + pane_bar,
                    rect.width - 2.0 * PANE_PADDING,
                    (rect.height - TAB_BAR_HEIGHT - PANE_PADDING - pane_bar).max(1.0),
                );
                renderer.begin_pane_grid(id);
                match self.panes.get(&id) {
                    Some(PaneKind::Terminal(pane)) => {
                        pane.render_grid(inner, renderer);
                        self.pane_generations.insert(id, pane.backend.grid_generation());
                    }
                    Some(PaneKind::Editor(pane)) => {
                        pane.render_grid_full(inner, renderer, p.gutter_text, p.gutter_active_text,
                            Some(p.diff_added_bg), Some(p.diff_removed_bg),
                            Some(p.diff_added_gutter), Some(p.diff_removed_gutter));
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
                    let bar_offset = bar_offset_for(active_id, &self.panes, &self.save_confirm);
                    let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP + bar_offset;
                    let inner = Rect::new(
                        panel_rect.x + PANE_PADDING,
                        content_top,
                        panel_rect.width - 2.0 * PANE_PADDING,
                        (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING - bar_offset).max(1.0),
                    );
                    renderer.begin_pane_grid(active_id);
                    pane.render_grid_full(inner, renderer, p.gutter_text, p.gutter_active_text,
                        Some(p.diff_added_bg), Some(p.diff_removed_bg),
                        Some(p.diff_added_gutter), Some(p.diff_removed_gutter));
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
            let pane_bar = bar_offset_for(id, &self.panes, &self.save_confirm);
            let inner = Rect::new(
                rect.x + PANE_PADDING,
                rect.y + TAB_BAR_HEIGHT + pane_bar,
                rect.width - 2.0 * PANE_PADDING,
                (rect.height - TAB_BAR_HEIGHT - PANE_PADDING - pane_bar).max(1.0),
            );
            match self.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => {
                    // Hide cursor when search bar is focused on this pane
                    if search_focus != Some(id) {
                        pane.render_cursor(inner, renderer, p.cursor_accent);
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
                            let sel_color = p.selection;
                            let grid = pane.backend.grid();
                            let max_rows = (inner.height / cell_size.height).ceil() as usize;
                            let max_cols = (inner.width / cell_size.width).floor() as usize;
                            let visible_rows = (grid.rows as usize).min(max_rows);
                            let visible_cols = (grid.cols as usize).min(max_cols);
                            // Center offset matching terminal grid
                            let actual_w = max_cols as f32 * cell_size.width;
                            let center_x = (inner.width - actual_w) / 2.0;
                            for row in start.0..=end.0.min(visible_rows.saturating_sub(1)) {
                                let col_start = if row == start.0 { start.1 } else { 0 };
                                let col_end = if row == end.0 { end.1 } else { visible_cols };
                                if col_start >= col_end {
                                    continue;
                                }
                                let rx = inner.x + center_x + col_start as f32 * cell_size.width;
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
                            // Center offset matching terminal grid
                            let max_cols = (inner.width / cell_size.width).floor() as usize;
                            let actual_w = max_cols as f32 * cell_size.width;
                            let center_x = (inner.width - actual_w) / 2.0;
                            // Visible absolute line range
                            let visible_start = history_size.saturating_sub(display_offset);
                            let visible_end = visible_start + screen_rows;
                            for (mi, m) in search.matches.iter().enumerate() {
                                if m.line < visible_start || m.line >= visible_end {
                                    continue;
                                }
                                let visual_row = m.line - visible_start;
                                let rx = inner.x + center_x + m.col as f32 * cell_size.width;
                                let ry = inner.y + visual_row as f32 * cell_size.height;
                                let rw = m.len as f32 * cell_size.width;
                                let color = if search.current == Some(mi) {
                                    p.search_current_bg
                                } else {
                                    p.search_match_bg
                                };
                                renderer.draw_rect(Rect::new(rx, ry, rw, cell_size.height), color);
                            }
                        }
                    }
                }
                Some(PaneKind::Editor(pane)) => {
                    if search_focus != Some(id) {
                        pane.render_cursor(inner, renderer, p.cursor_accent);
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
                            let sel_color = p.selection;
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
                                    // Full line width: use char count or visible cols
                                    let char_count = pane.editor.buffer.line(row).map_or(0, |l| l.chars().count());
                                    char_count.max(h_scroll + visible_cols)
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
                                    p.search_current_bg
                                } else {
                                    p.search_match_bg
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
                let bar_offset = bar_offset_for(active_id, &self.panes, &self.save_confirm);
                let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP + bar_offset;
                let inner = Rect::new(
                    panel_rect.x + PANE_PADDING,
                    content_top,
                    panel_rect.width - 2.0 * PANE_PADDING,
                    (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING - bar_offset).max(1.0),
                );
                if search_focus != Some(active_id) {
                    pane.render_cursor(inner, renderer, p.cursor_accent);
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
                        let sel_color = p.selection;
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
                                let char_count = pane.editor.buffer.line(row).map_or(0, |l| l.chars().count());
                                char_count.max(h_scroll + visible_cols)
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
                                p.search_current_bg
                            } else {
                                p.search_match_bg
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
            if matches!(self.pane_drag, PaneDragState::Idle) && !self.panel_border_dragging && !self.file_tree_border_dragging {
                match hover {
                    drag_drop::HoverTarget::FileTreeEntry(index) => {
                        if show_file_tree {
                            let cell_size = renderer.cell_size();
                            let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
                            let y = PANE_PADDING + *index as f32 * line_height - file_tree_scroll;
                            if y + line_height > 0.0 && y < logical.height {
                                let row_rect = Rect::new(0.0, y, self.file_tree_width - PANE_GAP, line_height);
                                renderer.draw_rect(row_rect, p.hover_file_tree);
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
                                let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
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
                            let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
                            if let Some(idx) = editor_panel_tabs.iter().position(|&id| id == *tab_id) {
                                let tx = tab_start_x + idx as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
                                let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 4.0;
                                let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - PANEL_TAB_CLOSE_SIZE) / 2.0;
                                let close_rect = Rect::new(close_x, close_y, PANEL_TAB_CLOSE_SIZE, PANEL_TAB_CLOSE_SIZE);
                                renderer.draw_rect(close_rect, p.hover_close);
                            }
                        }
                    }
                    drag_drop::HoverTarget::SplitBorder(dir) => {
                        // Highlight the border line between adjacent panes
                        for &(id_a, rect_a) in &visual_pane_rects {
                            match dir {
                                tide_core::SplitDirection::Horizontal => {
                                    let right_edge = rect_a.x + rect_a.width;
                                    for &(id_b, rect_b) in &visual_pane_rects {
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
                                    for &(id_b, rect_b) in &visual_pane_rects {
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
                        if show_file_tree {
                            let border_x = self.file_tree_width - 2.0;
                            let border_rect = Rect::new(border_x, 0.0, 4.0, logical.height);
                            renderer.draw_rect(border_rect, p.hover_panel_border);
                        }
                    }
                    drag_drop::HoverTarget::PanelBorder => {
                        if let Some(panel_rect) = editor_panel_rect {
                            let border_x = panel_rect.x - 2.0;
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
                        if let (Some(ref finder), Some(panel_rect)) = (&self.file_finder, editor_panel_rect) {
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
                let bar_w = SEARCH_BAR_WIDTH.min(rect.width - 16.0);
                if bar_w < 80.0 { continue; } // too narrow to render
                let bar_h = SEARCH_BAR_HEIGHT;
                let bar_x = rect.x + rect.width - bar_w - 8.0;
                let bar_y = rect.y + TAB_BAR_HEIGHT + 4.0;
                let bar_rect = Rect::new(bar_x, bar_y, bar_w, bar_h);

                // Background (top layer — fully opaque, covers text)
                renderer.draw_top_rect(bar_rect, p.search_bar_bg);

                // Border (only when focused)
                if *is_focused {
                    let bw = 1.0;
                    renderer.draw_top_rect(Rect::new(bar_x, bar_y, bar_w, bw), p.search_bar_border);
                    renderer.draw_top_rect(Rect::new(bar_x, bar_y + bar_h - bw, bar_w, bw), p.search_bar_border);
                    renderer.draw_top_rect(Rect::new(bar_x, bar_y, bw, bar_h), p.search_bar_border);
                    renderer.draw_top_rect(Rect::new(bar_x + bar_w - bw, bar_y, bw, bar_h), p.search_bar_border);
                }

                let text_x = bar_x + 6.0;
                let text_y = bar_y + (bar_h - cell_size.height) / 2.0;
                let text_style = TextStyle {
                    foreground: p.search_bar_text,
                    background: None,
                    bold: false,
                    dim: false,
                    italic: false,
                    underline: false,
                };
                let counter_style = TextStyle {
                    foreground: p.search_bar_counter,
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
                    let cursor_color = p.cursor_accent;
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

        // Render notification bars (conflict / save confirm) for all editor panes
        {
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
            for &(id, rect) in &visual_pane_rects {
                let content_top = rect.y + TAB_BAR_HEIGHT;
                let bar_x = rect.x + PANE_PADDING;
                let bar_w = rect.width - 2.0 * PANE_PADDING;
                bar_panes.push((id, Rect::new(bar_x, content_top, bar_w, CONFLICT_BAR_HEIGHT)));
            }

            for (pane_id, bar_rect) in bar_panes {
                // Check for save confirm bar first
                if let Some(ref sc) = self.save_confirm {
                    if sc.pane_id == pane_id {
                        // Render save confirm bar
                        renderer.draw_top_rect(bar_rect, p.conflict_bar_bg);
                        let text_y = bar_rect.y + (CONFLICT_BAR_HEIGHT - cell_size.height) / 2.0;
                        let text_style = TextStyle {
                            foreground: p.conflict_bar_text,
                            background: None,
                            bold: false,
                            dim: false,
                            italic: false,
                            underline: false,
                        };
                        renderer.draw_top_text("Unsaved changes", Vec2::new(bar_rect.x + 8.0, text_y), text_style, bar_rect);

                        let btn_style = TextStyle {
                            foreground: p.conflict_bar_btn_text,
                            background: None,
                            bold: true,
                            dim: false,
                            italic: false,
                            underline: false,
                        };
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

                // Conflict bar
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&pane_id) {
                    if pane.needs_notification_bar() {
                        renderer.draw_top_rect(bar_rect, p.conflict_bar_bg);
                        let text_y = bar_rect.y + (CONFLICT_BAR_HEIGHT - cell_size.height) / 2.0;
                        let text_style = TextStyle {
                            foreground: p.conflict_bar_text,
                            background: None,
                            bold: false,
                            dim: false,
                            italic: false,
                            underline: false,
                        };
                        let msg = if pane.file_deleted {
                            "File deleted on disk"
                        } else if pane.diff_mode {
                            "Comparing with disk"
                        } else {
                            "File changed on disk"
                        };
                        renderer.draw_top_text(msg, Vec2::new(bar_rect.x + 8.0, text_y), text_style, bar_rect);

                        let btn_style = TextStyle {
                            foreground: p.conflict_bar_btn_text,
                            background: None,
                            bold: true,
                            dim: false,
                            italic: false,
                            underline: false,
                        };
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

                        // Compare button (not in diff mode, not for deleted files)
                        if !pane.file_deleted && !pane.diff_mode {
                            let compare_text = "Compare";
                            let compare_w = compare_text.len() as f32 * cell_size.width + btn_pad * 2.0;
                            let compare_x = overwrite_x - compare_w - 4.0;
                            let compare_rect = Rect::new(compare_x, btn_y, compare_w, btn_h);
                            renderer.draw_top_rect(compare_rect, p.conflict_bar_btn);
                            renderer.draw_top_text(compare_text, Vec2::new(compare_x + btn_pad, text_y), btn_style, compare_rect);
                        }
                    }
                }
            }
        }

        // Render save-as inline edit overlay on the top layer (avoids chrome rebuild per keystroke)
        if let Some(ref save_as) = self.save_as_input {
            if let Some(panel_rect) = editor_panel_rect {
                if let Some(tab_index) = self.editor_panel_tabs.iter().position(|&id| id == save_as.pane_id) {
                    let cell_size = renderer.cell_size();
                    let cell_height = cell_size.height;
                    let tab_bar_top = panel_rect.y + PANE_PADDING;
                    let tab_start_x = panel_rect.x + PANE_PADDING - self.panel_tab_scroll;
                    let tx = tab_start_x + tab_index as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
                    let text_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;

                    // Clip to tab bounds within panel
                    let tab_bar_clip = Rect::new(
                        panel_rect.x + PANE_PADDING,
                        tab_bar_top,
                        panel_rect.width - 2.0 * PANE_PADDING,
                        PANEL_TAB_HEIGHT,
                    );
                    let title_clip_w = (PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 14.0)
                        .min((tab_bar_clip.x + tab_bar_clip.width - tx).max(0.0));
                    let clip_x = tx.max(tab_bar_clip.x);
                    let clip = Rect::new(clip_x, tab_bar_top, title_clip_w.max(0.0), PANEL_TAB_HEIGHT);

                    // Cover original tab title with background
                    renderer.draw_top_rect(
                        Rect::new(tx + 2.0, tab_bar_top + 2.0, PANEL_TAB_WIDTH - 4.0, PANEL_TAB_HEIGHT - 4.0),
                        p.panel_tab_bg_active,
                    );

                    // Draw inline editable filename
                    let input_style = TextStyle {
                        foreground: p.tab_text_focused,
                        background: None,
                        bold: true,
                        dim: false,
                        italic: false,
                        underline: false,
                    };
                    renderer.draw_top_text(
                        &save_as.query,
                        Vec2::new(tx + 12.0, text_y),
                        input_style,
                        clip,
                    );

                    // Cursor beam
                    let cursor_char_offset = save_as.query[..save_as.cursor].chars().count();
                    let cx = tx + 12.0 + cursor_char_offset as f32 * cell_size.width;
                    if cx >= clip.x && cx <= clip.x + clip.width {
                        renderer.draw_top_rect(
                            Rect::new(cx, text_y, 1.5, cell_height),
                            p.cursor_accent,
                        );
                    }
                }
            }
        }

        // Render file finder cursor beam on top layer
        if let (Some(ref finder), Some(panel_rect)) = (&self.file_finder, editor_panel_rect) {
            if editor_panel_tabs.is_empty() {
                let cell_size = renderer.cell_size();
                let cell_height = cell_size.height;
                let input_y = panel_rect.y + PANE_PADDING + 8.0;
                let input_h = cell_height + 12.0;
                let query_x = panel_rect.x + PANE_PADDING + 8.0 + 2.0 * cell_size.width;
                let query_y = input_y + (input_h - cell_height) / 2.0;
                let cursor_char_offset = finder.query[..finder.cursor].chars().count();
                let cx = query_x + cursor_char_offset as f32 * cell_size.width;
                renderer.draw_top_rect(
                    Rect::new(cx, query_y, 1.5, cell_height),
                    p.cursor_accent,
                );
            }
        }

        // Render IME preedit overlay (Korean composition in progress) — only for terminal panes
        if !self.ime_preedit.is_empty() {
            if let Some(focused_id) = focused {
                if let Some((_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == focused_id) {
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get(&focused_id) {
                        let cursor = pane.backend.cursor();
                        let cell_size = renderer.cell_size();
                        let inner_w = rect.width - 2.0 * PANE_PADDING;
                        let max_cols = (inner_w / cell_size.width).floor() as usize;
                        let actual_w = max_cols as f32 * cell_size.width;
                        let center_x = (inner_w - actual_w) / 2.0;
                        let inner_offset = Vec2::new(
                            rect.x + PANE_PADDING + center_x,
                            rect.y + TAB_BAR_HEIGHT,
                        );
                        let cx = inner_offset.x + cursor.col as f32 * cell_size.width;
                        let cy = inner_offset.y + cursor.row as f32 * cell_size.height;

                        // Draw preedit background
                        let preedit_chars: Vec<char> = self.ime_preedit.chars().collect();
                        let pw = preedit_chars.len().max(1) as f32 * cell_size.width;
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
                                Self::draw_swap_preview(renderer, target_rect, p);
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
                                Self::draw_insert_preview(renderer, screen_rect, p);
                            }
                        }
                    }
                }
                DropDestination::EditorPanel => {
                    if let Some(panel_rect) = editor_panel_rect {
                        Self::draw_insert_preview(renderer, panel_rect, p);
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

        // Reclaim completed GPU staging buffers to prevent memory accumulation.
        // Without this, write_buffer() staging allocations are never freed on macOS Metal.
        device.poll(wgpu::Maintain::Poll);
    }

    /// Insert preview: semi-transparent fill + thin border.
    fn draw_insert_preview(renderer: &mut tide_renderer::WgpuRenderer, preview: Rect, p: &ThemePalette) {
        renderer.draw_rect(preview, p.drop_fill);
        let bw = DROP_PREVIEW_BORDER_WIDTH;
        renderer.draw_rect(Rect::new(preview.x, preview.y, preview.width, bw), p.drop_border);
        renderer.draw_rect(Rect::new(preview.x, preview.y + preview.height - bw, preview.width, bw), p.drop_border);
        renderer.draw_rect(Rect::new(preview.x, preview.y, bw, preview.height), p.drop_border);
        renderer.draw_rect(Rect::new(preview.x + preview.width - bw, preview.y, bw, preview.height), p.drop_border);
    }

    /// Swap preview: thick border only, no fill — visually distinct from insert.
    fn draw_swap_preview(renderer: &mut tide_renderer::WgpuRenderer, preview: Rect, p: &ThemePalette) {
        let bw = SWAP_PREVIEW_BORDER_WIDTH;
        renderer.draw_rect(Rect::new(preview.x, preview.y, preview.width, bw), p.swap_border);
        renderer.draw_rect(Rect::new(preview.x, preview.y + preview.height - bw, preview.width, bw), p.swap_border);
        renderer.draw_rect(Rect::new(preview.x, preview.y, bw, preview.height), p.swap_border);
        renderer.draw_rect(Rect::new(preview.x + preview.width - bw, preview.y, bw, preview.height), p.swap_border);
    }
}
