//! NSApplication setup and main event loop.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicPtr, Ordering};

use objc2::rc::Retained;
use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
use objc2_foundation::MainThreadMarker;

use crate::{EventCallback, WakeCallback, WindowConfig};

use super::window::MacosWindow;

/// Global view pointer so background-thread wakers can trigger redraws
/// via `performSelectorOnMainThread`.
static GLOBAL_VIEW: AtomicPtr<std::ffi::c_void> = AtomicPtr::new(std::ptr::null_mut());

/// macOS platform entry point.
pub struct MacosApp;

impl MacosApp {
    /// Create the NSApplication, window, and run the main event loop.
    ///
    /// `callback` is invoked for every platform event (key, mouse, IME, resize, etc.).
    /// This function does **not** return — it calls `[NSApp run]`.
    pub fn run(config: WindowConfig, callback: EventCallback) -> ! {
        let mtm = MainThreadMarker::new()
            .expect("MacosApp::run must be called from the main thread");

        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Regular);

        // Create window + view
        let callback = Rc::new(RefCell::new(callback));
        let window = MacosWindow::new(&config, Rc::clone(&callback), mtm);

        // Store the view pointer globally so wakers can trigger redraws
        GLOBAL_VIEW.store(
            Retained::as_ptr(&window.view) as *mut std::ffi::c_void,
            Ordering::Release,
        );

        // Store the window so it lives as long as the app
        MAIN_WINDOW.with(|cell| {
            cell.replace(Some(window));
        });

        app.activateIgnoringOtherApps(true);

        // Run the event loop (never returns)
        unsafe { app.run(); }

        unreachable!("NSApp.run() should never return")
    }

    /// Create a waker that can be sent to background threads.
    /// When invoked, it wakes the run loop and triggers a redraw.
    pub fn create_waker() -> WakeCallback {
        std::sync::Arc::new(move || {
            unsafe {
                use objc2::msg_send_id;
                use objc2::rc::Retained;
                use objc2::runtime::AnyClass;
                use objc2_app_kit::NSEvent;

                // Post an application-defined event to wake CFRunLoop
                let cls = AnyClass::get("NSEvent").unwrap();
                let event: Option<Retained<NSEvent>> = msg_send_id![
                    cls,
                    otherEventWithType: 15_usize, // NSEventTypeApplicationDefined
                    location: objc2_foundation::NSPoint::new(0.0, 0.0),
                    modifierFlags: 0_usize,
                    timestamp: 0.0_f64,
                    windowNumber: 0_isize,
                    context: std::ptr::null::<objc2::runtime::AnyObject>(),
                    subtype: 0_i16,
                    data1: 0_isize,
                    data2: 0_isize
                ];
                if let Some(event) = event {
                    if let Some(mtm) = objc2_foundation::MainThreadMarker::new() {
                        let app = NSApplication::sharedApplication(mtm);
                        app.postEvent_atStart(&event, false);
                    }
                }

                // Also trigger a redraw on the view via performSelectorOnMainThread.
                // This ensures setNeedsDisplay is called on the main thread,
                // which will trigger drawRect: → RedrawRequested on the next display cycle.
                let view_raw = GLOBAL_VIEW.load(Ordering::Acquire);
                if !view_raw.is_null() {
                    let _: () = objc2::msg_send![
                        view_raw as *const objc2::runtime::AnyObject,
                        performSelectorOnMainThread: objc2::sel!(triggerRedraw),
                        withObject: std::ptr::null::<objc2::runtime::AnyObject>(),
                        waitUntilDone: false
                    ];
                }
            }
        })
    }
}

thread_local! {
    static MAIN_WINDOW: RefCell<Option<MacosWindow>> = RefCell::new(None);
}

/// Access the main window from within the run loop.
pub(crate) fn with_main_window<R>(f: impl FnOnce(&MacosWindow) -> R) -> Option<R> {
    MAIN_WINDOW.with(|cell| {
        let borrow = cell.borrow();
        borrow.as_ref().map(f)
    })
}
