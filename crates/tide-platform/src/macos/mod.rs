//! macOS native platform backend using objc2.

mod app;
pub mod cgs;
pub(crate) mod ime_proxy;
mod view;
pub mod webview;
mod window;

pub use app::MacosApp;
pub use window::MacosWindow;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::AtomicU64;

use crate::{EventCallback, PlatformEvent};

/// Last IME target pane ID, updated by `focus_ime_proxy`.
/// Used by `windowDidBecomeKey` to re-establish the first responder immediately
/// on the main thread, without waiting for the app thread round-trip.
pub(crate) static LAST_IME_TARGET: AtomicU64 = AtomicU64::new(0);

thread_local! {
    /// Queue for events that arrive during re-entrancy (callback already borrowed).
    /// Drained after the outer callback returns, so no events are lost.
    static REENTRANT_QUEUE: RefCell<Vec<PlatformEvent>> = RefCell::new(Vec::new());
}

/// Emit a platform event through the callback, catching panics at the FFI boundary.
///
/// Objective-C → Rust callbacks abort the process on panic, so we wrap every
/// event emission in `catch_unwind`. Re-entrant events (e.g. waker firing during
/// NSTextInputContext processing) are queued and drained after the outer callback
/// returns, so they are never lost.
pub(crate) fn emit_event(
    callback: &Rc<RefCell<EventCallback>>,
    event: PlatformEvent,
    source: &str,
) {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        app::with_main_window(|window| {
            if let Ok(mut cb) = callback.try_borrow_mut() {
                cb(event.clone(), window);

                // Drain any events that were queued during re-entrancy.
                // Loop until empty because processing queued events may
                // trigger further re-entrant events.
                loop {
                    let queued: Vec<PlatformEvent> = REENTRANT_QUEUE.with(|q| {
                        let mut q = q.borrow_mut();
                        if q.is_empty() { return Vec::new(); }
                        std::mem::take(&mut *q)
                    });
                    if queued.is_empty() {
                        break;
                    }
                    for queued_event in queued {
                        cb(queued_event, window);
                    }
                }
            } else {
                // Callback is already borrowed — queue for later delivery.
                //
                // Empty ImePreedit events during re-entrancy are artifacts of
                // internal focus management: makeFirstResponder → resignFirstResponder
                // → unmarkText emits ImePreedit("").  These must be dropped (not
                // queued) to prevent resetting Korean IME composition state.
                // Before the re-entrancy queue was added, ALL re-entrant events
                // were dropped, which masked this.
                if matches!(&event, PlatformEvent::ImePreedit { text, .. } if text.is_empty()) {
                    log::trace!("{source}: dropping empty ImePreedit (re-entrancy artifact)");
                } else {
                    log::trace!("{source}: event queued (re-entrancy): {event:?}");
                    REENTRANT_QUEUE.with(|q| {
                        q.borrow_mut().push(event.clone());
                    });
                }
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
