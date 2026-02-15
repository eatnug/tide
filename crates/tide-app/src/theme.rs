// UI theme constants — dark/light mode with warm neutral palette.

use tide_core::Color;

// ──────────────────────────────────────────────
// ThemePalette — all mode-dependent colors
// ──────────────────────────────────────────────

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub struct ThemePalette {
    // Surface
    pub surface_bg: Color,
    pub file_tree_bg: Color,
    pub border_color: Color,      // gap / clear color
    pub border_focused: Color,    // accent bar on focused pane

    // Text
    pub tree_text: Color,
    pub tree_dir: Color,
    pub tree_icon: Color,

    // Tab bar
    pub tab_text: Color,
    pub tab_text_focused: Color,

    // Editor
    pub gutter_text: Color,
    pub gutter_active_text: Color,
    pub editor_modified: Color,
    pub panel_tab_bg_active: Color,

    // Drop preview (mode-independent colors kept here for consistency)
    pub drop_fill: Color,
    pub drop_border: Color,
    pub swap_border: Color,

    // Scrollbar
    pub scrollbar_track: Color,
    pub scrollbar_thumb: Color,
    pub scrollbar_match: Color,
    pub scrollbar_current: Color,

    // Hover highlights (semi-transparent overlays)
    pub hover_file_tree: Color,
    pub hover_tab: Color,
    pub hover_close: Color,
    pub hover_panel_border: Color,

    // Search
    pub search_match_bg: Color,
    pub search_current_bg: Color,
    pub search_bar_bg: Color,
    pub search_bar_border: Color,
    pub search_bar_text: Color,
    pub search_bar_counter: Color,

    // IME preedit
    pub ime_preedit_bg: Color,
    pub ime_preedit_fg: Color,

    // Selection
    pub selection: Color,

    // Cursor accent
    pub cursor_accent: Color,

    // Conflict bar (file changed on disk while buffer dirty)
    pub conflict_bar_bg: Color,
    pub conflict_bar_text: Color,
    pub conflict_bar_btn: Color,
    pub conflict_bar_btn_text: Color,

    // Diff view
    pub diff_added_bg: Color,
    pub diff_removed_bg: Color,
    pub diff_added_gutter: Color,
    pub diff_removed_gutter: Color,

    // Header badges
    pub badge_bg: Color,
    pub badge_text: Color,
    pub badge_text_dimmed: Color,
    pub badge_git_branch: Color,
    #[allow(dead_code)]
    pub badge_git_additions: Color,
    #[allow(dead_code)]
    pub badge_git_deletions: Color,
    pub badge_conflict: Color,
    pub badge_deleted: Color,

    // Popup (branch switcher)
    pub popup_bg: Color,
    pub popup_border: Color,
    pub popup_selected: Color,

    // Link highlight
    pub link_color: Color,
}

// ──────────────────────────────────────────────
// DARK palette — deep black, monochrome, high contrast
// ──────────────────────────────────────────────

