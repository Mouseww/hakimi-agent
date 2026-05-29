//! MCP (Model Context Protocol) JSON-RPC types.
//!
//! Implements the wire format for MCP over JSON-RPC 2.0, including
//! initialize, tools/list, and tools/call request/response types.

use serde::{Deserialize, Serialize};
use serde_json::Value;

// ---------------------------------------------------------------------------
// JSON-RPC 2.0 base types
// ---------------------------------------------------------------------------

/// A JSON-RPC 2.0 request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub id: u64,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcRequest {
    pub fn new(id: u64, method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            method: method.into(),
            params,
        }
    }
}

/// A JSON-RPC 2.0 request received from the MCP server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcServerRequest {
    pub jsonrpc: String,
    pub id: Value,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

/// A JSON-RPC 2.0 error object.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("MCP client method '{method}' is not available"),
            data: None,
        }
    }

    pub fn invalid_params(message: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: message.into(),
            data: None,
        }
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: message.into(),
            data: None,
        }
    }
}

/// A JSON-RPC 2.0 response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

/// A JSON-RPC 2.0 response sent back to an MCP server request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcServerResponse {
    pub jsonrpc: String,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcServerResponse {
    pub fn success(id: Value, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: Value, error: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            id,
            result: None,
            error: Some(error),
        }
    }
}

// ---------------------------------------------------------------------------
// MCP: Initialize
// ---------------------------------------------------------------------------

/// Client capabilities advertised during initialization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub roots: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sampling: Option<SamplingCapability>,
}

impl ClientCapabilities {
    pub fn basic() -> Self {
        Self {
            roots: None,
            sampling: None,
        }
    }

    pub fn with_sampling() -> Self {
        Self {
            roots: None,
            sampling: Some(SamplingCapability::default()),
        }
    }
}

/// Client-side sampling capability advertised to MCP servers.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SamplingCapability {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<SamplingToolsCapability>,
}

/// Sampling tool-use capability marker.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SamplingToolsCapability {}

/// Client information sent in the initialize request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

/// Parameters for the `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeParams {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ClientCapabilities,
    #[serde(rename = "clientInfo")]
    pub client_info: ClientInfo,
}

/// Server capabilities returned in the initialize result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompts: Option<Value>,
}

/// Server information returned in the initialize result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
}

/// Result of the `initialize` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitializeResult {
    #[serde(rename = "protocolVersion")]
    pub protocol_version: String,
    pub capabilities: ServerCapabilities,
    #[serde(rename = "serverInfo")]
    pub server_info: ServerInfo,
}

// ---------------------------------------------------------------------------
// MCP: Tools
// ---------------------------------------------------------------------------

/// Definition of an MCP tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpToolDefinition {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "inputSchema")]
    pub input_schema: Value,
}

/// Result of the `tools/list` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListToolsResult {
    pub tools: Vec<McpToolDefinition>,
    #[serde(rename = "nextCursor", skip_serializing_if = "Option::is_none")]
    pub next_cursor: Option<String>,
}

/// Parameters for the `tools/call` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolParams {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arguments: Option<Value>,
}

/// A content block in a tool call result (text or image).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image {
        data: String,
        #[serde(rename = "mimeType")]
        mime_type: String,
    },
    #[serde(rename = "resource")]
    Resource { resource: Value },
}

/// Result of the `tools/call` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallToolResult {
    pub content: Vec<ContentBlock>,
    #[serde(rename = "isError", default)]
    pub is_error: bool,
}

impl CallToolResult {
    /// Extract all text content concatenated into a single string.
    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|block| match block {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

// ---------------------------------------------------------------------------
// MCP: Sampling
// ---------------------------------------------------------------------------

/// Parameters for server-initiated `sampling/createMessage`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageParams {
    pub messages: Vec<SamplingMessage>,
    #[serde(rename = "maxTokens")]
    pub max_tokens: u32,
    #[serde(rename = "systemPrompt", skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f32>,
    #[serde(rename = "modelPreferences", skip_serializing_if = "Option::is_none")]
    pub model_preferences: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tools: Option<Vec<McpToolDefinition>>,
}

/// A single sampling conversation message.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingMessage {
    pub role: String,
    pub content: Value,
}

/// Result returned to an MCP `sampling/createMessage` request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateMessageResult {
    pub role: String,
    pub content: ContentBlock,
    pub model: String,
    #[serde(rename = "stopReason", skip_serializing_if = "Option::is_none")]
    pub stop_reason: Option<String>,
}

// ---------------------------------------------------------------------------
// MCP: Notifications (no id, no result)
// ---------------------------------------------------------------------------

/// A JSON-RPC 2.0 notification (request with no id, expects no response).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JsonRpcNotification {
    pub jsonrpc: String,
    pub method: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub params: Option<Value>,
}

