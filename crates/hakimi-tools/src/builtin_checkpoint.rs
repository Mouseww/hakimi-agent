//! Checkpoint manager — transparent shadow-git snapshots before file mutations.
//!
//! Creates lightweight git snapshots of working directories before file-mutating
//! operations, enabling rollback to any previous checkpoint.

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use std::path::{Path, PathBuf};
use tracing::{info, warn};

use crate::Tool;

/// Shadow git store directory name.
const SHADOW_GIT_DIR: &str = ".hakimi-checkpoints";

/// Built-in tool for creating and managing filesystem checkpoints.
pub struct CheckpointTool;

#[async_trait]
impl Tool for CheckpointTool {
    fn name(&self) -> &str {
        "checkpoint"
    }

    fn toolset(&self) -> &str {
        "file"
    }

    fn description(&self) -> &str {
        "Create and manage filesystem checkpoints. Creates shadow-git snapshots \
         of the working directory before file mutations. Supports rollback to any \
         previous checkpoint."
    }

    fn emoji(&self) -> &str {
        "\u{1f3c3}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["create", "list", "rollback", "diff"],
                    "description": "Action to perform: create a checkpoint, list checkpoints, rollback to a checkpoint, or show diff since a checkpoint."
                },
                "checkpoint_id": {
                    "type": "string",
                    "description": "Checkpoint ID (required for rollback and diff actions)."
                },
                "label": {
                    "type": "string",
                    "description": "Optional label for the checkpoint (for create action)."
                }
            },
            "required": ["action"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: action".into()))?;

        let checkpoint_id = args
            .get("checkpoint_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");

        let label = args.get("label").and_then(|v| v.as_str()).unwrap_or("");

        let workdir = Path::new(&ctx.workdir);

        match action {
            "create" => create_checkpoint(workdir, label).await,
            "list" => list_checkpoints(workdir).await,
            "rollback" => {
                if checkpoint_id.is_empty() {
                    return Err(HakimiError::Tool(
                        "checkpoint_id is required for rollback action".into(),
                    ));
                }
                rollback_checkpoint(workdir, checkpoint_id).await
            }
            "diff" => {
                if checkpoint_id.is_empty() {
                    return Err(HakimiError::Tool(
                        "checkpoint_id is required for diff action".into(),
                    ));
                }
                diff_checkpoint(workdir, checkpoint_id).await
            }
            _ => Err(HakimiError::Tool(format!(
                "Unknown checkpoint action: '{action}'. Valid actions: create, list, rollback, diff"
            ))),
        }
    }
}

/// Initialize the shadow git store if it doesn't exist.
fn ensure_shadow_git(workdir: &Path) -> Result<PathBuf> {
    let git_dir = workdir.join(SHADOW_GIT_DIR);
    if !git_dir.join("HEAD").exists() {
        std::fs::create_dir_all(&git_dir).map_err(HakimiError::Io)?;
        // Initialize a bare-ish git repo.
        run_git(workdir, &["init", "--bare", &git_dir.to_string_lossy()])?;
        // Configure the shadow git repo.
        run_git(&git_dir, &["config", "user.email", "hakimi@checkpoint"])?;
        run_git(&git_dir, &["config", "user.name", "Hakimi Checkpoint"])?;
    }
    Ok(git_dir)
}

/// Create a new checkpoint.
async fn create_checkpoint(workdir: &Path, label: &str) -> Result<String> {
    let _git_dir = ensure_shadow_git(workdir)?;

    // Add all files in the working directory.
    run_git(workdir, &["add", "-A"])?;

    // Create a commit with a descriptive message.
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string();
    let message = if label.is_empty() {
        format!("checkpoint: {timestamp}")
    } else {
        format!("checkpoint: {label} ({timestamp})")
    };

    // Commit to the shadow git store.
    let _output = run_git_raw(workdir, &["commit", "-m", &message, "--allow-empty"])?;

    // Extract the commit hash.
    let hash = run_git_raw(workdir, &["rev-parse", "HEAD"])?;
    let hash = hash.trim();

    info!(hash = %hash, label = %label, "Checkpoint created");

    Ok(json!({
        "status": "created",
        "checkpoint_id": hash,
        "label": label,
        "message": message,
        "timestamp": timestamp
    })
    .to_string())
}

