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
        Some(PaneKind::Browser(bp)) => bp.title(),
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

/// Stacked pane tab width: pad + icon + space + text + indicator_space + pad.
pub(crate) fn stacked_tab_width(title: &str, cell_w: f32) -> f32 {
    // 2 chars for icon+space, 2 chars for close/modified indicator space
    STACKED_TAB_PAD * 2.0 + (title.chars().count() + 4) as f32 * cell_w
}

// ──────────────────────────────────────────────
// Nerd Font file icons
// ──────────────────────────────────────────────

pub(crate) fn file_icon(name: &str, is_dir: bool, expanded: bool) -> char {
    if is_dir {
        return if expanded { '\u{f07c}' } else { '\u{f07b}' };
    }
    // Per-extension Nerd Font icons
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext {
        "rs"   => '\u{e7a8}', // Rust
        "js"   => '\u{e74e}', // JavaScript
        "mjs"  => '\u{e74e}',
        "jsx"  => '\u{e7ba}', // React
        "ts"   => '\u{e628}', // TypeScript
        "mts"  => '\u{e628}',
        "tsx"  => '\u{e7ba}', // React (TSX)
        "py"   => '\u{e73c}', // Python
        "md" | "markdown" => '\u{e73e}', // Markdown
        "json" => '\u{e60b}', // JSON
        "toml" => '\u{e615}', // TOML (config)
        "yaml" | "yml" => '\u{e615}',
        "html" | "htm" => '\u{e736}', // HTML
        "css"  => '\u{e749}', // CSS
        "scss" | "sass" | "less" => '\u{e749}',
        "sh" | "bash" | "zsh" | "fish" => '\u{e795}', // Shell
        "go"   => '\u{e626}', // Go
        "rb"   => '\u{e739}', // Ruby
        "java" => '\u{e738}', // Java
        "c" | "h" => '\u{e61e}', // C
        "cpp" | "cc" | "cxx" | "hpp" => '\u{e61d}', // C++
        "swift" => '\u{e755}', // Swift
        "lua"  => '\u{e620}', // Lua
        "vim"  => '\u{e62b}', // Vim
        "lock" => '\u{f023}', // Lock file
        "svg"  => '\u{f1c5}', // Image/SVG
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "ico" => '\u{f1c5}',
        "pdf"  => '\u{f1c1}', // PDF
        "zip" | "tar" | "gz" | "bz2" | "xz" => '\u{f1c6}', // Archive
        "xml"  => '\u{e619}', // XML
        "sql"  => '\u{e706}', // Database
        "docker" | "dockerfile" => '\u{e7b0}', // Docker
        "git" | "gitignore" | "gitmodules" => '\u{e702}', // Git
        _      => '\u{f15b}', // generic file icon
    }
}

/// Return the icon color for a file extension (maps to theme colors).
pub(crate) fn file_icon_color(name: &str, p: &crate::theme::ThemePalette) -> tide_core::Color {
    let ext = name.rsplit('.').next().unwrap_or("");
    match ext {
        "rs"   => tide_core::Color::new(0.87, 0.52, 0.22, 1.0), // Rust orange
        "js" | "mjs" | "jsx" => tide_core::Color::new(0.95, 0.85, 0.30, 1.0), // JS yellow
        "ts" | "mts" | "tsx" => tide_core::Color::new(0.19, 0.54, 0.82, 1.0), // TS blue
        "py"   => tide_core::Color::new(0.35, 0.65, 0.85, 1.0), // Python blue
        "md" | "markdown" => tide_core::Color::new(0.50, 0.70, 0.90, 1.0), // Markdown light blue
        "json" | "toml" | "yaml" | "yml" => p.tree_dir_icon, // config warm
        "html" | "htm" => tide_core::Color::new(0.90, 0.45, 0.25, 1.0), // HTML orange
        "css" | "scss" | "sass" | "less" => tide_core::Color::new(0.35, 0.55, 0.90, 1.0), // CSS blue
        "go"   => tide_core::Color::new(0.30, 0.75, 0.85, 1.0), // Go cyan
        "swift" => tide_core::Color::new(0.95, 0.45, 0.25, 1.0), // Swift orange
        _      => p.tree_icon,
    }
}
