//! Keyboard event handling.
//!
//! With native IME, the platform calls ImeCommit for all committed text.
//! KeyDown only fires for keys NOT consumed by the IME (hotkeys, control keys).

use tide_core::{FileTreeSource, InputEvent, Key, Modifiers};

use crate::drag_drop::PaneDragState;
use crate::pane::PaneKind;
use crate::ui_state::FocusArea;
use crate::App;

impl App {
    pub(crate) fn handle_key_down(
        &mut self,
        key: Key,
        modifiers: Modifiers,
        chars: Option<String>,
    ) {
        // Cancel pane drag on Escape
        if !matches!(self.pane_drag, PaneDragState::Idle) {
            if matches!(key, Key::Escape) {
                self.pane_drag = PaneDragState::Idle;
                self.needs_redraw = true;
                return;
            }
        }

        // If the key produced text and no command modifiers are held,
        // route via the text input system.
        if let Some(ref text) = chars {
            if !modifiers.meta && !modifiers.ctrl && !modifiers.alt {
                self.send_text_to_target(text);
                self.needs_redraw = true;
                return;
            }
        }

        // Cmd+Q → quit
        if matches!(key, Key::Char('q'))
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

        // Config page interception
        if self.config_page.is_some() {
            self.handle_config_page_key(key, &modifiers);
            return;
        }

        // Context menu interception
        if self.context_menu.is_some() {
            self.handle_context_menu_key(key);
            return;
        }

        // File tree inline rename interception
        if self.file_tree_rename.is_some() {
            self.handle_file_tree_rename_key(key, &modifiers);
            return;
        }

        // Git switcher popup interception
        if self.git_switcher.is_some() {
            self.handle_git_switcher_key(key, &modifiers);
            return;
        }

        // File switcher popup interception
        if self.file_switcher.is_some() {
            self.handle_file_switcher_key(key, &modifiers);
            return;
        }

        // File finder interception
        if self.file_finder.is_some() {
            self.handle_file_finder_key(key, &modifiers);
            return;
        }

        // Save-as input interception
        if self.save_as_input.is_some() {
            self.handle_save_as_key(key, &modifiers);
            return;
        }

        // Branch cleanup bar interception
        if let Some(ref bc) = self.branch_cleanup {
            // Safety: clear stale state if the pane no longer exists
            if !self.panes.contains_key(&bc.pane_id) {
                self.branch_cleanup = None;
            } else {
                match key {
                    Key::Escape => {
                        self.cancel_branch_cleanup();
                    }
                    Key::Enter => {
                        // Enter → Keep (safe default: close without deleting)
                        self.confirm_branch_keep();
                    }
                    _ => {}
                }
                self.needs_redraw = true;
                return;
            }
        }

        // Save confirm bar interception
        if self.save_confirm.is_some() {
            if matches!(key, Key::Escape) {
                self.cancel_save_confirm();
            }
            self.needs_redraw = true;
            return;
        }

        // FocusArea interception
        match self.focus_area {
            FocusArea::FileTree => {
                if matches!(key, Key::Enter) && modifiers.meta {
                    self.handle_file_tree_nav_key(key, &modifiers);
                    return;
                }
                if modifiers.meta || (modifiers.ctrl && modifiers.shift) {
                    let input = InputEvent::KeyPress { key, modifiers };
                    let action = self.router.process(input, &self.pane_rects);
                    if !matches!(action, tide_input::Action::RouteToPane(_)) {
                        self.handle_action(action, Some(input));
                    }
                    self.needs_redraw = true;
                    return;
                }
                self.handle_file_tree_nav_key(key, &modifiers);
                return;
            }
            FocusArea::EditorDock => {
                // Global hotkeys take priority over URL bar input
                if modifiers.meta || (modifiers.ctrl && modifiers.shift) {
                    let input = InputEvent::KeyPress { key, modifiers };
                    let action = self.router.process(input, &self.pane_rects);
                    if !matches!(action, tide_input::Action::RouteToPane(_)) {
                        self.handle_action(action, Some(input));
                        self.needs_redraw = true;
                        return;
                    }
                }

                // Browser URL bar keyboard handling
                if let Some(active_id) = self.active_editor_tab() {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get(&active_id) {
                        if bp.url_input_focused {
                            self.handle_browser_url_bar_key(active_id, key, &modifiers);
                            return;
                        }
                        // Cmd+L → focus URL bar
                        if modifiers.meta && matches!(key, Key::Char('l') | Key::Char('L')) {
                            if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&active_id) {
                                bp.url_input_focused = true;
                                bp.url_input = bp.url.clone();
                                bp.url_input_cursor = bp.url_input.chars().count();
                                self.chrome_generation += 1;
                                self.needs_redraw = true;
                            }
                            return;
                        }
                    }
                }

                // Search bar interception (before routing to editor pane)
                if let Some(search_pane_id) = self.search_focus {
                    self.handle_search_bar_key(search_pane_id, key, &modifiers);
                    return;
                }

                if modifiers.meta || (modifiers.ctrl && modifiers.shift) {
                    let input = InputEvent::KeyPress { key, modifiers };
                    let action = self.router.process(input, &self.pane_rects);
                    let rerouted = if let tide_input::Action::RouteToPane(_) = &action {
                        self.active_editor_tab()
                            .map(tide_input::Action::RouteToPane)
                    } else {
                        None
                    };
                    self.handle_action(rerouted.unwrap_or(action), Some(input));
                    return;
                }
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
                // Fall through to normal routing
            }
        }

