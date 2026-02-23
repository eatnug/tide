use std::time::Instant;

use tide_core::PaneId;
use tide_platform::macos::webview::WebViewHandle;

/// A browser pane backed by a native WKWebView.
pub struct BrowserPane {
    pub id: PaneId,
    /// Current URL displayed by the webview.
    pub url: String,
    /// Editable URL bar text.
    pub url_input: String,
    /// Cursor position within url_input (char index, not byte offset).
    pub url_input_cursor: usize,
    /// Whether the URL bar has keyboard focus.
    pub url_input_focused: bool,
    /// Whether the webview is currently loading.
    pub loading: bool,
    /// Whether back navigation is available.
    pub can_go_back: bool,
    /// Whether forward navigation is available.
    pub can_go_forward: bool,
    /// The native WKWebView handle (created lazily when content_view_ptr is available).
    pub webview: Option<WebViewHandle>,
    /// Generation counter for dirty tracking.
    pub generation: u64,
    /// Last time sync_from_webview() actually ran (for throttling).
    pub last_sync: Instant,
    /// Whether this browser pane currently holds first responder status.
    pub is_first_responder: bool,
    /// Whether the webview needs to navigate to `url` once visible with a proper frame.
    pub needs_initial_navigate: bool,
}

impl BrowserPane {
    pub fn new(id: PaneId) -> Self {
        Self {
            id,
            url: String::new(),
            url_input: String::new(),
            url_input_cursor: 0,
            url_input_focused: true,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            webview: None,
            generation: 0,
            last_sync: Instant::now(),
            is_first_responder: false,
            needs_initial_navigate: false,
        }
    }

    pub fn with_url(id: PaneId, url: String) -> Self {
        let url_input = url.clone();
        let cursor = url_input.chars().count();
        Self {
            id,
            url: url.clone(),
            url_input,
            url_input_cursor: cursor,
            url_input_focused: false,
            loading: false,
            can_go_back: false,
            can_go_forward: false,
            webview: None,
            generation: 0,
            last_sync: Instant::now(),
            is_first_responder: false,
            needs_initial_navigate: true,
        }
    }

    /// Display title for the tab.
    pub fn title(&self) -> String {
        if let Some(ref wv) = self.webview {
            if let Some(t) = wv.current_title() {
                if !t.is_empty() {
                    return t;
                }
            }
        }
        if self.url.is_empty() {
            "New Tab".to_string()
        } else {
            self.url.clone()
        }
    }

    /// Navigate to a URL. Normalizes bare domains to https://.
    /// Localhost and 127.0.0.1 URLs default to http:// instead.
    pub fn navigate(&mut self, url: &str) {
        let normalized = if url.contains("://") {
            url.to_string()
        } else if url.starts_with("localhost") || url.starts_with("127.0.0.1") || url.starts_with("[::1]") {
            format!("http://{}", url)
        } else {
            format!("https://{}", url)
        };
        self.url = normalized.clone();
        self.url_input = normalized.clone();
        self.url_input_cursor = normalized.chars().count();
        if let Some(ref wv) = self.webview {
            wv.navigate(&normalized);
        }
        self.generation = self.generation.wrapping_add(1);
    }

    pub fn go_back(&mut self) {
        if let Some(ref wv) = self.webview {
            wv.go_back();
        }
    }

    pub fn go_forward(&mut self) {
        if let Some(ref wv) = self.webview {
            wv.go_forward();
        }
    }

    pub fn reload(&mut self) {
        if let Some(ref wv) = self.webview {
            wv.reload();
        }
    }

    /// Sync state from the native WKWebView (URL, title, loading, navigation state).
    /// Throttled to at most once per 500ms to reduce ObjC IPC overhead.
    pub fn sync_from_webview(&mut self) {
        const SYNC_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);
        let now = Instant::now();
        if now.duration_since(self.last_sync) < SYNC_INTERVAL {
            return;
        }
        self.last_sync = now;

        let Some(ref wv) = self.webview else { return };

        let new_url = wv.current_url().unwrap_or_default();
        let new_loading = wv.is_loading();
        let new_back = wv.can_go_back();
        let new_forward = wv.can_go_forward();

        if new_url != self.url || new_loading != self.loading
            || new_back != self.can_go_back || new_forward != self.can_go_forward
        {
            self.url = new_url.clone();
            if !self.url_input_focused {
                self.url_input = new_url;
                self.url_input_cursor = self.url_input.chars().count();
            }
            self.loading = new_loading;
            self.can_go_back = new_back;
            self.can_go_forward = new_forward;
            self.generation = self.generation.wrapping_add(1);
        }
    }

    /// Set the webview frame rect (logical points).
    pub fn set_frame(&self, x: f64, y: f64, w: f64, h: f64) {
        if let Some(ref wv) = self.webview {
            wv.set_frame(x, y, w, h);
        }
    }

    /// Show or hide the webview.
    pub fn set_visible(&self, visible: bool) {
        if let Some(ref wv) = self.webview {
            wv.set_visible(visible);
        }
    }

    /// Convert char-based cursor position to byte offset for String operations.
    pub fn cursor_byte_offset(&self) -> usize {
        self.url_input
            .char_indices()
            .nth(self.url_input_cursor)
            .map(|(i, _)| i)
            .unwrap_or(self.url_input.len())
    }

    /// Number of characters in the URL input.
    pub fn url_input_char_len(&self) -> usize {
        self.url_input.chars().count()
    }

    /// Remove the webview from the view hierarchy and drop the handle.
    pub fn destroy(&mut self) {
        if let Some(wv) = self.webview.take() {
            wv.remove_from_parent();
        }
    }
}
