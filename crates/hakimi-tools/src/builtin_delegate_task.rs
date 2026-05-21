use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{json, Value as JsonValue};
use tracing::debug;

use crate::Tool;

/// Built-in meta-tool for delegating sub-tasks to child agents.
/// Currently a placeholder that returns a structured execution plan.
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
        "Delegate a sub-task to a child agent. Provides a structured plan for task decomposition and parallel execution. Currently returns an execution plan; future versions will spawn child agents."
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
                    "description": "Optional list of toolset names the child agent should have access to (e.g., ['file', 'shell', 'web']). If omitted, uses default toolsets."
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
            .unwrap_or_else(|| vec!["file".into(), "shell".into(), "web".into()]);

        debug!(
            goal = %goal,
            context = %context,
            toolsets = ?toolsets,
            session_id = %ctx.session_id,
            "delegating task"
        );

        // Generate a structured execution plan
        let plan = json!({
            "status": "planned",
            "message": "Task delegation is not yet fully implemented. This is a structured plan for future child-agent execution.",
            "task": {
                "goal": goal,
                "context": if context.is_empty() { None } else { Some(context) },
                "parent_session": ctx.session_id,
                "assigned_toolsets": toolsets,
            },
            "execution_plan": {
                "steps": [
                    {
                        "step": 1,
                        "action": "analyze",
                        "description": "Analyze the goal and decompose into atomic sub-tasks"
                    },
                    {
                        "step": 2,
                        "action": "prepare",
                        "description": "Set up a child agent session with the assigned toolsets"
                    },
                    {
                        "step": 3,
                        "action": "execute",
                        "description": "Run the child agent with the goal and context as system instructions"
                    },
                    {
                        "step": 4,
                        "action": "collect",
                        "description": "Collect the child agent's result and report back to the parent session"
                    }
                ]
            },
            "limitations": [
                "Child agent spawning is not yet implemented",
                "This tool currently returns a plan only",
                "Future versions will support actual sub-task execution"
            ]
        });

        serde_json::to_string_pretty(&plan).map_err(|e| {
            HakimiError::Tool(format!("failed to serialize plan: {e}"))
        })
    }
}
