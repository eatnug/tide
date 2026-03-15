# Spec: Terminal Context

## Overview

### As-Is

- Every Pane determines its own context independently
- File tree root is determined by `focused_terminal_cwd()`, which falls back to the first terminal in layout order when a non-terminal Pane is focused
- Opening a file from the file tree always calls `layout.add_tab(focused_id, new_id)`, adding to the focused Pane's TabGroup regardless of its PaneKind
- This causes two bugs:
  1. Focusing an Editor opened from a different directory resets the file tree to the first terminal's cwd
  2. Opening a file while an Editor is focused creates a new split instead of adding to the existing non-terminal TabGroup

### To-Be

- Terminal is the **context provider** for all non-terminal Panes
- Each non-terminal Pane has an `associated_terminal: Option<PaneId>` pointing to its context terminal
- TabGroups are **kind-constrained**: a terminal TabGroup only holds terminals; non-terminal TabGroups hold Editor/Browser/Diff/Launcher
- File tree root follows the focused Pane's associated terminal cwd
- Opening a file routes to the correct non-terminal TabGroup, not the focused terminal's TabGroup
- When a terminal is closed (soft delete), its cwd data is retained so associated Panes keep their context
- When a terminal moves to another Workspace, its associated Panes move together

### Approach

1. Add `associated_terminal: Option<PaneId>` field to non-terminal Panes
2. Add `retained_contexts: HashMap<PaneId, PathBuf>` to App for soft-deleted terminal cwd retention
3. Enforce TabGroup kind constraint: reject adding a terminal to a non-terminal TabGroup and vice versa
4. Update `focused_terminal_cwd()` to resolve via `associated_terminal` chain
5. Update `open_editor_pane()` to find the correct non-terminal TabGroup
6. Update `close_pane()` to soft-delete terminals into `retained_contexts`
7. Update `move_pane_to_workspace()` to move associated Panes together

## Bounded Contexts

| Context | Role |
|---------|------|
| `tide-app` | Manages terminal association, retained context terminals, file open routing |
| `tide-layout` | TabGroup kind constraint enforcement |
| `tide-core` | PaneId, PaneKind types |

## Use Cases

### UC-1: AssociateTerminal

- **Actor**: System
- **Trigger**: A non-terminal Pane is created (ResolveLauncher, OpenFile, NewFile)
- **Precondition**: A terminal is focused or reachable
- **Flow**:
  1. Determine context terminal:
     - If focused Pane is a terminal → use it
     - If focused Pane is a non-terminal → use its `associated_terminal`
  2. Set `associated_terminal = Some(terminal_id)` on the new Pane
- **Postcondition**: New Pane has a terminal association
- **Business Rules**:
  - BR-1: Non-terminal Pane always inherits the context terminal from the creation context
  - BR-2: If no terminal is reachable, `associated_terminal` is None

### UC-2: ResolveFileTreeRoot

- **Actor**: System
- **Trigger**: Pane focus changes
- **Precondition**: File tree is visible
- **Flow**:
  1. Get focused Pane's associated terminal (or itself if it's a terminal)
  2. Look up terminal's cwd (live terminal or retained_contexts)
  3. Set file tree root to that cwd (or git repo root)
- **Postcondition**: File tree shows the correct directory for the focused Pane's context
- **Business Rules**:
  - BR-3: Focusing a terminal → file tree shows that terminal's cwd
  - BR-4: Focusing a non-terminal → file tree shows its associated terminal's cwd
  - BR-5: If associated terminal is a retained context → file tree shows retained context's last known cwd
  - BR-6: If no association exists → file tree stays unchanged (last_cwd)

### UC-3: OpenFileRouting

- **Actor**: User
- **Trigger**: Select file in FileTree or FileFinder
- **Precondition**: File path is valid, not already open
- **Flow**:
  1. Find a non-terminal TabGroup:
     a. If a non-terminal TabGroup exists → use the most recently focused one
     b. If none exists → create a new split next to the focused terminal
  2. Add new Editor Pane as a tab in the target TabGroup
  3. Set `associated_terminal` from creation context (UC-1)
- **Postcondition**: File opened in a non-terminal TabGroup
- **Business Rules**:
  - BR-7: Files never open in a terminal TabGroup
  - BR-8: If a non-terminal TabGroup exists, reuse it (no new split)
  - BR-9: If multiple non-terminal TabGroups exist, use the most recently focused one

### UC-4: TabGroupKindConstraint

