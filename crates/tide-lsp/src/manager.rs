// LspManager: orchestrates multiple LspClients (one per language).
// Spec: docs/specs/lsp-completion.md

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use crate::client::{LspClient, LspMessage};
use crate::install::{self, InstallStatus};
use crate::protocol;

/// Language identification for LSP server selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Language {
    TypeScript,
    Python,
    Rust,
    Go,
}

impl Language {
    /// Detect language from file extension.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "ts" | "tsx" | "js" | "jsx" | "mjs" | "cjs" => Some(Self::TypeScript),
            "py" | "pyi" => Some(Self::Python),
            "rs" => Some(Self::Rust),
            "go" => Some(Self::Go),
            _ => None,
        }
    }

    /// The LSP language ID string.
    pub fn language_id(&self) -> &'static str {
        match self {
            Self::TypeScript => "typescript",
            Self::Python => "python",
            Self::Rust => "rust",
            Self::Go => "go",
        }
    }

    /// Default server command name and arguments.
    fn default_command(&self) -> (&'static str, Vec<&'static str>) {
        match self {
            Self::TypeScript => ("typescript-language-server", vec!["--stdio"]),
            Self::Python => ("pyright-langserver", vec!["--stdio"]),
            Self::Rust => ("rust-analyzer", vec![]),
            Self::Go => ("gopls", vec!["serve"]),
        }
    }

    /// Resolve the server command: check Tide-managed path first, then system PATH.
    /// Returns (command_path, args) or None if not available.
    fn resolve_command(&self, shell_path: &str) -> Option<(String, Vec<&'static str>)> {
        let (default_cmd, args) = self.default_command();

        // 1. Check Tide-managed install (~/.tide/lsp/)
        if let Some(managed_path) = install::managed_server_path(*self) {
            return Some((managed_path.to_string_lossy().to_string(), args));
        }

        // 2. Check system PATH
        if which_exists(default_cmd, shell_path) {
            return Some((default_cmd.to_string(), args));
        }

        None
    }
}

