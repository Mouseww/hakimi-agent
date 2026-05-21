//! MCP client over stdio transport.
//!
//! Spawns an MCP server as a child process and communicates via JSON-RPC 2.0
//! over its stdin/stdout.

use std::process::Stdio;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::protocol::*;

/// MCP protocol version we declare during initialization.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Client name advertised to MCP servers.
const CLIENT_NAME: &str = "hakimi-agent";
const CLIENT_VERSION: &str = "0.1.0";

/// An MCP client that communicates with an MCP server over stdio.
pub struct McpClient {
    child: Child,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    next_id: Mutex<u64>,
    initialized: bool,
    server_info: Option<ServerInfo>,
}

impl McpClient {
    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    /// Spawn an MCP server as a child process and connect over stdio.
    pub async fn connect_stdio(command: &str, args: &[&str]) -> Result<Self> {
        info!(command, ?args, "spawning MCP server");

        let mut child = tokio::process::Command::new(command)
            .args(args)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to spawn MCP server: {command}"))?;

        let stdin = child.stdin.take().context("child has no stdin")?;
        let stdout = child.stdout.take().context("child has no stdout")?;

        Ok(Self {
            child,
            stdin: Mutex::new(stdin),
            stdout: Mutex::new(BufReader::new(stdout)),
            next_id: Mutex::new(1),
            initialized: false,
            server_info: None,
        })
    }

    // ------------------------------------------------------------------
    // High-level MCP methods
    // ------------------------------------------------------------------

    /// Perform the MCP `initialize` handshake.
    pub async fn initialize(&mut self) -> Result<()> {
        let params = InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: ClientCapabilities { roots: None },
            client_info: ClientInfo {
                name: CLIENT_NAME.to_string(),
                version: CLIENT_VERSION.to_string(),
            },
        };

        let resp = self.send_request("initialize", Some(json!(params))).await?;

        if let Some(err) = resp.error {
            anyhow::bail!(
                "initialize failed (code {}): {}",
                err.code,
                err.message
            );
        }

        let result: InitializeResult =
            serde_json::from_value(resp.result.context("initialize: missing result")?)?;

        info!(
            server = %result.server_info.name,
            version = %result.server_info.version,
            protocol = %result.protocol_version,
            "MCP server initialized"
        );

        self.server_info = Some(result.server_info);
        self.initialized = true;

        // Send the `notifications/initialized` notification (fire-and-forget).
        self.send_notification("notifications/initialized", None).await?;

