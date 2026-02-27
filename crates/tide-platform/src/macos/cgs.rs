//! Window embedding via Accessibility API (AXUIElement).
//!
//! Positions and resizes external app windows to appear inside Tide's dock panel area.
//! Requires Accessibility permission (System Settings → Privacy → Accessibility).

use std::ffi::c_void;

use objc2_foundation::{CGFloat, CGPoint, CGSize, NSString};

// ──────────────────────────────────────────────
// Accessibility API FFI
// ──────────────────────────────────────────────

#[link(name = "ApplicationServices", kind = "framework")]
extern "C" {
    fn AXUIElementCreateApplication(pid: i32) -> *const c_void;
    fn AXUIElementCopyAttributeValue(
        element: *const c_void,
        attribute: *const c_void,
        value: *mut *const c_void,
    ) -> i32;
    fn AXUIElementSetAttributeValue(
        element: *const c_void,
        attribute: *const c_void,
        value: *const c_void,
    ) -> i32;
    fn AXUIElementPerformAction(
        element: *const c_void,
        action: *const c_void,
    ) -> i32;
    fn AXValueCreate(value_type: u32, value: *const c_void) -> *const c_void;
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFRetain(cf: *const c_void) -> *const c_void;
    fn CFArrayGetCount(array: *const c_void) -> isize;
    fn CFArrayGetValueAtIndex(array: *const c_void, index: isize) -> *const c_void;
}

// Public API from CoreGraphics (CGWindow.h) — for window discovery
#[link(name = "CoreGraphics", kind = "framework")]
extern "C" {
    fn CGWindowListCopyWindowInfo(option: u32, relative_to: u32) -> *const c_void;
}

// CGS private APIs — for window level manipulation
extern "C" {
    fn _CGSDefaultConnection() -> u32;
    fn CGSSetWindowLevel(cid: u32, wid: u32, level: i32) -> i32;
}

// Window levels (from CGWindowLevel.h)
const K_CG_FLOATING_WINDOW_LEVEL: i32 = 3; // NSFloatingWindowLevel
const K_CG_NORMAL_WINDOW_LEVEL: i32 = 0;

// CoreFoundation constants
const K_CG_WINDOW_LIST_OPTION_ALL: u32 = 0;
const K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS: u32 = 1 << 4;
const K_CG_NULL_WINDOW_ID: u32 = 0;

// AXValue types
const AX_VALUE_CG_POINT: u32 = 1;
const AX_VALUE_CG_SIZE: u32 = 2;

// ──────────────────────────────────────────────
// Accessibility permission check
// ──────────────────────────────────────────────

/// Check if this process has Accessibility permission.
pub fn is_accessibility_trusted() -> bool {
    unsafe { AXIsProcessTrusted() }
}

/// Check Accessibility permission and prompt the user if not granted.
/// Returns true if already trusted, false if not (dialog will be shown).
pub fn ensure_accessibility_trusted() -> bool {
    unsafe {
        use objc2::msg_send_id;
        use objc2::rc::Retained;
        use objc2::runtime::{AnyClass, AnyObject};

        let key = NSString::from_str("AXTrustedCheckOptionPrompt");
        let yes: Retained<AnyObject> = msg_send_id![
            AnyClass::get("NSNumber").unwrap(),
            numberWithBool: true
        ];
        let options: Retained<AnyObject> = msg_send_id![
            AnyClass::get("NSDictionary").unwrap(),
            dictionaryWithObject: &*yes,
            forKey: &*key
        ];

        #[link(name = "ApplicationServices", kind = "framework")]
        extern "C" {
            fn AXIsProcessTrustedWithOptions(options: *const c_void) -> bool;
        }

        AXIsProcessTrustedWithOptions(&*options as *const _ as *const c_void)
    }
}

// ──────────────────────────────────────────────
// Embedded window (Accessibility API)
// ──────────────────────────────────────────────

/// Handle for an embedded external window managed via the Accessibility API.
pub struct EmbeddedWindow {
    pub window_id: u32,
    pub pid: u32,
    ax_window: *const c_void, // AXUIElementRef (retained)
}

// AXUIElementRef is thread-safe (CFType)
unsafe impl Send for EmbeddedWindow {}

