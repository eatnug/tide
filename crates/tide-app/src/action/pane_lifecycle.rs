use std::path::PathBuf;

use tide_core::{LayoutEngine, Renderer};

use crate::browser_pane::BrowserPane;
use crate::editor_pane::EditorPane;
use crate::pane::{PaneKind, TerminalPane};
use crate::{App, PaneAreaMode};

impl App {
    pub(crate) fn create_terminal_pane(&mut self, id: tide_core::PaneId, cwd: Option<std::path::PathBuf>) {
        let cell_size = match self.renderer.as_ref() {
            Some(r) => r.cell_size(),
            None => {
                log::error!("create_terminal_pane called before renderer initialized");
                return;
            }
        };
        let logical = self.logical_size();
        let cols = (logical.width / 2.0 / cell_size.width).max(1.0) as u16;
        let rows = (logical.height / cell_size.height).max(1.0) as u16;

        match TerminalPane::with_cwd(id, cols, rows, cwd) {
            Ok(pane) => {
                self.install_pty_waker(&pane);
                self.panes.insert(id, PaneKind::Terminal(pane));
                self.pending_ime_proxy_creates.push(id);
            }
            Err(e) => {
                log::error!("Failed to create terminal pane: {}", e);
            }
        }
    }

    /// Get the CWD of the currently focused terminal pane, if any.
    /// When an editor/diff pane is focused, resolves to its owning terminal's CWD.
    pub(super) fn focused_terminal_cwd(&self) -> Option<std::path::PathBuf> {
        let tid = self.focused_terminal_id()?;
        match self.panes.get(&tid) {
            Some(PaneKind::Terminal(p)) => p.backend.detect_cwd_fallback(),
            _ => None,
        }
    }

