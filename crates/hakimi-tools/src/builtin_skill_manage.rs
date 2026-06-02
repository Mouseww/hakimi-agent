use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool for managing skills (reusable prompt templates stored as markdown).
pub struct SkillManageTool;

/// Get the active runtime skills directory path.
fn skills_dir() -> std::path::PathBuf {
    hakimi_common::effective_hakimi_home().join("skills")
}

/// Find a skill file (SKILL.md in a dir, or name.md) by searching recursively.
async fn find_skill(name: &str) -> Option<std::path::PathBuf> {
    let dir = skills_dir();
    let mut dirs_to_visit = vec![dir.clone()];

    while let Some(current_dir) = dirs_to_visit.pop() {
        if let Ok(mut entries) = fs::read_dir(&current_dir).await {
            while let Ok(Some(entry)) = entries.next_entry().await {
                let path = entry.path();
                if path.is_dir() {
                    dirs_to_visit.push(path);
                    continue;
                }
                if path.is_file() {
                    // Check if it's `{name}.md` or `{name}/SKILL.md`
                    let file_name = path.file_name().unwrap_or_default().to_string_lossy();
                    if file_name == format!("{}.md", name) {
                        return Some(path);
                    }
                    if file_name == "SKILL.md"
                        && let Some(parent) = path.parent()
                        && parent.file_name().unwrap_or_default().to_string_lossy() == name
                    {
                        return Some(path);
                    }
                }
            }
        }
    }
    None
}

