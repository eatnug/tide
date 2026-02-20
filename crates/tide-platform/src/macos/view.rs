//! TideView: NSView subclass with NSTextInputClient for native IME support.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, Sel};
use objc2::{
    declare_class, msg_send, msg_send_id, mutability, sel, ClassType, DeclaredClass,
};
use objc2_foundation::MainThreadMarker;
use objc2_app_kit::{
    NSEvent, NSEventModifierFlags, NSTextInputClient, NSTrackingArea,
    NSTrackingAreaOptions, NSView,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSNotFound, NSPoint, NSRange, NSRect, NSSize, NSString,
};
use objc2_quartz_core::CAMetalLayer;

use tide_core::{Key, Modifiers};

use crate::{EventCallback, MouseButton, PlatformEvent};

// ──────────────────────────────────────────────
// TideView — NSView subclass
// ──────────────────────────────────────────────

pub struct TideViewIvars {
    callback: Rc<RefCell<EventCallback>>,
    marked_text: RefCell<String>,
    ime_cursor_rect: Cell<NSRect>,
    layer: RefCell<Option<Retained<CAMetalLayer>>>,
    /// Set to true when insertText/setMarkedText is called during interpretKeyEvents.
    /// Used to decide whether to emit a raw KeyDown event after interpretKeyEvents returns.
    ime_handled: Cell<bool>,
    /// Stashed event during keyDown so doCommandBySelector can read modifiers.
    current_event: RefCell<Option<Retained<NSEvent>>>,
}

