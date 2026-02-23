// Platform event dispatch.

use std::time::{Duration, Instant};

use tide_core::TerminalBackend;
use tide_platform::{PlatformEvent, PlatformWindow};

use crate::pane::PaneKind;
use crate::session;
use crate::theme::*;
use crate::ui_state::FocusArea;
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
            // Create IME proxy views for all panes created during init
            self.sync_ime_proxies(window, false);
            self.compute_layout();
        }

        // Determine if this is an input event for low-latency frame pacing (4ms vs 16ms)
        let is_input_event = matches!(
            event,
            PlatformEvent::KeyDown { .. }
                | PlatformEvent::ImeCommit(_)
                | PlatformEvent::ImePreedit { .. }
        );
        let input_event_start = if is_input_event {
            Some(Instant::now())
        } else {
            None
        };

        // Keyboard/IME/modifier events should NOT trigger redundant
        // makeFirstResponder calls — the resign/become cycle resets
        // NSTextInputContext, which breaks Korean IME composition on the
        // first character after an input method switch.
        // Only mouse, resize, and focus events may need to re-establish
        // the first responder (e.g. after clicking a WKWebView).
        let skip_ime_refocus = matches!(
            event,
            PlatformEvent::KeyDown { .. }
                | PlatformEvent::KeyUp { .. }
                | PlatformEvent::ImeCommit(_)
                | PlatformEvent::ImePreedit { .. }
                | PlatformEvent::ModifiersChanged(_)
        );

        match event {
            PlatformEvent::RedrawRequested => {
                if self.is_occluded { return; }

                // Poll background events (PTY output, file watcher, git) so that
                // waker-triggered redraws detect new terminal output and set
                // needs_redraw.  Without this, PTY output that arrives after the
                // previous render is missed and the character only appears on the
                // *next* keystroke ("one-beat delay").
                self.poll_background_events(window);

                if self.needs_redraw {
                    self.update();
                    self.render();
                    self.needs_redraw = false;
                    self.last_frame = Instant::now();

                    // Reveal window after first frame so the user never sees a blank window
                    if !self.window_shown {
                        window.show_window();
                        self.window_shown = true;
                    }
                }
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
                self.ime_cursor_dirty = true;
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
                    // Re-establish first responder for the current focused pane
                    self.sync_ime_proxies(window, false);
                }
            }
            PlatformEvent::Fullscreen(fs) => {
                self.is_fullscreen = fs;
                self.top_inset = if fs { 0.0 } else { TITLEBAR_HEIGHT };
                self.compute_layout();
                self.ime_cursor_dirty = true;
            }
            PlatformEvent::Occluded(occluded) => {
                self.is_occluded = occluded;
                if !occluded {
                    self.needs_redraw = true;
                }
            }
            PlatformEvent::WebViewFocused => {
                // WKWebView has first responder — set focus to EditorDock
                // so shortcuts like Cmd+W close the browser tab, not a terminal.
                self.focus_area = FocusArea::EditorDock;
                self.chrome_generation += 1;
                self.needs_redraw = true;
            }
            PlatformEvent::ImeCommit(text) => {
                self.handle_ime_commit(&text);
                self.ime_cursor_dirty = true;
            }
            PlatformEvent::ImePreedit { text, cursor: _ } => {
                self.handle_ime_preedit(&text);
                self.ime_cursor_dirty = true;
            }
            PlatformEvent::KeyDown { key, modifiers, chars } => {
                self.handle_key_down(key, modifiers, chars);
                self.ime_cursor_dirty = true;
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
                self.ime_cursor_dirty = true;
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

        // Process deferred fullscreen toggle (action handler has no window access)
        if self.pending_fullscreen_toggle {
            self.pending_fullscreen_toggle = false;
            window.set_fullscreen(!self.is_fullscreen);
        }

        // Sync IME proxy views: create/remove proxies and focus the right one.
        // Proxy view first-responder transitions automatically call unmarkText,
        // which clears any in-progress Korean IME composition.
        self.sync_ime_proxies(window, skip_ime_refocus);

        // Frame pacing: check if we need to redraw
        self.poll_background_events(window);

        // Frame-paced rendering: input events use a shorter interval (4ms / 250fps)
        // for responsive visual feedback; other events use the default 16ms / ~60fps cap.
        // Otherwise defer via a 0-delay timer so rapid bursts are coalesced.
        if self.needs_redraw && !self.is_occluded {
            let now = Instant::now();
            let min_interval = if is_input_event {
                Duration::from_millis(4)
            } else {
                Duration::from_millis(16)
            };
            if now.duration_since(self.last_frame) >= min_interval {
                self.update();
                self.render();
                self.needs_redraw = false;
                self.last_frame = now;

                if let Some(start) = input_event_start {
                    log::trace!("input->render: {}us", start.elapsed().as_micros());
                }

                if !self.window_shown {
                    window.show_window();
                    self.window_shown = true;
                }
            } else {
                if let Some(start) = input_event_start {
                    let since_last = now.duration_since(self.last_frame).as_micros();
                    log::trace!(
                        "input deferred ({}us since last frame, min={}ms)",
                        since_last,
                        min_interval.as_millis()
                    );
                    let _ = start; // suppress unused warning
                }
                window.request_redraw();
            }
        }
    }

    /// Process pending IME proxy view operations and focus the correct proxy.
    /// `skip_refocus`: when true, skip redundant `makeFirstResponder` calls
    /// if the IME target hasn't changed.  Keyboard/IME/modifier events set
    /// this to avoid the resign→become cycle that resets NSTextInputContext
    /// and breaks Korean IME composition after an input method switch.
    fn sync_ime_proxies(&mut self, window: &dyn PlatformWindow, skip_refocus: bool) {
        // Early return when there's nothing to do at all
        if self.pending_ime_proxy_creates.is_empty()
            && self.pending_ime_proxy_removes.is_empty()
        {
            let target = self.effective_ime_target();
            if target == self.last_ime_target {
                // Re-focus the proxy only for mouse/focus events —
                // macOS may have changed first responder (e.g. clicking WKWebView).
                // Skip for key/IME events to preserve NSTextInputContext state.
                if !skip_refocus {
                    if let Some(target) = target {
                        window.focus_ime_proxy(target);
                    }
                }
                return;
            }
        }

        for id in self.pending_ime_proxy_creates.drain(..) {
            window.create_ime_proxy(id);
        }
        for id in self.pending_ime_proxy_removes.drain(..) {
            window.remove_ime_proxy(id);
        }

        // Focus the proxy for the current effective target
        let target = self.effective_ime_target();
        if target != self.last_ime_target {
            // IME target changed — commit preedit to the *old* target pane,
            // then clear app-level IME state.
            if !self.ime_preedit.is_empty() {
                if let Some(old_target) = self.last_ime_target {
                    self.commit_text_to_pane(old_target, &self.ime_preedit.clone());
                }
            }
            self.ime_composing = false;
            self.ime_preedit.clear();
            self.needs_redraw = true;
            self.ime_cursor_dirty = true;
            self.last_ime_target = target;
        }
        if let Some(target) = target {
            window.focus_ime_proxy(target);
        }
    }

    /// Commit text directly to a specific pane (terminal write or editor insert).
    fn commit_text_to_pane(&mut self, pane_id: tide_core::PaneId, text: &str) {
        use crate::pane::PaneKind;
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                pane.backend.write(text.as_bytes());
            }
            Some(PaneKind::Editor(pane)) => {
                if !pane.preview_mode {
                    for ch in text.chars() {
                        let action = match ch {
                            ch if ch.is_control() => continue,
                            ch => tide_editor::EditorActionKind::InsertChar(ch),
                        };
                        pane.editor.handle_action(action);
                    }
                    self.pane_generations.remove(&pane_id);
                }
            }
            _ => {}
        }
    }

    /// The effective pane that will receive IME input, considering focus area.
    ///
    /// Returns `None` when the active browser tab's URL bar is NOT focused,
    /// so the WKWebView retains first responder and receives keyboard input
    /// for web content directly.
    pub(crate) fn effective_ime_target(&self) -> Option<tide_core::PaneId> {
        use crate::ui_state::FocusArea;
        let target = if self.focus_area == FocusArea::EditorDock {
            self.active_editor_tab().or(self.focused)
        } else {
            self.focused
        };
        // When a browser pane is the target but its URL bar is not focused,
        // return None so sync_ime_proxies won't steal first responder from WKWebView.
        if let Some(id) = target {
            if let Some(PaneKind::Browser(bp)) = self.panes.get(&id) {
                if !bp.url_input_focused {
                    return None;
                }
            }
        }
        target
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
                    self.ime_cursor_dirty = true;
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

    /// Update the IME cursor area on the proxy view so the candidate window
    /// appears next to the text cursor. Only recomputes when `ime_cursor_dirty` is set.
    fn update_ime_cursor_area(&mut self, window: &dyn PlatformWindow) {
        if !self.ime_cursor_dirty {
            return;
        }
        self.ime_cursor_dirty = false;
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
                    window.set_ime_proxy_cursor_area(
                        target_id,
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
                window.set_ime_proxy_cursor_area(
                    target_id,
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
