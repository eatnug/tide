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
    pub pane_bg: Color,           // pane background (slightly lighter than surface_bg)
    pub file_tree_bg: Color,
    pub border_color: Color,      // gap / clear color
    pub border_focused: Color,    // accent bar on focused pane
    pub border_subtle: Color,     // subtle border on all panels

    // Text
    pub tree_text: Color,       // file names
    pub tree_dir: Color,        // folder names
    pub tree_dir_icon: Color,   // folder icon color (warm)
    pub tree_icon: Color,       // file icon color (dim)
    pub tree_row_active: Color, // expanded folder row background

    // Tab bar
    pub tab_text: Color,
    pub tab_text_focused: Color,
    pub close_icon: Color,        // close button icon color
    pub dock_tab_underline: Color, // active dock tab bottom underline

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
    pub badge_bg_unfocused: Color, // unfocused pane badge background
    pub badge_text: Color,
    pub badge_text_dimmed: Color,
    pub badge_git_branch: Color,
    pub badge_git_worktree: Color,
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
    pub popup_scrim: Color,

    // Link highlight
    pub link_color: Color,

    // File tree git status
    pub git_modified: Color,
    pub git_added: Color,
    pub git_added_bg: Color,      // git added badge background
    pub git_conflict: Color,
}

// ──────────────────────────────────────────────
// DARK palette — deep black, monochrome, high contrast
// ──────────────────────────────────────────────

pub static DARK: ThemePalette = ThemePalette {
    // Surface — #0A0A0B base, #0E0E10 pane, #111113 panels
    surface_bg:     Color::new(0.039, 0.039, 0.043, 1.0),
    pane_bg:        Color::new(0.055, 0.055, 0.063, 1.0),
    file_tree_bg:   Color::new(0.067, 0.067, 0.075, 1.0),
    border_color:   Color::new(0.039, 0.039, 0.043, 1.0),
    border_focused: Color::new(0.769, 0.722, 0.651, 0.50),
    border_subtle:  Color::new(0.122, 0.122, 0.137, 1.0),

    // Text — warm neutral
    tree_text:      Color::new(0.545, 0.545, 0.565, 1.0),   // file names #8B8B90
    tree_dir:       Color::new(0.678, 0.678, 0.690, 1.0),   // folder names #ADADB0
    tree_dir_icon:  Color::new(0.831, 0.784, 0.714, 1.0),   // folder icons #D4C8B6
    tree_icon:      Color::new(0.290, 0.290, 0.306, 1.0),   // file icons #4A4A4E
    tree_row_active: Color::new(0.102, 0.102, 0.114, 1.0),  // expanded folder row bg #1A1A1D

    // Tab bar
    tab_text:           Color::new(0.420, 0.420, 0.439, 1.0),
    tab_text_focused:   Color::new(1.0, 1.0, 1.0, 1.0),
    close_icon:         Color::new(0.290, 0.290, 0.306, 1.0),
    dock_tab_underline: Color::new(0.769, 0.722, 0.651, 1.0),

    // Editor
    gutter_text:        Color::new(0.30, 0.30, 0.30, 1.0),
    gutter_active_text: Color::new(0.65, 0.65, 0.65, 1.0),
    editor_modified:    Color::new(0.831, 0.659, 0.263, 1.0),
    panel_tab_bg_active: Color::new(0.055, 0.055, 0.063, 1.0),

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
    search_bar_bg:      Color::new(0.10, 0.10, 0.10, 1.0),
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

    // Diff view — transparent overlays matching Tide.pen
    diff_added_bg:      Color::new(0.133, 0.773, 0.369, 0.071),  // #22C55E12
    diff_removed_bg:    Color::new(0.937, 0.267, 0.267, 0.071),  // #EF444412
    diff_added_gutter:  Color::new(0.525, 0.937, 0.675, 1.0),    // #86EFAC
    diff_removed_gutter: Color::new(0.937, 0.604, 0.604, 1.0),   // #EF9A9A

    // Header badges — alpha 0.094 = 0x18/0xFF per Tide.pen
    badge_bg:             Color::new(0.769, 0.722, 0.651, 0.094),
    badge_bg_unfocused:   Color::new(0.420, 0.420, 0.439, 0.094),
    badge_text:           Color::new(0.545, 0.545, 0.565, 1.0),
    badge_text_dimmed:    Color::new(0.290, 0.290, 0.306, 1.0),
    badge_git_branch:     Color::new(0.769, 0.722, 0.651, 1.0),
    badge_git_worktree:   Color::new(0.35, 0.80, 0.75, 1.0),
    badge_git_additions:  Color::new(0.30, 0.80, 0.40, 1.0),
    badge_git_deletions:  Color::new(0.90, 0.35, 0.35, 1.0),
    badge_conflict:       Color::new(0.90, 0.65, 0.25, 1.0),
    badge_deleted:        Color::new(0.90, 0.35, 0.35, 1.0),

    // Popup
    popup_bg:       Color::new(0.067, 0.067, 0.075, 1.0),
    popup_border:   Color::new(0.165, 0.165, 0.180, 1.0),
    popup_selected: Color::new(1.0, 1.0, 1.0, 0.024),
    popup_scrim:    Color::new(0.0, 0.0, 0.0, 0.60),

    // Link highlight — #4E94CE blue
    link_color: Color::new(0.306, 0.580, 0.808, 1.0),

    // File tree git status
    git_modified: Color::new(0.831, 0.659, 0.263, 1.0),   // warm yellow #D4A843
    git_added:    Color::new(0.133, 0.773, 0.369, 1.0),    // green #22C55E
    git_added_bg: Color::new(0.133, 0.773, 0.369, 0.18),   // green badge bg
    git_conflict: Color::new(0.90, 0.55, 0.20, 1.0),       // orange
};

