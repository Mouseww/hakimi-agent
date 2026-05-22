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

        // Build a filtered tool registry for the child agent.
        let child_registry = ToolRegistry::new();
        let all_tool_names = self.tool_registry.list().await;
        for tool_name in &all_tool_names {
            if let Some(tool) = self.tool_registry.get(tool_name).await {
                // If toolsets are specified, only include tools from those sets.
                // If no toolsets specified, include all tools.
                if toolsets.is_empty() || toolsets.contains(&tool.toolset().to_string()) {
                    child_registry.register(tool).await;
                }
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

        // Build the child agent.
        let mut child_agent = AIAgent::builder()
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
        let result = tokio::time::timeout(DEFAULT_DELEGATION_TIMEOUT, child_agent.chat(goal)).await;

        match result {
            Ok(Ok(response)) => {
                info!(
                    response_len = response.len(),
                    "Child agent delegation completed successfully"
                );
                Ok(response)
            }
            Ok(Err(e)) => {
                warn!(error = %e, "Child agent delegation failed");
                Err(e)
            }
            Err(_elapsed) => {
                warn!("Child agent delegation timed out after 60 seconds");
                Err(HakimiError::Other(
                    "Child agent timed out after 60 seconds".into(),
                ))
            }
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