impl EmbeddedWindow {
    /// Create from a PID by finding the app's main AX window.
    pub fn from_pid(pid: u32, window_id: u32) -> Option<Self> {
        unsafe {
            let ax_app = AXUIElementCreateApplication(pid as i32);
            if ax_app.is_null() {
                log::warn!("AXUIElementCreateApplication({}) returned null", pid);
                return None;
            }

            // Try multiple AX attributes to find the window
            let ax_window;

            // 1. Try AXWindows (array)
            let windows_attr = NSString::from_str("AXWindows");
            let mut windows_ref: *const c_void = std::ptr::null();
            let err = AXUIElementCopyAttributeValue(
                ax_app,
                &*windows_attr as *const _ as *const c_void,
                &mut windows_ref,
            );

            if err == 0 && !windows_ref.is_null() && CFArrayGetCount(windows_ref) > 0 {
                let count = CFArrayGetCount(windows_ref);
                // CFArrayGetValueAtIndex does NOT retain, so we must CFRetain
                let w = CFArrayGetValueAtIndex(windows_ref, 0);
                CFRetain(w);
                CFRelease(windows_ref);
                ax_window = w;
                log::info!("from_pid: using AXWindows[0] (count={})", count);
            } else {
                if !windows_ref.is_null() { CFRelease(windows_ref); }
                log::info!("from_pid: AXWindows unavailable (err={}), trying fallbacks", err);

                // 2. Try AXFocusedWindow
                let focused_attr = NSString::from_str("AXFocusedWindow");
                let mut focused_ref: *const c_void = std::ptr::null();
                let err2 = AXUIElementCopyAttributeValue(
                    ax_app,
                    &*focused_attr as *const _ as *const c_void,
                    &mut focused_ref,
                );

                if err2 == 0 && !focused_ref.is_null() {
                    // CopyAttributeValue returns retained value — we own it
                    ax_window = focused_ref;
                    log::info!("from_pid: using AXFocusedWindow");
                } else {
                    // 3. Try AXMainWindow
                    let main_attr = NSString::from_str("AXMainWindow");
                    let mut main_ref: *const c_void = std::ptr::null();
                    let err3 = AXUIElementCopyAttributeValue(
                        ax_app,
                        &*main_attr as *const _ as *const c_void,
                        &mut main_ref,
                    );

                    if err3 == 0 && !main_ref.is_null() {
                        ax_window = main_ref;
                        log::info!("from_pid: using AXMainWindow");
                    } else {
                        log::warn!(
                            "from_pid({}): no AX window found (AXWindows err={}, FocusedWindow err={}, MainWindow err={})",
                            pid, err, err2, err3
                        );
                        CFRelease(ax_app);
                        return None;
                    }
                }
            }

            CFRelease(ax_app);

            log::info!(
                "EmbeddedWindow::from_pid({}): got AX window (wid={})",
                pid, window_id,
            );

            Some(Self { window_id, pid, ax_window })
        }
    }

    /// Set the window position (screen coordinates, top-left origin).
    pub fn set_position(&self, x: f64, y: f64) {
        unsafe {
            let point = CGPoint::new(x as CGFloat, y as CGFloat);
            let attr = NSString::from_str("AXPosition");
            let value = AXValueCreate(AX_VALUE_CG_POINT, &point as *const _ as *const c_void);
            if value.is_null() {
                log::warn!("AXValueCreate(CGPoint) returned null");
                return;
            }
            let err = AXUIElementSetAttributeValue(
                self.ax_window,
                &*attr as *const _ as *const c_void,
                value,
            );
            CFRelease(value);
            if err != 0 {
                log::warn!("AXSetPosition({:.0},{:.0}) failed: {}", x, y, err);
            } else {
                log::info!("AXSetPosition({:.0},{:.0}) ok", x, y);
            }
        }
    }