// ──────────────────────────────────────────────
// LIGHT palette — clean off-white
// ──────────────────────────────────────────────

pub static LIGHT: ThemePalette = ThemePalette {
    // Surface
    surface_bg:     Color::new(0.97, 0.97, 0.96, 1.0),
    pane_bg:        Color::new(0.96, 0.96, 0.95, 1.0),
    file_tree_bg:   Color::new(0.94, 0.94, 0.93, 1.0),
    border_color:   Color::new(0.88, 0.88, 0.87, 1.0),
    border_focused: Color::new(0.0, 0.0, 0.0, 0.35),
    border_subtle:  Color::new(0.0, 0.0, 0.0, 0.08),

    // Text
    tree_text:      Color::new(0.25, 0.25, 0.25, 1.0),
    tree_dir:       Color::new(0.15, 0.15, 0.15, 1.0),
    tree_dir_icon:  Color::new(0.30, 0.30, 0.30, 1.0),
    tree_icon:      Color::new(0.55, 0.55, 0.55, 1.0),
    tree_row_active: Color::new(0.90, 0.90, 0.89, 1.0),

    // Tab bar
    tab_text:           Color::new(0.50, 0.50, 0.50, 1.0),
    tab_text_focused:   Color::new(0.10, 0.10, 0.10, 1.0),
    close_icon:         Color::new(0.55, 0.55, 0.55, 1.0),
    dock_tab_underline: Color::new(0.25, 0.25, 0.25, 1.0),

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
    search_bar_bg:      Color::new(0.96, 0.96, 0.95, 1.0),
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
    badge_bg_unfocused:   Color::new(0.0, 0.0, 0.0, 0.04),
    badge_text:           Color::new(0.35, 0.35, 0.35, 1.0),
    badge_text_dimmed:    Color::new(0.55, 0.55, 0.55, 1.0),
    badge_git_branch:     Color::new(0.45, 0.25, 0.80, 1.0),
    badge_git_worktree:   Color::new(0.15, 0.55, 0.50, 1.0),
    badge_git_additions:  Color::new(0.15, 0.55, 0.15, 1.0),
    badge_git_deletions:  Color::new(0.65, 0.15, 0.15, 1.0),
    badge_conflict:       Color::new(0.85, 0.55, 0.15, 1.0),
    badge_deleted:        Color::new(0.65, 0.15, 0.15, 1.0),

    // Popup
    popup_bg:       Color::new(0.96, 0.96, 0.95, 1.0),
    popup_border:   Color::new(0.0, 0.0, 0.0, 0.15),
    popup_selected: Color::new(0.0, 0.0, 0.0, 0.06),
    popup_scrim:    Color::new(0.0, 0.0, 0.0, 0.25),

    // Link highlight — #0969DA blue
    link_color: Color::new(0.035, 0.412, 0.855, 1.0),

    // File tree git status
    git_modified: Color::new(0.70, 0.58, 0.10, 1.0),   // warm yellow
    git_added:    Color::new(0.15, 0.55, 0.15, 1.0),    // green
    git_added_bg: Color::new(0.15, 0.55, 0.15, 0.094),  // green badge bg
    git_conflict: Color::new(0.80, 0.45, 0.10, 1.0),    // orange
};

