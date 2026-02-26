// Platform event dispatch and app thread main loop.

use std::time::{Duration, Instant};

use tide_core::TerminalBackend;
use tide_platform::{PlatformEvent, PlatformWindow, WindowProxy};

use crate::pane::PaneKind;
use crate::session;
use crate::theme::*;
use crate::ui_state::FocusArea;
use crate::App;

/// Events delivered to the app thread.
pub(crate) enum AppEvent {
    /// A platform event forwarded from the main thread.
    Platform(PlatformEvent),
    /// Wake signal from a background thread (PTY output, file watcher, etc.).
    Wake,
}

impl App {
    // ── Phase 1: one-time initialization on the main thread ──────────

    /// Perform one-time initialization that requires the real window handle
    /// (GPU surface creation, PTY pre-spawn, session restore).
    /// Called on the main thread before the app thread is spawned.
    pub(crate) fn init_phase1(&mut self, window: &dyn PlatformWindow) {
        self.content_view_ptr = window.content_view_ptr();
        self.window_ptr = window.window_ptr();

        let saved_session = session::load_session();
        let is_crash = session::is_crash_recovery();

        // Pre-spawn PTY with estimated dimensions (80x24) BEFORE GPU init.
        // The shell starts loading ~/.zshrc in parallel with GPU initialization,
        // so the prompt appears sooner after launch.
        let early_terminal =
            tide_terminal::Terminal::with_cwd(80, 24, None, self.dark_mode).ok();

        self.init_gpu(window); // Shell is loading in parallel

        if is_crash {
            if let Some(session) = saved_session {
                if !self.restore_from_session(session) {
                    self.create_initial_pane(early_terminal);
                }
                // Crash recovery succeeded: early_terminal dropped (kills extra shell)
            } else {
                self.create_initial_pane(early_terminal);
            }
        } else if let Some(ref session) = saved_session {
            self.restore_preferences(session, early_terminal);
        } else {
            self.create_initial_pane(early_terminal);
        }

        session::create_running_marker();
    }

    // ── Phase 2: app thread main loop ────────────────────────────────

    /// Run the app thread event loop.  Blocks on the event channel, processes
    /// events, polls background sources, and renders when needed.
    pub(crate) fn app_thread_run(
        mut self,
        event_rx: std::sync::mpsc::Receiver<AppEvent>,
        window: WindowProxy,
    ) {
        loop {
            let timeout = self.next_timeout();

            // Block until an event arrives or a timer fires
            let event = match event_rx.recv_timeout(timeout) {
                Ok(e) => Some(e),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => None,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            };

            // Process the received event and drain the queue
            for app_event in event.into_iter().chain(event_rx.try_iter()) {
                if let AppEvent::Platform(event) = app_event {
                    self.handle_platform_event(event, &window);
                }
            }

            // Poll background sources (PTY output, file watcher, git)
            self.poll_background_events(&window);

            // Cursor blink
            let blink_elapsed = Instant::now().duration_since(self.cursor_blink_at);
            let blink_phase = (blink_elapsed.as_millis() / 530) % 2 == 0;
            if blink_phase != self.cursor_visible {
                self.cursor_visible = blink_phase;
                self.needs_redraw = true;
            }

            // Render if needed
            if self.needs_redraw && !self.is_occluded && self.batch_depth == 0 {
                let now = Instant::now();
                let skip_coalesce = self.input_just_sent
                    || self.input_sent_at.map_or(false, |at| {
                        now.duration_since(at) < Duration::from_millis(16)
                    });
                if skip_coalesce
                    || now.duration_since(self.last_frame) >= Duration::from_millis(2)
                {
                    self.update();
                    if self.render() {
                        self.needs_redraw = false;
                        self.last_frame = now;

                        // Reveal window after first frame
                        if !self.window_shown {
                            window.show_window();
                            // Re-establish first responder: macOS may reset
                            // it during window lifecycle initialization
                            // (delegate is set after makeKeyAndOrderFront,
                            // so the initial Focused event is missed).
                            if let Some(target) = self.effective_ime_target() {
                                window.focus_ime_proxy(target);
                            }
                            self.window_shown = true;
                        }
                    }
                    // If render() returned false (render thread busy),
                    // the render thread waker will wake us when it finishes.
                }
            }
        }
    }

