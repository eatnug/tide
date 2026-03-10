## Summary

<!-- 1-3 sentences. What does this PR do and why? -->

## Bounded Context

<!-- Which crate(s) does this PR primarily affect? Check all that apply. -->

- [ ] `tide-core` — Shared types & traits
- [ ] `tide-layout` — SplitLayout, TabGroup
- [ ] `tide-terminal` — Terminal, PTY, GridSyncer
- [ ] `tide-editor` — EditorState, text buffer
- [ ] `tide-input` — Router, Hotkey, GlobalAction
- [ ] `tide-tree` — FsTree, filesystem
- [ ] `tide-platform` — PlatformEvent, native macOS
- [ ] `tide-renderer` — WgpuRenderer, GPU pipeline
- [ ] `tide-app` — App orchestrator

## Domain Changes

<!-- Use terms from docs/glossary.md. Be specific about what changed. -->

**Entities/Aggregates modified:**
<!-- e.g., "Added `preview_mode` field to EditorPane entity" -->

**Invariants preserved:**
<!-- e.g., "PaneId sync maintained — new pane added to both SplitLayout and App.panes" -->

**Invariants changed:**
<!-- e.g., "ModalStack now allows two concurrent modals (save_confirm + context_menu)" -->
<!-- Write "None" if no invariants changed -->

## Behavior Tests

<!-- List the behavior tests added or modified. Test name = natural language spec. -->

```
mod pane_lifecycle:
  - closing_last_pane_in_workspace_shows_launcher
  - ...
```

## How to Test

<!-- Manual testing steps if behavior tests don't cover everything -->