fn which_exists(cmd: &str, shell_path: &str) -> bool {
    std::process::Command::new("which")
        .arg(cmd)
        .env("PATH", shell_path)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

/// Resolve the user's full shell PATH by spawning a login shell.
/// Falls back to the current process PATH if the shell query fails.
fn resolve_shell_path() -> String {
    // Try to get PATH from a login shell
    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
    if let Ok(output) = std::process::Command::new(&shell)
        .args(["-l", "-c", "echo $PATH"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !path.is_empty() {
            return path;
        }
    }
    std::env::var("PATH").unwrap_or_default()
}

/// A completion response ready to be consumed by the UI.
pub struct CompletionResponse {
    pub request_id: u64,
    pub uri: String,
    pub items: Vec<CompletionItemData>,
}

/// Completion item data from the server, ready for UI consumption.
pub struct CompletionItemData {
    pub label: String,
    pub kind: Option<u32>,
    pub insert_text: Option<String>,
    pub sort_text: Option<String>,
    pub filter_text: Option<String>,
}

/// Manages all language server instances.
pub struct LspManager {
    clients: HashMap<Language, LspClient>,
    root_path: PathBuf,
    waker: Option<Arc<dyn Fn() + Send + Sync>>,
    /// Pending completion request: (language, request_id, uri)
    pending_completion: Option<(Language, u64, String)>,
    /// Track open documents per language: uri → language
    open_docs: HashMap<String, Language>,
    /// User's full shell PATH (resolved at creation time).
    shell_path: String,
    /// Installation status per language (for auto-install).
    install_status: HashMap<Language, InstallStatus>,
    /// Pending installs: language → join handle (background threads).
    install_handles: HashMap<Language, std::thread::JoinHandle<bool>>,
    /// Queued did_open calls waiting for install to complete: (uri, lang, text).
    pending_opens: Vec<(String, Language, String)>,
}

impl LspManager {
    pub fn new(root_path: PathBuf, waker: Option<Arc<dyn Fn() + Send + Sync>>) -> Self {
        let shell_path = resolve_shell_path();
        log::info!("LSP: resolved shell PATH: {}", &shell_path[..shell_path.len().min(200)]);
        Self {
            clients: HashMap::new(),
            root_path,
            waker,
            pending_completion: None,
            open_docs: HashMap::new(),
            shell_path,
            install_status: HashMap::new(),
            install_handles: HashMap::new(),
            pending_opens: Vec::new(),
        }
    }

    /// Ensure a language server is running for the given language.
    /// Returns true if the server is ready (or starting up).
    /// If the server binary isn't found, triggers background auto-install.
    pub fn ensure_server(&mut self, lang: Language) -> bool {
        if self.clients.contains_key(&lang) {
            return true;
        }

        // Try to resolve the server command (managed path or system PATH)
        if let Some((cmd, args)) = lang.resolve_command(&self.shell_path) {
            let args_refs: Vec<&str> = args.iter().map(|s| *s).collect();
            match LspClient::start(&cmd, &args_refs, &self.root_path, self.waker.clone(), &self.shell_path) {
                Some(client) => {
                    log::info!("LSP: started {} for {:?}", cmd, lang);
                    self.install_status.insert(lang, InstallStatus::Installed);
                    self.clients.insert(lang, client);
                    return true;
                }
                None => {
                    log::warn!("LSP: failed to start {} for {:?}", cmd, lang);
                    return false;
                }
            }
        }

        // Server not found — trigger auto-install if not already attempted
        let status = self.install_status.get(&lang).copied().unwrap_or(InstallStatus::NotAttempted);
        match status {
            InstallStatus::NotAttempted => {
                log::info!("LSP: {:?} server not found, starting auto-install", lang);
                self.install_status.insert(lang, InstallStatus::Installing);
                let shell_path = self.shell_path.clone();
                let waker = self.waker.clone();
                let handle = install::install_in_background(lang, shell_path, waker);
                self.install_handles.insert(lang, handle);
                false
            }
            InstallStatus::Installing => false, // Still installing
            InstallStatus::Installed => false,   // Installed but start failed (shouldn't happen)
            InstallStatus::Failed => false,      // Won't retry
        }
    }

    /// Get trigger characters for a language.
    pub fn trigger_characters(&self, lang: Language) -> &[String] {
        self.clients.get(&lang)
            .map(|c| c.trigger_characters.as_slice())
            .unwrap_or(&[])
    }

    /// Notify the server that a file was opened.
    pub fn did_open(&mut self, uri: &str, lang: Language, text: &str) {
        if !self.ensure_server(lang) {
            // Server not available yet — queue the open for when install completes
            let status = self.install_status.get(&lang).copied().unwrap_or(InstallStatus::NotAttempted);
            if status == InstallStatus::Installing {
                self.pending_opens.push((uri.to_string(), lang, text.to_string()));
            }
            return;
        }
        self.open_docs.insert(uri.to_string(), lang);
        if let Some(client) = self.clients.get_mut(&lang) {
            client.did_open(uri, lang.language_id(), text);
        }
    }

    /// Notify the server that a file was changed.
    pub fn did_change(&mut self, uri: &str, text: &str) {
        if let Some(&lang) = self.open_docs.get(uri) {
            if let Some(client) = self.clients.get_mut(&lang) {
                client.did_change(uri, text);
            }
        }
    }

    /// Notify the server that a file was saved.
    pub fn did_save(&mut self, uri: &str) {
        if let Some(&lang) = self.open_docs.get(uri) {
            if let Some(client) = self.clients.get_mut(&lang) {
                client.did_save(uri);
            }
        }
    }

    /// Notify the server that a file was closed.
    pub fn did_close(&mut self, uri: &str) {
        if let Some(lang) = self.open_docs.remove(uri) {
            if let Some(client) = self.clients.get_mut(&lang) {
                client.did_close(uri);
            }
            // Stop server if no more files of this language are open
            if !self.open_docs.values().any(|l| *l == lang) {
                if let Some(mut client) = self.clients.remove(&lang) {
                    log::info!("LSP: stopping server for {:?} (no more open files)", lang);
                    client.shutdown();
                }
            }
        }
    }

    /// Request completion at a position. The response will arrive asynchronously.
    pub fn request_completion(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        trigger_kind: u32,
        trigger_character: Option<&str>,
    ) {
        if let Some(&lang) = self.open_docs.get(uri) {
            if let Some(client) = self.clients.get_mut(&lang) {
                let id = client.request_completion(uri, line, character, trigger_kind, trigger_character);
                self.pending_completion = Some((lang, id, uri.to_string()));
            }
        }
    }

    /// Poll for completion responses. Call this from the main event loop.
    /// Returns a CompletionResponse if one is ready.
    pub fn poll(&mut self) -> Option<CompletionResponse> {
        // Check for completed background installations
        let installing_langs: Vec<Language> = self.install_handles.keys().copied().collect();
        for lang in installing_langs {
            let done = self.install_handles.get(&lang)
                .map(|h| h.is_finished())
                .unwrap_or(false);
            if done {
                if let Some(handle) = self.install_handles.remove(&lang) {
                    match handle.join() {
                        Ok(true) => {
                            log::info!("LSP: {:?} auto-install completed successfully", lang);
                            self.install_status.insert(lang, InstallStatus::Installed);
                            // Try to start the server now and replay pending opens
                            if self.ensure_server(lang) {
                                let pending: Vec<(String, Language, String)> = self.pending_opens
                                    .drain(..)
                                    .filter(|(_, l, _)| *l == lang)
                                    .collect();
                                for (uri, l, text) in pending {
                                    self.did_open(&uri, l, &text);
                                }
                            }
                        }
                        Ok(false) => {
                            log::warn!("LSP: {:?} auto-install failed", lang);
                            self.install_status.insert(lang, InstallStatus::Failed);
                        }
                        Err(_) => {
                            log::error!("LSP: {:?} install thread panicked", lang);
                            self.install_status.insert(lang, InstallStatus::Failed);
                        }
                    }
                }
            }
        }

        // Poll for pending initializations
        let langs: Vec<Language> = self.clients.keys().copied().collect();
        for lang in langs {
            if let Some(client) = self.clients.get_mut(&lang) {
                if !client.initialized {
                    client.poll_init();
                }
            }
        }

        let (lang, expected_id, ref uri) = self.pending_completion.as_ref()?;
        let lang = *lang;
        let expected_id = *expected_id;
        let uri = uri.clone();

        let client = self.clients.get_mut(&lang)?;

        while let Ok(msg) = client.rx.try_recv() {
            match msg {
                LspMessage::Response { id, result, error } => {
                    if id == expected_id {
                        self.pending_completion = None;
                        if error.is_some() {
                            return None;
                        }
                        return result.and_then(|v| parse_completion_response(v, &uri));
                    }
                }
                LspMessage::ServerExited => {
                    log::warn!("LSP: {:?} server exited unexpectedly", lang);
                    self.clients.remove(&lang);
                    self.pending_completion = None;
                    return None;
                }
                LspMessage::Notification { .. } => {
                    // Diagnostics, etc. — ignore for now
                }
            }
        }
        None
    }

    /// Shut down all language servers.
    pub fn shutdown_all(&mut self) {
        for (lang, mut client) in self.clients.drain() {
            log::info!("LSP: shutting down {:?}", lang);
            client.shutdown();
        }
    }

    /// Check if any server is running.
    pub fn has_any_server(&self) -> bool {
        !self.clients.is_empty()
    }
}

impl Drop for LspManager {
    fn drop(&mut self) {
        self.shutdown_all();
    }
}

fn parse_completion_response(value: serde_json::Value, uri: &str) -> Option<CompletionResponse> {
    // Response can be CompletionList or Vec<CompletionItem>
    let items = if let Ok(list) = serde_json::from_value::<protocol::CompletionList>(value.clone()) {
        list.items
    } else if let Ok(items) = serde_json::from_value::<Vec<protocol::LspCompletionItem>>(value) {
        items
    } else {
        return None;
    };

    let items: Vec<CompletionItemData> = items.into_iter().map(|item| {
        CompletionItemData {
            label: item.label,
            kind: item.kind,
            insert_text: item.insert_text,
            sort_text: item.sort_text,
            filter_text: item.filter_text,
        }
    }).collect();

    if items.is_empty() {
        return None;
    }

    Some(CompletionResponse {
        request_id: 0,
        uri: uri.to_string(),
        items,
    })
}

/// Convert a file path to an LSP URI.
pub fn path_to_uri(path: &std::path::Path) -> String {
    format!("file://{}", path.display())
}
