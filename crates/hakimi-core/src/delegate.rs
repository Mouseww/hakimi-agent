#![allow(dead_code)]
use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hakimi_common::{DelegateExecutor, HakimiError, Result, ToolProgressCallback};
use hakimi_tools::ToolRegistry;
use hakimi_transports::ProviderTransport;
use tokio::sync::{RwLock, Semaphore};
use tracing::info;

fn now_progress_timestamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs() % 86_400)
        .unwrap_or(0);
    let hour = secs / 3_600;
    let minute = (secs % 3_600) / 60;
    let second = secs % 60;
    format!("{hour:02}:{minute:02}:{second:02}")
}

fn truncate_progress_text(value: &str, max_chars: usize) -> String {
    let normalized = value.replace('\n', " ");
    let mut chars = normalized.chars();
    let truncated: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn emit_delegate_progress(
    callback: &Option<ToolProgressCallback>,
    task_id: &str,
    title: &str,
    line: impl AsRef<str>,
) {
    if let Some(cb) = callback {
        cb(format!(
            "\u{001e}hakimi_delegate:{}|{}|{}|{}",
            task_id,
            title,
            line.as_ref(),
            now_progress_timestamp()
        ));
    }
}

/// Default timeout for child agent execution (60 seconds).
const DEFAULT_DELEGATION_TIMEOUT: Duration = Duration::from_secs(60);

/// Default max iterations for a child agent.
const CHILD_MAX_ITERATIONS: usize = 10;

/// Maximum number of concurrent child agents.
const MAX_CONCURRENT_DELEGATIONS: usize = 5;

/// Task queue for internal coordination.
pub struct TaskQueue {
    tasks: VecDeque<QueuedTask>,
}

pub struct QueuedTask {
    pub id: String,
    pub goal: String,
    pub priority: u32,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self {
            tasks: VecDeque::new(),
        }
    }

    pub fn push(&mut self, task: QueuedTask) {
        self.tasks.push_back(task);
    }
}

impl Default for TaskQueue {
    fn default() -> Self {
        Self::new()
    }
}

impl TaskQueue {
    pub fn pop(&mut self) -> Option<QueuedTask> {
        self.tasks.pop_front()
    }
}

/// Core implementation of [`DelegateExecutor`].
///
/// Holds cloned references to the parent agent's shared resources so it can
/// spawn independent child agents on demand.
pub struct CoreDelegateExecutor {
    transport: Arc<dyn ProviderTransport>,
    context_engine: Arc<RwLock<dyn hakimi_context::ContextEngine>>,
    model: String,
    tool_registry: ToolRegistry,
    workdir: String,
    skill_store: Option<hakimi_skills::SkillStore>,
    progress_callback: Option<ToolProgressCallback>,
    task_queue: Arc<RwLock<TaskQueue>>,
    semaphore: Arc<Semaphore>,
}

impl CoreDelegateExecutor {
    /// Create a new executor from the parent agent's shared resources.
    pub fn new(
        transport: Arc<dyn ProviderTransport>,
        context_engine: Arc<RwLock<dyn hakimi_context::ContextEngine>>,
        model: String,
        tool_registry: ToolRegistry,
        workdir: String,
        skill_store: Option<hakimi_skills::SkillStore>,
        progress_callback: Option<ToolProgressCallback>,
    ) -> Self {
        Self {
            transport,
            context_engine,
            model,
            tool_registry,
            workdir,
            skill_store,
            progress_callback,
            task_queue: Arc::new(RwLock::new(TaskQueue::new())),
            semaphore: Arc::new(Semaphore::new(MAX_CONCURRENT_DELEGATIONS)),
        }
    }
}

#[async_trait]
impl DelegateExecutor for CoreDelegateExecutor {
    async fn execute_delegation(
        &self,
        goal: &str,
        context: &str,
        toolsets: &[String],
    ) -> Result<String> {
        let results = self
            .execute_batch_delegation(vec![(
                goal.to_string(),
                context.to_string(),
                toolsets.to_vec(),
            )])
            .await?;

        Ok(results.into_iter().next().unwrap_or_default())
    }

