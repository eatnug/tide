use std::collections::HashMap;
use std::path::PathBuf;

use tide_core::{FileGitStatus, FileTreeSource, Renderer, Vec2};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;

impl App {
    pub(crate) fn update_file_tree_cwd(&mut self) {
        if !self.show_file_tree {
            return;
        }

        let cwd = self.focused.and_then(|id| {
            match self.panes.get(&id) {
                Some(PaneKind::Terminal(p)) => p.backend.detect_cwd_fallback(),
                _ => None,
            }
        });

        if let Some(cwd) = cwd {
            if self.last_cwd.as_ref() != Some(&cwd) {
                self.last_cwd = Some(cwd.clone());
                if let Some(tree) = self.file_tree.as_mut() {
                    tree.set_root(cwd);
                }
                self.file_tree_scroll = 0.0;
                self.file_tree_scroll_target = 0.0;
                self.refresh_file_tree_git_status();
                self.chrome_generation += 1;
            }
        }
    }

    /// Refresh git status for all entries in the file tree.
    /// Synchronous call — fast on small repos (~5ms for `git status --porcelain`).
    pub(crate) fn refresh_file_tree_git_status(&mut self) {
        let tree_root = match self.file_tree.as_ref() {
            Some(tree) => tree.root().to_path_buf(),
            None => return,
        };

        let git_root = match tide_terminal::git::repo_root(&tree_root) {
            Some(root) => root,
            None => {
                self.file_tree_git_status.clear();
                self.file_tree_git_root = None;
                return;
            }
        };

        let entries = tide_terminal::git::status_files(&tree_root);
        let mut status_map: HashMap<PathBuf, FileGitStatus> = HashMap::new();

        for entry in entries {
            let git_status = parse_git_status_code(&entry.status);
            if let Some(gs) = git_status {
                // status_files returns paths relative to the repo root
                let abs_path = git_root.join(&entry.path);
                status_map.insert(abs_path, gs);
            }
        }

        self.file_tree_git_status = status_map;
        self.file_tree_git_root = Some(git_root);
    }

    pub(crate) fn file_tree_max_scroll(&self) -> f32 {
        let entry_count = self
            .file_tree
            .as_ref()
            .map(|t| t.visible_entries().len())
            .unwrap_or(0);
        let cell_size = match self.renderer.as_ref() {
            Some(r) => r.cell_size(),
            None => return 0.0,
        };
        let logical = self.logical_size();
        let tree_height = logical.height - self.top_inset;
        let content_height = PANE_PADDING + entry_count as f32 * cell_size.height * FILE_TREE_LINE_SPACING;
        (content_height - tree_height).max(0.0)
    }

    /// Poll CWD and shell idle state for all terminal panes (cheap, no subprocess).
    /// Also consumes git info results from the background poller thread.
    /// Bumps chrome_generation if anything changed.
    pub(crate) fn update_terminal_badges(&mut self) {
        let mut changed = false;
        let pane_ids: Vec<tide_core::PaneId> = self.panes.keys().copied().collect();

        for id in &pane_ids {
            if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(id) {
                // CWD (reads /proc or sysctl — no subprocess)
                let new_cwd = pane.backend.detect_cwd_fallback();
                if new_cwd != pane.cwd {
                    pane.cwd = new_cwd;
                    changed = true;
                }

                // Shell idle
                let new_idle = pane.backend.is_shell_idle();
                if new_idle != pane.shell_idle {
                    pane.shell_idle = new_idle;
                    changed = true;
                }
            }
        }

        if self.consume_git_poll_results() || changed {
            self.chrome_generation += 1;
            self.needs_redraw = true;
        }
    }

