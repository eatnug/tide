// Session persistence: save/restore workspace state across app restarts.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tide_core::{PaneId, SplitDirection};
use tide_layout::{LayoutSnapshot, SplitLayout};

use crate::pane::PaneKind;
use crate::App;

// ──────────────────────────────────────────────
// Serializable session types
// ──────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub layout: SessionLayout,
    pub focused_pane_id: Option<u64>,
    pub show_file_tree: bool,
    pub file_tree_width: f32,
    pub dark_mode: bool,
    pub window_width: f32,
    pub window_height: f32,
    #[serde(default = "default_sidebar_side")]
    pub sidebar_side: String,
    #[serde(default = "default_sidebar_outer")]
    pub sidebar_outer: bool,
}

fn default_sidebar_side() -> String {
    "left".to_string()
}

fn default_sidebar_outer() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
pub enum SessionLayout {
    Leaf {
        pane_id: u64,
        cwd: Option<PathBuf>,
    },
    Split {
        direction: String, // "horizontal" or "vertical"
        ratio: f32,
        left: Box<SessionLayout>,
        right: Box<SessionLayout>,
    },
}

// ──────────────────────────────────────────────
// Session file I/O
// ──────────────────────────────────────────────

fn session_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;
    Some(config_dir.join("tide").join("session.json"))
}

pub fn save_session(session: &Session) {
    let path = match session_path() {
        Some(p) => p,
        None => {
            log::warn!("Could not determine config directory for session save");
            return;
        }
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("Failed to create session directory: {}", e);
            return;
        }
    }

    match serde_json::to_string_pretty(session) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("Failed to write session file: {}", e);
            }
        }
        Err(e) => {
            log::error!("Failed to serialize session: {}", e);
        }
    }
}

pub fn load_session() -> Option<Session> {
    let path = session_path()?;
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

// ──────────────────────────────────────────────
// Running marker for crash recovery
// ──────────────────────────────────────────────

fn running_marker_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;
    Some(config_dir.join("tide").join("running"))
}

pub fn create_running_marker() {
    let path = match running_marker_path() {
        Some(p) => p,
        None => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, "");
}

pub fn delete_running_marker() {
    if let Some(path) = running_marker_path() {
        let _ = std::fs::remove_file(&path);
    }
}

pub fn is_crash_recovery() -> bool {
    running_marker_path().is_some_and(|p| p.exists())
}

// ──────────────────────────────────────────────
// Capture session from app state
// ──────────────────────────────────────────────

impl Session {
    pub fn from_app(app: &App) -> Self {
        let layout = match app.layout.snapshot() {
            Some(snap) => snapshot_to_session(&snap, app),
            None => SessionLayout::Leaf {
                pane_id: 1,
                cwd: std::env::current_dir().ok(),
            },
        };

        let logical_w = app.window_size.0 as f32 / app.scale_factor;
        let logical_h = app.window_size.1 as f32 / app.scale_factor;

        Session {
            layout,
            focused_pane_id: app.focused,
            show_file_tree: app.show_file_tree,
            file_tree_width: app.file_tree_width,
            dark_mode: app.dark_mode,
            window_width: logical_w,
            window_height: logical_h,
            sidebar_side: match app.sidebar_side {
                crate::LayoutSide::Left => "left".to_string(),
                crate::LayoutSide::Right => "right".to_string(),
            },
            sidebar_outer: true, // sidebar is always outermost
        }
    }
}

fn snapshot_to_session(snap: &LayoutSnapshot, app: &App) -> SessionLayout {
    match snap {
        LayoutSnapshot::Leaf { tabs, active } => {
            let id = tabs[*active];
            let cwd = match app.panes.get(&id) {
                Some(PaneKind::Terminal(pane)) => pane.backend.detect_cwd_fallback(),
                _ => None,
            };
            SessionLayout::Leaf {
                pane_id: id,
                cwd,
            }
        }
        LayoutSnapshot::Split {
            direction,
            ratio,
            left,
            right,
        } => SessionLayout::Split {
            direction: match direction {
                SplitDirection::Horizontal => "horizontal".to_string(),
                SplitDirection::Vertical => "vertical".to_string(),
            },
            ratio: *ratio,
            left: Box::new(snapshot_to_session(left, app)),
            right: Box::new(snapshot_to_session(right, app)),
        },
    }
}

// ──────────────────────────────────────────────
// Restore session into app
// ──────────────────────────────────────────────

