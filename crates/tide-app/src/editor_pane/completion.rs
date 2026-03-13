// CompletionPopup state for Editor panes.
// Spec: docs/specs/lsp-completion.md

/// A single completion suggestion from a language server.
#[derive(Debug, Clone)]
pub struct CompletionItem {
    pub label: String,
    pub kind: CompletionKind,
    pub insert_text: Option<String>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
}

/// Kind of a completion item, shown as an abbreviation in the popup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Function,
    Variable,
    Field,
    Type,
    Module,
    Keyword,
    Snippet,
    Property,
    Method,
    Constant,
    Other,
}

impl CompletionKind {
    /// Short abbreviation for display in the popup.
    pub fn abbr(&self) -> &'static str {
        match self {
            Self::Function => "fn",
            Self::Variable => "var",
            Self::Field => "fld",
            Self::Type => "typ",
            Self::Module => "mod",
            Self::Keyword => "kw",
            Self::Snippet => "snip",
            Self::Property => "prop",
            Self::Method => "mth",
            Self::Constant => "con",
            Self::Other => "",
        }
    }
}

/// Per-EditorPane completion popup state.
/// NOT part of ModalStack — coexists with typing.
#[derive(Debug, Clone)]
pub struct CompletionState {
    /// All items received from the language server.
    pub items: Vec<CompletionItem>,
    /// Indices into `items` that match the current filter prefix.
    pub filtered_indices: Vec<usize>,
    /// Index into `filtered_indices` of the currently selected item.
    pub selected_index: usize,
    /// Text typed since the completion was triggered (used for client-side filtering).
    pub prefix: String,
    /// Cursor position (line, col) where completion was triggered.
    pub trigger_line: usize,
    pub trigger_col: usize,
    /// Scroll offset within the popup (for long lists).
    pub scroll_offset: usize,
}

/// Maximum number of visible items in the completion popup.
pub const COMPLETION_VISIBLE_COUNT: usize = 10;

impl CompletionState {
    /// Create a new CompletionState with items from the language server.
    pub fn new(items: Vec<CompletionItem>, trigger_line: usize, trigger_col: usize) -> Self {
        let filtered_indices: Vec<usize> = (0..items.len()).collect();
        Self {
            items,
            filtered_indices,
            selected_index: 0,
            prefix: String::new(),
            trigger_line,
            trigger_col,
            scroll_offset: 0,
        }
    }

    /// Get the currently selected CompletionItem, if any.
    pub fn selected_item(&self) -> Option<&CompletionItem> {
        self.filtered_indices
            .get(self.selected_index)
            .and_then(|&idx| self.items.get(idx))
    }

    /// Move selection down. Wraps around at the bottom.
    pub fn select_next(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        self.selected_index = (self.selected_index + 1) % self.filtered_indices.len();
        self.ensure_selected_visible();
    }

    /// Move selection up. Wraps around at the top.
    pub fn select_prev(&mut self) {
        if self.filtered_indices.is_empty() {
            return;
        }
        if self.selected_index == 0 {
            self.selected_index = self.filtered_indices.len() - 1;
        } else {
            self.selected_index -= 1;
        }
        self.ensure_selected_visible();
    }

    /// Get the text to insert when accepting the selected completion.
    /// Uses insertText if available, otherwise falls back to label.
    pub fn insert_text(&self) -> Option<String> {
        self.selected_item().map(|item| {
            item.insert_text.clone().unwrap_or_else(|| item.label.clone())
        })
    }

