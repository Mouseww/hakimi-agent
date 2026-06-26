use async_trait::async_trait;
use hakimi_common::{HakimiError, Result, TeamCallContext, ToolContext};
use serde_json::{Value as JsonValue, json};

use crate::Tool;

/// Built-in tool: delegate a sub-task to a named teammate persona, or list teammates.
pub struct TeamTool;

#[async_trait]
impl Tool for TeamTool {
    fn name(&self) -> &str {
        "team"
    }

    fn toolset(&self) -> &str {
        "collaboration"
    }

    fn description(&self) -> &str {
        "Delegate a focused sub-task to a named teammate persona (each has its own model, skills, and memory). Use action='list' first to discover available teammates, then action='consult' to delegate. Use this when a teammate is better suited to part of the task."
    }

    fn emoji(&self) -> &str {
        "\u{1f91d}"
    }

    fn schema(&self) -> JsonValue {
        json!({
            "type": "object",
            "properties": {
                "action": { "type": "string", "enum": ["consult", "list"],
                    "description": "'list' = show available teammates; 'consult' = delegate to teammate(s). Omit to default to 'consult'." },
                "teammate": { "type": "string", "description": "Target teammate persona id (single consult)." },
                "teammates": { "type": "array", "items": {"type": "string"},
                    "description": "Multiple teammate persona ids for a parallel consult. Use instead of 'teammate'; if both are provided, 'teammates' takes precedence." },
                "task": { "type": "string", "description": "The sub-task or question for the teammate(s)." },
                "context": { "type": "string", "description": "Optional shared context and constraints." }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let Some(executor) = ctx.team_executor.clone() else {
            return Ok("Team collaboration is not enabled in this environment.".to_string());
        };

        let action = args.get("action").and_then(|v| v.as_str()).unwrap_or("consult");

        if action == "list" {
            let roster = executor.roster().await;
            if roster.is_empty() {
                return Ok("No teammates are available to consult.".to_string());
            }
            let lines: Vec<String> = roster
                .iter()
                .map(|t| format!("- {} ({}): {}", t.id, t.name, t.description))
                .collect();
            return Ok(format!("Available teammates:\n{}", lines.join("\n")));
        }

        if action != "consult" {
            return Err(HakimiError::Tool(format!(
                "unsupported team action '{action}'. Expected 'consult' or 'list'."
            )));
        }

        let task = args
            .get("task")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: task".into()))?;
        let context = args.get("context").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let progress = ctx.progress_callback.clone();

        // Multiple teammates -> parallel fan-out.
        if let Some(teammates) = args.get("teammates").and_then(|v| v.as_array()) {
            let ids: Vec<String> = teammates
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if ids.is_empty() {
                return Err(HakimiError::Tool("'teammates' must contain at least one id".into()));
            }
            let calls: Vec<TeamCallContext> = ids
                .iter()
                .map(|id| TeamCallContext {
                    teammate_id: id.clone(),
                    task: task.to_string(),
                    context: context.clone(),
                    progress: progress.clone(),
                })
                .collect();
            let answers = executor.consult_many(calls).await?;
            if answers.len() != ids.len() {
                return Err(HakimiError::Tool(format!(
                    "team consult_many returned {} answers for {} requests",
                    answers.len(),
                    ids.len()
                )));
            }
            let sections: Vec<String> = ids
                .iter()
                .zip(answers.iter())
                .map(|(id, answer)| format!("## {id}\n{answer}"))
                .collect();
            return Ok(sections.join("\n\n"));
        }

        // Single teammate.
        let teammate = args
            .get("teammate")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                HakimiError::Tool("provide 'teammate' (single) or 'teammates' (array)".into())
            })?;

        executor
            .consult(TeamCallContext {
                teammate_id: teammate.to_string(),
                task: task.to_string(),
                context,
                progress,
            })
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use hakimi_common::{TeamExecutor, TeammateInfo};

    /// Configurable stub for TeamExecutor.
    struct StubExec {
        /// Entries returned by `roster()`. Empty means no teammates.
        roster_entries: Vec<TeammateInfo>,
        /// If set, `consult()` returns this string. Otherwise returns "ok".
        consult_reply: Option<String>,
        /// If set, `consult_many()` echoes per-call answers using this prefix.
        /// Each answer is `"{prefix} {teammate_id}"`.
        consult_many_prefix: Option<String>,
    }

    impl StubExec {
        fn empty() -> Self {
            Self { roster_entries: vec![], consult_reply: None, consult_many_prefix: None }
        }
    }

    #[async_trait]
    impl TeamExecutor for StubExec {
        async fn roster(&self) -> Vec<TeammateInfo> {
            self.roster_entries.clone()
        }

        async fn consult(&self, _c: TeamCallContext) -> Result<String> {
            Ok(self.consult_reply.clone().unwrap_or_else(|| "ok".to_string()))
        }

        async fn consult_many(&self, calls: Vec<TeamCallContext>) -> Result<Vec<String>> {
            if let Some(prefix) = &self.consult_many_prefix {
                Ok(calls.iter().map(|c| format!("{} {}", prefix, c.teammate_id)).collect())
            } else {
                Ok(vec![])
            }
        }
    }

    #[test]
    fn tool_metadata() {
        let tool = TeamTool;
        assert_eq!(tool.name(), "team");
        assert_eq!(tool.toolset(), "collaboration");
        let schema = tool.schema();
        let required = schema["required"].as_array().unwrap();
        assert!(required.is_empty());
    }

    #[tokio::test]
    async fn execute_without_executor_degrades_gracefully() {
        let result = TeamTool.execute(&json!({"action": "list"}), &ToolContext::default()).await;
        assert!(result.unwrap().contains("not enabled"));
    }

    #[tokio::test]
    async fn consult_requires_task() {
        // team_executor None still returns the "not enabled" message before task checks,
        // so this asserts the missing-task path with a stub executor.
        let mut ctx = ToolContext::default();
        ctx.team_executor = Some(Arc::new(StubExec::empty()));
        let err = TeamTool
            .execute(&json!({"action": "consult", "teammate": "writer"}), &ctx)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("task"));
    }

    #[tokio::test]
    async fn list_action_renders_roster() {
        let stub = StubExec {
            roster_entries: vec![
                TeammateInfo {
                    id: "coder".to_string(),
                    name: "Code Expert".to_string(),
                    description: "writes code".to_string(),
                },
                TeammateInfo {
                    id: "writer".to_string(),
                    name: "Tech Writer".to_string(),
                    description: "writes docs".to_string(),
                },
            ],
            consult_reply: None,
            consult_many_prefix: None,
        };
        let mut ctx = ToolContext::default();
        ctx.team_executor = Some(Arc::new(stub));
        let output = TeamTool
            .execute(&json!({"action": "list"}), &ctx)
            .await
            .unwrap();
        assert!(output.contains("- coder (Code Expert): writes code"), "output: {output}");
        assert!(output.contains("- writer (Tech Writer): writes docs"), "output: {output}");
    }

    #[tokio::test]
    async fn fan_out_produces_sections_for_each_teammate() {
        let stub = StubExec {
            roster_entries: vec![],
            consult_reply: None,
            consult_many_prefix: Some("answer for".to_string()),
        };
        let mut ctx = ToolContext::default();
        ctx.team_executor = Some(Arc::new(stub));
        let output = TeamTool
            .execute(
                &json!({"action": "consult", "teammates": ["coder", "writer"], "task": "x"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(output.contains("## coder"), "output: {output}");
        assert!(output.contains("## writer"), "output: {output}");
        assert!(output.contains("answer for coder"), "output: {output}");
        assert!(output.contains("answer for writer"), "output: {output}");
    }

    #[tokio::test]
    async fn single_consult_returns_stub_reply() {
        let stub = StubExec {
            roster_entries: vec![],
            consult_reply: Some("Here is my expert answer.".to_string()),
            consult_many_prefix: None,
        };
        let mut ctx = ToolContext::default();
        ctx.team_executor = Some(Arc::new(stub));
        let output = TeamTool
            .execute(
                &json!({"action": "consult", "teammate": "writer", "task": "x"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(output.contains("Here is my expert answer."), "output: {output}");
    }
}
