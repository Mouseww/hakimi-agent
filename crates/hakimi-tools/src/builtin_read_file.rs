use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext, get_read_block_error};
use serde_json::{Value as JsonValue, json};
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool that reads a file and returns its content with line numbers.
pub struct ReadFileTool;

#[async_trait]
impl Tool for ReadFileTool {
    fn name(&self) -> &str {
        "read_file"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Read a file and return its content with line numbers. Supports optional offset (start line) and limit (max lines)."
    }

    fn emoji(&self) -> &str {
        "\u{1f4c4}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to read."
                },
                "offset": {
                    "type": "integer",
                    "description": "Line number to start reading from (1-indexed). Defaults to 1.",
                    "minimum": 1
                },
                "limit": {
                    "type": "integer",
                    "description": "Maximum number of lines to read. Defaults to 2000.",
                    "minimum": 1,
                    "maximum": 10000
                }
            },
            "required": ["path"]
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        // ~256 KB max for read results
        Some(256 * 1024)
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: path".into()))?;

        let offset = args
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;

        let limit = args
            .get("limit")
            .and_then(|v| v.as_u64())
            .unwrap_or(2000)
            .min(10000) as usize;

        // Resolve path relative to workdir if not absolute
        let requested_path = std::path::PathBuf::from(path);
        let full_path = if requested_path.is_absolute() {
            requested_path
        } else {
            std::path::PathBuf::from(&ctx.workdir).join(requested_path)
        };

        debug!(path = %full_path.display(), offset, limit, "reading file");

        if let Some(error) = get_read_block_error(&full_path) {
            return Err(HakimiError::Tool(error));
        }

        let content = fs::read_to_string(&full_path).await.map_err(|e| {
            debug!(path = %full_path.display(), error = %e, "failed to read file");
            HakimiError::Tool(format!("failed to read '{}': {}", full_path.display(), e))
        })?;

        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start = (offset - 1).min(total_lines);
        let end = (start + limit).min(total_lines);

        let mut result = String::new();
        for (i, line) in lines[start..end].iter().enumerate() {
            let line_num = start + i + 1;
            result.push_str(&format!("{line_num:>6}|{line}\n"));
        }

        if end < total_lines {
            result.push_str(&format!(
                "\n... showing lines {start_line}-{end_line} of {total_lines} total",
                start_line = start + 1,
                end_line = end,
                total_lines = total_lines,
            ));
        }

        Ok(result)
    }
}
