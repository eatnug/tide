// UI theme constants — modern flat border-based palette.

use tide_core::Color;

// Surface & borders
pub const SURFACE_BG: Color = Color::new(0.10, 0.10, 0.14, 1.0); // unified panel background
#[allow(dead_code)] // Value mirrored in renderer clear color
pub const BORDER_COLOR: Color = Color::new(0.18, 0.18, 0.24, 1.0); // border / clear color
pub const BORDER_FOCUSED: Color = Color::new(0.35, 0.58, 1.0, 0.7); // accent border on focused pane

// Text
pub const TREE_TEXT_COLOR: Color = Color::new(0.70, 0.72, 0.80, 1.0);
pub const TREE_DIR_COLOR: Color = Color::new(0.35, 0.58, 1.0, 1.0);
pub const TREE_ICON_COLOR: Color = Color::new(0.55, 0.58, 0.68, 1.0);

// Layout
pub const BORDER_WIDTH: f32 = 1.0; // border line width between panels
pub const PANE_GAP: f32 = BORDER_WIDTH; // gap = border width (gap IS the border)
pub const PANE_PADDING: f32 = 4.0; // inner padding
pub const FILE_TREE_WIDTH: f32 = 240.0; // file tree panel width

// Tab bar
pub const TAB_BAR_HEIGHT: f32 = 30.0;
pub const TAB_BAR_TEXT: Color = Color::new(0.55, 0.58, 0.68, 1.0);
pub const TAB_BAR_TEXT_FOCUSED: Color = Color::new(0.80, 0.82, 0.90, 1.0);

// Editor
#[allow(dead_code)]
pub const GUTTER_TEXT: Color = Color::new(0.40, 0.42, 0.50, 1.0);
#[allow(dead_code)]
pub const GUTTER_ACTIVE_TEXT: Color = Color::new(0.70, 0.72, 0.80, 1.0);
#[allow(dead_code)]
pub const EDITOR_MODIFIED_INDICATOR: Color = Color::new(0.90, 0.65, 0.25, 1.0);

// Editor panel (right-side)
pub const EDITOR_PANEL_WIDTH: f32 = 380.0;
pub const PANEL_TAB_HEIGHT: f32 = 30.0;
pub const PANEL_TAB_WIDTH: f32 = 140.0;
pub const PANEL_TAB_GAP: f32 = 2.0;
pub const PANEL_TAB_BG_ACTIVE: Color = Color::new(0.14, 0.14, 0.20, 1.0);
pub const PANEL_TAB_CLOSE_SIZE: f32 = 14.0;

// Drop preview (insert: fill + border)
pub const DROP_PREVIEW_FILL: Color = Color::new(0.35, 0.58, 1.0, 0.15);
pub const DROP_PREVIEW_BORDER: Color = Color::new(0.35, 0.58, 1.0, 0.6);
pub const DROP_PREVIEW_BORDER_WIDTH: f32 = 2.0;
// Drop preview (swap: border only, thicker)
pub const SWAP_PREVIEW_BORDER: Color = Color::new(0.75, 0.55, 1.0, 0.7);
pub const SWAP_PREVIEW_BORDER_WIDTH: f32 = 3.0;
pub const DRAG_THRESHOLD: f32 = 5.0;

// Scrollbar
pub const SCROLLBAR_WIDTH: f32 = 6.0;
pub const SCROLLBAR_TRACK: Color = Color::new(0.30, 0.30, 0.35, 0.10);
pub const SCROLLBAR_THUMB: Color = Color::new(0.60, 0.62, 0.70, 0.35);
pub const SCROLLBAR_MATCH: Color = Color::new(0.90, 0.70, 0.10, 0.80);
pub const SCROLLBAR_CURRENT_MATCH: Color = Color::new(1.0, 0.90, 0.20, 1.0);

// Hover highlights (overlay layer, semi-transparent)
pub const HOVER_FILE_TREE: Color = Color::new(1.0, 1.0, 1.0, 0.06);
pub const HOVER_TAB: Color = Color::new(1.0, 1.0, 1.0, 0.08);
pub const HOVER_CLOSE_BUTTON: Color = Color::new(1.0, 0.4, 0.4, 0.25);
pub const HOVER_PANEL_BORDER: Color = Color::new(0.35, 0.58, 1.0, 0.3);

// Search
pub const SEARCH_MATCH_BG: Color = Color::new(0.60, 0.50, 0.10, 0.35);
pub const SEARCH_CURRENT_BG: Color = Color::new(0.90, 0.70, 0.10, 0.55);
pub const SEARCH_BAR_BG: Color = Color::new(0.15, 0.15, 0.21, 1.0);
pub const SEARCH_BAR_BORDER: Color = Color::new(0.35, 0.58, 1.0, 0.5);
pub const SEARCH_BAR_TEXT: Color = Color::new(0.90, 0.91, 0.95, 1.0);
pub const SEARCH_BAR_COUNTER: Color = Color::new(0.55, 0.58, 0.68, 1.0);
pub const SEARCH_BAR_WIDTH: f32 = 260.0;
pub const SEARCH_BAR_HEIGHT: f32 = 28.0;
pub const SEARCH_BAR_CLOSE_SIZE: f32 = 20.0;
