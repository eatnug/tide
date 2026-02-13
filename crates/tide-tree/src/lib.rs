// File tree implementation (Stream D)
// Implements tide_core::FileTreeSource with fs watching via notify

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Instant;
use tide_core::{FileEntry, FileTreeSource, TreeEntry};

/// Reads a directory and returns sorted FileEntry items.
/// Directories come first, then files, each group sorted alphabetically (case-insensitive).
/// Permission errors and unreadable entries are silently skipped.
/// Symlinks are followed.
fn read_directory(path: &Path) -> Vec<FileEntry> {
    let read_dir = match std::fs::read_dir(path) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<FileEntry> = read_dir
        .filter_map(|entry| {
            let entry = entry.ok()?;
            // Follow symlinks: use std::fs::metadata (follows symlinks)
            // Fall back to symlink_metadata if that fails (e.g. broken symlink)
            let metadata = std::fs::metadata(entry.path())
                .or_else(|_| entry.metadata())
                .ok()?;
            let name = entry.file_name().into_string().ok()?;
            Some(FileEntry {
                name,
                path: entry.path(),
                is_dir: metadata.is_dir(),
            })
        })
        .collect();

    entries.sort_by(|a, b| {
        // Directories first
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            a.name
                .to_lowercase()
                .cmp(&b.name.to_lowercase())
                .then_with(|| a.name.cmp(&b.name))
        })
    });

    entries
}

pub struct FsTree {
    root: PathBuf,
    /// The flattened list of visible entries, rebuilt after any mutation.
    entries: Vec<TreeEntry>,
    /// Set of expanded directory paths.
    expanded: HashSet<PathBuf>,
    /// Cache of children per directory path (lazy-loaded).
    children_cache: HashMap<PathBuf, Vec<FileEntry>>,
    /// Filesystem watcher (held to keep the watch alive).
    watcher: Option<RecommendedWatcher>,
    /// Channel receiving raw filesystem events from the watcher.
    event_rx: Option<mpsc::Receiver<notify::Result<notify::Event>>>,
    /// Timestamp of the last processed event batch, used for debouncing.
    last_event_time: Option<Instant>,
}

impl FsTree {
    pub fn new(root: PathBuf) -> Self {
        let mut tree = FsTree {
            root: PathBuf::new(),
            entries: Vec::new(),
            expanded: HashSet::new(),
            children_cache: HashMap::new(),
            watcher: None,
            event_rx: None,
            last_event_time: None,
        };
        tree.set_root(root);
        tree
    }

    /// Call this periodically to process any pending filesystem events.
    /// Events are debounced: changes within 100ms of the last processed batch are ignored
    /// until 100ms has elapsed, at which point a single refresh is triggered.
    pub fn poll_events(&mut self) {
        let rx = match self.event_rx.as_ref() {
            Some(rx) => rx,
            None => return,
        };

        let mut has_relevant_event = false;

        // Drain all pending events from the channel.
        while let Ok(event_result) = rx.try_recv() {
            if let Ok(_event) = event_result {
                has_relevant_event = true;
            }
        }

        if !has_relevant_event {
            return;
        }

        // Debounce: skip if we processed events less than 100ms ago.
        let now = Instant::now();
        if let Some(last) = self.last_event_time {
            if now.duration_since(last).as_millis() < 100 {
                return;
            }
        }

        self.last_event_time = Some(now);
        self.refresh();
    }

    /// Start (or restart) the filesystem watcher on the current root.
    fn start_watcher(&mut self) {
        let (tx, rx) = mpsc::channel();

        let watcher = notify::recommended_watcher(tx);

        match watcher {
            Ok(mut w) => {
                // Watch the root recursively. Ignore errors (e.g. path doesn't exist).
                let _ = w.watch(&self.root, RecursiveMode::Recursive);
                self.watcher = Some(w);
                self.event_rx = Some(rx);
            }
            Err(_) => {
                self.watcher = None;
                self.event_rx = None;
            }
        }
    }

    /// Rebuild the flattened `entries` vec via depth-first traversal of expanded dirs.
    fn rebuild_visible(&mut self) {
        let mut result = Vec::new();
        self.walk_dir(&self.root.clone(), 0, &mut result);
        self.entries = result;
    }

    /// Recursive helper for depth-first traversal.
    fn walk_dir(&self, dir: &Path, depth: usize, out: &mut Vec<TreeEntry>) {
        let children = match self.children_cache.get(dir) {
            Some(c) => c,
            None => return,
        };

        for child in children {
            let is_expanded = child.is_dir && self.expanded.contains(&child.path);
            let has_children = child.is_dir;

            out.push(TreeEntry {
                entry: child.clone(),
                depth,
                is_expanded,
                has_children,
            });

            if is_expanded {
                self.walk_dir(&child.path, depth + 1, out);
            }
        }
    }