impl App {
    pub(crate) fn restore_from_session(&mut self, session: Session) -> bool {
        // Rebuild layout tree from session, collecting pane info
        let mut pane_infos: Vec<(PaneId, Option<PathBuf>)> = Vec::new();
        let snap = match session_to_snapshot(&session.layout, &mut pane_infos) {
            Some(s) => s,
            None => return false,
        };

        self.layout = SplitLayout::from_snapshot(snap);

        // Apply dark mode early so pane creation uses the correct palette
        self.dark_mode = session.dark_mode;

        // Create terminal panes
        let cell_size = self.cell_size();
        let logical = self.logical_size();
        let cols = if cell_size.width > 0.0 {
            ((logical.width / 2.0 / cell_size.width).max(1.0).min(1000.0)) as u16
        } else {
            80
        };
        let rows = if cell_size.height > 0.0 {
            ((logical.height / cell_size.height).max(1.0).min(500.0)) as u16
        } else {
            24
        };

        for (pane_id, cwd) in &pane_infos {
            match crate::pane::TerminalPane::with_cwd(*pane_id, cols, rows, cwd.clone(), self.dark_mode) {
                Ok(pane) => {
                    self.install_pty_waker(&pane);
                    self.panes.insert(*pane_id, PaneKind::Terminal(pane));
                    self.pending_ime_proxy_creates.push(*pane_id);
                }
                Err(e) => {
                    log::error!("Failed to create terminal pane {}: {}", pane_id, e);
                    return false;
                }
            }
        }

        // Restore UI state
        self.show_file_tree = session.show_file_tree;
        self.file_tree_width = session.file_tree_width;
        self.sidebar_side = match session.sidebar_side.as_str() {
            "right" => crate::LayoutSide::Right,
            _ => crate::LayoutSide::Left,
        };
        // Apply dark mode to renderer
        let border_color = self.palette().border_color;
        if let Some(renderer) = &mut self.renderer {
            renderer.clear_color = border_color;
        }

        // Resolve focus: try saved focus, fall back to first tree pane
        let all_pane_ids = self.layout.pane_ids();
        let focus_id = session
            .focused_pane_id
            .and_then(|id| {
                if self.panes.contains_key(&id) {
                    Some(id)
                } else {
                    None
                }
            })
            .or_else(|| all_pane_ids.first().copied());

        if let Some(id) = focus_id {
            self.focused = Some(id);
            self.router.set_focused(id);
        }

        // Initialize file tree
        let cwd = pane_infos
            .first()
            .and_then(|(_, c)| c.clone())
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from("/")));
        let tree = tide_tree::FsTree::new(cwd.clone());
        self.file_tree = Some(tree);
        self.last_cwd = Some(cwd);

        true
    }

    /// Restore only preferences (window size, theme, panel widths) from a session,
    /// then create a fresh initial pane. Used after intentional quit.
    pub(crate) fn restore_preferences(&mut self, session: &Session, early_terminal: Option<tide_terminal::Terminal>) {
        self.file_tree_width = session.file_tree_width;
        self.dark_mode = session.dark_mode;
        self.sidebar_side = match session.sidebar_side.as_str() {
            "right" => crate::LayoutSide::Right,
            _ => crate::LayoutSide::Left,
        };

        // Apply dark mode to renderer
        let border_color = self.palette().border_color;
        if let Some(renderer) = &mut self.renderer {
            renderer.clear_color = border_color;
        }

        self.create_initial_pane(early_terminal);
    }
}

