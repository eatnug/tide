use std::path::PathBuf;

use tide_core::{LayoutEngine, Renderer};

use crate::editor_pane::EditorPane;
use crate::pane::{PaneKind, TerminalPane};
use crate::App;

impl App {
    pub(crate) fn create_terminal_pane(&mut self, id: tide_core::PaneId, cwd: Option<std::path::PathBuf>) {
        let cell_size = self.renderer.as_ref().unwrap().cell_size();
        let logical = self.logical_size();
        let cols = (logical.width / 2.0 / cell_size.width).max(1.0) as u16;
        let rows = (logical.height / cell_size.height).max(1.0) as u16;

        match TerminalPane::with_cwd(id, cols, rows, cwd) {
            Ok(pane) => {
                self.install_pty_waker(&pane);
                self.panes.insert(id, PaneKind::Terminal(pane));
            }
            Err(e) => {
                log::error!("Failed to create terminal pane: {}", e);
            }
        }
    }

    /// Get the CWD of the currently focused terminal pane, if any.
    pub(super) fn focused_terminal_cwd(&self) -> Option<std::path::PathBuf> {
        let focused = self.focused?;
        match self.panes.get(&focused) {
            Some(PaneKind::Terminal(p)) => p.backend.detect_cwd_fallback(),
            _ => None,
        }
    }

    /// Create a new empty editor pane in the panel.
    /// Auto-shows the editor panel if it was hidden.
    pub(crate) fn new_editor_pane(&mut self) {
        if !self.show_editor_panel {
            self.show_editor_panel = true;
        }
        let panel_was_visible = !self.editor_panel_tabs.is_empty();
        let new_id = self.layout.alloc_id();
        let pane = EditorPane::new_empty(new_id);
        self.panes.insert(new_id, PaneKind::Editor(pane));
        self.editor_panel_tabs.push(new_id);
        self.editor_panel_active = Some(new_id);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.chrome_generation += 1;
        if !panel_was_visible {
            if !self.editor_panel_width_manual {
                self.editor_panel_width = self.auto_editor_panel_width();
            }
            self.compute_layout();
        }
        self.scroll_to_active_panel_tab();
    }

    /// Open a file in the editor panel. If already open, activate its tab.
    /// Auto-shows the editor panel if it was hidden.
    pub(crate) fn open_editor_pane(&mut self, path: PathBuf) {
        // Track whether panel needs layout recompute (becoming visible)
        let needs_layout = !self.show_editor_panel
            || (self.show_editor_panel && self.editor_panel_tabs.is_empty());

        // Auto-show editor panel if hidden
        if !self.show_editor_panel {
            self.show_editor_panel = true;
        }
        // Check if already open in panel tabs -> activate & focus
        for &tab_id in &self.editor_panel_tabs {
            if let Some(PaneKind::Editor(editor)) = self.panes.get(&tab_id) {
                if editor.editor.file_path() == Some(path.as_path()) {
                    self.editor_panel_active = Some(tab_id);
                    self.pane_generations.remove(&tab_id);
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.chrome_generation += 1;
                    if needs_layout {
                        if !self.editor_panel_width_manual {
                            self.editor_panel_width = self.auto_editor_panel_width();
                        }
                        self.compute_layout();
                    }
                    self.scroll_to_active_panel_tab();
                    return;
                }
            }
        }

        // Check if already open in split tree -> focus
        for (&id, pane) in &self.panes {
            if let PaneKind::Editor(editor) = pane {
                if editor.editor.file_path() == Some(path.as_path()) {
                    self.focused = Some(id);
                    self.router.set_focused(id);
                    self.chrome_generation += 1;
                    return;
                }
            }
        }

        // Create new editor pane in the panel
        let new_id = self.layout.alloc_id();
        match EditorPane::open(new_id, &path) {
            Ok(pane) => {
                self.panes.insert(new_id, PaneKind::Editor(pane));
                self.editor_panel_tabs.push(new_id);
                self.editor_panel_active = Some(new_id);
                self.focused = Some(new_id);
                self.router.set_focused(new_id);
                self.chrome_generation += 1;
                // Watch the file for external changes
                self.watch_file(&path);
                // Recompute layout if the panel just became visible (causes terminal resize)
                if needs_layout {
                    if !self.editor_panel_width_manual {
                        self.editor_panel_width = self.auto_editor_panel_width();
                    }
                    self.compute_layout();
                }
                self.scroll_to_active_panel_tab();
            }
            Err(e) => {
                log::error!("Failed to open editor for {:?}: {}", path, e);
            }
        }
    }

