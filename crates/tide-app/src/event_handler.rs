use std::time::Instant;

use winit::event::{ElementState, Ime, MouseButton as WinitMouseButton, MouseScrollDelta, WindowEvent};

use tide_core::{InputEvent, LayoutEngine, MouseButton, SplitDirection, TerminalBackend, Vec2};

use crate::drag_drop::{DropDestination, PaneDragState};
use crate::input::{winit_key_to_tide, winit_modifiers_to_tide};
use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;

impl App {
    pub(crate) fn handle_window_event(&mut self, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                std::process::exit(0);
            }
            WindowEvent::Resized(new_size) => {
                self.window_size = new_size;
                self.reconfigure_surface();
                self.compute_layout();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                self.scale_factor = scale_factor as f32;
            }
            WindowEvent::ModifiersChanged(modifiers) => {
                self.modifiers = modifiers.state();
            }
            WindowEvent::Ime(ime) => match ime {
                Ime::Commit(text) => {
                    // IME composed text (Korean, CJK, etc.) → write directly to terminal
                    if let Some(focused_id) = self.focused {
                        if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&focused_id) {
                            pane.backend.write(text.as_bytes());
                            self.input_just_sent = true;
                            self.input_sent_at = Some(Instant::now());
                        }
                    }
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                }
                Ime::Preedit(text, _) => {
                    self.ime_composing = !text.is_empty();
                    self.ime_preedit = text;
                }
                _ => {}
            },
            WindowEvent::KeyboardInput { event, .. } => {
                if event.state != ElementState::Pressed {
                    return;
                }

                // Cancel pane drag on Escape
                if !matches!(self.pane_drag, PaneDragState::Idle) {
                    if event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) {
                        self.pane_drag = PaneDragState::Idle;
                        return;
                    }
                }

                // During IME composition, only handle non-character keys
                if self.ime_composing
                    && matches!(event.logical_key, winit::keyboard::Key::Character(_))
                {
                    return;
                }

                if let Some(key) = winit_key_to_tide(&event.logical_key) {
                    let modifiers = winit_modifiers_to_tide(self.modifiers);
                    let input = InputEvent::KeyPress { key, modifiers };

                    let action = self.router.process(input, &self.pane_rects);
                    self.handle_action(action, Some(input));
                }
            }
            WindowEvent::MouseInput { state, button, .. } => {
                if state != ElementState::Pressed {
                    // Handle pane drag drop on mouse release
                    let drag_state = std::mem::replace(&mut self.pane_drag, PaneDragState::Idle);
                    match drag_state {
                        PaneDragState::Dragging { source_pane, from_panel, drop_target: Some(dest), .. } => {
                            self.handle_drop(source_pane, from_panel, dest);
                            return;
                        }
                        PaneDragState::PendingDrag { source_pane, .. } => {
                            // Click (no drag): just focus the pane
                            if self.focused != Some(source_pane) {
                                self.focused = Some(source_pane);
                                self.router.set_focused(source_pane);
                                self.chrome_generation += 1;
                                self.update_file_tree_cwd();
                            }
                            return;
                        }
                        PaneDragState::Dragging { .. } => {
                            // Drop with no valid target: cancel
                            return;
                        }
                        PaneDragState::Idle => {}
                    }

                    let was_dragging = self.router.is_dragging_border();
                    // End drag on mouse release
                    self.layout.end_drag();
                    self.router.end_drag();
                    // Apply final PTY resize now that drag is over
                    if was_dragging {
                        self.compute_layout();
                    }
                    return;
                }

                let btn = match button {
                    WinitMouseButton::Left => MouseButton::Left,
                    WinitMouseButton::Right => MouseButton::Right,
                    WinitMouseButton::Middle => MouseButton::Middle,
                    _ => return,
                };

                if btn == MouseButton::Left {
                    // Check panel tabs first for drag initiation
                    if let Some(tab_id) = self.panel_tab_at(self.last_cursor_pos) {
                        self.pane_drag = PaneDragState::PendingDrag {
                            source_pane: tab_id,
                            press_pos: self.last_cursor_pos,
                            from_panel: true,
                        };
                        // Activate and focus
                        self.editor_panel_active = Some(tab_id);
                        if self.focused != Some(tab_id) {
                            self.focused = Some(tab_id);
                            self.router.set_focused(tab_id);
                            self.chrome_generation += 1;
                        }
                        return;
                    }

                    // Check tree tab bars for drag initiation
                    if let Some(pane_id) = self.pane_at_tab_bar(self.last_cursor_pos) {
                        self.pane_drag = PaneDragState::PendingDrag {
                            source_pane: pane_id,
                            press_pos: self.last_cursor_pos,
                            from_panel: false,
                        };
                        // Focus the pane immediately
                        if self.focused != Some(pane_id) {
                            self.focused = Some(pane_id);
                            self.router.set_focused(pane_id);
                            self.chrome_generation += 1;
                            self.update_file_tree_cwd();
                        }
                        return;
                    }
                }

                let input = InputEvent::MouseClick {
                    position: self.last_cursor_pos,
                    button: btn,
                };

                let action = self.router.process(input, &self.pane_rects);
                self.handle_action(action, Some(input));
            }
            WindowEvent::CursorMoved { position, .. } => {
                let pos = Vec2::new(
                    position.x as f32 / self.scale_factor,
                    position.y as f32 / self.scale_factor,
                );
                self.last_cursor_pos = pos;

                // Handle pane drag state machine
                match &self.pane_drag {
                    PaneDragState::PendingDrag { source_pane, press_pos, from_panel } => {
                        let dx = pos.x - press_pos.x;
                        let dy = pos.y - press_pos.y;
                        if (dx * dx + dy * dy).sqrt() >= DRAG_THRESHOLD {
                            let source = *source_pane;
                            let fp = *from_panel;
                            let target = self.compute_drop_destination(pos, source, fp);
                            self.pane_drag = PaneDragState::Dragging {
                                source_pane: source,
                                from_panel: fp,
                                drop_target: target,
                            };
                        }
                        return;
                    }
                    PaneDragState::Dragging { source_pane, from_panel, .. } => {
                        let source = *source_pane;
                        let fp = *from_panel;
                        let target = self.compute_drop_destination(pos, source, fp);
                        self.pane_drag = PaneDragState::Dragging {
                            source_pane: source,
                            from_panel: fp,
                            drop_target: target,
                        };
                        return;
                    }
                    PaneDragState::Idle => {}
                }

                if self.router.is_dragging_border() {
                    // Adjust position for file tree offset
                    let drag_pos = if self.show_file_tree {
                        Vec2::new(pos.x - FILE_TREE_WIDTH, pos.y)
                    } else {
                        pos
                    };
                    self.layout.drag_border(drag_pos);
                    self.compute_layout();
                } else {
                    let input = InputEvent::MouseMove { position: pos };
                    let _ = self.router.process(input, &self.pane_rects);
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                let dy = match delta {
                    MouseScrollDelta::LineDelta(_, y) => y * 3.0,
                    MouseScrollDelta::PixelDelta(p) => p.y as f32 / 10.0,
                };

                // Check if scrolling over the file tree
                if self.show_file_tree && self.last_cursor_pos.x < FILE_TREE_WIDTH {
                    let new_scroll = (self.file_tree_scroll - dy * 10.0).max(0.0);
                    if new_scroll != self.file_tree_scroll {
                        self.file_tree_scroll = new_scroll;
                        self.chrome_generation += 1;
                    }
                } else if let Some(ref panel_rect) = self.editor_panel_rect {
                    if panel_rect.contains(self.last_cursor_pos) {
                        // Route scroll to active panel editor
                        if let Some(active_id) = self.editor_panel_active {
                            if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&active_id) {
                                use tide_editor::input::EditorAction;
                                if dy > 0.0 {
                                    pane.handle_action(EditorAction::ScrollUp(dy.abs()), 30);
                                } else {
                                    pane.handle_action(EditorAction::ScrollDown(dy.abs()), 30);
                                }
                            }
                        }
                    } else {
                        let input = InputEvent::MouseScroll {
                            delta: dy,
                            position: self.last_cursor_pos,
                        };
                        let action = self.router.process(input, &self.pane_rects);
                        self.handle_action(action, Some(input));
                    }
                } else {
                    let input = InputEvent::MouseScroll {
                        delta: dy,
                        position: self.last_cursor_pos,
                    };
                    let action = self.router.process(input, &self.pane_rects);
                    self.handle_action(action, Some(input));
                }
            }
            WindowEvent::RedrawRequested => {
                self.update();
                self.render();
                self.needs_redraw = false;
                self.last_frame = Instant::now();
            }
            _ => {}
        }
    }

    /// Handle editor panel click: tab switching, tab close, content area focus.
    pub(crate) fn handle_editor_panel_click(&mut self, pos: Vec2) {
        // Check close button first
        if let Some(tab_id) = self.panel_tab_close_at(pos) {
            self.close_editor_panel_tab(tab_id);
            return;
        }

        // Check tab bar click (switch tab)
        if let Some(tab_id) = self.panel_tab_at(pos) {
            self.editor_panel_active = Some(tab_id);
            self.focused = Some(tab_id);
            self.router.set_focused(tab_id);
            self.chrome_generation += 1;
            return;
        }

        // Content area click → focus active panel pane
        if let Some(active_id) = self.editor_panel_active {
            if self.focused != Some(active_id) {
                self.focused = Some(active_id);
                self.router.set_focused(active_id);
                self.chrome_generation += 1;
            }
        }
    }

    /// Handle a completed drop operation.
    fn handle_drop(&mut self, source: tide_core::PaneId, from_panel: bool, dest: DropDestination) {
        match dest {
            DropDestination::TreePane(target_id, zone) => {
                if from_panel {
                    // Moving from panel to tree: remove from panel, insert into tree
                    self.editor_panel_tabs.retain(|&id| id != source);
                    if self.editor_panel_active == Some(source) {
                        self.editor_panel_active = self.editor_panel_tabs.last().copied();
                    }

                    let (direction, insert_first) = match zone {
                        tide_core::DropZone::Top => (SplitDirection::Vertical, true),
                        tide_core::DropZone::Bottom => (SplitDirection::Vertical, false),
                        tide_core::DropZone::Left => (SplitDirection::Horizontal, true),
                        tide_core::DropZone::Right => (SplitDirection::Horizontal, false),
                        tide_core::DropZone::Center => {
                            // Swap: panel source takes target's place in tree, target goes to panel
                            // For simplicity, insert next to target on the right
                            (SplitDirection::Horizontal, false)
                        }
                    };

                    if zone == tide_core::DropZone::Center {
                        // For center drop from panel: just insert next to target
                        self.layout.insert_pane(target_id, source, direction, insert_first);
                    } else {
                        self.layout.insert_pane(target_id, source, direction, insert_first);
                    }

                    self.focused = Some(source);
                    self.router.set_focused(source);
                    self.chrome_generation += 1;
                    self.compute_layout();
                } else {
                    // Tree to tree: use existing move_pane
                    if self.layout.move_pane(source, target_id, zone) {
                        self.chrome_generation += 1;
                        self.compute_layout();
                    }
                }
            }
            DropDestination::EditorPanel => {
                // Moving from tree to panel
                // Only editor panes; terminal panes are rejected at compute_drop_destination
                self.layout.remove(source);
                if !self.editor_panel_tabs.contains(&source) {
                    self.editor_panel_tabs.push(source);
                }
                self.editor_panel_active = Some(source);
                self.focused = Some(source);
                self.router.set_focused(source);
                self.chrome_generation += 1;
                self.compute_layout();
            }
        }
    }
}
