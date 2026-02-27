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
    FileSwitcher,
    FileFinder,
    PanelPicker,
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
        if let Some(ref page) = self.config_page {
            return if page.copy_files_editing {
                TextInputTarget::ConfigPageCopyFiles
            } else if page.worktree_editing {
                TextInputTarget::ConfigPageWorktree
            } else {
                TextInputTarget::Consumed
            };
        }
        if self.context_menu.is_some() || self.save_confirm.is_some() {
            return TextInputTarget::Consumed;
        }
        // Text-input popups
        if self.file_tree_rename.is_some() {
            return TextInputTarget::FileTreeRename;
        }
        if self.git_switcher.is_some() {
            return TextInputTarget::GitSwitcher;
        }
        if self.file_switcher.is_some() {
            return TextInputTarget::FileSwitcher;
        }
        if self.file_finder.is_some() {
            return TextInputTarget::FileFinder;
        }
        if self.panel_picker.is_some() {
            return TextInputTarget::PanelPicker;
        }
        if self.save_as_input.is_some() {
            return TextInputTarget::SaveAsInput;
        }
        // Inline search bar
        if let Some(id) = self.search_focus {
            return TextInputTarget::SearchBar(id);
        }
        // Focus area
        match self.focus_area {
            FocusArea::FileTree => TextInputTarget::Consumed,
            FocusArea::EditorDock => {
                if let Some(id) = self.active_editor_tab() {
                    if let Some(PaneKind::Browser(bp)) = self.panes.get(&id) {
                        if bp.url_input_focused {
                            return TextInputTarget::BrowserUrlBar(id);
                        }
                        // When URL bar not focused, consume text (webview handles its own input)
                        return TextInputTarget::Consumed;
                    }
                    if let Some(PaneKind::App(_)) = self.panes.get(&id) {
                        // External app handles its own input
                        return TextInputTarget::Consumed;
                    }
                }
                let id = self.active_editor_tab().or(self.focused);
                id.map(TextInputTarget::Pane)
                    .unwrap_or(TextInputTarget::Consumed)
            }
            FocusArea::PaneArea => self
                .focused
                .map(TextInputTarget::Pane)
                .unwrap_or(TextInputTarget::Consumed),
        }
    }

    /// Compute visible editor rows and columns for a given pane.
    /// Used by text routing and IME commit paths to keep cursor visible.
    pub(crate) fn visible_editor_size(&self, pane_id: tide_core::PaneId) -> (usize, usize) {
        let cs = self.cached_cell_size;
        let content_top = self.pane_area_mode.content_top();
        let tree_rect = self.visual_pane_rects.iter()
            .find(|(pid, _)| *pid == pane_id)
            .map(|(_, r)| *r);
        if let Some(r) = tree_rect {
            let rows = ((r.height - content_top - crate::theme::PANE_PADDING) / cs.height).floor() as usize;
            let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
            let cols = ((r.width - 2.0 * crate::theme::PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
            (rows.max(1), cols.max(1))
        } else if let Some(pr) = self.editor_panel_rect {
            let content_height = (pr.height - crate::theme::PANE_PADDING - crate::theme::PANEL_TAB_HEIGHT - crate::theme::PANE_GAP - crate::theme::PANE_PADDING).max(1.0);
            let rows = (content_height / cs.height).floor() as usize;
            let gutter_width = crate::editor_pane::GUTTER_WIDTH_CELLS as f32 * cs.width;
            let cols = ((pr.width - 2.0 * crate::theme::PANE_PADDING - 2.0 * gutter_width) / cs.width).floor() as usize;
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
                if let Some(ref mut page) = self.config_page {
                    for ch in text.chars() {
                        page.copy_files_input.insert_char(ch);
                    }
                    page.dirty = true;
                    self.chrome_generation += 1;
                }
            }
            TextInputTarget::ConfigPageWorktree => {
                if let Some(ref mut page) = self.config_page {
                    for ch in text.chars() {
                        page.worktree_input.insert_char(ch);
                    }
                    page.dirty = true;
                    self.chrome_generation += 1;
                }
            }
            TextInputTarget::FileTreeRename => {
                if let Some(ref mut rename) = self.file_tree_rename {
                    for ch in text.chars() {
                        rename.input.insert_char(ch);
                    }
                    self.chrome_generation += 1;
                }
            }
            TextInputTarget::GitSwitcher => {
                if let Some(ref mut gs) = self.git_switcher {
                    for ch in text.chars() {
                        gs.insert_char(ch);
                    }
                    self.chrome_generation += 1;
                }
            }
            TextInputTarget::FileSwitcher => {
                if let Some(ref mut fs) = self.file_switcher {
                    for ch in text.chars() {
                        fs.insert_char(ch);
                    }
                    self.chrome_generation += 1;
                }
            }
            TextInputTarget::FileFinder => {
                if let Some(ref mut finder) = self.file_finder {
                    for ch in text.chars() {
                        finder.insert_char(ch);
                    }
                    self.chrome_generation += 1;
                }
            }
            TextInputTarget::PanelPicker => {
                if let Some(ref mut pp) = self.panel_picker {
                    for ch in text.chars() {
                        pp.insert_char(ch);
                    }
                    self.chrome_generation += 1;
                }
            }
            TextInputTarget::SaveAsInput => {
                if let Some(ref mut input) = self.save_as_input {
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
                self.chrome_generation += 1;
            }
            TextInputTarget::Pane(id) => {
                // Block text input in preview mode
                if let Some(PaneKind::Editor(pane)) = self.panes.get(&id) {
                    if pane.preview_mode {
                        self.needs_redraw = true;
                        return;
                    }
                }
                // Compute visible size before mutable borrow of panes
                let editor_size = self.visible_editor_size(id);
                match self.panes.get_mut(&id) {
                    Some(PaneKind::Terminal(pane)) => {
                        if pane.backend.display_offset() > 0 {
                            pane.backend.request_scroll_to_bottom();
                        }
                        pane.backend.write(text.as_bytes());
                        self.input_just_sent = true;
                        self.input_sent_at = Some(Instant::now());
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
                        // Redraw tab label when modified indicator changes
                        if pane.editor.is_modified() != was_modified {
                            self.chrome_generation += 1;
                        }
                        // Editor has no PTY output loop — must invalidate cache explicitly
                        self.pane_generations.remove(&id);
                    }
                    Some(PaneKind::Diff(_)) | Some(PaneKind::Browser(_)) | Some(PaneKind::App(_)) | None => {}
                }
            }
            TextInputTarget::Consumed => {}
        }
        self.needs_redraw = true;
    }

}
