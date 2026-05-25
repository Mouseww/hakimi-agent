use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, ToolContext};
use serde_json::{Value as JsonValue, json};
use tracing::info;

use crate::Tool;

/// 工具：委派任务给子代理
pub struct DelegateTaskTool;

#[async_trait]
impl Tool for DelegateTaskTool {
    fn name(&self) -> &str {
        "delegate_task"
    }

    fn toolset(&self) -> &str {
        "core"
    }

    fn description(&self) -> &str {
        "Delegate a specific task to a sub-agent. This spawns a new agent instance to handle the task in isolation."
    }

    fn emoji(&self) -> &str {
        "🤝"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "goal": {
                    "type": "string",
                    "description": "What the subagent should accomplish. Required if 'tasks' is not provided."
                },
                "context": {
                    "type": "string",
                    "description": "Additional context or constraints for the single task."
                },
                "toolsets": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of toolsets the sub-agent should have access to."
                },
                "tasks": {
                    "type": "array",
                    "description": "Batch mode: array of tasks to run in parallel. When provided, top-level goal/context/toolsets are ignored.",
                    "items": {
                        "type": "object",
                        "properties": {
                            "goal": { "type": "string" },
                            "context": { "type": "string" },
                            "toolsets": {
                                "type": "array",
                                "items": { "type": "string" }
                            }
                        },
                        "required": ["goal"]
                    }
                }
            }
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let mut batch_tasks = Vec::new();

        if let Some(tasks_arr) = args.get("tasks").and_then(|v| v.as_array()) {
            for task_obj in tasks_arr {
                let goal = task_obj
                    .get("goal")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                if goal.is_empty() {
                    return Err(HakimiError::Tool(
                        "Each item in 'tasks' must have a 'goal'".into(),
                    ));
                }
                let context = task_obj
                    .get("context")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                let toolsets: Vec<String> = task_obj
                    .get("toolsets")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|i| i.as_str().map(|s| s.to_string()))
                            .collect()
                    })
                    .unwrap_or_default();
                batch_tasks.push((goal, context, toolsets));
            }
        } else {
            let goal = args
                .get("goal")
                .and_then(|v| v.as_str())
                .ok_or_else(|| HakimiError::Tool("missing 'goal' or 'tasks' argument".into()))?
                .to_string();
            let context = args
                .get("context")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let toolsets: Vec<String> = args
                .get("toolsets")
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|i| i.as_str().map(|s| s.to_string()))
                        .collect()
                })
                .unwrap_or_default();
            batch_tasks.push((goal, context, toolsets));
        }

        info!(
            task_count = batch_tasks.len(),
            "Delegating task(s) via ToolContext"
        );

        if let Some(progress) = &ctx.progress_callback {
            let title = if batch_tasks.len() == 1 {
                "delegate_task · 1 个子任务".to_string()
            } else {
                format!("delegate_task · {} 个并发子任务", batch_tasks.len())
            };
            progress(format!(
                "\u{001e}hakimi_delegate:delegate_parent|{}|准备委派任务|{}",
                title,
                chrono::Local::now().format("%H:%M:%S")
            ));
        }

        if let Some(executor) = &ctx.delegate_executor {
            if batch_tasks.len() == 1 {
                let (goal, context, toolsets) = &batch_tasks[0];
                let result = executor.execute_delegation(goal, context, toolsets).await?;
                Ok(json!([result]).to_string())
            } else {
                let results = executor.execute_batch_delegation(batch_tasks).await?;
                Ok(json!(results).to_string())
            }
        } else {
            Err(HakimiError::Tool(
                "Delegation executor not available in current context".into(),
            ))
        }
    }
}
