# File Tree — tide-tree

**Role**: Filesystem directory tree with lazy loading and filesystem watching.

`crates/tide-tree/src/lib.rs`

## Aggregate: FsTree

```rust
FsTree {
    root: PathBuf,                            // Root directory
    entries: Vec<TreeEntry>,                  // Flattened visible tree
    expanded: HashSet<PathBuf>,               // Which directories are open
    children_cache: HashMap<PathBuf, Vec<FileEntry>>, // Lazy-loaded children
    watcher: Option<RecommendedWatcher>,      // notify crate filesystem watcher
    event_rx: Option<Receiver<notify::Event>>,// Filesystem events channel
    last_event_time: Option<Instant>,         // Debounce timestamp
    pending_events: bool,                     // Events arrived during debounce
}
```

## Flattening Algorithm

Depth-first traversal produces a single flat `Vec<TreeEntry>`:

```
project/
├── src/           depth=0, expanded=true
│   ├── main.rs    depth=1
│   └── lib.rs     depth=1
├── tests/         depth=0, expanded=false
└── Cargo.toml     depth=0
```

Result: `[src(0,expanded), main.rs(1), lib.rs(1), tests(0,collapsed), Cargo.toml(0)]`

Only expanded directories' children appear in the list. This flat list is what the UI renders.

## Key Operations

| Method | Description |
|--------|-------------|
| `set_root(path)` | Clear state, load root, start filesystem watcher |
| `toggle(path)` | Expand/collapse directory. Lazy-loads children on first expand |
| `refresh()` | Re-read all expanded directories, rebuild flat list |
| `poll_events()` | Drain filesystem events, debounce (100ms), trigger refresh |
| `visible_entries()` | Access the flattened TreeEntry list |

## Sorting

Directories first (alphabetically, case-insensitive), then files (same ordering).
Symlinks are followed (`std::fs::metadata`, not `symlink_metadata`).

## Filesystem Watching

- Uses `notify` crate (FSEvents on macOS, inotify on Linux)
- Recursive watch on root directory
- Events flow through unbounded MPSC channel
- **Debounce**: 100ms window after last processing to batch rapid changes (e.g., npm install)

## Git Status Integration

Git status is managed by `tide-app` (FileTreeModel), not by FsTree itself:
- Async git poller thread updates `HashMap<PathBuf, FileGitStatus>` in background
- `FileGitStatus`: `Modified`, `Added`, `Deleted`, `Untracked`, `Conflict`
- CWD tracking: follows focused Terminal's working directory to git root (sticky)
