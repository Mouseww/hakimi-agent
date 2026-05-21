use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{json, Value as JsonValue};
use tokio::process::Command;
use tracing::debug;

use crate::Tool;

/// Default timeout for code execution (seconds).
const DEFAULT_TIMEOUT_SECS: u64 = 300;

/// Built-in tool for executing code snippets in various languages.
pub struct CodeExecTool;

#[async_trait]
impl Tool for CodeExecTool {
    fn name(&self) -> &str {
        "code_exec"
    }

    fn toolset(&self) -> &str {
        "code"
    }

    fn description(&self) -> &str {
        "Execute a code snippet in Python, JavaScript (Node.js), or Bash. Writes the code to a temp file and runs it with the appropriate interpreter. Returns stdout, stderr, and exit code."
    }

    fn emoji(&self) -> &str {
        "\u{1f40d}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "language": {
                    "type": "string",
                    "description": "Programming language for the code snippet. Defaults to 'python'.",
                    "enum": ["python", "javascript", "bash"]
                },
                "code": {
                    "type": "string",
                    "description": "The code snippet to execute."
                },
                "timeout": {
                    "type": "integer",
                    "description": "Maximum time in seconds to wait for execution. Defaults to 300.",
                    "minimum": 1,
                    "maximum": 600
                }
            },
            "required": ["code"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let code = args
            .get("code")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: code".into()))?;

        let language = args
            .get("language")
            .and_then(|v| v.as_str())
            .unwrap_or("python");

        let timeout_secs = args
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(DEFAULT_TIMEOUT_SECS)
            .min(600);

        debug!(language = %language, timeout = timeout_secs, "executing code snippet");

        // Determine interpreter and file extension
        let (interpreter, extension) = match language {
            "python" => ("python3", ".py"),
            "javascript" => ("node", ".js"),
            "bash" => ("bash", ".sh"),
            _ => {
                return Err(HakimiError::Tool(format!(
                    "unsupported language '{}'. Supported: python, javascript, bash.",
                    language
                )));
            }
        };

        // Write code to a temp file
        let temp_dir = std::path::PathBuf::from(&ctx.workdir).join(".hakimi_tmp");
        tokio::fs::create_dir_all(&temp_dir).await.map_err(|e| {
            HakimiError::Tool(format!("failed to create temp directory: {e}"))
        })?;

        let temp_file = temp_dir.join(format!(
            "snippet_{}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos(),
            extension
        ));
        tokio::fs::write(&temp_file, code).await.map_err(|e| {
            HakimiError::Tool(format!("failed to write temp file: {e}"))
        })?;

        // Execute the code
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(timeout_secs),
            Command::new(interpreter)
                .arg(&temp_file)
                .current_dir(&ctx.workdir)
                .output(),
        )
        .await;

        // Clean up temp file
        let _ = tokio::fs::remove_file(&temp_file).await;

        let output = result
            .map_err(|_| {
                HakimiError::Tool(format!(
                    "code execution timed out after {}s ({})",
                    timeout_secs, language
                ))
            })?
            .map_err(|e| {
                HakimiError::Tool(format!("failed to execute {} code: {e}", language))
            })?;

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);

        let mut result = String::new();

        result.push_str(&format!("Language: {}\n", language));

        if !stdout.is_empty() {
            result.push_str("STDOUT:\n");
            result.push_str(&stdout);
        }

        if !stderr.is_empty() {
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str("STDERR:\n");
            result.push_str(&stderr);
        }

        if let Some(code) = output.status.code() {
            if !result.ends_with('\n') {
                result.push('\n');
            }
            result.push_str(&format!("EXIT CODE: {}", code));
        } else {
            result.push_str("\nEXIT CODE: (terminated by signal)");
        }

        if result.trim().is_empty() || result == format!("Language: {}", language) {
            result = format!("Language: {}\n(no output)", language);
        }

        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;

    fn test_ctx(workdir: &str) -> ToolContext {
        ToolContext {
            session_id: "test-session".to_string(),
            user_id: None,
            task_id: None,
            workdir: workdir.to_string(),
        }
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = CodeExecTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["properties"]["code"].is_object());
        assert!(schema["properties"]["language"].is_object());
        assert!(schema["properties"]["timeout"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "code"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = CodeExecTool;
        assert_eq!(tool.name(), "code_exec");
        assert_eq!(tool.toolset(), "code");
        assert!(tool.check_available());
        assert_eq!(tool.emoji(), "🐍");
    }

    #[tokio::test]
    async fn test_execute_python_code() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "code": "print('hello from python')",
            "language": "python"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("python"));
        assert!(result.contains("hello from python"));
        assert!(result.contains("EXIT CODE: 0"));
    }

    #[tokio::test]
    async fn test_execute_bash_code() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "code": "echo 'hello from bash'",
            "language": "bash"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("bash"));
        assert!(result.contains("hello from bash"));
        assert!(result.contains("EXIT CODE: 0"));
    }

    #[tokio::test]
    async fn test_execute_python_default_language() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "code": "print(2 + 2)"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("python"));
        assert!(result.contains("4"));
    }

    #[tokio::test]
    async fn test_execute_with_stderr() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "code": "import sys; sys.stderr.write('error output\\n')",
            "language": "python"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("STDERR"));
        assert!(result.contains("error output"));
    }

    #[tokio::test]
    async fn test_execute_nonzero_exit_code() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "code": "import sys; sys.exit(42)",
            "language": "python"
        });

        let result = CodeExecTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("EXIT CODE: 42"));
    }

    #[tokio::test]
    async fn test_unsupported_language_error() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "code": "print('hello')",
            "language": "haskell"
        });
        let err = CodeExecTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("unsupported language"));
    }

    #[tokio::test]
    async fn test_missing_code_error() {
        let ctx = test_ctx("/tmp");
        let args = json!({"language": "python"});
        let err = CodeExecTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("code"));
    }

    #[tokio::test]
    async fn test_temp_file_cleanup() {
        let dir = std::env::temp_dir().join("hakimi_test_code_exec_cleanup");
        let _ = tokio::fs::create_dir_all(&dir).await;

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "code": "print('temp file test')",
            "language": "python"
        });

        let _ = CodeExecTool.execute(&args, &ctx).await.unwrap();

        // Temp file should be cleaned up
        let temp_dir = dir.join(".hakimi_tmp");
        if temp_dir.exists() {
            let mut entries = tokio::fs::read_dir(&temp_dir).await.unwrap();
            let mut count = 0;
            while entries.next_entry().await.unwrap().is_some() {
                count += 1;
            }
            assert_eq!(count, 0, "temp file should have been cleaned up");
        }

        let _ = tokio::fs::remove_dir_all(&dir).await;
    }
}
