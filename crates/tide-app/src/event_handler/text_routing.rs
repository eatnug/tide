//! Unified text input routing.
//!
//! Every path that inserts text (IME Commit, keyboard Released handler,
//! future clipboard paste, etc.) calls `send_text_to_target()` which
//! uses `text_input_target()` to determine the single correct destination.

use std::time::Instant;

use tide_core::TerminalBackend;

use crate::pane::PaneKind;
use crate::ui_state::FocusArea;
use crate::App;

/// Where text input should be directed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextInputTarget {
    ConfigPageCopyFiles,
    ConfigPageWorktree,
    FileTreeRename,
    GitSwitcher,
    FileFinder,
    SaveAsInput,
    SearchBar(tide_core::PaneId),
    BrowserUrlBar(tide_core::PaneId),
    Pane(tide_core::PaneId),
    /// Input should be silently consumed (modal popup, file tree focus, etc.)
    Consumed,
}

impl App {
    /// Determine where text input should be routed based on current UI state.
    /// Checks modals/popups first (highest priority), then focus area.
    /// This is the single source of truth — keyboard, IME, and Released
    /// handlers all use this instead of maintaining separate if-else chains.
    pub(crate) fn text_input_target(&self) -> TextInputTarget {
        // Modal overlays (highest priority)
        if let Some(ref page) = self.modal.config_page {
            return if page.copy_files_editing {
                TextInputTarget::ConfigPageCopyFiles
            } else if page.worktree_editing {
                TextInputTarget::ConfigPageWorktree
            } else {
                TextInputTarget::Consumed
            };
        }
        if self.modal.context_menu.is_some() || self.modal.save_confirm.is_some() {
            return TextInputTarget::Consumed;
        }
        // Text-input popups
        if self.modal.file_tree_rename.is_some() {
            return TextInputTarget::FileTreeRename;
        }
        if self.modal.git_switcher.is_some() {
            return TextInputTarget::GitSwitcher;
        }
        if self.modal.file_finder.is_some() {
            return TextInputTarget::FileFinder;
        }
        if self.modal.save_as_input.is_some() {
            return TextInputTarget::SaveAsInput;
        }
        // Inline search bar
        if let Some(id) = self.search_focus {
            return TextInputTarget::SearchBar(id);
        }
        // Focus area
        match self.focus_area {
            FocusArea::FileTree => TextInputTarget::Consumed,
            FocusArea::PaneArea => {
                // Check if focused pane is a browser with URL bar focused
                if let Some(id) = self.focused {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get(&id) {
                        if bp.url_input_focused {
                            return TextInputTarget::BrowserUrlBar(id);
                        }
                        // When URL bar not focused, consume text (webview handles its own input)
                        return TextInputTarget::Consumed;
                    }
                }
                self.focused
                    .map(TextInputTarget::Pane)
                    .unwrap_or(TextInputTarget::Consumed)
            }
        }
    }