/// Helper to sanitize filenames
fn sanitize_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.contains('/')
        || name.contains('\\')
        || name.contains("..")
        || name.starts_with('.')
    {
        return Err(HakimiError::Tool(format!(
            "invalid skill name '{name}'. Names cannot be empty, contain path separators, '..' , or start with '.'"
        )));
    }
    Ok(())
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
        "Manage skills (create, patch, edit, delete, write_file, remove_file). Skills are your procedural memory."
    }

    fn emoji(&self) -> &str {
        "📚"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "patch", "edit", "delete", "write_file", "remove_file"]
                },
                "name": { "type": "string" },
                "category": { "type": "string" },
                "content": { "type": "string" },
                "old_string": { "type": "string" },
                "new_string": { "type": "string" },
                "replace_all": { "type": "boolean" },
                "file_path": { "type": "string" },
                "file_content": { "type": "string" },
                "absorbed_into": { "type": "string" }
            },
            "required": ["action", "name"]
        })
    }

    async fn execute(&self, args: &JsonValue, _ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing action".into()))?;
        let name = args
            .get("name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing name".into()))?;
        sanitize_name(name)?;

        debug!(action = %action, name = %name, "skill_manage operation");

        let dir = skills_dir();
        fs::create_dir_all(&dir)
            .await
            .map_err(|e| HakimiError::Tool(format!("failed to create skills directory: {e}")))?;

        match action {
            "create" => {
                if find_skill(name).await.is_some() {
                    return Err(HakimiError::Tool(format!(
                        "skill '{name}' already exists. Use patch or edit."
                    )));
                }
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing content".into()))?;

                let category = args.get("category").and_then(|v| v.as_str()).unwrap_or("");
                let skill_dir = if !category.is_empty() {
                    sanitize_name(category)?;
                    dir.join(category).join(name)
                } else {
                    dir.join(name)
                };

                fs::create_dir_all(&skill_dir)
                    .await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                let path = skill_dir.join("SKILL.md");
                fs::write(&path, content)
                    .await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;

                Ok(format!("Created skill '{name}' at {}", path.display()))
            }
            "edit" => {
                let path = find_skill(name)
                    .await
                    .ok_or_else(|| HakimiError::Tool(format!("skill '{name}' not found.")))?;
                let content = args
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing content".into()))?;
                fs::write(&path, content)
                    .await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Edited skill '{name}' ({})", path.display()))
            }
            "patch" => {
                let path = find_skill(name)
                    .await
                    .ok_or_else(|| HakimiError::Tool(format!("skill '{name}' not found.")))?;

                // Optional sub-file patching
                let target_path =
                    if let Some(sub_path) = args.get("file_path").and_then(|v| v.as_str()) {
                        if sub_path.contains("..") {
                            return Err(HakimiError::Tool("invalid file_path".into()));
                        }
                        path.parent().unwrap().join(sub_path)
                    } else {
                        path.clone()
                    };

                let old_string = args
                    .get("old_string")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing old_string".into()))?;
                let new_string = args
                    .get("new_string")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let replace_all = args
                    .get("replace_all")
                    .and_then(|v| v.as_bool())
                    .unwrap_or(false);

                let content = fs::read_to_string(&target_path)
                    .await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                let matches = content.matches(old_string).count();
                if matches == 0 {
                    return Err(HakimiError::Tool("old_string not found in file".into()));
                }
                if matches > 1 && !replace_all {
                    return Err(HakimiError::Tool(format!(
                        "old_string matched {} times. Be more specific or use replace_all=true.",
                        matches
                    )));
                }

                let new_content = if replace_all {
                    content.replace(old_string, new_string)
                } else {
                    content.replacen(old_string, new_string, 1)
                };

                fs::write(&target_path, new_content)
                    .await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Patched {} correctly.", target_path.display()))
            }
            "delete" => {
                let path = find_skill(name)
                    .await
                    .ok_or_else(|| HakimiError::Tool(format!("skill '{name}' not found.")))?;
                // If it's a directory (i.e. we found SKILL.md), delete the whole directory
                if path.file_name().unwrap_or_default() == "SKILL.md" {
                    fs::remove_dir_all(path.parent().unwrap())
                        .await
                        .map_err(|e| HakimiError::Tool(e.to_string()))?;
                } else {
                    fs::remove_file(&path)
                        .await
                        .map_err(|e| HakimiError::Tool(e.to_string()))?;
                }

                let absorbed = args
                    .get("absorbed_into")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                if absorbed.is_empty() {
                    Ok(format!("Deleted skill '{name}'."))
                } else {
                    Ok(format!(
                        "Deleted skill '{name}' (absorbed into '{absorbed}')."
                    ))
                }
            }
            "write_file" => {
                let path = find_skill(name)
                    .await
                    .ok_or_else(|| HakimiError::Tool(format!("skill '{name}' not found.")))?;
                let sub_path = args
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing file_path".into()))?;
                let file_content = args
                    .get("file_content")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing file_content".into()))?;

                if sub_path.contains("..") {
                    return Err(HakimiError::Tool("invalid file_path".into()));
                }
                let parent_dir = path.parent().unwrap();
                let target_path = parent_dir.join(sub_path);

                if let Some(p) = target_path.parent() {
                    fs::create_dir_all(p)
                        .await
                        .map_err(|e| HakimiError::Tool(e.to_string()))?;
                }

                fs::write(&target_path, file_content)
                    .await
                    .map_err(|e| HakimiError::Tool(e.to_string()))?;
                Ok(format!("Wrote file {} in skill '{name}'", sub_path))
            }
            "remove_file" => {
                let path = find_skill(name)
                    .await
                    .ok_or_else(|| HakimiError::Tool(format!("skill '{name}' not found.")))?;
                let sub_path = args
                    .get("file_path")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| HakimiError::Tool("missing file_path".into()))?;

                if sub_path.contains("..") {
                    return Err(HakimiError::Tool("invalid file_path".into()));
                }
                let target_path = path.parent().unwrap().join(sub_path);

                if target_path.exists() {
                    fs::remove_file(&target_path)
                        .await
                        .map_err(|e| HakimiError::Tool(e.to_string()))?;
                    Ok(format!("Removed file {} from skill '{name}'", sub_path))
                } else {
                    Err(HakimiError::Tool(format!(
                        "File {} does not exist",
                        sub_path
                    )))
                }
            }
            _ => Err(HakimiError::Tool(format!("invalid action '{action}'"))),
        }
    }
}
