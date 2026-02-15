use std::time::Instant;

use winit::event::Ime;

use tide_core::TerminalBackend;

use crate::pane::PaneKind;
use crate::App;

impl App {
    pub(crate) fn handle_ime(&mut self, ime: Ime) {
        match ime {
            Ime::Enabled => {
                self.ime_active = true;
            }
            Ime::Disabled => {
                self.ime_active = false;
                self.ime_composing = false;
                self.ime_preedit.clear();
                self.pending_hangul_initial = None;
            }
            Ime::Commit(text) => {
                // If we have a pending initial from a pre-IME keystroke,
                // try to combine it with the committed text.
                let output = if let Some(initial) = self.pending_hangul_initial.take() {
                    combine_initial_with_text(initial, &text)
                        .unwrap_or_else(|| { let mut s = String::new(); s.push(initial); s.push_str(&text); s })
                } else {
                    text
                };
                // Recover text dropped by the IME.  macOS Korean IME drops
                // the composed character when a non-Hangul key (e.g. ?) is
                // pressed during composition: it sends Preedit("") then
                // Commit("?") without committing the Korean text.  We saved
                // the cleared preedit in the Preedit handler, so prepend it
                // here if the commit doesn't already include it.
                let output = if let Some(dropped) = self.ime_dropped_preedit.take() {
                    if !output.starts_with(&dropped) {
                        format!("{}{}", dropped, output)
                    } else {
                        output
                    }
                } else {
                    output
                };
                // IME composed text → route to branch switcher, file finder, save-as input, search bar, or focused pane
                if self.branch_switcher.is_some() {
                    for ch in output.chars() {
                        if let Some(ref mut bs) = self.branch_switcher {
                            bs.insert_char(ch);
                            self.chrome_generation += 1;
                        }
                    }
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                    self.needs_redraw = true;
                    return;
                }
                if self.file_switcher.is_some() {
                    for ch in output.chars() {
                        if let Some(ref mut fs) = self.file_switcher {
                            fs.insert_char(ch);
                            self.chrome_generation += 1;
                        }
                    }
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                    self.needs_redraw = true;
                    return;
                }
                if self.file_finder.is_some() {
                    for ch in output.chars() {
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
                    for ch in output.chars() {
                        if let Some(ref mut input) = self.save_as_input {
                            input.insert_char(ch);
                        }
                    }
                    self.ime_composing = false;
                    self.ime_preedit.clear();
                    self.needs_redraw = true;
                    return;
                }
                if let Some(search_pane_id) = self.search_focus {
                    for ch in output.chars() {
                        self.search_bar_insert(search_pane_id, ch);
                    }
                } else if let Some(focused_id) = self.focused {
                    match self.panes.get_mut(&focused_id) {
                        Some(PaneKind::Terminal(pane)) => {
                            if pane.backend.display_offset() > 0 {
                                pane.backend.request_scroll_to_bottom();
                            }
                            pane.backend.write(output.as_bytes());
                            self.input_just_sent = true;
                            self.input_sent_at = Some(Instant::now());
                        }
                        Some(PaneKind::Editor(pane)) => {
                            for ch in output.chars() {
                                pane.editor.handle_action(tide_editor::EditorActionKind::InsertChar(ch));
                            }
                        }
                        Some(PaneKind::Diff(_)) => {}
                        None => {}
                    }
                }
                self.ime_composing = false;
                self.ime_preedit.clear();
            }
            Ime::Preedit(text, _cursor) => {
                // When composition is cleared (text becomes empty), save the
                // previous preedit text.  If the next Ime::Commit doesn't
                // contain it, the IME dropped it and we need to recover it.
                if text.is_empty() && !self.ime_preedit.is_empty() {
                    self.ime_dropped_preedit = Some(self.ime_preedit.clone());
                } else if !text.is_empty() {
                    // New/continued composition — any previously saved text
                    // is no longer relevant.
                    self.ime_dropped_preedit = None;
                }

                self.ime_composing = !text.is_empty();
                // If we have a pending initial, combine it with the
                // preedit text for display (e.g. ㅇ + ㅏ → 아).
                if !text.is_empty() {
                    if let Some(initial) = self.pending_hangul_initial {
                        if let Some(combined) = combine_initial_with_text(initial, &text) {
                            self.ime_preedit = combined;
                            return;
                        }
                    }
                }
                self.ime_preedit = text;
            }
        }
    }
}

/// Check if a character is in a Hangul Unicode range.
/// Covers Jamo, Compatibility Jamo, Syllables, and Extended Jamo blocks.
pub(crate) fn is_hangul_char(c: char) -> bool {
    matches!(c,
        '\u{1100}'..='\u{11FF}'   // Hangul Jamo
        | '\u{3130}'..='\u{318F}' // Hangul Compatibility Jamo
        | '\u{A960}'..='\u{A97F}' // Hangul Jamo Extended-A
        | '\u{AC00}'..='\u{D7AF}' // Hangul Syllables
        | '\u{D7B0}'..='\u{D7FF}' // Hangul Jamo Extended-B
    )
}

/// Map a Compatibility Jamo consonant to its Choseong (initial) index (0..18).
fn choseong_index(c: char) -> Option<u32> {
    // Compatibility Jamo consonants → Choseong index
    // ㄱ ㄲ ㄴ ㄷ ㄸ ㄹ ㅁ ㅂ ㅃ ㅅ ㅆ ㅇ ㅈ ㅉ ㅊ ㅋ ㅌ ㅍ ㅎ
    match c {
        'ㄱ' => Some(0),  'ㄲ' => Some(1),  'ㄴ' => Some(2),
        'ㄷ' => Some(3),  'ㄸ' => Some(4),  'ㄹ' => Some(5),
        'ㅁ' => Some(6),  'ㅂ' => Some(7),  'ㅃ' => Some(8),
        'ㅅ' => Some(9),  'ㅆ' => Some(10), 'ㅇ' => Some(11),
        'ㅈ' => Some(12), 'ㅉ' => Some(13), 'ㅊ' => Some(14),
        'ㅋ' => Some(15), 'ㅌ' => Some(16), 'ㅍ' => Some(17),
        'ㅎ' => Some(18),
        _ => None,
    }
}

/// Map a Compatibility Jamo vowel to its Jungseong (medial) index (0..20).
fn jungseong_index(c: char) -> Option<u32> {
    let code = c as u32;
    // ㅏ (0x314F) .. ㅣ (0x3163) → indices 0..20
    if (0x314F..=0x3163).contains(&code) {
        Some(code - 0x314F)
    } else {
        None
    }
}

/// Try to combine a Choseong (initial consonant) with a string that starts
/// with a Jungseong (vowel).  Returns the combined string if successful.
/// e.g. 'ㅇ' + "ㅏ" → "아",  'ㅇ' + "ㅏㄴ" → "안" (won't happen here).
pub(super) fn combine_initial_with_text(initial: char, text: &str) -> Option<String> {
    let cho = choseong_index(initial)?;
    let first = text.chars().next()?;
    let jung = jungseong_index(first)?;
    let syllable = char::from_u32(0xAC00 + (cho * 21 + jung) * 28)?;
    let mut result = String::new();
    result.push(syllable);
    result.extend(text.chars().skip(1));
    Some(result)
}
