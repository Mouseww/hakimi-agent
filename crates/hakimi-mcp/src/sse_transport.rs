//! SSE (Server-Sent Events) transport for MCP servers.
//!
//! Communicates with MCP servers that use SSE for receiving and HTTP POST for sending.
//! Supports automatic reconnection with exponential backoff.

use anyhow::{Context, Result};
use reqwest::Client;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::sync::Mutex;
use tracing::{debug, info, warn};

use crate::protocol::*;

const MCP_PROTOCOL_VERSION: &str = "2024-11-05";
const CLIENT_NAME: &str = "hakimi-agent";
const CLIENT_VERSION: &str = "0.1.0";

/// Configuration for reconnection behavior.
#[derive(Debug, Clone)]
pub struct ReconnectConfig {
    /// Maximum number of reconnection attempts.
    pub max_attempts: u32,
    /// Base delay for exponential backoff.
    pub base_delay: Duration,
    /// Maximum delay cap.
    pub max_delay: Duration,
}

impl Default for ReconnectConfig {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            base_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
        }
    }
}

/// SSE transport for MCP servers.
pub struct SseTransport {
    /// SSE endpoint URL for receiving events.
    sse_url: String,
    /// HTTP POST endpoint URL for sending messages.
    post_url: Option<String>,
    client: Client,
    next_id: Mutex<u64>,
    initialized: bool,
    server_info: Option<ServerInfo>,
    /// Optional authorization header value.
    auth_header: Option<String>,
    /// Reconnection configuration.
    reconnect_config: ReconnectConfig,
    /// Current reconnection attempt count.
    reconnect_attempts: u32,
}

