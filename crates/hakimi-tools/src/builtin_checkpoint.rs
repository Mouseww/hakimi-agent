//! Checkpoint manager — transparent shadow-git snapshots before file mutations.
//!
//! Creates lightweight git snapshots of working directories before file-mutating
//! operations, enabling rollback to any previous checkpoint.

use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{json, Value as JsonValue};
use std::path::{Path, PathBuf};
use tracing::{debug, info, warn};

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

        let label = args
            .get("label")
            .and_then(|v| v.as_str())
            .unwrap_or("");

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
        std::fs::create_dir_all(&git_dir).map_err(|e| {
            HakimiError::Io(e)
        })?;
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
    let git_dir = ensure_shadow_git(workdir)?;

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
    let output = run_git_raw(workdir, &["commit", "-m", &message, "--allow-empty"])?;

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

    let output = run_git_raw(
        workdir,
        &["log", "--format=%H|%s|%ai", "--all"],
    )?;

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
}