declare_class!(
    pub struct TideView;

    unsafe impl ClassType for TideView {
        type Super = NSView;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TideView";
    }

    impl DeclaredClass for TideView {
        type Ivars = TideViewIvars;
    }

    // ── NSView overrides ──

    unsafe impl TideView {
        #[method(acceptsFirstResponder)]
        fn accepts_first_responder(&self) -> Bool {
            Bool::YES
        }

        #[method(wantsLayer)]
        fn wants_layer(&self) -> Bool {
            Bool::YES
        }

        #[method(isFlipped)]
        fn is_flipped(&self) -> Bool {
            Bool::YES
        }

        #[method(makeBackingLayer)]
        fn make_backing_layer(&self) -> *mut AnyObject {
            let layer = unsafe { CAMetalLayer::new() };
            let ptr = Retained::as_ptr(&layer) as *mut AnyObject;
            *self.ivars().layer.borrow_mut() = Some(layer);
            ptr
        }

        #[method(viewDidChangeBackingProperties)]
        fn view_did_change_backing_properties(&self) {
            let scale = self.backing_scale();
            if let Some(ref layer) = *self.ivars().layer.borrow() {
                layer.setContentsScale(scale);
            }
            self.emit(PlatformEvent::ScaleFactorChanged(scale));
        }

        #[method(setFrameSize:)]
        fn set_frame_size(&self, new_size: NSSize) {
            unsafe {
                let _: () = msg_send![super(self), setFrameSize: new_size];
            }
            let scale = self.backing_scale();
            if let Some(ref layer) = *self.ivars().layer.borrow() {
                unsafe {
                    let drawable_size = objc2_foundation::CGSize::new(
                        new_size.width * scale,
                        new_size.height * scale,
                    );
                    layer.setDrawableSize(drawable_size);
                }
            }
            self.emit(PlatformEvent::Resized {
                width: (new_size.width * scale) as u32,
                height: (new_size.height * scale) as u32,
            });
        }

        // ── Keyboard events ──

        #[method(keyDown:)]
        fn key_down(&self, event: &NSEvent) {
            // Stash the event so doCommandBySelector can read modifiers
            *self.ivars().current_event.borrow_mut() = Some(event.retain());
            self.ivars().ime_handled.set(false);

            // Route through Cocoa's text input machinery.
            // This calls insertText:, setMarkedText:, or doCommandBySelector: synchronously.
            unsafe {
                let events = NSArray::from_slice(&[event]);
                let _: () = msg_send![self, interpretKeyEvents: &*events];
            }

            // If IME didn't handle it (no insertText/setMarkedText/doCommandBySelector match),
            // emit as a raw key event so the app can process shortcuts, etc.
            if !self.ivars().ime_handled.get() {
                let (key, modifiers) = key_and_modifiers_from_event(event);
                let chars = unsafe { event.characters().map(|s| s.to_string()) };
                self.emit(PlatformEvent::KeyDown {
                    key,
                    modifiers,
                    chars,
                });
            }

            *self.ivars().current_event.borrow_mut() = None;
        }

        #[method(keyUp:)]
        fn key_up(&self, event: &NSEvent) {
            let (key, modifiers) = key_and_modifiers_from_event(event);
            self.emit(PlatformEvent::KeyUp { key, modifiers });
        }

        #[method(flagsChanged:)]
        fn flags_changed(&self, event: &NSEvent) {
            let modifiers = modifiers_from_flags(unsafe { event.modifierFlags() });
            self.emit(PlatformEvent::ModifiersChanged(modifiers));
        }

        // ── Mouse events ──

        #[method(mouseDown:)]
        fn mouse_down(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseDown { button: MouseButton::Left, position: pos });
        }

        #[method(mouseUp:)]
        fn mouse_up(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseUp { button: MouseButton::Left, position: pos });
        }

        #[method(rightMouseDown:)]
        fn right_mouse_down(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseDown { button: MouseButton::Right, position: pos });
        }

        #[method(rightMouseUp:)]
        fn right_mouse_up(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseUp { button: MouseButton::Right, position: pos });
        }

        #[method(otherMouseDown:)]
        fn other_mouse_down(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseDown { button: MouseButton::Middle, position: pos });
        }

        #[method(otherMouseUp:)]
        fn other_mouse_up(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseUp { button: MouseButton::Middle, position: pos });
        }

        #[method(mouseMoved:)]
        fn mouse_moved(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseMoved { position: pos });
        }

        #[method(mouseDragged:)]
        fn mouse_dragged(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseMoved { position: pos });
        }

        #[method(rightMouseDragged:)]
        fn right_mouse_dragged(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseMoved { position: pos });
        }

        #[method(otherMouseDragged:)]
        fn other_mouse_dragged(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            self.emit(PlatformEvent::MouseMoved { position: pos });
        }

        #[method(scrollWheel:)]
        fn scroll_wheel(&self, event: &NSEvent) {
            let pos = self.mouse_pos(event);
            let (dx, dy) = unsafe {
                let has_precise: Bool = msg_send![event, hasPreciseScrollingDeltas];
                if has_precise.as_bool() {
                    let sdx: f64 = msg_send![event, scrollingDeltaX];
                    let sdy: f64 = msg_send![event, scrollingDeltaY];
                    (sdx as f32 / 10.0, sdy as f32 / 10.0)
                } else {
                    let sdx: f64 = msg_send![event, scrollingDeltaX];
                    let sdy: f64 = msg_send![event, scrollingDeltaY];
                    (sdx as f32 * 3.0, sdy as f32 * 3.0)
                }
            };
            self.emit(PlatformEvent::Scroll { dx, dy, position: pos });
        }

        #[method(acceptsFirstMouse:)]
        fn accepts_first_mouse(&self, _event: &NSEvent) -> Bool {
            Bool::YES
        }

        /// Called from background threads via performSelectorOnMainThread
        /// to wake the main thread and trigger a render.
        #[method(triggerRedraw)]
        fn trigger_redraw(&self) {
            self.emit(PlatformEvent::RedrawRequested);
        }
    }

    // ── NSTextInputClient protocol ──

    unsafe impl NSTextInputClient for TideView {
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
                    let window_rect: NSRect = msg_send![self, convertRect: ime_rect, toView: std::ptr::null::<NSView>()];
                    let screen_rect = window.convertRectToScreen(window_rect);
                    return screen_rect;
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
            // Map known selectors to key events
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
                    // Unknown selector — let keyDown fall through to emit raw key event.
                    // Don't set ime_handled so the stashed event is emitted as KeyDown.
                    return;
                }
            };
            if let Some(key) = key {
                // Read modifiers from the stashed event if available
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

impl TideView {
    pub fn new(callback: Rc<RefCell<EventCallback>>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(TideViewIvars {
            callback,
            marked_text: RefCell::new(String::new()),
            ime_cursor_rect: Cell::new(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(1.0, 20.0),
            )),
            layer: RefCell::new(None),
            ime_handled: Cell::new(false),
            current_event: RefCell::new(None),
        });
        let this: Retained<Self> = unsafe { msg_send_id![super(this), init] };

        // Enable tracking area for mouse moved events
        unsafe {
            let options = NSTrackingAreaOptions::NSTrackingMouseMoved
                | NSTrackingAreaOptions::NSTrackingActiveAlways
                | NSTrackingAreaOptions::NSTrackingInVisibleRect;
            let tracking_area = NSTrackingArea::initWithRect_options_owner_userInfo(
                mtm.alloc(),
                NSRect::ZERO,
                options,
                Some(&this),
                None,
            );
            this.addTrackingArea(&tracking_area);
        }

        this
    }

    /// Discard any in-progress IME composition by telling the input context
    /// to discard its marked text. This ensures the composition buffer doesn't
    /// carry over when switching panes.
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

    pub fn set_ime_cursor_rect(&self, x: f64, y: f64, w: f64, h: f64) {
        let frame = self.frame();
        let flipped_y = frame.size.height - y - h;
        self.ivars().ime_cursor_rect.set(NSRect::new(
            NSPoint::new(x, flipped_y),
            NSSize::new(w, h),
        ));
    }

    fn emit(&self, event: PlatformEvent) {
        super::app::with_main_window(|window| {
            // Use try_borrow_mut to avoid panics if the callback is re-entered
            // (e.g., waker's triggerRedraw firing during NSTextInputContext processing).
            if let Ok(mut cb) = self.ivars().callback.try_borrow_mut() {
                cb(event.clone(), window);
            }
        });
    }

    fn mouse_pos(&self, event: &NSEvent) -> (f64, f64) {
        let point = unsafe { event.locationInWindow() };
        let local: NSPoint = unsafe { msg_send![self, convertPoint: point, fromView: std::ptr::null::<NSView>()] };
        (local.x, local.y)
    }

    fn backing_scale(&self) -> f64 {
        unsafe {
            let window: Option<Retained<objc2_app_kit::NSWindow>> = msg_send_id![self, window];
            window.map(|w| {
                let s: f64 = msg_send![&w, backingScaleFactor];
                s
            }).unwrap_or(1.0)
        }
    }
}

