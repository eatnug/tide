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
        if !matches!(self.interaction.pane_drag, PaneDragState::Idle) {
            if matches!(key, Key::Escape) {
                self.interaction.pane_drag = PaneDragState::Idle;
                self.cache.needs_redraw = true;
                return;
            }
        }

        // If the key produced text and no command modifiers are held,
        // route via the text input system.
        // Exception: skip text routing when the active editor is in preview mode,
        // so keys like j/k/d/u fall through to the preview scroll handler.
        if let Some(ref text) = chars {
            if !modifiers.meta && !modifiers.ctrl && !modifiers.alt {
                let in_preview = self.focused
                    .and_then(|id| self.panes.get(&id))
                    .map(|p| matches!(p, PaneKind::Editor(ep) if ep.preview_mode))
                    .unwrap_or(false);
                if !in_preview {
                    self.send_text_to_target(text);
                    self.cache.needs_redraw = true;
                    return;
                }
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
        if self.modal.config_page.is_some() {
            self.handle_config_page_key(key, &modifiers);
            return;
        }

        // Context menu interception
        if self.modal.context_menu.is_some() {
            self.handle_context_menu_key(key);
            return;
        }

        // File tree inline rename interception
        if self.modal.file_tree_rename.is_some() {
            self.handle_file_tree_rename_key(key, &modifiers);
            return;
        }

        // Git switcher popup interception
        if self.modal.git_switcher.is_some() {
            self.handle_git_switcher_key(key, &modifiers);
            return;
        }

        // File finder interception
        if self.modal.file_finder.is_some() {
            self.handle_file_finder_key(key, &modifiers);
            return;
        }

        // Save-as input interception
        if self.modal.save_as_input.is_some() {
            self.handle_save_as_key(key, &modifiers);
            return;
        }

        // Branch cleanup bar interception
        if let Some(ref bc) = self.modal.branch_cleanup {
            // Safety: clear stale state if the pane no longer exists
            if !self.panes.contains_key(&bc.pane_id) {
                self.modal.branch_cleanup = None;
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
                self.cache.needs_redraw = true;
                return;
            }
        }

        // Save confirm bar interception
        if self.modal.save_confirm.is_some() {
            if matches!(key, Key::Escape) {
                self.cancel_save_confirm();
            }
            self.cache.needs_redraw = true;
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
                    self.cache.needs_redraw = true;
                    return;
                }
                self.handle_file_tree_nav_key(key, &modifiers);
                return;
            }
            FocusArea::PaneArea => {
                // Browser URL bar keyboard handling
                if let Some(focused_id) = self.focused {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get(&focused_id) {
                        if bp.url_input_focused {
                            // Global hotkeys take priority over URL bar input
                            if modifiers.meta || (modifiers.ctrl && modifiers.shift) {
                                let input = InputEvent::KeyPress { key, modifiers };
                                let action = self.router.process(input, &self.pane_rects);
                                if !matches!(action, tide_input::Action::RouteToPane(_)) {
                                    self.handle_action(action, Some(input));
                                    self.cache.needs_redraw = true;
                                    return;
                                }
                            }
                            self.handle_browser_url_bar_key(focused_id, key, &modifiers);
                            return;
                        }
                        // Cmd+L → focus URL bar
                        if modifiers.meta && matches!(key, Key::Char('l') | Key::Char('L')) {
                            if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&focused_id) {
                                bp.url_input_focused = true;
                                bp.url_input = bp.url.clone();
                                bp.url_input_cursor = bp.url_input.chars().count();
                                self.cache.chrome_generation += 1;
                                self.cache.needs_redraw = true;
                            }
                            return;
                        }
                    }
                }

                // Search bar interception (before routing to pane)
                if let Some(search_pane_id) = self.search_focus {
                    self.handle_search_bar_key(search_pane_id, key, &modifiers);
                    return;
                }

                // Fall through to normal routing
            }
        }

        let input = InputEvent::KeyPress { key, modifiers };
        let action = self.router.process(input, &self.pane_rects);
        self.handle_action(action, Some(input));
        self.cache.needs_redraw = true;
    }

    fn handle_git_switcher_key(&mut self, key: Key, modifiers: &Modifiers) {
        // Cmd+Backspace → delete selected item
        if matches!(key, Key::Backspace) && modifiers.meta && !modifiers.ctrl && !modifiers.alt {
            let selected = self.modal.git_switcher.as_ref().map(|gs| gs.selected);
            if let Some(selected) = selected {
                self.handle_git_switcher_button(crate::SwitcherButton::Delete(selected));
            }
            return;
        }

        match key {
            Key::Escape => {
                // If delete confirmation is active, cancel it first
                if let Some(ref mut gs) = self.modal.git_switcher {
                    if gs.delete_confirm.is_some() {
                        gs.delete_confirm = None;
                        self.cache.chrome_generation += 1;
                        self.cache.needs_redraw = true;
                        return;
                    }
                }
                self.modal.git_switcher = None;
            }
            Key::Tab => {
                if let Some(ref mut gs) = self.modal.git_switcher {
                    gs.delete_confirm = None;
                    gs.toggle_mode();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Enter => {
                let info = self.modal.git_switcher.as_ref().map(|gs| (gs.selected, gs.mode));
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
                if let Some(ref mut gs) = self.modal.git_switcher {
                    gs.delete_confirm = None;
                    gs.select_up();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Down => {
                if let Some(ref mut gs) = self.modal.git_switcher {
                    gs.delete_confirm = None;
                    gs.select_down();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Backspace => {
                if let Some(ref mut gs) = self.modal.git_switcher {
                    gs.delete_confirm = None;
                    gs.backspace();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Delete => {
                if let Some(ref mut gs) = self.modal.git_switcher {
                    gs.delete_char();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Left => {
                if let Some(ref mut gs) = self.modal.git_switcher {
                    gs.move_cursor_left();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Right => {
                if let Some(ref mut gs) = self.modal.git_switcher {
                    gs.move_cursor_right();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut gs) = self.modal.git_switcher {
                        gs.insert_char(ch);
                        self.cache.chrome_generation += 1;
                    }
                }
            }
            _ => {}
        }
        self.cache.needs_redraw = true;
    }

    fn handle_file_finder_key(&mut self, key: Key, modifiers: &Modifiers) {
        if (modifiers.meta || modifiers.ctrl)
            && matches!(key, Key::Char('k') | Key::Char('K'))
        {
            if let Some(ref mut finder) = self.modal.file_finder {
                finder.select_up();
                self.cache.chrome_generation += 1;
            }
            self.cache.needs_redraw = true;
            return;
        }
        if (modifiers.meta || modifiers.ctrl)
            && matches!(key, Key::Char('j') | Key::Char('J'))
        {
            if let Some(ref mut finder) = self.modal.file_finder {
                finder.select_down();
                self.cache.chrome_generation += 1;
            }
            self.cache.needs_redraw = true;
            return;
        }
        match key {
            Key::Escape => {
                self.close_file_finder();
            }
            Key::Enter => {
                let path = self.modal.file_finder.as_ref().and_then(|f| f.selected_path());
                let replace_id = self.modal.file_finder.as_ref().and_then(|f| f.replace_pane_id);
                self.close_file_finder();
                if let Some(path) = path {
                    if let Some(pane_id) = replace_id {
                        // Replace the launcher pane with an editor for the selected file
                        self.replace_pane_with_editor(pane_id, path);
                    } else {
                        self.open_editor_pane(path);
                    }
                }
            }
            Key::Up => {
                if let Some(ref mut finder) = self.modal.file_finder {
                    finder.select_up();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Down => {
                if let Some(ref mut finder) = self.modal.file_finder {
                    finder.select_down();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Backspace => {
                if let Some(ref mut finder) = self.modal.file_finder {
                    finder.backspace();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Delete => {
                if let Some(ref mut finder) = self.modal.file_finder {
                    finder.delete_char();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Left => {
                if let Some(ref mut finder) = self.modal.file_finder {
                    finder.move_cursor_left();
                }
            }
            Key::Right => {
                if let Some(ref mut finder) = self.modal.file_finder {
                    finder.move_cursor_right();
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut finder) = self.modal.file_finder {
                        finder.insert_char(ch);
                        self.cache.chrome_generation += 1;
                    }
                }
            }
            _ => {}
        }
        self.cache.needs_redraw = true;
    }

    fn handle_save_as_key(&mut self, key: Key, modifiers: &Modifiers) {
        match key {
            Key::Escape => {
                self.modal.save_as_input = None;
            }
            Key::Tab => {
                if let Some(ref mut input) = self.modal.save_as_input {
                    input.toggle_field();
                }
            }
            Key::Enter => {
                let resolved = self.modal.save_as_input.as_ref().and_then(|input| {
                    let pane_id = input.pane_id;
                    input.resolve_path().map(|p| (pane_id, p))
                });
                self.modal.save_as_input = None;
                if let Some((pane_id, path)) = resolved {
                    let path_str = path.to_string_lossy().to_string();
                    self.complete_save_as(pane_id, &path_str);
                }
            }
            Key::Backspace => {
                if let Some(ref mut input) = self.modal.save_as_input {
                    input.backspace();
                }
            }
            Key::Delete => {
                if let Some(ref mut input) = self.modal.save_as_input {
                    input.delete_char();
                }
            }
            Key::Left => {
                if let Some(ref mut input) = self.modal.save_as_input {
                    input.move_cursor_left();
                }
            }
            Key::Right => {
                if let Some(ref mut input) = self.modal.save_as_input {
                    input.move_cursor_right();
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut input) = self.modal.save_as_input {
                        input.insert_char(ch);
                    }
                }
            }
            _ => {}
        }
        self.cache.needs_redraw = true;
    }

    fn handle_context_menu_key(&mut self, key: Key) {
        match key {
            Key::Escape => {
                self.modal.context_menu = None;
            }
            Key::Up => {
                if let Some(ref mut menu) = self.modal.context_menu {
                    if menu.selected > 0 {
                        menu.selected -= 1;
                    }
                }
            }
            Key::Down => {
                if let Some(ref mut menu) = self.modal.context_menu {
                    if menu.selected + 1 < menu.items().len() {
                        menu.selected += 1;
                    }
                }
            }
            Key::Enter => {
                let selected = self.modal.context_menu.as_ref().map(|m| m.selected);
                if let Some(idx) = selected {
                    self.execute_context_menu_action(idx);
                }
                self.modal.context_menu = None;
            }
            _ => {}
        }
        self.cache.needs_redraw = true;
    }

    fn handle_file_tree_rename_key(&mut self, key: Key, modifiers: &Modifiers) {
        match key {
            Key::Escape => {
                self.modal.file_tree_rename = None;
                self.cache.chrome_generation += 1;
            }
            Key::Enter => {
                self.complete_file_tree_rename();
            }
            Key::Backspace => {
                if let Some(ref mut rename) = self.modal.file_tree_rename {
                    rename.input.backspace();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Delete => {
                if let Some(ref mut rename) = self.modal.file_tree_rename {
                    rename.input.delete_char();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Left => {
                if let Some(ref mut rename) = self.modal.file_tree_rename {
                    rename.input.move_cursor_left();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Right => {
                if let Some(ref mut rename) = self.modal.file_tree_rename {
                    rename.input.move_cursor_right();
                    self.cache.chrome_generation += 1;
                }
            }
            Key::Char(ch) => {
                if !modifiers.ctrl && !modifiers.meta {
                    if let Some(ref mut rename) = self.modal.file_tree_rename {
                        rename.input.insert_char(ch);
                        self.cache.chrome_generation += 1;
                    }
                }
            }
            _ => {}
        }
        self.cache.needs_redraw = true;
    }

    fn handle_file_tree_nav_key(&mut self, key: Key, _modifiers: &Modifiers) {
        let entry_count = self
            .ft.tree
            .as_ref()
            .map(|t| t.visible_entries().len())
            .unwrap_or(0);
        if entry_count == 0 {
            self.cache.needs_redraw = true;
            return;
        }

        match key {
            Key::Char('j') | Key::Down => {
                if self.ft.cursor + 1 < entry_count {
                    self.ft.cursor += 1;
                    self.cache.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            Key::Char('k') | Key::Up => {
                if self.ft.cursor > 0 {
                    self.ft.cursor -= 1;
                    self.cache.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            Key::Char('g') => {
                self.ft.cursor = 0;
                self.cache.chrome_generation += 1;
                self.auto_scroll_file_tree_cursor();
            }
            Key::Char('G') => {
                if entry_count > 0 {
                    self.ft.cursor = entry_count - 1;
                    self.cache.chrome_generation += 1;
                    self.auto_scroll_file_tree_cursor();
                }
            }
            Key::Enter => {
                if let Some(tree) = &self.ft.tree {
                    let entries = tree.visible_entries();
                    if let Some(entry) = entries.get(self.ft.cursor) {
                        if entry.entry.is_dir {
                            let path = entry.entry.path.clone();
                            if let Some(tree) = &mut self.ft.tree {
                                tree.toggle(&path);
                            }
                            self.cache.chrome_generation += 1;
                        } else {
                            let path = entry.entry.path.clone();
                            self.open_editor_pane(path);
                        }
                    }
                }
            }
            _ => {}
        }
        self.cache.needs_redraw = true;
    }

    pub(crate) fn auto_scroll_file_tree_cursor(&mut self) {
        if let Some(tree_rect) = self.ft.rect {
            let cell_size = self.cell_size();
            let line_height = cell_size.height * crate::theme::FILE_TREE_LINE_SPACING;
            let padding = crate::theme::PANE_PADDING;

            let cursor_y = padding + self.ft.cursor as f32 * line_height;
            let visible_top = self.ft.scroll;
            let visible_bottom = self.ft.scroll + tree_rect.height - padding * 2.0;

            if cursor_y < visible_top {
                self.ft.scroll_target = cursor_y;
                self.ft.scroll = cursor_y;
            } else if cursor_y + line_height > visible_bottom {
                self.ft.scroll_target =
                    cursor_y + line_height - (tree_rect.height - padding * 2.0);
                self.ft.scroll = self.ft.scroll_target;
            }
        }
    }

    fn handle_config_page_key(&mut self, key: Key, modifiers: &Modifiers) {
        use crate::ui_state::ConfigSection;

        let page = match self.modal.config_page.as_mut() {
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
            self.cache.chrome_generation += 1;
            self.cache.needs_redraw = true;
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
            self.cache.chrome_generation += 1;
            self.cache.needs_redraw = true;
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
            self.cache.chrome_generation += 1;
            self.cache.needs_redraw = true;
            return;
        }

        match key {
            Key::Escape => {
                self.close_config_page();
            }
            Key::Tab => {
                if let Some(page) = self.modal.config_page.as_mut() {
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
                    if let Some(page) = self.modal.config_page.as_mut() {
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
                    if let Some(page) = self.modal.config_page.as_mut() {
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
                if let Some(page) = self.modal.config_page.as_mut() {
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
                if let Some(page) = self.modal.config_page.as_mut() {
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
        self.cache.chrome_generation += 1;
        self.cache.needs_redraw = true;
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
        self.cache.chrome_generation += 1;
        self.cache.needs_redraw = true;
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
                Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) | Some(PaneKind::Launcher(_)) => {}
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
                    Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) | Some(PaneKind::Launcher(_)) => {}
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
