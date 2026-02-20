//! Native IME event handling.
//!
//! With the native NSTextInputClient implementation, the platform layer
//! handles all composition complexity. We just receive committed text
//! and preedit updates — no workarounds needed.

use crate::App;

impl App {
    /// Handle IME committed text (composition done).
    pub(crate) fn handle_ime_commit(&mut self, text: &str) {
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
        self.needs_redraw = true;
    }
}