    /// Set the window size. Returns the actual size after clamping by the app.
    pub fn set_size(&self, w: f64, h: f64) -> (f64, f64) {
        unsafe {
            let size = CGSize::new(w as CGFloat, h as CGFloat);
            let attr = NSString::from_str("AXSize");
            let value = AXValueCreate(AX_VALUE_CG_SIZE, &size as *const _ as *const c_void);
            if value.is_null() {
                log::warn!("AXValueCreate(CGSize) returned null");
                return (w, h);
            }
            let err = AXUIElementSetAttributeValue(
                self.ax_window,
                &*attr as *const _ as *const c_void,
                value,
            );
            CFRelease(value);
            if err != 0 {
                log::warn!("AXSetSize({:.0},{:.0}) failed: {}", w, h, err);
                return (w, h);
            }

            // Read back actual size (app may clamp to minimum)
            let mut actual_ref: *const c_void = std::ptr::null();
            let err2 = AXUIElementCopyAttributeValue(
                self.ax_window,
                &*attr as *const _ as *const c_void,
                &mut actual_ref,
            );
            if err2 == 0 && !actual_ref.is_null() {
                let mut actual_size = CGSize::new(0.0, 0.0);
                // AXValueGetValue type 2 = kAXValueCGSizeType
                #[link(name = "ApplicationServices", kind = "framework")]
                extern "C" {
                    fn AXValueGetValue(value: *const c_void, value_type: u32, value_out: *mut c_void) -> bool;
                }
                let ok = AXValueGetValue(actual_ref, AX_VALUE_CG_SIZE, &mut actual_size as *mut _ as *mut c_void);
                CFRelease(actual_ref);
                if ok {
                    let aw = actual_size.width as f64;
                    let ah = actual_size.height as f64;
                    if (aw - w).abs() > 1.0 || (ah - h).abs() > 1.0 {
                        log::info!("AXSetSize requested ({:.0},{:.0}), actual ({:.0},{:.0})", w, h, aw, ah);
                    }
                    return (aw, ah);
                }
            }
            (w, h)
        }
    }

    /// Raise the window to front (brings above other windows).
    pub fn raise(&self) {
        unsafe {
            let action = NSString::from_str("AXRaise");
            let err = AXUIElementPerformAction(
                self.ax_window,
                &*action as *const _ as *const c_void,
            );
            if err != 0 {
                log::warn!("AXRaise failed: {}", err);
            }
        }
    }

    /// Hide the window by moving it off-screen and resetting window level.
    pub fn order_out(&self) {
        unsafe {
            let cid = _CGSDefaultConnection();
            CGSSetWindowLevel(cid, self.window_id, K_CG_NORMAL_WINDOW_LEVEL);
        }
        self.set_position(-30000.0, -30000.0);
    }

    /// Bring the embedded window above Tide's window.
    /// Lowers Tide's window level via CGS (thread-safe) so the embedded window
    /// at normal level (0) appears above it.
    pub fn order_above(&self, tide_window_num: u32) {
        unsafe {
            let cid = _CGSDefaultConnection();
            // Lower Tide's window level below normal so the embedded window stays on top
            let r1 = CGSSetWindowLevel(cid, tide_window_num, -1);
            if r1 != 0 {
                log::warn!("CGSSetWindowLevel(tide wid={}, -1) err={}", tide_window_num, r1);
            }
        }
        self.raise();
    }

    /// Restore Tide's window level to normal (thread-safe via CGS).
    pub fn restore_tide_level(tide_window_num: u32) {
        unsafe {
            let cid = _CGSDefaultConnection();
            let r = CGSSetWindowLevel(cid, tide_window_num, K_CG_NORMAL_WINDOW_LEVEL);
            if r != 0 {
                log::warn!("CGSSetWindowLevel(restore tide wid={}, 0) err={}", tide_window_num, r);
            }
        }
    }

    /// Activate the target process to bring its windows above all other apps.
    /// Call this once when first embedding, not every frame.
    pub fn activate(&self) {
        unsafe {
            use objc2::msg_send;
            use objc2::runtime::{AnyClass, AnyObject};

            let app: *const AnyObject = msg_send![
                AnyClass::get("NSRunningApplication").unwrap(),
                runningApplicationWithProcessIdentifier: self.pid as i32
            ];
            if !app.is_null() {
                let ok: bool = msg_send![app, activateWithOptions: 3u64];
                if !ok {
                    log::warn!("activateWithOptions failed for pid={}", self.pid);
                } else {
                    log::info!("Activated app pid={}", self.pid);
                }
            }
        }
    }
}

impl Drop for EmbeddedWindow {
    fn drop(&mut self) {
        if !self.ax_window.is_null() {
            unsafe { CFRelease(self.ax_window); }
        }
    }
}

// ──────────────────────────────────────────────
// Window discovery (public CoreGraphics API)
// ──────────────────────────────────────────────

