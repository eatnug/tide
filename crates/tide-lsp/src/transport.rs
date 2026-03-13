// LSP stdio transport: reads/writes JSON-RPC messages over stdin/stdout of a child process.

use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, ChildStdout};

use crate::protocol::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse};

/// Write a JSON-RPC message to the LSP server's stdin.
pub fn send_request(stdin: &mut ChildStdin, request: &JsonRpcRequest) -> std::io::Result<()> {
    let body = serde_json::to_string(request)?;
    write_message(stdin, &body)
}

/// Write a JSON-RPC notification to the LSP server's stdin.
pub fn send_notification(stdin: &mut ChildStdin, notif: &JsonRpcNotification) -> std::io::Result<()> {
    let body = serde_json::to_string(notif)?;
    write_message(stdin, &body)
}

fn write_message(stdin: &mut ChildStdin, body: &str) -> std::io::Result<()> {
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    stdin.write_all(header.as_bytes())?;
    stdin.write_all(body.as_bytes())?;
    stdin.flush()
}

/// Read one JSON-RPC message from the LSP server's stdout.
/// Blocks until a complete message is available.
pub fn read_message(reader: &mut BufReader<ChildStdout>) -> std::io::Result<JsonRpcResponse> {
    // Read headers
    let mut content_length: usize = 0;
    loop {
        let mut header_line = String::new();
        reader.read_line(&mut header_line)?;
        let trimmed = header_line.trim();
        if trimmed.is_empty() {
            break;
        }
        if let Some(len_str) = trimmed.strip_prefix("Content-Length: ") {
            content_length = len_str.trim().parse().map_err(|e| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, format!("bad Content-Length: {}", e))
            })?;
        }
    }

    if content_length == 0 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "missing Content-Length header",
        ));
    }

    // Read body
    let mut body = vec![0u8; content_length];
    reader.read_exact(&mut body)?;
    let body_str = String::from_utf8(body).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid UTF-8: {}", e))
    })?;

    serde_json::from_str(&body_str).map_err(|e| {
        std::io::Error::new(std::io::ErrorKind::InvalidData, format!("invalid JSON: {}", e))
    })
}

/// Check if the language server process is still running.
pub fn is_alive(child: &mut Child) -> bool {
    matches!(child.try_wait(), Ok(None))
}
