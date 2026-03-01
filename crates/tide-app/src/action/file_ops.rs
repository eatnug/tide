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
        if let Some(ref tree) = self.file_tree {
            let root = tree.root();
            if root.is_dir() {
                return root.to_path_buf();
            }
        }
        // 4. Fallback
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    /// Open the file finder UI (floating popup).
    pub(crate) fn open_file_finder(&mut self) {
        let base_dir = self.resolve_base_dir();
        let mut entries: Vec<PathBuf> = Vec::new();
        Self::scan_dir(&base_dir, &base_dir, &mut entries, 0, 8);
        entries.sort();

        self.file_finder = Some(crate::FileFinderState::new(base_dir, entries));
        self.chrome_generation += 1;
    }

    /// Close the file finder UI.
    pub(crate) fn close_file_finder(&mut self) {
        if self.file_finder.is_some() {
            self.file_finder = None;
            self.chrome_generation += 1;
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

            // Skip hidden and common ignored directories
            if name.starts_with('.') || name == "node_modules" || name == "target" || name == "__pycache__" {
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
    /// If a DiffPane with the same CWD already exists in the panel, focus and refresh it.
    pub(crate) fn open_diff_pane(&mut self, cwd: PathBuf) {
        let tid = self.focused_terminal_id();
        // Check if already open in focused terminal's dock
        let tabs: Vec<tide_core::PaneId> = self.active_editor_tabs().to_vec();
        for &tab_id in &tabs {
            if let Some(PaneKind::Diff(dp)) = self.panes.get_mut(&tab_id) {
                if dp.cwd == cwd {
                    dp.refresh();
                    if let Some(tid) = tid {
                        if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                            tp.active_editor = Some(tab_id);
                        }
                    }
                    self.focused = Some(tab_id);
                    self.router.set_focused(tab_id);
                    self.chrome_generation += 1;
                    self.pane_generations.remove(&tab_id);
                    self.scroll_to_active_panel_tab();
                    return;
                }
            }
        }

        // Create new DiffPane in the editor panel
        if !self.show_editor_panel {
            self.show_editor_panel = true;
        }
        let needs_layout = self.active_editor_tabs().is_empty();
        let new_id = self.layout.alloc_id();
        let dp = crate::diff_pane::DiffPane::new(new_id, cwd);
        self.panes.insert(new_id, PaneKind::Diff(dp));
        if let Some(tid) = tid {
            if let Some(PaneKind::Terminal(tp)) = self.panes.get_mut(&tid) {
                tp.editors.push(new_id);
                tp.active_editor = Some(new_id);
            }
        }
        self.focused = Some(new_id);
        self.router.set_focused(new_id);
        self.chrome_generation += 1;
        if needs_layout {
            if !self.editor_panel_width_manual {
                self.editor_panel_width = self.auto_editor_panel_width();
            }
            self.compute_layout();
        }
        self.scroll_to_active_panel_tab();
    }
}
