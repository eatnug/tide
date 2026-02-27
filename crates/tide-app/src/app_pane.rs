//! AppPane: embeds an external application window inside a Tide dock panel tab.
//!
//! Uses CGS (Core Graphics Services) private APIs to reposition the external
//! app's window so it appears inside the Tide pane area.

use std::time::Instant;

use tide_core::PaneId;

/// State machine for an embedded app pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AppPaneState {
    /// App is being launched.
    Launching,
    /// App running, searching for its window.
    WaitingForWindow,
    /// Window captured and positioned inside Tide.
    Embedded,
    /// App exited unexpectedly.
    AppQuit,
}

/// An external application embedded in a Tide dock panel tab.
pub struct AppPane {
    pub id: PaneId,
    pub bundle_id: String,
    pub app_name: String,
    pub pid: Option<u32>,
    pub window_id: Option<u32>,
    pub state: AppPaneState,
    pub generation: u64,
    pub last_sync: Instant,
    /// Embedded window handle for CGS operations.
    pub embedded: Option<tide_platform::macos::cgs::EmbeddedWindow>,
}

impl AppPane {
    pub fn new(id: PaneId, bundle_id: String, app_name: String) -> Self {
        Self {
            id,
            bundle_id,
            app_name,
            pid: None,
            window_id: None,
            state: AppPaneState::Launching,
            generation: 0,
            last_sync: Instant::now(),
            embedded: None,
        }
    }

    /// Display title for the tab.
    pub fn title(&self) -> String {
        match self.state {
            AppPaneState::Launching => format!("{} (launching…)", self.app_name),
            AppPaneState::WaitingForWindow => format!("{} (waiting…)", self.app_name),
            AppPaneState::Embedded => self.app_name.clone(),
            AppPaneState::AppQuit => format!("{} (quit)", self.app_name),
        }
    }

    /// Release CGS ordering constraints and return the window to normal.
    pub fn destroy(&mut self) {
        if let Some(ref embedded) = self.embedded {
            embedded.order_out();
        }
        self.embedded = None;
        self.state = AppPaneState::AppQuit;
    }
}