/// List all checkpoints.
async fn list_checkpoints(workdir: &Path) -> Result<String> {
    let git_dir = workdir.join(SHADOW_GIT_DIR);
    if !git_dir.join("HEAD").exists() {
        return Ok(json!({
            "checkpoints": [],
            "message": "No checkpoints exist yet. Use action='create' to create one."
        })
        .to_string());
    }

    let output = run_git_raw(workdir, &["log", "--format=%H|%s|%ai", "--all"])?;

    let checkpoints: Vec<JsonValue> = output
        .lines()
        .filter(|line| !line.is_empty())
        .filter_map(|line| {
            let parts: Vec<&str> = line.splitn(3, '|').collect();
            if parts.len() >= 3 {
                Some(json!({
                    "id": parts[0],
                    "message": parts[1],
                    "timestamp": parts[2]
                }))
            } else {
                None
            }
        })
        .collect();

    Ok(json!({
        "checkpoints": checkpoints,
        "count": checkpoints.len()
    })
    .to_string())
}

/// Rollback to a specific checkpoint.
async fn rollback_checkpoint(workdir: &Path, checkpoint_id: &str) -> Result<String> {
    let git_dir = workdir.join(SHADOW_GIT_DIR);
    if !git_dir.join("HEAD").exists() {
        return Err(HakimiError::Tool(
            "No checkpoints exist. Cannot rollback.".into(),
        ));
    }

    // Reset the working directory to the checkpoint.
    run_git(workdir, &["checkout", checkpoint_id, "--", "."])?;

    info!(checkpoint_id = %checkpoint_id, "Rolled back to checkpoint");

    Ok(json!({
        "status": "rolled_back",
        "checkpoint_id": checkpoint_id,
        "message": format!("Successfully rolled back to checkpoint {}", &checkpoint_id[..8.min(checkpoint_id.len())])
    })
    .to_string())
}

/// Show diff since a checkpoint.
async fn diff_checkpoint(workdir: &Path, checkpoint_id: &str) -> Result<String> {
    let git_dir = workdir.join(SHADOW_GIT_DIR);
    if !git_dir.join("HEAD").exists() {
        return Err(HakimiError::Tool(
            "No checkpoints exist. Cannot diff.".into(),
        ));
    }

    let output = run_git_raw(workdir, &["diff", checkpoint_id])?;

    Ok(json!({
        "checkpoint_id": checkpoint_id,
        "diff": output,
        "has_changes": !output.trim().is_empty()
    })
    .to_string())
}