    /// Ensure a directory's children are loaded into the cache.
    fn ensure_loaded(&mut self, path: &Path) {
        if !self.children_cache.contains_key(path) {
            let children = read_directory(path);
            self.children_cache.insert(path.to_path_buf(), children);
        }
    }
}

impl tide_core::FileTreeSource for FsTree {
    fn set_root(&mut self, path: PathBuf) {
        self.root = path;
        self.expanded.clear();
        self.children_cache.clear();
        self.entries.clear();

        // Load the root directory's children.
        let children = read_directory(&self.root);
        self.children_cache.insert(self.root.clone(), children);

        self.rebuild_visible();
        self.start_watcher();
    }

    fn root(&self) -> &Path {
        &self.root
    }

    fn visible_entries(&self) -> &[TreeEntry] {
        &self.entries
    }

    fn toggle(&mut self, path: &Path) {
        if self.expanded.contains(path) {
            self.expanded.remove(path);
        } else {
            self.expanded.insert(path.to_path_buf());
            // Lazy-load children if not yet cached.
            self.ensure_loaded(path);
        }
        self.rebuild_visible();
    }

    fn refresh(&mut self) {
        // Re-read root directory.
        let root_children = read_directory(&self.root);
        self.children_cache.insert(self.root.clone(), root_children);

        // Re-read all expanded directories.
        let expanded_dirs: Vec<PathBuf> = self.expanded.iter().cloned().collect();
        for dir in &expanded_dirs {
            let children = read_directory(dir);
            self.children_cache.insert(dir.clone(), children);
        }

        self.rebuild_visible();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;
    use tide_core::FileTreeSource;

    /// Helper to create a temp directory with some structure.
    fn setup_temp_dir() -> TempDir {
        let tmp = TempDir::new().expect("failed to create temp dir");
        let root = tmp.path();

        // Create directories
        fs::create_dir(root.join("alpha_dir")).unwrap();
        fs::create_dir(root.join("beta_dir")).unwrap();

        // Create files
        fs::write(root.join("charlie.txt"), "hello").unwrap();
        fs::write(root.join("able.txt"), "world").unwrap();

        // Create a file inside alpha_dir
        fs::write(root.join("alpha_dir").join("inner.txt"), "inner").unwrap();

        tmp
    }

    #[test]
    fn test_set_root_populates_entries() {
        let tmp = setup_temp_dir();
        let tree = FsTree::new(tmp.path().to_path_buf());

        let entries = tree.visible_entries();
        assert!(!entries.is_empty(), "entries should be populated after set_root");
    }

    #[test]
    fn test_directories_sorted_before_files() {
        let tmp = setup_temp_dir();
        let tree = FsTree::new(tmp.path().to_path_buf());

        let entries = tree.visible_entries();

        // Find the index where directories end and files begin.
        let first_file_idx = entries.iter().position(|e| !e.entry.is_dir);
        let last_dir_idx = entries.iter().rposition(|e| e.entry.is_dir);

        if let (Some(first_file), Some(last_dir)) = (first_file_idx, last_dir_idx) {
            assert!(
                last_dir < first_file,
                "All directories should come before all files. last_dir={}, first_file={}",
                last_dir,
                first_file
            );
        }
    }

    #[test]
    fn test_alphabetical_within_groups() {
        let tmp = setup_temp_dir();
        let tree = FsTree::new(tmp.path().to_path_buf());

        let entries = tree.visible_entries();
        let names: Vec<&str> = entries.iter().map(|e| e.entry.name.as_str()).collect();

        // Directories: alpha_dir, beta_dir  (alphabetical)
        // Files: able.txt, charlie.txt  (alphabetical)
        assert_eq!(names, vec!["alpha_dir", "beta_dir", "able.txt", "charlie.txt"]);
    }

    #[test]
    fn test_toggle_expands_and_collapses_directory() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");

        // Initially collapsed: should have 4 top-level entries.
        assert_eq!(tree.visible_entries().len(), 4);

        // Expand alpha_dir.
        tree.toggle(&alpha_path);

        // Now we should see the inner file too: 4 + 1 = 5.
        assert_eq!(tree.visible_entries().len(), 5);

        // The alpha_dir entry should be marked as expanded.
        let alpha_entry = tree
            .visible_entries()
            .iter()
            .find(|e| e.entry.path == alpha_path)
            .expect("alpha_dir should be visible");
        assert!(alpha_entry.is_expanded);

        // Collapse alpha_dir.
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 4);

