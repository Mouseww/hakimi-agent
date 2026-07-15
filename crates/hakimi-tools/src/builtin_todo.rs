use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Maximum content length per todo item (防止单个任务过大)
const MAX_TODO_CONTENT_CHARS: usize = 4000;
/// Maximum number of todo items (防止列表无限增长)
const MAX_TODO_ITEMS: usize = 256;

/// Built-in tool for task/todo management (Hermes-aligned design).
pub struct TodoToolV2;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: String,
    pub content: String,
    #[serde(default = "default_status")]
    pub status: String, // pending, in_progress, completed, cancelled
}

fn default_status() -> String {
    "pending".to_string()
}

impl TodoItem {
    fn validate(mut self) -> Self {
        // Normalize status
        self.status = self.status.trim().to_lowercase();
        if !["pending", "in_progress", "completed", "cancelled"].contains(&self.status.as_str()) {
            self.status = "pending".to_string();
        }

        // Cap content length
        if self.content.len() > MAX_TODO_CONTENT_CHARS {
            self.content.truncate(MAX_TODO_CONTENT_CHARS - 14);
            self.content.push_str("… [truncated]");
        }

        self
    }
}

/// Get the todos directory path (~/.hakimi/todos/)
fn todos_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".hakimi")
        .join("todos")
}

/// Get the file path for a given session
fn session_file(session_id: &str) -> std::path::PathBuf {
    todos_dir().join(format!("{session_id}.json"))
}

/// Load todos from disk
async fn load_todos(session_id: &str) -> Result<Vec<TodoItem>> {
    let path = session_file(session_id);
    match fs::read_to_string(&path).await {
        Ok(data) => serde_json::from_str(&data)
            .map_err(|e| HakimiError::ToolSimple(format!("failed to parse todos file: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(HakimiError::ToolSimple(format!(
            "failed to read todos file: {e}"
        ))),
    }
}

/// Save todos to disk
async fn save_todos(session_id: &str, todos: &[TodoItem]) -> Result<()> {
    let dir = todos_dir();
    fs::create_dir_all(&dir)
        .await
        .map_err(|e| HakimiError::ToolSimple(format!("failed to create todos directory: {e}")))?;

    let path = session_file(session_id);
    let data = serde_json::to_string_pretty(todos)
        .map_err(|e| HakimiError::ToolSimple(format!("failed to serialize todos: {e}")))?;

    fs::write(&path, data)
        .await
        .map_err(|e| HakimiError::ToolSimple(format!("failed to write todos file: {e}")))?;

    Ok(())
}

/// Write todos (replace or merge mode)
async fn write_todos(
    session_id: &str,
    new_items: Vec<TodoItem>,
    merge: bool,
) -> Result<Vec<TodoItem>> {
    let mut items = if merge {
        load_todos(session_id).await?
    } else {
        Vec::new()
    };

    if merge {
        // Merge mode: update existing by id, append new ones
        let mut existing_map: std::collections::HashMap<String, TodoItem> = items
            .into_iter()
            .map(|item| (item.id.clone(), item))
            .collect();

        for item in new_items {
            let item = item.validate();
            existing_map.insert(item.id.clone(), item);
        }

        items = existing_map.into_values().collect();
    } else {
        // Replace mode: use new items entirely
        items = new_items.into_iter().map(|i| i.validate()).collect();
    }

    // Dedupe by id, keeping last occurrence
    let mut seen = std::collections::HashSet::new();
    items.retain(|item| seen.insert(item.id.clone()));

    // Cap total item count
    if items.len() > MAX_TODO_ITEMS {
        items.truncate(MAX_TODO_ITEMS);
    }

    save_todos(session_id, &items).await?;
    Ok(items)
}

/// Format todos as JSON result
fn format_result(items: &[TodoItem]) -> String {
    let pending = items.iter().filter(|i| i.status == "pending").count();
    let in_progress = items.iter().filter(|i| i.status == "in_progress").count();
    let completed = items.iter().filter(|i| i.status == "completed").count();
    let cancelled = items.iter().filter(|i| i.status == "cancelled").count();

    json!({
        "todos": items,
        "summary": {
            "total": items.len(),
            "pending": pending,
            "in_progress": in_progress,
            "completed": completed,
            "cancelled": cancelled,
        }
    })
    .to_string()
}

#[async_trait]
impl Tool for TodoToolV2 {
    fn name(&self) -> &str {
        "todo"
    }

    fn toolset(&self) -> &str {
        "productivity"
    }

    fn description(&self) -> &str {
        "Manage your task list for the current session. Use for complex tasks with 3+ steps or when the user provides multiple tasks. Call with no parameters to read the current list.\n\nWriting:\n- Provide 'todos' array to create/update items\n- merge=false (default): replace the entire list with a fresh plan\n- merge=true: update existing items by id, add any new ones\n\nEach item: {id: string, content: string, status: pending|in_progress|completed|cancelled}\nList order is priority. Only ONE item in_progress at a time.\nMark items completed immediately when done. If something fails, cancel it and add a revised item.\n\nAlways returns the full current list."
    }

