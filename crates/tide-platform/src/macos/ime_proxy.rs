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
    /// When true, suppress all event emission and committed_text mutation.
    /// Used during the flagsChanged priming cycle, which calls setMarkedText/
    /// unmarkText to nudge NSTextInputContext but should not produce app-visible
    /// side-effects.
    priming: Cell<bool>,
    /// Snapshot of whether marked text (IME composition) was active at the
    /// start of the current `keyDown` event.  When the Korean IME clears its
    /// last composing jamo it may call `doCommandBySelector: deleteBackward:`
    /// — but that backspace was consumed by the IME to finish its composition,
    /// not intended for the terminal.  This flag lets `deleteBackward:` detect
    /// that situation and suppress the spurious PTY backspace.
    composing_at_key_down: Cell<bool>,
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

        /// Report as AXTextArea so external apps (e.g. STT tools) recognise
        /// this view as a text input field via the Accessibility API.
        #[method_id(accessibilityRole)]
        fn accessibility_role(&self) -> Option<Retained<NSString>> {
            Some(NSString::from_str("AXTextArea"))
        }

        #[method(isAccessibilityElement)]
        fn is_accessibility_element(&self) -> Bool {
            Bool::YES
        }

        /// Return the parent view's screen frame so the accessibility system
        /// discovers this element during tree walks. Without this, the
        /// zero-frame ImeProxyView is invisible to accessibility clients.
        #[method(accessibilityFrame)]
        fn accessibility_frame(&self) -> NSRect {
            unsafe {
                self.superview()
                    .and_then(|superview| {
                        let window: Option<Retained<objc2_app_kit::NSWindow>> =
                            msg_send_id![&*superview, window];
                        window.map(|window| (superview, window))
                    })
                    .map(|(superview, window)| {
                        let view_bounds = superview.bounds();
                        let window_rect: NSRect = msg_send![
                            &*superview,
                            convertRect: view_bounds,
                            toView: std::ptr::null::<NSView>()
                        ];
                        window.convertRectToScreen(window_rect)
                    })
                    .unwrap_or(NSRect::ZERO)
            }
        }

        /// Return committed text as accessibility value so external tools
        /// (e.g. STT apps) recognise this as a writable text input.
        #[method_id(accessibilityValue)]
        fn accessibility_value_getter(&self) -> Option<Retained<NSString>> {
            let text = self.ivars().committed_text.borrow();
            Some(NSString::from_str(&text))
        }

        /// Accept text from accessibility clients (e.g. Nobs Whisper).
        /// Treats the incoming value as text to type into the terminal.
        #[method(setAccessibilityValue:)]
        fn set_accessibility_value(&self, value: &AnyObject) {
            let text = nsstring_from_anyobject(value);
            if !text.is_empty() {
                self.ivars().committed_text.borrow_mut().clear();
                self.ivars().committed_text.borrow_mut().push_str(&text);
                self.emit(PlatformEvent::ImeCommit(text));
            }
        }

        #[method_id(accessibilitySelectedText)]
        fn accessibility_selected_text(&self) -> Option<Retained<NSString>> {
            Some(NSString::from_str(""))
        }

        /// Insert text at the cursor via accessibility — the primary method
        /// STT tools use to inject transcribed text.
        #[method(setAccessibilitySelectedText:)]
        fn set_accessibility_selected_text(&self, value: &AnyObject) {
            let text = nsstring_from_anyobject(value);
            if !text.is_empty() {
                self.ivars().committed_text.borrow_mut().push_str(&text);
                self.emit(PlatformEvent::ImeCommit(text));
            }
        }

        #[method(accessibilitySelectedTextRange)]
        fn accessibility_selected_text_range(&self) -> NSRange {
            let len = utf16_len(&self.ivars().committed_text.borrow());
            NSRange::new(len, 0)
        }

        #[method(accessibilityNumberOfCharacters)]
        fn accessibility_number_of_characters(&self) -> isize {
            utf16_len(&self.ivars().committed_text.borrow()) as isize
        }

        /// Override NSResponder's `insertText:` for tools that call it
        /// directly, bypassing NSTextInputClient's `insertText:replacementRange:`.
        #[method(insertText:)]
        fn insert_text_responder(&self, string: &AnyObject) {
            let text = nsstring_from_anyobject(string);
            self.ivars().marked_text.borrow_mut().clear();
            self.ivars().committed_text.borrow_mut().push_str(&text);
            self.ivars().ime_handled.set(true);
            self.emit(PlatformEvent::ImeCommit(text));
        }

        #[method(keyDown:)]
        fn key_down(&self, event: &NSEvent) {
            *self.ivars().current_event.borrow_mut() = Some(event.retain());
            self.ivars().ime_handled.set(false);
            self.ivars().composing_at_key_down.set(
                !self.ivars().marked_text.borrow().is_empty(),
            );

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
        /// clear the preedit overlay and virtual text buffer, then prime the
        /// NSTextInputContext. The deactivating IME will call `insertText:`
        /// to commit any composing text — we must NOT commit it manually
        /// here, or the text gets committed twice (the IME commit + ours).
        #[method(flagsChanged:)]
        fn flags_changed(&self, event: &NSEvent) {
            // Clear the preedit overlay immediately (visual feedback).
            // Don't manually commit the composing text — the deactivating
            // IME will call insertText: to do that.
            {
                let had_preedit = !self.ivars().marked_text.borrow().is_empty();
                self.ivars().marked_text.borrow_mut().clear();
                if had_preedit {
                    self.emit(PlatformEvent::ImePreedit {
                        text: String::new(),
                        cursor: None,
                    });
                }
            }
            // Clear committed_text so the incoming IME starts from a clean
            // state and doesn't reference stale text via replacement ranges.
            self.ivars().committed_text.borrow_mut().clear();

            // Priming cycle: suppress event emission — the calls are only
            // meant to nudge NSTextInputContext, not produce app-visible events.
            self.ivars().priming.set(true);
            self.ivars().ime_handled.set(true);
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
            self.ivars().priming.set(false);
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
        }

        #[method(setMarkedText:selectedRange:replacementRange:)]
        fn set_marked_text_selected_range_replacement_range(
            &self,
            string: &AnyObject,
            _selected_range: NSRange,
            replacement_range: NSRange,
        ) {
            let text = nsstring_from_anyobject(string);

            // Handle replacement range: the IME wants to replace previously
            // committed text with the new preedit (e.g., Korean IME replacing
            // committed ㄱ with composing 가).  Erase the replaced characters
            // from the terminal so the preedit overlay renders at the correct
            // cursor position.
            if replacement_range.location != NSNotFound as usize {
                let committed_utf16 = utf16_len(&self.ivars().committed_text.borrow());
                if replacement_range.location < committed_utf16 {
                    let replace_end = (replacement_range.location + replacement_range.length)
                        .min(committed_utf16);
                    let clipped = NSRange::new(
                        replacement_range.location,
                        replace_end - replacement_range.location,
                    );
                    let (byte_start, byte_end, replaced_chars) = {
                        let buf = self.ivars().committed_text.borrow();
                        let (s, e) = utf16_range_to_byte_range(&buf, clipped);
                        (s, e, buf[s..e].chars().count())
                    };
                    if replaced_chars > 0 {
                        let mut dels = String::with_capacity(replaced_chars);
                        for _ in 0..replaced_chars {
                            dels.push('\x7f');
                        }
                        self.emit(PlatformEvent::ImeCommit(dels));
                        self.ivars()
                            .committed_text
                            .borrow_mut()
                            .replace_range(byte_start..byte_end, "");
                    }
                }
            }

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
                // When the Korean IME clears the last composing jamo it
                // performs a two-step: insertText(jamo) → deleteBackward:.
                // The insertText already pushed an ImeCommit into the
                // deferred queue.  Cancel that commit AND suppress the
                // backspace so the PTY never sees either — the net effect
                // is zero and the previously committed text is preserved.
                if key == Key::Backspace && self.ivars().composing_at_key_down.get() {
                    // 1. Pop the ImeCommit that insertText just enqueued.
                    {
                        let mut q = self.ivars().deferred_events.borrow_mut();
                        if matches!(q.last(), Some(PlatformEvent::ImeCommit(_))) {
                            q.pop();
                        }
                    }
                    // 2. Undo the committed_text push from insertText.
                    {
                        let mut buf = self.ivars().committed_text.borrow_mut();
                        if let Some((idx, _)) = buf.char_indices().last() {
                            buf.truncate(idx);
                        }
                    }
                    // 3. Clear residual preedit overlay.
                    self.ivars().marked_text.borrow_mut().clear();
                    self.emit(PlatformEvent::ImePreedit {
                        text: String::new(),
                        cursor: None,
                    });
                    self.ivars().ime_handled.set(true);
                    return;
                }

                // When the IME was composing at keyDown entry and has already
                // handled this key (committed or updated composition via
                // insertText:/setMarkedText:), any subsequent doCommandBySelector
                // is the IME passing through the triggering key.  Suppress it —
                // the key was consumed by the IME to finish the composition,
                // not intended for the terminal.
                if self.ivars().composing_at_key_down.get() && self.ivars().ime_handled.get() {
                    return;
                }

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
            priming: Cell::new(false),
            composing_at_key_down: Cell::new(false),
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
        if self.ivars().priming.get() {
            return; // Suppress during flagsChanged priming cycle
        }
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