    /// Consume git info results from the background poller (non-blocking).
    /// Returns true if any pane's git info actually changed.
    /// Called from about_to_wait() when git poller wakes the event loop,
    /// and from update_terminal_badges() during normal frame rendering.
    pub(crate) fn consume_git_poll_results(&mut self) -> bool {
        let rx = match self.git_poll_rx {
            Some(ref rx) => rx,
            None => return false,
        };
        let mut latest = None;
        while let Ok(result) = rx.try_recv() {
            latest = Some(result);
        }
        let git_results = match latest {
            Some(r) => r,
            None => return false,
        };

        let mut changed = false;
        let pane_ids: Vec<tide_core::PaneId> = self.panes.keys().copied().collect();
        for id in &pane_ids {
            if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(id) {
                if let Some(ref cwd) = pane.cwd {
                    if let Some((new_git, wt_count)) = git_results.get(cwd) {
                        let git_changed = match (&pane.git_info, new_git) {
                            (None, None) => false,
                            (Some(_), None) | (None, Some(_)) => true,
                            (Some(old), Some(new)) => {
                                old.branch != new.branch
                                    || old.status.changed_files != new.status.changed_files
                                    || old.status.additions != new.status.additions
                                    || old.status.deletions != new.status.deletions
                            }
                        };
                        if git_changed {
                            pane.git_info = new_git.clone();
                            changed = true;
                        }
                        if pane.worktree_count != *wt_count {
                            pane.worktree_count = *wt_count;
                            changed = true;
                        }
                    }
                }
            }
        }
        changed
    }

    /// Start the background git info poller thread.
    /// Collects unique CWDs from terminal panes and queries git info off the main thread.
    pub(crate) fn start_git_poller(&mut self) {
        if self.git_poll_handle.is_some() {
            return;
        }

        let (tx, rx) = std::sync::mpsc::channel();
        self.git_poll_rx = Some(rx);

        let stop_flag = self.git_poll_stop.clone();
        let proxy = self.event_loop_proxy.clone();

        // We need to send CWD list to the thread. We'll use a shared list.
        // The thread will re-read pane CWDs via a shared channel.
        // Simpler approach: thread polls at fixed interval, main thread sends CWD list.
        let (cwd_tx, cwd_rx) = std::sync::mpsc::channel::<Vec<std::path::PathBuf>>();
        // Store cwd_tx for the main thread to send CWD updates
        // We'll repurpose the periodic check to send CWDs

        let handle = std::thread::spawn(move || {
            while !stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                // Wait for CWD list from main thread (with timeout)
                let cwds = match cwd_rx.recv_timeout(std::time::Duration::from_secs(2)) {
                    Ok(cwds) => cwds,
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                };

                // Query git info for each unique CWD
                let mut results = std::collections::HashMap::new();
                for cwd in cwds {
                    if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let info = tide_terminal::git::detect_git_info(&cwd);
                    let wt_count = tide_terminal::git::count_worktrees(&cwd);
                    results.insert(cwd, (info, wt_count));
                }

                let _ = tx.send(results);
                if let Some(ref p) = proxy {
                    let _ = p.send_event(());
                }
            }
        });

