use std::collections::HashMap;

use tide_core::PaneId;

use crate::pane::PaneKind;

// ──────────────────────────────────────────────
// Tab bar title
// ──────────────────────────────────────────────

pub(crate) fn pane_title(panes: &HashMap<PaneId, PaneKind>, id: PaneId) -> String {
    match panes.get(&id) {
        Some(PaneKind::Terminal(pane)) => {
            if let Some(cwd) = pane.backend.detect_cwd_fallback() {
                let components: Vec<_> = cwd.components().collect();
                if components.len() <= 2 {
                    return cwd.display().to_string();
                } else {
                    let last_two: std::path::PathBuf =
                        components[components.len() - 2..].iter().collect();
                    return last_two.display().to_string();
                }
            }
            format!("Terminal {}", id)
        }
        Some(PaneKind::Editor(pane)) => pane.title(),
        Some(PaneKind::Diff(dp)) => format!("Git Changes ({})", dp.files.len()),
        None => format!("Pane {}", id),
    }
}

// ──────────────────────────────────────────────
// Panel tab title (truncated)
// ──────────────────────────────────────────────

pub(crate) fn panel_tab_title(panes: &HashMap<PaneId, PaneKind>, id: PaneId) -> String {
    let full = pane_title(panes, id);
    const MAX_CHARS: usize = 18;
    if full.chars().count() > MAX_CHARS {
        let truncated: String = full.chars().take(MAX_CHARS - 1).collect();
        format!("{}…", truncated)
    } else {
        full
    }
}

// ──────────────────────────────────────────────
// Variable-width tab helpers
// ──────────────────────────────────────────────

use crate::theme::STACKED_TAB_PAD;

/// Total width of all dock tabs (for scroll clamping).
pub(crate) fn dock_tabs_total_width(panes: &HashMap<PaneId, PaneKind>, tabs: &[PaneId], cell_w: f32) -> f32 {
    tabs.iter()
        .map(|&id| stacked_tab_width(&panel_tab_title(panes, id), cell_w))
        .sum()
}

/// Cumulative x offset of the dock tab at `index`.
pub(crate) fn dock_tab_x(panes: &HashMap<PaneId, PaneKind>, tabs: &[PaneId], index: usize, cell_w: f32) -> f32 {
    tabs.iter()
        .take(index)
        .map(|&id| stacked_tab_width(&panel_tab_title(panes, id), cell_w))
        .sum()
}

/// Stacked pane tab width: pad + text + pad.
pub(crate) fn stacked_tab_width(title: &str, cell_w: f32) -> f32 {
    STACKED_TAB_PAD * 2.0 + title.chars().count() as f32 * cell_w
}

// ──────────────────────────────────────────────
// Nerd Font file icons
// ──────────────────────────────────────────────

pub(crate) fn file_icon(_name: &str, is_dir: bool, expanded: bool) -> char {
    if is_dir {
        return if expanded { '\u{f07c}' } else { '\u{f07b}' };
    }
    '\u{f15b}' // generic file icon
}
