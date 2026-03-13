// LSP integration: wires tide-lsp into the App event loop.
// Spec: docs/specs/lsp-completion.md

use tide_core::PaneId;
use tide_lsp::manager::Language;

use crate::editor_pane::completion::{CompletionItem, CompletionKind, CompletionState};
use crate::pane::PaneKind;
use crate::App;

impl App {
    /// Initialize the LSP manager with the current working directory.
    pub(crate) fn init_lsp(&mut self) {
        let root = std::env::current_dir().unwrap_or_default();
        self.lsp = Some(tide_lsp::LspManager::new(root, self.event_loop_waker.clone()));
    }

    /// Notify the LSP that a file was opened in an editor pane.
    pub(crate) fn notify_lsp_did_open(&mut self, pane_id: PaneId) {
        let (uri, lang, text) = {
            let pane = match self.panes.get(&pane_id) {
                Some(PaneKind::Editor(p)) => p,
                _ => return,
            };
            let path = match pane.editor.file_path() {
                Some(p) => p,
                None => return,
            };
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let lang = match Language::from_extension(ext) {
                Some(l) => l,
                None => return,
            };
            let uri = tide_lsp::manager::path_to_uri(path);
            let text = pane.editor.buffer.lines.join("\n");
            (uri, lang, text)
        };
        if let Some(ref mut lsp) = self.lsp {
            lsp.did_open(&uri, lang, &text);
        }
    }

    /// Notify the LSP that a document changed.
    pub(crate) fn notify_lsp_did_change(&mut self, pane_id: PaneId) {
        let (uri, text) = {
            let pane = match self.panes.get(&pane_id) {
                Some(PaneKind::Editor(p)) => p,
                _ => return,
            };
            let path = match pane.editor.file_path() {
                Some(p) => p,
                None => return,
            };
            let uri = tide_lsp::manager::path_to_uri(path);
            let text = pane.editor.buffer.lines.join("\n");
            (uri, text)
        };
        if let Some(ref mut lsp) = self.lsp {
            lsp.did_change(&uri, &text);
        }
    }

    /// Notify the LSP that a file was saved.
    pub(crate) fn notify_lsp_did_save(&mut self, pane_id: PaneId) {
        let uri = {
            let pane = match self.panes.get(&pane_id) {
                Some(PaneKind::Editor(p)) => p,
                _ => return,
            };
            match pane.editor.file_path() {
                Some(p) => tide_lsp::manager::path_to_uri(p),
                None => return,
            }
        };
        if let Some(ref mut lsp) = self.lsp {
            lsp.did_save(&uri);
        }
    }

    /// Notify the LSP that a file was closed.
    pub(crate) fn notify_lsp_did_close(&mut self, pane_id: PaneId) {
        let uri = {
            let pane = match self.panes.get(&pane_id) {
                Some(PaneKind::Editor(p)) => p,
                _ => return,
            };
            match pane.editor.file_path() {
                Some(p) => tide_lsp::manager::path_to_uri(p),
                None => return,
            }
        };
        if let Some(ref mut lsp) = self.lsp {
            lsp.did_close(&uri);
        }
    }

    /// Check if the typed text should trigger completion.
    /// Triggers on: (1) trigger characters (e.g. `.`), (2) word characters for continuous completion.
    pub(crate) fn try_trigger_completion(&mut self, pane_id: PaneId, text: &str) {
        let last_char = match text.chars().last() {
            Some(ch) => ch,
            None => return,
        };

        let (uri, line, character, trigger_kind, trigger_char) = {
            let pane = match self.panes.get(&pane_id) {
                Some(PaneKind::Editor(p)) => p,
                _ => return,
            };
            let path = match pane.editor.file_path() {
                Some(p) => p,
                None => return,
            };
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            let lang = match Language::from_extension(ext) {
                Some(l) => l,
                None => return,
            };

            // Check if it's a trigger character
            let triggers = self.lsp.as_ref()
                .map(|lsp| lsp.trigger_characters(lang).to_vec())
                .unwrap_or_default();
            let s = last_char.to_string();
            let is_trigger = triggers.iter().any(|t| t == &s);
            let is_word_char = last_char.is_alphanumeric() || last_char == '_';

            if !is_trigger && !is_word_char {
                return;
            }

            let (kind, tchar) = if is_trigger {
                (tide_lsp::protocol::COMPLETION_TRIGGER_CHARACTER, Some(s))
            } else {
                (tide_lsp::protocol::COMPLETION_TRIGGER_INVOKED, None)
            };

            let uri = tide_lsp::manager::path_to_uri(path);
            let pos = pane.editor.cursor_position();
            let char_col = if let Some(line_text) = pane.editor.buffer.line(pos.line) {
                let byte_col = pos.col.min(line_text.len());
                line_text[..byte_col].chars().count() as u32
            } else {
                0
            };
            (uri, pos.line as u32, char_col, kind, tchar)
        };

        if let Some(ref mut lsp) = self.lsp {
            lsp.request_completion(
                &uri,
                line,
                character,
                trigger_kind,
                trigger_char.as_deref(),
            );
        }
    }

