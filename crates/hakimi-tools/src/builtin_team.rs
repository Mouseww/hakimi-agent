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
        "Delegate sub-tasks to specialized teammate personas. Each teammate has its own model, skills, and memory. PROACTIVELY use this tool in these scenarios: (1) Task requires domain expertise you lack (coding, writing, research, data analysis); (2) Parallel workstreams can speed up delivery; (3) Complex task benefits from divide-and-conquer; (4) Teammate's specialized skills outperform your general capabilities. Use action='list' first to discover teammates, then action='consult' to delegate. IMPORTANT: When delegating to multiple teammates, use the 'tasks' parameter to assign DIFFERENT sub-tasks to each teammate based on their specialization. Control execution with 'mode': 'parallel' (default, all run concurrently) or 'sequential' (run one after another with previous results as context for dependency chains). For complex workflows with mixed parallel/sequential needs, use 'stages' to group tasks into sequential phases where each phase's tasks run in parallel. Delegation is a strength, not a weakness — leverage your team early and often."
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
                    "description": "DEPRECATED: Multiple teammate persona ids receiving the SAME task. Use 'tasks' instead for proper task division." },
                "tasks": { "type": "array", "items": {
                    "type": "object",
                    "properties": {
                        "teammate": { "type": "string", "description": "Teammate persona id" },
                        "task": { "type": "string", "description": "Specific sub-task for this teammate" },
                        "context": { "type": "string", "description": "Optional context for this specific task" }
                    },
                    "required": ["teammate", "task"]
                }, "description": "PREFERRED: Array of distinct sub-tasks, each assigned to a specific teammate. By default executes in parallel. Use 'mode' to control execution order." },
                "mode": { "type": "string", "enum": ["parallel", "sequential"],
                    "description": "Execution mode for 'tasks'. 'parallel' (default) = all tasks run concurrently. 'sequential' = tasks run one after another, each receiving the previous task's result as context. Use sequential when later tasks depend on earlier results." },
                "stages": { "type": "array", "items": {
                    "type": "object",
                    "properties": {
                        "tasks": { "type": "array", "items": {
                            "type": "object",
                            "properties": {
                                "teammate": { "type": "string" },
                                "task": { "type": "string" },
                                "context": { "type": "string" }
                            },
                            "required": ["teammate", "task"]
                        }}
                    },
                    "required": ["tasks"]
                }, "description": "ADVANCED: Multi-stage execution. Each stage's tasks run in parallel, but stages execute sequentially. Each stage receives all previous stages' results. Use for complex workflows with mixed parallel/sequential needs." },
                "task": { "type": "string", "description": "The sub-task or question. Required for single/teammates modes, ignored if 'tasks' or 'stages' is provided." },
                "context": { "type": "string", "description": "Optional shared context. Ignored if 'tasks' or 'stages' is provided (each task can have its own context)." }
            },
            "required": []
        })
    }

    async fn execute(&self, args: &JsonValue, ctx: &ToolContext) -> Result<String> {
        let Some(executor) = ctx.team_executor.clone() else {
            return Ok("Team collaboration is not enabled in this environment.".to_string());
        };

        let action = args
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("consult");

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

        let progress = ctx.progress_callback.clone();

        // ADVANCED: Multi-stage execution
        if let Some(stages_array) = args.get("stages").and_then(|v| v.as_array()) {
            if stages_array.is_empty() {
                return Err(HakimiError::Tool("'stages' array must not be empty".into()));
            }
            let mut all_results = Vec::new();
            let mut accumulated_context = String::new();

            for (stage_idx, stage_obj) in stages_array.iter().enumerate() {
                let stage_tasks = stage_obj
                    .get("tasks")
                    .and_then(|v| v.as_array())
                    .ok_or_else(|| {
                        HakimiError::Tool(format!("stages[{stage_idx}] missing 'tasks'"))
                    })?;

                if stage_tasks.is_empty() {
                    return Err(HakimiError::Tool(format!(
                        "stages[{stage_idx}].tasks must not be empty"
                    )));
                }

                let mut calls = Vec::new();
                for (idx, task_obj) in stage_tasks.iter().enumerate() {
                    let teammate = task_obj
                        .get("teammate")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            HakimiError::Tool(format!(
                                "stages[{stage_idx}].tasks[{idx}] missing 'teammate'"
                            ))
                        })?;
                    let task = task_obj
                        .get("task")
                        .and_then(|v| v.as_str())
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .ok_or_else(|| {
                            HakimiError::Tool(format!(
                                "stages[{stage_idx}].tasks[{idx}] missing 'task'"
                            ))
                        })?;
                    let mut context = task_obj
                        .get("context")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    // Inject previous stages' results as context
                    if !accumulated_context.is_empty() {
                        context = if context.is_empty() {
                            format!("Previous stages' results:\n{}", accumulated_context)
                        } else {
                            format!(
                                "{}\n\nPrevious stages' results:\n{}",
                                context, accumulated_context
                            )
                        };
                    }

                    calls.push(TeamCallContext {
                        teammate_id: teammate.to_string(),
                        task: task.to_string(),
                        context,
                        progress: progress.clone(),
                    });
                }

                let teammate_ids: Vec<String> =
                    calls.iter().map(|c| c.teammate_id.clone()).collect();
                let task_titles: Vec<String> = stage_tasks
                    .iter()
                    .filter_map(|t| {
                        t.get("task")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string())
                    })
                    .collect();

                // Execute this stage in parallel
                let answers = executor.consult_many(calls).await?;

                // Build stage results
                let stage_results: Vec<String> = teammate_ids
                    .iter()
                    .zip(task_titles.iter())
                    .map(|(id, task)| {
                        format!(
                            "✓ Stage {} - {} completed: {}",
                            stage_idx + 1,
                            id,
                            task
                        )
                    })
                    .collect();

                // Accumulate for next stage
                for (i, answer) in answers.iter().enumerate() {
                    accumulated_context.push_str(&format!("\n[{}]: {}\n", teammate_ids[i], answer));
                }

                all_results.extend(stage_results);
            }

            return Ok(format!("Multi-stage collaboration completed ({} stages):\n{}", stages_array.len(), all_results.join("\n")));
        }

        // NEW: Structured tasks array - each teammate gets a different task
        if let Some(tasks_array) = args.get("tasks").and_then(|v| v.as_array()) {
            if tasks_array.is_empty() {
                return Err(HakimiError::Tool("'tasks' array must not be empty".into()));
            }

            let mode = args
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("parallel");

            let mut calls = Vec::new();
            for (idx, task_obj) in tasks_array.iter().enumerate() {
                let teammate = task_obj
                    .get("teammate")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| HakimiError::Tool(format!("tasks[{idx}] missing 'teammate'")))?;
                let task = task_obj
                    .get("task")
                    .and_then(|v| v.as_str())
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .ok_or_else(|| HakimiError::Tool(format!("tasks[{idx}] missing 'task'")))?;
                let context = task_obj
                    .get("context")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                calls.push(TeamCallContext {
                    teammate_id: teammate.to_string(),
                    task: task.to_string(),
                    context,
                    progress: progress.clone(),
                });
            }

            let teammate_ids: Vec<String> = calls.iter().map(|c| c.teammate_id.clone()).collect();
            let task_titles: Vec<String> = tasks_array
                .iter()
                .filter_map(|t| {
                    t.get("task")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string())
                })
                .collect();

            if mode == "sequential" {
                // Sequential execution: each task gets previous results as context
                let mut results = Vec::new();
                let mut accumulated_context = String::new();

                for (i, mut call) in calls.into_iter().enumerate() {
                    // Inject previous results as context
                    if !accumulated_context.is_empty() {
                        call.context = if call.context.is_empty() {
                            format!("Previous tasks' results:\n{}", accumulated_context)
                        } else {
                            format!(
                                "{}\n\nPrevious tasks' results:\n{}",
                                call.context, accumulated_context
                            )
                        };
                    }

                    let answer = executor.consult(call).await?;
                    accumulated_context.push_str(&format!("\n[{}]: {}\n", teammate_ids[i], answer));
                    results.push((teammate_ids[i].clone(), task_titles[i].clone(), answer));
                }

                let sections: Vec<String> = results
                    .iter()
                    .map(|(id, task, _answer)| format!("✓ {} completed: {}", id, task))
                    .collect();
                return Ok(format!("Sequential collaboration completed:\n{}", sections.join("\n")));
            } else {
                // Parallel execution (default)
                let answers = executor.consult_many(calls).await?;
                if answers.len() != teammate_ids.len() {
                    return Err(HakimiError::Tool(format!(
                        "team consult_many returned {} answers for {} requests",
                        answers.len(),
                        teammate_ids.len()
                    )));
                }
                let sections: Vec<String> = teammate_ids
                    .iter()
                    .zip(task_titles.iter())
                    .map(|(id, task)| format!("✓ {} completed: {}", id, task))
                    .collect();
                return Ok(format!("Parallel collaboration completed:\n{}", sections.join("\n")));
            }
        }

        // Legacy: shared task + context
        let task = args
            .get("task")
            .and_then(|v| v.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| HakimiError::Tool("missing required parameter: task".into()))?;
        let context = args
            .get("context")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        // Multiple teammates -> parallel fan-out (DEPRECATED: same task for all).
        if let Some(teammates) = args.get("teammates").and_then(|v| v.as_array()) {
            let ids: Vec<String> = teammates
                .iter()
                .filter_map(|v| v.as_str())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
            if ids.is_empty() {
                return Err(HakimiError::Tool(
                    "'teammates' must contain at least one id".into(),
                ));
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
                .map(|id| format!("✓ {} completed", id))
                .collect();
            return Ok(format!("Team collaboration completed:\n{}", sections.join("\n")));
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

        let result = executor
            .consult(TeamCallContext {
                teammate_id: teammate.to_string(),
                task: task.to_string(),
                context,
                progress,
            })
            .await?;
        
        Ok(format!("✓ {} completed task", teammate))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::{TeamExecutor, TeammateInfo};
    use std::sync::Arc;

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
            Self {
                roster_entries: vec![],
                consult_reply: None,
                consult_many_prefix: None,
            }
        }
    }

    #[async_trait]
    impl TeamExecutor for StubExec {
        async fn roster(&self) -> Vec<TeammateInfo> {
            self.roster_entries.clone()
        }

        async fn consult(&self, _c: TeamCallContext) -> Result<String> {
            Ok(self
                .consult_reply
                .clone()
                .unwrap_or_else(|| "ok".to_string()))
        }

        async fn consult_many(&self, calls: Vec<TeamCallContext>) -> Result<Vec<String>> {
            if let Some(prefix) = &self.consult_many_prefix {
                Ok(calls
                    .iter()
                    .map(|c| format!("{} {}", prefix, c.teammate_id))
                    .collect())
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
        let result = TeamTool
            .execute(&json!({"action": "list"}), &ToolContext::default())
            .await;
        assert!(result.unwrap().contains("not enabled"));
    }

    #[tokio::test]
    async fn consult_requires_task() {
        // team_executor None still returns the "not enabled" message before task checks,
        // so this asserts the missing-task path with a stub executor.
        let ctx = ToolContext {
            team_executor: Some(Arc::new(StubExec::empty())),
            ..Default::default()
        };
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
        let ctx = ToolContext {
            team_executor: Some(Arc::new(stub)),
            ..Default::default()
        };
        let output = TeamTool
            .execute(&json!({"action": "list"}), &ctx)
            .await
            .unwrap();
        assert!(
            output.contains("- coder (Code Expert): writes code"),
            "output: {output}"
        );
        assert!(
            output.contains("- writer (Tech Writer): writes docs"),
            "output: {output}"
        );
    }

    #[tokio::test]
    async fn fan_out_produces_sections_for_each_teammate() {
        let stub = StubExec {
            roster_entries: vec![],
            consult_reply: None,
            consult_many_prefix: Some("answer for".to_string()),
        };
        let ctx = ToolContext {
            team_executor: Some(Arc::new(stub)),
            ..Default::default()
        };
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
        let ctx = ToolContext {
            team_executor: Some(Arc::new(stub)),
            ..Default::default()
        };
        let output = TeamTool
            .execute(
                &json!({"action": "consult", "teammate": "writer", "task": "x"}),
                &ctx,
            )
            .await
            .unwrap();
        assert!(
            output.contains("Here is my expert answer."),
            "output: {output}"
        );
    }
}
