//! WKWebView wrapper for embedded browser functionality.
//!
//! Uses raw `objc2` message sends to interact with WebKit classes,
//! avoiding a direct WebKit crate dependency.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{msg_send, msg_send_id};
use objc2_foundation::{CGFloat, NSRect, NSPoint, NSSize, NSString};

/// Handle to a WKWebView instance, added as a subview of the parent NSView.
pub struct WebViewHandle {
    webview: Retained<AnyObject>,
}

impl WebViewHandle {
    /// Create a new WKWebView and add it as a subview of the given parent NSView.
    ///
    /// # Safety
    /// `parent_view` must be a valid pointer to an NSView that outlives this handle.
    pub unsafe fn new(parent_view: *mut std::ffi::c_void) -> Option<Self> {
        let parent: &AnyObject = &*(parent_view as *const AnyObject);

        // WKWebViewConfiguration
        let config_cls = AnyClass::get("WKWebViewConfiguration")?;
        let config: Retained<AnyObject> = msg_send_id![config_cls, new];

        // WKWebView initWithFrame:configuration:
        let wk_cls = AnyClass::get("WKWebView")?;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0));
        let webview: Retained<AnyObject> = msg_send_id![
            msg_send_id![wk_cls, alloc],
            initWithFrame: frame,
            configuration: &*config
        ];

        // Disable opaque background so rounded corners etc. work
        let _: () = msg_send![&webview, setOpaque: Bool::NO];

        // Hide initially until frame is set
        let _: () = msg_send![&webview, setHidden: Bool::YES];

        // Add as subview
        let _: () = msg_send![parent, addSubview: &*webview];

        Some(Self { webview })
    }

    /// Navigate to a URL string.
    pub fn navigate(&self, url: &str) {
        unsafe {
            let url_cls = AnyClass::get("NSURL").expect("NSURL class");
            let ns_url_str = NSString::from_str(url);
            let nsurl: Retained<AnyObject> =
                msg_send_id![url_cls, URLWithString: &*ns_url_str];

            let req_cls = AnyClass::get("NSURLRequest").expect("NSURLRequest class");
            let request: Retained<AnyObject> =
                msg_send_id![req_cls, requestWithURL: &*nsurl];

            let _: Retained<AnyObject> =
                msg_send_id![&self.webview, loadRequest: &*request];
        }
    }

    /// Go back in history.
    pub fn go_back(&self) {
        unsafe {
            let _: Retained<AnyObject> = msg_send_id![&self.webview, goBack];
        }
    }

    /// Go forward in history.
    pub fn go_forward(&self) {
        unsafe {
            let _: Retained<AnyObject> = msg_send_id![&self.webview, goForward];
        }
    }

    /// Reload the current page.
    pub fn reload(&self) {
        unsafe {
            let _: Retained<AnyObject> = msg_send_id![&self.webview, reload];
        }
    }

    /// Set the frame rect (in logical points) of the webview.
    pub fn set_frame(&self, x: f64, y: f64, w: f64, h: f64) {
        unsafe {
            let frame = NSRect::new(
                NSPoint::new(x as CGFloat, y as CGFloat),
                NSSize::new(w as CGFloat, h as CGFloat),
            );
            let _: () = msg_send![&self.webview, setFrame: frame];
        }
    }

    /// Show or hide the webview.
    pub fn set_visible(&self, visible: bool) {
        unsafe {
            let hidden = if visible { Bool::NO } else { Bool::YES };
            let _: () = msg_send![&self.webview, setHidden: hidden];
        }
    }

    /// Returns true if the webview can go back.
    pub fn can_go_back(&self) -> bool {
        unsafe {
            let val: Bool = msg_send![&self.webview, canGoBack];
            val.as_bool()
        }
    }

    /// Returns true if the webview can go forward.
    pub fn can_go_forward(&self) -> bool {
        unsafe {
            let val: Bool = msg_send![&self.webview, canGoForward];
            val.as_bool()
        }
    }

    /// Get the current URL as a string, if any.
    pub fn current_url(&self) -> Option<String> {
        unsafe {
            let url: Option<Retained<AnyObject>> = msg_send_id![&self.webview, URL];
            let url = url?;
            let abs: Option<Retained<AnyObject>> = msg_send_id![&url, absoluteString];
            let abs = abs?;
            // Convert NSString to Rust String
            let ns_str: &AnyObject = &abs;
            let utf8: *const std::ffi::c_char = msg_send![ns_str, UTF8String];
            if utf8.is_null() {
                None
            } else {
                Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
            }
        }
    }

    /// Get the current page title, if any.
    pub fn current_title(&self) -> Option<String> {
        unsafe {
            let title: Option<Retained<AnyObject>> = msg_send_id![&self.webview, title];
            let title = title?;
            let utf8: *const std::ffi::c_char = msg_send![&title, UTF8String];
            if utf8.is_null() {
                None
            } else {
                Some(std::ffi::CStr::from_ptr(utf8).to_string_lossy().into_owned())
            }
        }
    }

    /// Returns true if the webview is currently loading.
    pub fn is_loading(&self) -> bool {
        unsafe {
            let val: Bool = msg_send![&self.webview, isLoading];
            val.as_bool()
        }
    }

    /// Remove the webview from its superview.
    pub fn remove_from_parent(&self) {
        unsafe {
            let _: () = msg_send![&self.webview, removeFromSuperview];
        }
    }

    /// Make this webview the first responder of the given NSWindow,
    /// so keyboard events route to the webview.
    ///
    /// # Safety
    /// `window_ptr` must point to a valid NSWindow.
    pub unsafe fn make_first_responder(&self, window_ptr: *mut std::ffi::c_void) {
        let window: &AnyObject = &*(window_ptr as *const AnyObject);
        let _: Bool = msg_send![window, makeFirstResponder: &*self.webview];
    }

    /// Resign first responder from the webview and give it back to `view_ptr`.
    ///
    /// # Safety
    /// Both `window_ptr` and `view_ptr` must be valid pointers.
    pub unsafe fn resign_first_responder(
        &self,
        window_ptr: *mut std::ffi::c_void,
        view_ptr: *mut std::ffi::c_void,
    ) {
        let window: &AnyObject = &*(window_ptr as *const AnyObject);
        let view: &AnyObject = &*(view_ptr as *const AnyObject);
        let _: Bool = msg_send![window, makeFirstResponder: view];
    }
}