    fn emoji(&self) -> &str {
        "✅"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "todos": {
                    "type": "array",
                    "description": "Task items to write. Omit to read current list.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Unique item identifier"
                            },
                            "content": {
                                "type": "string",
                                "description": "Task description"
                            },
                            "status": {
                                "type": "string",
                                "enum": ["pending", "in_progress", "completed", "cancelled"],
                                "description": "Current status"
                            }
                        },
                        "required": ["id", "content", "status"]
                    }
                },
                "merge": {
                    "type": "boolean",
                    "description": "true: update existing items by id, add new ones. false (default): replace the entire list.",
                    "default": false
                }
            }
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let session_id = &ctx.session_id;

        // Check if todos parameter is provided
        if let Some(todos_val) = args.get("todos") {
            // Write mode
            let todos_array = todos_val
                .as_array()
                .ok_or_else(|| HakimiError::ToolSimple("'todos' must be an array".to_string()))?;

            let new_items: Vec<TodoItem> = todos_array
                .iter()
                .filter_map(|v| serde_json::from_value(v.clone()).ok())
                .collect();

            if new_items.is_empty() {
                return Err(HakimiError::ToolSimple(
                    "no valid todo items provided".to_string(),
                ));
            }

            let merge = args.get("merge").and_then(|v| v.as_bool()).unwrap_or(false);

            let items = write_todos(session_id, new_items, merge).await?;
            debug!("wrote {} todos (merge={})", items.len(), merge);
            Ok(format_result(&items))
        } else {
            // Read mode
            let items = load_todos(session_id).await?;
            debug!("read {} todos", items.len());
            Ok(format_result(&items))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_ctx(session_id: &str) -> ToolContext {
        ToolContext {
            session_id: session_id.to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
            ..Default::default()
        }
    }

    async fn cleanup(session_id: &str) {
        let _ = fs::remove_file(session_file(session_id)).await;
    }

    #[tokio::test]
    async fn test_empty_read() {
        let tool = TodoToolV2;
        let session_id = "test_empty";
        cleanup(session_id).await;

        let result = tool
            .execute(&json!({}), &test_ctx(session_id))
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["todos"].as_array().unwrap().len(), 0);
        assert_eq!(parsed["summary"]["total"], 0);

        cleanup(session_id).await;
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let tool = TodoToolV2;
        let session_id = "test_write";
        cleanup(session_id).await;

        // Write todos
        let write_args = json!({
            "todos": [
                {"id": "1", "content": "Task 1", "status": "pending"},
                {"id": "2", "content": "Task 2", "status": "in_progress"}
            ]
        });

        let result = tool
            .execute(&write_args, &test_ctx(session_id))
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["todos"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["summary"]["pending"], 1);
        assert_eq!(parsed["summary"]["in_progress"], 1);

        // Read todos
        let result = tool
            .execute(&json!({}), &test_ctx(session_id))
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();
        assert_eq!(parsed["todos"].as_array().unwrap().len(), 2);

        cleanup(session_id).await;
    }

    #[tokio::test]
    async fn test_merge_mode() {
        let tool = TodoToolV2;
        let session_id = "test_merge";
        cleanup(session_id).await;

        // Initial write
        let write_args = json!({
            "todos": [
                {"id": "1", "content": "Task 1", "status": "pending"},
                {"id": "2", "content": "Task 2", "status": "pending"}
            ]
        });
        tool.execute(&write_args, &test_ctx(session_id))
            .await
            .unwrap();

        // Merge update
        let merge_args = json!({
            "todos": [
                {"id": "1", "content": "Task 1 Updated", "status": "completed"},
                {"id": "3", "content": "Task 3", "status": "pending"}
            ],
            "merge": true
        });

        let result = tool
            .execute(&merge_args, &test_ctx(session_id))
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();

        assert_eq!(parsed["todos"].as_array().unwrap().len(), 3);
        assert_eq!(parsed["summary"]["completed"], 1);

        cleanup(session_id).await;
    }

    #[tokio::test]
    async fn test_content_truncation() {
        let tool = TodoToolV2;
        let session_id = "test_truncate";
        cleanup(session_id).await;

        let long_content = "x".repeat(MAX_TODO_CONTENT_CHARS + 100);
        let write_args = json!({
            "todos": [
                {"id": "1", "content": long_content, "status": "pending"}
            ]
        });

        let result = tool
            .execute(&write_args, &test_ctx(session_id))
            .await
            .unwrap();
        let parsed: JsonValue = serde_json::from_str(&result).unwrap();
        let content = parsed["todos"][0]["content"].as_str().unwrap();

        assert!(content.len() <= MAX_TODO_CONTENT_CHARS);
        assert!(content.ends_with("… [truncated]"));

        cleanup(session_id).await;
    }
}