// ──────────────────────────────────────────────
// Layout constants (mode-independent)
// ──────────────────────────────────────────────

pub const BORDER_WIDTH: f32 = 1.0;
pub const PANE_GAP: f32 = 2.0;
pub const PANE_PADDING: f32 = 12.0;
pub const PANE_CORNER_RADIUS: f32 = 6.0;
pub const FILE_TREE_LINE_SPACING: f32 = 1.5;
pub const FILE_TREE_ROW_RADIUS: f32 = 6.0;
pub const FILE_TREE_WIDTH: f32 = 240.0;

pub const TAB_BAR_HEIGHT: f32 = 32.0;

pub const EDITOR_PANEL_WIDTH: f32 = 340.0;
pub const PANEL_TAB_HEIGHT: f32 = 36.0;

pub const PANE_CLOSE_SIZE: f32 = 14.0;

pub const DROP_PREVIEW_BORDER_WIDTH: f32 = 2.0;
pub const SWAP_PREVIEW_BORDER_WIDTH: f32 = 3.0;
pub const DRAG_THRESHOLD: f32 = 5.0;

pub const SCROLLBAR_WIDTH: f32 = 6.0;

// Dock tab layout (variable width, per Tide.pen)
pub const DOCK_TAB_PAD: f32 = 12.0;
pub const DOCK_TAB_GAP: f32 = 6.0;
pub const DOCK_TAB_DOT_SIZE: f32 = 6.0;

// Stacked pane tab layout (inline text tabs)
pub const STACKED_TAB_PAD: f32 = 10.0;

pub const SEARCH_BAR_WIDTH: f32 = 260.0;
pub const SEARCH_BAR_HEIGHT: f32 = 28.0;
pub const SEARCH_BAR_CLOSE_SIZE: f32 = 20.0;

pub const CONFLICT_BAR_HEIGHT: f32 = 28.0;

// Header badges
/// Height of the macOS titlebar inset (traffic light area).
/// Used to offset all layout rects so content doesn't overlap the titlebar controls.
pub const TITLEBAR_HEIGHT: f32 = 40.0;

pub const BADGE_PADDING_H: f32 = 8.0;
pub const BADGE_GAP: f32 = 6.0;
pub const BADGE_RADIUS: f32 = 100.0;

// ── Popup layout constants ──
pub const POPUP_CORNER_RADIUS: f32 = 8.0;     // 팝업 라운드 코너 반지름
pub const POPUP_INPUT_PADDING: f32 = 10.0;    // 입력 필드 높이 = cell_h + 이 값
pub const POPUP_LINE_EXTRA: f32 = 4.0;        // 리스트 줄 높이 = cell_h + 이 값
pub const POPUP_TEXT_INSET: f32 = 8.0;        // 팝업 내부 텍스트 좌우 여백
pub const POPUP_BORDER_WIDTH: f32 = 1.0;      // 팝업 테두리 두께
pub const POPUP_SEPARATOR: f32 = 1.0;         // 구분선 두께
pub const POPUP_SEPARATOR_INSET: f32 = 4.0;   // 구분선 좌우 인셋
pub const POPUP_SELECTED_INSET: f32 = 2.0;    // 선택 하이라이트 좌우 인셋
pub const POPUP_MAX_VISIBLE: usize = 10;      // 리스트 최대 표시 항목
pub const CURSOR_BEAM_WIDTH: f32 = 1.5;       // 텍스트 커서 beam 너비
pub const FILE_SWITCHER_POPUP_W: f32 = 260.0; // 파일 스위처 팝업 너비
pub const SAVE_AS_POPUP_W: f32 = 310.0;       // Save-as 팝업 최대 너비
pub const CONTEXT_MENU_W: f32 = 140.0;        // 컨텍스트 메뉴 너비

// ── Config page layout constants ──
pub const CONFIG_PAGE_W: f32 = 560.0;
pub const CONFIG_PAGE_MAX_H: f32 = 480.0;
pub const CONFIG_PAGE_MAX_VISIBLE: usize = 14;