pub static DARK: ThemePalette = ThemePalette {
    // Surface — near-black, uniform (#0d0d0d)
    surface_bg:     Color::new(0.05, 0.05, 0.05, 1.0),
    file_tree_bg:   Color::new(0.05, 0.05, 0.05, 1.0),
    border_color:   Color::new(0.02, 0.02, 0.02, 1.0),
    border_focused: Color::new(1.0, 1.0, 1.0, 0.35),

    // Text — monochrome, high contrast
    tree_text:  Color::new(0.80, 0.80, 0.80, 1.0),
    tree_dir:   Color::new(0.86, 0.86, 0.86, 1.0),
    tree_icon:  Color::new(0.48, 0.48, 0.48, 1.0),

    // Tab bar
    tab_text:         Color::new(0.50, 0.50, 0.50, 1.0),
    tab_text_focused: Color::new(0.88, 0.88, 0.88, 1.0),

    // Editor
    gutter_text:        Color::new(0.30, 0.30, 0.30, 1.0),
    gutter_active_text: Color::new(0.65, 0.65, 0.65, 1.0),
    editor_modified:    Color::new(0.90, 0.65, 0.25, 1.0),
    panel_tab_bg_active: Color::new(0.09, 0.09, 0.09, 1.0),

    // Drop preview
    drop_fill:   Color::new(1.0, 1.0, 1.0, 0.06),
    drop_border: Color::new(1.0, 1.0, 1.0, 0.25),
    swap_border: Color::new(1.0, 1.0, 1.0, 0.35),

    // Scrollbar
    scrollbar_track:   Color::new(1.0, 1.0, 1.0, 0.03),
    scrollbar_thumb:   Color::new(1.0, 1.0, 1.0, 0.12),
    scrollbar_match:   Color::new(0.95, 0.75, 0.10, 0.85),
    scrollbar_current: Color::new(1.0, 0.90, 0.20, 1.0),

    // Hover
    hover_file_tree:    Color::new(1.0, 1.0, 1.0, 0.04),
    hover_tab:          Color::new(1.0, 1.0, 1.0, 0.05),
    hover_close:        Color::new(1.0, 0.4, 0.4, 0.20),
    hover_panel_border: Color::new(1.0, 1.0, 1.0, 0.12),

    // Search
    search_match_bg:    Color::new(0.65, 0.50, 0.10, 0.40),
    search_current_bg:  Color::new(0.95, 0.75, 0.10, 0.60),
    search_bar_bg:      Color::new(0.08, 0.08, 0.08, 1.0),
    search_bar_border:  Color::new(1.0, 1.0, 1.0, 0.15),
    search_bar_text:    Color::new(0.88, 0.88, 0.88, 1.0),
    search_bar_counter: Color::new(0.45, 0.45, 0.45, 1.0),

    // IME
    ime_preedit_bg: Color::new(0.10, 0.10, 0.10, 1.0),
    ime_preedit_fg: Color::new(0.95, 0.95, 0.95, 1.0),

    // Selection
    selection: Color::new(1.0, 1.0, 1.0, 0.10),

    // Cursor accent
    cursor_accent: Color::new(1.0, 1.0, 1.0, 1.0),

    // Conflict bar
    conflict_bar_bg:       Color::new(0.18, 0.15, 0.06, 1.0),
    conflict_bar_text:     Color::new(0.88, 0.78, 0.45, 1.0),
    conflict_bar_btn:      Color::new(0.24, 0.20, 0.08, 1.0),
    conflict_bar_btn_text: Color::new(0.92, 0.82, 0.50, 1.0),

    // Diff view
    diff_added_bg:      Color::new(0.10, 0.22, 0.10, 1.0),   // dark green bg
    diff_removed_bg:    Color::new(0.25, 0.10, 0.10, 1.0),   // dark red bg
    diff_added_gutter:  Color::new(0.30, 0.70, 0.30, 1.0),   // green gutter +
    diff_removed_gutter: Color::new(0.70, 0.30, 0.30, 1.0),  // red gutter -

    // Header badges
    badge_bg:             Color::new(1.0, 1.0, 1.0, 0.06),
    badge_text:           Color::new(0.70, 0.70, 0.70, 1.0),
    badge_text_dimmed:    Color::new(0.45, 0.45, 0.45, 1.0),
    badge_git_branch:     Color::new(0.65, 0.50, 1.0, 1.0),
    badge_git_additions:  Color::new(0.30, 0.80, 0.40, 1.0),
    badge_git_deletions:  Color::new(0.90, 0.35, 0.35, 1.0),
    badge_conflict:       Color::new(0.90, 0.65, 0.25, 1.0),
    badge_deleted:        Color::new(0.90, 0.35, 0.35, 1.0),

    // Popup
    popup_bg:       Color::new(0.10, 0.10, 0.10, 1.0),
    popup_border:   Color::new(1.0, 1.0, 1.0, 0.15),
    popup_selected: Color::new(1.0, 1.0, 1.0, 0.08),

    // Link highlight — #4E94CE blue
    link_color: Color::new(0.306, 0.580, 0.808, 1.0),
};

// ──────────────────────────────────────────────
// LIGHT palette — clean off-white
// ──────────────────────────────────────────────