    /// Compute the timeout for the next `recv_timeout` call.
    fn next_timeout(&self) -> Duration {
        let now = Instant::now();
        let mut timeout = Duration::from_millis(100); // default max sleep

        // Cursor blink: next toggle
        if self.focused.is_some() {
            let blink_elapsed = now.duration_since(self.cursor_blink_at);
            let next_toggle_ms = 530 - (blink_elapsed.as_millis() % 530) as u64;
            timeout = timeout.min(Duration::from_millis(next_toggle_ms));
        }

        // Deferred resize
        if let Some(at) = self.resize_deferred_at {
            if at > now {
                timeout = timeout.min(at - now);
            } else {
                return Duration::ZERO;
            }
        }

        // Badge check
        if let Some(at) = self.badge_check_at {
            if at > now {
                timeout = timeout.min(at - now);
            } else {
                return Duration::ZERO;
            }
        }

        // Frame pacing: if we need to render but are within 2ms coalescing window
        if self.needs_redraw && !self.is_occluded && self.batch_depth == 0 {
            let skip_coalesce = self.input_just_sent
                || self.input_sent_at.map_or(false, |at| {
                    now.duration_since(at) < Duration::from_millis(16)
                });
            if skip_coalesce {
                return Duration::ZERO; // render immediately
            }
            let since_last = now.duration_since(self.last_frame);
            if since_last < Duration::from_millis(2) {
                timeout = timeout.min(Duration::from_millis(2) - since_last);
            } else {
                return Duration::ZERO; // past coalescing window, render now
            }
        }

        timeout
    }

    // ── Event handler (runs on app thread) ───────────────────────────

    /// Process a single platform event.  Called from the app thread loop.
    pub(crate) fn handle_platform_event(
        &mut self,
        event: PlatformEvent,
        window: &WindowProxy,
    ) {
        match event {
            PlatformEvent::BatchStart => {
                self.batch_depth += 1;
                return;
            }
            PlatformEvent::BatchEnd => {
                self.batch_depth = self.batch_depth.saturating_sub(1);
                // Fall through to IME sync below
            }
            PlatformEvent::RedrawRequested => {
                // Rendering is handled by the app thread loop, not here.
                // RedrawRequested from the main thread is just a wake signal.
                self.needs_redraw = true;
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
                self.resize_deferred_at =
                    Some(Instant::now() + Duration::from_millis(50));
                self.compute_layout();
                self.ime_cursor_dirty = true;
                self.needs_redraw = true;
            }
            PlatformEvent::ScaleFactorChanged(scale) => {
                self.scale_factor = scale as f32;
                self.reconfigure_surface();
                self.compute_layout();
                self.chrome_generation += 1;
                self.needs_redraw = true;
            }
            PlatformEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers;
            }
            PlatformEvent::Focused(focused) => {
                if focused {
                    self.modifiers = tide_core::Modifiers::default();
                    self.sync_ime_proxies(window);
                }
            }
            PlatformEvent::Fullscreen {
                is_fullscreen,
                width,
                height,
            } => {
                self.is_fullscreen = is_fullscreen;
                self.top_inset = if is_fullscreen { 0.0 } else { TITLEBAR_HEIGHT };
                self.resize_deferred_at = None;

                // Use the size included in the event (avoids querying window
                // from the app thread, which would require a cross-thread call).
                if (width, height) != self.window_size {
                    self.window_size = (width, height);
                    self.reconfigure_surface();
                }

                self.compute_layout();
                self.ime_cursor_dirty = true;
                self.chrome_generation += 1;
                self.needs_redraw = true;
            }
            PlatformEvent::Occluded(occluded) => {
                self.is_occluded = occluded;
                if !occluded {
                    self.needs_redraw = true;
                }
            }
            PlatformEvent::WebViewFocused => {
                self.focus_area = FocusArea::EditorDock;
                self.chrome_generation += 1;
                self.needs_redraw = true;
            }
            PlatformEvent::ImeCommit(text) => {
                self.handle_ime_commit(&text);
                self.ime_cursor_dirty = true;
                self.cursor_blink_at = Instant::now();
                self.cursor_visible = true;
            }
            PlatformEvent::ImePreedit { text, cursor: _ } => {
                self.handle_ime_preedit(&text);
                self.ime_cursor_dirty = true;
                self.cursor_blink_at = Instant::now();
                self.cursor_visible = true;
            }
            PlatformEvent::KeyDown {
                key,
                modifiers,
                chars,
            } => {
                self.handle_key_down(key, modifiers, chars);
                self.ime_cursor_dirty = true;
                self.cursor_blink_at = Instant::now();
                self.cursor_visible = true;
            }
            PlatformEvent::KeyUp { .. } => {}
            PlatformEvent::MouseDown { button, position } => {
                let pos = self.physical_to_logical(position);
                self.last_cursor_pos = pos;
                let btn = platform_button_to_core(button);
                if let Some(btn) = btn {
                    self.handle_mouse_down(btn, window);
                }
                self.ime_cursor_dirty = true;
                self.cursor_blink_at = Instant::now();
                self.cursor_visible = true;
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
            PlatformEvent::Scroll {
                dx,
                dy,
                position,
            } => {
                let pos = self.physical_to_logical(position);
                self.last_cursor_pos = pos;
                self.handle_scroll(dx, dy);
            }
        }

        // Process deferred fullscreen toggle
        if self.pending_fullscreen_toggle {
            self.pending_fullscreen_toggle = false;
            window.set_fullscreen(!self.is_fullscreen);
        }

        // Sync IME proxy views
        self.sync_ime_proxies(window);
    }

