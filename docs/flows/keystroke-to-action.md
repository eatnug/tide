# Flow: Keystroke → Action

How a physical keypress becomes a state mutation.

## Participants

| Context | Role |
|---------|------|
| `tide-platform` | Captures OS event, translates to PlatformEvent |
| `tide-app` | Routes through modal/focus/router chain |
| `tide-input` | Matches Hotkey → GlobalAction |
| `tide-terminal` / `tide-editor` | Receives text input |

## Sequence

```
User presses Cmd+T
    │
    ▼
┌─ tide-platform ─────────────────────────────────┐
│ NSView.keyDown: → interpretKeyEvents:            │
│ Emits PlatformEvent::KeyDown { key: Char('t'),   │
│   modifiers: { meta: true }, chars: Some("t") }  │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ tide-app: handle_platform_event() ─────────────┐
│ Match PlatformEvent::KeyDown →                   │
│   handle_key_down(key, modifiers, chars)         │
└──────────────────────────────────────────────────┘
    │
    ▼
┌─ tide-app: handle_key_down() ───────────────────┐
│                                                  │
│ 1. Is config_page open? → NO                     │
│ 2. Is context_menu open? → NO                    │
│ 3. Is save_confirm open? → NO                    │
│ 4. Is save_as_input open? → NO                   │
│ 5. Is file_finder open? → NO                     │
│ 6. Is git_switcher open? → NO                    │
│ 7. Is file_tree_rename open? → NO                │
│ 8. FocusArea == FileTree? → NO                   │
│ 9. Router.process(KeyPress { Char('t'), meta })  │
│    ↓                                             │
│    Hotkey match: Cmd+T = GlobalAction::NewTab    │
│    Returns Action::GlobalAction(NewTab)          │
│                                                  │
│ 10. handle_action(NewTab)                        │
│     → new_terminal_tab()                         │
│     → Creates Launcher pane, adds to TabGroup    │
│     → Sets focus to new pane                     │
│     → invalidate_chrome()                        │
└──────────────────────────────────────────────────┘
```

## Alternative Path: Plain Text Input

```
User presses 'a' (no modifiers)
    │
    ▼
handle_key_down()
    │
    ├── Modal checks → all NO
    ├── FocusArea? → PaneArea
    ├── Router.process(KeyPress { Char('a'), none })
    │   → No hotkey match
    │   → Action::RouteToPane(focused_id)
    │
    ▼
send_text_to_target(focused_id, "a")
    │
    ├── If Terminal → terminal.backend.write(b"a")  (sends to PTY)
    └── If Editor → editor.handle_action(InsertChar('a'))
```

## Alternative Path: Modal Intercepts

```
User presses 'a' while file_finder is open
    │
    ▼
handle_key_down()
    │
    ├── Is file_finder open? → YES
    │   → file_finder.input.push('a')
    │   → file_finder.update_results()
    │   → RETURN (input consumed, never reaches Router)
```

## Related Behavior Tests

```
mod keyboard_routing:
  - cmd_t_triggers_new_tab_action
  - cmd_w_triggers_close_pane_action
  - unmodified_keys_route_to_focused_pane

mod modal_behavior:
  - file_finder_captures_text_instead_of_pane
  - config_page_blocks_all_text_input
  - modals_have_higher_priority_than_search_bar
```
