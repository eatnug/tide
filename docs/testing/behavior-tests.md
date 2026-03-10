# Behavior Tests — Living Specification

`crates/tide-app/src/behavior_tests.rs` is Tide's **executable specification**.
Reading just the test names should tell you what the system does.

## Principles

1. **Test name = natural language sentence** — `fn closing_a_dirty_editor_with_file_shows_save_confirm()`
2. **Organized by domain module** — `mod pane_lifecycle`, `mod focus_management`, etc.
3. **Red-Green** — write tests before implementing the feature
4. **Pure functions first** — prefer structures testable without GPU/PTY

## Current Module Structure

| Module | Domain | Tests | What it verifies |
|--------|--------|-------|------------------|
| `focus_management` | FocusArea, focus_terminal | 10 | Focus switching, zoom, file tree toggle |
| `modal_behavior` | ModalStack | 15 | Modal priority, ESC dismiss, input blocking |
| `pane_lifecycle` | PaneKind, TabGroup | 13 | Create, split, close, dedup open |
| `editor_behavior` | EditorPane | 8 | Preview, dirty detection, diff mode |
| `keyboard_routing` | GlobalAction, Router | 9 | Key mapping, modifiers, action dispatch |
| `launcher_behavior` | Launcher, LauncherChoice | 5 | Launcher resolution, invalid choices |
| `theme_behavior` | Theme, Color | 4 | Theme toggle, font size default, cache invalidation |
| `workspace_behavior` | Workspace, WorkspaceManager | 8 | Switch, save/load, wrap-around |
| `search_behavior` | SearchState | 6 | Search open/close, result navigation, wrap |
| `ime_behavior` | ImeState | 4 | IME preedit, commit, clear |
| `render_cache_behavior` | RenderCache, Generation | 5 | Cache invalidation, generation increment |
| `global_actions` | GlobalAction dispatch | 10 | Global action handling |
| `text_input_routing` | TextInput, send_text | 9 | Text routing target resolution |
| `session_behavior` | Session save/load | 3 | Session serialization, restore |
| `preview_scroll` | EditorPane preview | 8 | j/k/d/u/g/G scroll, clamp |

## Writing a New Test

### Step 1: Decide which module it belongs to

Check `docs/glossary.md` to identify which Entity/Aggregate you're testing.
Add the test to the matching module. If it's a new domain, create a new `mod`:

```rust
#[cfg(test)]
mod drag_drop_behavior {
    // ...
}
```

### Step 2: Name it as a natural language sentence

```rust
// Good — reading it tells you the behavior
fn dragging_pane_to_left_zone_creates_vertical_split()
fn dropping_on_workspace_sidebar_moves_pane_to_that_workspace()
fn drag_cancelled_on_escape_key()

// Bad — unclear what's being verified
fn test_drag()
fn drag_works()
fn check_split()
```

### Step 3: Given-When-Then structure

```rust
#[test]
fn closing_a_dirty_editor_with_file_shows_save_confirm() {
    // Given: a dirty editor with a file path
    let (mut app, id) = app_with_editor();
    if let Some(PaneKind::Editor(pane)) = app.panes.get_mut(&id) {
        pane.editor.insert_text("hello");
        pane.editor.buffer.file_path = Some(PathBuf::from("/tmp/test.txt"));
    }

    // When: attempt to close that Pane
    app.close_specific_pane(id);

    // Then: SaveConfirm modal is shown
    assert!(app.modal.save_confirm.is_some());
    assert_eq!(app.modal.save_confirm.as_ref().unwrap().pane_id, id);
}
```

### Step 4: Reuse helper functions

Each module has `test_app()` and `app_with_editor()` helpers.
Follow the same pattern when creating a new module:

```rust
fn test_app() -> App {
    let mut app = App::new();
    app.cached_cell_size = tide_core::Size::new(8.0, 16.0);
    app.window_size = (960, 640);
    app
}

fn app_with_editor() -> (App, u64) {
    let mut app = test_app();
    let (layout, id) = tide_layout::SplitLayout::with_initial_pane();
    app.layout = layout;
    let pane = EditorPane::new_empty(id);
    app.panes.insert(id, PaneKind::Editor(pane));
    app.focused = Some(id);
    app.focus_area = FocusArea::PaneArea;
    (app, id)
}
```

### Step 5: Assert invariants

Always verify relevant invariants at the end of your test:

```rust
// PaneId sync invariant
assert!(app.panes.contains_key(&new_id));
assert!(app.layout.pane_ids().contains(&new_id));

// Modal exclusivity invariant
assert!(app.modal.file_finder.is_none() || app.modal.git_switcher.is_none());
```

## Running Tests

```bash
# All behavior tests
cargo test -p tide-app behavior_tests

# Specific module
cargo test -p tide-app behavior_tests::pane_lifecycle

# Specific test
cargo test -p tide-app closing_a_dirty_editor_with_file_shows_save_confirm
```

## New Feature Checklist

When implementing a new feature:

- [ ] Does `docs/glossary.md` need new terms? Added them?
- [ ] Which Bounded Context (crate) does this belong to?
- [ ] Did you write behavior test scenarios first?
- [ ] Are test names natural language sentences?
- [ ] Do tests assert relevant invariants?
- [ ] Do existing tests still pass? (`cargo test -p tide-app`)