- **Actor**: System
- **Trigger**: Any operation that adds a Pane to a TabGroup (add_tab, drag-drop)
- **Precondition**: Target TabGroup exists
- **Flow**:
  1. Determine TabGroup kind from its existing members
  2. If new Pane's kind conflicts → reject or redirect
- **Postcondition**: TabGroup homogeneity maintained
- **Business Rules**:
  - BR-10: A TabGroup containing terminals only accepts terminals
  - BR-11: A TabGroup containing non-terminals only accepts non-terminals
  - BR-12: An empty TabGroup (Launcher only) accepts any kind; first concrete Pane sets the kind

### UC-5: CloseTerminal (Soft Delete)

- **Actor**: User
- **Trigger**: GlobalAction::ClosePane on a terminal Pane
- **Precondition**: Terminal Pane exists
- **Flow**:
  1. Record terminal's cwd in `retained_contexts[pane_id] = cwd`
  2. Remove terminal from layout and panes (normal close flow)
  3. Associated Panes retain their `associated_terminal` pointing to the retained context
- **Postcondition**: Terminal removed from UI, cwd preserved for associated Panes
- **Business Rules**:
  - BR-13: Closing a terminal preserves its cwd in retained_contexts
  - BR-14: Associated Panes' `associated_terminal` is NOT cleared on terminal close
  - BR-15: Ghost terminal is cleaned up when all its associated Panes are also closed

### UC-6: MoveTerminalToWorkspace

- **Actor**: User
- **Trigger**: Drag terminal Pane to Workspace sidebar
- **Precondition**: Terminal Pane has associated non-terminal Panes
- **Flow**:
  1. Collect all Panes with `associated_terminal == moving_terminal_id`
  2. Move the terminal and all associated Panes to the target Workspace
  3. Rebuild layout in target Workspace with the moved Panes
- **Postcondition**: Terminal and associated Panes moved together
- **Business Rules**:
  - BR-16: Moving a terminal to another Workspace moves all its associated Panes together
  - BR-17: Moving a non-terminal Pane alone does NOT move its associated terminal

## Invariants

1. **TabGroup homogeneity**: A TabGroup contains either only terminals or only non-terminals (Launcher is neutral until resolved)
2. **Ghost cleanup**: retained_contexts entry is removed when no Pane references it
3. **Association consistency**: `associated_terminal` points to either a live terminal PaneId or a retained_contexts key

## Tests

| UC | BR | Test |
|----|-----|------|
| UC-1 | BR-1 | `new_editor_inherits_associated_terminal_from_focused_terminal` |
| UC-1 | BR-1 | `new_editor_inherits_associated_terminal_from_focused_editor` |
| UC-1 | BR-2 | `pane_created_without_terminal_has_no_association` |
| UC-2 | BR-3 | `focusing_terminal_sets_file_tree_to_its_cwd` |
| UC-2 | BR-4 | `focusing_editor_sets_file_tree_to_associated_terminal_cwd` |
| UC-2 | BR-5 | `focusing_editor_with_retained context_terminal_uses_retained context_cwd` |
| UC-2 | BR-6 | `focusing_pane_without_association_keeps_file_tree_unchanged` |
| UC-3 | BR-7 | `open_file_never_adds_to_terminal_tab_group` |
| UC-3 | BR-8 | `open_file_reuses_existing_non_terminal_tab_group` |
| UC-3 | BR-9 | `open_file_uses_most_recently_focused_non_terminal_tab_group` |
| UC-4 | BR-10 | `adding_editor_to_terminal_tab_group_is_rejected` |
| UC-4 | BR-11 | `adding_terminal_to_editor_tab_group_is_rejected` |
| UC-5 | BR-13 | `closing_terminal_preserves_cwd_in_retained_contexts` |
| UC-5 | BR-14 | `associated_panes_retain_retained context_terminal_reference_after_close` |
| UC-5 | BR-15 | `retained context_terminal_cleaned_up_when_all_associated_panes_closed` |
| UC-6 | BR-16 | `moving_terminal_to_workspace_moves_associated_panes_together` |
| UC-6 | BR-17 | `moving_non_terminal_pane_alone_does_not_move_associated_terminal` |

## Location

| Layer | Crate | Key Files |
|-------|-------|-----------|
| Orchestrator | tide-app | `action/pane_lifecycle.rs`, `file_tree.rs`, `pane.rs` |
| Layout | tide-layout | `split_layout.rs`, `tab_group.rs` |
| Tests | tide-app | `behavior_tests.rs :: mod terminal_context` |
