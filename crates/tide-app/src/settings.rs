// Settings persistence: global app configuration stored separately from session state.
// Follows VS Code pattern: ~/.config/tide/settings.json

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TideSettings {
    #[serde(default)]
    pub worktree: WorktreeSettings,
}

impl Default for TideSettings {
    fn default() -> Self {
        Self {
            worktree: WorktreeSettings::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorktreeSettings {
    /// Pattern for worktree base directory. Variables: {repo_root}, {branch}.
    /// Example: "{repo_root}.worktree/{branch}"
    #[serde(default)]
    pub base_dir_pattern: Option<String>,

    /// Template for worktree directory naming. Variables: {branch}, {date}.
    /// Example: "{branch}"
    #[serde(default)]
    pub naming_template: Option<String>,

    /// Files/directories to auto-symlink from main worktree into new worktrees.
    /// Example: [".env", "node_modules"]
    #[serde(default)]
    pub auto_symlink: Vec<String>,

    /// Command to run after creating a new worktree.
    /// Example: "npm install"
    #[serde(default)]
    pub post_create_command: Option<String>,
}

impl Default for WorktreeSettings {
    fn default() -> Self {
        Self {
            base_dir_pattern: None,
            naming_template: None,
            auto_symlink: Vec::new(),
            post_create_command: None,
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
            // Default: sibling directory named after the branch
            let parent = repo_root.parent().unwrap_or(repo_root);
            parent.join(&sanitized_branch)
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
        Ok(data) => serde_json::from_str(&data).unwrap_or_default(),
        Err(_) => TideSettings::default(),
    }
}

pub fn save_settings(settings: &TideSettings) {
    let path = match settings_path() {
        Some(p) => p,
        None => {
            log::warn!("Could not determine config directory for settings save");
            return;
        }
    };

    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            log::error!("Failed to create settings directory: {}", e);
            return;
        }
    }

    match serde_json::to_string_pretty(settings) {
        Ok(json) => {
            if let Err(e) = std::fs::write(&path, json) {
                log::error!("Failed to write settings file: {}", e);
            }
        }
        Err(e) => {
            log::error!("Failed to serialize settings: {}", e);
        }
    }
}
