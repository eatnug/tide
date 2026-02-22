//! WKWebView wrapper for embedded browser functionality.
//!
//! Uses raw `objc2` message sends to interact with WebKit classes,
//! avoiding a direct WebKit crate dependency.

use objc2::rc::Retained;
use objc2::runtime::{AnyClass, AnyObject, Bool};
use objc2::{declare_class, msg_send, msg_send_id, mutability, ClassType, DeclaredClass};
use objc2_foundation::{CGFloat, MainThreadMarker, NSObject, NSRect, NSPoint, NSSize, NSString};

// ---------------------------------------------------------------------------
// WKUIDelegate — handles popups, JavaScript dialogs, etc.
// ---------------------------------------------------------------------------
declare_class!(
    struct TideUIDelegate;

    unsafe impl ClassType for TideUIDelegate {
        type Super = NSObject;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = "TideUIDelegate";
    }

    impl DeclaredClass for TideUIDelegate {
        type Ivars = ();
    }

    unsafe impl TideUIDelegate {
        #[method_id(init)]
        fn init(this: objc2::rc::Allocated<Self>) -> Option<Retained<Self>> {
            let this = this.set_ivars(());
            unsafe { msg_send_id![super(this), init] }
        }

        /// Handle window.open() by loading in the same webview (no popup windows).
        #[method_id(webView:createWebViewWithConfiguration:forNavigationAction:windowFeatures:)]
        fn create_webview(
            &self,
            webview: &AnyObject,
            _config: &AnyObject,
            navigation_action: &AnyObject,
            _window_features: &AnyObject,
        ) -> Option<Retained<AnyObject>> {
            unsafe {
                let request: Retained<AnyObject> = msg_send_id![navigation_action, request];
                let _: Option<Retained<AnyObject>> =
                    msg_send_id![webview, loadRequest: &*request];
            }
            None
        }

        /// Handle JavaScript alert() — show native NSAlert.
        #[method(webView:runJavaScriptAlertPanelWithMessage:initiatedByFrame:completionHandler:)]
        fn run_alert(
            &self,
            _webview: &AnyObject,
            message: &NSString,
            _frame: &AnyObject,
            completion: &block2::Block<dyn Fn()>,
        ) {
            unsafe {
                // Show a native NSAlert
                let alert_cls = AnyClass::get("NSAlert").expect("NSAlert class must exist");
                let alert: Retained<AnyObject> = msg_send_id![alert_cls, new];
                let _: () = msg_send![&alert, setMessageText: message];
                let _: () = msg_send![&alert, addButtonWithTitle: &*NSString::from_str("OK")];
                let _: isize = msg_send![&alert, runModal];
            }
            completion.call(());
        }

        /// Handle JavaScript confirm() — show native NSAlert with OK/Cancel.
        #[method(webView:runJavaScriptConfirmPanelWithMessage:initiatedByFrame:completionHandler:)]
        fn run_confirm(
            &self,
            _webview: &AnyObject,
            message: &NSString,
            _frame: &AnyObject,
            completion: &block2::Block<dyn Fn(Bool)>,
        ) {
            let result = unsafe {
                let alert_cls = AnyClass::get("NSAlert").expect("NSAlert class must exist");
                let alert: Retained<AnyObject> = msg_send_id![alert_cls, new];
                let _: () = msg_send![&alert, setMessageText: message];
                let _: () = msg_send![&alert, addButtonWithTitle: &*NSString::from_str("OK")];
                let _: () = msg_send![&alert, addButtonWithTitle: &*NSString::from_str("Cancel")];
                let response: isize = msg_send![&alert, runModal];
                // NSAlertFirstButtonReturn = 1000
                response == 1000
            };
            completion.call((Bool::new(result),));
        }

        /// Handle JavaScript prompt() — show native NSAlert with text field.
        #[method(webView:runJavaScriptTextInputPanelWithPrompt:defaultText:initiatedByFrame:completionHandler:)]
        fn run_prompt(
            &self,
            _webview: &AnyObject,
            prompt: &NSString,
            default_text: Option<&NSString>,
            _frame: &AnyObject,
            completion: &block2::Block<dyn Fn(*mut NSString)>,
        ) {
            unsafe {
                let alert_cls = AnyClass::get("NSAlert").expect("NSAlert class must exist");
                let alert: Retained<AnyObject> = msg_send_id![alert_cls, new];
                let _: () = msg_send![&alert, setMessageText: prompt];
                let _: () = msg_send![&alert, addButtonWithTitle: &*NSString::from_str("OK")];
                let _: () = msg_send![&alert, addButtonWithTitle: &*NSString::from_str("Cancel")];

                // Add a text field to the alert
                let text_field_cls = AnyClass::get("NSTextField").expect("NSTextField class must exist");
                let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(300.0, 24.0));
                let field: Retained<AnyObject> = msg_send_id![
                    msg_send_id![text_field_cls, alloc],
                    initWithFrame: frame
                ];
                if let Some(dt) = default_text {
                    let _: () = msg_send![&field, setStringValue: dt];
                }
                let _: () = msg_send![&alert, setAccessoryView: &*field];

                let response: isize = msg_send![&alert, runModal];
                if response == 1000 {
                    // NSAlertFirstButtonReturn — user clicked OK
                    let value: Retained<NSString> = msg_send_id![&field, stringValue];
                    completion.call((&*value as *const NSString as *mut NSString,));
                } else {
                    completion.call((std::ptr::null_mut(),));
                }
            }
        }
    }
);