/// Convert a `SessionLayout` to a `LayoutSnapshot`, collecting pane info.
/// Public for testing.
fn session_to_snapshot(
    layout: &SessionLayout,
    pane_infos: &mut Vec<(PaneId, Option<PathBuf>)>,
) -> Option<LayoutSnapshot> {
    match layout {
        SessionLayout::Leaf { pane_id, cwd } => {
            pane_infos.push((*pane_id, cwd.clone()));
            Some(LayoutSnapshot::Leaf { tabs: vec![*pane_id], active: 0 })
        }
        SessionLayout::Split {
            direction,
            ratio,
            left,
            right,
        } => {
            let dir = match direction.as_str() {
                "horizontal" => SplitDirection::Horizontal,
                "vertical" => SplitDirection::Vertical,
                _ => return None,
            };
            let l = session_to_snapshot(left, pane_infos)?;
            let r = session_to_snapshot(right, pane_infos)?;
            Some(LayoutSnapshot::Split {
                direction: dir,
                ratio: *ratio,
                left: Box::new(l),
                right: Box::new(r),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_layout_leaf_roundtrip() {
        let layout = SessionLayout::Leaf {
            pane_id: 42,
            cwd: Some(PathBuf::from("/home/user")),
        };
        let json = serde_json::to_string(&layout).unwrap();
        let restored: SessionLayout = serde_json::from_str(&json).unwrap();

        match restored {
            SessionLayout::Leaf { pane_id, cwd } => {
                assert_eq!(pane_id, 42);
                assert_eq!(cwd, Some(PathBuf::from("/home/user")));
            }
            _ => panic!("expected Leaf"),
        }
    }

    #[test]
    fn session_layout_split_roundtrip() {
        let layout = SessionLayout::Split {
            direction: "horizontal".to_string(),
            ratio: 0.5,
            left: Box::new(SessionLayout::Leaf {
                pane_id: 1,
                cwd: None,
            }),
            right: Box::new(SessionLayout::Leaf {
                pane_id: 2,
                cwd: Some(PathBuf::from("/tmp")),
            }),
        };
        let json = serde_json::to_string(&layout).unwrap();
        let restored: SessionLayout = serde_json::from_str(&json).unwrap();

        match restored {
            SessionLayout::Split { direction, ratio, left, right } => {
                assert_eq!(direction, "horizontal");
                assert!((ratio - 0.5).abs() < f32::EPSILON);
                match *left {
                    SessionLayout::Leaf { pane_id, .. } => assert_eq!(pane_id, 1),
                    _ => panic!("expected Leaf"),
                }
                match *right {
                    SessionLayout::Leaf { pane_id, cwd } => {
                        assert_eq!(pane_id, 2);
                        assert_eq!(cwd, Some(PathBuf::from("/tmp")));
                    }
                    _ => panic!("expected Leaf"),
                }
            }
            _ => panic!("expected Split"),
        }
    }

    #[test]
    fn session_full_roundtrip() {
        let session = Session {
            layout: SessionLayout::Leaf { pane_id: 1, cwd: None },
            focused_pane_id: Some(1),
            show_file_tree: true,
            file_tree_width: 250.0,
            dark_mode: true,
            window_width: 960.0,
            window_height: 640.0,
            sidebar_side: "left".to_string(),
            sidebar_outer: true,
        };
        let json = serde_json::to_string(&session).unwrap();
        let restored: Session = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.focused_pane_id, Some(1));
        assert!(restored.show_file_tree);
        assert!((restored.file_tree_width - 250.0).abs() < f32::EPSILON);
        assert!(restored.dark_mode);
    }

    #[test]
    fn session_to_snapshot_leaf() {
        let layout = SessionLayout::Leaf {
            pane_id: 10,
            cwd: Some(PathBuf::from("/home")),
        };
        let mut pane_infos = Vec::new();
        let snap = session_to_snapshot(&layout, &mut pane_infos).unwrap();

        assert_eq!(pane_infos.len(), 1);
        assert_eq!(pane_infos[0].0, 10);
        assert_eq!(pane_infos[0].1, Some(PathBuf::from("/home")));

        match snap {
            LayoutSnapshot::Leaf { tabs, active } => {
                assert_eq!(tabs, vec![10]);
                assert_eq!(active, 0);
            }
            _ => panic!("expected Leaf"),
        }
    }

    #[test]
    fn session_to_snapshot_split() {
        let layout = SessionLayout::Split {
            direction: "vertical".to_string(),
            ratio: 0.6,
            left: Box::new(SessionLayout::Leaf { pane_id: 1, cwd: None }),
            right: Box::new(SessionLayout::Leaf { pane_id: 2, cwd: None }),
        };
        let mut pane_infos = Vec::new();
        let snap = session_to_snapshot(&layout, &mut pane_infos).unwrap();

        assert_eq!(pane_infos.len(), 2);

        match snap {
            LayoutSnapshot::Split { direction, ratio, .. } => {
                assert_eq!(direction, SplitDirection::Vertical);
                assert!((ratio - 0.6).abs() < f32::EPSILON);
            }
            _ => panic!("expected Split"),
        }
    }

    #[test]
    fn session_to_snapshot_invalid_direction() {
        let layout = SessionLayout::Split {
            direction: "diagonal".to_string(),
            ratio: 0.5,
            left: Box::new(SessionLayout::Leaf { pane_id: 1, cwd: None }),
            right: Box::new(SessionLayout::Leaf { pane_id: 2, cwd: None }),
        };
        let mut pane_infos = Vec::new();
        assert!(session_to_snapshot(&layout, &mut pane_infos).is_none());
    }

    #[test]
    fn session_defaults_for_missing_fields() {
        // Simulate old session file without sidebar_side and sidebar_outer
        let json = r#"{
            "layout": {"Leaf": {"pane_id": 1, "cwd": null}},
            "focused_pane_id": 1,
            "show_file_tree": false,
            "file_tree_width": 200.0,
            "dark_mode": true,
            "window_width": 800.0,
            "window_height": 600.0
        }"#;
        let session: Session = serde_json::from_str(json).unwrap();
        assert_eq!(session.sidebar_side, "left");
        assert!(session.sidebar_outer);
    }
}
