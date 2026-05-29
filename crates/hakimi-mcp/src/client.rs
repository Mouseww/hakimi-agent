//! MCP client over stdio transport.
//!
//! Spawns an MCP server as a child process and communicates via JSON-RPC 2.0
//! over its stdin/stdout.

use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;

use anyhow::{Context, Result};
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout};
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::protocol::*;
use crate::sampling::{McpServerRequestHandler, handle_server_request};

/// MCP protocol version we declare during initialization.
const MCP_PROTOCOL_VERSION: &str = "2024-11-05";

/// Client name advertised to MCP servers.
const CLIENT_NAME: &str = "hakimi-agent";
const CLIENT_VERSION: &str = "0.2.1";

#[derive(Debug, Clone)]
struct ResolvedStdioCommand {
    executable: PathBuf,
    path_override: Option<OsString>,
}

/// An MCP client that communicates with an MCP server over stdio.
pub struct McpClient {
    child: Child,
    stdin: Mutex<ChildStdin>,
    stdout: Mutex<BufReader<ChildStdout>>,
    next_id: Mutex<u64>,
    initialized: bool,
    server_info: Option<ServerInfo>,
    server_request_handler: Option<Arc<dyn McpServerRequestHandler>>,
}

impl McpClient {
    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    /// Spawn an MCP server as a child process and connect over stdio.
    pub async fn connect_stdio(command: &str, args: &[&str]) -> Result<Self> {
        let resolved = resolve_stdio_command(command);
        info!(
            command,
            executable = %resolved.executable.display(),
            ?args,
            "spawning MCP server"
        );

        let mut process = tokio::process::Command::new(&resolved.executable);
        process.args(args);
        if let Some(path) = resolved.path_override {
            process.env("PATH", path);
        }

        let mut child = process
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
            server_request_handler: None,
        })
    }

    /// Attach a handler for server-initiated MCP requests such as
    /// `sampling/createMessage`.
    pub fn with_server_request_handler(
        mut self,
        handler: Arc<dyn McpServerRequestHandler>,
    ) -> Self {
        self.server_request_handler = Some(handler);
        self
    }

    // ------------------------------------------------------------------
    // High-level MCP methods
    // ------------------------------------------------------------------

    /// Perform the MCP `initialize` handshake.
    pub async fn initialize(&mut self) -> Result<()> {
        let params = InitializeParams {
            protocol_version: MCP_PROTOCOL_VERSION.to_string(),
            capabilities: if self.server_request_handler.is_some() {
                ClientCapabilities::with_sampling()
            } else {
                ClientCapabilities::basic()
            },
            client_info: ClientInfo {
                name: CLIENT_NAME.to_string(),
                version: CLIENT_VERSION.to_string(),
            },
        };

        let resp = self.send_request("initialize", Some(json!(params))).await?;

        if let Some(err) = resp.error {
            anyhow::bail!("initialize failed (code {}): {}", err.code, err.message);
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
        self.send_notification("notifications/initialized", None)
            .await?;

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

        debug!(
            tool = name,
            is_error = result.is_error,
            "tool call completed"
        );
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
            stdin
                .write_all(payload.as_bytes())
                .await
                .context("writing to MCP stdin")?;
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
        stdin
            .write_all(payload.as_bytes())
            .await
            .context("writing notification")?;
        stdin.flush().await.context("flushing notification")?;

        debug!(method, "sent notification");
        Ok(())
    }

    async fn send_server_response(&self, response: JsonRpcServerResponse) -> Result<()> {
        let mut payload = serde_json::to_string(&response)?;
        payload.push('\n');

        let mut stdin = self.stdin.lock().await;
        stdin
            .write_all(payload.as_bytes())
            .await
            .context("writing server-request response")?;
        stdin
            .flush()
            .await
            .context("flushing server-request response")?;

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

            if let Ok(request) = serde_json::from_str::<JsonRpcServerRequest>(trimmed) {
                debug!(
                    method = %request.method,
                    id = %request.id,
                    "received MCP server request"
                );
                let response =
                    handle_server_request(self.server_request_handler.as_ref(), request).await;
                if let Err(error) = self.send_server_response(response).await {
                    warn!(error = %error, "failed to answer MCP server request");
                }
                continue;
            }

            // Try to parse as a response after ruling out server requests.
            match serde_json::from_str::<JsonRpcResponse>(trimmed) {
                Ok(resp) if resp.id == expected_id => {
                    debug!(id = expected_id, "received response");
                    return Ok(resp);
                }
                Ok(resp) => {
                    // Response for a different id — shouldn't happen in our serial model.
                    warn!(
                        got = resp.id,
                        expected = expected_id,
                        "unexpected response id, skipping"
                    );
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

fn resolve_stdio_command(command: &str) -> ResolvedStdioCommand {
    let path_env = std::env::var_os("PATH");
    let home_dir = dirs::home_dir();
    let hakimi_home = std::env::var_os("HAKIMI_HOME").map(PathBuf::from);
    resolve_stdio_command_with_env(
        command,
        path_env.as_deref(),
        home_dir.as_deref(),
        hakimi_home.as_deref(),
    )
}

fn resolve_stdio_command_with_env(
    command: &str,
    path_env: Option<&OsStr>,
    home_dir: Option<&Path>,
    hakimi_home: Option<&Path>,
) -> ResolvedStdioCommand {
    if !is_bare_command(command) {
        return ResolvedStdioCommand {
            executable: PathBuf::from(command),
            path_override: None,
        };
    }

    if let Some(executable) = find_command_in_path(command, path_env) {
        return ResolvedStdioCommand {
            executable,
            path_override: None,
        };
    }

    if is_node_stdio_command(command)
        && let Some(executable) = node_fallback_candidates(command, home_dir, hakimi_home)
            .into_iter()
            .find(|candidate| is_executable_file(candidate))
    {
        let command_dir = executable.parent().map(Path::to_path_buf);
        return ResolvedStdioCommand {
            path_override: command_dir
                .as_deref()
                .and_then(|dir| prepend_path_dir(path_env, dir)),
            executable,
        };
    }

    ResolvedStdioCommand {
        executable: PathBuf::from(command),
        path_override: None,
    }
}

fn is_bare_command(command: &str) -> bool {
    !command.is_empty() && !command.contains('/') && !command.contains('\\')
}

fn is_node_stdio_command(command: &str) -> bool {
    let lower = command.to_ascii_lowercase();
    let stem = lower
        .strip_suffix(".exe")
        .or_else(|| lower.strip_suffix(".cmd"))
        .or_else(|| lower.strip_suffix(".bat"))
        .unwrap_or(&lower);
    matches!(stem, "node" | "npm" | "npx")
}

fn node_fallback_candidates(
    command: &str,
    home_dir: Option<&Path>,
    hakimi_home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    let default_hakimi_home = home_dir.map(|home| home.join(".hakimi"));
    if let Some(home) = hakimi_home.or(default_hakimi_home.as_deref()) {
        candidates.push(home.join("node").join("bin").join(command));
    }
    if let Some(home) = home_dir {
        candidates.push(home.join(".local").join("bin").join(command));
    }
    #[cfg(unix)]
    candidates.push(Path::new("/usr/local/bin").join(command));
    candidates
}

fn find_command_in_path(command: &str, path_env: Option<&OsStr>) -> Option<PathBuf> {
    let path_env = path_env?;
    for dir in std::env::split_paths(path_env) {
        for candidate_name in command_names(command) {
            let candidate = dir.join(candidate_name);
            if is_executable_file(&candidate) {
                return Some(candidate);
            }
        }
    }
    None
}

fn command_names(command: &str) -> Vec<OsString> {
    #[cfg(windows)]
    {
        if Path::new(command).extension().is_some() {
            return vec![OsString::from(command)];
        }
        let mut names = vec![OsString::from(command)];
        for ext in ["exe", "cmd", "bat"] {
            names.push(OsString::from(format!("{command}.{ext}")));
        }
        names
    }
    #[cfg(not(windows))]
    {
        vec![OsString::from(command)]
    }
}

fn prepend_path_dir(path_env: Option<&OsStr>, dir: &Path) -> Option<OsString> {
    let mut entries = vec![dir.to_path_buf()];
    if let Some(path_env) = path_env {
        entries.extend(std::env::split_paths(path_env).filter(|entry| entry != dir));
    }
    std::env::join_paths(entries).ok()
}

#[cfg(unix)]
fn is_executable_file(path: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    path.is_file()
        && path
            .metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_executable_file(path: &Path) -> bool {
    path.is_file()
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
    use std::time::{SystemTime, UNIX_EPOCH};

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
            capabilities: ClientCapabilities::basic(),
            client_info: ClientInfo {
                name: "test".to_string(),
                version: "0.2.1".to_string(),
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

    #[test]
    fn test_resolve_stdio_command_keeps_explicit_path() {
        let resolved = resolve_stdio_command_with_env(
            "/opt/mcp/server",
            Some(OsStr::new("/usr/bin")),
            None,
            None,
        );
        assert_eq!(resolved.executable, PathBuf::from("/opt/mcp/server"));
        assert!(resolved.path_override.is_none());
    }

    #[test]
    fn test_resolve_stdio_command_uses_existing_path_entry() {
        let dir = unique_test_dir("mcp-path-entry");
        std::fs::create_dir_all(&dir).expect("create temp dir");
        let executable = write_executable(&dir, "npx");
        let path = std::env::join_paths([dir.clone()]).expect("join PATH");

        let resolved = resolve_stdio_command_with_env("npx", Some(path.as_os_str()), None, None);

        assert_eq!(resolved.executable, executable);
        assert!(resolved.path_override.is_none());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_resolve_stdio_command_falls_back_to_hakimi_node_bin() {
        let home = unique_test_dir("mcp-hakimi-home");
        let node_bin = home.join("node").join("bin");
        let empty_path_dir = home.join("empty-path");
        std::fs::create_dir_all(&node_bin).expect("create node bin");
        std::fs::create_dir_all(&empty_path_dir).expect("create empty PATH dir");
        let executable = write_executable(&node_bin, "npx");
        let narrow_path = std::env::join_paths([empty_path_dir]).expect("join PATH");

        let resolved = resolve_stdio_command_with_env(
            "npx",
            Some(narrow_path.as_os_str()),
            None,
            Some(home.as_path()),
        );

        assert_eq!(resolved.executable, executable);
        let override_path = resolved.path_override.expect("PATH should be prepended");
        let entries: Vec<_> = std::env::split_paths(&override_path).collect();
        assert_eq!(entries.first(), Some(&node_bin));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn test_resolve_stdio_command_falls_back_to_user_local_bin() {
        let home = unique_test_dir("mcp-local-bin");
        let local_bin = home.join(".local").join("bin");
        let empty_path_dir = home.join("empty-path");
        std::fs::create_dir_all(&local_bin).expect("create local bin");
        std::fs::create_dir_all(&empty_path_dir).expect("create empty PATH dir");
        let executable = write_executable(&local_bin, "node");
        let narrow_path = std::env::join_paths([empty_path_dir]).expect("join PATH");

        let resolved = resolve_stdio_command_with_env(
            "node",
            Some(narrow_path.as_os_str()),
            Some(&home),
            None,
        );

        assert_eq!(resolved.executable, executable);
        let override_path = resolved.path_override.expect("PATH should be prepended");
        let entries: Vec<_> = std::env::split_paths(&override_path).collect();
        assert_eq!(entries.first(), Some(&local_bin));
        let _ = std::fs::remove_dir_all(&home);
    }

    #[test]
    fn test_resolve_stdio_command_does_not_fallback_for_non_node_command() {
        let home = unique_test_dir("mcp-non-node");
        let local_bin = home.join(".local").join("bin");
        let empty_path_dir = home.join("empty-path");
        std::fs::create_dir_all(&local_bin).expect("create local bin");
        std::fs::create_dir_all(&empty_path_dir).expect("create empty PATH dir");
        let _ = write_executable(&local_bin, "custom-hakimi-mcp");
        let narrow_path = std::env::join_paths([empty_path_dir]).expect("join PATH");

        let resolved = resolve_stdio_command_with_env(
            "custom-hakimi-mcp",
            Some(narrow_path.as_os_str()),
            Some(&home),
            None,
        );

        assert_eq!(resolved.executable, PathBuf::from("custom-hakimi-mcp"));
        assert!(resolved.path_override.is_none());
        let _ = std::fs::remove_dir_all(&home);
    }

    fn unique_test_dir(label: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("time should be monotonic enough for tests")
            .as_nanos();
        std::env::temp_dir().join(format!("hakimi-{label}-{nanos}"))
    }

    fn write_executable(dir: &Path, name: &str) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\nexit 0\n").expect("write executable");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mut permissions = std::fs::metadata(&path).expect("metadata").permissions();
            permissions.set_mode(0o755);
            std::fs::set_permissions(&path, permissions).expect("chmod");
        }
        path
    }
}
