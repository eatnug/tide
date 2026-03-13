// LspClient: manages a single language server process.
// Runs a background reader thread that forwards server responses to the main thread.

use std::collections::HashMap;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

use crate::protocol::*;
use crate::transport;

/// A message from the LSP reader thread to the main thread.
#[derive(Debug)]
pub enum LspMessage {
    /// Response to a request (completion, initialize, etc.)
    Response {
        id: u64,
        result: Option<serde_json::Value>,
        error: Option<JsonRpcError>,
    },
    /// Server-initiated notification (diagnostics, etc.) — future use
    Notification {
        method: String,
        params: Option<serde_json::Value>,
    },
    /// The server process has exited
    ServerExited,
}

/// Handle to a running language server.
pub struct LspClient {
    child: Child,
    stdin: std::process::ChildStdin,
    next_id: u64,
    pub rx: mpsc::Receiver<LspMessage>,
    stop_flag: Arc<AtomicBool>,
    _reader_handle: std::thread::JoinHandle<()>,
    pub server_capabilities: ServerCapabilities,
    pub trigger_characters: Vec<String>,
    /// Document versions tracked by this client (uri → version).
    pub doc_versions: HashMap<String, i64>,
    /// Whether the initialize handshake has completed.
    pub initialized: bool,
    /// The request ID of the pending initialize request.
    init_request_id: Option<u64>,
    /// Queued notifications to send after initialize completes.
    pending_after_init: Vec<(String, Option<serde_json::Value>)>,
}

