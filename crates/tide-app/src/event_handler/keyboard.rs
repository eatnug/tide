use std::time::Instant;

use winit::event::ElementState;

use tide_core::{FileTreeSource, InputEvent, Renderer, TerminalBackend};

use crate::drag_drop::PaneDragState;
use crate::input::{winit_key_to_tide, winit_modifiers_to_tide, winit_physical_key_to_tide};
use crate::pane::PaneKind;
use crate::ui_state::FocusArea;
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
                            if self.config_page.is_some() {
                                if let Some(ref mut page) = self.config_page {
                                    if page.worktree_editing {
                                        page.worktree_input.insert_char(c);
                                        page.dirty = true;
                                        self.chrome_generation += 1;
                                    }
                                }
                                self.needs_redraw = true;
                            } else if self.file_tree_rename.is_some() {
                                if let Some(ref mut rename) = self.file_tree_rename {
                                    rename.input.insert_char(c);
                                    self.chrome_generation += 1;
                                }
                                self.needs_redraw = true;
                            } else if self.git_switcher.is_some() {
                                if let Some(ref mut gs) = self.git_switcher {
                                    gs.insert_char(c);
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
                            } else if self.focus_area == crate::ui_state::FocusArea::FileTree {
                                // FileTree focused: consume character (don't send to terminal)
                            } else {
                                // Route to dock editor or focused pane
                                let target_id = if self.focus_area == crate::ui_state::FocusArea::EditorDock {
                                    self.active_editor_tab().or(self.focused)
                                } else {
                                    self.focused
                                };
                                if let Some(target_id) = target_id {
                                    match self.panes.get_mut(&target_id) {
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
                if is_non_hangul
                    || self.modifiers.control_key()
                    || self.modifiers.super_key()
                {
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

            // Config page interception: consume all keys when active
            if self.config_page.is_some() {
                self.handle_config_page_key(key, &modifiers);
                return;
            }

            // Context menu interception: consume all keys when active
            if self.context_menu.is_some() {
                self.handle_context_menu_key(key);
                return;
            }

            // File tree inline rename interception: consume all keys when active
            if self.file_tree_rename.is_some() {
                self.handle_file_tree_rename_key(key, &modifiers);
                return;
            }

            // Git switcher popup interception: consume all keys when active
            if self.git_switcher.is_some() {
                self.handle_git_switcher_key(key, &modifiers);
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

            // FocusArea interception: file tree or dock keyboard nav
            match self.focus_area {
                FocusArea::FileTree => {
                    // Cmd+Enter opens file/toggles folder
                    if matches!(key, tide_core::Key::Enter) && modifiers.meta {
                        self.handle_file_tree_nav_key(key, &modifiers);
                        return;
                    }
                    // Global hotkeys: pass through (but don't forward unrecognized to terminal)
                    if modifiers.meta || (modifiers.ctrl && modifiers.shift) {
                        let input = InputEvent::KeyPress { key, modifiers };
                        let action = self.router.process(input, &self.pane_rects);
                        if !matches!(action, tide_input::Action::RouteToPane(_)) {
                            self.handle_action(action, Some(input));
                        }
                        return;
                    }
                    // Plain keys → file tree navigation (j/k/g/G/Enter)
                    self.handle_file_tree_nav_key(key, &modifiers);
                    return;
                }
                FocusArea::EditorDock => {
                    // Global hotkeys: pass through, reroute unrecognized to editor
                    if modifiers.meta || (modifiers.ctrl && modifiers.shift) {
                        let input = InputEvent::KeyPress { key, modifiers };
                        let action = self.router.process(input, &self.pane_rects);
                        // When the router returns RouteToPane (unrecognized hotkey),
                        // reroute to the active editor tab so keys like Cmd+S reach the editor.
                        let rerouted = if let tide_input::Action::RouteToPane(_) = &action {
                            self.active_editor_tab().map(tide_input::Action::RouteToPane)
                        } else { None };
                        self.handle_action(rerouted.unwrap_or(action), Some(input));
                        return;
                    }
                    // Plain keys → forward to the focused editor pane in the dock
                    let input = InputEvent::KeyPress { key, modifiers };
                    if let Some(active_editor) = self.active_editor_tab() {
                        self.handle_action(
                            tide_input::Action::RouteToPane(active_editor),
                            Some(input),
                        );
                    }
                    return;
                }
                FocusArea::PaneArea => {
                    // Default: fall through to normal routing below
                }
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

    fn handle_git_switcher_key(&mut self, key: tide_core::Key, modifiers: &tide_core::Modifiers) {
        match key {
            tide_core::Key::Escape => {
                self.git_switcher = None;
            }
            tide_core::Key::Tab => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.toggle_mode();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Enter => {
                let selected = self.git_switcher.as_ref().map(|gs| gs.selected);
                if let Some(selected) = selected {
                    self.handle_git_switcher_button(crate::SwitcherButton::Switch(selected));
                }
                return; // handle_git_switcher_button already sets needs_redraw
            }
            tide_core::Key::Up => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.select_up();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Down => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.select_down();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Backspace => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.backspace();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Delete => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.delete_char();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Left => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.move_cursor_left();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Right => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.move_cursor_right();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut gs) = self.git_switcher {
                        gs.insert_char(ch);
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
                    if let Some(tid) = self.terminal_owning(pane_id) {
                        if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                            tp.active_editor = Some(pane_id);
                        }
                    }
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
            tide_core::Key::Tab => {
                if let Some(ref mut input) = self.save_as_input {
                    input.toggle_field();
                }
            }
            tide_core::Key::Enter => {
                let resolved = self.save_as_input.as_ref().and_then(|input| {
                    let pane_id = input.pane_id;
                    input.resolve_path().map(|p| (pane_id, p))
                });
                self.save_as_input = None;
                if let Some((pane_id, path)) = resolved {
                    let path_str = path.to_string_lossy().to_string();
                    self.complete_save_as(pane_id, &path_str);
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

    fn handle_context_menu_key(&mut self, key: tide_core::Key) {
        match key {
            tide_core::Key::Escape => {
                self.context_menu = None;
            }
            tide_core::Key::Up => {
                if let Some(ref mut menu) = self.context_menu {
                    if menu.selected > 0 {
                        menu.selected -= 1;
                    }
                }
            }
            tide_core::Key::Down => {
                if let Some(ref mut menu) = self.context_menu {
                    if menu.selected + 1 < crate::ContextMenuAction::ALL.len() {
                        menu.selected += 1;
                    }
                }
            }
            tide_core::Key::Enter => {
                let selected = self.context_menu.as_ref().map(|m| m.selected);
                if let Some(idx) = selected {
                    self.execute_context_menu_action(idx);
                }
                self.context_menu = None;
            }
            _ => {} // consume all other keys
        }
        self.needs_redraw = true;
    }

    fn handle_file_tree_rename_key(&mut self, key: tide_core::Key, modifiers: &tide_core::Modifiers) {
        match key {
            tide_core::Key::Escape => {
                self.file_tree_rename = None;
                self.chrome_generation += 1;
            }
            tide_core::Key::Enter => {
                self.complete_file_tree_rename();
            }
            tide_core::Key::Backspace => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    rename.input.backspace();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Delete => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    rename.input.delete_char();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Left => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    rename.input.move_cursor_left();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Right => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    rename.input.move_cursor_right();
                    self.chrome_generation += 1;
                }
            }
            tide_core::Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut rename) = self.file_tree_rename {
                        rename.input.insert_char(ch);
                        self.chrome_generation += 1;
                    }
                }
            }
            _ => {} // consume all other keys
        }
        self.needs_redraw = true;
    }

    fn handle_file_tree_nav_key(&mut self, key: tide_core::Key, _modifiers: &tide_core::Modifiers) {
        let entry_count = self.file_tree.as_ref()
            .map(|t| t.visible_entries().len())
            .unwrap_or(0);
        if entry_count == 0 {
            self.needs_redraw = true;
            return;
        }

        match key {
            tide_core::Key::Char('j') | tide_core::Key::Down => {
                if self.file_tree_cursor + 1 < entry_count {
                    self.file_tree_cursor += 1;
                    self.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            tide_core::Key::Char('k') | tide_core::Key::Up => {
                if self.file_tree_cursor > 0 {
                    self.file_tree_cursor -= 1;
                    self.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            tide_core::Key::Char('g') => {
                self.file_tree_cursor = 0;
                self.chrome_generation += 1;
                self.auto_scroll_file_tree_cursor();
            }
            tide_core::Key::Char('G') => {
                if entry_count > 0 {
                    self.file_tree_cursor = entry_count - 1;
                    self.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            tide_core::Key::Enter => {
                if let Some(tree) = &self.file_tree {
                    let entries = tree.visible_entries();
                    if let Some(entry) = entries.get(self.file_tree_cursor) {
                        if entry.entry.is_dir {
                            let path = entry.entry.path.clone();
                            if let Some(tree) = &mut self.file_tree {
                                tree.toggle(&path);
                            }
                            self.chrome_generation += 1;
                        } else {
                            let path = entry.entry.path.clone();
                            self.open_editor_pane(path);
                        }
                    }
                }
            }
            _ => {} // consume all other keys
        }
        self.needs_redraw = true;
    }

    /// Adjust file tree scroll so the cursor is visible.
    pub(crate) fn auto_scroll_file_tree_cursor(&mut self) {
        if let (Some(tree_rect), Some(renderer)) = (self.file_tree_rect, self.renderer.as_ref()) {
            let cell_size = renderer.cell_size();
            let line_height = cell_size.height * crate::theme::FILE_TREE_LINE_SPACING;
            let padding = crate::theme::PANE_PADDING;

            let cursor_y = padding + self.file_tree_cursor as f32 * line_height;
            let visible_top = self.file_tree_scroll;
            let visible_bottom = self.file_tree_scroll + tree_rect.height - padding * 2.0;

            if cursor_y < visible_top {
                self.file_tree_scroll_target = cursor_y;
                self.file_tree_scroll = cursor_y;
            } else if cursor_y + line_height > visible_bottom {
                self.file_tree_scroll_target = cursor_y + line_height - (tree_rect.height - padding * 2.0);
                self.file_tree_scroll = self.file_tree_scroll_target;
            }
        }
    }

    fn handle_config_page_key(&mut self, key: tide_core::Key, modifiers: &tide_core::Modifiers) {
        use crate::ui_state::ConfigSection;

        let page = match self.config_page.as_mut() {
            Some(p) => p,
            None => return,
        };

        // If recording a new keybinding, capture the next key
        if page.recording.is_some() {
            if matches!(key, tide_core::Key::Escape) {
                // Cancel recording
                page.recording = None;
            } else {
                let hotkey = tide_input::Hotkey::new(
                    key,
                    modifiers.shift,
                    modifiers.ctrl,
                    modifiers.meta,
                    modifiers.alt,
                );
                let action_index = page.recording.as_ref().unwrap().action_index;
                if action_index < page.bindings.len() {
                    page.bindings[action_index].1 = hotkey;
                    page.dirty = true;
                }
                page.recording = None;
            }
            self.chrome_generation += 1;
            self.needs_redraw = true;
            return;
        }

        // Worktree editing mode
        if page.worktree_editing {
            match key {
                tide_core::Key::Escape | tide_core::Key::Enter => {
                    page.worktree_editing = false;
                    page.dirty = true;
                }
                tide_core::Key::Backspace => {
                    page.worktree_input.backspace();
                    page.dirty = true;
                }
                tide_core::Key::Delete => {
                    page.worktree_input.delete_char();
                    page.dirty = true;
                }
                tide_core::Key::Left => {
                    page.worktree_input.move_cursor_left();
                }
                tide_core::Key::Right => {
                    page.worktree_input.move_cursor_right();
                }
                tide_core::Key::Char(ch) => {
                    if !modifiers.ctrl && !modifiers.meta {
                        page.worktree_input.insert_char(ch);
                        page.dirty = true;
                    }
                }
                _ => {}
            }
            self.chrome_generation += 1;
            self.needs_redraw = true;
            return;
        }

        match key {
            tide_core::Key::Escape => {
                // Close config page (saves if dirty)
                self.close_config_page();
            }
            tide_core::Key::Tab => {
                // Switch section
                let page = self.config_page.as_mut().unwrap();
                page.section = match page.section {
                    ConfigSection::Keybindings => ConfigSection::Worktree,
                    ConfigSection::Worktree => ConfigSection::Keybindings,
                };
                page.selected = 0;
                page.scroll_offset = 0;
            }
            tide_core::Key::Up | tide_core::Key::Char('k') => {
                if !modifiers.ctrl && !modifiers.meta {
                    let page = self.config_page.as_mut().unwrap();
                    if page.selected > 0 {
                        page.selected -= 1;
                        if page.selected < page.scroll_offset {
                            page.scroll_offset = page.selected;
                        }
                    }
                }
            }
            tide_core::Key::Down | tide_core::Key::Char('j') => {
                if !modifiers.ctrl && !modifiers.meta {
                    let page = self.config_page.as_mut().unwrap();
                    match page.section {
                        ConfigSection::Keybindings => {
                            if page.selected + 1 < page.bindings.len() {
                                page.selected += 1;
                                let max_visible = crate::theme::CONFIG_PAGE_MAX_VISIBLE;
                                if page.selected >= page.scroll_offset + max_visible {
                                    page.scroll_offset = page.selected.saturating_sub(max_visible - 1);
                                }
                            }
                        }
                        ConfigSection::Worktree => {} // single item
                    }
                }
            }
            tide_core::Key::Enter => {
                let page = self.config_page.as_mut().unwrap();
                match page.section {
                    ConfigSection::Keybindings => {
                        // Start recording
                        page.recording = Some(crate::RecordingState {
                            action_index: page.selected,
                        });
                    }
                    ConfigSection::Worktree => {
                        page.worktree_editing = true;
                    }
                }
            }
            tide_core::Key::Backspace => {
                let page = self.config_page.as_mut().unwrap();
                if page.section == ConfigSection::Keybindings && page.selected < page.bindings.len() {
                    // Reset to default
                    let action = &page.bindings[page.selected].0;
                    let defaults = tide_input::KeybindingMap::default_bindings();
                    if let Some((dh, _)) = defaults.iter().find(|(_, da)| da.action_key() == action.action_key()) {
                        page.bindings[page.selected].1 = dh.clone();
                        page.dirty = true;
                    }
                }
            }
            _ => {} // consume all other keys
        }
        self.chrome_generation += 1;
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