    /// Create a new empty editor pane in the panel.
    /// Auto-shows the editor panel if it was hidden.
    pub(crate) fn new_editor_pane(&mut self) {
        if !self.show_editor_panel {
            self.show_editor_panel = true;
            self.editor_panel_auto_shown = true;
        }
        let tid = self.focused_terminal_id();
        let panel_was_visible = !self.active_editor_tabs().is_empty();
        let new_id = self.layout.alloc_id();
        let pane = EditorPane::new_empty(new_id);
        self.panes.insert(new_id, PaneKind::Editor(pane));
        self.pending_ime_proxy_creates.push(new_id);
        if let Some(tid) = tid {
            if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                tp.editors.push(new_id);
                tp.active_editor = Some(new_id);
            }
        }
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.focus_area = crate::ui_state::FocusArea::EditorDock;
        self.chrome_generation += 1;
        if !panel_was_visible {
            if !self.editor_panel_width_manual {
                self.editor_panel_width = self.auto_editor_panel_width();
            }
            self.compute_layout();
        }
        self.scroll_to_active_panel_tab();
    }

    /// Open a browser pane in the editor dock panel.
    /// If `url` is Some, navigates to it immediately.
    pub(crate) fn open_browser_pane(&mut self, url: Option<String>) {
        if !self.show_editor_panel {
            self.show_editor_panel = true;
            self.editor_panel_auto_shown = true;
        }
        let tid = self.focused_terminal_id();
        let panel_was_visible = !self.active_editor_tabs().is_empty();
        let new_id = self.layout.alloc_id();
        let pane = match url {
            Some(ref u) => BrowserPane::with_url(new_id, u.clone()),
            None => BrowserPane::new(new_id),
        };
        self.panes.insert(new_id, PaneKind::Browser(pane));
        // Browser panes need an IME proxy for URL bar keyboard input
        self.pending_ime_proxy_creates.push(new_id);
        if let Some(tid) = tid {
            if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                tp.editors.push(new_id);
                tp.active_editor = Some(new_id);
            }
        }
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.focus_area = crate::ui_state::FocusArea::EditorDock;
        self.chrome_generation += 1;
        if !panel_was_visible && !self.editor_panel_width_manual {
            self.editor_panel_width = self.auto_editor_panel_width();
        }
        // Always recompute layout so the WKWebView is created and positioned
        self.compute_layout();
        self.scroll_to_active_panel_tab();
    }

    /// Open a file in the editor panel. If already open, activate its tab.
    /// Auto-shows the editor panel if it was hidden.
    pub(crate) fn open_editor_pane(&mut self, path: PathBuf) {
        let tid = self.focused_terminal_id();
        // Track whether panel needs layout recompute (becoming visible)
        let needs_layout = !self.show_editor_panel
            || (self.show_editor_panel && self.active_editor_tabs().is_empty());

        // Auto-show editor panel if hidden
        if !self.show_editor_panel {
            self.show_editor_panel = true;
            self.editor_panel_auto_shown = true;
        }
        // Check if already open in this terminal's dock tabs -> activate & focus
        let tabs: Vec<tide_core::PaneId> = self.active_editor_tabs().to_vec();
        for &tab_id in &tabs {
            if let Some(PaneKind::Editor(editor)) = self.panes.get(&tab_id) {
                if editor.editor.file_path() == Some(path.as_path()) {
                    if let Some(tid) = tid {
                        if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                            tp.active_editor = Some(tab_id);
                        }
                    }
                    self.pane_generations.remove(&tab_id);
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.focus_area = crate::ui_state::FocusArea::EditorDock;
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

        // Check if already open in another terminal's dock or split tree -> focus
        for (&id, pane) in &self.panes {
            if let PaneKind::Editor(editor) = pane {
                if editor.editor.file_path() == Some(path.as_path()) {
                    self.focused = Some(id);
                    self.router.set_focused(id);
                    self.focus_area = crate::ui_state::FocusArea::EditorDock;
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
                self.pending_ime_proxy_creates.push(new_id);
                if let Some(tid) = tid {
                    if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                        tp.editors.push(new_id);
                        tp.active_editor = Some(new_id);
                    }
                }
                self.focused = Some(new_id);
                self.router.set_focused(new_id);
                self.focus_area = crate::ui_state::FocusArea::EditorDock;
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

    /// Close an editor panel tab. If dirty (and has a file path), show save confirm bar instead.
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
                self.save_confirm = Some(crate::SaveConfirmState { pane_id: tab_id });
                // Ensure this tab is active and focused so the bar is visible
                if let Some(owner_tid) = self.terminal_owning(tab_id) {
                    if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&owner_tid) {
                        tp.active_editor = Some(tab_id);
                    }
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
        // Destroy webview before removing the pane
        if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(&tab_id) {
            bp.destroy();
        }
        // Cancel save-as if the target pane is being closed
        if self.save_as_input.as_ref().is_some_and(|s| s.pane_id == tab_id) {
            self.save_as_input = None;
        }
        // Cancel save confirm if the target pane is being closed
        if self.save_confirm.as_ref().is_some_and(|s| s.pane_id == tab_id) {
            self.save_confirm = None;
        }
        // Save the file's parent dir before removing (for focus matching)
        let closed_file_dir = if let Some(PaneKind::Editor(editor)) = self.panes.get(&tab_id) {
            editor.editor.file_path().and_then(|p| p.parent().map(|d| d.to_path_buf()))
        } else {
            None
        };
        // Unwatch the file before removing the pane
        let watch_path = if let Some(PaneKind::Editor(editor)) = self.panes.get(&tab_id) {
            editor.editor.file_path().map(|p| p.to_path_buf())
        } else {
            None
        };
        if let Some(path) = watch_path {
            self.unwatch_file(&path);
        }
        // Find and update the owning terminal
        let owner_tid = self.terminal_owning(tab_id);
        if let Some(tid) = owner_tid {
            if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                tp.editors.retain(|&id| id != tab_id);
                if tp.active_editor == Some(tab_id) {
                    tp.active_editor = tp.editors.last().copied();
                }
            }
        }
        self.panes.remove(&tab_id);
        self.cleanup_closed_pane_state(tab_id);

        // Check if this terminal now has no editors
        let owner_editors_empty = owner_tid
            .and_then(|tid| self.panes.get(&tid))
            .map(|pk| if let PaneKind::Terminal(tp) = pk { tp.editors.is_empty() } else { true })
            .unwrap_or(true);
        if owner_editors_empty && self.active_editor_tabs().is_empty() {
            self.show_editor_panel = false;
            self.editor_panel_maximized = false;
            self.editor_panel_width_manual = false;
        }

        // If focused pane was the closed tab, switch focus
        if self.focused == Some(tab_id) {
            let new_active = owner_tid
                .and_then(|tid| self.panes.get(&tid))
                .and_then(|pk| if let PaneKind::Terminal(tp) = pk { tp.active_editor } else { None });
            if let Some(active) = new_active {
                self.focused = Some(active);
                self.router.set_focused(active);
            } else {
                // No panel tabs left: find the terminal pane whose CWD best
                // matches the directory of the closed file.
                let best = closed_file_dir.as_ref().and_then(|file_dir| {
                    self.layout.pane_ids().into_iter()
                        .filter_map(|id| {
                            if let Some(PaneKind::Terminal(p)) = self.panes.get(&id) {
                                p.cwd.as_ref().map(|cwd| (id, cwd.clone()))
                            } else {
                                None
                            }
                        })
                        .filter(|(_, cwd)| file_dir.starts_with(cwd))
                        .max_by_key(|(_, cwd)| cwd.components().count())
                        .map(|(id, _)| id)
                });
                let target = best
                    .or_else(|| owner_tid)
                    .or_else(|| self.layout.pane_ids().first().copied());
                if let Some(id) = target {
                    self.focused = Some(id);
                    self.router.set_focused(id);
                } else {
                    self.focused = None;
                }
                // No editor tabs remain — return focus to the terminal pane area
                // so keyboard input is routed correctly instead of being lost in
                // the now-empty EditorDock.
                if self.focus_area == crate::ui_state::FocusArea::EditorDock {
                    self.focus_area = crate::ui_state::FocusArea::PaneArea;
                }
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
        if self.is_dock_editor(pane_id) {
            self.close_editor_panel_tab(pane_id);
            self.update_file_tree_cwd();
            return;
        }

        // If the pane is a terminal, check its owned editors for dirty state
        if matches!(self.panes.get(&pane_id), Some(PaneKind::Terminal(_))) {
            let first_dirty = if let Some(PaneKind::Terminal(tp)) = self.panes.get(&pane_id) {
                tp.editors.iter().find(|&&eid| {
                    matches!(self.panes.get(&eid), Some(PaneKind::Editor(ep)) if ep.editor.is_modified())
                }).copied()
            } else {
                None
            };

            if let Some(dirty_eid) = first_dirty {
                // Show save confirm for the first dirty editor
                self.save_confirm = Some(crate::SaveConfirmState { pane_id: dirty_eid });
                if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&pane_id) {
                    tp.active_editor = Some(dirty_eid);
                }
                self.focused = Some(dirty_eid);
                self.router.set_focused(dirty_eid);
                self.chrome_generation += 1;
                self.pane_generations.remove(&dirty_eid);
                self.pending_terminal_close = Some(pane_id);
                if !self.show_editor_panel {
                    self.show_editor_panel = true;
                    self.compute_layout();
                }
                return;
            }

            // All editors are clean → force close them all before closing the terminal
            let editor_ids: Vec<tide_core::PaneId> = if let Some(PaneKind::Terminal(tp)) = self.panes.get(&pane_id) {
                tp.editors.clone()
            } else {
                Vec::new()
            };
            for eid in &editor_ids {
                if let Some(PaneKind::Editor(editor)) = self.panes.get(eid) {
                    if let Some(path) = editor.editor.file_path().map(|p| p.to_path_buf()) {
                        self.unwatch_file(&path);
                    }
                }
                // Destroy webview before removing the pane from the map
                if let Some(PaneKind::Browser(bp)) = self.panes.get_mut(eid) {
                    bp.destroy();
                }
                self.panes.remove(eid);
                self.cleanup_closed_pane_state(*eid);
            }
            if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&pane_id) {
                tp.editors.clear();
                tp.active_editor = None;
            }
            // Fall through to force_close_specific_pane
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
    /// May show branch cleanup confirmation for terminals on non-main branches.
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
        if self.is_dock_editor(pane_id) {
            self.force_close_editor_panel_tab(pane_id);
            self.update_file_tree_cwd();
            return;
        }

        // If branch cleanup bar is already showing for this pane, block the close —
        // the user must resolve it via Delete/Keep/Cancel first.
        if self.branch_cleanup.as_ref().is_some_and(|bc| bc.pane_id == pane_id) {
            return;
        }

        // Branch cleanup check: if this is a terminal on a non-main branch,
        // prompt before closing (unless cleanup is already active for another pane).
        if self.branch_cleanup.is_none() {
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

                            self.branch_cleanup = Some(crate::BranchCleanupState {
                                pane_id,
                                branch: branch.clone(),
                                worktree_path: wt_path,
                                cwd: cwd.clone(),
                            });
                            self.chrome_generation += 1;
                            self.needs_redraw = true;
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
        // Handle stacked mode: advance to next tab or fall back to Split
        if let PaneAreaMode::Stacked(active) = self.pane_area_mode {
            if active == pane_id {
                let pane_ids = self.layout.pane_ids();
                let pos = pane_ids.iter().position(|&id| id == pane_id);
                // Try to advance to an adjacent pane
                let next = pos.and_then(|p| {
                    if p + 1 < pane_ids.len() {
                        Some(pane_ids[p + 1])
                    } else if p > 0 {
                        Some(pane_ids[p - 1])
                    } else {
                        None
                    }
                });
                if let Some(next_id) = next {
                    self.pane_area_mode = PaneAreaMode::Stacked(next_id);
                } else {
                    // Last pane — exit Stacked mode
                    self.pane_area_mode = PaneAreaMode::Split;
                }
            }
        }

        let remaining = self.layout.pane_ids();
        let has_any_dock_editors = self.panes.values().any(|pk| {
            if let PaneKind::Terminal(tp) = pk { !tp.editors.is_empty() } else { false }
        });
        if remaining.len() <= 1 && !has_any_dock_editors {
            let session = crate::session::Session::from_app(self);
            crate::session::save_session(&session);
            std::process::exit(0);
        }
        if remaining.len() <= 1 {
            // Last tree pane but panel has tabs -- focus panel instead
            if let Some(active) = self.active_editor_tab() {
                self.focused = Some(active);
                self.router.set_focused(active);
                self.chrome_generation += 1;
            }
            return;
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

        let old_tid = self.focused_terminal_id();
        self.layout.remove(pane_id);
        self.panes.remove(&pane_id);
        self.cleanup_closed_pane_state(pane_id);

        if let Some(next) = next_focus {
            self.focused = Some(next);
            self.router.set_focused(next);
        } else {
            self.focused = None;
        }

        self.chrome_generation += 1;
        self.compute_layout();
        self.update_file_tree_cwd();
        // Reset panel tab scroll when terminal context changed
        if self.focused_terminal_id() != old_tid {
            self.panel_tab_scroll = 0.0;
            self.panel_tab_scroll_target = 0.0;
        }
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
                let base_dir = self.resolve_base_dir();
                let anchor = self.active_panel_tab_rect()
                    .unwrap_or_else(|| tide_core::Rect::new(0.0, 0.0, 0.0, 0.0));
                self.save_as_input = Some(crate::SaveAsInput::new(pane_id, base_dir, anchor));
                return;
            }
            if let Err(e) = pane.editor.buffer.save() {
                log::error!("Save failed: {}", e);
                return;
            }
            pane.disk_changed = false;
        }
        // Close
        if self.is_dock_editor(pane_id) {
            self.force_close_editor_panel_tab(pane_id);
        } else {
            self.force_close_specific_pane(pane_id);
        }
        // Retry pending terminal close (may find more dirty editors)
        if let Some(tid) = self.pending_terminal_close.take() {
            if self.panes.contains_key(&tid) {
                self.close_specific_pane(tid);
            }
        }
    }

    /// Discard changes and close the pane from the save confirm bar.
    pub(crate) fn confirm_discard_and_close(&mut self) {
        let pane_id = match self.save_confirm.take() {
            Some(sc) => sc.pane_id,
            None => return,
        };
        if self.is_dock_editor(pane_id) {
            self.force_close_editor_panel_tab(pane_id);
        } else {
            self.force_close_specific_pane(pane_id);
        }
        // Retry pending terminal close (may find more dirty editors)
        if let Some(tid) = self.pending_terminal_close.take() {
            if self.panes.contains_key(&tid) {
                self.close_specific_pane(tid);
            }
        }
    }

    /// Cancel the save confirm bar.
    pub(crate) fn cancel_save_confirm(&mut self) {
        if self.save_confirm.is_some() {
            self.save_confirm = None;
            self.pending_terminal_close = None;
            self.chrome_generation += 1;
            self.pane_generations.clear();
        }
    }

    /// Delete the branch/worktree and proceed with closing the terminal pane.
    pub(crate) fn confirm_branch_delete(&mut self) {
        let bc = match self.branch_cleanup.take() {
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
        let bc = match self.branch_cleanup.take() {
            Some(bc) => bc,
            None => return,
        };
        self.close_pane_final(bc.pane_id);
    }

    /// Cancel the branch cleanup (abort the close entirely).
    pub(crate) fn cancel_branch_cleanup(&mut self) {
        if self.branch_cleanup.is_some() {
            self.branch_cleanup = None;
            self.chrome_generation += 1;
            self.needs_redraw = true;
        }
    }
}
