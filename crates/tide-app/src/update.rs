// update() method and file watcher methods extracted from main.rs

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
        let proxy = self.event_loop_proxy.clone();
        let dirty_flag = self.file_watch_dirty.clone();
        match notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
            dirty_flag.store(true, std::sync::atomic::Ordering::Relaxed);
            // Wake the event loop so file changes are processed immediately
            if let Some(ref p) = proxy {
                let _ = p.send_event(());
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
        // Process PTY output for terminal panes only
        for pane in self.panes.values_mut() {
            if let PaneKind::Terminal(terminal) = pane {
                if terminal.cursor_suppress > 0 {
                    terminal.cursor_suppress -= 1;
                    self.needs_redraw = true;
                }
                let old_gen = terminal.backend.grid_generation();
                terminal.backend.process();
                // Re-execute search when terminal output changes
                if terminal.backend.grid_generation() != old_gen {
                    if let Some(ref mut s) = terminal.search {
                        if !s.query.is_empty() {
                            search::execute_search_terminal(s, &terminal.backend);
                        }
                    }
                }
            }
        }

        // Poll file tree events
        if let Some(tree) = self.file_tree.as_mut() {
            let had_changes = tree.poll_events();
            if had_changes {
                self.chrome_generation += 1;
            }
        }

        // Poll editor file watch events
        if let Some(rx) = self.file_watch_rx.as_ref() {
            let mut changed_paths: Vec<PathBuf> = Vec::new();
            let mut removed_paths: Vec<PathBuf> = Vec::new();
            while let Ok(event_result) = rx.try_recv() {
                if let Ok(event) = event_result {
                    use notify::EventKind;
                    match event.kind {
                        EventKind::Modify(_) | EventKind::Create(_) => {
                            for path in event.paths {
                                if !changed_paths.contains(&path) {
                                    changed_paths.push(path);
                                }
                            }
                        }
                        EventKind::Remove(_) => {
                            for path in event.paths {
                                if !removed_paths.contains(&path) {
                                    removed_paths.push(path);
                                }
                            }
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
                                if !removed_paths.contains(changed_path) {
                                    removed_paths.push(changed_path.clone());
                                }
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
                        self.chrome_generation += 1;
                        self.pane_generations.remove(&id);
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
                            self.chrome_generation += 1;
                            self.pane_generations.remove(&id);
                        }
                    }
                }
            }
            for tab_id in tabs_to_close {
                self.close_editor_panel_tab(tab_id);
            }
        }

        // Smooth scroll animation
        const SCROLL_LERP: f32 = 0.45;
        const SCROLL_SNAP: f32 = 0.5;

        let ft_diff = self.file_tree_scroll_target - self.file_tree_scroll;
        if ft_diff.abs() > SCROLL_SNAP {
            self.file_tree_scroll += ft_diff * SCROLL_LERP;
            self.chrome_generation += 1;
            self.needs_redraw = true;
        } else if ft_diff.abs() > 0.0 {
            // Final snap (< 0.5px) — set position but skip chrome rebuild.
            // Next natural chrome rebuild will use the correct final value.
            self.file_tree_scroll = self.file_tree_scroll_target;
        }

        let pt_diff = self.panel_tab_scroll_target - self.panel_tab_scroll;
        if pt_diff.abs() > SCROLL_SNAP {
            self.panel_tab_scroll += pt_diff * SCROLL_LERP;
            self.chrome_generation += 1;
            self.needs_redraw = true;
        } else if pt_diff.abs() > 0.0 {
            self.panel_tab_scroll = self.panel_tab_scroll_target;
        }

        // Consume git info from background poller (non-blocking).
        // CWD/idle badge checks are event-driven (see badge_check_at in about_to_wait).
        // Git info still needs periodic consumption since it arrives asynchronously.
        self.update_terminal_badges();

        // Start git poller if not yet running
        if self.git_poll_handle.is_none() {
            self.start_git_poller();
        }
    }
}