    /// Close an editor panel tab. If dirty, show save confirm bar instead.
    pub(crate) fn close_editor_panel_tab(&mut self, tab_id: tide_core::PaneId) {
        // Check if editor is dirty -> show save confirm bar
        if let Some(PaneKind::Editor(pane)) = self.panes.get(&tab_id) {
            if pane.editor.is_modified() {
                self.save_confirm = Some(crate::SaveConfirmState { pane_id: tab_id });
                // Ensure this tab is active and focused so the bar is visible
                if self.editor_panel_tabs.contains(&tab_id) {
                    self.editor_panel_active = Some(tab_id);
                }
                self.focused = Some(tab_id);
                self.router.set_focused(tab_id);
                self.chrome_generation += 1;
                self.pane_generations.remove(&tab_id);
                return;
            }
        }
        self.force_close_editor_panel_tab(tab_id);
    }

    /// Force close an editor panel tab (no dirty check).
    pub(crate) fn force_close_editor_panel_tab(&mut self, tab_id: tide_core::PaneId) {
        // Cancel save-as if the target pane is being closed
        if self.save_as_input.as_ref().is_some_and(|s| s.pane_id == tab_id) {
            self.save_as_input = None;
        }
        // Cancel save confirm if the target pane is being closed
        if self.save_confirm.as_ref().is_some_and(|s| s.pane_id == tab_id) {
            self.save_confirm = None;
        }
        // Unwatch the file before removing the pane
        let watch_path = if let Some(PaneKind::Editor(editor)) = self.panes.get(&tab_id) {
            editor.editor.file_path().map(|p| p.to_path_buf())
        } else {
            None
        };
        if let Some(path) = watch_path {
            self.unwatch_file(&path);
        }
        self.editor_panel_tabs.retain(|&id| id != tab_id);
        self.panes.remove(&tab_id);
        self.cleanup_closed_pane_state(tab_id);

        if self.editor_panel_tabs.is_empty() {
            self.show_editor_panel = false;
            self.editor_panel_maximized = false;
            self.editor_panel_width_manual = false;
        }

        // Switch active to last remaining tab (or None)
        if self.editor_panel_active == Some(tab_id) {
            self.editor_panel_active = self.editor_panel_tabs.last().copied();
        }

        // If focused pane was the closed tab, switch focus
        if self.focused == Some(tab_id) {
            if let Some(active) = self.editor_panel_active {
                self.focused = Some(active);
                self.router.set_focused(active);
            } else if let Some(&first) = self.layout.pane_ids().first() {
                self.focused = Some(first);
                self.router.set_focused(first);
            } else {
                self.focused = None;
            }
        }

        self.pane_generations.clear();
        self.chrome_generation += 1;
        self.compute_layout();
        self.clamp_panel_tab_scroll();
        self.scroll_to_active_panel_tab();
    }

    /// Complete the save-as flow: resolve path, set file_path, detect syntax, save, watch.
    pub(crate) fn complete_save_as(&mut self, pane_id: tide_core::PaneId, filename: &str) {
        let path = if std::path::Path::new(filename).is_absolute() {
            PathBuf::from(filename)
        } else {
            self.resolve_base_dir().join(filename)
        };

        // Create parent dirs if needed
        if let Some(parent) = path.parent() {
            if !parent.exists() {
                let _ = std::fs::create_dir_all(parent);
            }
        }

        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
            pane.editor.buffer.file_path = Some(path.clone());
            pane.editor.detect_and_set_syntax(&path);
            if let Err(e) = pane.editor.buffer.save() {
                log::error!("Failed to save file: {}", e);
            }
            pane.disk_changed = false;
        }