    /// Explicitly trigger completion (Ctrl+Space).
    pub(crate) fn trigger_completion_explicit(&mut self, pane_id: PaneId) {
        let (uri, line, character) = {
            let pane = match self.panes.get(&pane_id) {
                Some(PaneKind::Editor(p)) => p,
                _ => return,
            };
            let path = match pane.editor.file_path() {
                Some(p) => p,
                None => return,
            };
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if Language::from_extension(ext).is_none() {
                return;
            }

            let uri = tide_lsp::manager::path_to_uri(path);
            let pos = pane.editor.cursor_position();
            let char_col = if let Some(line_text) = pane.editor.buffer.line(pos.line) {
                let byte_col = pos.col.min(line_text.len());
                line_text[..byte_col].chars().count() as u32
            } else {
                0
            };
            (uri, pos.line as u32, char_col)
        };

        if let Some(ref mut lsp) = self.lsp {
            lsp.request_completion(
                &uri, line, character,
                tide_lsp::protocol::COMPLETION_TRIGGER_INVOKED,
                None,
            );
        }
    }

    /// Poll the LSP manager for completion responses. Call from the event loop.
    pub(crate) fn poll_lsp(&mut self) -> bool {
        let response = {
            match self.lsp.as_mut() {
                Some(lsp) => lsp.poll(),
                None => None,
            }
        };

        if let Some(response) = response {
            // Find the editor pane that has this URI open
            let pane_id = self.find_pane_by_uri(&response.uri);
            if let Some(pane_id) = pane_id {
                let items: Vec<CompletionItem> = response.items.into_iter().map(|item| {
                    CompletionItem {
                        label: item.label,
                        kind: lsp_kind_to_completion_kind(item.kind),
                        insert_text: item.insert_text,
                        sort_text: item.sort_text,
                        filter_text: item.filter_text,
                    }
                }).collect();

                if !items.is_empty() {
                    if let Some(PaneKind::Editor(pane)) = self.panes.get_mut(&pane_id) {
                        let pos = pane.editor.cursor_position();
                        // Extract the word prefix at cursor (text from word start to cursor)
                        let prefix = if let Some(line_text) = pane.editor.buffer.line(pos.line) {
                            let byte_col = pos.col.min(line_text.len());
                            let before_cursor = &line_text[..byte_col];
                            // Walk backwards to find word start
                            let word_start = before_cursor.rfind(|ch: char| !ch.is_alphanumeric() && ch != '_')
                                .map(|i| i + before_cursor[i..].chars().next().map(|c| c.len_utf8()).unwrap_or(1))
                                .unwrap_or(0);
                            before_cursor[word_start..].to_string()
                        } else {
                            String::new()
                        };
                        let mut cs = CompletionState::new(items, pos.line, pos.col);
                        cs.prefix = prefix;
                        cs.apply_filter();
                        if !cs.is_empty() {
                            pane.completion = Some(cs);
                        }
                        self.cache.invalidate_pane(pane_id);
                    }
                }
            }
            return true;
        }
        false
    }

    fn find_pane_by_uri(&self, uri: &str) -> Option<PaneId> {
        for (&id, pane) in &self.panes {
            if let PaneKind::Editor(ep) = pane {
                if let Some(path) = ep.editor.file_path() {
                    if tide_lsp::manager::path_to_uri(path) == uri {
                        return Some(id);
                    }
                }
            }
        }
        None
    }
}

fn lsp_kind_to_completion_kind(kind: Option<u32>) -> CompletionKind {
    match tide_lsp::protocol::lsp_kind_to_u8(kind) {
        0 => CompletionKind::Function,
        1 => CompletionKind::Variable,
        2 => CompletionKind::Field,
        3 => CompletionKind::Type,
        4 => CompletionKind::Module,
        5 => CompletionKind::Keyword,
        6 => CompletionKind::Snippet,
        7 => CompletionKind::Property,
        8 => CompletionKind::Method,
        9 => CompletionKind::Constant,
        _ => CompletionKind::Other,
    }
}
