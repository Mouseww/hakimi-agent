use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{json, Value as JsonValue};
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool that writes content to a file, creating parent directories as needed.
pub struct WriteFileTool;

#[async_trait]
impl Tool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Write content to a file. Creates parent directories if they don't exist. Overwrites the file if it already exists."
    }

    fn emoji(&self) -> &str {
        "\u{270f}\u{fe0f}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to write."
                },
                "content": {
                    "type": "string",
                    "description": "The content to write to the file."
                }
            },
            "required": ["path", "content"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: path".into()))?;

        let content = args
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: content".into()))?;

        // Resolve path relative to workdir if not absolute
        let full_path = if path.starts_with('/') {
            std::path::PathBuf::from(path)
        } else {
            std::path::PathBuf::from(&ctx.workdir).join(path)
        };

        debug!(path = %full_path.display(), bytes = content.len(), "writing file");

        // Create parent directories
        if let Some(parent) = full_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                debug!(dir = %parent.display(), error = %e, "failed to create parent directories");
                HakimiError::Tool(format!(
                    "failed to create directories '{}': {}",
                    parent.display(),
                    e
                ))
            })?;
        }

        fs::write(&full_path, content).await.map_err(|e| {
            debug!(path = %full_path.display(), error = %e, "failed to write file");
            HakimiError::Tool(format!("failed to write '{}': {}", full_path.display(), e))
        })?;

        let line_count = content.lines().count();
        Ok(format!(
            "Successfully wrote {bytes} bytes ({lines} lines) to {path}",
            bytes = content.len(),
            lines = line_count,
            path = full_path.display(),
        ))
    }
}
