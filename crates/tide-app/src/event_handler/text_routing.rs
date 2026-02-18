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
    ConfigPageWorktree,
    FileTreeRename,
    GitSwitcher,
    FileSwitcher,
    FileFinder,
    SaveAsInput,
    SearchBar(tide_core::PaneId),
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
            return if page.worktree_editing {
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

    /// Route a text string to the current input target.
    /// Handles all side effects (chrome_generation, input_sent_at, scroll-to-bottom, etc.).
    pub(crate) fn send_text_to_target(&mut self, text: &str) {
        let target = self.text_input_target();
        match target {
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
            TextInputTarget::Pane(id) => {
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
                        for ch in text.chars() {
                            pane.editor
                                .handle_action(tide_editor::EditorActionKind::InsertChar(ch));
                        }
                    }
                    Some(PaneKind::Diff(_)) | None => {}
                }
            }
            TextInputTarget::Consumed => {}
        }
        self.needs_redraw = true;
    }

    /// Reset all IME-related state.
    /// Call on window focus regain, FocusArea transitions, and IME disable.
    pub(crate) fn reset_ime_state(&mut self) {
        self.ime_active = false;
        self.ime_composing = false;
        self.ime_preedit.clear();
        self.pending_hangul_initial = None;
        self.ime_dropped_preedit = None;
        self.last_pressed_with_text = None;
        self.ime_just_committed = false;
        self.ime_committed_physical_key = None;
    }
}
