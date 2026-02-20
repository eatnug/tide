use tide_core::PaneId;
use tide_platform::macos::webview::WebViewHandle;

/// A browser pane backed by a native WKWebView.
pub struct BrowserPane {
    pub id: PaneId,
    /// Current URL displayed by the webview.
    pub url: String,
    /// Editable URL bar text.
    pub url_input: String,
    /// Cursor position within url_input.
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
        }
    }

    pub fn with_url(id: PaneId, url: String) -> Self {
        let url_input = url.clone();
        let cursor = url_input.len();
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
    pub fn navigate(&mut self, url: &str) {
        let normalized = if !url.contains("://") {
            format!("https://{}", url)
        } else {
            url.to_string()
        };
        self.url = normalized.clone();
        self.url_input = normalized.clone();
        self.url_input_cursor = normalized.len();
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
    pub fn sync_from_webview(&mut self) {
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
                self.url_input_cursor = self.url_input.len();
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

    /// Remove the webview from the view hierarchy and drop the handle.
    pub fn destroy(&mut self) {
        if let Some(wv) = self.webview.take() {
            wv.remove_from_parent();
        }
    }
}
