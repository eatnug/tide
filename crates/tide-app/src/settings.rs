// Settings persistence: global app configuration stored separately from session state.
// Uses platform-native config dir: e.g. ~/Library/Application Support/tide/settings.json
// on macOS, ~/.config/tide/settings.json on Linux.

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

