// UI theme constants — Catppuccin-inspired depth-layered palette.

use tide_core::Color;

// Depth layers (darkest → lightest)
#[allow(dead_code)] // Used as reference; actual clear color is set in renderer
pub const WINDOW_BG: Color = Color::new(0.07, 0.07, 0.10, 1.0); // gap background
pub const TREE_BG: Color = Color::new(0.09, 0.09, 0.13, 1.0); // file tree
pub const PANE_BG: Color = Color::new(0.11, 0.11, 0.16, 1.0); // terminal pane
pub const PANE_BG_FOCUSED: Color = Color::new(0.13, 0.13, 0.18, 1.0); // focused pane

// Text
pub const TREE_TEXT_COLOR: Color = Color::new(0.70, 0.72, 0.80, 1.0);
pub const TREE_DIR_COLOR: Color = Color::new(0.35, 0.58, 1.0, 1.0);
pub const TREE_ICON_COLOR: Color = Color::new(0.55, 0.58, 0.68, 1.0);
pub const ACCENT_COLOR: Color = Color::new(0.35, 0.58, 1.0, 0.6); // focus accent bar

// Layout
pub const PANE_GAP: f32 = 4.0; // gap between panels
pub const PANE_RADIUS: f32 = 8.0; // rounded corner radius
pub const PANE_PADDING: f32 = 6.0; // inner padding (>= RADIUS/sqrt(2) ~ 5.66)
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
pub const PANEL_TAB_BG_ACTIVE: Color = Color::new(0.16, 0.16, 0.22, 1.0);
pub const PANEL_TAB_CLOSE_SIZE: f32 = 14.0;

// Drop preview
pub const DROP_PREVIEW_FILL: Color = Color::new(0.35, 0.58, 1.0, 0.15);
pub const DROP_PREVIEW_BORDER: Color = Color::new(0.35, 0.58, 1.0, 0.6);
pub const DROP_PREVIEW_BORDER_WIDTH: f32 = 2.0;
pub const DRAG_THRESHOLD: f32 = 5.0;