// ──────────────────────────────────────────────
// Window delegate
// ──────────────────────────────────────────────

pub struct TideWindowDelegateIvars {
    callback: Rc<RefCell<EventCallback>>,
}

declare_class!(
    pub struct TideWindowDelegate;

    unsafe impl ClassType for TideWindowDelegate {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TideWindowDelegate";
    }

    impl DeclaredClass for TideWindowDelegate {
        type Ivars = TideWindowDelegateIvars;
    }

    unsafe impl TideWindowDelegate {
        #[method(windowShouldClose:)]
        fn window_should_close(&self, _sender: &AnyObject) -> Bool {
            self.emit(PlatformEvent::CloseRequested);
            Bool::NO
        }

        #[method(windowDidBecomeKey:)]
        fn window_did_become_key(&self, _notification: &objc2_foundation::NSNotification) {
            self.emit(PlatformEvent::Focused(true));
        }

        #[method(windowDidResignKey:)]
        fn window_did_resign_key(&self, _notification: &objc2_foundation::NSNotification) {
            self.emit(PlatformEvent::Focused(false));
        }

        #[method(windowDidEnterFullScreen:)]
        fn window_did_enter_full_screen(&self, _notification: &objc2_foundation::NSNotification) {
            self.emit(PlatformEvent::Fullscreen(true));
        }

        #[method(windowDidExitFullScreen:)]
        fn window_did_exit_full_screen(&self, _notification: &objc2_foundation::NSNotification) {
            self.emit(PlatformEvent::Fullscreen(false));
        }

        #[method(windowDidChangeBackingProperties:)]
        fn window_did_change_backing_properties(
            &self,
            notification: &objc2_foundation::NSNotification,
        ) {
            unsafe {
                let obj = notification.object().unwrap();
                let scale: f64 = msg_send![&*obj, backingScaleFactor];
                self.emit(PlatformEvent::ScaleFactorChanged(scale));
            }
        }
    }
);

impl TideWindowDelegate {
    pub fn new(callback: Rc<RefCell<EventCallback>>, mtm: MainThreadMarker) -> Retained<Self> {
        let this = mtm.alloc::<Self>().set_ivars(TideWindowDelegateIvars { callback });
        unsafe { msg_send_id![super(this), init] }
    }

    fn emit(&self, event: PlatformEvent) {
        super::app::with_main_window(|window| {
            if let Ok(mut cb) = self.ivars().callback.try_borrow_mut() {
                cb(event.clone(), window);
            }
        });
    }
}

// ──────────────────────────────────────────────
// Key mapping
// ──────────────────────────────────────────────

