//! NSWindow wrapper implementing PlatformWindow.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use objc2::rc::Retained;
use objc2::msg_send;
use objc2_foundation::MainThreadMarker;
use objc2_app_kit::{
    NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{CGFloat, NSPoint, NSRect, NSSize, NSString};
use raw_window_handle::{
    AppKitDisplayHandle, AppKitWindowHandle, DisplayHandle, HandleError, HasDisplayHandle,
    HasWindowHandle, RawDisplayHandle, RawWindowHandle, WindowHandle,
};

use crate::{CursorIcon, EventCallback, PlatformWindow, WindowConfig};

use super::ime_proxy::ImeProxyView;
use super::view::TideView;

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

        let ns_window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                mtm.alloc(),
                content_rect,
                style,
                NSBackingStoreType::NSBackingStoreBuffered,
                false,
            )
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
                AnyClass::get("NSColor").unwrap(),
                colorWithRed: 0.08_f64,
                green: 0.08_f64,
                blue: 0.10_f64,
                alpha: 1.0_f64
            ];
            let _: () = msg_send![&ns_window, setBackgroundColor: &*bg_color];
        }

        // Center and show — dark background is already set so no white flash
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
        // For CAMetalLayer-backed views, setNeedsDisplay doesn't trigger drawRect:.
        // Rendering is driven directly by the event loop (handle_platform_event)
        // and by the waker (triggerRedraw via performSelectorOnMainThread).
        // This method exists for the trait but the main render path doesn't use it.
        unsafe { self.view.setNeedsDisplay(true); }
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
}
