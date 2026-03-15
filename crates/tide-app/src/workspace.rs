// Workspace management: save, load, switch, create, close workspaces.

use std::collections::HashMap;

use tide_core::{DropZone, LayoutEngine, PaneId};
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
        if self.ws.workspaces.is_empty() { return; }
        let ws = &mut self.ws.workspaces[self.ws.active];
        std::mem::swap(&mut self.layout, &mut ws.layout);
        std::mem::swap(&mut self.focused, &mut ws.focused);
        std::mem::swap(&mut self.panes, &mut ws.panes);
    }

    /// Load the active workspace's state from the workspaces vec into App fields.
    pub(crate) fn load_active_workspace(&mut self) {
        if self.ws.workspaces.is_empty() { return; }
        let ws = &mut self.ws.workspaces[self.ws.active];
        std::mem::swap(&mut self.layout, &mut ws.layout);
        std::mem::swap(&mut self.focused, &mut ws.focused);
        std::mem::swap(&mut self.panes, &mut ws.panes);
    }

    /// Switch to workspace at the given 0-based index.
    pub(crate) fn switch_workspace(&mut self, idx: usize) {
        if idx == self.ws.active || idx >= self.ws.workspaces.len() { return; }
        // Commit any pending IME composition to the current workspace's pane
        // before swapping state, otherwise the preedit text is lost.
        if self.ime.composing {
            if let Some(target) = self.ime.last_target {
                if !self.ime.preedit.is_empty() {
                    self.commit_text_to_pane(target, &self.ime.preedit.clone());
                }
            }
            self.ime.clear_composition();
            self.ime.last_target = None;
        }
        // Hide all browser WebViews in the current workspace before saving,
        // since native NSViews persist across workspace swaps.
        for pane in self.panes.values_mut() {
            if let PaneKind::Browser(bp) = pane {
                bp.set_visible(false);
                bp.is_first_responder = false;
            }
        }
        self.save_active_workspace();
        self.ws.active = idx;
        self.load_active_workspace();

        if let Some(id) = self.focused {
            self.router.set_focused(id);
        }
        self.pane_rects.clear();
        self.visual_pane_rects.clear();
        self.cache.pane_generations.clear();
        self.cache.invalidate_chrome();
        self.ime.cursor_dirty = true;
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

        let ws_name = format!("Workspace {}", self.ws.workspaces.len() + 1);
        self.ws.workspaces.push(Workspace {
            name: ws_name,
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        self.ws.active = self.ws.workspaces.len() - 1;

        self.create_terminal_pane(pane_id, None);
        self.router.set_focused(pane_id);
        self.focus_area = FocusArea::PaneArea;
        self.pane_rects.clear();
        self.visual_pane_rects.clear();
        self.cache.pane_generations.clear();
        self.cache.invalidate_chrome();
        self.compute_layout();
        self.update_file_tree_cwd();
    }

    /// Move a pane from the active workspace to a different workspace, then switch to it.
    /// If the pane is a terminal, all associated non-terminal panes move together,
    /// preserving their original TabGroup structure.
    pub(crate) fn move_pane_to_workspace(&mut self, pane_id: PaneId, target_idx: usize) {
        if target_idx == self.ws.active || target_idx >= self.ws.workspaces.len() {
            return;
        }

        // Collect associated panes if this is a terminal
        let associated_panes: Vec<PaneId> = if matches!(self.panes.get(&pane_id), Some(crate::PaneKind::Terminal(_))) {
            self.associated_terminal.iter()
                .filter(|(_, &tid)| tid == pane_id)
                .map(|(&pid, _)| pid)
                .collect()
        } else {
            Vec::new()
        };

        // Snapshot TabGroup membership BEFORE removing from layout.
        // Group associated panes by their TabGroup so we can reconstruct the structure.
        let mut tab_groups: Vec<Vec<PaneId>> = Vec::new();
        let mut grouped: std::collections::HashSet<PaneId> = std::collections::HashSet::new();
        for &pid in &associated_panes {
            if grouped.contains(&pid) {
                continue;
            }
            if let Some(tg) = self.layout.tab_group_containing(pid) {
                // Collect all associated panes that are in this same tab group
                let members: Vec<PaneId> = tg.tabs.iter()
                    .filter(|&&t| associated_panes.contains(&t))
                    .copied()
                    .collect();
                for &m in &members {
                    grouped.insert(m);
                }
                tab_groups.push(members);
            }
        }

        // All panes to move
        let all_panes_to_move: Vec<PaneId> = std::iter::once(pane_id)
            .chain(associated_panes.iter().copied())
            .collect();

        // Remove all from source layout and panes, collect PaneKind values
        let mut moved_panes: std::collections::HashMap<PaneId, crate::PaneKind> = std::collections::HashMap::new();
        for &pid in &all_panes_to_move {
            self.layout.remove(pid);
            if let Some(pane) = self.panes.remove(&pid) {
                self.cache.pane_generations.remove(&pid);
                self.interaction.scroll_accumulator.remove(&pid);
                if let Some(renderer) = self.renderer.as_mut() {
                    renderer.remove_pane_cache(pid);
                }
                moved_panes.insert(pid, pane);
            }
        }

        // Insert into target workspace, preserving TabGroup structure
        let target_ws = &mut self.ws.workspaces[target_idx];

        // 1. Insert the terminal pane
        if let Some(pane) = moved_panes.remove(&pane_id) {
            target_ws.layout.insert_at_root(pane_id, DropZone::Right);
            target_ws.panes.insert(pane_id, pane);
        }

        // 2. Insert associated panes, grouped by original TabGroup
        for group in &tab_groups {
            if group.is_empty() {
                continue;
            }
            // First pane in the group: insert as a new split
            let first = group[0];
            if let Some(pane) = moved_panes.remove(&first) {
                target_ws.layout.insert_at_root(first, DropZone::Right);
                target_ws.panes.insert(first, pane);
            }
            // Remaining panes: add as tabs in the same group
            for &pid in &group[1..] {
                if let Some(pane) = moved_panes.remove(&pid) {
                    target_ws.layout.add_tab(first, pid);
                    target_ws.panes.insert(pid, pane);
                }
            }
        }

        // 3. Insert any ungrouped associated panes (shouldn't happen, but safety)
        for (pid, pane) in moved_panes {
            target_ws.layout.insert_at_root(pid, DropZone::Right);
            target_ws.panes.insert(pid, pane);
        }

        // Update focus if the moved pane was focused
        if self.focused == Some(pane_id) || all_panes_to_move.contains(&self.focused.unwrap_or(0)) {
            self.focused = self.layout.pane_ids().into_iter().next();
            if let Some(id) = self.focused {
                self.router.set_focused(id);
            }
        }

        // Set focus in target workspace
        let target_ws = &mut self.ws.workspaces[target_idx];
        target_ws.focused = Some(pane_id);

        // Switch to the target workspace so the user sees the moved pane
        self.switch_workspace(target_idx);
    }

    /// Close the current workspace (only if more than one exists).
    pub(crate) fn close_workspace(&mut self) {
        if self.ws.workspaces.len() <= 1 { return; }

        // Destroy all panes in the current workspace
        let pane_ids: Vec<PaneId> = self.panes.keys().copied().collect();
        for id in pane_ids {
            if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&id) {
                bp.destroy();
            }
            self.panes.remove(&id);
            self.ime.pending_removes.push(id);
            self.cache.pane_generations.remove(&id);
            self.interaction.scroll_accumulator.remove(&id);
            if let Some(renderer) = self.renderer.as_mut() {
                renderer.remove_pane_cache(id);
            }
        }

        // Remove workspace from vec
        self.ws.workspaces.remove(self.ws.active);
        if self.ws.active >= self.ws.workspaces.len() {
            self.ws.active = self.ws.workspaces.len() - 1;
        }

        // Load the new active workspace
        self.load_active_workspace();
        if let Some(id) = self.focused {
            self.router.set_focused(id);
        }
        self.focus_area = FocusArea::PaneArea;
        self.pane_rects.clear();
        self.visual_pane_rects.clear();
        self.cache.pane_generations.clear();
        self.cache.invalidate_chrome();
        self.ime.cursor_dirty = true;
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
        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.active = 0;
        app.focused = Some(42);

        // Save: swaps app.focused ↔ ws[0].focused
        app.save_active_workspace();
        assert_eq!(app.ws.workspaces[0].focused, Some(42));
        assert_eq!(app.focused, None); // swapped out

        // Load: swaps back
        app.load_active_workspace();
        assert_eq!(app.focused, Some(42));
        assert_eq!(app.ws.workspaces[0].focused, None);
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
        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });

        // Set up WS1 as active with focused pane 100
        app.ws.active = 0;
        app.focused = Some(100);

        // Save WS1's state, switch to WS2
        // First save WS2 state manually so there's something to load
        app.save_active_workspace();
        app.ws.active = 1;
        app.focused = Some(200);
        app.save_active_workspace();

        // Now load WS1 back
        app.ws.active = 0;
        app.load_active_workspace();
        assert_eq!(app.focused, Some(100));

        // Switch to WS2 via the method
        app.switch_workspace(1);
        assert_eq!(app.ws.active, 1);
        assert_eq!(app.focused, Some(200));

        // Switch back to WS1
        app.switch_workspace(0);
        assert_eq!(app.ws.active, 0);
        assert_eq!(app.focused, Some(100));
    }

    #[test]
    fn switch_workspace_same_index_is_noop() {
        let mut app = test_app();
        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.active = 0;
        app.focused = Some(42);

        let gen_before = app.cache.chrome_generation;
        app.switch_workspace(0); // same index
        // Should not have changed anything
        assert_eq!(app.focused, Some(42));
        assert_eq!(app.cache.chrome_generation, gen_before);
    }

    #[test]
    fn switch_workspace_out_of_bounds_is_noop() {
        let mut app = test_app();
        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.active = 0;
        app.focused = Some(42);

        app.switch_workspace(99); // out of bounds
        assert_eq!(app.focused, Some(42));
        assert_eq!(app.ws.active, 0);
    }

    #[test]
    fn close_workspace_with_single_workspace_is_noop() {
        let mut app = test_app();
        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });

        app.close_workspace();
        assert_eq!(app.ws.workspaces.len(), 1); // still 1
    }

    #[test]
    fn close_workspace_removes_and_switches() {
        let mut app = test_app();

        // Set up two workspaces
        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: Some(200),
            panes: HashMap::new(),
        });
        app.ws.active = 0;
        app.focused = Some(100);

        // Close the first workspace
        app.close_workspace();

        assert_eq!(app.ws.workspaces.len(), 1);
        assert_eq!(app.ws.active, 0);
        assert_eq!(app.ws.workspaces[0].name, "WS2");
        // After close, the remaining workspace's state is loaded
        assert_eq!(app.focused, Some(200));
    }

    #[test]
    fn move_pane_to_workspace_transfers_pane_and_switches() {
        let mut app = test_app();

        // Create WS1 (active) with an editor pane
        let (layout, pane_id) = SplitLayout::with_initial_pane();
        app.layout = layout;
        app.focused = Some(pane_id);
        let pane = crate::editor_pane::EditorPane::new_empty(pane_id);
        app.panes.insert(pane_id, PaneKind::Editor(pane));

        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });

        // Create WS2 (empty, stored)
        app.ws.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.active = 0;

        let gen_before = app.cache.chrome_generation;

        // Move the pane to WS2
        app.move_pane_to_workspace(pane_id, 1);

        // Should have switched to WS2
        assert_eq!(app.ws.active, 1);
        // Moved pane should be focused in target workspace
        assert_eq!(app.focused, Some(pane_id));
        // Pane should exist in active workspace's panes
        assert!(app.panes.contains_key(&pane_id));
        // Chrome should be invalidated
        assert!(app.cache.chrome_generation > gen_before);
        // needs_redraw should be set
        assert!(app.cache.needs_redraw);
    }

    #[test]
    fn move_pane_to_workspace_cleans_up_scroll_accumulator() {
        let mut app = test_app();

        let (layout, pane_id) = SplitLayout::with_initial_pane();
        app.layout = layout;
        app.focused = Some(pane_id);
        let pane = crate::editor_pane::EditorPane::new_empty(pane_id);
        app.panes.insert(pane_id, PaneKind::Editor(pane));

        // Simulate scroll state
        app.interaction.scroll_accumulator.insert(pane_id, 3.5);

        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.active = 0;

        app.move_pane_to_workspace(pane_id, 1);

        // Scroll accumulator should not contain the old entry
        // (pane_generations.clear() in switch_workspace also cleared it)
        assert!(!app.interaction.scroll_accumulator.contains_key(&pane_id));
    }

    #[test]
    fn move_pane_to_workspace_source_loses_pane() {
        let mut app = test_app();

        // Create WS1 with two panes
        let (layout, pane_a) = SplitLayout::with_initial_pane();
        app.layout = layout;
        let pane_b = app.layout.alloc_id();
        app.layout.insert_at_root(pane_b, DropZone::Right);

        let editor_a = crate::editor_pane::EditorPane::new_empty(pane_a);
        let editor_b = crate::editor_pane::EditorPane::new_empty(pane_b);
        app.panes.insert(pane_a, PaneKind::Editor(editor_a));
        app.panes.insert(pane_b, PaneKind::Editor(editor_b));
        app.focused = Some(pane_a);

        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.active = 0;

        // Move pane_a to WS2
        app.move_pane_to_workspace(pane_a, 1);

        // Now in WS2 with pane_a
        assert_eq!(app.ws.active, 1);
        assert!(app.panes.contains_key(&pane_a));

        // Switch back to WS1
        app.switch_workspace(0);

        // WS1 should still have pane_b but not pane_a
        assert!(app.panes.contains_key(&pane_b));
        assert!(!app.panes.contains_key(&pane_a));
    }

    #[test]
    fn move_pane_to_same_workspace_is_noop() {
        let mut app = test_app();

        let (layout, pane_id) = SplitLayout::with_initial_pane();
        app.layout = layout;
        app.focused = Some(pane_id);
        let pane = crate::editor_pane::EditorPane::new_empty(pane_id);
        app.panes.insert(pane_id, PaneKind::Editor(pane));

        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.active = 0;

        let gen_before = app.cache.chrome_generation;
        app.move_pane_to_workspace(pane_id, 0); // same workspace

        // Should not have changed anything
        assert_eq!(app.ws.active, 0);
        assert_eq!(app.cache.chrome_generation, gen_before);
        assert!(app.panes.contains_key(&pane_id));
    }

    #[test]
    fn switch_workspace_sets_needs_redraw() {
        let mut app = test_app();

        app.ws.workspaces.push(Workspace {
            name: "WS1".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.workspaces.push(Workspace {
            name: "WS2".into(),
            layout: SplitLayout::new(),
            focused: None,
            panes: HashMap::new(),
        });
        app.ws.active = 0;
        app.focused = Some(100);
        app.save_active_workspace();
        app.ws.active = 1;
        app.focused = Some(200);
        app.save_active_workspace();
        app.ws.active = 0;
        app.load_active_workspace();

        app.cache.needs_redraw = false;
        app.switch_workspace(1);

        assert!(app.cache.needs_redraw);
        assert!(app.ime.cursor_dirty);
    }
}
