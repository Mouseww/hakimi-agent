use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext, get_write_block_error};
use serde_json::{Value as JsonValue, json};
use std::path::PathBuf;
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool that performs find-and-replace operations on files.
pub struct PatchTool;

#[async_trait]
impl Tool for PatchTool {
    fn name(&self) -> &str {
        "patch"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Find and replace text in a file. Supports replacing a single occurrence or all occurrences. The old_string must be unique in the file unless replace_all is true."
    }

    fn emoji(&self) -> &str {
        "\u{1f527}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Absolute or relative path to the file to modify."
                },
                "old_string": {
                    "type": "string",
                    "description": "The text to find in the file."
                },
                "new_string": {
                    "type": "string",
                    "description": "The text to replace old_string with."
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "If true, replace all occurrences. If false (default), require old_string to be unique in the file."
                }
            },
            "required": ["path", "old_string", "new_string"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let path = args
            .get("path")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: path".into()))?;

        let old_string = args
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: old_string".into()))?;

        let new_string = args
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: new_string".into()))?;

        let replace_all = args
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let requested_path = PathBuf::from(path);
        let full_path = if requested_path.is_absolute() {
            requested_path
        } else {
            PathBuf::from(&ctx.workdir).join(requested_path)
        };

        debug!(path = %full_path.display(), replace_all, "patching file");

        if let Some(error) = get_write_block_error(&full_path) {
            return Err(HakimiError::ToolSimple(error));
        }

        let content = fs::read_to_string(&full_path).await.map_err(|e| {
            debug!(path = %full_path.display(), error = %e, "failed to read file for patching");
            HakimiError::ToolSimple(format!("failed to read '{}': {}", full_path.display(), e))
        })?;

        if old_string.is_empty() {
            return Err(HakimiError::ToolSimple("old_string cannot be empty".into()));
        }

        if old_string == new_string {
            return Err(HakimiError::ToolSimple(
                "old_string and new_string are identical, nothing to replace".into(),
            ));
        }

        let count = content.matches(old_string).count();

        if count == 0 {
            return Err(HakimiError::ToolSimple(format!(
                "old_string not found in '{}'",
                full_path.display()
            )));
        }

        if !replace_all && count > 1 {
            return Err(HakimiError::ToolSimple(format!(
                "old_string found {count} times in '{}'. It must be unique. Use replace_all=true to replace all occurrences.",
                full_path.display()
            )));
        }

        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        fs::write(&full_path, &new_content).await.map_err(|e| {
            debug!(path = %full_path.display(), error = %e, "failed to write patched file");
            HakimiError::ToolSimple(format!("failed to write '{}': {}", full_path.display(), e))
        })?;

        let replacements = if replace_all { count } else { 1 };
        Ok(format!(
            "Successfully replaced {replacements} occurrence(s) in {}",
            full_path.display()
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;
    use tokio::fs;

    fn test_ctx(workdir: &str) -> ToolContext {
        ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: workdir.to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        }
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = PatchTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["required"].is_array());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "path"));
        assert!(required.iter().any(|v| v == "old_string"));
        assert!(required.iter().any(|v| v == "new_string"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = PatchTool;
        assert_eq!(tool.name(), "patch");
        assert_eq!(tool.toolset(), "file");
        assert!(tool.check_available());
        assert!(tool.max_result_size().is_none());
    }

    #[tokio::test]
    async fn test_replace_unique_string() {
        let dir = std::env::temp_dir().join("hakimi_test_patch_unique");
        let _ = fs::create_dir_all(&dir).await;
        let file = dir.join("test.txt");
        fs::write(&file, "hello world hello").await.unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "path": file.to_string_lossy(),
            "old_string": "world",
            "new_string": "rust"
        });

        let result = PatchTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("1 occurrence"));

        let content = fs::read_to_string(&file).await.unwrap();
        assert_eq!(content, "hello rust hello");

        let _ = fs::remove_file(&file).await;
        let _ = fs::remove_dir(&dir).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_patch_denies_static_sensitive_path_before_reading() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "path": "/etc/passwd",
            "old_string": "root",
            "new_string": "blocked"
        });

        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("sensitive system or credential path"));
    }

    #[tokio::test]
    async fn test_replace_all() {
        let dir = std::env::temp_dir().join("hakimi_test_patch_all");
        let _ = fs::create_dir_all(&dir).await;
        let file = dir.join("test.txt");
        fs::write(&file, "aaa bbb aaa bbb aaa").await.unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "path": file.to_string_lossy(),
            "old_string": "aaa",
            "new_string": "ccc",
            "replace_all": true
        });

        let result = PatchTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("3 occurrence"));

        let content = fs::read_to_string(&file).await.unwrap();
        assert_eq!(content, "ccc bbb ccc bbb ccc");

        let _ = fs::remove_file(&file).await;
        let _ = fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn test_non_unique_string_error() {
        let dir = std::env::temp_dir().join("hakimi_test_patch_nonunique");
        let _ = fs::create_dir_all(&dir).await;
        let file = dir.join("test.txt");
        fs::write(&file, "aaa bbb aaa").await.unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "path": file.to_string_lossy(),
            "old_string": "aaa",
            "new_string": "ccc"
        });

        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("found 2 times") || msg.contains("must be unique"));

        let _ = fs::remove_file(&file).await;
        let _ = fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn test_identical_strings_error() {
        let dir = std::env::temp_dir().join("hakimi_test_patch_identical");
        let _ = fs::create_dir_all(&dir).await;
        let file = dir.join("test.txt");
        fs::write(&file, "hello world").await.unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "path": file.to_string_lossy(),
            "old_string": "hello",
            "new_string": "hello"
        });

        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("identical"));

        let _ = fs::remove_file(&file).await;
        let _ = fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn test_empty_old_string_error() {
        let dir = std::env::temp_dir().join("hakimi_test_patch_empty");
        let _ = fs::create_dir_all(&dir).await;
        let file = dir.join("test.txt");
        fs::write(&file, "hello world").await.unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "path": file.to_string_lossy(),
            "old_string": "",
            "new_string": "foo"
        });

        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        let msg = format!("{err}");
        assert!(msg.contains("empty"));

        let _ = fs::remove_file(&file).await;
        let _ = fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn test_missing_path_error() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "old_string": "hello",
            "new_string": "world"
        });
        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("path"));
    }

    #[tokio::test]
    async fn test_missing_old_string_error() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "path": "/tmp/somefile",
            "new_string": "world"
        });
        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("old_string"));
    }

    #[tokio::test]
    async fn test_missing_new_string_error() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "path": "/tmp/somefile",
            "old_string": "hello"
        });
        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("new_string"));
    }

    #[tokio::test]
    async fn test_string_not_found_error() {
        let dir = std::env::temp_dir().join("hakimi_test_patch_notfound");
        let _ = fs::create_dir_all(&dir).await;
        let file = dir.join("test.txt");
        fs::write(&file, "hello world").await.unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "path": file.to_string_lossy(),
            "old_string": "notfound",
            "new_string": "new"
        });

        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("not found"));

        let _ = fs::remove_file(&file).await;
        let _ = fs::remove_dir(&dir).await;
    }

    #[tokio::test]
    async fn test_file_not_found_error() {
        let ctx = test_ctx("/tmp");
        let args = json!({
            "path": "/tmp/hakimi_nonexistent_file_12345.txt",
            "old_string": "hello",
            "new_string": "world"
        });
        let err = PatchTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("failed to read"));
    }

    #[tokio::test]
    async fn test_relative_path_resolves_to_workdir() {
        let dir = std::env::temp_dir().join("hakimi_test_patch_relative");
        let _ = fs::create_dir_all(&dir).await;
        let file = dir.join("relative.txt");
        fs::write(&file, "alpha beta gamma").await.unwrap();

        let ctx = test_ctx(&dir.to_string_lossy());
        let args = json!({
            "path": "relative.txt",
            "old_string": "beta",
            "new_string": "delta"
        });

        let result = PatchTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("1 occurrence"));

        let content = fs::read_to_string(&file).await.unwrap();
        assert_eq!(content, "alpha delta gamma");

        let _ = fs::remove_file(&file).await;
        let _ = fs::remove_dir(&dir).await;
    }
}