        Ok(())
    }

    /// List all tools offered by the MCP server.
    pub async fn list_tools(&mut self) -> Result<Vec<McpToolDefinition>> {
        self.ensure_initialized()?;

        let mut all_tools = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let mut params = json!({});
            if let Some(ref c) = cursor {
                params = json!({ "cursor": c });
            }

            let resp = self.send_request("tools/list", Some(params)).await?;

            if let Some(err) = resp.error {
                anyhow::bail!("tools/list failed (code {}): {}", err.code, err.message);
            }

            let result: ListToolsResult =
                serde_json::from_value(resp.result.context("tools/list: missing result")?)?;

            all_tools.extend(result.tools);

            cursor = result.next_cursor;
            if cursor.is_none() {
                break;
            }
        }

        debug!(count = all_tools.len(), "listed MCP tools");
        Ok(all_tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(
        &mut self,
        name: &str,
        arguments: Option<Value>,
    ) -> Result<CallToolResult> {
        self.ensure_initialized()?;

        let params = CallToolParams {
            name: name.to_string(),
            arguments,
        };

        let resp = self.send_request("tools/call", Some(json!(params))).await?;

        if let Some(err) = resp.error {
            anyhow::bail!(
                "tools/call '{name}' failed (code {}): {}",
                err.code,
                err.message
            );
        }

        let result: CallToolResult =
            serde_json::from_value(resp.result.context("tools/call: missing result")?)?;

        debug!(tool = name, is_error = result.is_error, "tool call completed");
        Ok(result)
    }

    /// Returns true if the client has been initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Returns the server info obtained during initialization, if any.
    pub fn server_info(&self) -> Option<&ServerInfo> {
        self.server_info.as_ref()
    }

    /// Shut down the MCP server child process gracefully.
    pub async fn shutdown(&mut self) -> Result<()> {
        info!("shutting down MCP server");

        // Try to send a shutdown notification; ignore errors if the process is already gone.
        let _ = self.send_request("shutdown", None).await;

        // Give the process a moment to exit, then kill it.
        tokio::select! {
            status = self.child.wait() => {
                match status {
                    Ok(s) => info!(?s, "MCP server exited"),
                    Err(e) => warn!(?e, "error waiting for MCP server"),
                }
            }
            _ = tokio::time::sleep(std::time::Duration::from_secs(5)) => {
                warn!("MCP server did not exit in time, killing");
                let _ = self.child.kill().await;
            }
        }

        Ok(())
    }

    // ------------------------------------------------------------------
    // JSON-RPC transport internals
    // ------------------------------------------------------------------

    async fn next_id(&self) -> u64 {
        let mut id = self.next_id.lock().await;
        let current = *id;
        *id += 1;
        current
    }

    /// Send a JSON-RPC request and wait for the response.
    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<JsonRpcResponse> {
        let id = self.next_id().await;
        let request = JsonRpcRequest::new(id, method, params);

        // Write the request as a single JSON line.
        let mut payload = serde_json::to_string(&request)?;
        payload.push('\n');

        {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(payload.as_bytes()).await.context("writing to MCP stdin")?;
            stdin.flush().await.context("flushing MCP stdin")?;
        }

        debug!(id, method, "sent request");

        // Read the response.
        self.read_response(id).await
    }

    /// Send a JSON-RPC notification (no id, no response expected).
    async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<()> {
        let notification = JsonRpcNotification::new(method, params);
        let mut payload = serde_json::to_string(&notification)?;
        payload.push('\n');

        let mut stdin = self.stdin.lock().await;
        stdin.write_all(payload.as_bytes()).await.context("writing notification")?;
        stdin.flush().await.context("flushing notification")?;

        debug!(method, "sent notification");
        Ok(())
    }

    /// Read one JSON-RPC response line, skipping any notifications or out-of-band messages.
    async fn read_response(&self, expected_id: u64) -> Result<JsonRpcResponse> {
        let mut stdout = self.stdout.lock().await;
        let mut line = String::new();

        loop {
            line.clear();
            let bytes_read = stdout
                .read_line(&mut line)
                .await
                .context("reading from MCP stdout")?;

            if bytes_read == 0 {
                anyhow::bail!("MCP server stdout closed unexpectedly");
            }

            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Try to parse as a response first.
            match serde_json::from_str::<JsonRpcResponse>(trimmed) {
                Ok(resp) if resp.id == expected_id => {
                    debug!(id = expected_id, "received response");
                    return Ok(resp);
                }
                Ok(resp) => {
                    // Response for a different id — shouldn't happen in our serial model.
                    warn!(got = resp.id, expected = expected_id, "unexpected response id, skipping");
                    continue;
                }
                Err(_) => {
                    // Might be a notification or log message from the server.
                    if let Ok(notif) = serde_json::from_str::<JsonRpcNotification>(trimmed) {
                        debug!(method = %notif.method, "received server notification");
                    } else {
                        // Could be a stderr-like log line.
                        warn!(line = trimmed, "unrecognized line from MCP server");
                    }
                    continue;
                }
            }
        }
    }

    fn ensure_initialized(&self) -> Result<()> {
        if !self.initialized {
            anyhow::bail!("MCP client not initialized; call initialize() first");
        }
        Ok(())
    }
}

impl Drop for McpClient {
    fn drop(&mut self) {
        if !self.initialized {
            return;
        }
        // Best-effort kill if the child is still running.
        // We can't do async here, so just try to start kill.
        let _ = self.child.start_kill();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_id_generation() {
        // Verify the JSON-RPC request structure is correct.
        let req = JsonRpcRequest::new(42, "tools/list", None);
        assert_eq!(req.id, 42);
        assert_eq!(req.jsonrpc, "2.0");
        assert_eq!(req.method, "tools/list");
        assert!(req.params.is_none());
    }

    #[test]
    fn test_initialize_params_serialization() {
        let params = InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ClientCapabilities { roots: None },
            client_info: ClientInfo {
                name: "test".to_string(),
                version: "0.1.0".to_string(),
            },
        };
        let v = json!(params);
        assert_eq!(v["protocolVersion"], "2024-11-05");
        assert_eq!(v["clientInfo"]["name"], "test");
    }

    #[test]
    fn test_call_tool_params_serialization() {
        let params = CallToolParams {
            name: "read_file".to_string(),
            arguments: Some(json!({"path": "/tmp/test.txt"})),
        };
        let v = json!(params);
        assert_eq!(v["name"], "read_file");
        assert_eq!(v["arguments"]["path"], "/tmp/test.txt");
    }
}
