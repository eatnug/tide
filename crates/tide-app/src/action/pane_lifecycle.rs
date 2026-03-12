use std::path::PathBuf;

use tide_core::LayoutEngine;

use crate::browser_pane::BrowserPane;
use crate::drag_drop::PaneDragState;
use crate::editor_pane::EditorPane;
use crate::pane::{PaneKind, TerminalPane};
use crate::App;

impl App {
    pub(crate) fn create_terminal_pane(&mut self, id: tide_core::PaneId, cwd: Option<std::path::PathBuf>) {
        let cell_size = self.cell_size();
        if cell_size.width <= 0.0 || cell_size.height <= 0.0 {
            log::error!("Cannot create terminal pane: cell_size is zero ({:?})", cell_size);
            return;
        }
        let logical = self.logical_size();
        let cols = ((logical.width / 2.0 / cell_size.width).max(1.0).min(1000.0)) as u16;
        let rows = ((logical.height / cell_size.height).max(1.0).min(500.0)) as u16;

        match TerminalPane::with_cwd(id, cols, rows, cwd, self.dark_mode) {
            Ok(pane) => {
                self.install_pty_waker(&pane);
                self.panes.insert(id, PaneKind::Terminal(pane));
                self.ime.pending_creates.push(id);
            }
            Err(e) => {
                log::error!("Failed to create terminal pane: {}", e);
            }
        }
    }

    /// Respawn a new shell in a dead terminal pane, preserving its position in the layout.
    pub(crate) fn respawn_terminal(&mut self, id: tide_core::PaneId) {
        // Get the CWD of the dead terminal before removing it
        let cwd = if let Some(PaneKind::Terminal(pane)) = self.panes.get(&id) {
            pane.cwd.clone().or_else(|| pane.backend.detect_cwd_fallback())
        } else {
            None
        };
        // Remove old terminal and create a new one in-place
        self.panes.remove(&id);
        self.create_terminal_pane(id, cwd);
        // Clear IME composition if the recreated pane was the target.
        if self.ime.last_target == Some(id) {
            self.ime.clear_composition();
            self.ime.last_target = None;
        }
        self.cache.invalidate_pane(id);
        self.cache.invalidate_chrome();
        self.compute_layout();
    }

    /// Get the CWD of the currently focused terminal pane, if any.
    /// When an editor/diff pane is focused, find the first terminal's CWD.
    pub(crate) fn focused_terminal_cwd(&self) -> Option<std::path::PathBuf> {
        // If focused pane is a terminal, use its CWD
        if let Some(focused) = self.focused {
            if let Some(PaneKind::Terminal(p)) = self.panes.get(&focused) {
                return p.backend.detect_cwd_fallback();
            }
        }
        // Otherwise, find any terminal pane and use its CWD
        for &id in &self.layout.pane_ids() {
            if let Some(PaneKind::Terminal(p)) = self.panes.get(&id) {
                if let Some(cwd) = p.backend.detect_cwd_fallback() {
                    return Some(cwd);
                }
            }
        }
        // Fall back to last known CWD
        self.last_cwd.clone()
    }

    /// Create a new empty editor pane as a tab in the focused pane's tab group.
    pub(crate) fn new_editor_pane(&mut self) {
        let focused = match self.focused {
            Some(id) => id,
            None => return,
        };
        let new_id = self.layout.alloc_id();
        let mut pane = EditorPane::new_empty(new_id);
        pane.editor.set_dark_mode(self.dark_mode);
        self.panes.insert(new_id, PaneKind::Editor(pane));
        self.ime.pending_creates.push(new_id);
        self.layout.add_tab(focused, new_id);
        self.layout.set_active_tab(new_id);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        if self.zoomed_pane.is_some() {
            self.zoomed_pane = Some(new_id);
        }
        self.focus_area = crate::ui_state::FocusArea::PaneArea;
        self.cache.invalidate_chrome();
        self.compute_layout();
    }

    /// Create a new Launcher tab in the focused pane's tab group.
    /// The Launcher shows a type-selection screen (T/E/O/B).
    pub(crate) fn new_terminal_tab(&mut self) {
        let focused = match self.focused {
            Some(id) => id,
            None => return,
        };
        let new_id = self.layout.alloc_id();
        self.layout.add_tab(focused, new_id);
        self.panes.insert(new_id, PaneKind::Launcher(new_id));
        self.ime.pending_creates.push(new_id);
        self.layout.set_active_tab(new_id);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        if self.zoomed_pane.is_some() {
            self.zoomed_pane = Some(new_id);
        }
        self.focus_area = crate::ui_state::FocusArea::PaneArea;
        self.cache.invalidate_chrome();
        self.compute_layout();
    }

