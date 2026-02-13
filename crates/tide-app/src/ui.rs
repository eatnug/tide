use std::collections::HashMap;

use tide_core::{DropZone, PaneId, Rect};

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
// Drop preview geometry
// ──────────────────────────────────────────────

pub(crate) fn compute_preview_rect(target_rect: Rect, zone: DropZone) -> Rect {
    match zone {
        DropZone::Top => Rect::new(
            target_rect.x,
            target_rect.y,
            target_rect.width,
            target_rect.height / 2.0,
        ),
        DropZone::Bottom => Rect::new(
            target_rect.x,
            target_rect.y + target_rect.height / 2.0,
            target_rect.width,
            target_rect.height / 2.0,
        ),
        DropZone::Left => Rect::new(
            target_rect.x,
            target_rect.y,
            target_rect.width / 2.0,
            target_rect.height,
        ),
        DropZone::Right => Rect::new(
            target_rect.x + target_rect.width / 2.0,
            target_rect.y,
            target_rect.width / 2.0,
            target_rect.height,
        ),
        DropZone::Center => target_rect,
    }
}

// ──────────────────────────────────────────────
// Nerd Font file icons
// ──────────────────────────────────────────────

pub(crate) fn file_icon(name: &str, is_dir: bool, expanded: bool) -> char {
    if is_dir {
        return if expanded { '\u{f07c}' } else { '\u{f07b}' };
    }
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext {
        "rs" => '\u{e7a8}',
        "toml" => '\u{e615}',
        "md" => '\u{e73e}',
        "json" => '\u{e60b}',
        "yaml" | "yml" => '\u{e615}',
        "js" => '\u{e74e}',
        "ts" => '\u{e628}',
        "tsx" | "jsx" => '\u{e7ba}',
        "py" => '\u{e73c}',
        "go" => '\u{e626}',
        "c" | "h" => '\u{e61e}',
        "cpp" | "hpp" | "cc" => '\u{e61d}',
        "lock" => '\u{f023}',
        "sh" | "bash" | "zsh" => '\u{e795}',
        "git" | "gitignore" => '\u{e702}',
        "css" => '\u{e749}',
        "html" => '\u{e736}',
        "svg" => '\u{e698}',
        "png" | "jpg" | "jpeg" | "gif" | "webp" => '\u{f1c5}',
        "txt" => '\u{f15c}',
        _ => '\u{f15b}',
    }
}