    /// Compute visible editor rows and columns for a given pane.
    /// Used by text routing and IME commit paths to keep cursor visible.
    pub(crate) fn visible_editor_size(&self, pane_id: tide_core::PaneId) -> (usize, usize) {
        let cs = self.cached_cell_size;
        let content_top = crate::theme::TAB_BAR_HEIGHT;
        let tree_rect = self.visual_pane_rects.iter()
            .find(|(pid, _)| *pid == pane_id)
            .map(|(_, r)| *r);
        if let Some(r) = tree_rect {
            let rows = ((r.height - content_top - crate::theme::PANE_PADDING) / cs.height).floor() as usize;
            let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
            let cols = ((r.width - 2.0 * crate::theme::PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
            (rows.max(1), cols.max(1))
        } else {
            (30, 80)
        }
    }

    /// Route a text string to the current input target.
    /// Handles all side effects (chrome_generation, input_sent_at, scroll-to-bottom, etc.).
    pub(crate) fn send_text_to_target(&mut self, text: &str) {
        let target = self.text_input_target();
        match target {
            TextInputTarget::ConfigPageCopyFiles => {
                if let Some(ref mut page) = self.modal.config_page {
                    for ch in text.chars() {
                        page.copy_files_input.insert_char(ch);
                    }
                    page.dirty = true;
                    self.cache.invalidate_chrome();
                }
            }
            TextInputTarget::ConfigPageWorktree => {
                if let Some(ref mut page) = self.modal.config_page {
                    for ch in text.chars() {
                        page.worktree_input.insert_char(ch);
                    }
                    page.dirty = true;
                    self.cache.invalidate_chrome();
                }
            }
            TextInputTarget::FileTreeRename => {
                if let Some(ref mut rename) = self.modal.file_tree_rename {
                    for ch in text.chars() {
                        rename.input.insert_char(ch);
                    }
                    self.cache.invalidate_chrome();
                }
            }
            TextInputTarget::GitSwitcher => {
                if let Some(ref mut gs) = self.modal.git_switcher {
                    for ch in text.chars() {
                        gs.insert_char(ch);
                    }
                    self.cache.invalidate_chrome();
                }
            }
            TextInputTarget::FileFinder => {
                if let Some(ref mut finder) = self.modal.file_finder {
                    for ch in text.chars() {
                        finder.insert_char(ch);
                    }
                    self.cache.invalidate_chrome();
                }
            }
            TextInputTarget::SaveAsInput => {
                if let Some(ref mut input) = self.modal.save_as_input {
                    for ch in text.chars() {
                        input.insert_char(ch);
                    }
                }
            }
            TextInputTarget::SearchBar(pane_id) => {
                for ch in text.chars() {
                    self.search_bar_insert(pane_id, ch);
                }
            }
            TextInputTarget::BrowserUrlBar(pane_id) => {
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&pane_id) {
                    for ch in text.chars() {
                        let byte_off = bp.cursor_byte_offset();
                        bp.url_input.insert(byte_off, ch);
                        bp.url_input_cursor += 1;
                    }
                }
                self.cache.invalidate_chrome();
            }
            TextInputTarget::Pane(id) => {
                // Block text input in preview mode
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&id) {
                    if pane.preview_mode {
                        self.cache.needs_redraw = true;
                        return;
                    }
                }
                // Compute visible size before mutable borrow of panes
                let editor_size = self.visible_editor_size(id);
                match self.panes.get_mut(&id) {
                    Some(PaneKind::Terminal(pane)) => {
                        if pane.child_dead {
                            self.respawn_terminal(id);
                        } else {
                            if pane.backend.display_offset() > 0 {
                                pane.backend.request_scroll_to_bottom();
                            }
                            pane.backend.write(text.as_bytes());
                            self.input_just_sent = true;
                            self.input_sent_at = Some(Instant::now());
                        }
                    }
                    Some(PaneKind::Editor(pane)) => {
                        let was_modified = pane.editor.is_modified();
                        // Delete selection on editing input (mirrors keybinding path)
                        if text.chars().any(|ch| !ch.is_control() || ch == '\r' || ch == '\n' || ch == '\u{7f}' || ch == '\u{8}') {
                            pane.delete_selection();
                            pane.selection = None;
                        }
                        for ch in text.chars() {
                            // Map control characters to editor actions
                            let action = match ch {
                                '\u{7f}' | '\u{8}' => tide_editor::EditorActionKind::Backspace,
                                '\r' | '\n' => tide_editor::EditorActionKind::Enter,
                                ch if ch.is_control() => continue,
                                ch => tide_editor::EditorActionKind::InsertChar(ch),
                            };
                            pane.editor.handle_action(action);
                        }
                        // Ensure cursor stays visible after editing (matches keybinding path)
                        let (visible_rows, visible_cols) = editor_size;
                        pane.editor.ensure_cursor_visible(visible_rows);
                        pane.editor.ensure_cursor_visible_h(visible_cols);
                        // Update completion filter or trigger new completion
                        let has_printable = text.chars().any(|ch| !ch.is_control());
                        let mut should_trigger = false;
                        if has_printable {
                            if pane.completion.is_some() {
                                // Client-side filter: append to prefix and re-filter
                                if let Some(ref mut cs) = pane.completion {
                                    for ch in text.chars() {
                                        if !ch.is_control() {
                                            cs.prefix.push(ch);
                                        }
                                    }
                                    cs.apply_filter();
                                    if cs.is_empty() {
                                        pane.completion = None;
                                        // BR-9a: filter dismissed → re-trigger for this character
                                        // (e.g. "." after a word should start new completion)
                                        should_trigger = true;
                                    }
                                }
                            } else {
                                should_trigger = true;
                            }
                        } else {
                            // Non-printable (Enter, Backspace) dismisses completion
                            pane.completion = None;
                        }
                        // Redraw tab label when modified indicator changes
                        if pane.editor.is_modified() != was_modified {
                            self.cache.invalidate_chrome();
                        }
                        // Notify LSP of document change BEFORE requesting completion
                        // (server needs updated buffer to provide correct results)
                        self.notify_lsp_did_change(id);
                        // Check if this character is a trigger character (after borrow is released)
                        if should_trigger {
                            self.try_trigger_completion(id, text);
                        }
                        // Editor has no PTY output loop — must invalidate cache explicitly
                        self.cache.invalidate_pane(id);
                    }
                    Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) | Some(PaneKind::Launcher(_)) | None => {}
                }
            }
            TextInputTarget::Consumed => {}
        }
        self.cache.needs_redraw = true;
    }

}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui_state::*;
    use tide_core::Rect;
    use std::path::PathBuf;

    fn test_app() -> App {
        let mut app = App::new();
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    #[test]
    fn default_no_focus_consumed() {
        let app = test_app();
        assert_eq!(app.text_input_target(), TextInputTarget::Consumed);
    }

    #[test]
    fn focused_editor_routes_to_pane() {
        let mut app = test_app();
        let id: tide_core::PaneId = 1;
        app.panes.insert(id, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id)));
        app.focused = Some(id);
        assert_eq!(app.text_input_target(), TextInputTarget::Pane(id));
    }

    #[test]
    fn file_finder_overrides_pane() {
        let mut app = test_app();
        let id: tide_core::PaneId = 1;
        app.panes.insert(id, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/tmp"), vec![]));
        assert_eq!(app.text_input_target(), TextInputTarget::FileFinder);
    }

    #[test]
    fn git_switcher_overrides_pane() {
        let mut app = test_app();
        let id: tide_core::PaneId = 1;
        app.panes.insert(id, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.modal.git_switcher = Some(GitSwitcherState::new(
            id,
            GitSwitcherMode::Branches,
            vec![],
            vec![],
            Rect::new(0.0, 0.0, 100.0, 30.0),
        ));
        assert_eq!(app.text_input_target(), TextInputTarget::GitSwitcher);
    }

    #[test]
    fn config_page_consumed_by_default() {
        let mut app = test_app();
        app.modal.config_page = Some(ConfigPageState::new(vec![], String::new(), String::new()));
        assert_eq!(app.text_input_target(), TextInputTarget::Consumed);
    }

    #[test]
    fn config_page_copy_files_editing() {
        let mut app = test_app();
        let mut cp = ConfigPageState::new(vec![], String::new(), String::new());
        cp.copy_files_editing = true;
        app.modal.config_page = Some(cp);
        assert_eq!(app.text_input_target(), TextInputTarget::ConfigPageCopyFiles);
    }

    #[test]
    fn config_page_worktree_editing() {
        let mut app = test_app();
        let mut cp = ConfigPageState::new(vec![], String::new(), String::new());
        cp.worktree_editing = true;
        app.modal.config_page = Some(cp);
        assert_eq!(app.text_input_target(), TextInputTarget::ConfigPageWorktree);
    }

    #[test]
    fn config_page_overrides_file_finder() {
        let mut app = test_app();
        app.modal.file_finder = Some(FileFinderState::new(PathBuf::from("/tmp"), vec![]));
        app.modal.config_page = Some(ConfigPageState::new(vec![], String::new(), String::new()));
        // config_page has higher priority
        assert_eq!(app.text_input_target(), TextInputTarget::Consumed);
    }

    #[test]
    fn search_bar_routes_to_search() {
        let mut app = test_app();
        let id: tide_core::PaneId = 1;
        app.panes.insert(id, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.search_focus = Some(id);
        assert_eq!(app.text_input_target(), TextInputTarget::SearchBar(id));
    }

    #[test]
    fn file_tree_focus_consumed() {
        let mut app = test_app();
        let id: tide_core::PaneId = 1;
        app.panes.insert(id, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.focus_area = FocusArea::FileTree;
        assert_eq!(app.text_input_target(), TextInputTarget::Consumed);
    }

    #[test]
    fn save_as_input_routes() {
        let mut app = test_app();
        app.modal.save_as_input = Some(SaveAsInput::new(1, PathBuf::from("/tmp"), Rect::new(0.0, 0.0, 100.0, 30.0)));
        assert_eq!(app.text_input_target(), TextInputTarget::SaveAsInput);
    }

    #[test]
    fn file_tree_rename_routes() {
        let mut app = test_app();
        app.modal.file_tree_rename = Some(FileTreeRenameState {
            entry_index: 0,
            original_path: PathBuf::from("/tmp/file.txt"),
            input: InputLine::with_text("file.txt".to_string()),
        });
        assert_eq!(app.text_input_target(), TextInputTarget::FileTreeRename);
    }

    #[test]
    fn context_menu_consumed() {
        let mut app = test_app();
        let id: tide_core::PaneId = 1;
        app.panes.insert(id, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.modal.context_menu = Some(ContextMenuState {
            entry_index: 0,
            path: PathBuf::from("/tmp"),
            is_dir: false,
            shell_idle: true,
            position: tide_core::Vec2::new(0.0, 0.0),
            selected: 0,
        });
        assert_eq!(app.text_input_target(), TextInputTarget::Consumed);
    }

    #[test]
    fn priority_git_switcher_over_search_bar() {
        let mut app = test_app();
        let id: tide_core::PaneId = 1;
        app.panes.insert(id, PaneKind::Editor(crate::editor_pane::EditorPane::new_empty(id)));
        app.focused = Some(id);
        app.search_focus = Some(id);
        app.modal.git_switcher = Some(GitSwitcherState::new(
            id,
            GitSwitcherMode::Branches,
            vec![],
            vec![],
            Rect::new(0.0, 0.0, 100.0, 30.0),
        ));
        // git_switcher has higher priority than search_focus
        assert_eq!(app.text_input_target(), TextInputTarget::GitSwitcher);
    }
}
