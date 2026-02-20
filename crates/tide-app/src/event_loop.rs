// Platform event dispatch — replaces winit's ApplicationHandler.

use std::time::{Duration, Instant};

use tide_core::TerminalBackend;
use tide_platform::{PlatformEvent, PlatformWindow};

use crate::pane::PaneKind;
use crate::session;
use crate::theme::*;
use crate::App;

impl App {
    /// Main entry point for all platform events.
    /// Called by the native run loop for every key, mouse, IME, resize, etc.
    pub(crate) fn handle_platform_event(
        &mut self,
        event: PlatformEvent,
        window: &dyn PlatformWindow,
    ) {
        // One-time initialization on first event
        if self.surface.is_none() {
            self.init_gpu(window);

            // Capture platform pointers for webview management
            self.content_view_ptr = window.content_view_ptr();
            self.window_ptr = window.window_ptr();

            let saved_session = session::load_session();
            let is_crash = session::is_crash_recovery();
            let restored = match saved_session {
                Some(session) if is_crash => self.restore_from_session(session),
                Some(ref session) => {
                    self.restore_preferences(session);
                    true
                }
                None => false,
            };
            if !restored {
                self.create_initial_pane();
            }
            session::create_running_marker();
            self.compute_layout();
        }

        // Track the effective target pane before event processing so we can
        // discard IME composition if focus moves to a different pane.
        let target_before = self.effective_ime_target();

        match event {
            PlatformEvent::RedrawRequested => {
                self.update();
                self.render();
                self.needs_redraw = false;
                self.last_frame = Instant::now();
                return;
            }
            PlatformEvent::CloseRequested => {
                let session = session::Session::from_app(self);
                session::save_session(&session);
                session::delete_running_marker();
                std::process::exit(0);
            }
            PlatformEvent::Resized { width, height } => {
                self.window_size = (width, height);
                self.reconfigure_surface();
                self.resize_deferred_at = Some(Instant::now() + Duration::from_millis(100));
                self.compute_layout();
            }
            PlatformEvent::ScaleFactorChanged(scale) => {
                self.scale_factor = scale as f32;
            }
            PlatformEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }
            PlatformEvent::Focused(focused) => {
                if focused {
                    self.modifiers = tide_core::Modifiers::default();
                    self.reset_ime_state();
                    window.discard_marked_text();
                }
            }
            PlatformEvent::Fullscreen(fs) => {
                self.is_fullscreen = fs;
                self.top_inset = if fs { 0.0 } else { TITLEBAR_HEIGHT };
                self.compute_layout();
            }
            PlatformEvent::ImeCommit(text) => {
                self.handle_ime_commit(&text);
            }
            PlatformEvent::ImePreedit { text, cursor: _ } => {
                self.handle_ime_preedit(&text);
            }
            PlatformEvent::KeyDown { key, modifiers, chars } => {
                self.handle_key_down(key, modifiers, chars);
            }
            PlatformEvent::KeyUp { .. } => {
                // Native IME handles all text routing via ImeCommit/ImePreedit,
                // so we don't need to process KeyUp events for text input.
            }
            PlatformEvent::MouseDown { button, position } => {
                let pos = self.physical_to_logical(position);
                self.last_cursor_pos = pos;
                let btn = platform_button_to_core(button);
                if let Some(btn) = btn {
                    self.handle_mouse_down(btn, window);
                }
            }
            PlatformEvent::MouseUp { button, position } => {
                let pos = self.physical_to_logical(position);
                self.last_cursor_pos = pos;
                let btn = platform_button_to_core(button);
                if let Some(btn) = btn {
                    self.handle_mouse_up(btn);
                }
            }
            PlatformEvent::MouseMoved { position } => {
                let pos = self.physical_to_logical(position);
                self.handle_cursor_moved_logical(pos, window);
            }
            PlatformEvent::Scroll { dx, dy, position } => {
                let pos = self.physical_to_logical(position);
                self.last_cursor_pos = pos;
                self.handle_scroll(dx, dy);
            }
        }