impl LspClient {
    /// Spawn a language server process and perform the initialize handshake.
    /// Returns None if the server binary is not found or initialization fails.
    pub fn start(
        command: &str,
        args: &[&str],
        root_path: &PathBuf,
        waker: Option<Arc<dyn Fn() + Send + Sync>>,
        shell_path: &str,
    ) -> Option<Self> {
        let mut child = Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .current_dir(root_path)
            .env("PATH", shell_path)
            .spawn()
            .ok()?;

        let stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;

        let (tx, rx) = mpsc::channel();
        let stop_flag = Arc::new(AtomicBool::new(false));
        let stop_clone = stop_flag.clone();

        let reader_handle = std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                if stop_clone.load(Ordering::Relaxed) {
                    break;
                }
                match transport::read_message(&mut reader) {
                    Ok(response) => {
                        if let Some(method) = response.method {
                            // Server notification
                            let _ = tx.send(LspMessage::Notification {
                                method,
                                params: response.params,
                            });
                        } else if let Some(id) = response.id {
                            // Response to our request
                            let _ = tx.send(LspMessage::Response {
                                id,
                                result: response.result,
                                error: response.error,
                            });
                        }
                        if let Some(ref w) = waker {
                            w();
                        }
                    }
                    Err(_) => {
                        let _ = tx.send(LspMessage::ServerExited);
                        if let Some(ref w) = waker {
                            w();
                        }
                        break;
                    }
                }
            }
        });

        let mut client = LspClient {
            child,
            stdin,
            next_id: 1,
            rx,
            stop_flag,
            _reader_handle: reader_handle,
            server_capabilities: ServerCapabilities::default(),
            trigger_characters: Vec::new(),
            doc_versions: HashMap::new(),
            initialized: false,
            init_request_id: None,
            pending_after_init: Vec::new(),
        };

        // Send initialize request (non-blocking — response processed in poll_init)
        client.send_initialize(root_path);

        Some(client)
    }

    fn send_initialize(&mut self, root_path: &PathBuf) {
        let root_uri = format!("file://{}", root_path.display());
        let params = InitializeParams {
            process_id: Some(std::process::id()),
            root_uri: Some(root_uri),
            capabilities: ClientCapabilities {
                text_document: TextDocumentClientCapabilities {
                    completion: Some(CompletionClientCapabilities {
                        completion_item: Some(CompletionItemCapabilities {
                            snippet_support: false,
                        }),
                    }),
                    synchronization: Some(TextDocumentSyncClientCapabilities {
                        dynamic_registration: false,
                        did_save: true,
                    }),
                },
            },
        };

        let id = self.send_request("initialize", Some(serde_json::to_value(params).unwrap()));
        self.init_request_id = Some(id);
    }

    /// Process initialize response if pending. Returns true if just became initialized.
    pub fn poll_init(&mut self) -> bool {
        let init_id = match self.init_request_id {
            Some(id) => id,
            None => return false,
        };

        while let Ok(msg) = self.rx.try_recv() {
            match msg {
                LspMessage::Response { id, result, error } if id == init_id => {
                    self.init_request_id = None;
                    if error.is_some() {
                        return false;
                    }
                    if let Some(result) = result {
                        if let Ok(init_result) = serde_json::from_value::<InitializeResult>(result) {
                            self.trigger_characters = init_result
                                .capabilities
                                .completion_provider
                                .as_ref()
                                .and_then(|p| p.trigger_characters.clone())
                                .unwrap_or_default();
                            self.server_capabilities = init_result.capabilities;
                        }
                    }
                    // Send initialized notification
                    self.send_notification("initialized", Some(serde_json::json!({})));
                    self.initialized = true;

                    // Flush queued notifications
                    let pending = std::mem::take(&mut self.pending_after_init);
                    for (method, params) in pending {
                        self.send_notification(&method, params);
                    }

                    return true;
                }
                LspMessage::ServerExited => {
                    self.init_request_id = None;
                    return false;
                }
                _ => {
                    // Ignore other messages during init
                }
            }
        }
        false
    }

    /// Send a request to the server. Returns the request ID.
    pub fn send_request(&mut self, method: &str, params: Option<serde_json::Value>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        let request = JsonRpcRequest::new(id, method, params);
        if let Err(e) = transport::send_request(&mut self.stdin, &request) {
            log::warn!("LSP send_request error: {}", e);
        }
        id
    }

    /// Send a notification to the server (no response expected).
    pub fn send_notification(&mut self, method: &str, params: Option<serde_json::Value>) {
        let notif = JsonRpcNotification::new(method, params);
        if let Err(e) = transport::send_notification(&mut self.stdin, &notif) {
            log::warn!("LSP send_notification error: {}", e);
        }
    }

    // ── Document Sync ──

    pub fn did_open(&mut self, uri: &str, language_id: &str, text: &str) {
        self.doc_versions.insert(uri.to_string(), 1);
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": 1,
                "text": text,
            }
        });
        if self.initialized {
            self.send_notification("textDocument/didOpen", Some(params));
        } else {
            self.pending_after_init.push(("textDocument/didOpen".to_string(), Some(params)));
        }
    }

    pub fn did_change(&mut self, uri: &str, text: &str) {
        let version = self.doc_versions.entry(uri.to_string()).or_insert(0);
        *version += 1;
        let v = *version;
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "version": v,
            },
            "contentChanges": [{ "text": text }],
        });
        if self.initialized {
            self.send_notification("textDocument/didChange", Some(params));
        } else {
            self.pending_after_init.push(("textDocument/didChange".to_string(), Some(params)));
        }
    }

    pub fn did_save(&mut self, uri: &str) {
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
        });
        if self.initialized {
            self.send_notification("textDocument/didSave", Some(params));
        } else {
            self.pending_after_init.push(("textDocument/didSave".to_string(), Some(params)));
        }
    }

    pub fn did_close(&mut self, uri: &str) {
        self.doc_versions.remove(uri);
        let params = serde_json::json!({
            "textDocument": { "uri": uri },
        });
        if self.initialized {
            self.send_notification("textDocument/didClose", Some(params));
        } else {
            self.pending_after_init.push(("textDocument/didClose".to_string(), Some(params)));
        }
    }

    // ── Completion ──

    /// Request completion at the given position. Returns the request ID (0 if not ready).
    pub fn request_completion(
        &mut self,
        uri: &str,
        line: u32,
        character: u32,
        trigger_kind: u32,
        trigger_character: Option<&str>,
    ) -> u64 {
        if !self.initialized {
            return 0;
        }
        let params = CompletionParams {
            text_document: TextDocumentIdentifier { uri: uri.to_string() },
            position: LspPosition { line, character },
            context: Some(CompletionContext {
                trigger_kind,
                trigger_character: trigger_character.map(|s| s.to_string()),
            }),
        };
        self.send_request("textDocument/completion", Some(serde_json::to_value(params).unwrap()))
    }

    // ── Lifecycle ──

    pub fn shutdown(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        let _ = self.send_request("shutdown", None);
        // Give the server a moment to respond
        std::thread::sleep(std::time::Duration::from_millis(200));
        self.send_notification("exit", None);
        // Force kill if still alive
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        let _ = self.child.kill();
    }
}