fn key_from_keycode(keycode: u16) -> Key {
    match keycode {
        0x00 => Key::Char('a'), 0x01 => Key::Char('s'), 0x02 => Key::Char('d'),
        0x03 => Key::Char('f'), 0x04 => Key::Char('h'), 0x05 => Key::Char('g'),
        0x06 => Key::Char('z'), 0x07 => Key::Char('x'), 0x08 => Key::Char('c'),
        0x09 => Key::Char('v'), 0x0B => Key::Char('b'), 0x0C => Key::Char('q'),
        0x0D => Key::Char('w'), 0x0E => Key::Char('e'), 0x0F => Key::Char('r'),
        0x10 => Key::Char('y'), 0x11 => Key::Char('t'), 0x12 => Key::Char('1'),
        0x13 => Key::Char('2'), 0x14 => Key::Char('3'), 0x15 => Key::Char('4'),
        0x16 => Key::Char('6'), 0x17 => Key::Char('5'), 0x19 => Key::Char('9'),
        0x1A => Key::Char('7'), 0x1C => Key::Char('8'), 0x1D => Key::Char('0'),
        0x1E => Key::Char(']'), 0x1F => Key::Char('o'), 0x20 => Key::Char('u'),
        0x21 => Key::Char('['), 0x22 => Key::Char('i'), 0x23 => Key::Char('p'),
        0x25 => Key::Char('l'), 0x26 => Key::Char('j'), 0x28 => Key::Char('k'),
        0x2A => Key::Char('\\'), 0x2B => Key::Char(','), 0x2C => Key::Char('/'),
        0x2D => Key::Char('n'), 0x2E => Key::Char('m'), 0x2F => Key::Char('.'),
        0x27 => Key::Char('\''), 0x29 => Key::Char(';'),
        0x18 => Key::Char('='), 0x1B => Key::Char('-'), 0x32 => Key::Char('`'),
        0x24 => Key::Enter, 0x30 => Key::Tab, 0x31 => Key::Char(' '),
        0x33 => Key::Backspace, 0x35 => Key::Escape, 0x75 => Key::Delete,
        0x7E => Key::Up, 0x7D => Key::Down, 0x7B => Key::Left, 0x7C => Key::Right,
        0x73 => Key::Home, 0x77 => Key::End, 0x74 => Key::PageUp, 0x79 => Key::PageDown,
        0x72 => Key::Insert,
        0x7A => Key::F(1), 0x78 => Key::F(2), 0x63 => Key::F(3), 0x76 => Key::F(4),
        0x60 => Key::F(5), 0x61 => Key::F(6), 0x62 => Key::F(7), 0x64 => Key::F(8),
        0x65 => Key::F(9), 0x6D => Key::F(10), 0x67 => Key::F(11), 0x6F => Key::F(12),
        _ => Key::Char('?'),
    }
}

fn key_and_modifiers_from_event(event: &NSEvent) -> (Key, Modifiers) {
    let keycode = unsafe { event.keyCode() };
    let flags = unsafe { event.modifierFlags() };
    let modifiers = modifiers_from_flags(flags);

    if modifiers.meta || modifiers.ctrl {
        return (key_from_keycode(keycode), modifiers);
    }

    let key = unsafe {
        event.characters().and_then(|s| {
            let s = s.to_string();
            let mut chars = s.chars();
            if let Some(c) = chars.next() {
                if chars.next().is_none() && !c.is_control() {
                    return Some(Key::Char(c));
                }
            }
            None
        })
    };

    (key.unwrap_or_else(|| key_from_keycode(keycode)), modifiers)
}

fn modifiers_from_flags(flags: NSEventModifierFlags) -> Modifiers {
    Modifiers {
        shift: flags.contains(NSEventModifierFlags::NSEventModifierFlagShift),
        ctrl: flags.contains(NSEventModifierFlags::NSEventModifierFlagControl),
        alt: flags.contains(NSEventModifierFlags::NSEventModifierFlagOption),
        meta: flags.contains(NSEventModifierFlags::NSEventModifierFlagCommand),
    }
}

/// Extract a Rust String from an ObjC object that is either NSString or NSAttributedString.
/// Used by insertText: and setMarkedText: which can receive either type.
fn nsstring_from_anyobject(obj: &AnyObject) -> String {
    unsafe {
        // insertText: / setMarkedText: receive either NSString or NSAttributedString.
        // Check the type first to avoid sending unrecognized selectors.
        let nsstring_cls = NSString::class();
        let is_string: Bool = msg_send![obj, isKindOfClass: nsstring_cls];
        if is_string.as_bool() {
            // Directly reinterpret as NSString
            let s = &*(obj as *const AnyObject as *const NSString);
            return s.to_string();
        }
        // NSAttributedString — call -string to get the plain text
        let s: Retained<NSString> = msg_send_id![obj, string];
        s.to_string()
    }
}
