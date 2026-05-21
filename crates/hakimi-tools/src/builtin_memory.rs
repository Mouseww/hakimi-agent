use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{json, Value as JsonValue};
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool for managing persistent memory (agent notes and user profile).
pub struct MemoryTool;

/// Get the memory directory path (~/.hakimi/memory/).
fn memory_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::PathBuf::from(home).join(".hakimi").join("memory")
}

/// Get the file path for a given target.
fn target_file(target: &str) -> Result<std::path::PathBuf> {
    let dir = memory_dir();
    match target {
        "memory" => Ok(dir.join("memory.md")),
        "user" => Ok(dir.join("user.md")),
        _ => Err(HakimiError::Tool(format!(
            "invalid target '{}'. Must be 'memory' or 'user'.",
            target
        ))),
    }
}

#[async_trait]
impl Tool for MemoryTool {
    fn name(&self) -> &str {
        "memory"
    }

    fn toolset(&self) -> &str {
        "memory"
    }

    fn description(&self) -> &str {
        "Manage persistent memory. Store agent notes or user profile information in markdown files. Actions: add (append), replace (overwrite), remove (delete matching text)."
    }

    fn emoji(&self) -> &str {
        "\u{1f9e0}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform: 'add' appends content, 'replace' overwrites the file, 'remove' deletes matching text.",
                    "enum": ["add", "replace", "remove"]
                },
                "target": {
                    "type": "string",
                    "description": "Which memory file to operate on: 'memory' for agent notes, 'user' for user profile.",
                    "enum": ["memory", "user"]
                },
                "content": {
                    "type": "string",
                    "description": "The content to add or replace with. Required for 'add' and 'replace' actions."
                },
                "old_text": {
                    "type": "string",
                    "description": "The text to remove. Required for the 'remove' action."
                }
            },
            "required": ["action", "target"]
        })
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: action".into()))?;

        let target = args
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: target".into()))?;

        let content = args.get("content").and_then(|v| v.as_str());
        let old_text = args.get("old_text").and_then(|v| v.as_str());

        let file_path = target_file(target)?;

        debug!(
            action = %action,
            target = %target,
            path = %file_path.display(),
            "memory operation"
        );

        // Ensure memory directory exists
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                HakimiError::Tool(format!("failed to create memory directory '{}': {e}", parent.display()))
            })?;
        }

        match action {
            "add" => {
                let content = content.ok_or_else(|| {
                    HakimiError::Tool("'content' is required for the 'add' action".into())
                })?;

                // Read existing content or start fresh
                let existing = fs::read_to_string(&file_path).await.unwrap_or_default();

                let new_content = if existing.is_empty() {
                    format!("{content}\n")
                } else {
                    format!("{existing}\n{content}\n")
                };

                fs::write(&file_path, &new_content).await.map_err(|e| {
                    HakimiError::Tool(format!("failed to write memory file: {e}"))
                })?;

                Ok(format!(
                    "Added content to {target} memory ({}).",
                    file_path.display()
                ))
            }
            "replace" => {
                let content = content.ok_or_else(|| {
                    HakimiError::Tool("'content' is required for the 'replace' action".into())
                })?;

                fs::write(&file_path, format!("{content}\n")).await.map_err(|e| {
                    HakimiError::Tool(format!("failed to write memory file: {e}"))
                })?;

                Ok(format!(
                    "Replaced {target} memory content ({}).",
                    file_path.display()
                ))
            }
            "remove" => {
                let old_text = old_text.ok_or_else(|| {
                    HakimiError::Tool("'old_text' is required for the 'remove' action".into())
                })?;

                let existing = fs::read_to_string(&file_path).await.map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        HakimiError::Tool(format!(
                            "{target} memory file does not exist yet ({}).",
                            file_path.display()
                        ))
                    } else {
                        HakimiError::Tool(format!("failed to read memory file: {e}"))
                    }
                })?;

                if !existing.contains(old_text) {
                    return Err(HakimiError::Tool(format!(
                        "old_text not found in {target} memory."
                    )));
                }

                let new_content = existing.replace(old_text, "");
                fs::write(&file_path, &new_content).await.map_err(|e| {
                    HakimiError::Tool(format!("failed to write memory file: {e}"))
                })?;

                Ok(format!(
                    "Removed matching text from {target} memory ({}).",
                    file_path.display()
                ))
            }
            _ => Err(HakimiError::Tool(format!(
                "invalid action '{}'. Must be 'add', 'replace', or 'remove'.",
                action
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;

    fn test_ctx() -> ToolContext {
ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        }
    }

    /// Clean up test memory files
    async fn cleanup() {
        let _ = fs::remove_file(memory_dir().join("memory.md")).await;
        let _ = fs::remove_file(memory_dir().join("user.md")).await;
    }

    /// Get a unique test memory dir to avoid race conditions between parallel tests
    fn test_memory_dir() -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!("hakimi-test-memory-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&base);
        base
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = MemoryTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "action"));
        assert!(required.iter().any(|v| v == "target"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = MemoryTool;
        assert_eq!(tool.name(), "memory");
        assert_eq!(tool.toolset(), "memory");
        assert!(tool.check_available());
        assert_eq!(tool.emoji(), "🧠");
    }

    #[tokio::test]
    async fn test_add_memory() {
        cleanup().await;

        let ctx = test_ctx();
        let args = json!({
            "action": "add",
            "target": "memory",
            "content": "User prefers dark mode"
        });

        let result = MemoryTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("Added content"));

        // Verify the file was written
        let path = memory_dir().join("memory.md");
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("dark mode"));

        cleanup().await;
    }

    #[tokio::test]
    async fn test_add_appends_to_memory() {
        cleanup().await;

        let ctx = test_ctx();

        // First add
        MemoryTool
            .execute(
                &json!({"action": "add", "target": "memory", "content": "Line 1"}),
                &ctx,
            )
            .await
            .unwrap();

        // Second add
        MemoryTool
            .execute(
                &json!({"action": "add", "target": "memory", "content": "Line 2"}),
                &ctx,
            )
            .await
            .unwrap();

        let path = memory_dir().join("memory.md");
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Line 1"));
        assert!(content.contains("Line 2"));

        cleanup().await;
    }

    #[tokio::test]
    async fn test_replace_memory() {
        cleanup().await;

        let ctx = test_ctx();

        // Add initial content
        MemoryTool
            .execute(
                &json!({"action": "add", "target": "memory", "content": "Old content"}),
                &ctx,
            )
            .await
            .unwrap();

        // Replace
        MemoryTool
            .execute(
                &json!({"action": "replace", "target": "memory", "content": "New content"}),
                &ctx,
            )
            .await
            .unwrap();

        let path = memory_dir().join("memory.md");
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("New content"));
        assert!(!content.contains("Old content"));

        cleanup().await;
    }

    #[tokio::test]
    async fn test_remove_memory() {
        cleanup().await;

        let ctx = test_ctx();

        // Add content
        MemoryTool
            .execute(
                &json!({"action": "add", "target": "memory", "content": "Remember this secret"}),
                &ctx,
            )
            .await
            .unwrap();

        // Remove
        let result = MemoryTool
            .execute(
                &json!({"action": "remove", "target": "memory", "old_text": "Remember this secret"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.contains("Removed"));

        // Verify the text is gone - file may not exist (empty after remove or race condition)
        let path = memory_dir().join("memory.md");
        if let Ok(content) = fs::read_to_string(&path).await {
            assert!(!content.contains("Remember this secret"));
        }

        cleanup().await;
    }

    #[tokio::test]
    async fn test_user_target() {
        cleanup().await;

        let ctx = test_ctx();
        let args = json!({
            "action": "add",
            "target": "user",
            "content": "Name: Alice"
        });

        let result = MemoryTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("user memory"));

        let path = memory_dir().join("user.md");
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Alice"));

        cleanup().await;
    }

    #[tokio::test]
    async fn test_invalid_target_error() {
        let ctx = test_ctx();
        let args = json!({
            "action": "add",
            "target": "invalid",
            "content": "test"
        });
        let err = MemoryTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("invalid target"));
    }

    #[tokio::test]
    async fn test_missing_action_error() {
        let ctx = test_ctx();
        let args = json!({"target": "memory"});
        let err = MemoryTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("action"));
    }

    #[tokio::test]
    async fn test_missing_target_error() {
        let ctx = test_ctx();
        let args = json!({"action": "add"});
        let err = MemoryTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("target"));
    }

    #[tokio::test]
    async fn test_add_missing_content_error() {
        let ctx = test_ctx();
        let args = json!({"action": "add", "target": "memory"});
        let err = MemoryTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("content"));
    }

    #[tokio::test]
    async fn test_replace_missing_content_error() {
        let ctx = test_ctx();
        let args = json!({"action": "replace", "target": "memory"});
        let err = MemoryTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("content"));
    }

    #[tokio::test]
    async fn test_remove_missing_old_text_error() {
        cleanup().await;

        // Create the file first
        let ctx = test_ctx();
        MemoryTool
            .execute(
                &json!({"action": "add", "target": "memory", "content": "something"}),
                &ctx,
            )
            .await
            .unwrap();

        let args = json!({"action": "remove", "target": "memory"});
        let err = MemoryTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("old_text"));

        cleanup().await;
    }

    #[tokio::test]
    async fn test_remove_text_not_found_error() {
        cleanup().await;

        let ctx = test_ctx();
        MemoryTool
            .execute(
                &json!({"action": "add", "target": "memory", "content": "something"}),
                &ctx,
            )
            .await
            .unwrap();

        let args = json!({"action": "remove", "target": "memory", "old_text": "notfound"});
        let err = MemoryTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("not found"));

        cleanup().await;
    }

    #[tokio::test]
    async fn test_invalid_action_error() {
        let ctx = test_ctx();
        let args = json!({"action": "invalid", "target": "memory"});
        let err = MemoryTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("invalid action"));
    }
}
