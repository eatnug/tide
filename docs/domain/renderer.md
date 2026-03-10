# Renderer — tide-renderer

**Role**: GPU rendering pipeline. Converts draw commands into wgpu GPU operations.
Owns the glyph atlas, pipelines, and per-pane grid caches.

`crates/tide-renderer/src/`

## Aggregate: WgpuRenderer

### 4 Render Layers (bottom to top)

```
Layer 1: Grid         — Terminal/editor cell backgrounds + text glyphs (instanced)
Layer 2: Chrome       — UI chrome: tab bars, borders, file tree, search bar (cached)
Layer 3: Overlay      — Per-frame: cursor, IME preedit, selection highlight (rebuilt every frame)
Layer 4: Top          — Modals, search results, rounded rects (rebuilt every frame)
```

Each layer has its own vertex/index buffers. Only dirty layers are re-uploaded to GPU.

### Pipelines (GPU shaders)

| Pipeline | Purpose | Technique |
|----------|---------|-----------|
| `rect_pipeline` | Solid rectangles | 2-triangle quads |
| `chrome_rounded_pipeline` | Rounded rectangles (UI) | SDF in fragment shader |
| `glyph_pipeline` | Text rendering | MSDF (multi-channel signed distance field) |
| `grid_bg_pipeline` | Cell backgrounds | Instanced rendering |
| `grid_glyph_pipeline` | Cell text | Instanced rendering |

### GlyphAtlas

```
4096×4096 RGBA texture
├── Row-based bin packing
├── On-demand rasterization (MSDF via MsdfFontStore)
├── Cache: HashMap<(char, bold, italic), AtlasRegion>
├── Warmup: ASCII + common Korean Jamo pre-rasterized at startup
└── Overflow: full reset + re-rasterize (logged as warning)
```

### Per-Pane Grid Cache

```rust
PaneGridCache {
    bg_instances: Vec<GridBgInstance>,      // Cell background quads
    glyph_instances: Vec<GridGlyphInstance>, // Cell text quads
}
```

**Incremental update**: When only one pane changes (e.g., terminal output), only that pane's cache is rebuilt. Other panes' instances are reused. The `assemble_grid()` method concatenates all pane caches into the global instance buffer.

### Font System

Two systems in tandem:
1. **cosmic-text** (FontSystem) — font discovery + shaping + CJK/emoji fallback chain
2. **MsdfFontStore** — direct MSDF rasterization of monospace glyphs

Pre-computation:
- `precompute_cell_sizes()` — measures cell width/height for font sizes 8..=32
- `warmup_ascii()` — pre-rasterizes 95 printable ASCII characters
- `warmup_common_unicode()` — pre-rasterizes Korean Jamo + common CJK

## Render Frame Lifecycle

```
begin_frame(size)
    │  Clear all per-frame vertex buffers
    │
    ├── begin_pane_grid(pane_id)
    │     draw_cell() × N           ← instanced grid instances
    │   end_pane_grid()
    │   (repeat for each dirty pane)
    │
    ├── assemble_grid(pane_order)    ← concatenate pane caches → instance buffers
    │
    ├── draw_rect(), draw_text()     ← chrome layer (if chrome_generation changed)
    │
    ├── draw_rect(), draw_text()     ← overlay layer (cursor, selection, IME)
    │
    └── draw_top_rounded_rect()      ← top layer (modals)

end_frame()
    │
    ▼
render_frame(surface, device, queue)
    │  Upload dirty buffers to GPU
    │  Encode 4 render passes
    │  Submit command buffer
    │  Present frame
```

## Key Methods

| Method | Purpose |
|--------|---------|
| `begin_frame(size)` | Clear per-frame buffers |
| `begin_pane_grid(id)` / `end_pane_grid()` | Scope instanced grid drawing to one pane |
| `assemble_grid(order)` | Concatenate pane caches into instance buffers |
| `draw_rect(rect, color)` | Add solid rectangle |
| `draw_text(text, pos, style, clip)` | Add text with clipping |
| `draw_cell(char, row, col, style, size, offset)` | Add one grid cell |
| `draw_top_rounded_rect(rect, color, radius)` | SDF rounded rect on top layer |
| `ensure_glyph_cached(char, bold, italic)` | Rasterize and cache glyph on demand |
| `render_frame(surface, device, queue)` | Submit GPU work |

## Performance Design

1. **Instanced rendering**: One draw call per grid pass (bg + glyph), not per-cell
2. **Per-pane caching**: Unchanged panes skip grid rebuild entirely
3. **Chrome caching**: Tab bars, borders rebuild only when `chrome_generation` changes
4. **MSDF text**: Resolution-independent, single texture lookup per glyph
5. **Atlas warmup**: ASCII + Korean pre-rasterized at startup to avoid frame hitches
