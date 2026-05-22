use async_trait::async_trait;
use hakimi_common::{Result, ToolContext, HakimiError};
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
                "task": {
                    "type": "string",
                    "description": "The specific task description for the sub-agent."
                },
                "context": {
                    "type": "string",
                    "description": "Additional context or constraints for the task."
                },
                "toolsets": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Optional list of toolsets the sub-agent should have access to."
                }
            },
            "required": ["task"]
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let task = args.get("task").and_then(|v| v.as_str()).ok_or_else(|| {
            HakimiError::Tool("missing 'task' argument".into())
        })?;
        let context = args.get("context").and_then(|v| v.as_str()).unwrap_or("");
        
        let toolsets: Vec<String> = args.get("toolsets")
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|i| i.as_str().map(|s| s.to_string())).collect())
            .unwrap_or_default();

        info!(task = %task, "Delegating task via ToolContext");

        if let Some(executor) = &ctx.delegate_executor {
            let result = executor.execute_delegation(task, context, &toolsets).await?;
            Ok(result)
        } else {
            Err(HakimiError::Tool("Delegation executor not available in current context".into()))
        }
    }
}