        // Search bar interception
        if let Some(search_pane_id) = self.search_focus {
            self.handle_search_bar_key(search_pane_id, key, &modifiers);
            return;
        }

        let input = InputEvent::KeyPress { key, modifiers };
        let action = self.router.process(input, &self.pane_rects);
        self.handle_action(action, Some(input));
        self.needs_redraw = true;
    }

    fn handle_git_switcher_key(&mut self, key: Key, modifiers: &Modifiers) {
        // Cmd+Backspace → delete selected item
        if matches!(key, Key::Backspace) && modifiers.meta && !modifiers.ctrl && !modifiers.alt {
            let selected = self.git_switcher.as_ref().map(|gs| gs.selected);
            if let Some(selected) = selected {
                self.handle_git_switcher_button(crate::SwitcherButton::Delete(selected));
            }
            return;
        }

        match key {
            Key::Escape => {
                // If delete confirmation is active, cancel it first
                if let Some(ref mut gs) = self.git_switcher {
                    if gs.delete_confirm.is_some() {
                        gs.delete_confirm = None;
                        self.chrome_generation += 1;
                        self.needs_redraw = true;
                        return;
                    }
                }
                self.git_switcher = None;
            }
            Key::Tab => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.delete_confirm = None;
                    gs.toggle_mode();
                    self.chrome_generation += 1;
                }
            }
            Key::Enter => {
                let info = self.git_switcher.as_ref().map(|gs| (gs.selected, gs.mode));
                if let Some((selected, mode)) = info {
                    let btn = if modifiers.meta {
                        // Cmd+Enter → always New Pane
                        crate::SwitcherButton::NewPane(selected)
                    } else {
                        match mode {
                            crate::GitSwitcherMode::Branches => crate::SwitcherButton::Switch(selected),
                            // Worktrees: Enter triggers NewPane (no Switch action)
                            crate::GitSwitcherMode::Worktrees => crate::SwitcherButton::NewPane(selected),
                        }
                    };
                    self.handle_git_switcher_button(btn);
                }
                return;
            }
            Key::Up => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.delete_confirm = None;
                    gs.select_up();
                    self.chrome_generation += 1;
                }
            }
            Key::Down => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.delete_confirm = None;
                    gs.select_down();
                    self.chrome_generation += 1;
                }
            }
            Key::Backspace => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.delete_confirm = None;
                    gs.backspace();
                    self.chrome_generation += 1;
                }
            }
            Key::Delete => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.delete_char();
                    self.chrome_generation += 1;
                }
            }
            Key::Left => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.move_cursor_left();
                    self.chrome_generation += 1;
                }
            }
            Key::Right => {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.move_cursor_right();
                    self.chrome_generation += 1;
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut gs) = self.git_switcher {
                        gs.insert_char(ch);
                        self.chrome_generation += 1;
                    }
                }
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    fn handle_file_switcher_key(&mut self, key: Key, modifiers: &Modifiers) {
        match key {
            Key::Escape => {
                self.file_switcher = None;
            }
            Key::Enter => {
                let selected_pane_id = self
                    .file_switcher
                    .as_ref()
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
            Key::Up => {
                if let Some(ref mut fs) = self.file_switcher {
                    fs.select_up();
                    self.chrome_generation += 1;
                }
            }
            Key::Down => {
                if let Some(ref mut fs) = self.file_switcher {
                    fs.select_down();
                    let visible_rows = 10usize;
                    if fs.selected >= fs.scroll_offset + visible_rows {
                        fs.scroll_offset = fs.selected.saturating_sub(visible_rows - 1);
                    }
                    self.chrome_generation += 1;
                }
            }
            Key::Backspace => {
                if let Some(ref mut fs) = self.file_switcher {
                    fs.backspace();
                    self.chrome_generation += 1;
                }
            }
            Key::Char(ch) => {
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

    fn handle_file_finder_key(&mut self, key: Key, modifiers: &Modifiers) {
        if (modifiers.meta || modifiers.ctrl)
            && matches!(key, Key::Char('k') | Key::Char('K'))
        {
            if let Some(ref mut finder) = self.file_finder {
                finder.select_up();
                self.chrome_generation += 1;
            }
            self.needs_redraw = true;
            return;
        }
        if (modifiers.meta || modifiers.ctrl)
            && matches!(key, Key::Char('j') | Key::Char('J'))
        {
            if let Some(ref mut finder) = self.file_finder {
                finder.select_down();
                self.chrome_generation += 1;
            }
            self.needs_redraw = true;
            return;
        }
        match key {
            Key::Escape => {
                self.close_file_finder();
            }
            Key::Enter => {
                let path = self.file_finder.as_ref().and_then(|f| f.selected_path());
                self.close_file_finder();
                if let Some(path) = path {
                    self.open_editor_pane(path);
                }
            }
            Key::Up => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.select_up();
                    self.chrome_generation += 1;
                }
            }
            Key::Down => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.select_down();
                    self.chrome_generation += 1;
                }
            }
            Key::Backspace => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.backspace();
                    self.chrome_generation += 1;
                }
            }
            Key::Delete => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.delete_char();
                    self.chrome_generation += 1;
                }
            }
            Key::Left => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.move_cursor_left();
                }
            }
            Key::Right => {
                if let Some(ref mut finder) = self.file_finder {
                    finder.move_cursor_right();
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut finder) = self.file_finder {
                        finder.insert_char(ch);
                        self.chrome_generation += 1;
                    }
                }
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    fn handle_save_as_key(&mut self, key: Key, modifiers: &Modifiers) {
        match key {
            Key::Escape => {
                self.save_as_input = None;
            }
            Key::Tab => {
                if let Some(ref mut input) = self.save_as_input {
                    input.toggle_field();
                }
            }
            Key::Enter => {
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
            Key::Backspace => {
                if let Some(ref mut input) = self.save_as_input {
                    input.backspace();
                }
            }
            Key::Delete => {
                if let Some(ref mut input) = self.save_as_input {
                    input.delete_char();
                }
            }
            Key::Left => {
                if let Some(ref mut input) = self.save_as_input {
                    input.move_cursor_left();
                }
            }
            Key::Right => {
                if let Some(ref mut input) = self.save_as_input {
                    input.move_cursor_right();
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut input) = self.save_as_input {
                        input.insert_char(ch);
                    }
                }
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    fn handle_context_menu_key(&mut self, key: Key) {
        match key {
            Key::Escape => {
                self.context_menu = None;
            }
            Key::Up => {
                if let Some(ref mut menu) = self.context_menu {
                    if menu.selected > 0 {
                        menu.selected -= 1;
                    }
                }
            }
            Key::Down => {
                if let Some(ref mut menu) = self.context_menu {
                    if menu.selected + 1 < menu.items().len() {
                        menu.selected += 1;
                    }
                }
            }
            Key::Enter => {
                let selected = self.context_menu.as_ref().map(|m| m.selected);
                if let Some(idx) = selected {
                    self.execute_context_menu_action(idx);
                }
                self.context_menu = None;
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    fn handle_file_tree_rename_key(&mut self, key: Key, modifiers: &Modifiers) {
        match key {
            Key::Escape => {
                self.file_tree_rename = None;
                self.chrome_generation += 1;
            }
            Key::Enter => {
                self.complete_file_tree_rename();
            }
            Key::Backspace => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    rename.input.backspace();
                    self.chrome_generation += 1;
                }
            }
            Key::Delete => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    rename.input.delete_char();
                    self.chrome_generation += 1;
                }
            }
            Key::Left => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    rename.input.move_cursor_left();
                    self.chrome_generation += 1;
                }
            }
            Key::Right => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    rename.input.move_cursor_right();
                    self.chrome_generation += 1;
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut rename) = self.file_tree_rename {
                        rename.input.insert_char(ch);
                        self.chrome_generation += 1;
                    }
                }
            }
            _ => {}
        }
        self.needs_redraw = true;
    }

    fn handle_file_tree_nav_key(&mut self, key: Key, _modifiers: &Modifiers) {
        let entry_count = self
            .file_tree
            .as_ref()
            .map(|t| t.visible_entries().len())
            .unwrap_or(0);
        if entry_count == 0 {
            self.needs_redraw = true;
            return;
        }

        match key {
            Key::Char('j') | Key::Down => {
                if self.file_tree_cursor + 1 < entry_count {
                    self.file_tree_cursor += 1;
                    self.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            Key::Char('k') | Key::Up => {
                if self.file_tree_cursor > 0 {
                    self.file_tree_cursor -= 1;
                    self.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            Key::Char('g') => {
                self.file_tree_cursor = 0;
                self.chrome_generation += 1;
                self.auto_scroll_file_tree_cursor();
            }
            Key::Char('G') => {
                if entry_count > 0 {
                    self.file_tree_cursor = entry_count - 1;
                    self.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            Key::Enter => {
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
            _ => {}
        }
        self.needs_redraw = true;
    }

    pub(crate) fn auto_scroll_file_tree_cursor(&mut self) {
        if let Some(tree_rect) = self.file_tree_rect {
            let cell_size = self.cell_size();
            let line_height = cell_size.height * crate::theme::FILE_TREE_LINE_SPACING;
            let padding = crate::theme::PANE_PADDING;

            let cursor_y = padding + self.file_tree_cursor as f32 * line_height;
            let visible_top = self.file_tree_scroll;
            let visible_bottom = self.file_tree_scroll + tree_rect.height - padding * 2.0;

            if cursor_y < visible_top {
                self.file_tree_scroll_target = cursor_y;
                self.file_tree_scroll = cursor_y;
            } else if cursor_y + line_height > visible_bottom {
                self.file_tree_scroll_target =
                    cursor_y + line_height - (tree_rect.height - padding * 2.0);
                self.file_tree_scroll = self.file_tree_scroll_target;
            }
        }
    }

    fn handle_config_page_key(&mut self, key: Key, modifiers: &Modifiers) {
        use crate::ui_state::ConfigSection;

        let page = match self.config_page.as_mut() {
            Some(p) => p,
            None => return,
        };

        if page.recording.is_some() {
            if matches!(key, Key::Escape) {
                page.recording = None;
            } else {
                let hotkey = tide_input::Hotkey::new(
                    key,
                    modifiers.shift,
                    modifiers.ctrl,
                    modifiers.meta,
                    modifiers.alt,
                );
                let Some(recording) = page.recording.as_ref() else { return };
                let action_index = recording.action_index;
                if action_index < page.bindings.len() {
                    for (i, (_, existing)) in page.bindings.iter_mut().enumerate() {
                        if i != action_index && *existing == hotkey {
                            *existing = tide_input::Hotkey::new(
                                Key::Char('?'),
                                false,
                                false,
                                false,
                                false,
                            );
                        }
                    }
                    page.bindings[action_index].1 = hotkey;
                    page.dirty = true;
                }
                page.recording = None;
            }
            self.chrome_generation += 1;
            self.needs_redraw = true;
            return;
        }

        if page.copy_files_editing {
            match key {
                Key::Escape | Key::Enter => {
                    page.copy_files_editing = false;
                    page.dirty = true;
                }
                Key::Backspace => {
                    page.copy_files_input.backspace();
                    page.dirty = true;
                }
                Key::Delete => {
                    page.copy_files_input.delete_char();
                    page.dirty = true;
                }
                Key::Left => {
                    page.copy_files_input.move_cursor_left();
                }
                Key::Right => {
                    page.copy_files_input.move_cursor_right();
                }
                Key::Char(ch) => {
                    if !modifiers.ctrl && !modifiers.meta {
                        page.copy_files_input.insert_char(ch);
                        page.dirty = true;
                    }
                }
                _ => {}
            }
            self.chrome_generation += 1;
            self.needs_redraw = true;
            return;
        }

        if page.worktree_editing {
            match key {
                Key::Escape | Key::Enter => {
                    page.worktree_editing = false;
                    page.dirty = true;
                }
                Key::Backspace => {
                    page.worktree_input.backspace();
                    page.dirty = true;
                }
                Key::Delete => {
                    page.worktree_input.delete_char();
                    page.dirty = true;
                }
                Key::Left => {
                    page.worktree_input.move_cursor_left();
                }
                Key::Right => {
                    page.worktree_input.move_cursor_right();
                }
                Key::Char(ch) => {
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
            Key::Escape => {
                self.close_config_page();
            }
            Key::Tab => {
                if let Some(page) = self.config_page.as_mut() {
                    page.section = match page.section {
                        ConfigSection::Keybindings => ConfigSection::Worktree,
                        ConfigSection::Worktree => ConfigSection::Keybindings,
                    };
                    page.selected = 0;
                    page.scroll_offset = 0;
                }
            }
            Key::Up | Key::Char('k') => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(page) = self.config_page.as_mut() {
                        match page.section {
                            ConfigSection::Keybindings => {
                                if page.selected > 0 {
                                    page.selected -= 1;
                                    if page.selected < page.scroll_offset {
                                        page.scroll_offset = page.selected;
                                    }
                                }
                            }
                            ConfigSection::Worktree => {
                                if page.selected_field > 0 {
                                    page.selected_field -= 1;
                                }
                            }
                        }
                    }
                }
            }
            Key::Down | Key::Char('j') => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(page) = self.config_page.as_mut() {
                        match page.section {
                            ConfigSection::Keybindings => {
                                if page.selected + 1 < page.bindings.len() {
                                    page.selected += 1;
                                    let max_visible = crate::theme::CONFIG_PAGE_MAX_VISIBLE;
                                    if page.selected >= page.scroll_offset + max_visible {
                                        page.scroll_offset =
                                            page.selected.saturating_sub(max_visible - 1);
                                    }
                                }
                            }
                            ConfigSection::Worktree => {
                                if page.selected_field < 1 {
                                    page.selected_field += 1;
                                }
                            }
                        }
                    }
                }
            }
            Key::Enter => {
                if let Some(page) = self.config_page.as_mut() {
                    match page.section {
                        ConfigSection::Keybindings => {
                            page.recording = Some(crate::RecordingState {
                                action_index: page.selected,
                            });
                        }
                        ConfigSection::Worktree => {
                            match page.selected_field {
                                0 => page.worktree_editing = true,
                                1 => page.copy_files_editing = true,
                                _ => {}
                            }
                        }
                    }
                }
            }
            Key::Backspace => {
                if let Some(page) = self.config_page.as_mut() {
                    if page.section == ConfigSection::Keybindings
                        && page.selected < page.bindings.len()
                    {
                        let action = &page.bindings[page.selected].0;
                        let defaults = tide_input::KeybindingMap::default_bindings();
                        if let Some((dh, _)) = defaults
                            .iter()
                            .find(|(_, da)| da.action_key() == action.action_key())
                        {
                            page.bindings[page.selected].1 = dh.clone();
                            page.dirty = true;
                        }
                    }
                }
            }
            _ => {}
        }
        self.chrome_generation += 1;
        self.needs_redraw = true;
    }

    fn handle_browser_url_bar_key(
        &mut self,
        pane_id: tide_core::PaneId,
        key: Key,
        modifiers: &Modifiers,
    ) {
        match key {
            Key::Enter => {
                // Navigate to URL and unfocus
                let url = if let Some(PaneKind::Browser(bp)) = self.panes.get(&pane_id) {
                    bp.url_input.clone()
                } else {
                    return;
                };
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&pane_id) {
                    bp.url_input_focused = false;
                    bp.navigate(&url);
                }
            }
            Key::Escape => {
                // Unfocus, revert text to current URL
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&pane_id) {
                    bp.url_input = bp.url.clone();
                    bp.url_input_cursor = bp.url_input.chars().count();
                    bp.url_input_focused = false;
                }
            }
            Key::Backspace => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&pane_id) {
                    if bp.url_input_cursor > 0 {
                        bp.url_input_cursor -= 1;
                        let byte_off = bp.cursor_byte_offset();
                        bp.url_input.remove(byte_off);
                    }
                }
            }
            Key::Delete => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&pane_id) {
                    if bp.url_input_cursor < bp.url_input_char_len() {
                        let byte_off = bp.cursor_byte_offset();
                        bp.url_input.remove(byte_off);
                    }
                }
            }
            Key::Left => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&pane_id) {
                    if bp.url_input_cursor > 0 {
                        bp.url_input_cursor -= 1;
                    }
                }
            }
            Key::Right => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&pane_id) {
                    if bp.url_input_cursor < bp.url_input_char_len() {
                        bp.url_input_cursor += 1;
                    }
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&pane_id) {
                        let byte_off = bp.cursor_byte_offset();
                        bp.url_input.insert(byte_off, ch);
                        bp.url_input_cursor += 1;
                    }
                }
            }
            _ => {}
        }
        self.chrome_generation += 1;
        self.needs_redraw = true;
    }

    fn handle_search_bar_key(
        &mut self,
        search_pane_id: tide_core::PaneId,
        key: Key,
        modifiers: &Modifiers,
    ) {
        if matches!(key, Key::Char('f') | Key::Char('F'))
            && (modifiers.meta || modifiers.ctrl)
            && !(modifiers.meta && modifiers.ctrl)
        {
            match self.panes.get_mut(&search_pane_id) {
                Some(PaneKind::Terminal(pane)) => {
                    pane.search = None;
                }
                Some(PaneKind::Editor(pane)) => {
                    pane.search = None;
                }
                Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
                None => {}
            }
            self.search_focus = None;
            return;
        }

        match key {
            Key::Escape => {
                match self.panes.get_mut(&search_pane_id) {
                    Some(PaneKind::Terminal(pane)) => {
                        pane.search = None;
                    }
                    Some(PaneKind::Editor(pane)) => {
                        pane.search = None;
                    }
                    Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) => {}
                    None => {}
                }
                self.search_focus = None;
            }
            Key::Enter => {
                if modifiers.shift {
                    self.search_prev_match(search_pane_id);
                } else {
                    self.search_next_match(search_pane_id);
                }
            }
            Key::Backspace => {
                self.search_bar_backspace(search_pane_id);
            }
            Key::Delete => {
                self.search_bar_delete(search_pane_id);
            }
            Key::Left => {
                self.search_bar_cursor_left(search_pane_id);
            }
            Key::Right => {
                self.search_bar_cursor_right(search_pane_id);
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    self.search_bar_insert(search_pane_id, ch);
                }
            }
            _ => {}
        }
    }
}