        self.git_poll_handle = Some(handle);
        // Store cwd_tx — we need it accessible. Add a field.
        self.git_poll_cwd_tx = Some(cwd_tx);
    }

    /// Execute a context menu action (Delete or Rename).
    pub(crate) fn execute_context_menu_action(&mut self, action_index: usize) {
        let menu = match self.context_menu.take() {
            Some(m) => m,
            None => return,
        };

        let action = match crate::ContextMenuAction::ALL.get(action_index) {
            Some(a) => *a,
            None => return,
        };

        match action {
            crate::ContextMenuAction::Delete => {
                let result = if menu.is_dir {
                    std::fs::remove_dir_all(&menu.path)
                } else {
                    std::fs::remove_file(&menu.path)
                };
                if let Err(e) = result {
                    log::error!("Failed to delete {:?}: {}", menu.path, e);
                }
                if let Some(tree) = self.file_tree.as_mut() {
                    tree.refresh();
                }
                self.refresh_file_tree_git_status();
                self.chrome_generation += 1;
            }
            crate::ContextMenuAction::Rename => {
                let file_name = menu.path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();
                self.file_tree_rename = Some(crate::FileTreeRenameState {
                    entry_index: menu.entry_index,
                    original_path: menu.path,
                    input: crate::InputLine::with_text(file_name),
                });
                self.chrome_generation += 1;
            }
        }
        self.needs_redraw = true;
    }

    /// Complete an inline file tree rename: move the file, refresh the tree.
    pub(crate) fn complete_file_tree_rename(&mut self) {
        let rename = match self.file_tree_rename.take() {
            Some(r) => r,
            None => return,
        };

        let new_name = rename.input.text.trim().to_string();
        if new_name.is_empty() || new_name == rename.original_path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default() {
            // No change or empty — cancel
            self.chrome_generation += 1;
            return;
        }

        let new_path = rename.original_path.parent()
            .map(|p| p.join(&new_name))
            .unwrap_or_else(|| PathBuf::from(&new_name));

        if let Err(e) = std::fs::rename(&rename.original_path, &new_path) {
            log::error!("Failed to rename {:?} → {:?}: {}", rename.original_path, new_path, e);
        }
        if let Some(tree) = self.file_tree.as_mut() {
            tree.refresh();
        }
        self.refresh_file_tree_git_status();
        self.chrome_generation += 1;
        self.needs_redraw = true;
    }

    pub(crate) fn handle_file_tree_click(&mut self, position: Vec2) {
        // Dismiss context menu and complete/cancel rename on any left click
        self.context_menu = None;
        if self.file_tree_rename.is_some() {
            self.complete_file_tree_rename();
        }

        if !self.show_file_tree {
            return;
        }
        let ft_rect = match self.file_tree_rect {
            Some(r) => r,
            None => return,
        };
        if position.x < ft_rect.x || position.x >= ft_rect.x + ft_rect.width {
            return;
        }

        let cell_size = match self.renderer.as_ref() {
            Some(r) => r.cell_size(),
            None => return,
        };

        let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
        // Account for padding offset (no gap — tree is flush with window edge)
        let ft_y = self.file_tree_rect.map(|r| r.y).unwrap_or(self.top_inset);
        let adjusted_y = position.y - ft_y - PANE_PADDING;
        let index = ((adjusted_y + self.file_tree_scroll) / line_height) as usize;

        // Extract click info from file tree (borrow released before open_editor_pane)
        let click_result = if let Some(tree) = self.file_tree.as_mut() {
            let entries = tree.visible_entries();
            if index < entries.len() {
                let entry = entries[index].clone();
                if entry.entry.is_dir {
                    tree.toggle(&entry.entry.path);
                    self.chrome_generation += 1;
                    None
                } else {
                    Some(entry.entry.path.clone())
                }
            } else {
                None
            }
        } else {
            None
        };

        if let Some(path) = click_result {
            self.open_editor_pane(path);
        }
    }
}

/// Parse a 2-char git porcelain status code into a FileGitStatus.
fn parse_git_status_code(code: &str) -> Option<FileGitStatus> {
    let bytes = code.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let x = bytes[0]; // index (staging area)
    let y = bytes[1]; // working tree

    // Conflict states: both modified, or various add/delete combos
    if (x == b'U' || y == b'U')
        || (x == b'A' && y == b'A')
        || (x == b'D' && y == b'D')
    {
        return Some(FileGitStatus::Conflict);
    }

    // Untracked
    if x == b'?' && y == b'?' {
        return Some(FileGitStatus::Untracked);
    }

    // Added (new file in index)
    if x == b'A' {
        return Some(FileGitStatus::Added);
    }

    // Deleted
    if x == b'D' || y == b'D' {
        return Some(FileGitStatus::Deleted);
    }

    // Modified (either in index or working tree)
    if x == b'M' || y == b'M' || x == b'R' || x == b'C' {
        return Some(FileGitStatus::Modified);
    }

    None
}
