use std::time::Instant;

use winit::event::ElementState;

use tide_core::{InputEvent, Renderer, TerminalBackend};

use crate::drag_drop::PaneDragState;
use crate::input::{winit_key_to_tide, winit_modifiers_to_tide, winit_physical_key_to_tide};
use crate::pane::PaneKind;
use crate::App;

use super::ime::is_hangul_char;

impl App {
    pub(crate) fn handle_keyboard_input(&mut self, event: winit::event::KeyEvent) {
        // Track whether this Pressed event has text — used to prevent
        // the Released handler from sending a duplicate character.
        if event.state == ElementState::Pressed && event.text.is_some() {
            self.last_pressed_with_text = Some(event.physical_key.clone());
        }
        if event.state != ElementState::Pressed {
            // On macOS, when a non-Hangul key (e.g. Shift+/ → ?)
            // is pressed during Korean IME composition, the Pressed
            // event is consumed by the IME and only a Released event
            // arrives with the character.  Send it directly to the
            // focused pane.
            if event.state == ElementState::Released && self.ime_active {
                // Skip if the corresponding Pressed event already had text
                // (meaning it was processed normally, not consumed by IME).
                if let Some(ref pressed_key) = self.last_pressed_with_text {
                    if *pressed_key == event.physical_key {
                        self.last_pressed_with_text = None;
                        return;
                    }
                }
                if let winit::keyboard::Key::Character(ref s) = event.logical_key {
                    if let Some(c) = s.as_str().chars().next() {
                        if !is_hangul_char(c) {
                            if self.branch_switcher.is_some() {
                                if let Some(ref mut bs) = self.branch_switcher {
                                    bs.insert_char(c);
                                    self.chrome_generation += 1;
                                }
                                self.needs_redraw = true;
                            } else if self.file_switcher.is_some() {
                                if let Some(ref mut fs) = self.file_switcher {
                                    fs.insert_char(c);
                                    self.chrome_generation += 1;
                                }
                                self.needs_redraw = true;
                            } else if self.file_finder.is_some() {
                                if let Some(ref mut finder) = self.file_finder {
                                    finder.insert_char(c);
                                    self.chrome_generation += 1;
                                }
                                self.needs_redraw = true;
                            } else if let Some(search_pane_id) = self.search_focus {
                                self.search_bar_insert(search_pane_id, c);
                            } else if let Some(focused_id) = self.focused {
                                match self.panes.get_mut(&focused_id) {
                                    Some(PaneKind::Terminal(pane)) => {
                                        pane.backend.write(s.as_bytes());
                                        self.input_just_sent = true;
                                        self.input_sent_at = Some(Instant::now());
                                    }
                                    Some(PaneKind::Editor(pane)) => {
                                        pane.editor.handle_action(
                                            tide_editor::EditorActionKind::InsertChar(c),
                                        );
                                    }
                                    Some(PaneKind::Diff(_)) => {}
                                    None => {}
                                }
                            }
                        }
                    }
                }
            }
            return;
        }

        // Cancel pane drag on Escape
        if !matches!(self.pane_drag, PaneDragState::Idle) {
            if event.logical_key == winit::keyboard::Key::Named(winit::keyboard::NamedKey::Escape) {
                self.pane_drag = PaneDragState::Idle;
                return;
            }
        }

        // Skip character keys that IME is handling.
        // Track whether this event should bypass the text.is_none() skip.
        let mut ime_pass_through = false;
        if matches!(event.logical_key, winit::keyboard::Key::Character(_)) {
            if self.ime_composing && event.text.is_none() {
                // During active composition, only skip Hangul characters
                // — they will be delivered via Ime::Commit.
                // Non-Hangul characters (e.g. '?', '!', numbers) pressed
                // during composition won't arrive via Ime::Commit on macOS
                // (KeyboardInput fires BEFORE Ime::Commit), so let them
                // fall through to be sent directly to the terminal.
                let is_non_hangul = matches!(
                    &event.logical_key,
                    winit::keyboard::Key::Character(s)
                        if s.as_str().chars().next().map_or(false, |c| !is_hangul_char(c))
                );
                if is_non_hangul {
                    ime_pass_through = true;
                } else {
                    return;
                }
            }

            // Handle Hangul characters from KeyboardInput.
            if let winit::keyboard::Key::Character(ref s) = event.logical_key {
                if let Some(c) = s.as_str().chars().next() {
                    if is_hangul_char(c)
                        && !self.modifiers.control_key()
                        && !self.modifiers.super_key()
                        && !self.modifiers.alt_key()
                    {
                        if self.ime_active {
                            // IME is active — it will deliver via Preedit/Commit.
                            return;
                        }
                        // IME not yet active (first char after language switch).
                        // Store as pending and show as preedit; the next
                        // Ime::Preedit/Commit will combine it.
                        self.pending_hangul_initial = Some(c);
                        self.ime_preedit = s.to_string();
                        return;
                    }
                }
            }

            // For non-Hangul characters: skip only when the system didn't produce
            // committed text (event.text is None), meaning IME consumed the
            // keystroke and will deliver it via Ime::Commit.
            // Exception: during active composition (ime_pass_through), the
            // non-Hangul key won't arrive via Ime::Commit so must not be skipped.
            // Previous code also checked `ime_active` here, but that flag can
            // get stuck on macOS (한/영 toggle doesn't always fire Ime::Disabled),
            // causing numbers and ASCII to be silently dropped.
            if event.text.is_none()
                && !ime_pass_through
                && !self.modifiers.control_key()
                && !self.modifiers.super_key()
                && !self.modifiers.alt_key()
            {
                return;
            }
        }

        // When Cmd/Ctrl is held, prefer physical key so hotkeys work
        // regardless of IME language (e.g. Korean Cmd+ㅠ → physical B → Cmd+B)
        let modifiers = winit_modifiers_to_tide(self.modifiers);
        let key_opt = if modifiers.ctrl || modifiers.meta {
            winit_physical_key_to_tide(&event.physical_key)
                .or_else(|| winit_key_to_tide(&event.logical_key))
        } else {
            winit_key_to_tide(&event.logical_key)
        };

        if let Some(key) = key_opt {
            // Cmd+Q / Ctrl+Q → quit the app
            if matches!(key, tide_core::Key::Char('q'))
                && modifiers.meta
                && !modifiers.ctrl
                && !modifiers.shift
                && !modifiers.alt
            {
                let session = crate::session::Session::from_app(self);
                crate::session::save_session(&session);
                crate::session::delete_running_marker();
                std::process::exit(0);
            }

            // Branch switcher popup interception: consume all keys when active
            if self.branch_switcher.is_some() {
                self.handle_branch_switcher_key(key, &modifiers);
                return;
            }

            // File switcher popup interception: consume all keys when active
            if self.file_switcher.is_some() {
                self.handle_file_switcher_key(key, &modifiers);
                return;
            }

            // File finder interception: consume all keys when active
            if self.file_finder.is_some() {
                self.handle_file_finder_key(key, &modifiers);
                return;
            }

            // Save-as input interception: consume all keys when active
            if self.save_as_input.is_some() {
                self.handle_save_as_key(key, &modifiers);
                return;
            }

            // Save confirm bar interception: Escape cancels, block other keys
            if self.save_confirm.is_some() {
                if matches!(key, tide_core::Key::Escape) {
                    self.cancel_save_confirm();
                }
                // Block all other keys while save confirm is active
                self.needs_redraw = true;
                return;
            }

            // Search bar key interception: when search is focused, consume keys
            if let Some(search_pane_id) = self.search_focus {
                self.handle_search_bar_key(search_pane_id, key, &modifiers);
                return;
            }

            let input = InputEvent::KeyPress { key, modifiers };

            let action = self.router.process(input, &self.pane_rects);
            self.handle_action(action, Some(input));
        }
    }

