// LSP protocol types (minimal subset for completion).
// Uses manual types instead of lsp-types crate to avoid heavy dependency.

use serde::{Deserialize, Serialize};

// ── JSON-RPC ──

#[derive(Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: &str, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            method: method.to_string(),
            params,
        }
    }
}

#[derive(Serialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: &'static str,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<serde_json::Value>,
}

impl JsonRpcNotification {
    pub fn new(method: &str, params: Option<serde_json::Value>) -> Self {
        Self {
            jsonrpc: "2.0",
            method: method.to_string(),
            params,
        }
    }
}

#[derive(Deserialize, Debug)]
pub struct JsonRpcResponse {
    pub id: Option<u64>,
    pub result: Option<serde_json::Value>,
    pub error: Option<JsonRpcError>,
    pub method: Option<String>,
    #[allow(dead_code)]
    pub params: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
}

// ── LSP Initialization ──

#[derive(Serialize)]
pub struct InitializeParams {
    #[serde(rename = "processId")]
    pub process_id: Option<u32>,
    #[serde(rename = "rootUri")]
    pub root_uri: Option<String>,
    pub capabilities: ClientCapabilities,
}

#[derive(Serialize)]
pub struct ClientCapabilities {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentClientCapabilities,
}

#[derive(Serialize)]
pub struct TextDocumentClientCapabilities {
    pub completion: Option<CompletionClientCapabilities>,
    pub synchronization: Option<TextDocumentSyncClientCapabilities>,
}

#[derive(Serialize)]
pub struct CompletionClientCapabilities {
    #[serde(rename = "completionItem")]
    pub completion_item: Option<CompletionItemCapabilities>,
}

#[derive(Serialize)]
pub struct CompletionItemCapabilities {
    #[serde(rename = "snippetSupport")]
    pub snippet_support: bool,
}

#[derive(Serialize)]
pub struct TextDocumentSyncClientCapabilities {
    #[serde(rename = "dynamicRegistration")]
    pub dynamic_registration: bool,
    #[serde(rename = "didSave")]
    pub did_save: bool,
}

// ── Server Capabilities (from initialize response) ──

#[derive(Deserialize, Debug, Default)]
pub struct ServerCapabilities {
    #[serde(rename = "completionProvider")]
    pub completion_provider: Option<CompletionOptions>,
    #[serde(rename = "textDocumentSync")]
    pub text_document_sync: Option<serde_json::Value>,
}

#[derive(Deserialize, Debug)]
pub struct CompletionOptions {
    #[serde(rename = "triggerCharacters")]
    pub trigger_characters: Option<Vec<String>>,
}

#[derive(Deserialize, Debug)]
pub struct InitializeResult {
    pub capabilities: ServerCapabilities,
}

// ── Text Document ──

#[derive(Serialize)]
pub struct TextDocumentItem {
    pub uri: String,
    #[serde(rename = "languageId")]
    pub language_id: String,
    pub version: i64,
    pub text: String,
}

#[derive(Serialize)]
pub struct VersionedTextDocumentIdentifier {
    pub uri: String,
    pub version: i64,
}

#[derive(Serialize)]
pub struct TextDocumentIdentifier {
    pub uri: String,
}

#[derive(Serialize)]
pub struct TextDocumentContentChangeEvent {
    pub text: String,
}

// ── Completion ──

#[derive(Serialize)]
pub struct CompletionParams {
    #[serde(rename = "textDocument")]
    pub text_document: TextDocumentIdentifier,
    pub position: LspPosition,
    pub context: Option<CompletionContext>,
}

#[derive(Serialize)]
pub struct CompletionContext {
    #[serde(rename = "triggerKind")]
    pub trigger_kind: u32,
    #[serde(rename = "triggerCharacter")]
    pub trigger_character: Option<String>,
}

// triggerKind constants
pub const COMPLETION_TRIGGER_INVOKED: u32 = 1;
pub const COMPLETION_TRIGGER_CHARACTER: u32 = 2;

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct LspPosition {
    pub line: u32,
    pub character: u32,
}

#[derive(Deserialize, Debug)]
pub struct CompletionList {
    #[serde(rename = "isIncomplete")]
    pub is_incomplete: bool,
    pub items: Vec<LspCompletionItem>,
}

#[derive(Deserialize, Debug)]
pub struct LspCompletionItem {
    pub label: String,
    pub kind: Option<u32>,
    #[serde(rename = "insertText")]
    pub insert_text: Option<String>,
    #[serde(rename = "sortText")]
    pub sort_text: Option<String>,
    #[serde(rename = "filterText")]
    pub filter_text: Option<String>,
    pub detail: Option<String>,
    #[serde(rename = "textEdit")]
    pub text_edit: Option<serde_json::Value>,
}

// CompletionItemKind constants (from LSP spec)
pub const COMPLETION_KIND_TEXT: u32 = 1;
pub const COMPLETION_KIND_METHOD: u32 = 2;
pub const COMPLETION_KIND_FUNCTION: u32 = 3;
pub const COMPLETION_KIND_CONSTRUCTOR: u32 = 4;
pub const COMPLETION_KIND_FIELD: u32 = 5;
pub const COMPLETION_KIND_VARIABLE: u32 = 6;
pub const COMPLETION_KIND_CLASS: u32 = 7;
pub const COMPLETION_KIND_INTERFACE: u32 = 8;
pub const COMPLETION_KIND_MODULE: u32 = 9;
pub const COMPLETION_KIND_PROPERTY: u32 = 10;
pub const COMPLETION_KIND_KEYWORD: u32 = 14;
pub const COMPLETION_KIND_SNIPPET: u32 = 15;
pub const COMPLETION_KIND_CONSTANT: u32 = 21;

/// Map LSP CompletionItemKind number to a display abbreviation.
pub fn completion_kind_abbr(kind: Option<u32>) -> &'static str {
    match kind {
        Some(COMPLETION_KIND_METHOD) => "mth",
        Some(COMPLETION_KIND_FUNCTION) | Some(COMPLETION_KIND_CONSTRUCTOR) => "fn",
        Some(COMPLETION_KIND_FIELD) => "fld",
        Some(COMPLETION_KIND_VARIABLE) => "var",
        Some(COMPLETION_KIND_CLASS) | Some(COMPLETION_KIND_INTERFACE) => "typ",
        Some(COMPLETION_KIND_MODULE) => "mod",
        Some(COMPLETION_KIND_PROPERTY) => "prop",
        Some(COMPLETION_KIND_KEYWORD) => "kw",
        Some(COMPLETION_KIND_SNIPPET) => "snip",
        Some(COMPLETION_KIND_CONSTANT) => "con",
        _ => "",
    }
}

/// Map LSP CompletionItemKind number to our CompletionKind enum value.
pub fn lsp_kind_to_u8(kind: Option<u32>) -> u8 {
    match kind {
        Some(COMPLETION_KIND_FUNCTION) | Some(COMPLETION_KIND_CONSTRUCTOR) => 0,
        Some(COMPLETION_KIND_VARIABLE) => 1,
        Some(COMPLETION_KIND_FIELD) => 2,
        Some(COMPLETION_KIND_CLASS) | Some(COMPLETION_KIND_INTERFACE) => 3,
        Some(COMPLETION_KIND_MODULE) => 4,
        Some(COMPLETION_KIND_KEYWORD) => 5,
        Some(COMPLETION_KIND_SNIPPET) => 6,
        Some(COMPLETION_KIND_PROPERTY) => 7,
        Some(COMPLETION_KIND_METHOD) => 8,
        Some(COMPLETION_KIND_CONSTANT) => 9,
        _ => 10,
    }
}