        let alpha_entry = tree
            .visible_entries()
            .iter()
            .find(|e| e.entry.path == alpha_path)
            .expect("alpha_dir should be visible");
        assert!(!alpha_entry.is_expanded);
    }

    #[test]
    fn test_visible_entries_respects_collapsed_state() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");
        let beta_path = tmp.path().join("beta_dir");

        // Expand alpha_dir (has inner.txt) -- should add 1 child.
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 5);

        // Expand beta_dir (empty) -- no new children.
        tree.toggle(&beta_path);
        assert_eq!(tree.visible_entries().len(), 5);

        // Collapse alpha_dir -- removes 1 child.
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 4);
    }

    #[test]
    fn test_depth_of_nested_entries() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");
        tree.toggle(&alpha_path);

        for entry in tree.visible_entries() {
            if entry.entry.path == alpha_path {
                assert_eq!(entry.depth, 0, "alpha_dir should be at depth 0");
            }
            if entry.entry.name == "inner.txt" {
                assert_eq!(entry.depth, 1, "inner.txt should be at depth 1");
            }
        }
    }

    #[test]
    fn test_refresh_picks_up_new_files() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let initial_count = tree.visible_entries().len();
        assert_eq!(initial_count, 4);

        // Create a new file in the root.
        fs::write(tmp.path().join("new_file.txt"), "new").unwrap();

        // Before refresh, tree doesn't know about the new file.
        assert_eq!(tree.visible_entries().len(), 4);

        // After refresh, tree picks it up.
        tree.refresh();
        assert_eq!(tree.visible_entries().len(), 5);

        // The new file should be in the list.
        let has_new = tree
            .visible_entries()
            .iter()
            .any(|e| e.entry.name == "new_file.txt");
        assert!(has_new, "new_file.txt should appear after refresh");
    }

    #[test]
    fn test_refresh_picks_up_new_files_in_expanded_dir() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 5);

        // Add a new file inside expanded alpha_dir.
        fs::write(alpha_path.join("new_inner.txt"), "new inner").unwrap();

        tree.refresh();
        assert_eq!(tree.visible_entries().len(), 6);
    }

    #[test]
    fn test_set_root_resets_state() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        let alpha_path = tmp.path().join("alpha_dir");
        tree.toggle(&alpha_path);
        assert_eq!(tree.visible_entries().len(), 5);

        // Create a new temp dir and set it as root.
        let tmp2 = TempDir::new().unwrap();
        fs::write(tmp2.path().join("only.txt"), "only").unwrap();

        tree.set_root(tmp2.path().to_path_buf());

        assert_eq!(tree.root(), tmp2.path());
        assert_eq!(tree.visible_entries().len(), 1);
        assert!(!tree.expanded.contains(&alpha_path));
    }

    #[test]
    fn test_has_children_flag() {
        let tmp = setup_temp_dir();
        let tree = FsTree::new(tmp.path().to_path_buf());

        for entry in tree.visible_entries() {
            if entry.entry.is_dir {
                assert!(entry.has_children, "directories should have has_children=true");
            } else {
                assert!(!entry.has_children, "files should have has_children=false");
            }
        }
    }

    #[test]
    fn test_permission_error_skips_entry() {
        // read_directory should not panic on a nonexistent path
        let entries = read_directory(Path::new("/nonexistent_path_12345"));
        assert!(entries.is_empty());
    }

    #[test]
    fn test_toggle_nonexistent_path_does_not_panic() {
        let tmp = setup_temp_dir();
        let mut tree = FsTree::new(tmp.path().to_path_buf());

        // Toggling a path that doesn't exist should not panic.
        tree.toggle(Path::new("/nonexistent_path_12345"));
    }

    #[test]
    fn test_symlink_followed() {
        let tmp = setup_temp_dir();
        let root = tmp.path();

        // Create a directory and a symlink to it.
        let real_dir = root.join("real_dir");
        fs::create_dir(&real_dir).unwrap();
        fs::write(real_dir.join("file_in_real.txt"), "content").unwrap();

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(&real_dir, root.join("link_dir")).unwrap();
        }

        let mut tree = FsTree::new(root.to_path_buf());

        #[cfg(unix)]
        {
            // The symlink should appear as a directory.
            let link_entry = tree
                .visible_entries()
                .iter()
                .find(|e| e.entry.name == "link_dir");
            assert!(link_entry.is_some(), "symlink should be visible");
            assert!(
                link_entry.unwrap().entry.is_dir,
                "symlink to dir should show as dir"
            );

            // Expanding the symlink should show the contents of real_dir.
            tree.toggle(&root.join("link_dir"));
            let has_inner = tree
                .visible_entries()
                .iter()
                .any(|e| e.entry.name == "file_in_real.txt");
            assert!(has_inner, "expanding symlink dir should show inner files");
        }
    }
}