impl JsonRpcNotification {
    pub fn new(method: impl Into<String>, params: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: method.into(),
            params,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_serialization() {
        let req = JsonRpcRequest::new(
            1,
            "initialize",
            Some(json!({"protocolVersion": "2024-11-05"})),
        );
        let s = serde_json::to_string(&req).unwrap();
        assert!(s.contains("\"jsonrpc\":\"2.0\""));
        assert!(s.contains("\"method\":\"initialize\""));
    }

    #[test]
    fn test_response_deserialization() {
        let json_str = r#"{"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2024-11-05","capabilities":{},"serverInfo":{"name":"test","version":"0.2.1"}}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).unwrap();
        assert_eq!(resp.id, 1);
        assert!(resp.error.is_none());
        let result: InitializeResult = serde_json::from_value(resp.result.unwrap()).unwrap();
        assert_eq!(result.server_info.name, "test");
    }

    #[test]
    fn test_sampling_capability_serialization() {
        let params = InitializeParams {
            protocol_version: "2024-11-05".to_string(),
            capabilities: ClientCapabilities::with_sampling(),
            client_info: ClientInfo {
                name: "test".to_string(),
                version: "0.2.1".to_string(),
            },
        };
        let v = json!(params);
        assert_eq!(v["protocolVersion"], "2024-11-05");
        assert_eq!(v["clientInfo"]["name"], "test");
        assert!(v["capabilities"]["sampling"].is_object());
    }

    #[test]
    fn test_basic_client_capabilities_omit_sampling() {
        let v = json!(ClientCapabilities::basic());
        assert!(v.get("sampling").is_none());
    }

    #[test]
    fn test_tool_definition_roundtrip() {
        let tool = McpToolDefinition {
            name: "read_file".to_string(),
            description: Some("Read a file".to_string()),
            input_schema: json!({"type": "object", "properties": {"path": {"type": "string"}}}),
        };
        let s = serde_json::to_string(&tool).unwrap();
        let back: McpToolDefinition = serde_json::from_str(&s).unwrap();
        assert_eq!(back.name, "read_file");
    }

    #[test]
    fn test_call_tool_result_text_content() {
        let result = CallToolResult {
            content: vec![
                ContentBlock::Text {
                    text: "line1".to_string(),
                },
                ContentBlock::Text {
                    text: "line2".to_string(),
                },
            ],
            is_error: false,
        };
        assert_eq!(result.text_content(), "line1\nline2");
    }

    #[test]
    fn test_content_block_serde() {
        let block = ContentBlock::Text {
            text: "hello".to_string(),
        };
        let s = serde_json::to_string(&block).unwrap();
        assert!(s.contains("\"type\":\"text\""));
        assert!(s.contains("\"text\":\"hello\""));
    }

    #[test]
    fn test_image_content_block() {
        let block = ContentBlock::Image {
            data: "base64data".to_string(),
            mime_type: "image/png".to_string(),
        };
        let s = serde_json::to_string(&block).unwrap();
        assert!(s.contains("\"type\":\"image\""));
        assert!(s.contains("\"mimeType\":\"image/png\""));
    }

    #[test]
    fn test_error_response() {
        let json_str =
            r#"{"jsonrpc":"2.0","id":1,"error":{"code":-32600,"message":"Invalid Request"}}"#;
        let resp: JsonRpcResponse = serde_json::from_str(json_str).unwrap();
        assert!(resp.result.is_none());
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32600);
    }

    #[test]
    fn test_notification_serialization() {
        let notif = JsonRpcNotification::new("notifications/initialized", None);
        let s = serde_json::to_string(&notif).unwrap();
        assert!(s.contains("\"jsonrpc\":\"2.0\""));
        // Notifications must NOT have an "id" field
        assert!(!s.contains("\"id\""));
    }

    #[test]
    fn test_server_response_error_serialization() {
        let response =
            JsonRpcServerResponse::error(json!("abc"), JsonRpcError::method_not_found("demo"));
        let v = json!(response);
        assert_eq!(v["id"], "abc");
        assert_eq!(v["error"]["code"], -32601);
        assert!(v.get("result").is_none());
    }

    #[test]
    fn test_create_message_params_deserialization() {
        let params: CreateMessageParams = serde_json::from_value(json!({
            "messages": [{"role": "user", "content": {"type": "text", "text": "hi"}}],
            "maxTokens": 128,
            "systemPrompt": "answer briefly",
            "temperature": 0.1
        }))
        .unwrap();

        assert_eq!(params.max_tokens, 128);
        assert_eq!(params.system_prompt.as_deref(), Some("answer briefly"));
        assert_eq!(params.messages[0].role, "user");
    }
}
