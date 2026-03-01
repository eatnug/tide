//! NSWindow wrapper implementing PlatformWindow.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool};
use objc2::{declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_foundation::MainThreadMarker;
use objc2_app_kit::{
    NSBackingStoreType, NSView, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{
    CGFloat, NSMutableArray, NSPoint, NSRect, NSSize, NSString,
};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};

use crate::{CursorIcon, EventCallback, PlatformWindow, WindowConfig};

/// Initial window background color (dark gray) to avoid white flash before
/// the first GPU frame renders. RGB values in 0.0–1.0 range.
const INITIAL_BG_RED: f64 = 0.08;
const INITIAL_BG_GREEN: f64 = 0.08;
const INITIAL_BG_BLUE: f64 = 0.10;

use super::ime_proxy::ImeProxyView;
use super::view::TideView;

// ── TideWindow: NSWindow subclass for accessibility ──

declare_class!(
    pub struct TideWindow;

    unsafe impl ClassType for TideWindow {
        type Super = NSWindow;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TideWindow";
    }

    impl DeclaredClass for TideWindow {
        type Ivars = ();
    }

    unsafe impl TideWindow {
        /// Legacy accessibility API override. macOS queries NSWindow
        /// accessibility attributes via this method, not the modern
        /// property-based API.
        #[method_id(accessibilityAttributeValue:)]
        fn accessibility_attribute_value(
            &self,
            attribute: &NSString,
        ) -> Option<Retained<AnyObject>> {
            unsafe {
                let attr = attribute.to_string();

                if attr == "AXFocusedUIElement" {
                    let responder: Option<Retained<AnyObject>> =
                        msg_send_id![self, firstResponder];
                    let ime_cls = objc2::runtime::AnyClass::get("ImeProxyView");
                    let is_ime_proxy = responder.as_ref().map_or(false, |r| {
                        ime_cls.map_or(false, |c| {
                            let yes: Bool = msg_send![&**r, isKindOfClass: c];
                            yes.as_bool()
                        })
                    });
                    if is_ime_proxy {
                        responder
                    } else {
                        msg_send_id![super(self), accessibilityAttributeValue: attribute]
                    }
                } else if attr == "AXChildren" {
                    let arr: Retained<NSMutableArray> = msg_send_id![
                        objc2::runtime::AnyClass::get("NSMutableArray")
                            .expect("NSMutableArray"),
                        new
                    ];
                    let default: Option<Retained<AnyObject>> =
                        msg_send_id![super(self), accessibilityAttributeValue: attribute];
                    if let Some(ref def) = default {
                        // default is an NSArray
                        let _: () = msg_send![&*arr, addObjectsFromArray: &**def];
                    }
                    let content_view: Option<Retained<NSView>> =
                        msg_send_id![self, contentView];
                    if let Some(cv) = content_view {
                        let subviews = cv.subviews();
                        let ime_cls = objc2::runtime::AnyClass::get("ImeProxyView");
                        for i in 0..subviews.len() {
                            let sv = subviews.objectAtIndex(i);
                            let is_ime = ime_cls.map_or(false, |c| {
                                let yes: Bool = msg_send![&*sv, isKindOfClass: c];
                                yes.as_bool()
                            });
                            if is_ime {
                                let _: () = msg_send![&*arr, addObject: &*sv];
                            }
                        }
                    }
                    let obj: Retained<AnyObject> = Retained::cast(arr);
                    Some(obj)
                } else {
                    msg_send_id![super(self), accessibilityAttributeValue: attribute]
                }
            }
        }
    }
);

/// macOS window backed by NSWindow + TideView.
pub struct MacosWindow {
    pub(crate) ns_window: Retained<NSWindow>,
    pub(crate) view: Retained<TideView>,
    callback: Rc<RefCell<EventCallback>>,
    mtm: MainThreadMarker,
    ime_proxies: RefCell<HashMap<u64, Retained<ImeProxyView>>>,
}

impl MacosWindow {
    pub fn new(
        config: &WindowConfig,
        callback: Rc<RefCell<EventCallback>>,
        mtm: MainThreadMarker,
    ) -> Self {
        let content_rect = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(config.width as CGFloat, config.height as CGFloat),
        );

        let mut style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable
            | NSWindowStyleMask::Resizable;

        if config.transparent_titlebar {
            style |= NSWindowStyleMask::FullSizeContentView;
        }

        let ns_window: Retained<NSWindow> = unsafe {
            let allocated = mtm.alloc::<TideWindow>().set_ivars(());
            let window: Retained<TideWindow> = msg_send_id![
                super(allocated),
                initWithContentRect: content_rect,
                styleMask: style,
                backing: NSBackingStoreType::NSBackingStoreBuffered,
                defer: false
            ];
            Retained::into_super(window)
        };
        if config.transparent_titlebar {
            ns_window.setTitlebarAppearsTransparent(true);
            ns_window.setTitleVisibility(
                objc2_app_kit::NSWindowTitleVisibility::NSWindowTitleHidden,
            );
        }

        // Set minimum size
        ns_window.setMinSize(NSSize::new(
            config.min_width as CGFloat,
            config.min_height as CGFloat,
        ));

        // Set title
        let title = NSString::from_str(&config.title);
        ns_window.setTitle(&title);

        // Create our custom NSView
        let view = TideView::new(Rc::clone(&callback), mtm);

        // Set as content view
        ns_window.setContentView(Some(&view));
        // makeFirstResponder expects &NSResponder
        let responder: &objc2_app_kit::NSResponder = &view;
        ns_window.makeFirstResponder(Some(responder));

        // Set dark background to avoid white flash before first GPU frame
        unsafe {
            use objc2::msg_send_id;
            use objc2::runtime::AnyClass;
            let bg_color: Retained<objc2::runtime::AnyObject> = msg_send_id![
                AnyClass::get("NSColor").expect("NSColor class must exist"),
                colorWithRed: INITIAL_BG_RED,
                green: INITIAL_BG_GREEN,
                blue: INITIAL_BG_BLUE,
                alpha: 1.0_f64
            ];
            let _: () = msg_send![&ns_window, setBackgroundColor: &*bg_color];
        }

        // Start invisible — show_window() reveals after the first frame renders,
        // so the user never sees a blank window during GPU initialization.
        unsafe {
            let _: () = msg_send![&ns_window, setAlphaValue: 0.0_f64];
        }

        // Center and show (invisible due to alpha=0, but events still flow)
        ns_window.center();
        ns_window.makeKeyAndOrderFront(None);

        // Set the window delegate for resize/focus/close events
        let delegate = super::view::TideWindowDelegate::new(Rc::clone(&callback), mtm);
        unsafe {
            let _: () = msg_send![&ns_window, setDelegate: &*delegate];
        }
        // Keep the delegate alive by leaking it (lives for the entire app)
        std::mem::forget(delegate);

        MacosWindow {
            ns_window,
            view,
            callback: Rc::clone(&callback),
            mtm,
            ime_proxies: RefCell::new(HashMap::new()),
        }
    }
}

