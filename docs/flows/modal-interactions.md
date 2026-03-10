# Flow: Modal Interactions

How modals intercept input, enforce exclusivity, and return control.

## Participants

| Context | Role |
|---------|------|
| `tide-app` | ModalStack owns all modal state |
| `tide-input` | Router is BYPASSED when modal is open |

## ModalStack Invariant

**At most one modal is open at a time.**

```rust
ModalStack {
    file_finder: Option<FileFinderState>,
    git_switcher: Option<GitSwitcherState>,
    config_page: Option<ConfigPageState>,
    save_confirm: Option<SaveConfirmState>,
    save_as_input: Option<SaveAsInput>,
    context_menu: Option<ContextMenuState>,
    file_tree_rename: Option<FileTreeRenameState>,
    branch_cleanup: Option<BranchCleanupState>,
}
```

`is_any_open()` returns true if any field is `Some`.

## Input Interception Priority

When `handle_key_down()` runs, modals are checked **before** the Router:

```
KeyDown event
    │
    ├─1─ config_page open?     → config page handles it (highest priority)
    ├─2─ context_menu open?    → ESC closes, click selects
    ├─3─ save_confirm open?    → Y saves+closes, N force-closes, ESC cancels
    ├─4─ save_as_input open?   → text input to filename, Enter saves, ESC cancels
    ├─5─ file_finder open?     → text input to filter, ↑↓ navigate, Enter opens, ESC closes
    ├─6─ git_switcher open?    → text input to filter, ↑↓ navigate, Enter switches, ESC closes
    ├─7─ file_tree_rename?     → text input to name, Enter confirms, ESC cancels
    │
    └─── None open → proceeds to FocusArea dispatch → Router
```

**Critical**: If a modal consumes the event, the function RETURNS immediately.
Input never reaches the Router or panes.

## Flow: File Finder (Shift+Shift)

```
Double-tap Shift detected (ModifiersChanged timing)
    │
    ▼
modal.file_finder = Some(FileFinderState::new())
    │
    ▼
User types "mai"
    │
    ├── handle_key_down() → file_finder is open
    │   ├── 'm' → input.push('m'), update_results()
    │   ├── 'a' → input.push('a'), update_results()
    │   └── 'i' → input.push('i'), update_results()
    │
    ▼
User presses Enter
    │
    ├── Selected result: "src/main.rs"
    ├── open_editor_pane("src/main.rs")
    └── modal.file_finder = None  (modal closed)
```

## Flow: Save Confirm

```
User presses Cmd+W on dirty editor with file path
    │
    ▼
close_specific_pane(id)
    │
    ├── Dirty + has file? → YES
    │   └── modal.save_confirm = Some(SaveConfirmState { pane_id: id })
    │       RETURN (pane NOT closed yet)
    │
    ▼
User sees: "Save changes? (Y)es / (N)o / (Esc)ape"
    │
    ├── Y → editor.save() + force_close_editor_panel_tab(id)
    ├── N → force_close_editor_panel_tab(id) (discard changes)
    └── ESC → modal.save_confirm = None (cancel, keep pane)
```

## Flow: Context Menu (Right-click)

```
Right-click on file tree item
    │
    ├── modal.context_menu = Some(ContextMenuState { items, position })
    │
    ▼
User clicks menu item (or presses number key)
    │
    ├── Execute selected action (copy path, delete, rename, etc.)
    └── modal.context_menu = None
```

## ESC Closes Everything

Every modal responds to ESC:

```
handle_key_down(Escape, ...)
    │
    ├── config_page open?     → config_page = None
    ├── context_menu open?    → context_menu = None
    ├── save_confirm open?    → save_confirm = None (cancel)
    ├── save_as_input open?   → save_as_input = None
    ├── file_finder open?     → file_finder = None
    ├── git_switcher open?    → git_switcher = None
    └── file_tree_rename?     → file_tree_rename = None
```

## Related Behavior Tests

```
mod modal_behavior:
  - new_app_has_no_modals_open
  - close_all_dismisses_every_modal
  - config_page_blocks_all_text_input
  - context_menu_blocks_text_input
  - save_confirm_blocks_text_input
  - file_finder_captures_text_instead_of_pane
  - git_switcher_captures_text_instead_of_pane
  - modals_have_higher_priority_than_search_bar
  - config_page_has_highest_priority_over_all_modals
  - escape_in_keyboard_handler_closes_file_finder
  - escape_closes_git_switcher
  - escape_closes_save_as_input
  - escape_closes_context_menu
  - escape_cancels_save_confirm
  - escape_closes_file_tree_rename
```
