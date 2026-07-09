use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde::{Deserialize, Serialize};
use serde_json::{Value as JsonValue, json};
use tokio::fs;
use tracing::debug;

use crate::Tool;

/// Built-in tool for task/todo management.
pub struct TodoTool;

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TodoItem {
    id: String,
    content: String,
    status: String, // pending, in_progress, completed, cancelled
}

/// Get the todos directory path (~/.hakimi/todos/).
fn todos_dir() -> std::path::PathBuf {
    dirs::home_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(".hakimi")
        .join("todos")
}

/// Get the file path for a given session.
fn session_file(session_id: &str) -> std::path::PathBuf {
    todos_dir().join(format!("{session_id}.json"))
}

/// Load todos from disk.
async fn load_todos(session_id: &str) -> Result<Vec<TodoItem>> {
    let path = session_file(session_id);
    match fs::read_to_string(&path).await {
        Ok(data) => serde_json::from_str(&data)
            .map_err(|e| HakimiError::ToolSimple(format!("failed to parse todos file: {e}"))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(HakimiError::ToolSimple(format!("failed to read todos file: {e}"))),
    }
}

/// Save todos to disk.
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

#[async_trait]
impl Tool for TodoTool {
    fn name(&self) -> &str {
        "todo"
    }

    fn toolset(&self) -> &str {
        "productivity"
    }

    fn description(&self) -> &str {
        "Manage a session-scoped task/todo list. Actions: 'list' returns all todos, 'create' adds new todos, 'update' modifies existing todos by id, 'read' returns a specific todo by id."
    }

    fn emoji(&self) -> &str {
        "\u{2705}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "description": "Action to perform: 'list' returns all todos, 'create' adds new todos, 'update' modifies existing todos by id, 'read' returns a specific todo by id.",
                    "enum": ["read", "create", "update", "list"]
                },
                "todos": {
                    "type": "array",
                    "description": "Array of todo objects for 'create' and 'update' actions. Each object must have 'id' and 'content' (for create), and optionally 'status' (pending, in_progress, completed, cancelled).",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "string",
                                "description": "Unique identifier for the todo item."
                            },
                            "content": {
                                "type": "string",
                                "description": "Description of the task."
                            },
                            "status": {
                                "type": "string",
                                "description": "Status of the todo: pending, in_progress, completed, or cancelled.",
                                "enum": ["pending", "in_progress", "completed", "cancelled"]
                            }
                        },
                        "required": ["id"]
                    }
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::ToolSimple("missing required parameter: action".into()))?;

        let session_id = &ctx.session_id;

        debug!(action = %action, session_id = %session_id, "todo operation");

        match action {
            "list" => {
                let todos = load_todos(session_id).await?;
                if todos.is_empty() {
                    return Ok("No todos found for this session.".to_string());
                }
                let mut result = String::new();
                for todo in &todos {
                    let status_icon = match todo.status.as_str() {
                        "pending" => "\u{25cb}",
                        "in_progress" => "\u{25cf}",
                        "completed" => "\u{2713}",
                        "cancelled" => "\u{2717}",
                        _ => "?",
                    };
                    result.push_str(&format!(
                        "{} [{}] {} - {}\n",
                        status_icon, todo.id, todo.content, todo.status
                    ));
                }
                Ok(result)
            }
            "read" => {
                let todos = args
                    .get("todos")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        HakimiError::ToolSimple(
                            "'todos' array with 'id' is required for 'read' action".into(),
                        )
                    })?;
                let target_id = todos
                    .first()
                    .and_then(|t| t.get("id"))
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        HakimiError::ToolSimple(
                            "'id' is required in the first todo object for 'read'".into(),
                        )
                    })?;

                let all_todos = load_todos(session_id).await?;
                let todo = all_todos
                    .iter()
                    .find(|t| t.id == target_id)
                    .ok_or_else(|| {
                        HakimiError::ToolSimple(format!("todo with id '{}' not found", target_id))
                    })?;

                Ok(serde_json::to_string_pretty(todo)
                    .map_err(|e| HakimiError::ToolSimple(format!("failed to serialize todo: {e}")))?)
            }
            "create" => {
                let new_todos = args
                    .get("todos")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        HakimiError::ToolSimple("'todos' array is required for 'create' action".into())
                    })?;

                let mut existing = load_todos(session_id).await?;
                let mut created = 0;

                for item in new_todos {
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| HakimiError::ToolSimple("each todo must have an 'id'".into()))?
                        .to_string();

                    let content = item
                        .get("content")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let status = item
                        .get("status")
                        .and_then(|v| v.as_str())
                        .unwrap_or("pending")
                        .to_string();

                    // Check for duplicate id
                    if existing.iter().any(|t| t.id == id) {
                        return Err(HakimiError::ToolSimple(format!(
                            "todo with id '{}' already exists. Use 'update' to modify it.",
                            id
                        )));
                    }

                    existing.push(TodoItem {
                        id,
                        content,
                        status,
                    });
                    created += 1;
                }

                save_todos(session_id, &existing).await?;
                Ok(format!(
                    "Created {} todo(s). Total: {}.",
                    created,
                    existing.len()
                ))
            }
            "update" => {
                let update_todos =
                    args.get("todos")
                        .and_then(|v| v.as_array())
                        .ok_or_else(|| {
                            HakimiError::ToolSimple(
                                "'todos' array is required for 'update' action".into(),
                            )
                        })?;

                let mut existing = load_todos(session_id).await?;
                let mut updated = 0;

                for item in update_todos {
                    let id = item
                        .get("id")
                        .and_then(|v| v.as_str())
                        .ok_or_else(|| HakimiError::ToolSimple("each todo must have an 'id'".into()))?;

                    let todo = existing.iter_mut().find(|t| t.id == id).ok_or_else(|| {
                        HakimiError::ToolSimple(format!("todo with id '{}' not found", id))
                    })?;

                    if let Some(content) = item.get("content").and_then(|v| v.as_str()) {
                        todo.content = content.to_string();
                    }
                    if let Some(status) = item.get("status").and_then(|v| v.as_str()) {
                        todo.status = status.to_string();
                    }
                    updated += 1;
                }

                save_todos(session_id, &existing).await?;
                Ok(format!("Updated {} todo(s).", updated))
            }
            _ => Err(HakimiError::ToolSimple(format!(
                "invalid action '{}'. Must be 'read', 'create', 'update', or 'list'.",
                action
            ))),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolContext;

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

    /// Clean up test todo files
    async fn cleanup(session_id: &str) {
        let _ = fs::remove_file(session_file(session_id)).await;
    }

    #[test]
    fn test_schema_is_valid() {
        let tool = TodoTool;
        let schema = tool.schema();
        assert_eq!(schema["type"], "object");
        assert!(schema["properties"].is_object());
        assert!(schema["properties"]["action"].is_object());
        assert!(schema["properties"]["todos"].is_object());
        let required = schema["required"].as_array().unwrap();
        assert!(required.iter().any(|v| v == "action"));
    }

    #[test]
    fn test_tool_properties() {
        let tool = TodoTool;
        assert_eq!(tool.name(), "todo");
        assert_eq!(tool.toolset(), "productivity");
        assert!(tool.check_available());
        assert_eq!(tool.emoji(), "✅");
    }

    #[tokio::test]
    async fn test_create_todo() {
        let sid = "hakimi_test_todo_create";
        cleanup(sid).await;

        let ctx = test_ctx(sid);
        let args = json!({
            "action": "create",
            "todos": [
                {"id": "task-1", "content": "Buy groceries", "status": "pending"},
                {"id": "task-2", "content": "Write tests"}
            ]
        });

        let result = TodoTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("Created 2 todo(s)"));

        cleanup(sid).await;
    }

    #[tokio::test]
    async fn test_list_todos() {
        let sid = "hakimi_test_todo_list";
        cleanup(sid).await;

        let ctx = test_ctx(sid);

        // Create first
        let create_args = json!({
            "action": "create",
            "todos": [{"id": "t1", "content": "Item one"}]
        });
        TodoTool.execute(&create_args, &ctx).await.unwrap();

        // List
        let list_args = json!({"action": "list"});
        let result = TodoTool.execute(&list_args, &ctx).await.unwrap();
        assert!(result.contains("Item one"));
        assert!(result.contains("[t1]"));

        cleanup(sid).await;
    }

    #[tokio::test]
    async fn test_list_empty_todos() {
        let sid = "hakimi_test_todo_list_empty";
        cleanup(sid).await;

        let ctx = test_ctx(sid);
        let args = json!({"action": "list"});
        let result = TodoTool.execute(&args, &ctx).await.unwrap();
        assert!(result.contains("No todos"));

        cleanup(sid).await;
    }

    #[tokio::test]
    async fn test_update_todo_status() {
        let sid = "hakimi_test_todo_update";
        cleanup(sid).await;

        let ctx = test_ctx(sid);

        // Create
        let create_args = json!({
            "action": "create",
            "todos": [{"id": "u1", "content": "Task to update"}]
        });
        TodoTool.execute(&create_args, &ctx).await.unwrap();

        // Update
        let update_args = json!({
            "action": "update",
            "todos": [{"id": "u1", "status": "completed"}]
        });
        let result = TodoTool.execute(&update_args, &ctx).await.unwrap();
        assert!(result.contains("Updated 1"));

        // Verify via list
        let list_args = json!({"action": "list"});
        let list_result = TodoTool.execute(&list_args, &ctx).await.unwrap();
        assert!(list_result.contains("completed"));

        cleanup(sid).await;
    }

    #[tokio::test]
    async fn test_read_todo() {
        let sid = "hakimi_test_todo_read";
        cleanup(sid).await;

        let ctx = test_ctx(sid);

        // Create
        let create_args = json!({
            "action": "create",
            "todos": [{"id": "r1", "content": "Readable task"}]
        });
        TodoTool.execute(&create_args, &ctx).await.unwrap();

        // Read
        let read_args = json!({
            "action": "read",
            "todos": [{"id": "r1"}]
        });
        let result = TodoTool.execute(&read_args, &ctx).await.unwrap();
        assert!(result.contains("Readable task"));
        assert!(result.contains("r1"));

        cleanup(sid).await;
    }

    #[tokio::test]
    async fn test_create_duplicate_todo_error() {
        let sid = "hakimi_test_todo_dup";
        cleanup(sid).await;

        let ctx = test_ctx(sid);

        let create_args = json!({
            "action": "create",
            "todos": [{"id": "d1", "content": "First"}]
        });
        TodoTool.execute(&create_args, &ctx).await.unwrap();

        let dup_args = json!({
            "action": "create",
            "todos": [{"id": "d1", "content": "Duplicate"}]
        });
        let err = TodoTool.execute(&dup_args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("already exists"));

        cleanup(sid).await;
    }

    #[tokio::test]
    async fn test_update_nonexistent_todo_error() {
        let sid = "hakimi_test_todo_update_missing";
        cleanup(sid).await;

        let ctx = test_ctx(sid);
        let args = json!({
            "action": "update",
            "todos": [{"id": "nope", "status": "completed"}]
        });
        let err = TodoTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("not found"));

        cleanup(sid).await;
    }

    #[tokio::test]
    async fn test_missing_action_error() {
        let ctx = test_ctx("hakimi_test_todo_no_action");
        let args = json!({});
        let err = TodoTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("action"));
    }

    #[tokio::test]
    async fn test_invalid_action_error() {
        let ctx = test_ctx("hakimi_test_todo_bad_action");
        let args = json!({"action": "invalid"});
        let err = TodoTool.execute(&args, &ctx).await.unwrap_err();
        assert!(format!("{err}").contains("invalid action"));
    }
}