        // If the effective target pane changed, commit any in-progress composition
        // to the OLD target, then clear IME state so it doesn't carry over.
        let target_after = self.effective_ime_target();
        if target_before != target_after && self.ime_composing {
            if let Some(old_id) = target_before {
                if !self.ime_preedit.is_empty() {
                    let preedit = self.ime_preedit.clone();
                    match self.panes.get_mut(&old_id) {
                        Some(PaneKind::Terminal(pane)) => {
                            pane.backend.write(preedit.as_bytes());
                        }
                        Some(PaneKind::Editor(pane)) => {
                            for ch in preedit.chars() {
                                pane.editor
                                    .handle_action(tide_editor::EditorActionKind::InsertChar(ch));
                            }
                        }
                        _ => {}
                    }
                }
            }
            self.reset_ime_state();
            window.discard_marked_text();
        }

        // Frame pacing: check if we need to redraw
        self.poll_background_events(window);

        // Render directly at the end of event handling.
        // This avoids deferred selector / re-entrancy issues with CAMetalLayer views.
        if self.needs_redraw {
            self.update();
            self.render();
            self.needs_redraw = false;
            self.last_frame = Instant::now();
        }
    }

    /// The effective pane that will receive IME input, considering focus area.
    fn effective_ime_target(&self) -> Option<tide_core::PaneId> {
        use crate::ui_state::FocusArea;
        if self.focus_area == FocusArea::EditorDock {
            self.active_editor_tab().or(self.focused)
        } else {
            self.focused
        }
    }

    /// Convert logical position (from NSView, already in view coords) to our logical coords
    fn physical_to_logical(&self, pos: (f64, f64)) -> tide_core::Vec2 {
        // NSView positions are already in logical (point) coordinates when isFlipped
        tide_core::Vec2::new(pos.0 as f32, pos.1 as f32)
    }

    /// Poll background events (PTY output, file watcher, git poller) and manage frame pacing.
    pub(crate) fn poll_background_events(&mut self, window: &dyn PlatformWindow) {
        // Deferred PTY resize
        if let Some(at) = self.resize_deferred_at {
            if Instant::now() >= at {
                self.resize_deferred_at = None;
                self.compute_layout();
                self.needs_redraw = true;
            }
        }

        // Check PTY output
        let mut had_pty_output = false;
        for pane in self.panes.values() {
            if let PaneKind::Terminal(terminal) = pane {
                if terminal.backend.has_new_output() {
                    self.needs_redraw = true;
                    self.input_just_sent = false;
                    self.input_sent_at = None;
                    had_pty_output = true;
                    break;
                }
            }
        }

        if had_pty_output {
            self.badge_check_at = Some(Instant::now() + Duration::from_millis(150));
        }

        // File watcher
        if self
            .file_watch_dirty
            .swap(false, std::sync::atomic::Ordering::Relaxed)
        {
            self.needs_redraw = true;
        }

        // Git poller
        if self.consume_git_poll_results() {
            self.chrome_generation += 1;
            self.needs_redraw = true;
        }

        // Badge check
        if let Some(check_at) = self.badge_check_at {
            if Instant::now() >= check_at {
                self.badge_check_at = None;
                self.update_file_tree_cwd();
                self.update_terminal_badges();

                if let Some(ref tx) = self.git_poll_cwd_tx {
                    let cwds: std::collections::HashSet<std::path::PathBuf> = self
                        .panes
                        .values()
                        .filter_map(|pane| {
                            if let PaneKind::Terminal(p) = pane {
                                p.cwd.clone()
                            } else {
                                None
                            }
                        })
                        .collect();
                    let _ = tx.send(cwds.into_iter().collect());
                }
            }
        }

        // Update IME cursor area so the candidate window follows the text cursor
        self.update_ime_cursor_area(window);
    }

    /// Update the IME cursor area on the native window so the candidate window
    /// appears next to the text cursor.
    fn update_ime_cursor_area(&self, window: &dyn PlatformWindow) {
        use crate::ui_state::FocusArea;
        use tide_core::{Renderer, TerminalBackend};

        let renderer = match self.renderer.as_ref() {
            Some(r) => r,
            None => return,
        };
        let cell_size = renderer.cell_size();

        // Determine the effective target pane
        let target_id = if self.focus_area == FocusArea::EditorDock {
            self.active_editor_tab().or(self.focused)
        } else {
            self.focused
        };
        let target_id = match target_id {
            Some(id) => id,
            None => return,
        };

        match self.panes.get(&target_id) {
            Some(PaneKind::Terminal(pane)) => {
                // Find the visual rect for this pane
                if let Some((_, rect)) = self.visual_pane_rects.iter().find(|(id, _)| *id == target_id) {
                    let cursor = pane.backend.cursor();
                    let inner_w = rect.width - 2.0 * crate::theme::PANE_PADDING;
                    let max_cols = (inner_w / cell_size.width).floor() as usize;
                    let actual_w = max_cols as f32 * cell_size.width;
                    let center_x = (inner_w - actual_w) / 2.0;
                    let top = self.pane_area_mode.content_top();
                    let cx = rect.x + crate::theme::PANE_PADDING + center_x + cursor.col as f32 * cell_size.width;
                    let cy = rect.y + top + cursor.row as f32 * cell_size.height;
                    window.set_ime_cursor_area(
                        cx as f64,
                        cy as f64,
                        cell_size.width as f64,
                        cell_size.height as f64,
                    );
                }
            }
            Some(PaneKind::Editor(pane)) => {
                let pos = pane.editor.cursor_position();
                let scroll = pane.editor.scroll_offset();
                let h_scroll = pane.editor.h_scroll_offset();
                if pos.line < scroll {
                    return;
                }
                let visual_row = pos.line - scroll;
                let cursor_char_col = if let Some(line_text) = pane.editor.buffer.line(pos.line) {
                    let byte_col = pos.col.min(line_text.len());
                    line_text[..byte_col].chars().count()
                } else {
                    0
                };
                if cursor_char_col < h_scroll {
                    return;
                }
                let visual_col = cursor_char_col - h_scroll;
                let gutter_cells = 5usize;

                // Check visual rect first (tree editor), then panel rect
                let (inner_x, inner_y) = if let Some((_, rect)) = self.visual_pane_rects.iter().find(|(id, _)| *id == target_id) {
                    let top = self.pane_area_mode.content_top();
                    (rect.x + crate::theme::PANE_PADDING, rect.y + top)
                } else if let Some(panel_rect) = self.editor_panel_rect {
                    let content_top = panel_rect.y + crate::theme::PANE_PADDING + crate::theme::PANEL_TAB_HEIGHT + crate::theme::PANE_GAP;
                    (panel_rect.x + crate::theme::PANE_PADDING, content_top)
                } else {
                    return;
                };

                let gutter_width = gutter_cells as f32 * cell_size.width;
                let cx = inner_x + gutter_width + visual_col as f32 * cell_size.width;
                let cy = inner_y + visual_row as f32 * cell_size.height;
                window.set_ime_cursor_area(
                    cx as f64,
                    cy as f64,
                    cell_size.width as f64,
                    cell_size.height as f64,
                );
            }
            _ => {}
        }
    }
}

fn platform_button_to_core(
    button: tide_platform::MouseButton,
) -> Option<tide_core::MouseButton> {
    match button {
        tide_platform::MouseButton::Left => Some(tide_core::MouseButton::Left),
        tide_platform::MouseButton::Right => Some(tide_core::MouseButton::Right),
        tide_platform::MouseButton::Middle => Some(tide_core::MouseButton::Middle),
        _ => None,
    }
}
