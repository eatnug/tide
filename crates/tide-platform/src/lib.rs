//! Platform abstraction layer for Tide.
//!
//! Provides native windowing, input, and IME support via platform-specific backends.
//! Currently implements macOS via `objc2`; Windows/Linux backends can be added later.

#[cfg(target_os = "macos")]
pub mod macos;

use raw_window_handle::{HasDisplayHandle, HasWindowHandle};
use tide_core::{Key, Modifiers};

// ──────────────────────────────────────────────
// Platform Events
// ──────────────────────────────────────────────

/// Platform-agnostic event delivered by the native backend.
#[derive(Debug, Clone)]
pub enum PlatformEvent {
    /// A key was pressed. `chars` contains the text produced (if any).
    KeyDown {
        key: Key,
        modifiers: Modifiers,
        chars: Option<String>,
    },
    /// A key was released.
    KeyUp {
        key: Key,
        modifiers: Modifiers,
    },
    /// Modifier key state changed (Shift, Ctrl, Alt, Meta).
    ModifiersChanged(Modifiers),

    // ── IME ──
    /// IME committed final text (composition done).
    ImeCommit(String),
    /// IME preedit (composition in progress).
    ImePreedit {
        text: String,
        cursor: Option<usize>,
    },

    // ── Mouse ──
    MouseDown {
        button: MouseButton,
        position: (f64, f64),
    },
    MouseUp {
        button: MouseButton,
        position: (f64, f64),
    },
    MouseMoved {
        position: (f64, f64),
    },
    Scroll {
        dx: f32,
        dy: f32,
        position: (f64, f64),
    },

    // ── Window ──
    Resized {
        width: u32,
        height: u32,
    },
    ScaleFactorChanged(f64),
    Focused(bool),
    CloseRequested,
    RedrawRequested,
    Fullscreen {
        is_fullscreen: bool,
        /// Current window inner size at the time of the fullscreen transition.
        /// Included so the app thread doesn't need to query the window.
        width: u32,
        height: u32,
    },
    /// The window's occlusion state changed (fully obscured or visible again).
    Occluded(bool),

    /// The window's first responder is a non-Tide view (e.g. WKWebView).
    /// Emitted from performKeyEquivalent so the app can update focus state
    /// before processing the shortcut.
    WebViewFocused,

    /// The window was moved (position changed on screen).
    /// Used to reposition embedded app windows.
    WindowMoved,

    /// Begin an event batch: suppress rendering until the matching `BatchEnd`.
    /// Used by ImeProxyView to flush deferred IME events atomically so that
    /// intermediate states (e.g. Backspace before replacement commit) never
    /// render a partial frame.
    BatchStart,
    /// End an event batch and allow rendering to proceed.
    BatchEnd,
}

/// Mouse button identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    Other(u16),
}

// ──────────────────────────────────────────────
// Cursor icons
// ──────────────────────────────────────────────

/// Platform-agnostic cursor icon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CursorIcon {
    Default,
    Pointer,
    Grab,
    ColResize,
    RowResize,
}

// ──────────────────────────────────────────────
// Window trait
// ──────────────────────────────────────────────

/// A platform window that can be used for rendering and input.
pub trait PlatformWindow: HasWindowHandle + HasDisplayHandle {
    fn request_redraw(&self);
    fn set_cursor_icon(&self, icon: CursorIcon);
    fn inner_size(&self) -> (u32, u32);
    fn scale_factor(&self) -> f64;
    fn set_fullscreen(&self, fullscreen: bool);
    fn is_fullscreen(&self) -> bool;

    // ── Per-pane IME proxy management ──

    /// Create an IME proxy view for the given pane. Idempotent.
    fn create_ime_proxy(&self, pane_id: u64);
    /// Remove the IME proxy view for the given pane. No-op if not present.
    fn remove_ime_proxy(&self, pane_id: u64);
    /// Make the proxy for the given pane the first responder (receives keyboard/IME).
    /// Triggers `unmarkText` on the previously focused proxy, clearing any
    /// in-progress IME composition.
    fn focus_ime_proxy(&self, pane_id: u64);
    /// Update the IME candidate window position for a specific pane's proxy.
    fn set_ime_proxy_cursor_area(&self, pane_id: u64, x: f64, y: f64, w: f64, h: f64);

    /// Return a raw pointer to the content NSView (macOS) for subview management.
    /// Returns `None` on platforms that don't support native subviews.
    fn content_view_ptr(&self) -> Option<*mut std::ffi::c_void> {
        None
    }

    /// Return a raw pointer to the NSWindow (macOS) for first responder management.
    /// Returns `None` on platforms that don't support this.
    fn window_ptr(&self) -> Option<*mut std::ffi::c_void> {
        None
    }

    /// Reveal the window (set alpha to 1). Called after the first frame renders
    /// so the user never sees a blank window during GPU initialization.
    fn show_window(&self) {}
}

