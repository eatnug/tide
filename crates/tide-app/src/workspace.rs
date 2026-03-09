// Workspace management: save, load, switch, create, close workspaces.

use std::collections::HashMap;

use tide_core::PaneId;
use tide_layout::SplitLayout;

use crate::pane::PaneKind;
use crate::ui_state::FocusArea;
use crate::App;

/// A workspace groups its own layout, panes, and focus state.
/// The active workspace's data is swapped into/from App fields.
pub(crate) struct Workspace {
    pub name: String,
    pub layout: SplitLayout,
    pub focused: Option<PaneId>,
    pub panes: HashMap<PaneId, PaneKind>,
}

impl App {
    /// Save the active workspace's state back into the workspaces vec.
    pub(crate) fn save_active_workspace(&mut self) {
        if self.workspaces.is_empty() { return; }
        let ws = &mut self.workspaces[self.active_workspace];
        std::mem::swap(&mut self.layout, &mut ws.layout);
        std::mem::swap(&mut self.focused, &mut ws.focused);
        std::mem::swap(&mut self.panes, &mut ws.panes);
    }

    /// Load the active workspace's state from the workspaces vec into App fields.
    pub(crate) fn load_active_workspace(&mut self) {
        if self.workspaces.is_empty() { return; }
        let ws = &mut self.workspaces[self.active_workspace];
        std::mem::swap(&mut self.layout, &mut ws.layout);
        std::mem::swap(&mut self.focused, &mut ws.focused);
        std::mem::swap(&mut self.panes, &mut ws.panes);
    }

    /// Switch to workspace at the given 0-based index.
    pub(crate) fn switch_workspace(&mut self, idx: usize) {
        if idx == self.active_workspace || idx >= self.workspaces.len() { return; }
        // Hide all browser WebViews in the current workspace before saving,
        // since native NSViews persist across workspace swaps.
        for pane in self.panes.values_mut() {
            if let PaneKind::Browser(bp) = pane {
                bp.set_visible(false);
                bp.is_first_responder = false;
            }
        }
        self.save_active_workspace();
        self.active_workspace = idx;
        self.load_active_workspace();

        if let Some(id) = self.focused {
            self.router.set_focused(id);
        }
        self.pane_rects.clear();
        self.visual_pane_rects.clear();
        self.pane_generations.clear();
        self.chrome_generation += 1;
        self.compute_layout();
        self.update_file_tree_cwd();
        self.sync_browser_webview_frames();
    }

