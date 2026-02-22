//! macOS native platform backend using objc2.

mod app;
pub(crate) mod ime_proxy;
mod view;
pub mod webview;
mod window;

pub use app::MacosApp;
pub use window::MacosWindow;

use std::cell::RefCell;
use std::rc::Rc;

use crate::{EventCallback, PlatformEvent};

/// Emit a platform event through the callback, catching panics at the FFI boundary.
///
/// Objective-C → Rust callbacks abort the process on panic, so we wrap every
/// event emission in `catch_unwind`. Re-entrancy (e.g. waker firing during
/// NSTextInputContext processing) is handled via `try_borrow_mut`.
pub(crate) fn emit_event(
    callback: &Rc<RefCell<EventCallback>>,
    event: PlatformEvent,
    source: &str,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app::with_main_window(|window| {
            if let Ok(mut cb) = callback.try_borrow_mut() {
                cb(event.clone(), window);
            } else {
                log::warn!("{source}: event dropped (re-entrancy): {event:?}");
            }
        });
    }));
    if let Err(e) = result {
        let msg = if let Some(s) = e.downcast_ref::<&str>() {
            s.to_string()
        } else if let Some(s) = e.downcast_ref::<String>() {
            s.clone()
        } else {
            "unknown panic".to_string()
        };
        eprintln!("[tide] PANIC in {source} callback: {msg}");
    }
}
