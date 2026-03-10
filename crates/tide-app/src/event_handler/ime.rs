//! Native IME event handling.
//!
//! With the native NSTextInputClient implementation, the platform layer
//! handles all composition complexity. We just receive committed text
//! and preedit updates — no workarounds needed.

use crate::pane::PaneKind;
use crate::App;

impl App {
    /// Handle IME committed text (composition done).
    pub(crate) fn handle_ime_commit(&mut self, text: &str) {
        // Modal popups intercept text BEFORE any pane-level handling.
        // On macOS, all text arrives via ImeCommit, so popups must be checked here.
        if self.modal.file_finder.is_some() {
            for ch in text.chars() {
                if let Some(ref mut finder) = self.modal.file_finder {
                    finder.insert_char(ch);
                    self.cache.chrome_generation += 1;
                }
            }
            self.ime.composing = false;
            self.ime.preedit.clear();
            self.cache.needs_redraw = true;
            return;
        }
        if self.modal.save_as_input.is_some() {
            for ch in text.chars() {
                if let Some(ref mut input) = self.modal.save_as_input {
                    input.insert_char(ch);
                }
            }
            self.ime.composing = false;
            self.ime.preedit.clear();
            self.cache.needs_redraw = true;
            return;
        }
        if self.modal.git_switcher.is_some() {
            for ch in text.chars() {
                if let Some(ref mut gs) = self.modal.git_switcher {
                    gs.insert_char(ch);
                    self.cache.chrome_generation += 1;
                }
            }
            self.ime.composing = false;
            self.ime.preedit.clear();
            self.cache.needs_redraw = true;
            return;
        }
        // Browser URL bar text input
        if let Some(focused_id) = self.focused {
            if let Some(PaneKind::Browser(bp)) = self.panes.get(&focused_id) {
                if bp.url_input_focused {
                    for ch in text.chars() {
                        if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&focused_id) {
                            let byte_off = bp.cursor_byte_offset();
                            bp.url_input.insert(byte_off, ch);
                            bp.url_input_cursor += 1;
                        }
                    }
                    self.cache.chrome_generation += 1;
                    self.ime.composing = false;
                    self.ime.preedit.clear();
                    self.cache.needs_redraw = true;
                    return;
                }
            }
        }

        // Preview mode: ignore text input (scroll via Cmd+D/U and mouse/trackpad)
        if let Some(id) = self.focused {
            let is_preview = self
                .panes
                .get(&id)
                .map(|p| matches!(p, PaneKind::Editor(ep) if ep.preview_mode))
                .unwrap_or(false);
            if is_preview {
                self.ime.composing = false;
                self.ime.preedit.clear();
                self.cache.needs_redraw = true;
                return;
            }
        }
        // Launcher pane: intercept single-char text to resolve launcher choice.
        // On macOS, plain keys (t/e/o/b) arrive via ImeCommit, not KeyDown.
        if let Some(id) = self.focused {
            if matches!(self.panes.get(&id), Some(PaneKind::Launcher(_))) {
                for ch in text.chars() {
                    let choice = match ch {
                        't' | 'T' | 'ㅅ' => Some(crate::action::LauncherChoice::Terminal),
                        'e' | 'E' | 'ㄷ' => Some(crate::action::LauncherChoice::NewFile),
                        'o' | 'O' | 'ㅐ' => Some(crate::action::LauncherChoice::OpenFile),
                        'b' | 'B' | 'ㅠ' => Some(crate::action::LauncherChoice::Browser),
                        _ => None,
                    };
                    if let Some(c) = choice {
                        self.resolve_launcher(id, c);
                        self.ime.composing = false;
                        self.ime.preedit.clear();
                        self.cache.needs_redraw = true;
                        return;
                    }
                }
                // Non-matching text: ignore for launcher
                self.ime.composing = false;
                self.ime.preedit.clear();
                self.cache.needs_redraw = true;
                return;
            }
        }

        self.send_text_to_target(text);
        self.ime.composing = false;
        self.ime.preedit.clear();
        self.cache.needs_redraw = true;
    }

    /// Handle IME preedit update (composition in progress).
    pub(crate) fn handle_ime_preedit(&mut self, text: &str) {
        // Launcher pane: immediately resolve on preedit so Korean IME
        // doesn't require a second keystroke to commit the character.
        if !text.is_empty() {
            if let Some(id) = self.focused {
                if matches!(self.panes.get(&id), Some(PaneKind::Launcher(_))) {
                    let first_char = text.chars().next();
                    let choice = match first_char {
                        Some('ㅅ') => Some(crate::action::LauncherChoice::Terminal),
                        Some('ㄷ') => Some(crate::action::LauncherChoice::NewFile),
                        Some('ㅐ') => Some(crate::action::LauncherChoice::OpenFile),
                        Some('ㅠ') => Some(crate::action::LauncherChoice::Browser),
                        _ => None,
                    };
                    if let Some(c) = choice {
                        self.resolve_launcher(id, c);
                        self.ime.composing = false;
                        self.ime.preedit.clear();
                        self.cache.needs_redraw = true;
                        return;
                    }
                }
            }
        }

        self.ime.composing = !text.is_empty();
        self.ime.preedit = text.to_string();
        // Invalidate the grid cache for the target editor pane so the preedit
        // shift is re-rendered (editor generation doesn't change for preedit).
        if let Some(target) = self.effective_ime_target() {
            self.cache.pane_generations.remove(&target);
        }
        // Invalidate chrome for browser URL bar preedit display
        self.cache.chrome_generation += 1;
        self.cache.needs_redraw = true;
    }
}
