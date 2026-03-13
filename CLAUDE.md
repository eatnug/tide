# Tide — Project Rules

## Evidence-First (BLOCKING)

**Every factual claim in a response MUST be backed by evidence you gathered in this conversation.**
Evidence means: code you read, search results you got, docs you checked, or something the user told you.
If you have no evidence, you MUST gather it before responding — or explicitly ask the user.

**This is a blocking rule.** Do NOT write a response containing factual claims, then plan to verify afterward. Verify FIRST, respond SECOND.

How to apply:
1. **Before describing how something works** — read the relevant code first
2. **Before claiming something exists or doesn't exist** — search for it first
3. **Before proposing an approach** — understand the current system by reading code first
4. **If no evidence can be found** — say "I don't know" and ask the user

Violations include:
- Describing architecture, threading model, data flow, or patterns without reading the code
- Claiming a feature exists or doesn't exist without searching
- Proposing technical tradeoffs based on assumptions about the codebase
- Saying "currently X works like Y" without having read X

This applies to everything: how code works, what a function does, whether something is used, side effects of a change, external library APIs and their behavior, etc.

## Domain Language (Required)

All code, commits, PRs, and discussions MUST use the terms defined in `docs/glossary.md`.
Before writing code or describing changes, check the glossary. If a term doesn't exist, add it.

Key terms to always use precisely:
- **Pane** (not "panel", "tab", "window") — a content container with a PaneId
- **PaneKind** — the 5 types: Terminal, Editor, Diff, Browser, Launcher
- **Workspace** — an isolated set of panes + layout + focus (not "tab group", "session")
- **TabGroup** — multiple panes stacked in one layout slot (not "workspace")
- **FocusArea** — FileTree or PaneArea (not "focus mode", "focus zone")
- **SplitLayout** — the binary tree of splits (not "grid", "tiling")
- **ModalStack** — mutually-exclusive popups (not "dialog", "overlay")
- **GlobalAction** — a user-intent command from keybinding (not "event", "message")
- **Generation** — monotonic counter for cache invalidation (not "version", "revision")

## Bounded Contexts (Crates)

Each crate is a bounded context. Know which one you're touching:

| Crate | Responsibility | Key Entities |
|-------|---------------|--------------|
| `tide-core` | Shared types & traits | PaneId, Rect, Key, TerminalGrid |
| `tide-layout` | Binary split tree | SplitLayout, TabGroup |
| `tide-terminal` | PTY & grid sync | Terminal, GridSyncer |
| `tide-editor` | Text buffer & cursor | EditorState |
| `tide-input` | Keybinding resolution | Router, Hotkey, GlobalAction |
| `tide-tree` | Filesystem & git status | FsTree |
| `tide-platform` | Native macOS windowing | PlatformEvent, PlatformWindow |
| `tide-renderer` | GPU rendering pipeline | WgpuRenderer, GlyphAtlas |
| `tide-app` | Orchestrator | App, WorkspaceManager, ModalStack |

## Feature Development (MUST)

When adding a new feature or fixing a bug, follow this order. **Do not skip steps or reverse the order.**

```
1. Spec   → Understand the system → Clarify requirements with user → Write spec
2. Test   → Write behavior tests for each Business Rule (crates/tide-app/src/behavior_tests.rs)
3. Code   → Write code that passes the tests
```

- Never skip or reverse this order — even when told "just do it all" or "don't ask questions". Those instructions mean "work autonomously", NOT "skip the process".
- No code without a spec, no implementation without tests
- Same applies when modifying existing specs: spec change → test change → code change
- When a new requirement is discovered mid-implementation, STOP coding and loop back: update spec → add test → then code
- Use domain terms from `docs/glossary.md` when writing specs. Add new terms to glossary first if needed.

### Spec Format (`docs/specs/{feature}.md`)

```markdown
# Spec: {Name}

## Overview
### As-Is             ← Current state and problems (concrete, code-based)
### To-Be             ← Target state after changes
### Approach          ← Step-by-step plan to get there
## Bounded Contexts    ← Related crates
## Use Cases           ← Actor, Trigger, Precondition, Flow, Postcondition, Business Rules
## Invariants          ← Invariants that must hold
## Tests               ← UC ↔ BR ↔ test function mapping table
## Location            ← Code location
```

### Test Conventions

- Test module comment: `// Spec: docs/specs/{feature}.md`
- UC section comment: `// --- UC-N: {Name} ---`
- Each test references its BR: `// UC-N BR-M: {rule description}`
- Test name = natural language sentence: `fn closing_last_pane_in_workspace_shows_launcher()`

### Naming Rule

- Glossary Term = code type name (must match)
- Spec Use Case name = test section comment (must match)
- Business Rule number = referenced in test function comment

See `docs/testing/behavior-tests.md` for the full guide.

## Commit Messages

Format: `<verb> <what> in <bounded-context>`

```
Add pane drag preview in tide-app
Fix TabGroup active index after close in tide-layout
Extract GridSyncer dirty tracking in tide-terminal
```

- Verb: Add (new feature), Fix (bug), Extract (refactor), Remove, Update
- What: Use domain terms from glossary
- Bounded context: Which crate is primarily affected

## PR Description

Follow the template in `.github/PULL_REQUEST_TEMPLATE.md`. Must include:
- Which Spec(s) and Use Case(s) are affected (e.g. `pane-lifecycle UC-5: ClosePane`)
- Which Bounded Context(s) are touched
- Which Entities/Aggregates are modified
- Which Invariants are preserved or changed
- Which behavior tests were added (with BR references)

## Architecture Invariants

These must NEVER be violated:

1. **PaneId sync**: Every PaneId in SplitLayout MUST exist in App.panes HashMap, and vice versa
2. **Single active workspace**: Only the active Workspace is loaded into App fields; others are cold-stored in WorkspaceManager
3. **Modal exclusivity**: At most one modal in ModalStack can be open at a time
4. **Input routing priority**: Modal → FocusArea → Router → TextInput (never skip a level)
5. **Generation monotonicity**: chrome_generation and pane_generations only increase, never decrease or reset **within a workspace session**. Exception: pane_generations is cleared on workspace switch (entirely new pane set)
6. **IME proxy lifecycle**: Every pane with keyboard focus must have an active IME proxy; proxy must be synced on every event

## File Structure

- `docs/glossary.md` — Single source of truth for all domain terms
- `docs/context-map.md` — How bounded contexts relate
- `docs/domain/*.md` — Per-context deep dives
- `docs/specs/*.md` — Use Case specs with Business Rules (testable)
- `docs/testing/behavior-tests.md` — How to write behavior tests
- `crates/tide-app/src/behavior_tests.rs` — Living specification (117+ tests)
