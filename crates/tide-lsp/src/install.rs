// Auto-installation of LSP server binaries.
// Downloads/installs servers to ~/.tide/lsp/ so they work without
// requiring system-wide installation.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::manager::Language;

/// Base directory for Tide-managed LSP servers.
pub fn lsp_base_dir() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join(".tide")
        .join("lsp")
}

/// Binary directory for standalone executables (rust-analyzer, gopls).
pub fn lsp_bin_dir() -> PathBuf {
    lsp_base_dir().join("bin")
}

/// npm directory for Node.js-based servers.
fn lsp_npm_dir() -> PathBuf {
    lsp_base_dir().join("npm")
}

/// Get the path to a Tide-managed LSP server binary, if installed.
pub fn managed_server_path(lang: Language) -> Option<PathBuf> {
    let path = match lang {
        Language::Rust => lsp_bin_dir().join("rust-analyzer"),
        Language::Go => lsp_bin_dir().join("gopls"),
        Language::TypeScript => lsp_npm_dir().join("node_modules").join(".bin").join("typescript-language-server"),
        Language::Python => lsp_npm_dir().join("node_modules").join(".bin").join("pyright-langserver"),
    };
    if path.exists() {
        Some(path)
    } else {
        None
    }
}

/// Installation status for a language server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallStatus {
    /// Not yet attempted.
    NotAttempted,
    /// Currently being installed in background.
    Installing,
    /// Installation succeeded.
    Installed,
    /// Installation failed (won't retry until restart).
    Failed,
}

/// Install a language server in the background.
/// Calls waker when done so the event loop can pick up the result.
pub fn install_in_background(
    lang: Language,
    shell_path: String,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
) -> std::thread::JoinHandle<bool> {
    std::thread::Builder::new()
        .name(format!("lsp-install-{:?}", lang))
        .spawn(move || {
            let result = install_server(lang, &shell_path);
            if let Some(w) = waker {
                w();
            }
            result
        })
        .expect("failed to spawn LSP install thread")
}

/// Install a language server synchronously. Returns true on success.
fn install_server(lang: Language, shell_path: &str) -> bool {
    log::info!("LSP install: starting {:?}", lang);
    let result = match lang {
        Language::Rust => install_rust_analyzer(),
        Language::TypeScript => install_npm_server("typescript-language-server", &["typescript"], shell_path),
        Language::Python => install_npm_server("pyright", &[], shell_path),
        Language::Go => install_gopls(shell_path),
    };
    match result {
        Ok(()) => {
            log::info!("LSP install: {:?} succeeded", lang);
            true
        }
        Err(e) => {
            log::error!("LSP install: {:?} failed: {}", lang, e);
            false
        }
    }
}

/// Download rust-analyzer from GitHub releases.
fn install_rust_analyzer() -> Result<(), String> {
    let bin_dir = lsp_bin_dir();
    std::fs::create_dir_all(&bin_dir).map_err(|e| format!("mkdir: {}", e))?;

    let target = if cfg!(target_arch = "aarch64") {
        "aarch64-apple-darwin"
    } else {
        "x86_64-apple-darwin"
    };
    let asset = format!("rust-analyzer-{}.gz", target);

    // Get latest release tag
    let tag = get_latest_github_release("rust-lang", "rust-analyzer")?;
    let url = format!(
        "https://github.com/rust-lang/rust-analyzer/releases/download/{}/{}",
        tag, asset
    );

    log::info!("LSP install: downloading rust-analyzer from {}", url);

    let gz_path = bin_dir.join("rust-analyzer.gz");
    let bin_path = bin_dir.join("rust-analyzer");

    // Download with curl
    run_command("curl", &["-L", "-o", gz_path.to_str().unwrap(), &url], None, "")?;

    // Decompress
    run_command("gunzip", &["-f", gz_path.to_str().unwrap()], None, "")?;

    // Make executable
    run_command("chmod", &["+x", bin_path.to_str().unwrap()], None, "")?;

    Ok(())
}

/// Install an npm-based LSP server (typescript-language-server, pyright).
fn install_npm_server(package: &str, extra_packages: &[&str], shell_path: &str) -> Result<(), String> {
    // Check if node/npm is available
    if !command_exists("node", shell_path) {
        return Err("Node.js is not installed — required for this LSP server".to_string());
    }

    let npm_dir = lsp_npm_dir();
    std::fs::create_dir_all(&npm_dir).map_err(|e| format!("mkdir: {}", e))?;

    // Initialize package.json if missing
    let pkg_json = npm_dir.join("package.json");
    if !pkg_json.exists() {
        std::fs::write(&pkg_json, "{\"private\": true}")
            .map_err(|e| format!("write package.json: {}", e))?;
    }

    // npm install
    let mut args = vec!["install", package];
    args.extend(extra_packages);
    run_command("npm", &args, Some(&npm_dir), shell_path)?;

    Ok(())
}

/// Install gopls using `go install`.
fn install_gopls(shell_path: &str) -> Result<(), String> {
    if !command_exists("go", shell_path) {
        return Err("Go is not installed — required for gopls".to_string());
    }

    let bin_dir = lsp_bin_dir();
    std::fs::create_dir_all(&bin_dir).map_err(|e| format!("mkdir: {}", e))?;

    // GOBIN controls where `go install` puts the binary
    run_command_with_env(
        "go",
        &["install", "golang.org/x/tools/gopls@latest"],
        &[("GOBIN", bin_dir.to_str().unwrap())],
        shell_path,
    )
}

/// Get the latest release tag from a GitHub repo using the GitHub API.
fn get_latest_github_release(owner: &str, repo: &str) -> Result<String, String> {
    let url = format!(
        "https://api.github.com/repos/{}/{}/releases/latest",
        owner, repo
    );
    let output = std::process::Command::new("curl")
        .args(["-s", "-L", "-H", "Accept: application/vnd.github.v3+json", &url])
        .output()
        .map_err(|e| format!("curl: {}", e))?;

    if !output.status.success() {
        return Err("GitHub API request failed".to_string());
    }

    let body = String::from_utf8_lossy(&output.stdout);
    // Simple JSON parsing for "tag_name": "..."
    let tag = body
        .split("\"tag_name\"")
        .nth(1)
        .and_then(|s| s.split('"').nth(1))
        .ok_or_else(|| "Failed to parse release tag from GitHub API".to_string())?;

    Ok(tag.to_string())
}

fn command_exists(cmd: &str, shell_path: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .env("PATH", shell_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn run_command(cmd: &str, args: &[&str], cwd: Option<&Path>, shell_path: &str) -> Result<(), String> {
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    if !shell_path.is_empty() {
        command.env("PATH", shell_path);
    }
    if let Some(dir) = cwd {
        command.current_dir(dir);
    }
    let output = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("{} failed to execute: {}", cmd, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} exited with {}: {}", cmd, output.status, stderr.chars().take(500).collect::<String>()));
    }
    Ok(())
}

fn run_command_with_env(
    cmd: &str,
    args: &[&str],
    env: &[(&str, &str)],
    shell_path: &str,
) -> Result<(), String> {
    let mut command = std::process::Command::new(cmd);
    command.args(args);
    if !shell_path.is_empty() {
        command.env("PATH", shell_path);
    }
    for (k, v) in env {
        command.env(k, v);
    }
    let output = command
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()
        .map_err(|e| format!("{} failed to execute: {}", cmd, e))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!("{} exited with {}: {}", cmd, output.status, stderr.chars().take(500).collect::<String>()));
    }
    Ok(())
}