impl HasWindowHandle for MacosWindow {
    fn window_handle(&self) -> Result<WindowHandle<'_>, HandleError> {
        let ns_view_ptr = Retained::as_ptr(&self.view) as *mut std::ffi::c_void;
        let handle = AppKitWindowHandle::new(
            std::ptr::NonNull::new(ns_view_ptr).expect("view pointer is non-null"),
        );
        let raw = RawWindowHandle::AppKit(handle);
        Ok(unsafe { WindowHandle::borrow_raw(raw) })
    }
}

impl HasDisplayHandle for MacosWindow {
    fn display_handle(&self) -> Result<DisplayHandle<'_>, HandleError> {
        let handle = AppKitDisplayHandle::new();
        let raw = RawDisplayHandle::AppKit(handle);
        Ok(unsafe { DisplayHandle::borrow_raw(raw) })
    }
}

impl PlatformWindow for MacosWindow {
    fn request_redraw(&self) {
        // Schedule triggerRedraw on the main thread (same mechanism the waker uses).
        // The RedrawRequested handler checks `needs_redraw` to skip redundant renders.
        unsafe {
            let _: () = objc2::msg_send![
                &*self.view,
                performSelectorOnMainThread: objc2::sel!(triggerRedraw),
                withObject: std::ptr::null::<objc2::runtime::AnyObject>(),
                waitUntilDone: false
            ];
        }
    }