/// Find the main window of a process by PID.
/// Returns (CGWindowID, window_name) if found.
pub fn find_window_by_pid(pid: u32) -> Option<(u32, String)> {
    unsafe {
        use objc2::runtime::AnyObject;
        use objc2::{msg_send, msg_send_id};
        use objc2::rc::Retained;

        let options = K_CG_WINDOW_LIST_OPTION_ALL | K_CG_WINDOW_LIST_EXCLUDE_DESKTOP_ELEMENTS;
        let info_list = CGWindowListCopyWindowInfo(options, K_CG_NULL_WINDOW_ID);
        if info_list.is_null() {
            log::warn!("find_window_by_pid({}): CGWindowListCopyWindowInfo returned null", pid);
            return None;
        }

        let array: &AnyObject = &*(info_list as *const AnyObject);
        let count: usize = msg_send![array, count];

        let pid_key = NSString::from_str("kCGWindowOwnerPID");
        let name_key = NSString::from_str("kCGWindowName");
        let number_key = NSString::from_str("kCGWindowNumber");
        let layer_key = NSString::from_str("kCGWindowLayer");

        let mut result = None;

        for i in 0..count {
            let dict: Retained<AnyObject> = msg_send_id![array, objectAtIndex: i];

            let pid_obj: *const AnyObject = msg_send![&*dict, objectForKey: &*pid_key];
            if pid_obj.is_null() { continue; }
            let window_pid: i32 = msg_send![pid_obj, intValue];
            if window_pid as u32 != pid { continue; }

            let layer_obj: *const AnyObject = msg_send![&*dict, objectForKey: &*layer_key];
            if !layer_obj.is_null() {
                let layer: i32 = msg_send![layer_obj, intValue];
                if layer != 0 { continue; }
            }

            let num_obj: *const AnyObject = msg_send![&*dict, objectForKey: &*number_key];
            if num_obj.is_null() { continue; }
            let window_id: u32 = msg_send![num_obj, unsignedIntValue];

            let name_obj: *const AnyObject = msg_send![&*dict, objectForKey: &*name_key];
            let name = if !name_obj.is_null() {
                let ns_name: &NSString = &*(name_obj as *const NSString);
                ns_name.to_string()
            } else {
                String::new()
            };

            log::info!("find_window_by_pid({}): found wid={} name={:?}", pid, window_id, name);
            result = Some((window_id, name));
            break;
        }

        let _: () = msg_send![array, release];
        result
    }
}

// ──────────────────────────────────────────────
// App launch helpers
// ──────────────────────────────────────────────

/// Launch an app by bundle ID or return its PID if already running.
pub fn launch_or_find_app(bundle_id: &str) -> Option<u32> {
    unsafe {
        use objc2::msg_send;
        use objc2::msg_send_id;
        use objc2::rc::Retained;
        use objc2::runtime::AnyObject;

        log::info!("launch_or_find_app: checking for {}", bundle_id);

        let ns_bundle_id = NSString::from_str(bundle_id);
        let running_apps: Retained<AnyObject> = msg_send_id![
            objc2::runtime::AnyClass::get("NSRunningApplication").unwrap(),
            runningApplicationsWithBundleIdentifier: &*ns_bundle_id
        ];
        let count: usize = msg_send![&*running_apps, count];
        if count > 0 {
            let app: Retained<AnyObject> = msg_send_id![&*running_apps, objectAtIndex: 0usize];
            let pid: i32 = msg_send![&*app, processIdentifier];
            log::info!("launch_or_find_app: already running, pid={}", pid);
            if pid > 0 {
                // Activate the app to ensure it creates a window if needed
                let ok: bool = msg_send![&*app, activateWithOptions: 3u64];
                log::info!("launch_or_find_app: activated existing app (ok={})", ok);
                return Some(pid as u32);
            }
        }

        // Not running — launch via NSWorkspace
        log::info!("launch_or_find_app: launching {}", bundle_id);
        let workspace: Retained<AnyObject> = msg_send_id![
            objc2::runtime::AnyClass::get("NSWorkspace").unwrap(),
            sharedWorkspace
        ];

        let url: *const AnyObject = msg_send![&*workspace, URLForApplicationWithBundleIdentifier: &*ns_bundle_id];
        if url.is_null() {
            log::warn!("launch_or_find_app: no app URL for {}", bundle_id);
            return None;
        }

        let config: Retained<AnyObject> = msg_send_id![
            objc2::runtime::AnyClass::get("NSWorkspaceOpenConfiguration").unwrap(),
            configuration
        ];

        let _: () = msg_send![
            &*workspace,
            openApplicationAtURL: url,
            configuration: &*config,
            completionHandler: std::ptr::null::<c_void>()
        ];

        // Poll for app to start (up to 2 seconds)
        for attempt in 0..20 {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let running_apps2: Retained<AnyObject> = msg_send_id![
                objc2::runtime::AnyClass::get("NSRunningApplication").unwrap(),
                runningApplicationsWithBundleIdentifier: &*ns_bundle_id
            ];
            let count2: usize = msg_send![&*running_apps2, count];
            if count2 > 0 {
                let app: Retained<AnyObject> = msg_send_id![&*running_apps2, objectAtIndex: 0usize];
                let pid: i32 = msg_send![&*app, processIdentifier];
                if pid > 0 {
                    log::info!("launch_or_find_app: started after {}ms, pid={}", (attempt + 1) * 100, pid);
                    return Some(pid as u32);
                }
            }
        }

        log::warn!("launch_or_find_app: timed out for {}", bundle_id);
        None
    }
}