        self.watch_file(&path);
        self.chrome_generation += 1;
    }

    /// Close a specific pane by its ID (used by close button clicks).
    pub(crate) fn close_specific_pane(&mut self, pane_id: tide_core::PaneId) {
        // If the pane is in the editor panel, close the panel tab (with dirty check)
        if self.editor_panel_tabs.contains(&pane_id) {
            self.close_editor_panel_tab(pane_id);
            self.update_file_tree_cwd();
            return;
        }

        // Check if editor is dirty -> show save confirm bar
        if let Some(PaneKind::Editor(pane)) = self.panes.get(&pane_id) {
            if pane.editor.is_modified() {
                self.save_confirm = Some(crate::SaveConfirmState { pane_id });
                self.focused = Some(pane_id);
                self.router.set_focused(pane_id);
                self.chrome_generation += 1;
                self.pane_generations.remove(&pane_id);
                return;
            }
        }

        self.force_close_specific_pane(pane_id);
    }

    /// Force close a specific pane (no dirty check).
    pub(crate) fn force_close_specific_pane(&mut self, pane_id: tide_core::PaneId) {
        // Cancel save-as if the target pane is being closed
        if self.save_as_input.as_ref().is_some_and(|s| s.pane_id == pane_id) {
            self.save_as_input = None;
        }
        // Cancel save confirm
        if self.save_confirm.as_ref().is_some_and(|s| s.pane_id == pane_id) {
            self.save_confirm = None;
        }
        // If the pane is in the editor panel, force close the panel tab
        if self.editor_panel_tabs.contains(&pane_id) {
            self.force_close_editor_panel_tab(pane_id);
            self.update_file_tree_cwd();
            return;
        }

        // Clear maximize if the maximized pane is being closed
        if self.maximized_pane == Some(pane_id) {
            self.maximized_pane = None;
        }

        let remaining = self.layout.pane_ids();
        if remaining.len() <= 1 && self.editor_panel_tabs.is_empty() {
            let session = crate::session::Session::from_app(self);
            crate::session::save_session(&session);
            std::process::exit(0);
        }
        if remaining.len() <= 1 {
            // Last tree pane but panel has tabs -- focus panel instead
            if let Some(active) = self.editor_panel_active {
                self.focused = Some(active);
                self.router.set_focused(active);
                self.chrome_generation += 1;
            }
            return;
        }

        self.layout.remove(pane_id);
        self.panes.remove(&pane_id);
        self.cleanup_closed_pane_state(pane_id);

        // Focus the first remaining pane
        let remaining = self.layout.pane_ids();
        if let Some(&next) = remaining.first() {
            self.focused = Some(next);
            self.router.set_focused(next);
        } else {
            self.focused = None;
        }

        self.chrome_generation += 1;
        self.compute_layout();
        self.update_file_tree_cwd();
    }

    /// Save and close the pane from the save confirm bar.
    pub(crate) fn confirm_save_and_close(&mut self) {
        let pane_id = match self.save_confirm.take() {
            Some(sc) => sc.pane_id,
            None => return,
        };
        // Save
        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
            if pane.editor.file_path().is_none() {
                // Untitled file -> open save-as input
                self.save_as_input = Some(crate::SaveAsInput::new(pane_id));
                return;
            }
            if let Err(e) = pane.editor.buffer.save() {
                log::error!("Save failed: {}", e);
                return;
            }
            pane.disk_changed = false;
        }
        // Close
        if self.editor_panel_tabs.contains(&pane_id) {
            self.force_close_editor_panel_tab(pane_id);
        } else {
            self.force_close_specific_pane(pane_id);
        }
    }

    /// Discard changes and close the pane from the save confirm bar.
    pub(crate) fn confirm_discard_and_close(&mut self) {
        let pane_id = match self.save_confirm.take() {
            Some(sc) => sc.pane_id,
            None => return,
        };
        if self.editor_panel_tabs.contains(&pane_id) {
            self.force_close_editor_panel_tab(pane_id);
        } else {
            self.force_close_specific_pane(pane_id);
        }
    }

    /// Cancel the save confirm bar.
    pub(crate) fn cancel_save_confirm(&mut self) {
        if self.save_confirm.is_some() {
            self.save_confirm = None;
            self.chrome_generation += 1;
            self.pane_generations.clear();
        }
    }
}
