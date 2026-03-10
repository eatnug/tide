# Flow: Terminal Output

How PTY bytes become pixels on screen.

## Participants

| Context | Role |
|---------|------|
| `tide-terminal` | PTY → grid sync (3 threads) |
| `tide-app` | Calls `process()`, checks `grid_generation`, triggers render |
| `tide-renderer` | Converts grid cells to GPU instances |

## Sequence

```
Shell outputs "hello\n"
    │
    ▼
┌─ PTY Thread (alacritty EventLoop) ──────────────┐
│ 1. read() from OS pipe → bytes "hello\n"         │
│ 2. Feed to VT parser → updates Term grid cells   │
│ 3. Set dirty.store(true)                         │
│ 4. Unpark sync thread                            │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ Sync Thread (GridSyncer) ──────────────────────┐
│ Phase 1 (lock ~1-10ms):                          │
│   Lock Term, copy palette + all cells + cursor   │
│   Release lock                                   │
│                                                  │
│ Phase 2 (no lock):                               │
│   Diff raw_buf vs prev_raw_buf                   │
│   Convert only changed cells:                    │
│     ANSI color → Color (dark/light aware)        │
│     Apply DIM (×0.65), INVERSE (swap fg/bg)      │
│   Scan for URLs (regex)                          │
│   Detect inverse cursor (TUI apps)               │
│   Increment grid_generation                      │
│                                                  │
│   Write to shared snapshot                       │
│   Set snapshot_ready = true                      │
│   Call waker() → wake main thread                │
│   Park until next dirty signal                   │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ Main Thread (App::update) ─────────────────────┐
│ terminal.process()                               │
│   └── consume_snapshot() → swap in new grid      │
│                                                  │
│ Check: grid_generation changed?                  │
│   └── YES → cache.invalidate_pane(id)            │
│            cache.needs_redraw = true             │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ Main Thread (App::render) ─────────────────────┐
│ renderer.begin_pane_grid(pane_id)                │
│   for each (row, col) in grid:                   │
│     renderer.draw_cell(char, row, col, style)    │
│ renderer.end_pane_grid()                         │
│                                                  │
│ renderer.assemble_grid(pane_order)               │
│   └── Only this pane's cache was rebuilt;        │
│       other panes reuse cached instances         │
│                                                  │
│ renderer.render_frame()                          │
│   └── Upload instances → GPU                     │
│       Encode render pass                         │
│       Submit + present                           │
└──────────────────────────────────────────────────┘
```

## Performance Path

| Step | Cost | Optimization |
|------|------|-------------|
| PTY read | O(bytes) | alacritty's optimized parser |
| Grid sync | O(changed_cells) | Diff against previous frame |
| Color conversion | O(changed_cells) | Skip unchanged cells |
| Snapshot swap | O(1) | Pointer swap, no copy |
| Grid cache rebuild | O(rows × cols) | Only for this pane |
| Instance assembly | O(total_instances) | Incremental if pane order unchanged |
| GPU draw | O(1) | Instanced rendering (1 draw call) |

## Generation Tracking

```
grid_generation (tide-terminal)    ← incremented on cell change
    │
    ▼
pane_generations[id] (tide-app)    ← compared to detect change
    │
    ▼
grid_dirty_panes (tide-renderer)   ← marks pane for cache rebuild
```

If a terminal has no output (idle shell), none of these increment → zero GPU work.