    async fn execute_batch_delegation(
        &self,
        tasks: Vec<(String, String, Vec<String>)>,
    ) -> Result<Vec<String>> {
        info!(
            task_count = tasks.len(),
            "Spawning child agents for batch delegation"
        );

        let mut futures = Vec::new();

        for (goal, context, toolsets) in tasks {
            let transport = self.transport.clone();
            let parent_model = self.model.clone();
            let parent_skill_store = self.skill_store.clone();
            let progress_callback = self.progress_callback.clone();
            let all_tool_names = self.tool_registry.list().await;

            // Build a filtered tool registry for this child
            let child_registry = ToolRegistry::new();
            for tool_name in &all_tool_names {
                if let Some(tool) = self.tool_registry.get(tool_name).await
                    && (toolsets.is_empty() || toolsets.contains(&tool.toolset().to_string()))
                {
                    child_registry.register(tool).await;
                }
            }

            let semaphore = self.semaphore.clone();
            let _workdir = self.workdir.clone();

            // Generate a unique session ID for the child
            let child_session_id = format!("child_{}", uuid::Uuid::new_v4().simple());
            let child_label = child_session_id.chars().rev().take(6).collect::<String>();
            let progress_task_id = child_session_id.clone();
            let progress_title = format!(
                "子代理 {} · {}",
                child_label,
                truncate_progress_text(&goal, 32)
            );
            emit_delegate_progress(
                &progress_callback,
                &progress_task_id,
                &progress_title,
                "已创建子代理",
            );

            // Optionally, we could create an isolated sub-directory for the child's workdir here.
            // For now, we share the parent's workdir but they run in parallel.

            let future = async move {
                // Acquire a permit before spawning a child agent to control concurrency.
                emit_delegate_progress(
                    &progress_callback,
                    &progress_task_id,
                    &progress_title,
                    "等待并发执行许可",
                );
                let _permit = semaphore.acquire().await.map_err(|e| {
                    HakimiError::Tool(format!("failed to acquire delegation permit: {e}"))
                })?;

                let mut attempts = 0;
                let max_attempts = 3;
                emit_delegate_progress(
                    &progress_callback,
                    &progress_task_id,
                    &progress_title,
                    "开始执行任务",
                );

                loop {
                    attempts += 1;

                    // Build the system prompt for the child agent.
                    let child_instructions = "You are a sub-agent delegated by a parent agent. You have your own local runtime skill working set seeded from this subtask; use relevant local skills when helpful, but do not dump skill text back to the parent. Return only concise task results in this format:\nStatus: success | partial | failed\nSummary:\nFindings:\nFiles inspected:\nCommands run:\nRisks:\nRecommendations:";
                    let system_prompt = if context.is_empty() {
                        format!("{child_instructions}\n\nYour task: {goal}")
                    } else {
                        format!(
                            "{child_instructions}\n\nYour task: {goal}\n\nContext and constraints:\n{context}"
                        )
                    };

                    let seed_text = if context.is_empty() {
                        goal.clone()
                    } else {
                        format!("{goal}\n\n{context}")
                    };
                    let child_skill_store = parent_skill_store
                        .as_ref()
                        .map(|store| store.fork_for_subtask(&seed_text));

                    let mut child_agent = crate::AIAgent::new(
                        &parent_model,
                        transport.clone(),
                        child_registry.clone(),
                        child_skill_store,
                    );
                    child_agent.set_system_prompt(system_prompt);
                    child_agent.set_session_id(child_session_id.clone());
                    if let Some(parent_progress) = progress_callback.clone() {
                        let child_progress_task_id = progress_task_id.clone();
                        let child_progress_title = progress_title.clone();
                        child_agent.set_streaming_callback(Some(std::sync::Arc::new(
                            move |token: String| {
                                if let Some(tool_notice) =
                                    token.strip_prefix("\u{001e}hakimi_tool:")
                                {
                                    emit_delegate_progress(
                                        &Some(parent_progress.clone()),
                                        &child_progress_task_id,
                                        &child_progress_title,
                                        tool_notice.trim(),
                                    );
                                } else if let Some(review_notice) =
                                    token.strip_prefix("\u{001e}hakimi_review:")
                                {
                                    emit_delegate_progress(
                                        &Some(parent_progress.clone()),
                                        &child_progress_task_id,
                                        &child_progress_title,
                                        review_notice.trim(),
                                    );
                                }
                            },
                        )));
                    }
                    // Not mounting the parent's ContextEngine so the child runs with clean context.

                    // Execute
                    match child_agent.run_conversation(&goal).await {
                        Ok(res) => {
                            emit_delegate_progress(
                                &progress_callback,
                                &progress_task_id,
                                &progress_title,
                                "完成，返回结果",
                            );
                            return Ok(res.final_response);
                        }
                        Err(e) => {
                            if attempts >= max_attempts {
                                emit_delegate_progress(
                                    &progress_callback,
                                    &progress_task_id,
                                    &progress_title,
                                    format!("失败: {e}"),
                                );
                                return Err(HakimiError::Tool(format!(
                                    "Child agent failed after {max_attempts} attempts: {e}"
                                )));
                            }
                            emit_delegate_progress(
                                &progress_callback,
                                &progress_task_id,
                                &progress_title,
                                format!("第 {} 次失败，准备重试", attempts),
                            );
                            tracing::warn!(
                                error = %e,
                                attempt = attempts,
                                "Child agent failed, retrying"
                            );
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                        }
                    }
                }
            };

            // We use tokio::spawn to ensure they run concurrently on the executor
            futures.push(tokio::spawn(future));
        }

        // Wait for all children to complete
        let mut results = Vec::new();
        for join_handle in futures::future::join_all(futures).await {
            match join_handle {
                Ok(Ok(result_text)) => results.push(result_text),
                Ok(Err(e)) => results.push(format!("Task failed: {e}")),
                Err(e) => results.push(format!("Task panicked: {e}")),
            }
        }

        Ok(results)
    }

    async fn enqueue_task(&self, _goal: &str, _priority: u32) -> Result<String> {
        Err(HakimiError::Tool(
            "Task queueing is not yet implemented".into(),
        ))
    }
}
