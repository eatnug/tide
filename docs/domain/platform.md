# Platform — tide-platform

**Role**: Anti-Corruption Layer between native macOS and the domain.
Translates OS events into domain events. The only crate that touches Objective-C.

`crates/tide-platform/src/`

## PlatformEvent (20 variants)

The only way the outside world enters the system.

### Keyboard & IME
| Variant | Fields | Description |
|---------|--------|-------------|
| `KeyDown` | `key, modifiers, chars` | Key press with optional text |
| `KeyUp` | `key, modifiers` | Key release |
| `ModifiersChanged` | `Modifiers` | Shift/Ctrl/Alt/Meta state changed |
| `ImeCommit` | `String` | IME composition confirmed (final text) |
| `ImePreedit` | `text, cursor` | IME composition in progress (uncommitted) |

### Mouse
| Variant | Fields | Description |
|---------|--------|-------------|
| `MouseDown` | `button, position` | Button pressed |
| `MouseUp` | `button, position` | Button released |
| `MouseMoved` | `position` | Cursor moved |
| `Scroll` | `dx, dy, position` | Scroll wheel / trackpad |

### Window
| Variant | Fields | Description |
|---------|--------|-------------|
| `Resized` | `width, height` | Window resized |
| `ScaleFactorChanged` | `f64` | Display DPI changed |
| `Focused` | `bool` | Window gained/lost focus |
| `CloseRequested` | — | User clicked close button |
| `RedrawRequested` | — | OS wants a redraw |
| `Fullscreen` | `is_fullscreen, width, height` | Fullscreen transition |
| `Occluded` | `bool` | Window fully hidden/visible |

### Batching & WebView
| Variant | Description |
|---------|-------------|
| `BatchStart` | Begin event batch (suppress rendering until BatchEnd) |
| `BatchEnd` | End event batch |
| `WebViewFocused` | First responder is WebView, not Tide |

## Trait: PlatformWindow

The contract between App and the native window.

```rust
trait PlatformWindow {
    // Window management
    fn request_redraw(&self);
    fn inner_size(&self) -> (u32, u32);
    fn scale_factor(&self) -> f64;
    fn set_fullscreen(&self, fullscreen: bool);
    fn is_fullscreen(&self) -> bool;
    fn set_cursor_icon(&self, icon: CursorIcon);
    fn show_window(&self);

    // IME proxy lifecycle (per-Pane)
    fn create_ime_proxy(&self, pane_id: u64);
    fn remove_ime_proxy(&self, pane_id: u64);
    fn focus_ime_proxy(&self, pane_id: u64);
    fn set_ime_proxy_cursor_area(&self, pane_id: u64, x, y, w, h: f64);
}
```

## WindowCommand (8 variants)

App → Platform direction. Sent through a command channel.

| Command | Description |
|---------|-------------|
| `RequestRedraw` | Request next frame |
| `ShowWindow` | Reveal window (initially invisible) |
| `SetFullscreen(bool)` | Toggle fullscreen |
| `SetCursorIcon(CursorIcon)` | Change mouse cursor |
| `CreateImeProxy(pane_id)` | Create per-Pane IME proxy NSView |
| `RemoveImeProxy(pane_id)` | Remove IME proxy |
| `FocusImeProxy(pane_id)` | Make proxy first responder |
| `SetImeCursorArea { pane_id, x, y, w, h }` | Position IME candidate window |

## macOS Implementation

### File Structure
| File | Purpose |
|------|---------|
| `macos/app.rs` | NSApplication setup, event loop |
| `macos/view.rs` | TideView (main NSView), keyboard/mouse dispatch |
| `macos/window.rs` | MacosWindow, PlatformWindow trait impl |
| `macos/ime_proxy.rs` | ImeProxyView — per-Pane NSTextInputClient |
| `macos/webview.rs` | WKWebView integration |

### Key Mechanisms

**IME Proxy Pattern**: Each Pane gets an invisible `ImeProxyView` subview that implements `NSTextInputClient`. `focus_ime_proxy()` calls `makeFirstResponder:` to route IME events to the correct Pane.

**Re-entrancy Safety**: `REENTRANT_QUEUE` catches events that fire during callback execution. Empty ImePreedit events during re-entrancy are dropped.

**Window Initialization**: Window starts at alpha=0 (invisible). After first GPU frame, `show_window()` reveals it. This avoids the white flash during GPU initialization.

**CRITICAL**: `focus_ime_proxy()` must be called on every event. macOS can unpredictably change the first responder, causing total keyboard input loss.
