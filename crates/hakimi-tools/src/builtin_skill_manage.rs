use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool for managing skills (reusable prompt templates stored as markdown).
pub struct SkillManageTool;

/// Get the skills directory path (~/.hakimi/skills/).
fn skills_dir() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
    std::path::PathBuf::from(home)
        .join(".hakimi")
        .join("skills")
}

/// Get the file path for a given skill name.
fn skill_file(name: &str) -> Result<std::path::PathBuf> {
    // Sanitize the name to prevent path traversal
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.starts_with('.')
    {
        return Err(HakimiError::Tool(format!(
            "invalid skill name '{name}'. Names cannot be empty, contain path separators, '..' , or start with '.'."
        )));
    }
    Ok(skills_dir().join(format!("{name}.md")))
}

#[async_trait]
impl Tool for SkillManageTool {
    fn name(&self) -> &str {
        "skill_manage"
    }

    fn toolset(&self) -> &str {
        "meta"
    }

    fn description(&self) -> &str {
        "Manage reusable skill templates stored as markdown files. Actions: create (write new skill), read (get skill content), update (modify existing skill), delete (remove skill), list (show all skills)."
    }

    fn emoji(&self) -> &str {
        "\u{1f4da}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform on skills.",
                    "enum": ["create", "read", "update", "delete", "list"]
                },
                "name": {
                    "type": "string",
                    "description": "Name of the skill. Required for create, read, update, and delete actions. Must be a simple identifier (no slashes or dots)."
                },
                "content": {
                    "type": "string",
                    "description": "Markdown content for the skill. Required for create and update actions."
                }
            },
            "required": ["action"]
        })
    }

    fn max_result_size(&self) -> Option<usize> {
        Some(64 * 1024)
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: action".into()))?;

        let name = args.get("name").and_then(|v| v.as_str());
        let content = args.get("content").and_then(|v| v.as_str());

        debug!(action = %action, name = ?name, "skill_manage operation");

        // Ensure skills directory exists
        let dir = skills_dir();
        fs::create_dir_all(&dir).await.map_err(|e| {
            HakimiError::Tool(format!(
                "failed to create skills directory '{}': {e}",
                dir.display()
            ))
        })?;

        match action {
            "create" => {
                let name = name.ok_or_else(|| {
                    HakimiError::Tool("'name' is required for 'create' action".into())
                })?;
                let content = content.ok_or_else(|| {
                    HakimiError::Tool("'content' is required for 'create' action".into())
                })?;

                let path = skill_file(name)?;

                // Check if skill already exists
                if path.exists() {
                    return Err(HakimiError::Tool(format!(
                        "skill '{name}' already exists. Use 'update' to modify it."
                    )));
                }

                fs::write(&path, format!("{content}\n"))
                    .await
                    .map_err(|e| HakimiError::Tool(format!("failed to write skill file: {e}")))?;

                Ok(format!("Created skill '{name}' ({}).", path.display()))
            }
            "read" => {
                let name = name.ok_or_else(|| {
                    HakimiError::Tool("'name' is required for 'read' action".into())
                })?;

                let path = skill_file(name)?;

                let content = fs::read_to_string(&path).await.map_err(|e| {
                    if e.kind() == std::io::ErrorKind::NotFound {
                        HakimiError::Tool(format!("skill '{name}' not found."))
                    } else {
                        HakimiError::Tool(format!("failed to read skill file: {e}"))
                    }
                })?;

                Ok(content)
            }
            "update" => {
                let name = name.ok_or_else(|| {
                    HakimiError::Tool("'name' is required for 'update' action".into())
                })?;
                let content = content.ok_or_else(|| {
                    HakimiError::Tool("'content' is required for 'update' action".into())
                })?;

                let path = skill_file(name)?;

                // Check that the skill exists
                if !path.exists() {
                    return Err(HakimiError::Tool(format!(
                        "skill '{name}' not found. Use 'create' to make a new skill."
                    )));
                }

                fs::write(&path, format!("{content}\n"))
                    .await
                    .map_err(|e| HakimiError::Tool(format!("failed to write skill file: {e}")))?;

                Ok(format!("Updated skill '{name}' ({}).", path.display()))
            }
            "delete" => {
                let name = name.ok_or_else(|| {
                    HakimiError::Tool("'name' is required for 'delete' action".into())
                })?;

                let path = skill_file(name)?;

                if !path.exists() {
                    return Err(HakimiError::Tool(format!("skill '{name}' not found.")));
                }

                fs::remove_file(&path)
                    .await
                    .map_err(|e| HakimiError::Tool(format!("failed to delete skill file: {e}")))?;

                Ok(format!("Deleted skill '{name}'."))
            }
            "list" => {
                let mut entries = fs::read_dir(&dir).await.map_err(|e| {
                    HakimiError::Tool(format!("failed to read skills directory: {e}"))
                })?;

                let mut skills = Vec::new();
                while let Some(entry) = entries.next_entry().await.map_err(|e| {
                    HakimiError::Tool(format!("failed to read directory entry: {e}"))
                })? {
                    let path = entry.path();
                    if path.extension().is_some_and(|ext| ext == "md")
                        && let Some(stem) = path.file_stem() {
                            skills.push(stem.to_string_lossy().to_string());
                    }
                }

                skills.sort();

                if skills.is_empty() {
                    return Ok("No skills found.".to_string());
                }

                let mut output = format!("Skills ({}):\n", skills.len());
                for skill in &skills {
                    output.push_str(&format!("  - {skill}\n"));
                }
                Ok(output)
            }
            _ => Err(HakimiError::Tool(format!(
                "invalid action '{}'. Must be 'create', 'read', 'update', 'delete', or 'list'.",
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
            delegate_executor: None, ..Default::default() }
    }

    /// Generate unique skill name to avoid test collisions
    fn unique_name(prefix: &str) -> String {
        format!("{}_{}", prefix, std::process::id())
    }

    /// Clean up a test skill file
    async fn cleanup_skill(name: &str) {
        let _ = fs::remove_file(skills_dir().join(format!("{name}.md"))).await;
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = SkillManageTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["name"].is_object());
        assert!(schema["properties"]["content"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "action"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = SkillManageTool;
        assert_eq!(tool.name(), "skill_manage");
        assert_eq!(tool.toolset(), "meta");
        assert!(tool.check_available());
        assert_eq!(tool.max_result_size(), Some(64 * 1024));
    }

    #[tokio::test]
    async fn test_create_skill() {
        let name = unique_name("test_create_skill");
        cleanup_skill(&name).await;

        let ctx = test_ctx();
        let args = json!({
            "action": "create",
            "name": name,
            "content": "# My Skill\nUse this for testing."
        });

        let result = SkillManageTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("Created skill"));
        assert!(result.contains(&name));

        // Verify file exists
        let path = skill_file(&name).unwrap();
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("My Skill"));

        cleanup_skill(&name).await;
    }

    #[tokio::test]
    async fn test_read_skill() {
        let name = unique_name("test_read_skill");
        cleanup_skill(&name).await;

        let ctx = test_ctx();

        // Create first
        SkillManageTool
            .execute(
                &json!({"action": "create", "name": name, "content": "# Readable Skill"}),
                &ctx,
            )
            .await
            .unwrap();

        // Read
        let result = SkillManageTool
            .execute(&json!({"action": "read", "name": name}), &ctx)
            .await
            .unwrap();
        assert!(result.contains("Readable Skill"));

        cleanup_skill(&name).await;
    }

    #[tokio::test]
    async fn test_list_skills() {
        let name = unique_name("test_list_skill");
        cleanup_skill(&name).await;

        let ctx = test_ctx();

        // Create a skill
        SkillManageTool
            .execute(
                &json!({"action": "create", "name": name, "content": "# Listed"}),
                &ctx,
            )
            .await
            .unwrap();

        // List
        let result = SkillManageTool
            .execute(&json!({"action": "list"}), &ctx)
            .await
            .unwrap();
        assert!(result.contains("Skills"));
        // Our skill should be in the list
        assert!(result.contains(&name));

        cleanup_skill(&name).await;
    }

    #[tokio::test]
    async fn test_delete_skill() {
        let name = unique_name("test_delete_skill");
        cleanup_skill(&name).await;

        let ctx = test_ctx();

        // Create
        SkillManageTool
            .execute(
                &json!({"action": "create", "name": name, "content": "# Deletable"}),
                &ctx,
            )
            .await
            .unwrap();

        // Delete
        let result = SkillManageTool
            .execute(&json!({"action": "delete", "name": name}), &ctx)
            .await
            .unwrap();
        assert!(result.contains("Deleted"));

        // Verify it's gone
        let path = skill_file(&name).unwrap();
        assert!(!path.exists());
    }

    #[tokio::test]
    async fn test_update_skill() {
        let name = unique_name("test_update_skill");
        cleanup_skill(&name).await;

        let ctx = test_ctx();

        // Create
        SkillManageTool
            .execute(
                &json!({"action": "create", "name": name, "content": "# Old"}),
                &ctx,
            )
            .await
            .unwrap();

        // Update
        let result = SkillManageTool
            .execute(
                &json!({"action": "update", "name": name, "content": "# Updated"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(result.contains("Updated"));

        // Verify content
        let path = skill_file(&name).unwrap();
        let content = fs::read_to_string(&path).await.unwrap();
        assert!(content.contains("Updated"));
        assert!(!content.contains("Old"));

        cleanup_skill(&name).await;
    }

    #[tokio::test]
    async fn test_create_duplicate_skill_error() {
        let name = unique_name("test_dup_skill");
        cleanup_skill(&name).await;

        let ctx = test_ctx();

        SkillManageTool
            .execute(
                &json!({"action": "create", "name": name, "content": "# First"}),
                &ctx,
            )
            .await
            .unwrap();

        let err = SkillManageTool
            .execute(
                &json!({"action": "create", "name": name, "content": "# Duplicate"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("already exists"));

        cleanup_skill(&name).await;
    }

    #[tokio::test]
    async fn test_read_nonexistent_skill_error() {
        let ctx = test_ctx();
        let err = SkillManageTool
            .execute(
                &json!({"action": "read", "name": "hakimi_nonexistent_skill_xyz"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[tokio::test]
    async fn test_delete_nonexistent_skill_error() {
        let ctx = test_ctx();
        let err = SkillManageTool
            .execute(
                &json!({"action": "delete", "name": "hakimi_nonexistent_skill_xyz"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[tokio::test]
    async fn test_update_nonexistent_skill_error() {
        let ctx = test_ctx();
        let err = SkillManageTool
            .execute(
                &json!({"action": "update", "name": "hakimi_nonexistent_skill_xyz", "content": "# New"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("not found"));
    }

    #[tokio::test]
    async fn test_invalid_skill_name() {
        let ctx = test_ctx();
        let err = SkillManageTool
            .execute(
                &json!({"action": "create", "name": "../traversal", "content": "# Bad"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("invalid skill name"));
    }

    #[tokio::test]
    async fn test_empty_skill_name() {
        let ctx = test_ctx();
        let err = SkillManageTool
            .execute(
                &json!({"action": "create", "name": "", "content": "# Empty"}),
                &ctx,
            )
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("invalid skill name"));
    }

    #[tokio::test]
    async fn test_missing_action_error() {
        let ctx = test_ctx();
        let err = SkillManageTool.execute(&json!({}), &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("action"));
    }

    #[tokio::test]
    async fn test_create_missing_name_error() {
        let ctx = test_ctx();
        let err = SkillManageTool
            .execute(&json!({"action": "create", "content": "# No name"}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("name"));
    }

    #[tokio::test]
    async fn test_create_missing_content_error() {
        let ctx = test_ctx();
        let err = SkillManageTool
            .execute(&json!({"action": "create", "name": "no_content"}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("content"));
    }

    #[tokio::test]
    async fn test_invalid_action_error() {
        let ctx = test_ctx();
        let err = SkillManageTool
            .execute(&json!({"action": "invalid"}), &ctx)
            .await
            .unwrap_err();
        assert!(format!("{err}").contains("invalid action"));
    }
}
