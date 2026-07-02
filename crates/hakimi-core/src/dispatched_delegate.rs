//! Dispatched delegate executor — wraps CoreDelegateExecutor with model dispatch support.

use std::sync::Arc;

use async_trait::async_trait;
use hakimi_common::{DelegateExecutor, HakimiError, Result, ToolProgressCallback};
use hakimi_config::ModelConfig;
use hakimi_tools::ToolRegistry;
use hakimi_transports::ProviderTransport;
use tokio::sync::RwLock;

use crate::DispatchedAgent;

/// Delegate executor that creates DispatchedAgent children instead of plain AIAgent.
pub struct DispatchedDelegateExecutor {
    transport: Arc<dyn ProviderTransport>,
    #[allow(dead_code)]
    context_engine: Arc<RwLock<dyn hakimi_context::ContextEngine>>,
    model_config: ModelConfig,
    tool_registry: ToolRegistry,
    #[allow(dead_code)]
    workdir: String,
    skill_store: Option<hakimi_skills::SkillStore>,
    progress_callback: Option<ToolProgressCallback>,
    parent_depth: usize,
}

impl DispatchedDelegateExecutor {
    /// Create a new dispatched delegate executor.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        transport: Arc<dyn ProviderTransport>,
        context_engine: Arc<RwLock<dyn hakimi_context::ContextEngine>>,
        model_config: ModelConfig,
        tool_registry: ToolRegistry,
        workdir: String,
        skill_store: Option<hakimi_skills::SkillStore>,
        progress_callback: Option<ToolProgressCallback>,
        parent_depth: usize,
    ) -> Self {
        Self {
            transport,
            context_engine,
            model_config,
            tool_registry,
            workdir,
            skill_store,
            progress_callback,
            parent_depth,
        }
    }
}

#[async_trait]
impl DelegateExecutor for DispatchedDelegateExecutor {
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
        results
            .into_iter()
            .next()
            .ok_or_else(|| HakimiError::Tool("No result returned from delegation".to_string()))
    }

    async fn execute_batch_delegation(
        &self,
        tasks: Vec<(String, String, Vec<String>)>,
    ) -> Result<Vec<String>> {
        // Create DispatchedAgent children with inherited model_config and depth+1.
        use std::sync::Arc;
        use tokio::sync::Semaphore;

        const MAX_CONCURRENT_DELEGATIONS: usize = 3;

        let semaphore = Arc::new(Semaphore::new(MAX_CONCURRENT_DELEGATIONS));
        let mut futures = Vec::new();

        for (goal, context, _toolsets) in tasks {
            let transport = self.transport.clone();
            // TODO: Implement proper toolset filtering (requires async ToolRegistry API refactor)
            let child_registry = self.tool_registry.clone();

            let parent_skill_store = self.skill_store.clone();
            let progress_callback = self.progress_callback.clone();
            let semaphore = semaphore.clone();
            let model_config = self.model_config.clone();
            let child_depth = self.parent_depth + 1;

            // Generate unique session ID
            let child_session_id = format!("child_{}", uuid::Uuid::new_v4().simple());

            let future = async move {
                let _permit = semaphore.acquire().await.map_err(|e| {
                    HakimiError::Tool(format!("failed to acquire delegation permit: {e}"))
                })?;

                let mut attempts = 0;
                let max_attempts = 3;

                loop {
                    attempts += 1;

                    // Build system prompt
                    let child_instructions = "You are a sub-agent delegated by a parent agent. You have your own local runtime skill working set seeded from this subtask; use relevant local skills when helpful, but do not dump skill text back to the parent. Return only concise task results in this format:\\nStatus: success | partial | failed\\nSummary:\\nFindings:\\nFiles inspected:\\nCommands run:\\nRisks:\\nRecommendations:";
                    let system_prompt = if context.is_empty() {
                        format!("{child_instructions}\\n\\nYour task: {goal}")
                    } else {
                        format!(
                            "{child_instructions}\\n\\nYour task: {goal}\\n\\nContext and constraints:\\n{context}"
                        )
                    };

                    let seed_text = if context.is_empty() {
                        goal.clone()
                    } else {
                        format!("{goal}\\n\\n{context}")
                    };
                    let child_skill_store = parent_skill_store
                        .as_ref()
                        .map(|store| store.fork_for_subtask(&seed_text));

                    // Get base model name from config
                    // The actual model will be selected dynamically by DispatchedAgent
                    let base_model = model_config
                        .tiers
                        .as_ref()
                        .map(|t| t.primary.model.clone())
                        .unwrap_or_else(|| model_config.default.clone());

                    // Create base AIAgent with parent's transport
                    // DispatchedAgent will create tier-specific agents as needed
                    let mut child_agent = crate::AIAgent::new(
                        &base_model,
                        transport.clone(),
                        child_registry.clone(),
                        child_skill_store,
                    );
                    child_agent.set_system_prompt(system_prompt);
                    child_agent.set_session_id(child_session_id.clone());
                    if let Some(parent_progress) = progress_callback.clone() {
                        child_agent.set_streaming_callback(Some(Arc::new(move |token: String| {
                            // Forward tool/review notices to parent
                            if token.starts_with("\\u{001e}hakimi_tool:")
                                || token.starts_with("\\u{001e}hakimi_review:")
                            {
                                parent_progress(token);
                            }
                        })));
                    }

                    // Wrap in DispatchedAgent to inherit dispatch config
                    let mut dispatched_child =
                        DispatchedAgent::new(child_agent, model_config.clone(), child_depth)?;

                    // Execute
                    match dispatched_child.run_conversation(&goal).await {
                        Ok(res) => {
                            return Ok(res.final_response);
                        }
                        Err(e) => {
                            if attempts >= max_attempts {
                                return Err(HakimiError::Tool(format!(
                                    "Child agent failed after {max_attempts} attempts: {e}"
                                )));
                            }
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

            futures.push(tokio::spawn(future));
        }

        // Wait for all children
        let mut results = Vec::new();
        for join_handle in futures::future::join_all(futures).await {
            match join_handle {
                Ok(Ok(result)) => results.push(result),
                Ok(Err(e)) => return Err(e),
                Err(e) => return Err(HakimiError::Tool(format!("Child agent panicked: {e}"))),
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
