use std::collections::HashMap;
use std::path::PathBuf;

use tide_core::{FileGitStatus, FileTreeSource, TerminalBackend, Vec2};

use crate::pane::PaneKind;
use crate::theme::*;
use crate::App;

/// Results from the background git poller (one entry per CWD).
pub(crate) type GitPollResults = HashMap<PathBuf, GitPollCwdResult>;

pub(crate) struct GitPollCwdResult {
    pub git_info: Option<tide_terminal::git::GitInfo>,
    pub worktree_count: usize,
    pub repo_root: Option<PathBuf>,
    pub status_entries: Vec<tide_terminal::git::StatusEntry>,
}

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
                // Use git root as tree root when inside a repo (sticky);
                // otherwise follow CWD directly.
                // Use cached repo_root from the git poller — never call git synchronously.
                let tree_root = match self.cached_repo_roots.get(&cwd) {
                    Some(Some(root)) => root.clone(),
                    Some(None) => cwd, // not in a git repo
                    None => cwd,       // not cached yet — will update when poller finishes
                };
                let current_root = self.file_tree.as_ref().map(|t| t.root().to_path_buf());
                if current_root.as_ref() != Some(&tree_root) {
                    if let Some(tree) = self.file_tree.as_mut() {
                        tree.set_root(tree_root);
                    }
                    self.file_tree_scroll = 0.0;
                    self.file_tree_scroll_target = 0.0;
                    self.chrome_generation += 1;
                    // File tree git status will be updated when git poller results arrive.
                }
            }
        }
    }

    /// Apply pre-computed git status entries to the file tree.
    /// Called from consume_git_poll_results with data from the background thread.
    fn apply_file_tree_git_status(
        &mut self,
        git_root: &PathBuf,
        entries: &[tide_terminal::git::StatusEntry],
    ) {
        let tree_root = match self.file_tree.as_ref() {
            Some(tree) => tree.root().to_path_buf(),
            None => return,
        };

        let mut status_map: HashMap<PathBuf, FileGitStatus> = HashMap::new();

        for entry in entries {
            let git_status = parse_git_status_code(&entry.status);
            if let Some(gs) = git_status {
                let rel = if entry.path.ends_with('/') {
                    &entry.path[..entry.path.len() - 1]
                } else {
                    &entry.path
                };
                let abs_path = git_root.join(rel);
                status_map.insert(abs_path, gs);
            }
        }

        let mut dir_status: HashMap<PathBuf, FileGitStatus> = HashMap::new();
        for (path, &status) in &status_map {
            if path.is_dir() {
                let entry = dir_status.entry(path.clone()).or_insert(status);
                *entry = merge_git_status(*entry, status);
            }
            let mut ancestor = path.parent();
            while let Some(dir) = ancestor {
                if dir < tree_root {
                    break;
                }
                let entry = dir_status.entry(dir.to_path_buf()).or_insert(status);
                *entry = merge_git_status(*entry, status);
                if dir == tree_root {
                    break;
                }
                ancestor = dir.parent();
            }
        }

        self.file_tree_git_status = status_map;
        self.file_tree_dir_git_status = dir_status;
        self.file_tree_git_root = Some(git_root.clone());
    }

    /// Trigger the git poller to re-run for the current CWDs.
    /// Used when file tree filesystem events require a git status refresh.
    pub(crate) fn trigger_git_poll(&self) {
        if let Some(ref tx) = self.git_poll_cwd_tx {
            let cwds: std::collections::HashSet<PathBuf> = self
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

    pub(crate) fn file_tree_max_scroll(&self) -> f32 {
        let entry_count = self
            .file_tree
            .as_ref()
            .map(|t| t.visible_entries().len())
            .unwrap_or(0);
        let cell_size = self.cell_size();
        let logical = self.logical_size();
        let tree_height = logical.height - self.top_inset - FILE_TREE_HEADER_HEIGHT;
        let content_height = entry_count as f32 * cell_size.height * FILE_TREE_LINE_SPACING;
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

        // Update cached repo roots and pane git info
        let pane_ids: Vec<tide_core::PaneId> = self.panes.keys().copied().collect();
        for id in &pane_ids {
            if let Some(PaneKind::Terminal(pane)) = self.panes.get_mut(id) {
                if let Some(ref cwd) = pane.cwd {
                    if let Some(result) = git_results.get(cwd) {
                        let git_changed = match (&pane.git_info, &result.git_info) {
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
                            pane.git_info = result.git_info.clone();
                            changed = true;
                        }
                        if pane.worktree_count != result.worktree_count {
                            pane.worktree_count = result.worktree_count;
                            changed = true;
                        }
                    }
                }
            }
        }

        // Update cached repo roots (for update_file_tree_cwd)
        for (cwd, result) in &git_results {
            self.cached_repo_roots.insert(cwd.clone(), result.repo_root.clone());
        }

        // Update file tree git status from poller results
        if let Some(tree) = self.file_tree.as_ref() {
            let tree_root = tree.root().to_path_buf();
            // Find the result whose repo_root matches the current tree root
            for result in git_results.values() {
                if result.repo_root.as_ref() == Some(&tree_root) {
                    self.apply_file_tree_git_status(&tree_root, &result.status_entries);
                    changed = true;
                    break;
                }
            }
        }

        // Trigger file tree CWD update with newly cached repo roots
        if self.show_file_tree {
            let cwd = self.focused.and_then(|id| {
                match self.panes.get(&id) {
                    Some(PaneKind::Terminal(p)) => p.cwd.clone(),
                    _ => None,
                }
            });
            if let Some(cwd) = cwd {
                if let Some(Some(root)) = self.cached_repo_roots.get(&cwd) {
                    let current_root = self.file_tree.as_ref().map(|t| t.root().to_path_buf());
                    if current_root.as_ref() != Some(root) {
                        let root = root.clone();
                        if let Some(tree) = self.file_tree.as_mut() {
                            tree.set_root(root);
                        }
                        self.file_tree_scroll = 0.0;
                        self.file_tree_scroll_target = 0.0;
                        changed = true;
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
        let waker = self.event_loop_waker.clone();

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
                let mut results: GitPollResults = std::collections::HashMap::new();
                for cwd in cwds {
                    if stop_flag.load(std::sync::atomic::Ordering::Relaxed) {
                        break;
                    }
                    let git_info = tide_terminal::git::detect_git_info(&cwd);
                    let worktree_count = tide_terminal::git::count_worktrees(&cwd);
                    let repo_root = tide_terminal::git::repo_root(&cwd);
                    let status_entries = tide_terminal::git::status_files(&cwd);
                    results.insert(cwd, GitPollCwdResult {
                        git_info,
                        worktree_count,
                        repo_root,
                        status_entries,
                    });
                }

                let _ = tx.send(results);
                if let Some(ref w) = waker {
                    w();
                }
            }
        });

        self.git_poll_handle = Some(handle);
        // Store cwd_tx — we need it accessible. Add a field.
        self.git_poll_cwd_tx = Some(cwd_tx);
    }

    /// Execute a context menu action.
    pub(crate) fn execute_context_menu_action(&mut self, action_index: usize) {
        let menu = match self.context_menu.take() {
            Some(m) => m,
            None => return,
        };

        let items = crate::ContextMenuAction::items(menu.is_dir, menu.shell_idle);
        let action = match items.get(action_index) {
            Some(a) => *a,
            None => return,
        };

        match action {
            crate::ContextMenuAction::CdHere => {
                let path_str = menu.path.to_string_lossy();
                let cmd = format!("cd {}\n", crate::shell_escape(&path_str));
                let tid = self.focused_terminal_id();
                if let Some(tid) = tid {
                    if let Some(crate::PaneKind::Terminal(pane)) = self.panes.get_mut(&tid) {
                        pane.backend.write(cmd.as_bytes());
                    }
                }
            }
            crate::ContextMenuAction::OpenTerminalHere => {
                self.split_pane(
                    tide_core::SplitDirection::Horizontal,
                    Some(menu.path),
                );
            }
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
                self.trigger_git_poll();
                self.chrome_generation += 1;
            }
            crate::ContextMenuAction::RevealInFinder => {
                if menu.is_dir {
                    let _ = std::process::Command::new("open")
                        .arg(&menu.path)
                        .spawn();
                } else {
                    let _ = std::process::Command::new("open")
                        .arg("-R")
                        .arg(&menu.path)
                        .spawn();
                }
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
        self.trigger_git_poll();
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

        let cell_size = self.cell_size();

        let line_height = cell_size.height * FILE_TREE_LINE_SPACING;
        // Account for inset content rect and header offset.
        let content_y = self
            .file_tree_rect
            .map(|r| r.y + PANE_CORNER_RADIUS)
            .unwrap_or(self.top_inset + PANE_CORNER_RADIUS);
        let adjusted_y = position.y - content_y - FILE_TREE_HEADER_HEIGHT;
        let index = ((adjusted_y + self.file_tree_scroll) / line_height) as usize;

        // Extract click info from file tree (borrow released before open_editor_pane)
        let click_result = if let Some(tree) = self.file_tree.as_mut() {
            let entries = tree.visible_entries();
            if index < entries.len() {
                let entry = entries[index].clone();
                if entry.entry.is_dir {
                    tree.toggle(&entry.entry.path);
                    self.chrome_generation += 1;
                    self.needs_redraw = true;
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

/// Merge two git statuses with priority: Conflict > Modified > rest.
fn merge_git_status(a: FileGitStatus, b: FileGitStatus) -> FileGitStatus {
    use FileGitStatus::*;
    match (a, b) {
        (Conflict, _) | (_, Conflict) => Conflict,
        (Modified, _) | (_, Modified) => Modified,
        _ => a,
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