/// Handle to a WKWebView instance, added as a subview of the parent NSView.
pub struct WebViewHandle {
    webview: Retained<AnyObject>,
    /// Retained so the weak UIDelegate reference stays valid.
    _ui_delegate: Retained<TideUIDelegate>,
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

        // Enable JavaScript popup windows
        let prefs: Retained<AnyObject> = msg_send_id![&config, preferences];
        let _: () = msg_send![&prefs, setJavaScriptCanOpenWindowsAutomatically: Bool::YES];

        // Request desktop content mode (WKWebpagePreferences)
        let page_prefs_cls = AnyClass::get("WKWebpagePreferences");
        if let Some(cls) = page_prefs_cls {
            let page_prefs: Retained<AnyObject> = msg_send_id![cls, new];
            // WKContentMode.desktop = 1
            let _: () = msg_send![&page_prefs, setPreferredContentMode: 1_isize];
            let _: () = msg_send![&config, setDefaultWebpagePreferences: &*page_prefs];
        }

        // WKWebView initWithFrame:configuration:
        let wk_cls = AnyClass::get("WKWebView")?;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(100.0, 100.0));
        let webview: Retained<AnyObject> = msg_send_id![
            msg_send_id![wk_cls, alloc],
            initWithFrame: frame,
            configuration: &*config
        ];

        // Set a complete Safari user agent so sites like Google serve full
        // CSS/JS instead of degraded experiences for unknown browsers.
        let ua = NSString::from_str(
            "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
             AppleWebKit/605.1.15 (KHTML, like Gecko) \
             Version/18.3 Safari/605.1.15",
        );
        let _: () = msg_send![&webview, setCustomUserAgent: &*ua];

        // Enable trackpad swipe gestures for back/forward navigation
        let _: () = msg_send![&webview, setAllowsBackForwardNavigationGestures: Bool::YES];

        // Disable opaque background so rounded corners etc. work
        let _: () = msg_send![&webview, setOpaque: Bool::NO];

        // Hide initially until frame is set
        let _: () = msg_send![&webview, setHidden: Bool::YES];

        // Set up UI delegate for popup handling and JavaScript dialogs
        let mtm = MainThreadMarker::new().expect("must be on main thread");
        let delegate: Retained<TideUIDelegate> = unsafe {
            msg_send_id![mtm.alloc::<TideUIDelegate>(), init]
        };
        let _: () = msg_send![&webview, setUIDelegate: &*delegate];

        // Add as subview
        let _: () = msg_send![parent, addSubview: &*webview];

        Some(Self { webview, _ui_delegate: delegate })
    }

    /// Navigate to a URL string.
    pub fn navigate(&self, url: &str) {
        unsafe {
            let url_cls = AnyClass::get("NSURL").expect("NSURL class");
            let ns_url_str = NSString::from_str(url);
            let nsurl: Option<Retained<AnyObject>> =
                msg_send_id![url_cls, URLWithString: &*ns_url_str];
            let Some(nsurl) = nsurl else { return };

            let req_cls = AnyClass::get("NSURLRequest").expect("NSURLRequest class");
            let request: Retained<AnyObject> =
                msg_send_id![req_cls, requestWithURL: &*nsurl];

            let _: Option<Retained<AnyObject>> =
                msg_send_id![&self.webview, loadRequest: &*request];
        }
    }

    /// Go back in history.
    pub fn go_back(&self) {
        unsafe {
            let _: Option<Retained<AnyObject>> = msg_send_id![&self.webview, goBack];
        }
    }

    /// Go forward in history.
    pub fn go_forward(&self) {
        unsafe {
            let _: Option<Retained<AnyObject>> = msg_send_id![&self.webview, goForward];
        }
    }

    /// Reload the current page.
    pub fn reload(&self) {
        unsafe {
            let _: Option<Retained<AnyObject>> = msg_send_id![&self.webview, reload];
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
