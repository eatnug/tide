// Git integration: branch detection and status parsing via CLI.
// All functions are called from background threads — never from the main/render thread.

use std::path::Path;
use std::process::Command;

/// Run a git command. Returns None if the command fails.
/// Safe to call from background threads only (may block on git I/O).
fn run_git(args: &[&str], cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;
    if output.status.success() {
        Some(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        None
    }
}

/// Git repository information for a working directory.
#[derive(Debug, Clone)]
pub struct GitInfo {
    pub branch: String,
    pub status: GitStatus,
}

/// Summary of uncommitted changes.
#[derive(Debug, Clone, Default)]
pub struct GitStatus {
    pub changed_files: usize,
    pub additions: usize,
    pub deletions: usize,
}

/// Detect git branch and status for the given working directory.
/// Returns None if not inside a git repo or git is not available.
pub fn detect_git_info(cwd: &Path) -> Option<GitInfo> {
    let branch = detect_branch(cwd)?;
    let status = detect_status(cwd);
    Some(GitInfo { branch, status })
}

fn detect_branch(cwd: &Path) -> Option<String> {
    let text = run_git(&["rev-parse", "--abbrev-ref", "HEAD"], cwd)?;
    let branch = text.trim().to_string();
    if branch.is_empty() { None } else { Some(branch) }
}

/// A single file entry from `git status --porcelain`.
#[derive(Debug, Clone)]
pub struct StatusEntry {
    pub status: String,
    pub path: String,
}

/// Information about a git branch.
#[derive(Debug, Clone)]
pub struct BranchInfo {
    pub name: String,
    pub is_current: bool,
    pub is_remote: bool,
}

/// List files with their status from `git status --porcelain`.
pub fn status_files(cwd: &Path) -> Vec<StatusEntry> {
    let text = match run_git(&["status", "--porcelain"], cwd) {
        Some(t) => t,
        None => return Vec::new(),
    };
    text.lines()
        .filter(|l| l.len() >= 4)
        .map(|l| StatusEntry {
            status: l[..2].to_string(),
            path: l[3..].to_string(),
        })
        .collect()
}

/// Get unified diff for a single file.
pub fn file_diff(cwd: &Path, path: &str) -> Option<String> {
    let text = run_git(&["diff", "--", path], cwd)?;
    if text.is_empty() { None } else { Some(text) }
}

/// List local branches only.
pub fn list_branches(cwd: &Path) -> Vec<BranchInfo> {
    let text = match run_git(&["branch", "--format=%(HEAD) %(refname:short)"], cwd) {
        Some(t) => t,
        None => return Vec::new(),
    };
    text.lines()
        .filter(|l| l.len() >= 2)
        .map(|l| {
            let is_current = l.starts_with('*');
            let name = l[2..].trim().to_string();
            BranchInfo { name, is_current, is_remote: false }
        })
        .collect()
}

fn detect_status(cwd: &Path) -> GitStatus {
    let mut status = GitStatus::default();

    if let Some(text) = run_git(&["status", "--porcelain"], cwd) {
        status.changed_files = text.lines().filter(|l| !l.is_empty()).count();
    }

    if let Some(text) = run_git(&["diff", "--numstat"], cwd) {
        for line in text.lines() {
            let parts: Vec<&str> = line.split('\t').collect();
            if parts.len() >= 2 {
                status.additions += parts[0].parse::<usize>().unwrap_or(0);
                status.deletions += parts[1].parse::<usize>().unwrap_or(0);
            }
        }
    }

    status
}
