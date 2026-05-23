use std::collections::VecDeque;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use hakimi_common::{DelegateExecutor, HakimiError, Result};
use hakimi_context::SimpleContextEngine;
use hakimi_tools::ToolRegistry;
use hakimi_transports::ProviderTransport;
use tokio::sync::{RwLock, Semaphore};
use tracing::{info, warn};

use crate::AIAgent;

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
    ) -> Self {
        Self {
            transport,
            context_engine,
            model,
            tool_registry,
            workdir,
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
        info!(
            goal = %goal,
            context = %context,
            toolsets = ?toolsets,
            "Spawning child agent for delegation"
        );

        // Acquire a permit before spawning a child agent to control concurrency.
        let _permit =
            self.semaphore.acquire().await.map_err(|e| {
                HakimiError::Tool(format!("failed to acquire delegation permit: {e}"))
            })?;

        // Retry logic for transient failures
        let mut attempts = 0;
        let max_attempts = 3;

        loop {
            attempts += 1;

            // Build a filtered tool registry for the child agent.
            let child_registry = ToolRegistry::new();
            let all_tool_names = self.tool_registry.list().await;
            for tool_name in &all_tool_names {
                if let Some(tool) = self.tool_registry.get(tool_name).await
                    && (toolsets.is_empty() || toolsets.contains(&tool.toolset().to_string()))
                {
                    child_registry.register(tool).await;
                }
            }

            // Build the system prompt for the child agent.
            let system_prompt = if context.is_empty() {
                format!(
                    "You are a sub-agent delegated by a parent agent. Your task: {goal}. Complete this task and return a clear, concise result."
                )
            } else {
                format!(
                    "You are a sub-agent delegated by a parent agent. Your task: {goal}. \
                     Context: {context}. Complete this task and return a clear, concise result."
                )
            };

            // Create a fresh context engine for the child agent (isolated from parent).
            let parent_context_length = {
                let engine = self.context_engine.read().await;
                engine.context_length()
            };
            let child_context_engine =
                Arc::new(RwLock::new(SimpleContextEngine::new(parent_context_length)));

            // The parent agent's context_engine might be a SmartContextEngine or SimpleContextEngine.
            // In simple cases we just share parent's builder parameters, but we must explicitly
            // pass an empty tool_registry and context_engine since we rebuild them.
            // Build the child agent.
            let child_agent = AIAgent::builder()
                .model(&self.model)
                .transport(self.transport.clone())
                .context_engine(child_context_engine)
                .tool_registry(child_registry)
                .system_prompt(system_prompt)
                .workdir(&self.workdir)
                .max_iterations(CHILD_MAX_ITERATIONS)
                .build()
                .map_err(|e| HakimiError::Tool(format!("failed to create child agent: {e}")))?;

            // Run the child agent with a timeout.
            // Using tokio::spawn to ensure the nested executor runs independently and handles
            // inner tool calls properly without deadlocking the parent's tokio context tasks.
            let result = tokio::time::timeout(DEFAULT_DELEGATION_TIMEOUT, async {
                let mut child = child_agent;
                let g = goal.to_string();
                tokio::task::spawn(async move { child.chat(&g).await }).await
            })
            .await;

            // Record execution metadata
            info!(
                task_goal = %goal,
                attempt = attempts,
                "Delegation attempt in progress"
            );

            match result {
                Ok(join_res) => match join_res {
                    Ok(Ok(response)) => {
                        info!(
                            response_len = response.len(),
                            attempts = attempts,
                            "Child agent delegation completed successfully"
                        );
                        return Ok(response);
                    }
                    Ok(Err(e)) => {
                        warn!(error = %e, attempts = attempts, "Child agent delegation failed");
                        if attempts >= max_attempts {
                            return Err(e);
                        }
                    }
                    Err(join_err) => {
                        let e = HakimiError::Tool(format!(
                            "Child agent task panicked or was cancelled: {}",
                            join_err
                        ));
                        warn!(error = %e, attempts = attempts, "Child agent delegation task failed");
                        if attempts >= max_attempts {
                            return Err(e);
                        }
                    }
                },
                Err(_elapsed) => {
                    warn!(
                        attempts = attempts,
                        "Child agent delegation timed out after 60 seconds"
                    );
                    if attempts >= max_attempts {
                        return Err(HakimiError::Other(
                            "Child agent timed out after 60 seconds after maximum retries".into(),
                        ));
                    }
                }
            }

            // Wait briefly before retrying
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    async fn enqueue_task(&self, goal: &str, priority: u32) -> Result<String> {
        let task_id = uuid::Uuid::new_v4().to_string();
        let task = QueuedTask {
            id: task_id.clone(),
            goal: goal.to_string(),
            priority,
        };
        let mut queue = self.task_queue.write().await;
        queue.push(task);
        info!(task_id = %task_id, goal = %goal, "Task enqueued for delegation");
        Ok(task_id)
    }
}
