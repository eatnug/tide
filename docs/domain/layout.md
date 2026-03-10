# Layout — tide-layout

**Role**: Owns the binary split tree that arranges Panes in the window.
Knows nothing about Pane content — only PaneIds and rectangles.

`crates/tide-layout/src/`

## Aggregate: SplitLayout

```rust
SplitLayout {
    root: Option<Node>,             // Binary tree root (None if empty)
    next_id: PaneId,                // Counter for allocating new PaneIds
    active_drag: Option<Vec<bool>>, // Path to the split border being dragged
    last_window_size: Option<Size>, // Needed for drag reconstruction
}
```

### Binary Tree Structure

```
Node::Split {
    direction: SplitDirection,   // Horizontal or Vertical
    ratio: f32,                  // 0.0–1.0, clamped to [0.1, 0.9]
    left: Box<Node>,             // Left/top subtree
    right: Box<Node>,            // Right/bottom subtree
}
│
Node::Leaf(TabGroup)             // Terminal node — holds stacked tabs
```

Example: Two panes side by side, left pane has 2 tabs:
```
Split(Vertical, 0.5)
├── Leaf(TabGroup { tabs: [1, 2], active: 0 })
└── Leaf(TabGroup { tabs: [3], active: 0 })
```

## Entity: TabGroup

```rust
TabGroup {
    tabs: Vec<PaneId>,    // All Panes in this slot (order = tab bar order)
    active: usize,        // Index of the visible tab
}
```

**Key methods:**
- `add_tab(id)` → inserts after active tab, makes it active
- `remove_tab(id)` → removes, adjusts active index
- `set_active(id)` → switches visible tab
- `active_pane()` → returns the currently visible PaneId

## Key Operations

### split(pane, direction) → PaneId
1. Allocate new PaneId via `next_id += 1`
2. Find the Leaf containing `pane`
3. Replace it with: `Split { direction, ratio: 0.5, left: original_leaf, right: new_leaf }`
4. **Equalization**: if split direction matches parent's direction, redistribute ratios equally

### remove(pane)
1. Remove pane from its TabGroup
2. If TabGroup becomes empty → remove the Leaf
3. If parent Split now has only one child → collapse (replace Split with remaining child)

### compute(window_size) → Vec<(PaneId, Rect)>
1. Recursively walk the tree
2. At each Split: divide Rect by direction and ratio
3. At each Leaf: emit `(active_pane_id, rect)` — only active tab gets a Rect

### Drag & Drop

**Border dragging:**
- `begin_drag(position)` → find closest split border, store path
- `drag_border(position)` → update ratio on the target split node

**Pane movement:**
- `move_pane(source, target, zone)` → remove source, insert next to target
- `move_pane_to_root(source, zone)` → wrap remaining tree, add source at edge
- `simulate_drop(...)` → clone tree, apply move, return preview Rect

### Serialization: LayoutSnapshot
```rust
enum LayoutSnapshot {
    Leaf { tabs: Vec<PaneId>, active: usize },
    Split { direction, ratio, left, right },
}
```
Used for workspace save/load. `snapshot()` captures tree, `from_snapshot()` restores it.

## Invariants

1. **Every PaneId appears exactly once** across all TabGroups
2. **Ratio is clamped** to [0.1, 0.9] — no invisible panes
3. **No empty TabGroups** — removing last tab removes the Leaf
4. **No single-child Splits** — after removal, tree collapses
5. **next_id only increases** — PaneIds are never reused
