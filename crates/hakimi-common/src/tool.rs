use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// A tool call requested by the assistant.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    /// Unique identifier for this tool call.
    pub id: String,

    /// Name of the tool/function to invoke.
    pub name: String,

    /// JSON-encoded arguments string.
    pub arguments: String,

    /// Index of this tool call in a batch (provider-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u32>,
}

/// The result of executing a tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    /// ID of the tool call this result corresponds to.
    pub tool_call_id: String,

    /// Name of the tool that was executed.
    pub name: String,

    /// Text content of the result.
    pub content: String,

    /// Whether the tool execution resulted in an error.
    #[serde(default)]
    pub is_error: bool,
}

/// Definition of a tool that can be called by the model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Name of the tool.
    pub name: String,

    /// Human-readable description of what the tool does.
    pub description: String,

    /// JSON Schema describing the tool's parameters.
    pub parameters: JsonValue,
}

/// Contextual information available during tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    /// ID of the current session.
    pub session_id: String,

    /// ID of the user who initiated the request.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_id: Option<String>,

    /// ID of the current task, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,

    /// Working directory for the tool execution.
    pub workdir: String,
}
