// Session persistence: save/restore workspace state across app restarts.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tide_core::{PaneId, Renderer, SplitDirection};
use tide_layout::{LayoutSnapshot, SplitLayout};

use crate::pane::PaneKind;
use crate::App;

// ──────────────────────────────────────────────
// Serializable session types
// ──────────────────────────────────────────────

#[derive(Serialize, Deserialize)]
pub struct Session {
    pub layout: SessionLayout,
    pub editor_tabs: Vec<SessionEditorTab>,
    pub editor_active_index: Option<usize>,
    pub focused_pane_id: Option<u64>,
    pub show_file_tree: bool,
    pub file_tree_width: f32,
    pub show_editor_panel: bool,
    pub editor_panel_width: f32,
    pub dark_mode: bool,
    pub window_width: f32,
    pub window_height: f32,
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

#[derive(Serialize, Deserialize)]
pub struct SessionEditorTab {
    pub pane_id: u64,
    pub file_path: PathBuf,
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

        let mut editor_tabs = Vec::new();
        let mut editor_active_index = None;
        for &tab_id in app.editor_panel_tabs.iter() {
            if let Some(PaneKind::Editor(editor)) = app.panes.get(&tab_id) {
                if let Some(path) = editor.editor.file_path() {
                    editor_tabs.push(SessionEditorTab {
                        pane_id: tab_id,
                        file_path: path.to_path_buf(),
                    });
                    if app.editor_panel_active == Some(tab_id) {
                        editor_active_index = Some(editor_tabs.len() - 1);
                    }
                }
            }
        }

        let logical_w = app.window_size.width as f32 / app.scale_factor;
        let logical_h = app.window_size.height as f32 / app.scale_factor;

        Session {
            layout,
            editor_tabs,
            editor_active_index,
            focused_pane_id: app.focused,
            show_file_tree: app.show_file_tree,
            file_tree_width: app.file_tree_width,
            show_editor_panel: app.show_editor_panel,
            editor_panel_width: app.editor_panel_width,
            dark_mode: app.dark_mode,
            window_width: logical_w,
            window_height: logical_h,
        }
    }
}

fn snapshot_to_session(snap: &LayoutSnapshot, app: &App) -> SessionLayout {
    match snap {
        LayoutSnapshot::Leaf(id) => {
            let cwd = match app.panes.get(id) {
                Some(PaneKind::Terminal(pane)) => pane.backend.detect_cwd_fallback(),
                _ => None,
            };
            SessionLayout::Leaf {
                pane_id: *id,
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

        // Create terminal panes
        let cell_size = match self.renderer.as_ref() {
            Some(r) => r.cell_size(),
            None => return false,
        };
        let logical = self.logical_size();
        let cols = (logical.width / 2.0 / cell_size.width).max(1.0) as u16;
        let rows = (logical.height / cell_size.height).max(1.0) as u16;

        for (pane_id, cwd) in &pane_infos {
            match crate::pane::TerminalPane::with_cwd(*pane_id, cols, rows, cwd.clone()) {
                Ok(pane) => {
                    self.install_pty_waker(&pane);
                    self.panes.insert(*pane_id, PaneKind::Terminal(pane));
                }
                Err(e) => {
                    log::error!("Failed to create terminal pane {}: {}", pane_id, e);
                    return false;
                }
            }
        }

        // Restore editor panel tabs (skip files that no longer exist)
        let mut restored_tabs = Vec::new();
        let mut active_tab: Option<PaneId> = None;
        for (i, tab) in session.editor_tabs.iter().enumerate() {
            if !tab.file_path.is_file() {
                continue;
            }
            let new_id = self.layout.alloc_id();
            match crate::editor_pane::EditorPane::open(new_id, &tab.file_path) {
                Ok(pane) => {
                    self.panes.insert(new_id, PaneKind::Editor(pane));
                    restored_tabs.push(new_id);
                    self.watch_file(&tab.file_path);
                    if session.editor_active_index == Some(i) {
                        active_tab = Some(new_id);
                    }
                }
                Err(e) => {
                    log::error!("Failed to restore editor tab {:?}: {}", tab.file_path, e);
                }
            }
        }

        self.editor_panel_tabs = restored_tabs;
        self.editor_panel_active = active_tab.or_else(|| self.editor_panel_tabs.last().copied());

        // Restore UI state
        self.show_file_tree = session.show_file_tree;
        self.file_tree_width = session.file_tree_width;
        self.show_editor_panel = session.show_editor_panel && !self.editor_panel_tabs.is_empty();
        self.editor_panel_width = session.editor_panel_width;
        if session.show_editor_panel && !self.editor_panel_tabs.is_empty() {
            self.editor_panel_width_manual = true;
        }
        self.dark_mode = session.dark_mode;

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
}

fn session_to_snapshot(
    layout: &SessionLayout,
    pane_infos: &mut Vec<(PaneId, Option<PathBuf>)>,
) -> Option<LayoutSnapshot> {
    match layout {
        SessionLayout::Leaf { pane_id, cwd } => {
            pane_infos.push((*pane_id, cwd.clone()));
            Some(LayoutSnapshot::Leaf(*pane_id))
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
