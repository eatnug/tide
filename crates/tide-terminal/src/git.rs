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

/// Information about a git worktree.
#[derive(Debug, Clone)]
pub struct WorktreeInfo {
    pub path: std::path::PathBuf,
    pub branch: Option<String>,
    pub commit: String,
    pub is_main: bool,
    pub is_current: bool,
}

/// List all worktrees for the repository containing `cwd`.
pub fn list_worktrees(cwd: &Path) -> Vec<WorktreeInfo> {
    let text = match run_git(&["worktree", "list", "--porcelain"], cwd) {
        Some(t) => t,
        None => return Vec::new(),
    };

    // Determine the canonical path of `cwd` for is_current detection
    let cwd_canonical = std::fs::canonicalize(cwd).unwrap_or_else(|_| cwd.to_path_buf());

    let mut worktrees = Vec::new();
    let mut current_path: Option<std::path::PathBuf> = None;
    let mut current_commit = String::new();
    let mut current_branch: Option<String> = None;
    let mut is_main = false;
    let mut entry_index = 0;

    for line in text.lines() {
        if let Some(path_str) = line.strip_prefix("worktree ") {
            // Flush previous entry
            if let Some(path) = current_path.take() {
                let is_current = std::fs::canonicalize(&path)
                    .unwrap_or_else(|_| path.clone())
                    == cwd_canonical;
                worktrees.push(WorktreeInfo {
                    path,
                    branch: current_branch.take(),
                    commit: std::mem::take(&mut current_commit),
                    is_main,
                    is_current,
                });
            }
            current_path = Some(std::path::PathBuf::from(path_str));
            current_commit = String::new();
            current_branch = None;
            is_main = entry_index == 0; // first worktree is the main one
            entry_index += 1;
        } else if let Some(hash) = line.strip_prefix("HEAD ") {
            current_commit = hash.to_string();
        } else if let Some(branch_ref) = line.strip_prefix("branch ") {
            // branch refs/heads/main → "main"
            current_branch = Some(
                branch_ref
                    .strip_prefix("refs/heads/")
                    .unwrap_or(branch_ref)
                    .to_string(),
            );
        }
        // "bare", "detached", blank lines are skipped
    }

    // Flush last entry
    if let Some(path) = current_path.take() {
        let is_current = std::fs::canonicalize(&path)
            .unwrap_or_else(|_| path.clone())
            == cwd_canonical;
        worktrees.push(WorktreeInfo {
            path,
            branch: current_branch.take(),
            commit: current_commit,
            is_main,
            is_current,
        });
    }

    worktrees
}

/// Add a new worktree. If `new_branch` is true, creates a new branch.
pub fn add_worktree(
    cwd: &Path,
    path: &Path,
    branch: &str,
    new_branch: bool,
) -> Result<(), String> {
    let path_str = path.to_string_lossy().to_string();
    let mut args = vec!["worktree", "add"];
    if new_branch {
        args.push("-b");
        args.push(branch);
        args.push(&path_str);
    } else {
        args.push(&path_str);
        args.push(branch);
    }

    let output = Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(stderr.trim().to_string())
    }
}

/// Remove a worktree. If `force` is true, uses --force flag.
pub fn remove_worktree(cwd: &Path, path: &Path, force: bool) -> Result<(), String> {
    let path_str = path.to_string_lossy().to_string();
    let mut args = vec!["worktree", "remove"];
    if force {
        args.push("--force");
    }
    args.push(&path_str);

    let output = Command::new("git")
        .args(&args)
        .current_dir(cwd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("Failed to run git: {}", e))?;

    if output.status.success() {
        Ok(())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        Err(stderr.trim().to_string())
    }
}

/// Count worktrees for a repo (lightweight, for badge display).
pub fn count_worktrees(cwd: &Path) -> usize {
    let text = match run_git(&["worktree", "list", "--porcelain"], cwd) {
        Some(t) => t,
        None => return 0,
    };
    text.lines().filter(|l| l.starts_with("worktree ")).count()
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
