# Spec: LSP Completion

Inline code completion powered by Language Server Protocol.

## Overview

### As-Is

Editor panes have syntax highlighting (syntect) but no code intelligence. No LSP infrastructure exists.
Users must rely on external tools for code completion.

### To-Be

As the user types in an Editor pane, a CompletionPopup appears below the cursor showing relevant
suggestions from a language server. The user can navigate, accept, or dismiss completions with keyboard.

Supported languages (v1): TypeScript, Python, Rust, Go.

### Approach

1. **New crate `tide-lsp`**: LSP client that manages language server processes and JSON-RPC communication.
   - One background thread per language server instance (follows git poller pattern: std::thread + mpsc + WakeCallback)
   - Uses `lsp-types` crate for protocol types, manual JSON-RPC serialization (no tokio/async)
   - Full document sync (TextDocumentSyncKind::Full) for v1

2. **CompletionPopup on EditorPane**: Not part of ModalStack (must coexist with typing).
   - `EditorPane.completion: Option<CompletionState>` — per-pane completion state
   - Rendered in overlay layer, positioned at cursor
   - Input routing: intercepts Up/Down/Tab/Enter/Esc when active

3. **Integration in tide-app**:
   - LspManager holds all LspClient instances (one per language)
   - Editor buffer changes → didChange notification to LSP
   - Character typed → completion request (or client-side filter of existing items)
   - Poll LSP responses in event loop (same as git poller / file watcher)

## Bounded Contexts

| Context | Role |
|---------|------|
| `tide-lsp` (new) | LSP client: process lifecycle, JSON-RPC, document sync, request/response |
| `tide-editor` | Provides buffer content and cursor position for LSP sync |
| `tide-app` | CompletionPopup UI, LspManager orchestration, input routing, rendering |
| `tide-core` | Shared types (may add CompletionItem if needed cross-crate) |

## Use Cases

### UC-1: StartLanguageServer

- **Actor**: System
- **Trigger**: User opens a file with a recognized language extension in an Editor pane
- **Precondition**: No LspClient is running for that language
- **Flow**:
  1. Detect language from file extension (e.g., `.ts` → TypeScript, `.py` → Python, `.rs` → Rust, `.go` → Go)
  2. Look up server command (e.g., `typescript-language-server --stdio`)
  3. Check if server binary exists in PATH
  4. Spawn server process with stdio transport
  5. Send `initialize` request, wait for `initialized` response
  6. Send `textDocument/didOpen` for the opened file
- **Postcondition**: LspClient running, server initialized, document registered
- **Business Rules**:
  - BR-1: Language server starts only when a file of that language is first opened
  - BR-2: If server binary is not found in PATH, silently skip (no error to user)
  - BR-3: Only one server per language runs at a time
  - BR-4: Server receives workspace root as rootUri (cwd of Tide)

### UC-2: SyncDocument

- **Actor**: System
- **Trigger**: User modifies buffer in an Editor pane that has an active LspClient
- **Precondition**: LspClient running for this file's language
- **Flow**:
  1. After buffer mutation, send `textDocument/didChange` with full document content
  2. On file save, send `textDocument/didSave`
  3. On editor pane close, send `textDocument/didClose`
- **Postcondition**: Language server has up-to-date document state
- **Business Rules**:
  - BR-5: didChange sends full content (TextDocumentSyncKind::Full)
  - BR-6: didChange is debounced — coalesce rapid keystrokes (e.g., 50ms after last keystroke)
  - BR-7: didOpen sent when a file is opened and server is already running
  - BR-8: didClose sent when editor pane is closed

### UC-3: ShowCompletion

- **Actor**: User
- **Trigger**: User types a character in Editor, or presses Ctrl+Space
- **Precondition**: Editor pane focused, LspClient running for this language
- **Flow**:
  1. **Trigger check**: character is a trigger character (`.`, `:`, etc. from server capabilities) or Ctrl+Space
  2. Send `textDocument/completion` request with current cursor position
  3. Server responds with completion items
  4. CompletionPopup opens below cursor with filtered items
  5. If user continues typing within the same word, filter items client-side (no new LSP request)
  6. If user types a new trigger character, send new completion request
- **Postcondition**: CompletionPopup visible with relevant items
- **Business Rules**:
  - BR-9: Completion triggers on server-defined trigger characters (e.g. `.`, `:`)
  - BR-9a: When client-side filter produces zero matches AND the dismissing character is a trigger character, a new completion request is sent (not silently dismissed)
  - BR-9b: `didChange` must be sent to the LSP server BEFORE `completion` request (server needs updated buffer)
  - BR-10: Ctrl+Space triggers completion explicitly at any position
  - BR-11: Continued typing within same prefix filters existing items client-side
  - BR-12: CompletionPopup shows max 10 visible items with scroll
  - BR-13: Items sorted by server-provided sortText, then label
  - BR-14: Each item shows: label + kind abbreviation (fn, var, typ, mod, kw, snip, etc.)
  - BR-15: CompletionPopup positioned below cursor line; flips above if insufficient space below

### UC-4: NavigateCompletion

- **Actor**: User
- **Trigger**: Up/Down arrow keys while CompletionPopup is open
- **Precondition**: CompletionPopup is visible with items
- **Flow**:
  1. Up → select previous item (wrap to bottom at top)
  2. Down → select next item (wrap to top at bottom)
  3. Selected item scrolls into view
