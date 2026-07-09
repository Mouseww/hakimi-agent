use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool for managing persistent memory (agent notes and user profile).
///
/// Supports an optional custom base directory for testing; defaults to
/// the active Hakimi runtime home in production.
pub struct MemoryTool {
    /// Override directory for tests. `None` → default runtime `memory/`.
    base_dir: Option<std::path::PathBuf>,
}

impl MemoryTool {
    /// Create a MemoryTool that uses the default runtime `memory/` directory.
    pub fn new() -> Self {
        Self { base_dir: None }
    }

    /// Create a MemoryTool rooted at a custom directory (for testing).
    pub fn with_dir(dir: std::path::PathBuf) -> Self {
        Self {
            base_dir: Some(dir),
        }
    }

    /// Resolve the memory directory.
    fn memory_dir(&self) -> std::path::PathBuf {
        if let Some(ref dir) = self.base_dir {
            return dir.clone();
        }
        hakimi_common::effective_hakimi_home().join("memory")
    }

    /// Resolve the file path for a given target.
    fn target_file(&self, target: &str) -> Result<std::path::PathBuf> {
        let dir = self.memory_dir();
        match target {
            "memory" => Ok(dir.join("memory.md")),
            "user" => Ok(dir.join("user.md")),
            "working_memory" | "working" => Ok(dir.join("working_memory.md")),
            _ => Err(HakimiError::ToolSimple(format!(
                "invalid target '{}'. Must be 'memory', 'user', or 'working_memory'.",
                target
            ))),
        }
    }
}