    /// Process pending IME proxy view operations and focus the correct proxy.
    ///
    /// Always calls `focus_ime_proxy` at the end, even when the target hasn't
    /// changed. macOS may unpredictably reset the first responder during event
    /// processing, so we must re-establish it unconditionally.
    pub(crate) fn sync_ime_proxies(&mut self, window: &WindowProxy) {
        for id in self.pending_ime_proxy_creates.drain(..) {
            window.create_ime_proxy(id);
        }
        for id in self.pending_ime_proxy_removes.drain(..) {
            window.remove_ime_proxy(id);
        }

        let target = self.effective_ime_target();
        if target != self.last_ime_target {
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

    /// Commit text directly to a specific pane.
    fn commit_text_to_pane(&mut self, pane_id: tide_core::PaneId, text: &str) {
        use crate::pane::PaneKind;
        // Compute visible size before mutable borrow of panes
        let editor_size = self.visible_editor_size(pane_id);
        match self.panes.get_mut(&pane_id) {
            Some(PaneKind::Terminal(pane)) => {
                pane.backend.write(text.as_bytes());
            }
            Some(PaneKind::Editor(pane)) => {
                if !pane.preview_mode {
                    pane.delete_selection();
                    pane.selection = None;
                    for ch in text.chars() {
                        let action = match ch {
                            ch if ch.is_control() => continue,
                            ch => tide_editor::EditorActionKind::InsertChar(ch),
                        };
                        pane.editor.handle_action(action);
                    }
                    // Ensure cursor stays visible after editing
                    let (visible_rows, visible_cols) = editor_size;
                    pane.editor.ensure_cursor_visible(visible_rows);
                    pane.editor.ensure_cursor_visible_h(visible_cols);
                    self.pane_generations.remove(&pane_id);
                }
            }
            _ => {}
        }
    }

    /// The effective pane that will receive IME input, considering focus area.
    pub(crate) fn effective_ime_target(&self) -> Option<tide_core::PaneId> {
        use crate::ui_state::FocusArea;
        let target = if self.focus_area == FocusArea::EditorDock {
            self.active_editor_tab().or(self.focused)
        } else {
            self.focused
        };
        if let Some(id) = target {
            if let Some(PaneKind::Browser(bp)) = self.panes.get(&id) {
                if !bp.url_input_focused {
                    return None;
                }
            }
        }
        target
    }

    fn physical_to_logical(&self, pos: (f64, f64)) -> tide_core::Vec2 {
        tide_core::Vec2::new(pos.0 as f32, pos.1 as f32)
    }

    /// Poll background events (PTY output, file watcher, git).
    pub(crate) fn poll_background_events(&mut self, window: &WindowProxy) {
        self.poll_render_result();

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

        // Update IME cursor area
        self.update_ime_cursor_area(window);
    }

    /// Update the IME cursor area on the proxy view.
    fn update_ime_cursor_area(&mut self, window: &WindowProxy) {
        if !self.ime_cursor_dirty {
            return;
        }
        self.ime_cursor_dirty = false;
        use crate::ui_state::FocusArea;
        use tide_core::TerminalBackend;

        let cell_size = self.cell_size();

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
                if let Some((_, rect)) = self
                    .visual_pane_rects
                    .iter()
                    .find(|(id, _)| *id == target_id)
                {
                    let cursor = pane.backend.cursor();
                    let inner_w = rect.width - 2.0 * crate::theme::PANE_PADDING;
                    let max_cols = (inner_w / cell_size.width).floor() as usize;
                    let actual_w = max_cols as f32 * cell_size.width;
                    let center_x = (inner_w - actual_w) / 2.0;
                    let top = self.pane_area_mode.content_top();
                    let cx = rect.x
                        + crate::theme::PANE_PADDING
                        + center_x
                        + cursor.col as f32 * cell_size.width;
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
                let cursor_char_col =
                    if let Some(line_text) = pane.editor.buffer.line(pos.line) {
                        let byte_col = pos.col.min(line_text.len());
                        line_text[..byte_col].chars().count()
                    } else {
                        0
                    };
                if cursor_char_col < h_scroll {
                    return;
                }
                let visual_col = cursor_char_col - h_scroll;
                let gutter_cells = crate::editor_pane::GUTTER_WIDTH_CELLS;

                let (inner_x, inner_y) = if let Some((_, rect)) = self
                    .visual_pane_rects
                    .iter()
                    .find(|(id, _)| *id == target_id)
                {
                    let top = self.pane_area_mode.content_top();
                    (rect.x + crate::theme::PANE_PADDING, rect.y + top)
                } else if let Some(panel_rect) = self.editor_panel_rect {
                    let content_top = panel_rect.y
                        + crate::theme::PANE_PADDING
                        + crate::theme::PANEL_TAB_HEIGHT
                        + crate::theme::PANE_GAP;
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
