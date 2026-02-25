//! ImeProxyView: invisible per-pane NSView subclass for IME isolation.
//!
//! Each terminal/editor pane gets its own ImeProxyView that implements
//! NSTextInputClient. macOS routes keyboard/IME events to whichever proxy
//! is the first responder, giving each pane an independent NSTextInputContext.
//! This prevents Korean IME composition from carrying over when switching panes.
//!
//! ## Korean IME inline composition
//!
//! The Korean IME on macOS may commit the first consonant via `insertText:`
//! instead of `setMarkedText:` after an input method switch. Subsequent
//! characters are then composed inline using `insertText:replacementRange:`
//! — the IME reads back the committed text via `attributedSubstringForProposedRange:`
//! and replaces it with the composed syllable. To support this, we maintain a
//! virtual text buffer (`committed_text`) and emit Backspace events before each
//! replacement commit so the terminal stays in sync.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, Sel};
use objc2::{
    declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass,
};
use objc2_app_kit::{NSEvent, NSTextInputClient, NSView};
use objc2_foundation::MainThreadMarker;
use objc2_foundation::{
    NSArray, NSAttributedString, NSNotFound, NSPoint, NSRange, NSRect, NSSize, NSString,
};

use tide_core::{Key, Modifiers};

use crate::{EventCallback, PlatformEvent};

use super::view::{
    key_and_modifiers_from_event, modifiers_from_flags, nsstring_from_anyobject,
};

pub struct ImeProxyViewIvars {
    callback: Rc<RefCell<EventCallback>>,
    marked_text: RefCell<String>,
    ime_cursor_rect: Cell<NSRect>,
    ime_handled: Cell<bool>,
    current_event: RefCell<Option<Retained<NSEvent>>>,
    /// Virtual text buffer: stores committed text so that `selectedRange`,
    /// `markedRange`, and `attributedSubstringForProposedRange` can return
    /// consistent, correct values. All range calculations use UTF-16 code
    /// units (matching NSString/NSRange conventions).
    committed_text: RefCell<String>,
    /// When true, PlatformEvents are accumulated instead of emitted immediately.
    /// This prevents the full event pipeline (poll_background_events, rendering,
    /// etc.) from running mid-interpretKeyEvents, which causes TUI apps like
    /// Claude Code to corrupt cursor position during Korean IME composition.
    deferring: Cell<bool>,
    /// Accumulated events during interpretKeyEvents. Flushed after it returns.
    deferred_events: RefCell<Vec<PlatformEvent>>,
}