/// Run a git command in the given directory.
fn run_git(workdir: &Path, args: &[&str]) -> Result<()> {
    let output = std::process::Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()
        .map_err(|e| HakimiError::Other(format!("Failed to run git: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        // Don't fail on "nothing to commit" errors.
        if stderr.contains("nothing to commit") || stderr.contains("no changes added") {
            return Ok(());
        }
        warn!(args = ?args, stderr = %stderr, "Git command failed");
    }
    Ok(())
}

/// Run a git command and return stdout as a string.
fn run_git_raw(workdir: &Path, args: &[&str]) -> Result<String> {
    let output = std::process::Command::new("git")
        .current_dir(workdir)
        .args(args)
        .output()
        .map_err(|e| HakimiError::Other(format!("Failed to run git: {e}")))?;

    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        if stderr.contains("does not have any commits") || stderr.contains("unknown revision") {
            return Ok(String::new());
        }
        warn!(args = ?args, stderr = %stderr, "Git command failed");
    }
    Ok(stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_metadata() {
        let tool = CheckpointTool;
        assert_eq!(tool.name(), "checkpoint");
        assert_eq!(tool.toolset(), "file");
        assert!(!tool.description().is_empty());
    }

    #[test]
    fn test_schema_required_fields() {
        let tool = CheckpointTool;
        let schema = tool.schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.contains(&json!("action")));
    }

    #[tokio::test]
    async fn test_invalid_action() {
        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({"action": "invalid"}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_rollback_missing_id() {
        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({"action": "rollback"}), &ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_diff_missing_id() {
        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({"action": "diff"}), &ctx).await;
        assert!(result.is_err());
    }

    #[test]
    fn schema_has_valid_actions_enum() {
        let tool = CheckpointTool;
        let schema = tool.schema();
        let actions = schema["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum should be an array");
        let action_strs: Vec<&str> = actions.iter().map(|v| v.as_str().unwrap()).collect();
        assert!(
            action_strs.contains(&"create"),
            "enum must contain 'create'"
        );
        assert!(action_strs.contains(&"list"), "enum must contain 'list'");
        assert!(
            action_strs.contains(&"rollback"),
            "enum must contain 'rollback'"
        );
        assert!(action_strs.contains(&"diff"), "enum must contain 'diff'");
    }

    #[test]
    fn test_toolset_is_file() {
        let tool = CheckpointTool;
        assert_eq!(tool.toolset(), "file");
    }

    #[test]
    fn test_emoji_not_empty() {
        let tool = CheckpointTool;
        assert!(!tool.emoji().is_empty());
    }

    #[tokio::test]
    async fn test_create_checkpoint_in_tempdir() {
        let tmp = tempfile::tempdir().unwrap();
        // The checkpoint tool runs git add/commit in workdir, so we need a real repo.
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        // Seed the temp dir with a file so there's something to commit.
        std::fs::write(tmp.path().join("hello.txt"), "checkpoint content").unwrap();

        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({"action": "create"}), &ctx).await;
        assert!(result.is_ok(), "create should succeed: {:?}", result.err());
        let body: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(body["status"], "created");
        assert!(!body["checkpoint_id"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_list_checkpoints_after_create() {
        let tmp = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::fs::write(tmp.path().join("file.txt"), "data").unwrap();

        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };

        // Create a checkpoint.
        let create_res = tool.execute(&json!({"action": "create"}), &ctx).await;
        assert!(create_res.is_ok(), "create failed: {:?}", create_res.err());

        // List checkpoints.
        let list_res = tool.execute(&json!({"action": "list"}), &ctx).await;
        assert!(list_res.is_ok(), "list failed: {:?}", list_res.err());
        let body: serde_json::Value = serde_json::from_str(&list_res.unwrap()).unwrap();
        let count = body["count"].as_u64().expect("count should be a number");
        assert!(count >= 1, "expected at least 1 checkpoint, got {}", count);
    }

    #[tokio::test]
    async fn test_create_checkpoint_with_label() {
        let tmp = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::fs::write(tmp.path().join("labeled.txt"), "labeled content").unwrap();

        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool
            .execute(
                &json!({"action": "create", "label": "before-refactor"}),
                &ctx,
            )
            .await;
        assert!(
            result.is_ok(),
            "create with label failed: {:?}",
            result.err()
        );
        let body: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        assert_eq!(body["label"], "before-refactor");
        let msg = body["message"].as_str().unwrap();
        assert!(
            msg.contains("before-refactor"),
            "commit message should contain the label, got: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_missing_action_fails() {
        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: "/tmp".to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool.execute(&json!({}), &ctx).await;
        assert!(result.is_err(), "missing action should fail");
    }

    #[tokio::test]
    async fn test_list_checkpoints_empty_repo() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };
        // No shadow git dir exists — list should return empty
        let result = tool.execute(&json!({"action": "list"}), &ctx).await;
        assert!(
            result.is_ok(),
            "list on empty dir should succeed: {:?}",
            result.err()
        );
        let body: serde_json::Value = serde_json::from_str(&result.unwrap()).unwrap();
        let checkpoints = body["checkpoints"].as_array().unwrap();
        assert!(checkpoints.is_empty(), "should have no checkpoints");
    }

    #[tokio::test]
    async fn test_rollback_with_no_checkpoints() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };
        // No shadow git dir — rollback should fail
        let result = tool
            .execute(
                &json!({"action": "rollback", "checkpoint_id": "abc123"}),
                &ctx,
            )
            .await;
        assert!(result.is_err(), "rollback with no checkpoints should fail");
    }

    #[tokio::test]
    async fn test_diff_with_no_checkpoints() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };
        // No shadow git dir — diff should fail
        let result = tool
            .execute(&json!({"action": "diff", "checkpoint_id": "abc123"}), &ctx)
            .await;
        assert!(result.is_err(), "diff with no checkpoints should fail");
    }

    #[tokio::test]
    async fn test_create_multiple_checkpoints_and_list() {
        let tmp = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::fs::write(tmp.path().join("file.txt"), "v1").unwrap();

        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };

        // Create first checkpoint
        let r1 = tool
            .execute(&json!({"action": "create", "label": "v1"}), &ctx)
            .await;
        assert!(r1.is_ok(), "first create failed: {:?}", r1.err());

        // Modify file and create second checkpoint
        std::fs::write(tmp.path().join("file.txt"), "v2").unwrap();
        let r2 = tool
            .execute(&json!({"action": "create", "label": "v2"}), &ctx)
            .await;
        assert!(r2.is_ok(), "second create failed: {:?}", r2.err());

        // List should show 2 checkpoints
        let list_res = tool.execute(&json!({"action": "list"}), &ctx).await;
        assert!(list_res.is_ok(), "list failed: {:?}", list_res.err());
        let body: serde_json::Value = serde_json::from_str(&list_res.unwrap()).unwrap();
        let count = body["count"].as_u64().unwrap();
        assert!(count >= 2, "expected at least 2 checkpoints, got {}", count);

        // Verify both labels appear in checkpoint messages
        let checkpoints = body["checkpoints"].as_array().unwrap();
        let messages: Vec<&str> = checkpoints
            .iter()
            .map(|cp| cp["message"].as_str().unwrap())
            .collect();
        assert!(
            messages.iter().any(|m| m.contains("v1")),
            "should have v1 checkpoint"
        );
        assert!(
            messages.iter().any(|m| m.contains("v2")),
            "should have v2 checkpoint"
        );
    }

    #[tokio::test]
    async fn test_create_and_diff_checkpoint() {
        let tmp = tempfile::tempdir().unwrap();
        std::process::Command::new("git")
            .args(["init"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(tmp.path())
            .output()
            .unwrap();
        std::fs::write(tmp.path().join("file.txt"), "original").unwrap();

        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };

        // Create checkpoint
        let create_res = tool
            .execute(&json!({"action": "create", "label": "baseline"}), &ctx)
            .await
            .unwrap();
        let create_body: serde_json::Value = serde_json::from_str(&create_res).unwrap();
        let cp_id = create_body["checkpoint_id"].as_str().unwrap().to_string();

        // Modify file
        std::fs::write(tmp.path().join("file.txt"), "modified").unwrap();

        // Diff against the checkpoint
        let diff_res = tool
            .execute(&json!({"action": "diff", "checkpoint_id": cp_id}), &ctx)
            .await;
        assert!(
            diff_res.is_ok(),
            "diff should succeed: {:?}",
            diff_res.err()
        );
        let diff_body: serde_json::Value = serde_json::from_str(&diff_res.unwrap()).unwrap();
        assert_eq!(diff_body["has_changes"], true, "should detect changes");
        let diff_text = diff_body["diff"].as_str().unwrap();
        assert!(
            diff_text.contains("original"),
            "diff should mention original content"
        );
        assert!(
            diff_text.contains("modified"),
            "diff should mention modified content"
        );
    }

    #[tokio::test]
    async fn test_schema_has_action_enum() {
        let tool = CheckpointTool;
        let schema = tool.schema();
        let action_prop = &schema["properties"]["action"];
        assert_eq!(action_prop["type"], "string");
        assert!(!action_prop["description"].as_str().unwrap().is_empty());
    }

    #[tokio::test]
    async fn test_schema_required_field_is_action() {
        let tmp = tempfile::tempdir().unwrap();
        let tool = CheckpointTool;
        let ctx = ToolContext {
            session_id: "test".to_string(),
            user_id: None,
            task_id: None,
            workdir: tmp.path().to_string_lossy().to_string(),
            model: None,
            delegate_executor: None,
        };
        let result = tool
            .execute(&json!({ "action": "unknown_action" }), &ctx)
            .await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("Unknown checkpoint action"));
    }

    #[tokio::test]
    async fn test_tool_name_and_toolset() {
        let tool = CheckpointTool;
        assert_eq!(tool.name(), "checkpoint");
        assert_eq!(tool.toolset(), "file");
        assert!(!tool.description().is_empty());
        assert!(!tool.emoji().is_empty());
    }
}