    /// Replace a Launcher pane with the chosen pane type.
    pub(crate) fn resolve_launcher(&mut self, launcher_id: tide_core::PaneId, choice: LauncherChoice) {
        match choice {
            LauncherChoice::Terminal => {
                // Remove the old launcher's IME proxy before creating the replacement.
                // The new pane reuses the same PaneId, so without this the platform's
                // ime_proxies map still holds the stale launcher proxy and
                // create_ime_proxy() skips creation — leaving first responder on the
                // old proxy and routing keyboard input to the wrong pane.
                self.ime.pending_removes.push(launcher_id);
                let cwd = self.focused_terminal_cwd();
                self.panes.remove(&launcher_id);
                self.create_terminal_pane(launcher_id, cwd);
            }
            LauncherChoice::NewFile => {
                self.ime.pending_removes.push(launcher_id);
                let mut pane = crate::editor_pane::EditorPane::new_empty(launcher_id);
                pane.editor.set_dark_mode(self.dark_mode);
                self.panes.insert(launcher_id, PaneKind::Editor(pane));
                self.ime.pending_creates.push(launcher_id);
            }
            LauncherChoice::OpenFile => {
                // Keep the launcher alive — the file finder will replace it
                // with the selected file's editor pane (same layout slot).
                self.open_file_finder_with_replace(Some(launcher_id));
                return;
            }
            LauncherChoice::Browser => {
                self.ime.pending_removes.push(launcher_id);
                let pane = crate::browser_pane::BrowserPane::new(launcher_id);
                self.panes.insert(launcher_id, PaneKind::Browser(pane));
                self.ime.pending_creates.push(launcher_id);
            }
        }
        self.focused = Some(launcher_id);
        self.router.set_focused(launcher_id);
        self.cache.invalidate_chrome();
        self.cache.pane_generations.clear();
        self.compute_layout();
    }


    /// Split the focused pane and show a Launcher in the new tab group.
    /// Used by keyboard-initiated splits (Cmd+\, etc.).
    pub(crate) fn split_with_launcher(&mut self, direction: tide_core::SplitDirection) {
        let focused = match self.focused {
            Some(id) => id,
            None => return,
        };
        if self.zoomed_pane.is_some() {
            self.zoomed_pane = None;
            self.cache.pane_generations.clear();
        }
        let new_id = self.layout.split(focused, direction);
        self.panes.insert(new_id, PaneKind::Launcher(new_id));
        self.ime.pending_creates.push(new_id);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.cache.invalidate_chrome();
        self.compute_layout();
    }

    /// Open a browser pane to the right of the focused pane's tab group.
    /// If a tab group already exists to the right, the browser is added there.
    /// Otherwise a new horizontal split is created.
    pub(crate) fn open_browser_pane(&mut self, url: Option<String>) {
        let focused = match self.focused {
            Some(id) => id,
            None => return,
        };
        let new_id = self.layout.alloc_id();
        let pane = match url {
            Some(ref u) => BrowserPane::with_url(new_id, u.clone()),
            None => BrowserPane::new(new_id),
        };
        self.panes.insert(new_id, PaneKind::Browser(pane));
        self.ime.pending_creates.push(new_id);
        self.add_pane_to_right(focused, new_id);
        self.layout.set_active_tab(new_id);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.focus_area = crate::ui_state::FocusArea::PaneArea;
        self.cache.invalidate_chrome();
        self.compute_layout();
    }