- **Postcondition**: Selection updated, popup scrolled if needed
- **Business Rules**:
  - BR-16: Down selects next item
  - BR-17: Up selects previous item
  - BR-18: Selection wraps around (bottom → top, top → bottom)
  - BR-19: Popup auto-scrolls to keep selected item visible

### UC-5: AcceptCompletion

- **Actor**: User
- **Trigger**: Tab or Enter while CompletionPopup is open
- **Precondition**: CompletionPopup has a selected item
- **Flow**:
  1. Get selected CompletionItem's insertText (or label if no insertText)
  2. Delete the current word prefix (characters typed since completion trigger)
  3. Insert completion text at cursor position
  4. Close CompletionPopup
- **Postcondition**: Completion text inserted, popup closed
- **Business Rules**:
  - BR-20: Tab accepts selected completion
  - BR-21: Enter accepts selected completion
  - BR-22: Inserted text replaces the typed prefix, not appended after it
  - BR-23: Cursor moves to end of inserted text
  - BR-24: If completionItem has textEdit, use that instead of insertText

### UC-6: DismissCompletion

- **Actor**: User
- **Trigger**: Escape, cursor movement (arrow left/right, click), or no matching items
- **Precondition**: CompletionPopup is open
- **Flow**:
  1. Close CompletionPopup
  2. Return input routing to normal Editor handling
- **Postcondition**: CompletionPopup closed
- **Business Rules**:
  - BR-25: Escape dismisses completion
  - BR-26: Moving cursor (left/right arrows, mouse click) dismisses completion
  - BR-27: Completion dismissed when filter matches zero items
  - BR-28: Switching pane or opening a modal dismisses completion

### UC-7: StopLanguageServer

- **Actor**: System
- **Trigger**: All editor panes of a language are closed, or app shutdown
- **Precondition**: LspClient is running for that language
- **Flow**:
  1. Send `shutdown` request to server
  2. Wait for response (with timeout)
  3. Send `exit` notification
  4. Kill process if still alive after timeout
- **Postcondition**: Server process terminated, LspClient removed
- **Business Rules**:
  - BR-29: Server stopped when last file of that language is closed
  - BR-30: Server stopped gracefully on app shutdown (shutdown → exit)
  - BR-31: Server force-killed after 3s timeout if shutdown hangs

## Invariants

1. **CompletionPopup independence**: CompletionPopup is NOT in ModalStack — it is per-EditorPane state and does not enforce modal exclusivity
2. **Input routing priority**: When CompletionPopup is active: Modal → CompletionPopup → FocusArea → Router → TextInput
3. **Document sync consistency**: If an LspClient is running for a language, every open Editor of that language has sent didOpen
4. **Single server per language**: At most one LspClient per language type exists in LspManager
5. **No crash on missing server**: Missing server binary never causes an error visible to the user

## Tests

| UC | BR | Test |
|----|-----|------|
| UC-1 | BR-1 | `language_server_starts_on_first_file_open` |
| UC-1 | BR-2 | `missing_server_binary_silently_skipped` |
| UC-1 | BR-3 | `only_one_server_per_language` |
| UC-2 | BR-5 | `did_change_sends_full_content` |
| UC-2 | BR-7 | `did_open_sent_when_server_already_running` |
| UC-2 | BR-8 | `did_close_sent_when_editor_closes` |
| UC-3 | BR-9a | `trigger_char_after_filter_dismiss_sends_new_request` |
| UC-3 | BR-9b | `did_change_sent_before_completion_request` |
| UC-3 | BR-10 | `ctrl_space_triggers_completion` |
| UC-3 | BR-11 | `typing_filters_existing_completions_client_side` |
| UC-3 | BR-12 | `completion_popup_shows_max_ten_items` |
| UC-3 | BR-15 | `completion_popup_flips_above_when_near_bottom` |
| UC-4 | BR-16 | `down_selects_next_completion_item` |
| UC-4 | BR-17 | `up_selects_previous_completion_item` |
| UC-4 | BR-18 | `completion_selection_wraps_around` |
| UC-5 | BR-20 | `tab_accepts_selected_completion` |
| UC-5 | BR-21 | `enter_accepts_selected_completion` |
| UC-5 | BR-22 | `accepted_completion_replaces_typed_prefix` |
| UC-6 | BR-25 | `escape_dismisses_completion` |
| UC-6 | BR-26 | `cursor_movement_dismisses_completion` |
| UC-6 | BR-27 | `completion_dismissed_when_no_matches` |
| UC-6 | BR-28 | `switching_pane_dismisses_completion` |
| UC-7 | BR-29 | `server_stops_when_last_file_closes` |
| UC-7 | BR-30 | `server_stops_gracefully_on_app_shutdown` |

## Location

| Layer | Crate | Key Files |
|-------|-------|-----------|
| LspClient | tide-lsp (new) | `lib.rs`, `client.rs`, `transport.rs`, `protocol.rs` |
| CompletionPopup state | tide-app | `editor_pane/completion.rs` (new) |
| CompletionPopup rendering | tide-app | `rendering/overlays.rs` |
| Input routing | tide-app | `event_handler/keyboard.rs` |
| LspManager | tide-app | `lsp_manager.rs` (new) |
| Tests | tide-app | `behavior_tests.rs :: mod lsp_completion` |
