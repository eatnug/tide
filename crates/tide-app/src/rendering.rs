use tide_core::{Color, FileTreeSource, Rect, Renderer, TerminalBackend, TextStyle, Vec2};

use crate::drag_drop::{DropDestination, PaneDragState};
use crate::pane::PaneKind;
use crate::theme::*;
use crate::ui::{compute_preview_rect, file_icon, pane_title, panel_tab_title};
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
            self.last_chrome_generation = self.chrome_generation.wrapping_sub(1);
        }

        renderer.begin_frame(logical);

        // Rebuild chrome layer only when chrome content changed (panel backgrounds, file tree)
        let chrome_dirty = self.chrome_generation != self.last_chrome_generation;
        if chrome_dirty {
            renderer.invalidate_chrome();

            // Draw file tree panel if visible (rounded rect background)
            if show_file_tree {
                let tree_visual_rect = Rect::new(
                    PANE_GAP,
                    PANE_GAP,
                    FILE_TREE_WIDTH - PANE_GAP - PANE_GAP / 2.0,
                    logical.height - PANE_GAP * 2.0,
                );
                renderer.draw_chrome_rounded_rect(tree_visual_rect, TREE_BG, PANE_RADIUS);

                if let Some(tree) = self.file_tree.as_ref() {
                    let cell_size = renderer.cell_size();
                    let line_height = cell_size.height;
                    let indent_width = cell_size.width * 1.5;
                    let left_padding = PANE_GAP + PANE_PADDING;

                    let entries = tree.visible_entries();
                    for (i, entry) in entries.iter().enumerate() {
                        let y = PANE_GAP + PANE_PADDING + i as f32 * line_height - file_tree_scroll;
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

            // Draw editor panel if visible
            if let Some(panel_rect) = editor_panel_rect {
                renderer.draw_chrome_rounded_rect(panel_rect, TREE_BG, PANE_RADIUS);

                let cell_size = renderer.cell_size();
                let cell_height = cell_size.height;
                let tab_bar_top = panel_rect.y + PANE_PADDING;
                let tab_start_x = panel_rect.x + PANE_PADDING;

                // Draw horizontal tab bar
                for (i, &tab_id) in editor_panel_tabs.iter().enumerate() {
                    let tx = tab_start_x + i as f32 * (PANEL_TAB_WIDTH + PANEL_TAB_GAP);
                    let is_active = editor_panel_active == Some(tab_id);

                    // Tab background
                    if is_active {
                        let tab_bg_rect = Rect::new(tx, tab_bar_top, PANEL_TAB_WIDTH, PANEL_TAB_HEIGHT);
                        renderer.draw_chrome_rounded_rect(tab_bg_rect, PANEL_TAB_BG_ACTIVE, 4.0);
                    }

                    // Tab title
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
                        italic: false,
                        underline: false,
                    };
                    let text_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
                    let clip = Rect::new(tx, tab_bar_top, PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 8.0, PANEL_TAB_HEIGHT);
                    renderer.draw_chrome_text(
                        &title,
                        Vec2::new(tx + 6.0, text_y),
                        style,
                        clip,
                    );

                    // Close "x" button
                    let close_x = tx + PANEL_TAB_WIDTH - PANEL_TAB_CLOSE_SIZE - 4.0;
                    let close_y = tab_bar_top + (PANEL_TAB_HEIGHT - cell_height) / 2.0;
                    let close_style = TextStyle {
                        foreground: TAB_BAR_TEXT,
                        background: None,
                        bold: false,
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

                // Focus accent bar if panel's active pane is focused
                if let Some(active) = editor_panel_active {
                    if focused == Some(active) {
                        let bar_h = 2.0;
                        let bar_rect = Rect::new(
                            panel_rect.x + PANE_RADIUS,
                            panel_rect.y,
                            panel_rect.width - PANE_RADIUS * 2.0,
                            bar_h,
                        );
                        renderer.draw_chrome_rect(bar_rect, ACCENT_COLOR);
                    }
                }
            }

            // Draw pane backgrounds as rounded rects
            for &(id, rect) in &visual_pane_rects {
                let bg = if focused == Some(id) {
                    PANE_BG_FOCUSED
                } else {
                    PANE_BG
                };
                renderer.draw_chrome_rounded_rect(rect, bg, PANE_RADIUS);
            }

            // Focus accent bar: thin colored bar at the top of the focused pane (tree panes)
            if let Some(fid) = focused {
                if let Some(&(_, rect)) = visual_pane_rects.iter().find(|(id, _)| *id == fid) {
                    let bar_h = 2.0;
                    let bar_rect = Rect::new(
                        rect.x + PANE_RADIUS,
                        rect.y,
                        rect.width - PANE_RADIUS * 2.0,
                        bar_h,
                    );
                    renderer.draw_chrome_rect(bar_rect, ACCENT_COLOR);
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

        // Check if grid needs rebuild (any pane content or layout changed)
        // Include active panel pane in dirty checking
        let mut grid_dirty = false;
        for &(id, _) in &visual_pane_rects {
            let gen = match self.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => pane.backend.grid_generation(),
                Some(PaneKind::Editor(pane)) => pane.generation(),
                None => continue,
            };
            let prev = self.pane_generations.get(&id).copied().unwrap_or(u64::MAX);
            if gen != prev {
                grid_dirty = true;
                break;
            }
        }
        // Also check active panel editor pane
        if !grid_dirty {
            if let Some(active_id) = editor_panel_active {
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&active_id) {
                    let gen = pane.generation();
                    let prev = self.pane_generations.get(&active_id).copied().unwrap_or(u64::MAX);
                    if gen != prev {
                        grid_dirty = true;
                    }
                }
            }
        }

        // Rebuild grid layer only when content or layout changed
        if grid_dirty {
            renderer.invalidate_grid();
            for &(id, rect) in &visual_pane_rects {
                let inner = Rect::new(
                    rect.x + PANE_PADDING,
                    rect.y + TAB_BAR_HEIGHT,
                    rect.width - 2.0 * PANE_PADDING,
                    rect.height - TAB_BAR_HEIGHT - PANE_PADDING,
                );
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
            }

            // Render active panel editor grid
            if let (Some(active_id), Some(panel_rect)) = (editor_panel_active, editor_panel_rect) {
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&active_id) {
                    let content_top = panel_rect.y + PANE_PADDING + PANEL_TAB_HEIGHT + PANE_GAP;
                    let inner = Rect::new(
                        panel_rect.x + PANE_PADDING,
                        content_top,
                        panel_rect.width - 2.0 * PANE_PADDING,
                        (panel_rect.height - PANE_PADDING - PANEL_TAB_HEIGHT - PANE_GAP - PANE_PADDING).max(1.0),
                    );
                    pane.render_grid(inner, renderer);
                    self.pane_generations.insert(active_id, pane.generation());
                }
            }
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
                Some(PaneKind::Terminal(pane)) => pane.render_cursor(inner, renderer),
                Some(PaneKind::Editor(pane)) => pane.render_cursor(inner, renderer),
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
                pane.render_cursor(inner, renderer);
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
        if let PaneDragState::Dragging { drop_target: Some(ref dest), .. } = &self.pane_drag {
            match dest {
                DropDestination::TreePane(target_id, zone) => {
                    if let Some(&(_, target_rect)) = visual_pane_rects.iter().find(|(id, _)| *id == *target_id) {
                        let preview = compute_preview_rect(target_rect, *zone);
                        Self::draw_drop_preview_border(renderer, preview);
                    }
                }
                DropDestination::EditorPanel => {
                    if let Some(panel_rect) = editor_panel_rect {
                        Self::draw_drop_preview_border(renderer, panel_rect);
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

    fn draw_drop_preview_border(renderer: &mut tide_renderer::WgpuRenderer, preview: Rect) {
        renderer.draw_rect(preview, DROP_PREVIEW_FILL);
        let bw = DROP_PREVIEW_BORDER_WIDTH;
        renderer.draw_rect(Rect::new(preview.x, preview.y, preview.width, bw), DROP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x, preview.y + preview.height - bw, preview.width, bw), DROP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x, preview.y, bw, preview.height), DROP_PREVIEW_BORDER);
        renderer.draw_rect(Rect::new(preview.x + preview.width - bw, preview.y, bw, preview.height), DROP_PREVIEW_BORDER);
    }
}