declare_class!(
    pub struct ImeProxyView;

    unsafe impl ClassType for ImeProxyView {
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "ImeProxyView";
    }

    impl DeclaredClass for ImeProxyView {
        type Ivars = ImeProxyViewIvars;
    }

    unsafe impl ImeProxyView {
        #[method(acceptsFirstResponder)]
        fn accepts_first_responder(&self) -> Bool {
            Bool::YES
        }

        #[method(isFlipped)]
        fn is_flipped(&self) -> Bool {
            Bool::YES
        }

        #[method(keyDown:)]
        fn key_down(&self, event: &NSEvent) {
            *self.ivars().current_event.borrow_mut() = Some(event.retain());
            self.ivars().ime_handled.set(false);

            // Begin deferring: accumulate PlatformEvents instead of emitting
            // them immediately. This prevents the full event pipeline (including
            // poll_background_events / rendering) from running mid-interpretKeyEvents,
            // which causes TUI app redraws to corrupt cursor position during
            // Korean IME composition.
            self.ivars().deferring.set(true);

            unsafe {
                // Activate the input context before processing the key event.
                // After an input method switch, the context may not be "current"
                // until explicitly activated.
                let ic: Option<Retained<AnyObject>> = msg_send_id![self, inputContext];
                if let Some(ref ic) = ic {
                    let _: () = msg_send![ic, activate];
                }

                let events = NSArray::from_slice(&[event]);
                let _: () = msg_send![self, interpretKeyEvents: &*events];
            }

            // Stop deferring and flush all accumulated events.
            self.ivars().deferring.set(false);
            self.flush_deferred_events();

            if !self.ivars().ime_handled.get() {
                let (key, modifiers) = key_and_modifiers_from_event(event);
                let chars = unsafe { event.characters().map(|s| s.to_string()) };
                self.emit(PlatformEvent::KeyDown { key, modifiers, chars });
            }

            *self.ivars().current_event.borrow_mut() = None;
        }

        #[method(keyUp:)]
        fn key_up(&self, event: &NSEvent) {
            let (key, modifiers) = key_and_modifiers_from_event(event);
            self.emit(PlatformEvent::KeyUp { key, modifiers });
        }

        /// On modifier changes (including Caps Lock input method toggle),
        /// prime the NSTextInputContext by cycling through a setMarkedText/
        /// unmarkText sequence. This nudges the context's internal state so
        /// the Korean IME's inline composition (via replacementRange) works
        /// more reliably after an input method switch.
        #[method(flagsChanged:)]
        fn flags_changed(&self, event: &NSEvent) {
            self.ivars().ime_handled.set(true); // prevent side-effect emissions
            unsafe {
                let empty = NSString::from_str("");
                let range = NSRange::new(0, 0);
                let _: () = msg_send![
                    self,
                    setMarkedText: &*empty,
                    selectedRange: range,
                    replacementRange: range
                ];
                let _: () = msg_send![self, unmarkText];
            }
            self.ivars().ime_handled.set(false);
            // Forward to next responder (TideView) for ModifiersChanged emission
            unsafe {
                let _: () = msg_send![super(self), flagsChanged: event];
            }
        }
    }

    // ── NSTextInputClient protocol ──

    unsafe impl NSTextInputClient for ImeProxyView {
        #[method(insertText:replacementRange:)]
        fn insert_text_replacement_range(
            &self,
            string: &AnyObject,
            replacement_range: NSRange,
        ) {
            let text = nsstring_from_anyobject(string);
            self.ivars().marked_text.borrow_mut().clear();

            // Handle replacement range: the IME wants to replace previously
            // committed text (e.g., Korean IME composing syllables inline).
            // Prepend DEL (0x7F) bytes to erase the old characters so the
            // entire replacement is emitted as a single ImeCommit.  This
            // ensures a single atomic PTY write — separate Backspace events
            // would be two write() calls that TUI apps (Claude Code) may
            // process in different read chunks, causing the delete to land
            // before the replacement text arrives.
            let commit_text = if replacement_range.location != NSNotFound as usize {
                let (byte_start, byte_end, replaced_chars) = {
                    let buf = self.ivars().committed_text.borrow();
                    let (s, e) = utf16_range_to_byte_range(&buf, replacement_range);
                    (s, e, buf[s..e].chars().count())
                };

                let mut combined = String::with_capacity(replaced_chars + text.len());
                for _ in 0..replaced_chars {
                    combined.push('\x7f');
                }
                combined.push_str(&text);

                let mut buf = self.ivars().committed_text.borrow_mut();
                buf.replace_range(byte_start..byte_end, &text);
                combined
            } else {
                self.ivars().committed_text.borrow_mut().push_str(&text);
                text
            };

            self.ivars().ime_handled.set(true);
            self.emit(PlatformEvent::ImeCommit(commit_text));

            // Clear committed_text after each commit so the IME never sees
            // stale text on the next keystroke.  Without this, the IME uses
            // replacement ranges (insertText:replacementRange:) for inline
            // composition, which emits DEL bytes to erase old characters.
            // In a terminal, the cursor may have moved (TUI redraws, etc.),
            // making the replacement count wrong — causing "devouring."
            // Clearing matches iTerm2/Ghostty behavior (no committed_text
            // buffer) and forces the IME to use the clean setMarkedText →
            // insertText(NSNotFound) path for all composition.
            self.ivars().committed_text.borrow_mut().clear();
        }

        #[method(setMarkedText:selectedRange:replacementRange:)]
        fn set_marked_text_selected_range_replacement_range(
            &self,
            string: &AnyObject,
            _selected_range: NSRange,
            _replacement_range: NSRange,
        ) {
            let text = nsstring_from_anyobject(string);
            *self.ivars().marked_text.borrow_mut() = text.clone();
            self.ivars().ime_handled.set(true);
            self.emit(PlatformEvent::ImePreedit { text, cursor: None });
        }

        #[method(unmarkText)]
        fn unmark_text(&self) {
            self.ivars().marked_text.borrow_mut().clear();
            self.emit(PlatformEvent::ImePreedit { text: String::new(), cursor: None });
        }

        #[method(hasMarkedText)]
        fn has_marked_text(&self) -> Bool {
            if self.ivars().marked_text.borrow().is_empty() {
                Bool::NO
            } else {
                Bool::YES
            }
        }

        #[method(markedRange)]
        fn marked_range(&self) -> NSRange {
            let marked = self.ivars().marked_text.borrow();
            if marked.is_empty() {
                NSRange::new(NSNotFound as usize, 0)
            } else {
                let committed_utf16 = utf16_len(&self.ivars().committed_text.borrow());
                let marked_utf16 = utf16_len(&marked);
                NSRange::new(committed_utf16, marked_utf16)
            }
        }

        #[method(selectedRange)]
        fn selected_range(&self) -> NSRange {
            let committed_utf16 = utf16_len(&self.ivars().committed_text.borrow());
            let marked_utf16 = utf16_len(&self.ivars().marked_text.borrow());
            NSRange::new(committed_utf16 + marked_utf16, 0)
        }

        #[method_id(attributedSubstringForProposedRange:actualRange:)]
        fn attributed_substring_for_proposed_range(
            &self,
            range: NSRange,
            actual_range: *mut NSRange,
        ) -> Option<Retained<NSAttributedString>> {
            // Build virtual text: committed + marked
            let committed = self.ivars().committed_text.borrow();
            let marked = self.ivars().marked_text.borrow();
            let mut full_text = committed.clone();
            full_text.push_str(&marked);

            let full_utf16_len = utf16_len(&full_text);

            let substring = if range.location < full_utf16_len {
                let end = (range.location + range.length).min(full_utf16_len);
                let (byte_start, byte_end) = utf16_range_to_byte_range(
                    &full_text,
                    NSRange::new(range.location, end - range.location),
                );
                if !actual_range.is_null() {
                    unsafe { *actual_range = NSRange::new(range.location, end - range.location); }
                }
                full_text[byte_start..byte_end].to_string()
            } else {
                if !actual_range.is_null() {
                    unsafe { *actual_range = range; }
                }
                String::new()
            };

            Some(NSAttributedString::from_nsstring(&NSString::from_str(&substring)))
        }

        #[method_id(validAttributesForMarkedText)]
        fn valid_attributes_for_marked_text(&self) -> Retained<NSArray<NSString>> {
            NSArray::new()
        }

        #[method(firstRectForCharacterRange:actualRange:)]
        fn first_rect_for_character_range(
            &self,
            _range: NSRange,
            _actual_range: *mut NSRange,
        ) -> NSRect {
            let ime_rect = self.ivars().ime_cursor_rect.get();
            unsafe {
                let window: Option<Retained<objc2_app_kit::NSWindow>> =
                    msg_send_id![self, window];
                if let Some(window) = window {
                    if let Some(superview) = self.superview() {
                        let window_rect: NSRect = msg_send![&*superview, convertRect: ime_rect, toView: std::ptr::null::<NSView>()];
                        let screen_rect = window.convertRectToScreen(window_rect);
                        return screen_rect;
                    }
                }
            }
            ime_rect
        }

        #[method(characterIndexForPoint:)]
        fn character_index_for_point(&self, _point: NSPoint) -> usize {
            NSNotFound as usize
        }

        #[method(doCommandBySelector:)]
        fn do_command_by_selector(&self, selector: Sel) {
            let key = match selector {
                s if s == sel!(moveUp:) => Some(Key::Up),
                s if s == sel!(moveDown:) => Some(Key::Down),
                s if s == sel!(moveLeft:) => Some(Key::Left),
                s if s == sel!(moveRight:) => Some(Key::Right),
                s if s == sel!(moveWordLeft:) => Some(Key::Left),
                s if s == sel!(moveWordRight:) => Some(Key::Right),
                s if s == sel!(insertNewline:) => Some(Key::Enter),
                s if s == sel!(deleteBackward:) => Some(Key::Backspace),
                s if s == sel!(deleteForward:) => Some(Key::Delete),
                s if s == sel!(insertTab:) => Some(Key::Tab),
                s if s == sel!(insertBacktab:) => Some(Key::Tab),
                s if s == sel!(cancelOperation:) => Some(Key::Escape),
                s if s == sel!(moveToBeginningOfLine:) => Some(Key::Home),
                s if s == sel!(moveToEndOfLine:) => Some(Key::End),
                s if s == sel!(pageUp:) => Some(Key::PageUp),
                s if s == sel!(pageDown:) => Some(Key::PageDown),
                s if s == sel!(noop:) => None,
                _ => {
                    return;
                }
            };
            if let Some(key) = key {
                // Keep committed_text in sync.
                // Backspace: remove last character. All other keys that change
                // cursor position or terminal state: clear entirely to prevent
                // the virtual buffer from going stale.
                if key == Key::Backspace {
                    let mut buf = self.ivars().committed_text.borrow_mut();
                    if let Some((idx, _)) = buf.char_indices().last() {
                        buf.truncate(idx);
                    }
                } else {
                    self.ivars().committed_text.borrow_mut().clear();
                }

                let modifiers = if selector == sel!(insertBacktab:) {
                    Modifiers { shift: true, ..Default::default() }
                } else if let Some(event) = self.ivars().current_event.borrow().as_ref() {
                    modifiers_from_flags(unsafe { event.modifierFlags() })
                } else {
                    Modifiers::default()
                };
                self.ivars().ime_handled.set(true);
                self.emit(PlatformEvent::KeyDown { key, modifiers, chars: None });
            }
        }
    }
);

