// Search state and logic for Cmd+F search in terminal and editor panes.

use tide_terminal::Terminal;

/// A single match location.
#[derive(Debug, Clone)]
pub struct SearchMatch {
    pub line: usize, // terminal: absolute line from top of scrollback; editor: buffer line index
    pub col: usize,
    pub len: usize,
}

/// Search state for a single pane.
pub struct SearchState {
    pub query: String,
    pub cursor: usize,            // cursor position within query string
    pub matches: Vec<SearchMatch>,
    pub current: Option<usize>,   // index into matches
    pub visible: bool,
}

impl SearchState {
    pub fn new() -> Self {
        Self {
            query: String::new(),
            cursor: 0,
            matches: Vec::new(),
            current: None,
            visible: true,
        }
    }

    pub fn insert_char(&mut self, ch: char) {
        self.query.insert(self.cursor, ch);
        self.cursor += ch.len_utf8();
    }

    pub fn backspace(&mut self) {
        if self.cursor > 0 {
            // Find the previous char boundary
            let prev = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub fn delete_char(&mut self) {
        if self.cursor < self.query.len() {
            let next = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
            self.query.drain(self.cursor..next);
        }
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.query[..self.cursor]
                .char_indices()
                .next_back()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            self.cursor = self.query[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.query.len());
        }
    }

    pub fn next_match(&mut self) {
        if self.matches.is_empty() {
            self.current = None;
            return;
        }
        self.current = Some(match self.current {
            Some(i) => (i + 1) % self.matches.len(),
            None => 0,
        });
    }

    pub fn prev_match(&mut self) {
        if self.matches.is_empty() {
            self.current = None;
            return;
        }
        self.current = Some(match self.current {
            Some(0) => self.matches.len() - 1,
            Some(i) => i - 1,
            None => self.matches.len() - 1,
        });
    }

    /// Display string like "3/42" or "0/0".
    pub fn current_display(&self) -> String {
        match self.current {
            Some(i) => format!("{}/{}", i + 1, self.matches.len()),
            None => format!("0/{}", self.matches.len()),
        }
    }
}

/// Execute search over a terminal's full scrollback + screen.
/// Preserves the current match position across re-executions (e.g., when
/// scrolling triggers a grid refresh).
pub fn execute_search_terminal(state: &mut SearchState, terminal: &Terminal) {
    // Remember current match position so we can restore it
    let prev_pos = state.current
        .and_then(|i| state.matches.get(i))
        .map(|m| (m.line, m.col));

    state.matches.clear();
    state.current = None;

    if state.query.is_empty() {
        return;
    }

    let results = terminal.search_buffer(&state.query);
    state.matches = results
        .into_iter()
        .map(|(line, col, len)| SearchMatch { line, col, len })
        .collect();

    if !state.matches.is_empty() {
        // Try to restore current match to the same position
        state.current = if let Some((line, col)) = prev_pos {
            state.matches.iter()
                .position(|m| m.line == line && m.col == col)
                .or(Some(0))
        } else {
            Some(0)
        };
    }
}

/// Execute search over an editor buffer's lines.
/// Preserves the current match position across re-executions.
pub fn execute_search_editor(state: &mut SearchState, lines: &[String]) {
    let prev_pos = state.current
        .and_then(|i| state.matches.get(i))
        .map(|m| (m.line, m.col));

    state.matches.clear();
    state.current = None;

    if state.query.is_empty() {
        return;
    }

    let query_lower = state.query.to_lowercase();
    let query_char_len = state.query.chars().count();
    for (line_idx, line) in lines.iter().enumerate() {
        let line_lower = line.to_lowercase();
        let mut start = 0;
        while let Some(byte_pos) = line_lower[start..].find(&query_lower) {
            let byte_col = start + byte_pos;
            // Convert byte offset to char column for rendering
            let char_col = line[..byte_col].chars().count();
            state.matches.push(SearchMatch {
                line: line_idx,
                col: char_col,
                len: query_char_len,
            });
            start = byte_col + 1;
        }
    }

    if !state.matches.is_empty() {
        state.current = if let Some((line, col)) = prev_pos {
            state.matches.iter()
                .position(|m| m.line == line && m.col == col)
                .or(Some(0))
        } else {
            Some(0)
        };
    }
}
