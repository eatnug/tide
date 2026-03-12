// update() method and file watcher methods extracted from main.rs

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::mpsc;

use notify::{self, Watcher};

use tide_core::TerminalBackend;

use crate::pane::PaneKind;
use crate::search;
use crate::App;

impl App {
    /// Ensure the file watcher is initialized. Returns true if watcher is available.
    pub(crate) fn ensure_file_watcher(&mut self) -> bool {
        if self.file_watcher.is_some() {
            return true;
        }
        let (tx, rx) = mpsc::channel();
        let waker = self.event_loop_waker.clone();
        let dirty_flag = self.file_watch_dirty.clone();
        match notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
            dirty_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            // Wake the event loop so file changes are processed immediately
            if let Some(ref w) = waker {
                w();
            }
        }) {
            Ok(watcher) => {
                self.file_watcher = Some(watcher);
                self.file_watch_rx = Some(rx);
                true
            }
            Err(e) => {
                log::error!("Failed to create file watcher: {}", e);
                false
            }
        }
    }

    /// Start watching a file path for changes.
    pub(crate) fn watch_file(&mut self, path: &std::path::Path) {
        if !self.ensure_file_watcher() {
            return;
        }
        if let Some(watcher) = self.file_watcher.as_mut() {
            if let Err(e) = watcher.watch(path, notify::RecursiveMode::NonRecursive) {
                log::error!("Failed to watch {:?}: {}", path, e);
            }
        }
    }

    /// Stop watching a file path.
    pub(crate) fn unwatch_file(&mut self, path: &std::path::Path) {
        if let Some(watcher) = self.file_watcher.as_mut() {
            let _ = watcher.unwatch(path);
        }
    }

    pub(crate) fn update(&mut self) {
        let mut had_terminal_output = false;

        // Rapid-update detection: when frames are coming faster than 8ms,
        // skip non-critical work (browser sync, file tree, badge updates)
        // to keep drag and resize interactions smooth.
        let now = std::time::Instant::now();
        let is_rapid = now.duration_since(self.last_frame) < std::time::Duration::from_millis(8);

        // Process PTY output for terminal panes only
        for pane in self.panes.values_mut() {
            if let PaneKind::Terminal(terminal) = pane {
                if terminal.cursor_suppress > 0 {
                    terminal.cursor_suppress -= 1;
                    self.cache.needs_redraw = true;
                }
                let old_gen = terminal.backend.grid_generation();
                let t0 = std::time::Instant::now();
                terminal.backend.process();
                let elapsed = t0.elapsed();
                if elapsed.as_micros() > 0 {
                    log::trace!("process: {}us", elapsed.as_micros());
                }
                // Re-execute search when terminal output changes
                if terminal.backend.grid_generation() != old_gen {
                    had_terminal_output = true;
                    if let Some(ref mut s) = terminal.search {
                        if !s.input.is_empty() {
                            search::execute_search_terminal(s, &terminal.backend);
                        }
                    }
                }
            }
        }

        // Keep file tree/CWD in sync with terminal output (works for RedrawRequested path too).
        // Skip during rapid updates — these are non-critical and can run on the next calm frame.
        if had_terminal_output && !is_rapid {
            self.update_file_tree_cwd();
            self.update_terminal_badges();

            if let Some(ref tx) = self.git_poll_cwd_tx {
                let cwds: HashSet<PathBuf> = self
                    .panes
                    .values()
                    .filter_map(|pane| {
                        if let PaneKind::Terminal(p) = pane {
                            p.cwd.clone()
                        } else {
                            None
                        }
                    })
                    .collect();
                let _ = tx.send(cwds.into_iter().collect());
            }
        }

        // Poll file tree events — skip during rapid updates
        if !is_rapid {
            if let Some(tree) = self.ft.tree.as_mut() {
                let had_changes = tree.poll_events();
                if had_changes {
                    // Trigger git poller to refresh status asynchronously
                    // instead of blocking the app-thread with synchronous git calls.
                    self.trigger_git_poll();
                    self.cache.invalidate_chrome();
                } else if tree.has_pending_events() {
                    // Events are pending but deferred by debounce — keep the event
                    // loop alive so they are processed after the debounce window.
                    self.cache.needs_redraw = true;
                }
            }
        }

        // Detect editor is_modified() transitions (catches undo back to clean state).
        // Only re-check when the buffer generation has changed to avoid expensive
        // Vec<String> comparison on every frame.
        {
            let mut modified_changed = false;
            for pane in self.panes.values_mut() {
                if let PaneKind::Editor(ep) = pane {
                    let gen = ep.editor.generation();
                    if gen != ep.last_checked_gen {
                        ep.last_checked_gen = gen;
                        let current = ep.editor.is_modified();
                        if current != ep.last_is_modified {
                            ep.last_is_modified = current;
                            modified_changed = true;
                        }
                    }
                }
            }
            if modified_changed {
                self.cache.invalidate_chrome();
            }
        }

        // Poll editor file watch events — always process regardless of is_rapid.
        // File watcher events are lightweight (one reload per changed file) and
        // losing them causes stale editor content when external tools (e.g. Claude
        // Code) edit files while terminal output is active.
        if let Some(rx) = self.file_watch_rx.as_ref() {
            let mut changed_paths: HashSet<PathBuf> = HashSet::new();
            let mut removed_paths: HashSet<PathBuf> = HashSet::new();
            while let Ok(event_result) = rx.try_recv() {
                if let Ok(event) = event_result {
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            changed_paths.extend(event.paths);
                        }
                        EventKind::Remove(_) => {
                            removed_paths.extend(event.paths);
                        }
                        _ => {}
                    }
                }
            }
            for changed_path in &changed_paths {
                // Find editor panes with this file path
                let matching_ids: Vec<tide_core::PaneId> = self.panes.iter()
                    .filter_map(|(&id, pane)| {
                        if let PaneKind::Editor(editor) = pane {
                            if editor.editor.file_path() == Some(changed_path.as_path()) {
                                return Some(id);
                            }
                        }
                        None
                    })
                    .collect();

                // Check if the file actually exists (macOS FSEvents may report
                // Modify events for deleted files)
                let file_exists = changed_path.exists();

                for id in matching_ids {
                    if let Some(PaneKind::Editor(editor_pane)) = self.panes.get_mut(&id) {
                        if !file_exists {
                            // File doesn't exist — treat as deletion
                            if !editor_pane.editor.is_modified() {
                                // Buffer clean → will be closed below via removed_paths
                                // Add to removed_paths to avoid duplication
                                removed_paths.insert(changed_path.clone());
                            } else {
                                editor_pane.disk_changed = true;
                                editor_pane.file_deleted = true;
                                // Exit diff mode — disk content is stale
                                editor_pane.diff_mode = false;
                                editor_pane.disk_content = None;
                            }
                        } else {
                            // File was recreated or modified
                            editor_pane.file_deleted = false;
                            editor_pane.diff_mode = false;
                            editor_pane.disk_content = None;
                            if !editor_pane.editor.is_modified() {
                                // Buffer clean → auto-reload silently
                                if let Err(e) = editor_pane.editor.reload() {
                                    log::error!("Failed to reload {:?}: {}", changed_path, e);
                                }
                                editor_pane.disk_changed = false;
                            } else {
                                // Buffer dirty → mark disk changed, let user decide
                                editor_pane.disk_changed = true;
                            }
                        }
                        self.cache.invalidate_chrome();
                        self.cache.invalidate_pane(id);
                    }
                }
            }

            // Handle removed files: close clean tabs, mark dirty tabs
            let mut tabs_to_close: Vec<tide_core::PaneId> = Vec::new();
            for removed_path in &removed_paths {
                let matching_ids: Vec<tide_core::PaneId> = self.panes.iter()
                    .filter_map(|(&id, pane)| {
                        if let PaneKind::Editor(editor) = pane {
                            if editor.editor.file_path() == Some(removed_path.as_path()) {
                                return Some(id);
                            }
                        }
                        None
                    })
                    .collect();

                for id in matching_ids {
                    if let Some(PaneKind::Editor(editor_pane)) = self.panes.get_mut(&id) {
                        if !editor_pane.editor.is_modified() {
                            // Buffer clean → close the tab
                            tabs_to_close.push(id);
                        } else {
                            // Buffer dirty → mark as deleted and disk changed
                            editor_pane.disk_changed = true;
                            editor_pane.file_deleted = true;
                            // Exit diff mode — disk content is stale
                            editor_pane.diff_mode = false;
                            editor_pane.disk_content = None;
                            self.cache.invalidate_chrome();
                            self.cache.invalidate_pane(id);
                        }
                    }
                }
            }
            for tab_id in tabs_to_close {
                self.close_editor_panel_tab(tab_id);
            }
        }

        // Clamp file tree scroll to valid range after resize, collapse, or tree changes.
        if self.ft.visible {
            let max = self.file_tree_max_scroll();
            if self.ft.scroll_target > max {
                self.ft.scroll_target = max;
            }
            if self.ft.scroll > max {
                self.ft.scroll = max;
            }
        }

        // Smooth scroll animation
        const SCROLL_LERP: f32 = 0.45;
        const SCROLL_SNAP: f32 = 0.5;

        let ft_diff = self.ft.scroll_target - self.ft.scroll;
        if ft_diff.abs() > SCROLL_SNAP {
            self.ft.scroll += ft_diff * SCROLL_LERP;
            self.cache.invalidate_chrome();
        } else if ft_diff.abs() > 0.0 {
            // Final snap (< 0.5px) — set position but skip chrome rebuild.
            // Next natural chrome rebuild will use the correct final value.
            self.ft.scroll = self.ft.scroll_target;
        }

        // Consume git info from background poller (non-blocking).
        // Skip during rapid updates — badge refresh is cosmetic, not critical.
        if !is_rapid {
            self.update_terminal_badges();
        }

        // Start git poller if not yet running
        if self.git_poll_handle.is_none() {
            self.start_git_poller();
        }

        // Periodically check if terminal child processes are still alive (~2s interval).
        // This detects dead shells in both active and background workspaces.
        if now.duration_since(self.last_child_check) > std::time::Duration::from_secs(2) {
            self.last_child_check = now;
            // Active workspace panes
            let mut newly_dead: Vec<u64> = Vec::new();
            for (&id, pane) in self.panes.iter_mut() {
                if let PaneKind::Terminal(t) = pane {
                    if !t.child_dead && !t.backend.is_child_alive() {
                        t.child_dead = true;
                        newly_dead.push(id);
                    }
                }
            }
            if !newly_dead.is_empty() {
                for id in &newly_dead {
                    self.cache.invalidate_pane(*id);
                }
                self.cache.invalidate_chrome();
            }
            // Background workspace panes
            for ws in &mut self.ws.workspaces {
                for pane in ws.panes.values_mut() {
                    if let PaneKind::Terminal(t) = pane {
                        if !t.child_dead && !t.backend.is_child_alive() {
                            t.child_dead = true;
                        }
                    }
                }
            }
        }
    }
}