    fn handle_branch_switcher_key(&mut self, key: tide_core::Key, modifiers: &tide_core::Modifiers) {
        match key {
            tide_core::Key::Escape => {
                self.branch_switcher = None;
            }
            tide_core::Key::Enter => {
                let selected = self.branch_switcher.as_ref()
                    .and_then(|bs| bs.selected_branch().map(|b| (bs.pane_id, b.name.clone())));
                self.branch_switcher = None;
                if let Some((pane_id, branch_name)) = selected {
                    // Inject `git checkout <branch>\n` into the terminal
                    if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(&pane_id) {
                        let cmd = format!("git checkout {}\n", branch_name);
                        pane.backend.write(cmd.as_bytes());
                    }
                }
            }
            tide_core::Key::Up => {
                if let Some(ref mut bs) = self.branch_switcher {
                    bs.select_up();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Down => {
                if let Some(ref mut bs) = self.branch_switcher {
                    bs.select_down();
                    // Auto-scroll
                    let visible_rows = 10usize; // matches render max
                    if bs.selected >= bs.scroll_offset + visible_rows {
                        bs.scroll_offset = bs.selected.saturating_sub(visible_rows - 1);
                    }
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Backspace => {
                if let Some(ref mut bs) = self.branch_switcher {
                    bs.backspace();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut bs) = self.branch_switcher {
                        bs.insert_char(ch);
                        self.chrome_generation += 1;
                    }
                }
            }
            _ => {} // consume all other keys
        }
        self.needs_redraw = true;
    }

    fn handle_file_switcher_key(&mut self, key: tide_core::Key, modifiers: &tide_core::Modifiers) {
        match key {
            tide_core::Key::Escape => {
                self.file_switcher = None;
            }
            tide_core::Key::Enter => {
                let selected_pane_id = self.file_switcher.as_ref()
                    .and_then(|fs| fs.selected_entry().map(|e| e.pane_id));
                self.file_switcher = None;
                if let Some(pane_id) = selected_pane_id {
                    self.editor_panel_active = Some(pane_id);
                    self.chrome_generation += 1;
                    self.pane_generations.remove(&pane_id);
                }
            }
            tide_core::Key::Up => {
                if let Some(ref mut fs) = self.file_switcher {
                    fs.select_up();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Down => {
                if let Some(ref mut fs) = self.file_switcher {
                    fs.select_down();
                    let visible_rows = 10usize;
                    if fs.selected >= fs.scroll_offset + visible_rows {
                        fs.scroll_offset = fs.selected.saturating_sub(visible_rows - 1);
                    }
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Backspace => {
                if let Some(ref mut fs) = self.file_switcher {
                    fs.backspace();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut fs) = self.file_switcher {
                        fs.insert_char(ch);
                        self.chrome_generation += 1;
                    }
                }
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    fn handle_file_finder_key(&mut self, key: tide_core::Key, modifiers: &tide_core::Modifiers) {
        // Cmd+Enter / Ctrl+Enter → toggle maximize
        if matches!(key, tide_core::Key::Enter) && (modifiers.meta || modifiers.ctrl) {
            self.editor_panel_maximized = !self.editor_panel_maximized;
            self.chrome_generation += 1;
            self.compute_layout();
            self.needs_redraw = true;
            return;
        }
        // Cmd+K / Cmd+J → select up/down (vim-style)
        if (modifiers.meta || modifiers.ctrl)
            && matches!(key, tide_core::Key::Char('k') | tide_core::Key::Char('K'))
        {
            if let Some(ref mut finder) = self.file_finder {
                finder.select_up();
                self.chrome_generation += 1;
            }
            self.needs_redraw = true;
            return;
        }
        if (modifiers.meta || modifiers.ctrl)
            && matches!(key, tide_core::Key::Char('j') | tide_core::Key::Char('J'))
        {
            if let Some(ref mut finder) = self.file_finder {
                finder.select_down();
                let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                if let (Some(cs), Some(panel_rect)) = (cell_size, self.editor_panel_rect) {
                    let line_height = cs.height * crate::theme::FILE_TREE_LINE_SPACING;
                    let input_y = panel_rect.y + crate::theme::PANE_PADDING + 8.0;
                    let input_h = cs.height + 12.0;
                    let list_top = input_y + input_h + 8.0;
                    let list_bottom = panel_rect.y + panel_rect.height - crate::theme::PANE_PADDING;
                    let visible_rows = ((list_bottom - list_top) / line_height).floor() as usize;
                    if finder.selected >= finder.scroll_offset + visible_rows {
                        finder.scroll_offset = finder.selected.saturating_sub(visible_rows - 1);
                    }
                }
                self.chrome_generation += 1;
            }
            self.needs_redraw = true;
            return;
        }
        match key {
            tide_core::Key::Escape => {
                self.close_file_finder();
            }
            tide_core::Key::Enter => {
                let path = self.file_finder.as_ref().and_then(|f| f.selected_path());
                self.close_file_finder();
                if let Some(path) = path {
                    self.open_editor_pane(path);
                }
            }
            tide_core::Key::Up => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.select_up();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Down => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.select_down();
                    // Auto-scroll: ensure selected item is visible
                    let cell_size = self.renderer.as_ref().map(|r| r.cell_size());
                    if let (Some(cs), Some(panel_rect)) = (cell_size, self.editor_panel_rect) {
                        let line_height = cs.height * crate::theme::FILE_TREE_LINE_SPACING;
                        let input_y = panel_rect.y + crate::theme::PANE_PADDING + 8.0;
                        let input_h = cs.height + 12.0;
                        let list_top = input_y + input_h + 8.0;
                        let list_bottom = panel_rect.y + panel_rect.height - crate::theme::PANE_PADDING;
                        let visible_rows = ((list_bottom - list_top) / line_height).floor() as usize;
                        if finder.selected >= finder.scroll_offset + visible_rows {
                            finder.scroll_offset = finder.selected.saturating_sub(visible_rows - 1);
                        }
                    }
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Backspace => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.backspace();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Delete => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.delete_char();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Left => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.move_cursor_left();
                }
            }
            tide_core::Key::Right => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.move_cursor_right();
                }
            }
            tide_core::Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut finder) = self.file_finder {
                        finder.insert_char(ch);
                        self.chrome_generation += 1;
                    }
                }
            }
            _ => {} // consume all other keys
        }
        self.needs_redraw = true;
    }

    fn handle_save_as_key(&mut self, key: tide_core::Key, modifiers: &tide_core::Modifiers) {
        match key {
            tide_core::Key::Escape => {
                self.save_as_input = None;
            }
            tide_core::Key::Enter => {
                let pane_id = self.save_as_input.as_ref().unwrap().pane_id;
                let filename = self.save_as_input.as_ref().unwrap().query.clone();
                self.save_as_input = None;
                if !filename.is_empty() {
                    self.complete_save_as(pane_id, &filename);
                }
            }
            tide_core::Key::Backspace => {
                if let Some(ref mut input) = self.save_as_input {
                    input.backspace();
                }
            }
            tide_core::Key::Delete => {
                if let Some(ref mut input) = self.save_as_input {
                    input.delete_char();
                }
            }
            tide_core::Key::Left => {
                if let Some(ref mut input) = self.save_as_input {
                    input.move_cursor_left();
                }
            }
            tide_core::Key::Right => {
                if let Some(ref mut input) = self.save_as_input {
                    input.move_cursor_right();
                }
            }
            tide_core::Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut input) = self.save_as_input {
                        input.insert_char(ch);
                    }
                }
            }
            _ => {} // consume all other keys
        }
        self.needs_redraw = true;
    }

    fn handle_search_bar_key(&mut self, search_pane_id: tide_core::PaneId, key: tide_core::Key, modifiers: &tide_core::Modifiers) {
        // Cmd+F while search is focused → close search (toggle)
        if matches!(key, tide_core::Key::Char('f') | tide_core::Key::Char('F'))
            && (modifiers.meta || modifiers.ctrl)
            && !(modifiers.meta && modifiers.ctrl)
        {
            match self.panes.get_mut(&search_pane_id) {
                Some(PaneKind::Terminal(pane)) => { pane.search = None; }
                Some(PaneKind::Editor(pane)) => { pane.search = None; }
                Some(PaneKind::Diff(_)) => {}
                None => {}
            }
            self.search_focus = None;
            return;
        }

        match key {
            tide_core::Key::Escape => {
                // Close search
                match self.panes.get_mut(&search_pane_id) {
                    Some(PaneKind::Terminal(pane)) => { pane.search = None; }
                    Some(PaneKind::Editor(pane)) => { pane.search = None; }
                    Some(PaneKind::Diff(_)) => {}
                    None => {}
                }
                self.search_focus = None;
            }
            tide_core::Key::Enter => {
                if modifiers.shift {
                    self.search_prev_match(search_pane_id);
                } else {
                    self.search_next_match(search_pane_id);
                }
            }
            tide_core::Key::Backspace => {
                self.search_bar_backspace(search_pane_id);
            }
            tide_core::Key::Delete => {
                self.search_bar_delete(search_pane_id);
            }
            tide_core::Key::Left => {
                self.search_bar_cursor_left(search_pane_id);
            }
            tide_core::Key::Right => {
                self.search_bar_cursor_right(search_pane_id);
            }
            tide_core::Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    self.search_bar_insert(search_pane_id, ch);
                }
            }
            _ => {} // consume all other keys
        }
    }
}
