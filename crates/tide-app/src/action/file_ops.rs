use std::path::PathBuf;

use tide_core::FileTreeSource;

use crate::pane::PaneKind;
use crate::App;

impl App {
    /// Get a working directory for file operations: try focused terminal, then any terminal,
    /// then file tree root, then std::env::current_dir.
    pub(super) fn resolve_base_dir(&self) -> PathBuf {
        // 1. Focused terminal CWD
        if let Some(cwd) = self.focused_terminal_cwd() {
            return cwd;
        }
        // 2. Any terminal pane's CWD
        for pane in self.panes.values() {
            if let PaneKind::Terminal(p) = pane {
                if let Some(cwd) = p.backend.detect_cwd_fallback() {
                    return cwd;
                }
            }
        }
        // 3. File tree root
        if let Some(ref tree) = self.ft.tree {
            let root = tree.root();
            if root.is_dir() {
                return root.to_path_buf();
            }
        }
        // 4. Fallback
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    /// Open the file finder UI (floating popup).
    /// If `replace_pane_id` is Some, the selected file will replace that pane
    /// instead of opening as a new tab.
    pub(crate) fn open_file_finder_with_replace(&mut self, replace_pane_id: Option<tide_core::PaneId>) {
        // Cancel any in-progress drag when opening a modal
        self.interaction.pane_drag = crate::drag_drop::PaneDragState::Idle;

        let base_dir = self.resolve_base_dir();
        let mut entries: Vec<PathBuf> = Vec::new();
        Self::scan_dir(&base_dir, &base_dir, &mut entries, 0, 8);
        entries.sort();

        let mut state = crate::FileFinderState::new(base_dir, entries);
        state.replace_pane_id = replace_pane_id;
        self.modal.file_finder = Some(state);
        self.cache.invalidate_chrome();
        // Hide browser webviews so they don't cover the popup
        self.sync_browser_webview_frames();
    }

    /// Open the file finder UI (floating popup).
    pub(crate) fn open_file_finder(&mut self) {
        self.open_file_finder_with_replace(None);
    }

    /// Close the file finder UI.
    pub(crate) fn close_file_finder(&mut self) {
        if self.modal.file_finder.is_some() {
            self.modal.file_finder = None;
            self.cache.invalidate_chrome();
            // Re-show browser webviews that were hidden for the popup
            self.sync_browser_webview_frames();
        }
    }

    /// Recursively scan a directory, collecting file paths relative to base_dir.
    fn scan_dir(dir: &std::path::Path, base_dir: &std::path::Path, entries: &mut Vec<PathBuf>, depth: usize, max_depth: usize) {
        if depth > max_depth {
            return;
        }
        let read_dir = match std::fs::read_dir(dir) {
            Ok(rd) => rd,
            Err(_) => return,
        };
        let mut subdirs: Vec<PathBuf> = Vec::new();
        for entry in read_dir.flatten() {
            let path = entry.path();
            let file_name = entry.file_name();
            let name = file_name.to_string_lossy();

            // Skip .git and common large/generated directories
            if name == ".git" || name == "node_modules" || name == "target" || name == "__pycache__" {
                continue;
            }

            if path.is_dir() {
                subdirs.push(path);
            } else if path.is_file() {
                if let Ok(rel) = path.strip_prefix(base_dir) {
                    entries.push(rel.to_path_buf());
                }
            }
        }
        for subdir in subdirs {
            Self::scan_dir(&subdir, base_dir, entries, depth + 1, max_depth);
        }
    }

    /// Open or focus a DiffPane for the given CWD.
    /// If a DiffPane with the same CWD already exists, focus and refresh it.
    pub(crate) fn open_diff_pane(&mut self, cwd: PathBuf) {
        let focused = match self.focused {
            Some(id) => id,
            None => return,
        };

        // Check if already open anywhere -> refresh and focus
        for (&tab_id, pane) in &mut self.panes {
            if let PaneKind::Diff(dp) = pane {
                if dp.cwd == cwd {
                    dp.refresh();
                    self.layout.set_active_tab(tab_id);
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.cache.invalidate_chrome();
                    self.cache.invalidate_pane(tab_id);
                    return;
                }
            }
        }

        // Create new DiffPane as a tab
        let new_id = self.layout.alloc_id();
        let dp = crate::diff_pane::DiffPane::new(new_id, cwd);
        self.panes.insert(new_id, PaneKind::Diff(dp));
        self.layout.add_tab(focused, new_id);
        self.layout.set_active_tab(new_id);
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.cache.invalidate_chrome();
        self.compute_layout();
    }
}
