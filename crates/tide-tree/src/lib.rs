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
    pub fn poll_events(&mut self) -> bool {
        let rx = match self.event_rx.as_ref() {
            Some(rx) => rx,
            None => return false,
        };

        let mut has_relevant_event = false;

        // Drain all pending events from the channel.
        while let Ok(event_result) = rx.try_recv() {
            if let Ok(_event) = event_result {
                has_relevant_event = true;
            }
        }

        if !has_relevant_event {
            return false;
        }

        // Debounce: skip if we processed events less than 100ms ago.
        let now = Instant::now();
        if let Some(last) = self.last_event_time {
            if now.duration_since(last).as_millis() < 100 {
                return false;
            }
        }

        self.last_event_time = Some(now);
        self.refresh();
        true
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

mod tests;