pub static LIGHT: ThemePalette = ThemePalette {
    // Surface
    surface_bg:     Color::new(0.97, 0.97, 0.96, 1.0),
    file_tree_bg:   Color::new(0.94, 0.94, 0.93, 1.0),
    border_color:   Color::new(0.88, 0.88, 0.87, 1.0),
    border_focused: Color::new(0.0, 0.0, 0.0, 0.35),

    // Text
    tree_text:  Color::new(0.25, 0.25, 0.25, 1.0),
    tree_dir:   Color::new(0.15, 0.15, 0.15, 1.0),
    tree_icon:  Color::new(0.40, 0.40, 0.40, 1.0),

    // Tab bar
    tab_text:         Color::new(0.50, 0.50, 0.50, 1.0),
    tab_text_focused: Color::new(0.10, 0.10, 0.10, 1.0),

    // Editor
    gutter_text:        Color::new(0.62, 0.62, 0.62, 1.0),
    gutter_active_text: Color::new(0.25, 0.25, 0.25, 1.0),
    editor_modified:    Color::new(0.85, 0.55, 0.15, 1.0),
    panel_tab_bg_active: Color::new(0.91, 0.91, 0.90, 1.0),

    // Drop preview
    drop_fill:   Color::new(0.0, 0.0, 0.0, 0.08),
    drop_border: Color::new(0.0, 0.0, 0.0, 0.25),
    swap_border: Color::new(0.0, 0.0, 0.0, 0.35),

    // Scrollbar
    scrollbar_track:   Color::new(0.0, 0.0, 0.0, 0.05),
    scrollbar_thumb:   Color::new(0.0, 0.0, 0.0, 0.18),
    scrollbar_match:   Color::new(0.90, 0.70, 0.10, 0.70),
    scrollbar_current: Color::new(0.85, 0.65, 0.05, 1.0),

    // Hover
    hover_file_tree:    Color::new(0.0, 0.0, 0.0, 0.05),
    hover_tab:          Color::new(0.0, 0.0, 0.0, 0.06),
    hover_close:        Color::new(1.0, 0.3, 0.3, 0.18),
    hover_panel_border: Color::new(0.0, 0.0, 0.0, 0.12),

    // Search
    search_match_bg:    Color::new(0.90, 0.80, 0.20, 0.25),
    search_current_bg:  Color::new(0.95, 0.75, 0.10, 0.45),
    search_bar_bg:      Color::new(1.0, 1.0, 1.0, 1.0),
    search_bar_border:  Color::new(0.0, 0.0, 0.0, 0.15),
    search_bar_text:    Color::new(0.10, 0.10, 0.10, 1.0),
    search_bar_counter: Color::new(0.50, 0.50, 0.50, 1.0),

    // IME
    ime_preedit_bg: Color::new(0.90, 0.90, 0.90, 1.0),
    ime_preedit_fg: Color::new(0.10, 0.10, 0.10, 1.0),

    // Selection
    selection: Color::new(0.0, 0.0, 0.0, 0.12),

    // Cursor accent
    cursor_accent: Color::new(0.15, 0.15, 0.15, 1.0),

    // Conflict bar
    conflict_bar_bg:       Color::new(1.0, 0.96, 0.84, 1.0),
    conflict_bar_text:     Color::new(0.42, 0.32, 0.08, 1.0),
    conflict_bar_btn:      Color::new(0.92, 0.88, 0.72, 1.0),
    conflict_bar_btn_text: Color::new(0.32, 0.24, 0.04, 1.0),

    // Diff view
    diff_added_bg:      Color::new(0.85, 0.95, 0.85, 1.0),   // light green bg
    diff_removed_bg:    Color::new(0.95, 0.85, 0.85, 1.0),   // light red bg
    diff_added_gutter:  Color::new(0.15, 0.55, 0.15, 1.0),   // green gutter +
    diff_removed_gutter: Color::new(0.60, 0.15, 0.15, 1.0),  // red gutter -

    // Header badges
    badge_bg:             Color::new(0.0, 0.0, 0.0, 0.06),
    badge_text:           Color::new(0.35, 0.35, 0.35, 1.0),
    badge_text_dimmed:    Color::new(0.55, 0.55, 0.55, 1.0),
    badge_git_branch:     Color::new(0.45, 0.25, 0.80, 1.0),
    badge_git_additions:  Color::new(0.15, 0.55, 0.15, 1.0),
    badge_git_deletions:  Color::new(0.65, 0.15, 0.15, 1.0),
    badge_conflict:       Color::new(0.85, 0.55, 0.15, 1.0),
    badge_deleted:        Color::new(0.65, 0.15, 0.15, 1.0),

    // Popup
    popup_bg:       Color::new(0.96, 0.96, 0.95, 1.0),
    popup_border:   Color::new(0.0, 0.0, 0.0, 0.15),
    popup_selected: Color::new(0.0, 0.0, 0.0, 0.06),

    // Link highlight — #0969DA blue
    link_color: Color::new(0.035, 0.412, 0.855, 1.0),
};

// ──────────────────────────────────────────────
// Layout constants (mode-independent)
// ──────────────────────────────────────────────

pub const BORDER_WIDTH: f32 = 2.0;
pub const PANE_GAP: f32 = 4.0;
pub const PANE_PADDING: f32 = 10.0;
pub const FILE_TREE_LINE_SPACING: f32 = 1.5;
pub const FILE_TREE_WIDTH: f32 = 240.0;

pub const TAB_BAR_HEIGHT: f32 = 30.0;

pub const EDITOR_PANEL_WIDTH: f32 = 380.0;
pub const PANEL_TAB_HEIGHT: f32 = 30.0;
pub const PANEL_TAB_WIDTH: f32 = 140.0;
pub const PANEL_TAB_GAP: f32 = 2.0;
pub const PANEL_TAB_CLOSE_SIZE: f32 = 14.0;

pub const PANE_CLOSE_SIZE: f32 = 14.0;

pub const DROP_PREVIEW_BORDER_WIDTH: f32 = 2.0;
pub const SWAP_PREVIEW_BORDER_WIDTH: f32 = 3.0;
pub const DRAG_THRESHOLD: f32 = 5.0;

pub const SCROLLBAR_WIDTH: f32 = 6.0;

pub const SEARCH_BAR_WIDTH: f32 = 260.0;
pub const SEARCH_BAR_HEIGHT: f32 = 28.0;
pub const SEARCH_BAR_CLOSE_SIZE: f32 = 20.0;

pub const CONFLICT_BAR_HEIGHT: f32 = 26.0;

// Header badges
pub const BADGE_PADDING_H: f32 = 6.0;
pub const BADGE_GAP: f32 = 4.0;
pub const BADGE_RADIUS: f32 = 3.0;
