use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tracing::{debug, warn};

use crate::Tool;

/// Built-in meta-tool for delegating sub-tasks to child agents.
///
/// When the parent agent's `ToolContext` includes a `DelegateExecutor`, this
/// tool spawns a real child agent to accomplish the goal. Otherwise, it returns
/// a structured plan describing what would happen.
pub struct DelegateTaskTool;

#[async_trait]
impl Tool for DelegateTaskTool {
    fn name(&self) -> &str {
        "delegate_task"
    }

    fn toolset(&self) -> &str {
        "meta"
    }

    fn description(&self) -> &str {
        "Delegate a sub-task to a child agent. The child agent runs independently with its own \
         conversation loop, tool access, and timeout. Use this to parallelize work or isolate \
         sub-tasks."
    }

    fn emoji(&self) -> &str {
        "\u{1f91d}"
    }

    fn schema(&self) -> JsonValue {
        json!({
                "type": "object",
                "properties": {
                    "goal": {
                        "type": "string",
                        "description": "The goal or objective for the sub-task. Should be a clear, actionable description of what the child agent should accomplish."
                    },
                    "context": {
                        "type": "string",
                        "description": "Optional additional context, constraints, or background information for the sub-task."
                    },
                "toolsets": {
                    "type": "array",
                    "items": {
                        "type": "string"
                    },
                    "description": "Optional list of toolset names the child agent should have access to (e.g., ['file', 'shell', 'web']). If omitted, the child agent gets access to all tools."
                },
                "enqueue": {
                    "type": "boolean",
                    "description": "If true, the task will be added to a background queue instead of executing immediately."
                }
            },
            "required": ["goal"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let goal = args
            .get("goal")
            .and_then(|v| v.as_str())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: goal".into()))?;

        let context = args.get("context").and_then(|v| v.as_str()).unwrap_or("");

        let toolsets: Vec<String> = args
            .get("toolsets")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(|s| s.to_string()))
                    .collect()
            })
            .unwrap_or_default();

        let enqueue = args
            .get("enqueue")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        debug!(
            goal = %goal,
            context = %context,
            toolsets = ?toolsets,
            enqueue = enqueue,
            session_id = %ctx.session_id,
            "delegating task"
        );

        // Attempt to use the delegate executor for real delegation.
        if let Some(ref executor) = ctx.delegate_executor {
            if enqueue {
                match executor.enqueue_task(goal, 1).await {
                    Ok(task_id) => {
                        let response = json!({
                            "status": "enqueued",
                            "goal": goal,
                            "task_id": task_id,
                            "parent_session": ctx.session_id,
                        });
                        return serde_json::to_string_pretty(&response).map_err(|e| {
                            HakimiError::Tool(format!("failed to serialize result: {e}"))
                        });
                    }
                    Err(e) => {
                        warn!(error = %e, "Failed to enqueue task");
                        let response = json!({
                            "status": "failed_to_enqueue",
                            "goal": goal,
                            "error": format!("{e}"),
                            "parent_session": ctx.session_id,
                        });
                        return serde_json::to_string_pretty(&response).map_err(|e| {
                            HakimiError::Tool(format!("failed to serialize error: {e}"))
                        });
                    }
                }
            }

            match executor.execute_delegation(goal, context, &toolsets).await {
                Ok(result) => {
                    let response = json!({
                        "status": "completed",
                        "goal": goal,
                        "result": result,
                        "parent_session": ctx.session_id,
                    });
                    return serde_json::to_string_pretty(&response).map_err(|e| {
                        HakimiError::Tool(format!("failed to serialize result: {e}"))
                    });
                }
                Err(e) => {
                    warn!(error = %e, "Child agent delegation failed");
                    let response = json!({
                        "status": "failed",
                        "goal": goal,
                        "error": format!("{e}"),
                        "parent_session": ctx.session_id,
                    });
                    return serde_json::to_string_pretty(&response)
                        .map_err(|e| HakimiError::Tool(format!("failed to serialize error: {e}")));
                }
            }
        }

        // Fallback: no delegate executor available (e.g., in tests or standalone tools).
        warn!("No delegate executor available, returning execution plan");
        let plan = json!({
            "status": "planned",
            "message": "No delegate executor is configured. This is a structured plan for the delegation.",
            "task": {
                "goal": goal,
                "context": if context.is_empty() { None } else { Some(context) },
                "parent_session": ctx.session_id,
                "assigned_toolsets": toolsets,
            },
        });

        serde_json::to_string_pretty(&plan)
            .map_err(|e| HakimiError::Tool(format!("failed to serialize plan: {e}")))
    }
}