    /// Create a new workspace with a single terminal pane and switch to it.
    pub(crate) fn new_workspace(&mut self) {
        // Hide browser WebViews from current workspace
        for pane in self.panes.values_mut() {
            if let PaneKind::Browser(bp) = pane {
                bp.set_visible(false);
                bp.is_first_responder = false;
            }
        }
        self.save_active_workspace();

        let (layout, pane_id) = SplitLayout::with_initial_pane();
        self.layout = layout;
        self.focused = Some(pane_id);
        self.panes = HashMap::new();

        let ws_name = format!("Workspace {}", self.workspaces.len() + 1);
        self.workspaces.push(Workspace {
            name: ws_name,
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        self.active_workspace = self.workspaces.len() - 1;

        self.create_terminal_pane(pane_id, None);
        self.router.set_focused(pane_id);
        self.focus_area = FocusArea::PaneArea;
        self.pane_rects.clear();
        self.visual_pane_rects.clear();
        self.pane_generations.clear();
        self.chrome_generation += 1;
        self.compute_layout();
        self.update_file_tree_cwd();
    }

    /// Close the current workspace (only if more than one exists).
    pub(crate) fn close_workspace(&mut self) {
        if self.workspaces.len() <= 1 { return; }

        // Destroy all panes in the current workspace
        let pane_ids: Vec<PaneId> = self.panes.keys().copied().collect();
        for id in pane_ids {
            if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&id) {
                bp.destroy();
            }
            self.panes.remove(&id);
            self.pending_ime_proxy_removes.push(id);
            self.pane_generations.remove(&id);
            self.scroll_accumulator.remove(&id);
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.remove_pane_cache(id);
            }
        }

        // Remove workspace from vec
        self.workspaces.remove(self.active_workspace);
        if self.active_workspace >= self.workspaces.len() {
            self.active_workspace = self.workspaces.len() - 1;
        }

        // Load the new active workspace
        self.load_active_workspace();
        if let Some(id) = self.focused {
            self.router.set_focused(id);
        }
        self.focus_area = FocusArea::PaneArea;
        self.pane_rects.clear();
        self.visual_pane_rects.clear();
        self.pane_generations.clear();
        self.chrome_generation += 1;
        self.compute_layout();
        self.update_file_tree_cwd();
        self.sync_browser_webview_frames();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app() -> App {
        let mut app = App::new();
        // Set a non-zero cell size so compute_layout doesn't degenerate
        app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
        app.window_size = (960, 640);
        app
    }

    #[test]
    fn save_load_roundtrip() {
        let mut app = test_app();
        app.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.active_workspace = 0;
        app.focused = Some(42);

        // Save: swaps app.focused ↔ ws[0].focused
        app.save_active_workspace();
        assert_eq!(app.workspaces[0].focused, Some(42));
        assert_eq!(app.focused, None); // swapped out

        // Load: swaps back
        app.load_active_workspace();
        assert_eq!(app.focused, Some(42));
        assert_eq!(app.workspaces[0].focused, None);
    }

    #[test]
    fn save_load_empty_workspaces_is_noop() {
        let mut app = test_app();
        app.focused = Some(10);

        // No workspaces — should not panic
        app.save_active_workspace();
        assert_eq!(app.focused, Some(10)); // unchanged
        app.load_active_workspace();
        assert_eq!(app.focused, Some(10)); // unchanged
    }

    #[test]
    fn switch_workspace_swaps_state() {
        let mut app = test_app();

        // Create two workspaces with different focused pane IDs
        app.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });

        // Set up WS1 as active with focused pane 100
        app.active_workspace = 0;
        app.focused = Some(100);

        // Save WS1's state, switch to WS2
        // First save WS2 state manually so there's something to load
        app.save_active_workspace();
        app.active_workspace = 1;
        app.focused = Some(200);
        app.save_active_workspace();

        // Now load WS1 back
        app.active_workspace = 0;
        app.load_active_workspace();
        assert_eq!(app.focused, Some(100));

        // Switch to WS2 via the method
        app.switch_workspace(1);
        assert_eq!(app.active_workspace, 1);
        assert_eq!(app.focused, Some(200));

        // Switch back to WS1
        app.switch_workspace(0);
        assert_eq!(app.active_workspace, 0);
        assert_eq!(app.focused, Some(100));
    }

    #[test]
    fn switch_workspace_same_index_is_noop() {
        let mut app = test_app();
        app.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.active_workspace = 0;
        app.focused = Some(42);

        let gen_before = app.chrome_generation;
        app.switch_workspace(0); // same index
        // Should not have changed anything
        assert_eq!(app.focused, Some(42));
        assert_eq!(app.chrome_generation, gen_before);
    }

    #[test]
    fn switch_workspace_out_of_bounds_is_noop() {
        let mut app = test_app();
        app.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.active_workspace = 0;
        app.focused = Some(42);

        app.switch_workspace(99); // out of bounds
        assert_eq!(app.focused, Some(42));
        assert_eq!(app.active_workspace, 0);
    }

    #[test]
    fn close_workspace_with_single_workspace_is_noop() {
        let mut app = test_app();
        app.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });

        app.close_workspace();
        assert_eq!(app.workspaces.len(), 1); // still 1
    }

    #[test]
    fn close_workspace_removes_and_switches() {
        let mut app = test_app();

        // Set up two workspaces
        app.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: Some(200),
            panes: HashMap::new(),
        });
        app.active_workspace = 0;
        app.focused = Some(100);

        // Close the first workspace
        app.close_workspace();

        assert_eq!(app.workspaces.len(), 1);
        assert_eq!(app.active_workspace, 0);
        assert_eq!(app.workspaces[0].name, "WS2");
        // After close, the remaining workspace's state is loaded
        assert_eq!(app.focused, Some(200));
    }
}
