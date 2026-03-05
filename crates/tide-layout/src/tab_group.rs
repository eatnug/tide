use tide_core::PaneId;

/// A group of tabbed panes within a single split-tree leaf.
/// Each leaf in the split tree holds a TabGroup instead of a single PaneId.
#[derive(Debug, Clone)]
pub struct TabGroup {
    pub tabs: Vec<PaneId>,
    pub active: usize,
}

impl TabGroup {
    /// Create a TabGroup with a single tab.
    pub fn single(id: PaneId) -> Self {
        Self {
            tabs: vec![id],
            active: 0,
        }
    }

    /// The currently active pane in this group.
    pub fn active_pane(&self) -> PaneId {
        self.tabs[self.active]
    }

    /// Whether the group contains the given pane.
    pub fn contains(&self, id: PaneId) -> bool {
        self.tabs.contains(&id)
    }

    /// Add a tab after the currently active tab.
    /// Returns the index where it was inserted.
    pub fn add_tab(&mut self, id: PaneId) -> usize {
        let insert_at = self.active + 1;
        self.tabs.insert(insert_at, id);
        self.active = insert_at;
        insert_at
    }

    /// Remove a tab by PaneId. Returns true if removed.
    /// Adjusts `active` index if needed.
    pub fn remove_tab(&mut self, id: PaneId) -> bool {
        if let Some(idx) = self.tabs.iter().position(|&t| t == id) {
            self.tabs.remove(idx);
            if self.tabs.is_empty() {
                self.active = 0;
            } else if self.active >= self.tabs.len() {
                self.active = self.tabs.len() - 1;
            } else if idx < self.active {
                self.active -= 1;
            }
            true
        } else {
            false
        }
    }

    /// Set the active tab to the given PaneId.
    /// Returns true if the pane was found.
    pub fn set_active(&mut self, id: PaneId) -> bool {
        if let Some(idx) = self.tabs.iter().position(|&t| t == id) {
            self.active = idx;
            true
        } else {
            false
        }
    }

    /// Number of tabs in the group.
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Whether the group is empty (should not normally happen).
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }
}