    /// Replace an existing pane (e.g. a Launcher) with an editor for the given file.
    /// The editor reuses the same layout slot (PaneId stays in the same TabGroup position).
    pub(crate) fn replace_pane_with_editor(&mut self, pane_id: tide_core::PaneId, path: PathBuf) {
        // Check if already open anywhere -> activate & focus (and close the launcher)
        for (&id, pane) in &self.panes {
            if let PaneKind::Editor(editor) = pane {
                if editor.editor.file_path() == Some(path.as_path()) {
                    // File already open — focus it and close the launcher
                    self.layout.set_active_tab(id);
                    self.cache.invalidate_pane(id);
                    self.focused = Some(id);
                    self.router.set_focused(id);
                    self.focus_area = crate::ui_state::FocusArea::PaneArea;
                    // Remove the launcher pane
                    self.layout.remove(pane_id);
                    self.panes.remove(&pane_id);
                    self.cleanup_closed_pane_state(pane_id);
                    self.cache.invalidate_chrome();
                    self.compute_layout();
                    return;
                }
            }
        }

        // Replace the pane in-place: swap PaneKind from Launcher to Editor
        match EditorPane::open(pane_id, &path) {
            Ok(mut pane) => {
                pane.editor.set_dark_mode(self.dark_mode);
                self.panes.insert(pane_id, PaneKind::Editor(pane));
                // Clear IME composition if the replaced pane was the target.
                if self.ime.last_target == Some(pane_id) {
                    self.ime.clear_composition();
                    self.ime.last_target = None;
                }
                self.focused = Some(pane_id);
                self.router.set_focused(pane_id);
                self.focus_area = crate::ui_state::FocusArea::PaneArea;
                self.cache.invalidate_chrome();
                self.cache.pane_generations.clear();
                self.watch_file(&path);
                self.compute_layout();
            }
            Err(e) => {
                log::error!("Failed to open editor for {:?}: {}", path, e);
            }
        }
    }

    /// Open a file to the right of the focused pane's tab group.
    /// If a tab group already exists to the right, the editor is added there.
    /// Otherwise a new horizontal split is created.
    /// If already open, activate its tab.
    pub(crate) fn open_editor_pane(&mut self, path: PathBuf) {
        let focused = match self.focused {
            Some(id) => id,
            None => return,
        };

        // Check if already open anywhere -> activate & focus
        for (&id, pane) in &self.panes {
            if let PaneKind::Editor(editor) = pane {
                if editor.editor.file_path() == Some(path.as_path()) {
                    self.layout.set_active_tab(id);
                    self.cache.invalidate_pane(id);
                    self.focused = Some(id);
                    self.router.set_focused(id);
                    self.focus_area = crate::ui_state::FocusArea::PaneArea;
                    self.cache.invalidate_chrome();
                    self.compute_layout();
                    return;
                }
            }
        }

        // Create new editor pane to the right
        let new_id = self.layout.alloc_id();
        match EditorPane::open(new_id, &path) {
            Ok(mut pane) => {
                pane.editor.set_dark_mode(self.dark_mode);
                self.panes.insert(new_id, PaneKind::Editor(pane));
                self.ime.pending_creates.push(new_id);
                self.add_pane_to_right(focused, new_id);
                self.layout.set_active_tab(new_id);
                self.focused = Some(new_id);
                self.router.set_focused(new_id);
                self.focus_area = crate::ui_state::FocusArea::PaneArea;
                self.cache.invalidate_chrome();
                // Watch the file for external changes
                self.watch_file(&path);
                self.compute_layout();
            }
            Err(e) => {
                log::error!("Failed to open editor for {:?}: {}", path, e);
            }
        }
    }

