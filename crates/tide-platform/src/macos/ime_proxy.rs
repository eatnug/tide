//! ImeProxyView: invisible per-pane NSView subclass for IME isolation.
//!
//! Each terminal/editor pane gets its own ImeProxyView that implements
//! NSTextInputClient. macOS routes keyboard/IME events to whichever proxy
//! is the first responder, giving each pane an independent NSTextInputContext.
//! This prevents Korean IME composition from carrying over when switching panes.

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

            unsafe {
                let events = NSArray::from_slice(&[event]);
                let _: () = msg_send![self, interpretKeyEvents: &*events];
            }

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
    }

    // ── NSTextInputClient protocol ──

    unsafe impl NSTextInputClient for ImeProxyView {
        #[method(insertText:replacementRange:)]
        fn insert_text_replacement_range(
            &self,
            string: &AnyObject,
            _replacement_range: NSRange,
        ) {
            let text = nsstring_from_anyobject(string);
            self.ivars().marked_text.borrow_mut().clear();
            self.ivars().ime_handled.set(true);
            self.emit(PlatformEvent::ImeCommit(text));
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
            let text = self.ivars().marked_text.borrow();
            if text.is_empty() {
                NSRange::new(NSNotFound as usize, 0)
            } else {
                NSRange::new(0, text.len())
            }
        }

        #[method(selectedRange)]
        fn selected_range(&self) -> NSRange {
            let text = self.ivars().marked_text.borrow();
            if text.is_empty() {
                NSRange::new(NSNotFound as usize, 0)
            } else {
                NSRange::new(text.len(), 0)
            }
        }

        #[method_id(attributedSubstringForProposedRange:actualRange:)]
        fn attributed_substring_for_proposed_range(
            &self,
            _range: NSRange,
            _actual_range: *mut NSRange,
        ) -> Option<Retained<NSAttributedString>> {
            None
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
                    // Convert from parent TideView coordinate space to window coords
                    if let Some(superview) = self.superview() {  // safe: inside unsafe block
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
        });
        unsafe { msg_send_id![super(this), initWithFrame: NSRect::ZERO] }
    }

    pub fn set_ime_cursor_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        // Cursor rect is in TideView (superview) coordinate space.
        // TideView is flipped but convertRect:toView:nil handles the flip,
        // so we need to store in the superview's native coordinate space.
        if let Some(superview) = unsafe { self.superview() } {
            let frame = superview.frame();
            let flipped_y = frame.size.height - y - h;
            self.ivars().ime_cursor_rect.set(NSRect::new(
                NSPoint::new(x, flipped_y),
                NSSize::new(w, h),
            ));
        }
    }

    #[allow(dead_code)]
    pub fn discard_marked_text(&self) {
        self.ivars().marked_text.borrow_mut().clear();
        unsafe {
            let ic: Option<Retained<objc2_app_kit::NSTextInputContext>> =
                msg_send_id![self, inputContext];
            if let Some(ic) = ic {
                let _: () = msg_send![&ic, discardMarkedText];
            }
        }
    }

    fn emit(&self, event: PlatformEvent) {
        super::app::with_main_window(|window| {
            if let Ok(mut cb) = self.ivars().callback.try_borrow_mut() {
                cb(event.clone(), window);
            }
        });
    }
}