/// Check if a process is still alive.
pub fn is_pid_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

// ──────────────────────────────────────────────
// Coordinate conversion
// ──────────────────────────────────────────────

/// Convert a Tide pixel rect to screen coordinates for the Accessibility API.
///
/// The Accessibility API uses screen coordinates with top-left origin
/// (same as Quartz display coordinates).
///
/// Parameters:
/// - `window_ptr`: raw pointer to the NSWindow
/// - `pixel_rect`: (x, y, w, h) in Tide's coordinate space (pixels, top-left origin)
/// - `scale_factor`: backing scale factor (2.0 on Retina)
///
/// Returns: (x, y, w, h) in screen coordinates (points, top-left origin).
pub fn tide_rect_to_screen(
    window_ptr: *mut c_void,
    pixel_rect: (f64, f64, f64, f64),
    scale_factor: f64,
) -> (f64, f64, f64, f64) {
    unsafe {
        use objc2::msg_send;
        use objc2_foundation::NSRect;

        let window = window_ptr as *const objc2::runtime::AnyObject;

        let window_frame: NSRect = msg_send![window, frame];
        let content_view: *const objc2::runtime::AnyObject = msg_send![window, contentView];
        let content_frame: NSRect = msg_send![content_view, frame];
        let screen: *const objc2::runtime::AnyObject = msg_send![window, screen];
        let screen_frame: NSRect = msg_send![screen, frame];
        let screen_height = screen_frame.size.height;

        let (px, py, pw, ph) = pixel_rect;

        // Convert pixels to points
        let lx = px / scale_factor;
        let ly = py / scale_factor;
        let lw = pw / scale_factor;
        let lh = ph / scale_factor;

        // Content view origin in screen coords (bottom-left origin)
        let content_origin_x = window_frame.origin.x + content_frame.origin.x;
        let content_origin_y = window_frame.origin.y + content_frame.origin.y;
        let content_height = content_frame.size.height;

        // Screen x: content left + offset from content left
        let screen_x = content_origin_x + lx;

        // Screen y: convert from top-left within content to screen top-left
        // bottom_left_y = bottom of content + content_height - offset_from_top - height
        let bottom_left_y = content_origin_y + content_height - ly - lh;
        let screen_y = screen_height - bottom_left_y - lh;

        log::info!(
            "tide_rect_to_screen: win_frame=({:.1},{:.1},{:.1},{:.1}) content_frame=({:.1},{:.1},{:.1},{:.1}) screen_h={:.1} → ({:.1},{:.1},{:.1},{:.1})",
            window_frame.origin.x, window_frame.origin.y, window_frame.size.width, window_frame.size.height,
            content_frame.origin.x, content_frame.origin.y, content_frame.size.width, content_frame.size.height,
            screen_height, screen_x, screen_y, lw, lh,
        );

        (screen_x, screen_y, lw, lh)
    }
}

/// Get the CGWindowNumber of Tide's NSWindow.
pub fn window_number(window_ptr: *mut c_void) -> u32 {
    unsafe {
        use objc2::msg_send;
        let window = window_ptr as *const objc2::runtime::AnyObject;
        let num: i64 = msg_send![window, windowNumber];
        num as u32
    }
}