impl ImeProxyView {
    pub fn new(
        callback: Rc<RefCell<EventCallback>>,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(ImeProxyViewIvars {
            callback,
            marked_text: RefCell::new(String::new()),
            ime_cursor_rect: Cell::new(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(1.0, 20.0),
            )),
            ime_handled: Cell::new(false),
            current_event: RefCell::new(None),
            committed_text: RefCell::new(String::new()),
            deferring: Cell::new(false),
            deferred_events: RefCell::new(Vec::new()),
        });
        unsafe { msg_send_id![super(this), initWithFrame: NSRect::ZERO] }
    }

    pub fn set_ime_cursor_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        if let Some(superview) = unsafe { self.superview() } {
            let frame = superview.frame();
            let flipped_y = frame.size.height - y - h;
            self.ivars().ime_cursor_rect.set(NSRect::new(
                NSPoint::new(x, flipped_y),
                NSSize::new(w, h),
            ));
        }
    }

    fn emit(&self, event: PlatformEvent) {
        if self.ivars().deferring.get() {
            self.ivars().deferred_events.borrow_mut().push(event);
        } else {
            super::emit_event(&self.ivars().callback, event, "ImeProxyView");
        }
    }

    /// Flush all deferred events, emitting them in order.
    /// Wraps the batch with BatchStart/BatchEnd so the app suppresses
    /// rendering until all events are processed — prevents flicker from
    /// intermediate states (e.g. Backspace before replacement commit).
    fn flush_deferred_events(&self) {
        let events: Vec<PlatformEvent> = self.ivars().deferred_events.borrow_mut().drain(..).collect();
        if events.is_empty() {
            return;
        }
        super::emit_event(&self.ivars().callback, PlatformEvent::BatchStart, "ImeProxyView");
        for event in events {
            super::emit_event(&self.ivars().callback, event, "ImeProxyView");
        }
        super::emit_event(&self.ivars().callback, PlatformEvent::BatchEnd, "ImeProxyView");
    }
}

/// Count UTF-16 code units in a Rust string.
fn utf16_len(s: &str) -> usize {
    s.chars().map(|c| c.len_utf16()).sum()
}

/// Convert an NSRange (in UTF-16 code units) to a byte range in a Rust string.
fn utf16_range_to_byte_range(s: &str, range: NSRange) -> (usize, usize) {
    let mut utf16_pos = 0;
    let mut byte_start = s.len();
    let mut byte_end = s.len();

    for (byte_idx, ch) in s.char_indices() {
        if utf16_pos == range.location {
            byte_start = byte_idx;
        }
        utf16_pos += ch.len_utf16();
        if utf16_pos == range.location + range.length {
            byte_end = byte_idx + ch.len_utf8();
            break;
        }
    }
    if utf16_pos == range.location && byte_start == s.len() {
        byte_start = s.len();
        byte_end = s.len();
    }

    (byte_start, byte_end)
}