// ──────────────────────────────────────────────
// Window configuration
// ──────────────────────────────────────────────

/// Configuration for creating a platform window.
pub struct WindowConfig {
    pub title: String,
    pub width: f64,
    pub height: f64,
    pub min_width: f64,
    pub min_height: f64,
    pub transparent_titlebar: bool,
}

impl Default for WindowConfig {
    fn default() -> Self {
        Self {
            title: "Tide".to_string(),
            width: 960.0,
            height: 640.0,
            min_width: 400.0,
            min_height: 300.0,
            transparent_titlebar: true,
        }
    }
}

// ──────────────────────────────────────────────
// App callback
// ──────────────────────────────────────────────

/// Callback invoked by the platform for each event.
/// The `&dyn PlatformWindow` reference is valid for the duration of the call.
pub type EventCallback = Box<dyn FnMut(PlatformEvent, &dyn PlatformWindow)>;

/// Callback to wake the event loop from a background thread.
/// Uses Arc so it can be cloned and sent to multiple background threads.
pub type WakeCallback = std::sync::Arc<dyn Fn() + Send + Sync + 'static>;

// ──────────────────────────────────────────────
// Window commands (app thread → main thread)
// ──────────────────────────────────────────────

/// Commands that the app thread sends to the main thread for execution.
/// These wrap all `PlatformWindow` methods that mutate UI state.
#[derive(Debug)]
pub enum WindowCommand {
    RequestRedraw,
    ShowWindow,
    SetFullscreen(bool),
    SetCursorIcon(CursorIcon),
    CreateImeProxy(u64),
    RemoveImeProxy(u64),
    FocusImeProxy(u64),
    SetImeCursorArea {
        pane_id: u64,
        x: f64,
        y: f64,
        w: f64,
        h: f64,
    },
}

/// Execute a `WindowCommand` on the main thread using the actual window.
pub fn execute_window_command(window: &dyn PlatformWindow, cmd: WindowCommand) {
    match cmd {
        WindowCommand::RequestRedraw => window.request_redraw(),
        WindowCommand::ShowWindow => window.show_window(),
        WindowCommand::SetFullscreen(fs) => window.set_fullscreen(fs),
        WindowCommand::SetCursorIcon(icon) => window.set_cursor_icon(icon),
        WindowCommand::CreateImeProxy(id) => window.create_ime_proxy(id),
        WindowCommand::RemoveImeProxy(id) => window.remove_ime_proxy(id),
        WindowCommand::FocusImeProxy(id) => window.focus_ime_proxy(id),
        WindowCommand::SetImeCursorArea { pane_id, x, y, w, h } => {
            window.set_ime_proxy_cursor_area(pane_id, x, y, w, h);
        }
    }
}

// ──────────────────────────────────────────────
// Window proxy (Send, for app thread)
// ──────────────────────────────────────────────

/// A thread-safe proxy for sending window commands from the app thread.
/// Commands are queued and executed on the main thread.
#[derive(Clone)]
pub struct WindowProxy {
    cmd_tx: std::sync::mpsc::Sender<WindowCommand>,
    waker: WakeCallback,
}

impl WindowProxy {
    pub fn new(
        cmd_tx: std::sync::mpsc::Sender<WindowCommand>,
        waker: WakeCallback,
    ) -> Self {
        Self { cmd_tx, waker }
    }

    fn send(&self, cmd: WindowCommand) {
        let _ = self.cmd_tx.send(cmd);
    }

    /// Send commands and wake the main thread to execute them.
    fn send_and_wake(&self, cmd: WindowCommand) {
        self.send(cmd);
        (self.waker)();
    }

    pub fn request_redraw(&self) {
        self.send_and_wake(WindowCommand::RequestRedraw);
    }

    pub fn show_window(&self) {
        self.send_and_wake(WindowCommand::ShowWindow);
    }

    pub fn set_fullscreen(&self, fullscreen: bool) {
        self.send_and_wake(WindowCommand::SetFullscreen(fullscreen));
    }

    pub fn set_cursor_icon(&self, icon: CursorIcon) {
        self.send(WindowCommand::SetCursorIcon(icon));
    }

    pub fn create_ime_proxy(&self, pane_id: u64) {
        self.send_and_wake(WindowCommand::CreateImeProxy(pane_id));
    }

    pub fn remove_ime_proxy(&self, pane_id: u64) {
        self.send_and_wake(WindowCommand::RemoveImeProxy(pane_id));
    }

    pub fn focus_ime_proxy(&self, pane_id: u64) {
        self.send_and_wake(WindowCommand::FocusImeProxy(pane_id));
    }

    pub fn set_ime_proxy_cursor_area(&self, pane_id: u64, x: f64, y: f64, w: f64, h: f64) {
        self.send(WindowCommand::SetImeCursorArea { pane_id, x, y, w, h });
    }
}
