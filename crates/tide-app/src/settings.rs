// Settings persistence: global app configuration stored separately from session state.
// Uses platform-native config dir: e.g. ~/Library/Application Support/tide/settings.json
// on macOS, ~/.config/tide/settings.json on Linux.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TideSettings {
    #[serde(default)]
    pub worktree: WorktreeSettings,
    #[serde(default)]
    pub keybindings: Vec<KeybindingOverride>,
}

impl Default for TideSettings {
    fn default() -> Self {
        Self {
            worktree: WorktreeSettings::default(),
            keybindings: Vec::new(),
        }
    }
}

/// A single keybinding override stored in settings.json.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeybindingOverride {
    pub action: String,
    pub key: String,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub meta: bool,
    #[serde(default)]
    pub alt: bool,
}

impl KeybindingOverride {
    /// Convert to a (Hotkey, GlobalAction) pair.
    pub fn to_binding(&self) -> Option<(tide_input::Hotkey, tide_input::GlobalAction)> {
        let action = tide_input::GlobalAction::from_action_key(&self.action)?;
        let key = tide_input::Hotkey::key_from_name(&self.key)?;
        let hotkey = tide_input::Hotkey::new(key, self.shift, self.ctrl, self.meta, self.alt);
        Some((hotkey, action))
    }

    /// Create from a Hotkey and GlobalAction.
    pub fn from_binding(hotkey: &tide_input::Hotkey, action: &tide_input::GlobalAction) -> Self {
        Self {
            action: action.action_key().to_string(),
            key: hotkey.key_name(),
            shift: hotkey.shift,
            ctrl: hotkey.ctrl,
            meta: hotkey.meta,
            alt: hotkey.alt,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSettings {
    /// Pattern for worktree base directory. Variables: {repo_root}, {branch}.
    /// Example: "{repo_root}.worktree/{branch}"
    #[serde(default)]
    pub base_dir_pattern: Option<String>,
}

impl Default for WorktreeSettings {
    fn default() -> Self {
        Self {
            base_dir_pattern: None,
        }
    }
}

impl WorktreeSettings {
    /// Compute the worktree path for a given branch name and repo root.
    pub fn compute_worktree_path(&self, repo_root: &std::path::Path, branch: &str) -> PathBuf {
        let sanitized_branch = branch.replace('/', "-");

        if let Some(ref pattern) = self.base_dir_pattern {
            let root_str = repo_root.to_string_lossy();
            let path_str = pattern
                .replace("{repo_root}", &root_str)
                .replace("{branch}", &sanitized_branch);
            PathBuf::from(path_str)
        } else {
            // Default: {repo_root}.worktree/{branch}
            let mut wt_dir = repo_root.as_os_str().to_owned();
            wt_dir.push(".worktree");
            PathBuf::from(wt_dir).join(&sanitized_branch)
        }
    }
}

fn settings_path() -> Option<PathBuf> {
    let config_dir = dirs::config_dir()?;
    Some(config_dir.join("tide").join("settings.json"))
}

pub fn load_settings() -> TideSettings {
    let path = match settings_path() {
        Some(p) => p,
        None => return TideSettings::default(),
    };

    match std::fs::read_to_string(&path) {
        Ok(data) => match serde_json::from_str(&data) {
            Ok(settings) => settings,
            Err(e) => {
                log::warn!("Failed to parse {}: {}", path.display(), e);
                TideSettings::default()
            }
        },
        Err(_) => TideSettings::default(),
    }
}

pub fn save_settings(settings: &TideSettings) {
    let path = match settings_path() {
        Some(p) => p,
        None => {
            log::warn!("Cannot determine settings path");
            return;
        }
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("Failed to create config dir {}: {}", parent.display(), e);
            return;
        }
    }

    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("Failed to write {}: {}", path.display(), e);
            }
        }
        Err(e) => {
            log::error!("Failed to serialize settings: {}", e);
        }
    }
}

/// Build a KeybindingMap from settings overrides.
pub fn build_keybinding_map(settings: &TideSettings) -> tide_input::KeybindingMap {
    if settings.keybindings.is_empty() {
        return tide_input::KeybindingMap::new();
    }
    let overrides: Vec<(tide_input::Hotkey, tide_input::GlobalAction)> = settings
        .keybindings
        .iter()
        .filter_map(|o| o.to_binding())
        .collect();
    tide_input::KeybindingMap::with_overrides(overrides)
}

