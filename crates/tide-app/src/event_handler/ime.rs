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
        if self.file_finder.is_some() {
            for ch in text.chars() {
                if let Some(ref mut finder) = self.file_finder {
                    finder.insert_char(ch);
                    self.chrome_generation += 1;
                }
            }
            self.ime_composing = false;
            self.ime_preedit.clear();
            self.needs_redraw = true;
            return;
        }
        if self.save_as_input.is_some() {
            for ch in text.chars() {
                if let Some(ref mut input) = self.save_as_input {
                    input.insert_char(ch);
                }
            }
            self.ime_composing = false;
            self.ime_preedit.clear();
            self.needs_redraw = true;
            return;
        }
        if self.git_switcher.is_some() {
            for ch in text.chars() {
                if let Some(ref mut gs) = self.git_switcher {
                    gs.insert_char(ch);
                    self.chrome_generation += 1;
                }
            }
            self.ime_composing = false;
            self.ime_preedit.clear();
            self.needs_redraw = true;
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
                    self.chrome_generation += 1;
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                    self.needs_redraw = true;
                    return;
                }
            }
        }

        // In editor preview mode, handle scroll keys directly instead of
        // routing through send_text_to_target (which blocks text in preview mode).
        // On macOS, plain text keys like j/k/d/u arrive via ImeCommit,
        // not KeyDown, so they never reach the preview scroll handler otherwise.
        if let Some(id) = self.focused {
            let is_preview = self
                .panes
                .get(&id)
                .map(|p| matches!(p, PaneKind::Editor(ep) if ep.preview_mode))
                .unwrap_or(false);
            if is_preview {
                let visible_rows = self
                    .visual_pane_rects
                    .iter()
                    .find(|(pid, _)| *pid == id)
                    .map(|(_, r)| {
                        let cs = self.cached_cell_size;
                        let content_h = (r.height
                            - crate::theme::TAB_BAR_HEIGHT
                            - crate::theme::PANE_PADDING)
                            .max(1.0);
                        (content_h / cs.height).floor() as usize
                    })
                    .unwrap_or(30);
                if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&id) {
                    let total = pane.preview_line_count();
                    let max_scroll = total.saturating_sub(visible_rows);
                    let mut changed = false;
                    for ch in text.chars() {
                        match ch {
                            'j' => {
                                if pane.preview_scroll < max_scroll {
                                    pane.preview_scroll += 1;
                                    changed = true;
                                }
                            }
                            'k' => {
                                if pane.preview_scroll > 0 {
                                    pane.preview_scroll -= 1;
                                    changed = true;
                                }
                            }
                            'd' => {
                                let half = visible_rows / 2;
                                let new = (pane.preview_scroll + half).min(max_scroll);
                                if new != pane.preview_scroll {
                                    pane.preview_scroll = new;
                                    changed = true;
                                }
                            }
                            'u' => {
                                let half = visible_rows / 2;
                                let new = pane.preview_scroll.saturating_sub(half);
                                if new != pane.preview_scroll {
                                    pane.preview_scroll = new;
                                    changed = true;
                                }
                            }
                            'g' => {
                                if pane.preview_scroll != 0 {
                                    pane.preview_scroll = 0;
                                    changed = true;
                                }
                            }
                            'G' => {
                                if pane.preview_scroll != max_scroll {
                                    pane.preview_scroll = max_scroll;
                                    changed = true;
                                }
                            }
                            'h' => {
                                if pane.preview_h_scroll > 0 {
                                    pane.preview_h_scroll =
                                        pane.preview_h_scroll.saturating_sub(2);
                                    changed = true;
                                }
                            }
                            'l' => {
                                let max_w = pane.preview_max_line_width();
                                if pane.preview_h_scroll < max_w {
                                    pane.preview_h_scroll += 2;
                                    changed = true;
                                }
                            }
                            _ => {}
                        }
                    }
                    if changed {
                        self.pane_generations.remove(&id);
                    }
                }
                self.ime_composing = false;
                self.ime_preedit.clear();
                self.needs_redraw = true;
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
                        self.ime_composing = false;
                        self.ime_preedit.clear();
                        self.needs_redraw = true;
                        return;
                    }
                }
                // Non-matching text: ignore for launcher
                self.ime_composing = false;
                self.ime_preedit.clear();
                self.needs_redraw = true;
                return;
            }
        }

        self.send_text_to_target(text);
        self.ime_composing = false;
        self.ime_preedit.clear();
        self.needs_redraw = true;
    }

    /// Handle IME preedit update (composition in progress).
    pub(crate) fn handle_ime_preedit(&mut self, text: &str) {
        self.ime_composing = !text.is_empty();
        self.ime_preedit = text.to_string();
        // Invalidate the grid cache for the target editor pane so the preedit
        // shift is re-rendered (editor generation doesn't change for preedit).
        if let Some(target) = self.effective_ime_target() {
            self.pane_generations.remove(&target);
        }
        // Invalidate chrome for browser URL bar preedit display
        self.chrome_generation += 1;
        self.needs_redraw = true;
    }
}
