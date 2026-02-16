// ApplicationHandler implementation extracted from main.rs

use std::path::PathBuf;
use std::time::{Duration, Instant};

use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, MouseButton as WinitMouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{WindowAttributes, WindowId};

use std::sync::Arc;

use tide_core::{FileTreeSource, Renderer};

use crate::drag_drop::PaneDragState;
use crate::pane::PaneKind;
use crate::session;
use crate::theme::*;
use crate::App;

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        // Try loading a saved session to restore window size
        let saved_session = session::load_session();
        let (win_w, win_h) = saved_session
            .as_ref()
            .map(|s| (s.window_width as f64, s.window_height as f64))
            .unwrap_or((960.0, 640.0));

        let attrs = WindowAttributes::default()
            .with_title("Tide")
            .with_inner_size(LogicalSize::new(win_w, win_h))
            .with_min_inner_size(LogicalSize::new(400.0, 300.0));

        // macOS: transparent titlebar with content extending beneath it
        #[cfg(target_os = "macos")]
        let attrs = {
            use winit::platform::macos::WindowAttributesExtMacOS;
            attrs
                .with_titlebar_transparent(true)
                .with_fullsize_content_view(true)
                .with_title_hidden(true)
        };

        let window = Arc::new(event_loop.create_window(attrs).expect("create window"));
        window.set_ime_allowed(true);

        self.window = Some(window);
        self.init_gpu();

        // Crash recovery: if the running marker exists, the previous session
        // ended abnormally → restore everything.  Otherwise only restore prefs.
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

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Handle RedrawRequested directly — must NOT fall through to the
        // unconditional `needs_redraw = true` at the end of this function,
        // otherwise every completed frame immediately requests another frame,
        // creating an infinite render loop that leaks GPU staging memory.
        if matches!(event, WindowEvent::RedrawRequested) {
            self.update();
            self.render();
            self.needs_redraw = false;
            self.last_frame = Instant::now();
            return;
        }

        // Handle search bar clicks before anything else
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.check_search_bar_click() {
                self.needs_redraw = true;
                return;
            }
        }

        // Handle empty panel "New File" / "Open File" button clicks
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.is_on_new_file_button(self.last_cursor_pos) {
                self.new_editor_pane();
                self.needs_redraw = true;
                return;
            }
            if self.is_on_open_file_button(self.last_cursor_pos) {
                self.open_file_finder();
                self.needs_redraw = true;
                return;
            }
        }

        // Handle file finder item click
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            // Context menu: click inside → execute, any left click → dismiss
            if self.context_menu.is_some() {
                if let Some(idx) = self.context_menu_item_at(self.last_cursor_pos) {
                    self.execute_context_menu_action(idx);
                }
                self.context_menu = None;
                self.needs_redraw = true;
                return;
            }

            // Save-as popup: click outside → dismiss, click inside → consume
            if self.save_as_input.is_some() {
                if !self.save_as_contains(self.last_cursor_pos) {
                    self.save_as_input = None;
                }
                self.needs_redraw = true;
                return;
            }

            // File finder: click on item → open, click outside → dismiss
            if self.file_finder.is_some() {
                if let Some(idx) = self.file_finder_item_at(self.last_cursor_pos) {
                    if let Some(ref finder) = self.file_finder {
                        if let Some(&entry_idx) = finder.filtered.get(idx) {
                            let path = finder.base_dir.join(&finder.entries[entry_idx]);
                            self.close_file_finder();
                            self.open_editor_pane(path);
                            self.needs_redraw = true;
                            return;
                        }
                    }
                } else if !self.file_finder_contains(self.last_cursor_pos) {
                    self.close_file_finder();
                }
                self.needs_redraw = true;
                return;
            }

            // Git switcher popup click handling
            if self.git_switcher.is_some() {
                // Check button clicks first (worktree mode)
                if let Some(btn) = self.git_switcher_button_at(self.last_cursor_pos) {
                    self.handle_git_switcher_button(btn);
                    self.needs_redraw = true;
                    return;
                }
                // Check item clicks
                if let Some(idx) = self.git_switcher_item_at(self.last_cursor_pos) {
                    // Both modes: click selects (buttons handle actions)
                    if let Some(ref mut gs) = self.git_switcher {
                        gs.selected = idx;
                        self.chrome_generation += 1;
                    }
                    self.needs_redraw = true;
                    return;
                } else if !self.git_switcher_contains(self.last_cursor_pos) {
                    // Click outside popup → close it
                    self.git_switcher = None;
                    self.needs_redraw = true;
                    return;
                }
            }

            // File switcher popup click handling
            if self.file_switcher.is_some() {
                if let Some(idx) = self.file_switcher_item_at(self.last_cursor_pos) {
                    let selected_pane_id = self.file_switcher.as_ref()
                        .and_then(|fs| {
                            let entry_idx = *fs.filtered.get(idx)?;
                            Some(fs.entries.get(entry_idx)?.pane_id)
                        });
                    self.file_switcher = None;
                    if let Some(pane_id) = selected_pane_id {
                        if let Some(tid) = self.terminal_owning(pane_id) {
                            if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                                tp.active_editor = Some(pane_id);
                            }
                        }
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&pane_id);
                    }
                    self.needs_redraw = true;
                    return;
                } else if !self.file_switcher_contains(self.last_cursor_pos) {
                    self.file_switcher = None;
                    self.needs_redraw = true;
                    return;
                }
            }

        }

        // Handle editor panel clicks before general routing
        // Tab clicks flow through to handle_window_event for drag support.
        // Only intercept: close buttons and content area clicks.
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if let Some(ref panel_rect) = self.editor_panel_rect {
                let near_border = (self.last_cursor_pos.x - panel_rect.x).abs() < 5.0;
                let in_handle_strip = self.last_cursor_pos.y >= panel_rect.y
                    && self.last_cursor_pos.y < panel_rect.y + PANE_PADDING;
                if panel_rect.contains(self.last_cursor_pos) && !near_border && !in_handle_strip {
                    // Tab close button → handle here
                    if let Some(tab_id) = self.panel_tab_close_at(self.last_cursor_pos) {
                        self.close_editor_panel_tab(tab_id);
                        self.needs_redraw = true;
                        return;
                    }
                    // Tab click → cancel save confirm and let flow for drag initiation
                    if self.panel_tab_at(self.last_cursor_pos).is_some() {
                        self.cancel_save_confirm();
                        // fall through
                    } else if self.handle_notification_bar_click(self.last_cursor_pos) {
                        // Conflict bar button was clicked
                        return;
                    } else {
                        // Content area click → focus + cursor + start selection drag
                        self.mouse_left_pressed = true;
                        self.handle_editor_panel_click(self.last_cursor_pos);
                        self.needs_redraw = true;
                        return;
                    }
                }
            }
        }

        // Handle notification bar clicks on left-side panes
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            // Check left-side pane notification bars (not inside panel)
            let in_panel = self.editor_panel_rect.is_some_and(|pr| pr.contains(self.last_cursor_pos));
            if !in_panel && self.handle_notification_bar_click(self.last_cursor_pos) {
                return;
            }
        }

        // Handle header badge clicks (close, git branch, git status)
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.check_header_click() {
                return;
            }
        }

        // Handle pane tab bar close button clicks (fallback for header)
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if let Some(pane_id) = self.pane_tab_close_at(self.last_cursor_pos) {
                self.close_specific_pane(pane_id);
                self.needs_redraw = true;
                return;
            }
        }

        // Handle right-click on file tree → context menu
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Right,
            ..
        } = &event
        {
            if self.show_file_tree {
                if let Some(ft_rect) = self.file_tree_rect {
                    let pos = self.last_cursor_pos;
                    if pos.x >= ft_rect.x && pos.x < ft_rect.x + ft_rect.width && pos.y >= ft_rect.y + PANE_PADDING {
                        // Compute entry index (same math as handle_file_tree_click)
                        if let Some(renderer) = self.renderer.as_ref() {
                            let cell_size = renderer.cell_size();
                            let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
                            let ft_y = ft_rect.y;
                            let adjusted_y = pos.y - ft_y - PANE_PADDING;
                            let index = ((adjusted_y + self.file_tree_scroll) / line_height) as usize;

                            if let Some(tree) = self.file_tree.as_ref() {
                                let entries = tree.visible_entries();
                                if index < entries.len() {
                                    let entry = &entries[index];
                                    // Dismiss any existing context menu or rename
                                    self.context_menu = None;
                                    self.file_tree_rename = None;
                                    self.context_menu = Some(crate::ContextMenuState {
                                        entry_index: index,
                                        path: entry.entry.path.clone(),
                                        is_dir: entry.entry.is_dir,
                                        position: pos,
                                        selected: 0,
                                    });
                                    self.needs_redraw = true;
                                    return;
                                }
                            }
                        }
                    }
                }
            }
        }

        // Handle file tree clicks before general routing
        // (skip the top handle strip so drag-to-move can work)
        if let WindowEvent::MouseInput {
            state: ElementState::Pressed,
            button: WinitMouseButton::Left,
            ..
        } = &event
        {
            if self.show_file_tree {
                if let Some(ft_rect) = self.file_tree_rect {
                    let pos = self.last_cursor_pos;
                    if pos.x >= ft_rect.x && pos.x < ft_rect.x + ft_rect.width && pos.y >= ft_rect.y + PANE_PADDING {
                        self.handle_file_tree_click(pos);
                        return;
                    }
                }
            }
        }

        // Check if this event can skip a redraw (idle cursor hover, modifier change)
        let skip_redraw = match &event {
            WindowEvent::CursorMoved { .. } => {
                !self.mouse_left_pressed
                    && !self.file_tree_border_dragging
                    && !self.panel_border_dragging
                    && !self.sidebar_handle_dragging
                    && !self.dock_handle_dragging
                    && !self.router.is_dragging_border()
                    && matches!(self.pane_drag, PaneDragState::Idle)
            }
            WindowEvent::ModifiersChanged(_) => true,
            _ => false,
        };

        self.handle_window_event(event);

        if !skip_redraw {
            self.needs_redraw = true;
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        // Deferred PTY resize: after window resize events stop, apply final terminal sizes
        if let Some(at) = self.resize_deferred_at {
            if Instant::now() >= at {
                self.resize_deferred_at = None;

                self.compute_layout(); // PTY resize now happens (flag cleared)
                self.needs_redraw = true;
            }
        }

        // Check if any terminal has new PTY output (cheap atomic load)
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

        // PTY just produced output → schedule a deferred badge check.
        // After a cd (by user or AI agent), the shell emits a new prompt.
        // We check CWD/idle 150ms after the last output burst settles.
        if had_pty_output {
            self.badge_check_at = Some(Instant::now() + Duration::from_millis(150));
        }

        // Check if file watcher has pending events
        if self.file_watch_dirty.swap(false, std::sync::atomic::Ordering::Relaxed) {
            self.needs_redraw = true;
        }

        // Consume git poller results (woken via EventLoopProxy::send_event)
        if self.consume_git_poll_results() {
            self.chrome_generation += 1;
            self.needs_redraw = true;
        }

        if self.needs_redraw {
            // Adaptive frame throttling: start at ~120fps, drop to ~30fps during
            // sustained PTY output (e.g. long-running commands) to save battery/GPU.
            let min_frame_time = if self.consecutive_dirty_frames > 60 {
                Duration::from_micros(33_333) // ~30fps after 0.5s of continuous output
            } else {
                Duration::from_micros(8_333) // ~120fps normal
            };
            let elapsed = self.last_frame.elapsed();
            if elapsed < min_frame_time {
                event_loop.set_control_flow(ControlFlow::wait_duration(min_frame_time - elapsed));
                return;
            }
            self.consecutive_dirty_frames += 1;
            if let Some(window) = &self.window {
                window.request_redraw();
            }
        } else if self.input_just_sent {
            // User just typed — reset adaptive throttle so next PTY burst starts at 120fps
            self.consecutive_dirty_frames = 0;
            // Poll aggressively while awaiting PTY response after keypress
            // 50ms safety timeout: stop polling if PTY hasn't responded
            if self.input_sent_at.is_some_and(|t| t.elapsed() > Duration::from_millis(50)) {
                self.input_just_sent = false;
                self.input_sent_at = None;
                event_loop.set_control_flow(ControlFlow::wait_duration(Duration::from_millis(8)));
            } else {
                event_loop.set_control_flow(ControlFlow::Poll);
            }
        } else {
            // Idle — check if a deferred badge update is due
            self.consecutive_dirty_frames = 0;

            if let Some(check_at) = self.badge_check_at {
                let now = Instant::now();
                if now >= check_at {
                    // PTY output settled — run CWD/idle badge check now
                    self.badge_check_at = None;
                    self.update_file_tree_cwd();
                    self.update_terminal_badges();

                    // Send CWDs to git poller so git badges update too
                    if let Some(ref tx) = self.git_poll_cwd_tx {
                        let cwds: std::collections::HashSet<PathBuf> = self.panes.values()
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

                    // If badge changed, request a frame
                    if self.needs_redraw {
                        if let Some(window) = &self.window {
                            window.request_redraw();
                        }
                        return;
                    }
                } else {
                    // Not yet — sleep until the scheduled check time
                    event_loop.set_control_flow(ControlFlow::WaitUntil(check_at));
                    return;
                }
            }

            // Truly idle: sleep until PTY waker or user input wakes us
            event_loop.set_control_flow(ControlFlow::Wait);
        }
    }
}
