use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{json, Value as JsonValue};
use tokio::process::Command;
use tracing::debug;

use crate::Tool;

/// Default timeout for terminal commands (seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 180;

/// Built-in tool that executes shell commands.
pub struct TerminalTool;

#[async_trait]
impl Tool for TerminalTool {
    fn name(&self) -> &str {
        "terminal"
    }

    fn toolset(&self) -> &str {
        "shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command via bash and return stdout + stderr. Supports optional timeout and working directory."
    }

    fn emoji(&self) -> &str {
        "\u{1f4bb}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "command": {
                    "type": "string",
                    "description": "The shell command to execute."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Maximum time in seconds to wait for the command to finish. Defaults to 180.",
                    "minimum": 1,
                    "maximum": 600
                },
                "workdir": {
                    "type": "string",
                    "description": "Working directory for the command. Defaults to the tool context workdir."
                }
            },
            "required": ["command"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let command = args
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: command".into()))?;

        let timeout_secs = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(600);

        let workdir = args
            .get("workdir")
            .and_then(|v| v.as_str())
            .unwrap_or(&ctx.workdir);

        debug!(command = %command, timeout = timeout_secs, workdir = %workdir, "executing terminal command");

        let output = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            Command::new("bash")
                .arg("-c")
                .arg(command)
                .current_dir(workdir)
                .output(),
        )
        .await
        .map_err(|_| {
            debug!(command = %command, timeout = timeout_secs, "command timed out");
            HakimiError::Tool(format!(
                "command timed out after {timeout_secs}s: {command}"
            ))
        })?
        .map_err(|e| {
            debug!(command = %command, error = %e, "failed to spawn command");
            HakimiError::Tool(format!("failed to execute command: {e}"))
        })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();

        if !stdout.is_empty() {
            result.push_str("STDOUT:\n");
            result.push_str(&stdout);
        }

        if !stderr.is_empty() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str("STDERR:\n");
            result.push_str(&stderr);
        }

        if let Some(code) = output.status.code() {
            if !result.is_empty() {
                result.push('\n');
            }
            result.push_str(&format!("EXIT CODE: {code}"));
        } else {
            result.push_str("\nEXIT CODE: (terminated by signal)");
        }

        if result.trim().is_empty() {
            result = "(no output)".to_string();
        }

        Ok(result)
    }
}
