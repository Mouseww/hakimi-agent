use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

/// A tool call requested by the assistant.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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

/// Trait for executing delegated sub-tasks via child agents.
///
/// Implementors hold the shared resources (transport, context engine, model,
/// tool registry) needed to spawn and run a child agent. The `delegate_task`
/// tool calls through this trait to perform actual delegation.
#[async_trait]
pub trait DelegateExecutor: Send + Sync {
    /// Spawn a child agent to accomplish `goal` with the given `context` and
    /// restricted to the listed `toolsets`. Returns the child agent's final
    /// text response.
    async fn execute_delegation(
        &self,
        goal: &str,
        context: &str,
        toolsets: &[String],
    ) -> crate::Result<String>;
}

/// Contextual information available during tool execution.
#[derive(Clone, Serialize, Deserialize)]
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

    /// Model identifier for spawning child agents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Executor for delegating sub-tasks to child agents.
    /// Holds shared resources (transport, context engine, tool registry).
    #[serde(skip)]
    pub delegate_executor: Option<Arc<dyn DelegateExecutor>>,
}

impl std::fmt::Debug for ToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ToolContext")
            .field("session_id", &self.session_id)
            .field("user_id", &self.user_id)
            .field("task_id", &self.task_id)
            .field("workdir", &self.workdir)
            .field("model", &self.model)
            .field("delegate_executor", &self.delegate_executor.is_some())
            .finish()
    }
}