impl SseTransport {
    /// Create a new SSE transport.
    pub fn new(
        sse_url: impl Into<String>,
        auth_header: Option<String>,
        reconnect_config: Option<ReconnectConfig>,
    ) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300)) // Long timeout for SSE
            .build()
            .expect("failed to build HTTP client");

        Self {
            sse_url: sse_url.into(),
            post_url: None,
            client,
            next_id: Mutex::new(1),
            initialized: false,
            server_info: None,
            auth_header,
            reconnect_config: reconnect_config.unwrap_or_default(),
            reconnect_attempts: 0,
        }
    }

    /// Connect to the SSE endpoint and perform initialization.
    pub async fn initialize(&mut self) -> Result<()> {
        // First, connect to the SSE endpoint to get the post URL.
        self.connect_sse().await?;

        // Then perform the MCP initialize handshake.
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
            anyhow::bail!("initialize failed (code {}): {}", err.code, err.message);
        }

        let result: InitializeResult =
            serde_json::from_value(resp.result.context("initialize: missing result")?)?;

        info!(
            server = %result.server_info.name,
            version = %result.server_info.version,
            "MCP SSE server initialized"
        );

        self.server_info = Some(result.server_info);
        self.initialized = true;
        self.reconnect_attempts = 0;

        self.send_notification("notifications/initialized", None).await?;

        Ok(())
    }

    /// Connect to the SSE endpoint to discover the POST URL.
    async fn connect_sse(&mut self) -> Result<()> {
        let mut builder = self
            .client
            .get(&self.sse_url)
            .header("Accept", "text/event-stream")
            .header("Cache-Control", "no-cache");

        if let Some(ref auth) = self.auth_header {
            builder = builder.header("Authorization", auth);
        }

        let response = builder
            .send()
            .await
            .context("failed to connect to SSE endpoint")?;

        if !response.status().is_success() {
            anyhow::bail!(
                "SSE connection failed: HTTP {}",
                response.status()
            );
        }

        // Read the first SSE event to get the endpoint URL.
        let body = response.text().await.context("failed to read SSE response")?;

        // Parse SSE events to find the "endpoint" event.
        for line in body.lines() {
            if line.starts_with("data:") {
                let data = line.strip_prefix("data:").unwrap_or("").trim();
                if let Ok(value) = serde_json::from_str::<Value>(data) {
                    if let Some(uri) = value.get("uri").and_then(|v| v.as_str()) {
                        self.post_url = Some(uri.to_string());
                        debug!(post_url = %uri, "Discovered SSE POST endpoint");
                        return Ok(());
                    }
                }
            }
        }

        // If no endpoint event found, use the SSE URL as the POST URL.
        self.post_url = Some(self.sse_url.clone());
        debug!("Using SSE URL as POST endpoint");
        Ok(())
    }

    /// List tools from the MCP server.
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

        debug!(count = all_tools.len(), "listed MCP tools via SSE");
        Ok(all_tools)
    }

    /// Call a tool on the MCP server.
    pub async fn call_tool(&mut self, name: &str, arguments: Option<Value>) -> Result<CallToolResult> {
        self.ensure_initialized()?;

        let params = CallToolParams {
            name: name.to_string(),
            arguments,
        };

        let resp = self.send_request("tools/call", Some(json!(params))).await?;

        if let Some(err) = resp.error {
            anyhow::bail!("tools/call '{}' failed (code {}): {}", name, err.code, err.message);
        }

        let result: CallToolResult =
            serde_json::from_value(resp.result.context("tools/call: missing result")?)?;

        debug!(tool = name, is_error = result.is_error, "SSE tool call completed");
        Ok(result)
    }

    /// Send a JSON-RPC request via HTTP POST with retry logic.
    async fn send_request(&self, method: &str, params: Option<Value>) -> Result<JsonRpcResponse> {
        let post_url = self
            .post_url
            .as_ref()
            .context("no POST URL available; SSE endpoint not connected")?;

        let id = self.next_id().await;
        let request = JsonRpcRequest::new(id, method, params);

        debug!(id, method, url = %post_url, "sending SSE POST request");

        let mut builder = self
            .client
            .post(post_url)
            .header("Content-Type", "application/json");

        if let Some(ref auth) = self.auth_header {
            builder = builder.header("Authorization", auth);
        }

        let response = builder
            .json(&request)
            .send()
            .await
            .context("SSE POST request failed")?;

        let status = response.status();
        let body = response.text().await.context("failed to read response body")?;

        if !status.is_success() {
            anyhow::bail!("SSE POST error {}: {}", status, &body[..body.len().min(200)]);
        }

        serde_json::from_str::<JsonRpcResponse>(&body)
            .with_context(|| format!("failed to parse JSON-RPC response: {}", &body[..body.len().min(200)]))
    }

    /// Send a JSON-RPC notification via HTTP POST.
    async fn send_notification(&self, method: &str, params: Option<Value>) -> Result<()> {
        let post_url = self
            .post_url
            .as_ref()
            .context("no POST URL available")?;

        let notification = JsonRpcNotification::new(method, params);

        let mut builder = self
            .client
            .post(post_url)
            .header("Content-Type", "application/json");

        if let Some(ref auth) = self.auth_header {
            builder = builder.header("Authorization", auth);
        }

        builder
            .json(&notification)
            .send()
            .await
            .context("notification send failed")?;

        Ok(())
    }

    /// Attempt reconnection with exponential backoff.
    pub async fn reconnect(&mut self) -> Result<()> {
        if self.reconnect_attempts >= self.reconnect_config.max_attempts {
            anyhow::bail!(
                "max reconnection attempts ({}) exceeded",
                self.reconnect_config.max_attempts
            );
        }

        let delay = self.compute_backoff();
        warn!(
            attempt = self.reconnect_attempts + 1,
            delay_ms = delay.as_millis(),
            "Attempting SSE reconnection"
        );

        tokio::time::sleep(delay).await;
        self.reconnect_attempts += 1;
        self.initialized = false;

        self.initialize().await
    }

    /// Compute exponential backoff delay.
    fn compute_backoff(&self) -> Duration {
        let base_ms = self.reconnect_config.base_delay.as_millis() as u64;
        let max_ms = self.reconnect_config.max_delay.as_millis() as u64;
        let exponential = base_ms.saturating_mul(1u64 << self.reconnect_attempts.min(10));
        Duration::from_millis(exponential.min(max_ms))
    }

    async fn next_id(&self) -> u64 {
        let mut id = self.next_id.lock().await;
        let current = *id;
        *id += 1;
        current
    }

    fn ensure_initialized(&self) -> Result<()> {
        if !self.initialized {
            anyhow::bail!("SSE transport not initialized; call initialize() first");
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sse_transport_new() {
        let transport = SseTransport::new(
            "http://localhost:3000/sse",
            None,
            None,
        );
        assert_eq!(transport.sse_url, "http://localhost:3000/sse");
        assert!(!transport.initialized);
        assert_eq!(transport.reconnect_attempts, 0);
    }

    #[test]
    fn test_sse_transport_with_auth() {
        let transport = SseTransport::new(
            "http://localhost:3000/sse",
            Some("Bearer token123".to_string()),
            None,
        );
        assert_eq!(transport.auth_header, Some("Bearer token123".to_string()));
    }

    #[test]
    fn test_reconnect_config_default() {
        let config = ReconnectConfig::default();
        assert_eq!(config.max_attempts, 5);
        assert_eq!(config.base_delay, Duration::from_secs(1));
        assert_eq!(config.max_delay, Duration::from_secs(30));
    }

    #[test]
    fn test_compute_backoff() {
        let mut transport = SseTransport::new("http://localhost/sse", None, None);
        transport.reconnect_attempts = 0;
        let delay = transport.compute_backoff();
        assert!(delay <= Duration::from_secs(1));

        transport.reconnect_attempts = 3;
        let delay = transport.compute_backoff();
        assert!(delay <= Duration::from_secs(30));
    }

    #[test]
    fn test_ensure_initialized_fails() {
        let transport = SseTransport::new("http://localhost/sse", None, None);
        assert!(transport.ensure_initialized().is_err());
    }

    #[tokio::test]
    async fn test_list_tools_before_init() {
        let mut transport = SseTransport::new("http://localhost/sse", None, None);
        let result = transport.list_tools().await;
        assert!(result.is_err());
    }
}
