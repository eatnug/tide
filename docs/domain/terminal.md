# Terminal — tide-terminal

**Role**: PTY management and grid synchronization.
Wraps alacritty_terminal with a 3-thread architecture for responsive terminal rendering.

`crates/tide-terminal/src/lib.rs`

## Aggregate: Terminal

```rust
Terminal {
    term: Arc<FairMutex<Term>>,        // alacritty terminal emulator
    notifier: Notifier,                 // Channel to PTY event loop
    cached_grid: TerminalGrid,          // Current visible grid (main thread copy)
    cached_cursor: CursorState,         // Current cursor state
    current_dir: Option<PathBuf>,       // Detected CWD
    cols: u16, rows: u16,              // Grid dimensions

    // Sync thread communication
    snapshot: Arc<Mutex<SharedSnapshot>>,  // Exchange point
    snapshot_ready: Arc<AtomicBool>,       // "New data available"
    dirty: Arc<AtomicBool>,                // "PTY has new output"
    waker: Arc<Mutex<Option<WakeCallback>>>, // Main thread waker

    // State tracking
    grid_generation: u64,               // Monotonic counter (only increases on content change)
    stay_at_bottom: Arc<AtomicBool>,    // Auto-scroll mode
    url_ranges: Vec<Vec<(usize, usize)>>, // Detected URLs per row
    inverse_cursor: Option<(u16, u16)>,   // TUI cursor fallback
    pending_pty_resize: Option<(WindowSize, Instant)>, // Debounced resize (50ms)
}
```

## 3-Thread Architecture

```
┌─────────────┐     ┌──────────────┐     ┌─────────────┐
│  PTY Thread  │────▸│  Sync Thread │────▸│ Main Thread  │
│ (alacritty)  │     │ (GridSyncer) │     │  (App)       │
│              │     │              │     │              │
│ reads bytes  │     │ diffs grid   │     │ swaps        │
│ parses VT    │     │ converts     │     │ snapshot     │
│ updates Term │     │ colors       │     │ renders      │
│              │     │ detects URLs │     │              │
└─────────────┘     └──────────────┘     └─────────────┘
        │                   │                    │
        │   dirty flag      │  snapshot_ready    │
        ├──────────────────▸├───────────────────▸│
        │                   │  waker callback    │
                            ├───────────────────▸│
```

### PTY Thread (alacritty EventLoop)
- Reads bytes from shell process via OS pipe
- Parses VT escape sequences, updates `Term` grid cells
- Sets `dirty` flag when new output arrives

### Sync Thread (GridSyncer)
Two-phase algorithm:

**Phase 1** (lock held ~1-10ms):
1. Lock `Term`, copy palette + all grid cells + cursor into local buffer
2. Release lock immediately

**Phase 2** (no lock):
1. Diff against previous frame — only convert changed cells
2. Convert ANSI colors → `Color` (dark/light mode aware)
3. Apply DIM (×0.65), INVERSE (swap fg/bg) flags
4. Scan for URL patterns (regex: `https?://`)
5. Detect inverse cursor (TUI apps that hide cursor)
6. Increment `grid_generation` if any cell changed
7. Write results to shared `snapshot`
8. Set `snapshot_ready`, call `waker` to wake main thread
9. Park until next `dirty` signal

### Main Thread (App)
- `Terminal::process()`:
  1. Flush debounced PTY resize if 50ms elapsed
  2. Call `consume_snapshot()` — swap in latest data (cheap pointer swap)

## Key Methods

| Method | Purpose |
|--------|---------|
| `process()` | Consume PTY output + flush pending resize |
| `grid()` | Access the cached TerminalGrid |
| `cursor()` | Access the cached CursorState |
| `write(data)` | Send bytes to PTY (keyboard input) |
| `resize(cols, rows)` | Queue debounced PTY resize |
| `cwd()` | Get detected working directory |

## Performance Optimizations

1. **Diff-based sync**: Only convert cells that actually changed between frames
2. **Debounced resize**: PTY resize throttled to 50ms to prevent SIGWINCH storms
3. **Snapshot swap**: Main thread never blocks on sync — just swaps a pointer
4. **Parked sync thread**: Sleeps when no PTY output, woken by dirty flag
5. **Generation tracking**: Renderer skips unchanged panes via `grid_generation`
