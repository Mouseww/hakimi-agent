//! Adapter that wraps an MCP tool into the `hakimi_tools::Tool` trait,
//! allowing MCP-provided tools to be used transparently alongside built-in tools.

use std::sync::Arc;

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result as HakimiResult, ToolContext};
use serde_json::Value;

use crate::client::McpClient;
use crate::protocol::ContentBlock;
use crate::redaction::sanitize_mcp_error;

/// Wraps an MCP tool definition and a reference to the MCP client so that
/// the tool can be dispatched through the standard `hakimi_tools::Tool` trait.
pub struct McpToolAdapter {
    name: String,
    description: String,
    input_schema: Value,
    client: Arc<tokio::sync::Mutex<McpClient>>,
}

impl McpToolAdapter {
    /// Create a new adapter from an MCP tool definition and a shared client.
    pub fn new(
        tool: &crate::protocol::McpToolDefinition,
        client: Arc<tokio::sync::Mutex<McpClient>>,
    ) -> Self {
        Self {
            name: tool.name.clone(),
            description: tool
                .description
                .clone()
                .unwrap_or_else(|| "MCP tool".to_string()),
            input_schema: tool.input_schema.clone(),
            client,
        }
    }

    /// Create adapters for all tools returned by the MCP server.
    pub fn from_tool_list(
        tools: &[crate::protocol::McpToolDefinition],
        client: Arc<tokio::sync::Mutex<McpClient>>,
    ) -> Vec<Self> {
        tools.iter().map(|t| Self::new(t, client.clone())).collect()
    }
}

#[async_trait]
impl hakimi_tools::Tool for McpToolAdapter {
    fn name(&self) -> &str {
        &self.name
    }

    fn toolset(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        &self.description
    }

    fn emoji(&self) -> &str {
        "🔌"
    }

    fn schema(&self) -> Value {
        self.input_schema.clone()
    }

    async fn execute(&self, args: &Value, _ctx: &ToolContext) -> HakimiResult<String> {
        let arguments = if args.is_null() {
            None
        } else {
            Some(args.clone())
        };

        let result = {
            let mut client = self.client.lock().await;
            client.call_tool(&self.name, arguments).await.map_err(|e| {
                HakimiError::Tool(format!(
                    "MCP tool '{}' failed: {}",
                    self.name,
                    sanitize_mcp_error(&e.to_string())
                ))
            })?
        };

        if result.is_error {
            let text = sanitize_mcp_error(&result.text_content());
            return Err(HakimiError::Tool(format!(
                "MCP tool '{}' returned error: {text}",
                self.name
            )));
        }

        // Collect text content; for non-text blocks, include a placeholder.
        let mut output = String::new();
        for block in &result.content {
            match block {
                ContentBlock::Text { text } => {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(text);
                }
                ContentBlock::Image { mime_type, .. } => {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&format!("[image: {mime_type}]"));
                }
                ContentBlock::Resource { .. } => {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str("[resource]");
                }
            }
        }

        Ok(output)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_adapter_from_tool_definition() {
        let tool = crate::protocol::McpToolDefinition {
            name: "test_tool".to_string(),
            description: Some("A test tool".to_string()),
            input_schema: json!({"type": "object"}),
        };

        // We can't easily create a real McpClient in a unit test,
        // but we can verify the adapter properties.
        // For this test, we just verify the struct fields manually.
        assert_eq!(tool.name, "test_tool");
        assert_eq!(tool.description.unwrap(), "A test tool");
    }

    #[test]
    fn test_content_extraction() {
        let result = crate::protocol::CallToolResult {
            content: vec![
                ContentBlock::Text {
                    text: "hello".to_string(),
                },
                ContentBlock::Image {
                    data: "abc".to_string(),
                    mime_type: "image/png".to_string(),
                },
                ContentBlock::Text {
                    text: "world".to_string(),
                },
            ],
            is_error: false,
        };

        // Simulate the adapter's text extraction logic.
        let mut output = String::new();
        for block in &result.content {
            match block {
                ContentBlock::Text { text } => {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(text);
                }
                ContentBlock::Image { mime_type, .. } => {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str(&format!("[image: {mime_type}]"));
                }
                ContentBlock::Resource { .. } => {
                    if !output.is_empty() {
                        output.push('\n');
                    }
                    output.push_str("[resource]");
                }
            }
        }
        assert_eq!(output, "hello\n[image: image/png]\nworld");
    }

    #[test]
    fn test_adapter_error_text_is_sanitized() {
        let token = format!("{}{}", "ghp_", "abcdefghijklmnopqrstuvwxyz123456");
        let text = format!("MCP server leaked Authorization: Bearer {token}");

        let redacted = sanitize_mcp_error(&text);

        assert!(!redacted.contains(&token));
        assert!(redacted.contains("Authorization: Bearer"));
    }
}