    /// Open a file in the editor and jump to a specific line.
    pub(crate) fn open_editor_pane_at_line(&mut self, path: PathBuf, line: Option<usize>) {
        self.open_editor_pane(path);
        if let Some(line) = line {
            if let Some(active_id) = self.focused {
                let visible_rows = self.visible_editor_size(active_id).0;
                if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&active_id) {
                    let target_line = line.saturating_sub(1); // 1-based to 0-based
                    pane.handle_action(
                        tide_editor::input::EditorAction::SetCursor { line: target_line, col: 0 },
                        visible_rows,
                    );
                    pane.editor.ensure_cursor_visible(visible_rows.max(30));
                }
            }
        }
    }

    /// Close a pane tab. If dirty (and has a file path), show save confirm bar instead.
    /// Untitled (new) files and browser panes close immediately without prompting.
    pub(crate) fn close_editor_panel_tab(&mut self, tab_id: tide_core::PaneId) {
        // Browser panes close immediately (no dirty check)
        if matches!(self.panes.get(&tab_id), Some(PaneKind::Browser(_))) {
            self.force_close_editor_panel_tab(tab_id);
            return;
        }
        // Check if editor is dirty -> show save confirm bar (skip for untitled files)
        if let Some(PaneKind::Editor(pane)) = self.panes.get(&tab_id) {
            if pane.editor.is_modified() && pane.editor.file_path().is_some() {
                self.modal.save_confirm = Some(crate::SaveConfirmState { pane_id: tab_id });
                // Ensure this tab is active and focused so the bar is visible
                self.layout.set_active_tab(tab_id);
                self.focused = Some(tab_id);
                self.router.set_focused(tab_id);
                self.cache.invalidate_chrome();
                self.cache.invalidate_pane(tab_id);
                return;
            }
        }
        self.force_close_editor_panel_tab(tab_id);
    }

    /// Force close a pane tab (no dirty check).
    pub(crate) fn force_close_editor_panel_tab(&mut self, tab_id: tide_core::PaneId) {
        // Cancel drag if the closing pane is the drag source
        if self.interaction.pane_drag.source_pane() == Some(tab_id) {
            self.interaction.pane_drag = PaneDragState::Idle;
        }
        // Destroy webview before removing the pane
        if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&tab_id) {
            bp.destroy();
        }
        // Cancel save-as if the target pane is being closed
        if self.modal.save_as_input.as_ref().is_some_and(|s| s.pane_id == tab_id) {
            self.modal.save_as_input = None;
        }
        // Cancel save confirm if the target pane is being closed
        if self.modal.save_confirm.as_ref().is_some_and(|s| s.pane_id == tab_id) {
            self.modal.save_confirm = None;
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

        // Remove from layout (handles multi-tab groups automatically)
        self.layout.remove(tab_id);
        self.panes.remove(&tab_id);
        self.cleanup_closed_pane_state(tab_id);

        // If focused pane was the closed tab, switch focus to the layout's
        // active tab in the same group (set by TabGroup::remove_tab).
        if self.focused == Some(tab_id) {
            let remaining = self.layout.pane_ids();
            let target = if remaining.is_empty() {
                None
            } else {
                // The layout already adjusted the active tab index in the
                // group that contained tab_id. Find which pane is now active
                // by checking each remaining pane's group.
                // First, try the first remaining pane's group active pane
                // (covers single-group and multi-group cases).
                let mut active_in_group = None;
                for &id in &remaining {
                    if let Some(tg) = self.layout.tab_group_containing(id) {
                        let ap = tg.active_pane();
                        if active_in_group.is_none() {
                            active_in_group = Some(ap);
                            break;
                        }
                    }
                }
                active_in_group.or_else(|| remaining.first().copied())
            };
            if let Some(id) = target {
                self.focused = Some(id);
                self.router.set_focused(id);
                self.layout.set_active_tab(id);
            } else {
                self.focused = None;
            }
            self.focus_area = crate::ui_state::FocusArea::PaneArea;
        }

        // Check if layout is now empty
        if self.layout.pane_ids().is_empty() {
            // If other workspaces exist, close this one instead of exiting
            if self.ws.workspaces.len() > 1 {
                self.close_workspace();
                return;
            }
            let session = crate::session::Session::from_app(self);
            crate::session::save_session(&session);
            crate::session::delete_running_marker();
            std::process::exit(0);
        }

        self.cache.pane_generations.clear();
        self.cache.invalidate_chrome();
        self.compute_layout();
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
        self.cache.invalidate_chrome();
    }

    /// Close a specific pane by its ID (used by close button clicks).
    pub(crate) fn close_specific_pane(&mut self, pane_id: tide_core::PaneId) {
        // Check if editor is dirty -> show save confirm bar
        if let Some(PaneKind::Editor(pane)) = self.panes.get(&pane_id) {
            if pane.editor.is_modified() && pane.editor.file_path().is_some() {
                self.modal.save_confirm = Some(crate::SaveConfirmState { pane_id });
                self.layout.set_active_tab(pane_id);
                self.focused = Some(pane_id);
                self.router.set_focused(pane_id);
                self.cache.invalidate_chrome();
                self.cache.invalidate_pane(pane_id);
                return;
            }
        }

        // Browser panes and clean editors close immediately
        if matches!(self.panes.get(&pane_id), Some(PaneKind::Editor(_) | PaneKind::Browser(_) | PaneKind::Diff(_))) {
            self.force_close_editor_panel_tab(pane_id);
            self.update_file_tree_cwd();
            return;
        }

        // Terminal pane: proceed to force close (with branch cleanup check)
        self.force_close_specific_pane(pane_id);
    }

    /// Force close a specific pane (no dirty check).
    /// May show branch cleanup confirmation for terminals on non-main branches.
    pub(crate) fn force_close_specific_pane(&mut self, pane_id: tide_core::PaneId) {
        // Cancel save-as if the target pane is being closed
        if self.modal.save_as_input.as_ref().is_some_and(|s| s.pane_id == pane_id) {
            self.modal.save_as_input = None;
        }
        // Cancel save confirm
        if self.modal.save_confirm.as_ref().is_some_and(|s| s.pane_id == pane_id) {
            self.modal.save_confirm = None;
        }

        // Non-terminal panes: close directly
        if !matches!(self.panes.get(&pane_id), Some(PaneKind::Terminal(_))) {
            self.force_close_editor_panel_tab(pane_id);
            self.update_file_tree_cwd();
            return;
        }

        // If branch cleanup bar is already showing for this pane, block the close —
        // the user must resolve it via Delete/Keep/Cancel first.
        if self.modal.branch_cleanup.as_ref().is_some_and(|bc| bc.pane_id == pane_id) {
            return;
        }

        // Branch cleanup check: if this is a terminal on a non-main branch,
        // prompt before closing (unless cleanup is already active for another pane).
        if self.modal.branch_cleanup.is_none() {
            if let Some(PaneKind::Terminal(pane)) = self.panes.get(&pane_id) {
                if let (Some(ref gi), Some(ref cwd)) = (&pane.git_info, &pane.cwd) {
                    let branch = &gi.branch;
                    if branch != "main" && branch != "master" {
                        // Check no other terminal pane is on the same branch
                        let other_on_same = self.panes.iter().any(|(&id, pk)| {
                            if id == pane_id { return false; }
                            if let PaneKind::Terminal(tp) = pk {
                                tp.git_info.as_ref()
                                    .map(|g| g.branch == *branch)
                                    .unwrap_or(false)
                            } else {
                                false
                            }
                        });
                        if !other_on_same {
                            // Detect if cwd is in a worktree
                            let worktrees = tide_terminal::git::list_worktrees(cwd);
                            let wt_path = worktrees.iter()
                                .find(|wt| wt.is_current && !wt.is_main)
                                .map(|wt| wt.path.clone());

                            self.modal.branch_cleanup = Some(crate::BranchCleanupState {
                                pane_id,
                                branch: branch.clone(),
                                worktree_path: wt_path,
                                cwd: cwd.clone(),
                            });
                            self.cache.invalidate_chrome();
                            return;
                        }
                    }
                }
            }
        }

        self.close_pane_final(pane_id);
    }

    /// Close a pane unconditionally (no dirty check, no branch cleanup check).
    /// Used by branch cleanup confirm/keep methods after cleanup is resolved.
    fn close_pane_final(&mut self, pane_id: tide_core::PaneId) {
        // Cancel drag if the closing pane is the drag source
        if self.interaction.pane_drag.source_pane() == Some(pane_id) {
            self.interaction.pane_drag = PaneDragState::Idle;
        }
        let remaining = self.layout.pane_ids();
        if remaining.len() <= 1 {
            // If other workspaces exist, close this one instead of exiting
            if self.ws.workspaces.len() > 1 {
                self.close_workspace();
                return;
            }
            let session = crate::session::Session::from_app(self);
            crate::session::save_session(&session);
            std::process::exit(0);
        }

        // Determine which pane to focus before removing: prefer previous (left/above)
        let pane_ids = self.layout.pane_ids();
        let pos = pane_ids.iter().position(|&id| id == pane_id);
        let next_focus = pos.and_then(|p| {
            if p > 0 {
                Some(pane_ids[p - 1]) // previous (left/above)
            } else if p + 1 < pane_ids.len() {
                Some(pane_ids[p + 1]) // next (right/below)
            } else {
                None
            }
        });

        self.layout.remove(pane_id);
        self.panes.remove(&pane_id);
        self.cleanup_closed_pane_state(pane_id);

        if let Some(next) = next_focus {
            self.focused = Some(next);
            self.router.set_focused(next);
        } else {
            self.focused = None;
        }

        self.cache.invalidate_chrome();
        self.compute_layout();
        self.update_file_tree_cwd();
    }

    /// Save and close the pane from the save confirm bar.
    pub(crate) fn confirm_save_and_close(&mut self) {
        let pane_id = match self.modal.save_confirm.take() {
            Some(sc) => sc.pane_id,
            None => return,
        };
        // Save
        if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
            if pane.editor.file_path().is_none() {
                // Untitled file -> open save-as input
                let base_dir = self.resolve_base_dir();
                let anchor = self.visual_pane_rects.iter()
                    .find(|(id, _)| *id == pane_id)
                    .map(|(_, r)| tide_core::Rect::new(r.x, r.y, r.width, crate::theme::TAB_BAR_HEIGHT))
                    .unwrap_or_else(|| tide_core::Rect::new(0.0, 0.0, 0.0, 0.0));
                self.modal.save_as_input = Some(crate::SaveAsInput::new(pane_id, base_dir, anchor));
                return;
            }
            if let Err(e) = pane.editor.buffer.save() {
                log::error!("Save failed: {}", e);
                return;
            }
            pane.disk_changed = false;
        }
        // Close
        self.force_close_editor_panel_tab(pane_id);
        // Retry pending terminal close (may find more dirty editors)
        if let Some(tid) = self.pending_terminal_close.take() {
            if self.panes.contains_key(&tid) {
                self.close_specific_pane(tid);
            }
        }
    }

    /// Discard changes and close the pane from the save confirm bar.
    pub(crate) fn confirm_discard_and_close(&mut self) {
        let pane_id = match self.modal.save_confirm.take() {
            Some(sc) => sc.pane_id,
            None => return,
        };
        self.force_close_editor_panel_tab(pane_id);
        // Retry pending terminal close (may find more dirty editors)
        if let Some(tid) = self.pending_terminal_close.take() {
            if self.panes.contains_key(&tid) {
                self.close_specific_pane(tid);
            }
        }
    }

    /// Cancel the save confirm bar.
    pub(crate) fn cancel_save_confirm(&mut self) {
        if self.modal.save_confirm.is_some() {
            self.modal.save_confirm = None;
            self.pending_terminal_close = None;
            self.cache.invalidate_chrome();
            self.cache.pane_generations.clear();
        }
    }

    /// Delete the branch/worktree and proceed with closing the terminal pane.
    pub(crate) fn confirm_branch_delete(&mut self) {
        let bc = match self.modal.branch_cleanup.take() {
            Some(bc) => bc,
            None => return,
        };
        // Resolve the main worktree path BEFORE closing anything.
        // bc.cwd may be inside a worktree that will be removed.
        let main_cwd = if bc.worktree_path.is_some() {
            let worktrees = tide_terminal::git::list_worktrees(&bc.cwd);
            worktrees.iter()
                .find(|wt| wt.is_main)
                .map(|wt| wt.path.clone())
                .unwrap_or_else(|| bc.cwd.clone())
        } else {
            bc.cwd.clone()
        };
        // Close the pane first so the terminal process releases the directory
        self.close_pane_final(bc.pane_id);
        // Remove worktree if applicable (directory is now free)
        if let Some(ref wt_path) = bc.worktree_path {
            if let Err(e) = tide_terminal::git::remove_worktree(&main_cwd, wt_path, true) {
                log::error!("Failed to remove worktree: {}", e);
            }
        }
        // Delete the branch from the main repo
        if let Err(e) = tide_terminal::git::delete_branch(&main_cwd, &bc.branch, true) {
            log::error!("Failed to delete branch: {}", e);
        }
    }

    /// Keep the branch and proceed with closing the terminal pane.
    pub(crate) fn confirm_branch_keep(&mut self) {
        let bc = match self.modal.branch_cleanup.take() {
            Some(bc) => bc,
            None => return,
        };
        self.close_pane_final(bc.pane_id);
    }

    /// Cancel the branch cleanup (abort the close entirely).
    pub(crate) fn cancel_branch_cleanup(&mut self) {
        if self.modal.branch_cleanup.is_some() {
            self.modal.branch_cleanup = None;
            self.cache.invalidate_chrome();
        }
    }

    /// Add a pane to the right of the focused pane's tab group.
    /// If a tab group already exists to the right, add there.
    /// Otherwise split horizontally to create a new tab group on the right.
    fn add_pane_to_right(&mut self, focused: tide_core::PaneId, new_id: tide_core::PaneId) {
        if let Some(right_pane) = self.layout.right_neighbor_pane(focused) {
            // Right neighbor exists — add as a tab in that group
            self.layout.add_tab(right_pane, new_id);
        } else {
            // No right neighbor — split the focused pane horizontally
            self.layout.insert_pane(focused, new_id, tide_core::SplitDirection::Horizontal, false);
        }
    }
}

/// Launcher type selection choices.
pub(crate) enum LauncherChoice {
    Terminal,
    NewFile,
    OpenFile,
    Browser,
}