    fn set_cursor_icon(&self, icon: CursorIcon) {
        unsafe {
            use objc2_app_kit::NSCursor;
            let cursor = match icon {
                CursorIcon::Default => NSCursor::arrowCursor(),
                CursorIcon::Pointer => NSCursor::pointingHandCursor(),
                CursorIcon::Grab => NSCursor::openHandCursor(),
                CursorIcon::ColResize => NSCursor::resizeLeftRightCursor(),
                CursorIcon::RowResize => NSCursor::resizeUpDownCursor(),
            };
            cursor.set();
        }
    }

    fn inner_size(&self) -> (u32, u32) {
        let frame = self.view.frame();
        let scale = self.scale_factor();
        (
            (frame.size.width * scale) as u32,
            (frame.size.height * scale) as u32,
        )
    }

    fn scale_factor(&self) -> f64 {
        unsafe {
            let backing: CGFloat = msg_send![&self.ns_window, backingScaleFactor];
            backing
        }
    }

    fn set_fullscreen(&self, fullscreen: bool) {
        let is_fs = self.is_fullscreen();
        if fullscreen != is_fs {
            self.ns_window.toggleFullScreen(None);
        }
    }

    fn is_fullscreen(&self) -> bool {
        let mask = self.ns_window.styleMask();
        mask.contains(NSWindowStyleMask::FullScreen)
    }

    fn create_ime_proxy(&self, pane_id: u64) {
        let mut proxies = self.ime_proxies.borrow_mut();
        if proxies.contains_key(&pane_id) {
            return;
        }
        let proxy = ImeProxyView::new(Rc::clone(&self.callback), self.mtm);
        unsafe { self.view.addSubview(&proxy) };
        proxies.insert(pane_id, proxy);
    }

    fn remove_ime_proxy(&self, pane_id: u64) {
        let mut proxies = self.ime_proxies.borrow_mut();
        if let Some(proxy) = proxies.remove(&pane_id) {
            unsafe { proxy.removeFromSuperview() };
        }
    }

    fn focus_ime_proxy(&self, pane_id: u64) {
        super::LAST_IME_TARGET.store(pane_id, std::sync::atomic::Ordering::Relaxed);
        let proxies = self.ime_proxies.borrow();
        if let Some(proxy) = proxies.get(&pane_id) {
            let responder: &objc2_app_kit::NSResponder = proxy;
            self.ns_window.makeFirstResponder(Some(responder));
        }
    }

    fn set_ime_proxy_cursor_area(&self, pane_id: u64, x: f64, y: f64, w: f64, h: f64) {
        let proxies = self.ime_proxies.borrow();
        if let Some(proxy) = proxies.get(&pane_id) {
            proxy.set_ime_cursor_rect(x, y, w, h);
        }
    }

    fn content_view_ptr(&self) -> Option<*mut std::ffi::c_void> {
        Some(Retained::as_ptr(&self.view) as *mut std::ffi::c_void)
    }

    fn window_ptr(&self) -> Option<*mut std::ffi::c_void> {
        Some(Retained::as_ptr(&self.ns_window) as *mut std::ffi::c_void)
    }

    fn show_window(&self) {
        unsafe {
            let _: () = msg_send![&self.ns_window, setAlphaValue: 1.0_f64];
        }
    }
}
