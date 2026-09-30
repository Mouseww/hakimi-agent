//! MCP server mode: expose Hakimi's own tools over stdio JSON-RPC.
//!
//! Hakimi has always been an MCP *client* (`crate::client`), talking to other
//! people's servers. This module is the mirror image: it lets an external agent
//! host — Zed, OpenCode, Claude Code, Codex — treat Hakimi as one MCP server
//! among its others. The tool surface is Hakimi's real [`ToolRegistry`], so
//! whatever the agent can do locally, the host can ask it to do remotely.
//!
//! The wire types come from [`crate::protocol`], the same types the client
//! uses, so the two halves cannot drift apart.

use std::sync::Arc;

use hakimi_common::{ToolContext, ToolDefinition};
use hakimi_tools::ToolRegistry;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tracing::{debug, info, warn};

use crate::protocol::{
    CallToolParams, CallToolResult, ContentBlock, InitializeResult, JsonRpcError, ListToolsResult,
    McpToolDefinition, ServerCapabilities, ServerInfo,
};

/// Protocol revision this server speaks.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

/// An MCP server that exposes a Hakimi [`ToolRegistry`] over stdio.
pub struct McpServer {
    registry: ToolRegistry,
    ctx: Arc<ToolContext>,
    name: String,
    version: String,
}

impl McpServer {
    /// Wrap a registry and the context its tools should run under.
    pub fn new(registry: ToolRegistry, ctx: ToolContext) -> Self {
        Self {
            registry,
            ctx: Arc::new(ctx),
            name: "hakimi-agent".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        }
    }

    /// Override the name advertised in `initialize`.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Handle one JSON-RPC message.
    ///
    /// Returns `None` for notifications (no `id`), which must not be answered.
    pub async fn handle_message(&self, raw: &str) -> Option<Value> {
        let message: Value = match serde_json::from_str(raw) {
            Ok(value) => value,
            Err(err) => {
                warn!(error = %err, "discarding malformed JSON-RPC message");
                return Some(json!({
                    "jsonrpc": "2.0",
                    "id": Value::Null,
                    "error": JsonRpcError::invalid_params(format!("invalid JSON: {err}")),
                }));
            }
        };

        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string();
        let params = message.get("params").cloned().unwrap_or(Value::Null);
        let id = message.get("id").cloned();

        // Notifications carry no id and must never be answered.
        let Some(id) = id else {
            debug!(method = %method, "received MCP notification");
            return None;
        };

        match self.dispatch(&method, &params).await {
            Ok(result) => Some(json!({"jsonrpc": "2.0", "id": id, "result": result})),
            Err(error) => Some(json!({"jsonrpc": "2.0", "id": id, "error": error})),
        }
    }

    async fn dispatch(&self, method: &str, params: &Value) -> Result<Value, JsonRpcError> {
        match method {
            "initialize" => serde_json::to_value(self.initialize_result())
                .map_err(|err| JsonRpcError::internal(err.to_string())),
            "ping" => Ok(json!({})),
            "tools/list" => serde_json::to_value(self.list_tools().await)
                .map_err(|err| JsonRpcError::internal(err.to_string())),
            "tools/call" => self.call_tool(params).await,
            other => Err(JsonRpcError {
                code: -32601,
                message: format!("method '{other}' is not supported by the Hakimi MCP server"),
                data: None,
            }),
        }
    }

    fn initialize_result(&self) -> InitializeResult {
        InitializeResult {
            protocol_version: PROTOCOL_VERSION.to_string(),
            capabilities: ServerCapabilities {
                tools: Some(json!({ "listChanged": false })),
                resources: None,
                prompts: None,
            },
            server_info: ServerInfo {
                name: self.name.clone(),
                version: self.version.clone(),
            },
        }
    }

    async fn list_tools(&self) -> ListToolsResult {
        let definitions: Vec<ToolDefinition> = self.registry.get_definitions().await;
        let mut tools: Vec<McpToolDefinition> = definitions
            .into_iter()
            .map(|definition| McpToolDefinition {
                name: definition.name,
                description: Some(definition.description),
                input_schema: definition.parameters,
            })
            .collect();
        // Stable ordering keeps `tools/list` diffable between calls.
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        ListToolsResult {
            tools,
            next_cursor: None,
        }
    }

    async fn call_tool(&self, params: &Value) -> Result<Value, JsonRpcError> {
        let call: CallToolParams = serde_json::from_value(params.clone())
            .map_err(|err| JsonRpcError::invalid_params(format!("bad tools/call params: {err}")))?;
        let arguments = call.arguments.unwrap_or_else(|| json!({}));
        info!(tool = %call.name, "MCP server dispatching tool");

        let result = match self
            .registry
            .dispatch(&call.name, &arguments, self.ctx.as_ref())
            .await
        {
            Ok(text) => CallToolResult {
                content: vec![ContentBlock::Text { text }],
                is_error: false,
            },
            Err(err) => CallToolResult {
                content: vec![ContentBlock::Text {
                    text: format!("Error: {err}"),
                }],
                is_error: true,
            },
        };

        serde_json::to_value(result).map_err(|err| JsonRpcError::internal(err.to_string()))
    }

    /// Read newline-delimited JSON-RPC from stdin, write replies to stdout,
    /// until the host closes the pipe.
    pub async fn serve_stdio(self) -> anyhow::Result<()> {
        let mut lines = BufReader::new(tokio::io::stdin()).lines();
        let mut stdout = tokio::io::stdout();

        info!(server = %self.name, "Hakimi MCP server listening on stdio");

        while let Some(line) = lines.next_line().await? {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Some(response) = self.handle_message(line).await {
                let mut encoded = serde_json::to_vec(&response)?;
                encoded.push(b'\n');
                stdout.write_all(&encoded).await?;
                stdout.flush().await?;
            }
        }

        info!("Hakimi MCP server stdio closed; exiting");
        Ok(())
    }
}
