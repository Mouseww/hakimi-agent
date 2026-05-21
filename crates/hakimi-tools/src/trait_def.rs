use async_trait::async_trait;
use hakimi_common::{Result, ToolContext};
use serde_json::Value as JsonValue;

/// Core trait that every tool in the Hakimi Agent must implement.
#[async_trait]
pub trait Tool: Send + Sync {
    /// Unique name of the tool (used for dispatch).
    fn name(&self) -> &str;

    /// Toolset / category this tool belongs to (e.g. "file", "shell").
    fn toolset(&self) -> &str;

    /// Human-readable description of what the tool does.
    fn description(&self) -> &str;

    /// Emoji icon for the tool (defaults to ⚡).
    fn emoji(&self) -> &str {
        "\u{26a1}"
    }

    /// JSON Schema describing the tool's parameters.
    fn schema(&self) -> JsonValue;

    /// Whether the tool is currently available for use.
    fn check_available(&self) -> bool {
        true
    }

    /// Optional maximum size (in bytes) for the tool result.
    /// Results exceeding this may be truncated by the framework.
    fn max_result_size(&self) -> Option<usize> {
        None
    }

    /// Execute the tool with the given arguments and context.
    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String>;
}