    /// Re-filter and sort items using fuzzy matching (VS Code-style).
    /// Characters in the prefix must appear in order in the candidate.
    /// Scoring: consecutive matches, word boundary matches, prefix matches get bonuses.
    pub fn apply_filter(&mut self) {
        if self.prefix.is_empty() {
            self.filtered_indices = (0..self.items.len()).collect();
            self.selected_index = 0;
            self.scroll_offset = 0;
            return;
        }

        let pattern: Vec<char> = self.prefix.to_lowercase().chars().collect();

        // Score each item with fuzzy matching
        let mut scored: Vec<(usize, i32)> = self.items.iter().enumerate()
            .filter_map(|(i, item)| {
                let text = item.filter_text.as_deref().unwrap_or(&item.label);
                fuzzy_score(&pattern, text).map(|score| (i, score))
            })
            .collect();

        // Sort by score descending (higher = better), then by original index
        scored.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));

        self.filtered_indices = scored.into_iter().map(|(i, _)| i).collect();
        self.selected_index = 0;
        self.scroll_offset = 0;
    }

    /// Whether the popup has any visible items.
    pub fn is_empty(&self) -> bool {
        self.filtered_indices.is_empty()
    }

    /// Ensure the selected item is within the visible scroll window.
    fn ensure_selected_visible(&mut self) {
        if self.selected_index < self.scroll_offset {
            self.scroll_offset = self.selected_index;
        } else if self.selected_index >= self.scroll_offset + COMPLETION_VISIBLE_COUNT {
            self.scroll_offset = self.selected_index + 1 - COMPLETION_VISIBLE_COUNT;
        }
    }

    /// Get visible items (accounting for scroll offset and max visible count).
    pub fn visible_items(&self) -> impl Iterator<Item = (usize, &CompletionItem)> {
        self.filtered_indices.iter()
            .enumerate()
            .skip(self.scroll_offset)
            .take(COMPLETION_VISIBLE_COUNT)
            .filter_map(move |(display_idx, &item_idx)| {
                self.items.get(item_idx).map(|item| (display_idx, item))
            })
    }
}

/// Fuzzy match scoring (VS Code-style).
/// Returns Some(score) if all characters in `pattern` appear in order in `text`.
/// Higher score = better match.
///
/// Bonuses:
///   +6  character matches at the start of the word (prefix)
///   +5  character matches at a word boundary (after `_`, `.`, `-`, or camelCase)
///   +4  consecutive matching characters (compounding)
///   +1  base match
/// Penalties:
///   -3  gap between matched characters
fn fuzzy_score(pattern: &[char], text: &str) -> Option<i32> {
    if pattern.is_empty() {
        return Some(0);
    }

    let text_chars: Vec<char> = text.chars().collect();
    let text_lower: Vec<char> = text.to_lowercase().chars().collect();

    if pattern.len() > text_lower.len() {
        return None;
    }

    // Quick check: all pattern chars exist in text in order
    {
        let mut ti = 0;
        for &pc in pattern {
            while ti < text_lower.len() && text_lower[ti] != pc {
                ti += 1;
            }
            if ti >= text_lower.len() {
                return None;
            }
            ti += 1;
        }
    }

    // Greedy scoring with best-match heuristic
    let mut score: i32 = 0;
    let mut pi = 0; // pattern index
    let mut consecutive = 0;
    let mut last_match_idx: Option<usize> = None;

    for ti in 0..text_lower.len() {
        if pi < pattern.len() && text_lower[ti] == pattern[pi] {
            // Base match
            score += 1;

            // Prefix bonus: matching at position 0, 1, 2...
            if ti == pi {
                score += 6;
            }

            // Word boundary bonus
            if ti == 0 || is_word_boundary(&text_chars, ti) {
                score += 5;
            }

            // Case-exact bonus
            if text_chars[ti].to_lowercase().eq(pattern[pi].to_lowercase()) &&
               text_chars[ti] == pattern[pi] {
                // Exact case match (pattern char was already lowered, skip this)
            }

            // Consecutive bonus
            if let Some(last) = last_match_idx {
                if ti == last + 1 {
                    consecutive += 1;
                    score += 4 * consecutive;
                } else {
                    // Gap penalty
                    let gap = (ti - last - 1) as i32;
                    score -= gap.min(5) * 3;
                    consecutive = 0;
                }
            }

            last_match_idx = Some(ti);
            pi += 1;
        }
    }

    if pi == pattern.len() {
        Some(score)
    } else {
        None
    }
}

/// Check if position `i` in `chars` is a word boundary.
fn is_word_boundary(chars: &[char], i: usize) -> bool {
    if i == 0 {
        return true;
    }
    let prev = chars[i - 1];
    let curr = chars[i];
    // After separator
    if prev == '_' || prev == '.' || prev == '-' || prev == ' ' || prev == '/' {
        return true;
    }
    // camelCase boundary: lowercase followed by uppercase
    if prev.is_lowercase() && curr.is_uppercase() {
        return true;
    }
    false
}