impl Default for MemoryTool {
    fn default() -> Self {
        Self::new()
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
        "Manage persistent memory. Store agent notes, user profile, or working memory (current session) in markdown files. Actions: add (append), replace (overwrite), remove (delete matching text). Targets: 'memory' (long-term notes), 'user' (profile), 'working_memory' (session-scoped)."
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
                    "description": "Which memory file to operate on: 'memory' for agent notes, 'user' for user profile, 'working_memory' for current session.",
                    "enum": ["memory", "user", "working_memory"]
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
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: action".into()))?;

        let target = args
            .get("target")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: target".into()))?;

        let content = args.get("content").and_then(|v| v.as_str());
        let old_text = args.get("old_text").and_then(|v| v.as_str());

        let file_path = self.target_file(target)?;

        debug!(
            action = %action,
            target = %target,
            path = %file_path.display(),
            "memory operation"
        );

        // Ensure memory directory exists
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).await.map_err(|e| {
                HakimiError::ToolSimple(format!(
                    "failed to create memory directory '{}': {e}",
                    parent.display()
                ))
            })?;
        }

        match action {
            "add" => {
                let content = content.ok_or_else(|| {
                    HakimiError::ToolSimple("'content' is required for the 'add' action".into())
                })?;

                // Read existing content or start fresh
                let existing = fs::read_to_string(&file_path).await.unwrap_or_default();

                let new_content = if existing.is_empty() {
                    format!("{content}\n")
                } else {
                    format!("{existing}\n{content}\n")
                };

                fs::write(&file_path, &new_content).await.map_err(|e| {
                    HakimiError::ToolSimple(format!("failed to write memory file: {e}"))
                })?;

                Ok(format!(
                    "Added content to {target} memory ({}).",
                    file_path.display()
                ))
            }
            "replace" => {
                let content = content.ok_or_else(|| {
                    HakimiError::ToolSimple("'content' is required for the 'replace' action".into())
                })?;

                fs::write(&file_path, format!("{content}\n"))
                    .await
                    .map_err(|e| {
                        HakimiError::ToolSimple(format!("failed to write memory file: {e}"))
                    })?;

                Ok(format!(
                    "Replaced {target} memory content ({}).",
                    file_path.display()
                ))
            }
            "remove" => {
                let old_text = old_text.ok_or_else(|| {
                    HakimiError::ToolSimple("'old_text' is required for the 'remove' action".into())
                })?;

                let existing = fs::read_to_string(&file_path).await.map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        HakimiError::ToolSimple(format!(
                            "{target} memory file does not exist yet ({}).",
                            file_path.display()
                        ))
                    } else {
                        HakimiError::ToolSimple(format!("failed to read memory file: {e}"))
                    }
                })?;

                if !existing.contains(old_text) {
                    return Err(HakimiError::ToolSimple(format!(
                        "old_text not found in {target} memory."
                    )));
                }

                let new_content = existing.replace(old_text, "");
                fs::write(&file_path, &new_content).await.map_err(|e| {
                    HakimiError::ToolSimple(format!("failed to write memory file: {e}"))
                })?;

                Ok(format!(
                    "Removed matching text from {target} memory ({}).",
                    file_path.display()
                ))
            }
            _ => Err(HakimiError::ToolSimple(format!(
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

    /// Create a ToolContext and a MemoryTool backed by a unique temp directory.
    /// Returns `(tool, ctx, temp_dir)` — the temp_dir is kept alive so it is
    /// not deleted until the test finishes.
    fn setup() -> (MemoryTool, ToolContext, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("failed to create temp dir");
        let tool = MemoryTool::with_dir(dir.path().to_path_buf());
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        };
        (tool, ctx, dir)
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = MemoryTool::new();
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "action"));
        assert!(required.iter().any(|v| v == "target"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = MemoryTool::new();
        assert_eq!(tool.name(), "memory");
        assert_eq!(tool.toolset(), "memory");
        assert!(tool.check_available());
        assert_eq!(tool.emoji(), "🧠");
    }

    #[tokio::test]
    async fn test_add_memory() {
        let (tool, ctx, _dir) = setup();

        let args = json!({
            "action": "add",
            "target": "memory",
            "content": "User prefers dark mode"
        });

        let result = tool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("Added content"));

        // Verify the file was written
        let path = _dir.path().join("memory.md");
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("dark mode"));
    }

    #[tokio::test]
    async fn test_add_appends_to_memory() {
        let (tool, ctx, _dir) = setup();

        // First add
        tool.execute(
            &json!({"action": "add", "target": "memory", "content": "Line 1"}),
            &ctx,
        )
        .await
        .unwrap();

        // Second add
        tool.execute(
            &json!({"action": "add", "target": "memory", "content": "Line 2"}),
            &ctx,
        )
        .await
        .unwrap();

        let path = _dir.path().join("memory.md");
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Line 1"));
        assert!(content.contains("Line 2"));
    }

    #[tokio::test]
    async fn test_replace_memory() {
        let (tool, ctx, _dir) = setup();

        // Add initial content
        tool.execute(
            &json!({"action": "add", "target": "memory", "content": "Old content"}),
            &ctx,
        )
        .await
        .unwrap();

        // Replace
        tool.execute(
            &json!({"action": "replace", "target": "memory", "content": "New content"}),
            &ctx,
        )
        .await
        .unwrap();

        let path = _dir.path().join("memory.md");
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("New content"));
        assert!(!content.contains("Old content"));
    }

    #[tokio::test]
    async fn test_remove_memory() {
        let (tool, ctx, _dir) = setup();

        // Add content
        tool.execute(
            &json!({"action": "add", "target": "memory", "content": "Remember this secret"}),
            &ctx,
        )
        .await
        .unwrap();

        // Remove
        let result = tool
            .execute(
                &json!({"action": "remove", "target": "memory", "old_text": "Remember this secret"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.contains("Removed"));

        // Verify the text is gone
        let path = _dir.path().join("memory.md");
        if let Ok(content) = fs::read_to_string(&path).await {
            assert!(!content.contains("Remember this secret"));
        }
    }

    #[tokio::test]
    async fn test_user_target() {
        let (tool, ctx, _dir) = setup();
        let args = json!({
            "action": "add",
            "target": "user",
            "content": "Name: Alice"
        });

        let result = tool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("user memory"));

        let path = _dir.path().join("user.md");
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Alice"));
    }

    #[tokio::test]
    async fn test_invalid_target_error() {
        let (tool, ctx, _dir) = setup();
        let args = json!({
            "action": "add",
            "target": "invalid",
            "content": "test"
        });
        let err = tool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("invalid target"));
    }

    #[tokio::test]
    async fn test_missing_action_error() {
        let (tool, ctx, _dir) = setup();
        let args = json!({"target": "memory"});
        let err = tool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("action"));
    }

    #[tokio::test]
    async fn test_missing_target_error() {
        let (tool, ctx, _dir) = setup();
        let args = json!({"action": "add"});
        let err = tool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("target"));
    }

    #[tokio::test]
    async fn test_add_missing_content_error() {
        let (tool, ctx, _dir) = setup();
        let args = json!({"action": "add", "target": "memory"});
        let err = tool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("content"));
    }

    #[tokio::test]
    async fn test_replace_missing_content_error() {
        let (tool, ctx, _dir) = setup();
        let args = json!({"action": "replace", "target": "memory"});
        let err = tool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("content"));
    }

    #[tokio::test]
    async fn test_remove_missing_old_text_error() {
        let (tool, ctx, _dir) = setup();

        // Create the file first
        tool.execute(
            &json!({"action": "add", "target": "memory", "content": "something"}),
            &ctx,
        )
        .await
        .unwrap();

        let args = json!({"action": "remove", "target": "memory"});
        let err = tool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("old_text"));
    }

    #[tokio::test]
    async fn test_remove_text_not_found_error() {
        let (tool, ctx, _dir) = setup();

        tool.execute(
            &json!({"action": "add", "target": "memory", "content": "something"}),
            &ctx,
        )
        .await
        .unwrap();

        let args = json!({"action": "remove", "target": "memory", "old_text": "notfound"});
        let err = tool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[tokio::test]
    async fn test_invalid_action_error() {
        let (tool, ctx, _dir) = setup();
        let args = json!({"action": "invalid", "target": "memory"});
        let err = tool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("invalid action"));
    }
}
