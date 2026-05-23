use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hakimi_common::{HakimiError, Message, Result, ToolContext};
use hakimi_context::ContextEngine;
use hakimi_tools::ToolRegistry;
use hakimi_transports::ProviderTransport;
use tokio::sync::RwLock;
use tracing::info;
use uuid::Uuid;

use crate::conversation::ConversationResult;
use crate::loop_impl;

/// The central AI agent that orchestrates LLM interactions, tool dispatch,
/// and context management.
///
/// Use [`AIAgent::builder()`] to construct an instance via the builder pattern.
pub struct AIAgent {
    pub(crate) model: String,
    pub(crate) max_iterations: usize,
    pub(crate) transport: Arc<dyn ProviderTransport>,
    pub(crate) tool_registry: ToolRegistry,
    pub(crate) context_engine: Arc<RwLock<dyn ContextEngine>>,
    pub(crate) session_id: String,
    pub(crate) platform: Option<String>,
    pub(crate) user_id: Option<String>,
    pub(crate) chat_id: Option<String>,
    pub(crate) messages: Vec<Message>,
    pub(crate) interrupt: Arc<AtomicBool>,
    pub(crate) workdir: String,
    pub(crate) system_prompt: Option<String>,
    pub(crate) streaming: bool,
    pub(crate) streaming_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    pub(crate) knowledge_searcher: Option<Arc<dyn hakimi_common::KnowledgeSearcher>>,
    pub(crate) skill_store: Option<hakimi_skills::SkillStore>,
}

impl AIAgent {
    /// Create a new agent from its components.
    pub fn new(
        model: impl Into<String>,
        transport: Arc<dyn ProviderTransport>,
        tool_registry: ToolRegistry,
        skill_store: Option<hakimi_skills::SkillStore>,
    ) -> Self {
        let engine = hakimi_context::SmartContextEngine::new(128000, None);
        Self::builder()
            .model(model)
            .transport(transport)
            .context_engine(Arc::new(tokio::sync::RwLock::new(engine)))
            .tool_registry(tool_registry)
            .build()
            .expect("failed to build agent with defaults")
            .with_skill_store(skill_store)
    }

    /// Set or replace the skill store.
    pub fn with_skill_store(mut self, store: Option<hakimi_skills::SkillStore>) -> Self {
        self.skill_store = store;
        self
    }
}

/// Builder for constructing an [`AIAgent`].
///
/// Required fields: `model`, `transport`, `context_engine`.
/// All other fields have sensible defaults.
#[derive(Clone)]
pub struct AIAgentBuilder {
    model: Option<String>,
    max_iterations: Option<usize>,
    transport: Option<Arc<dyn ProviderTransport>>,
    tool_registry: Option<ToolRegistry>,
    context_engine: Option<Arc<RwLock<dyn ContextEngine>>>,
    session_id: Option<String>,
    platform: Option<String>,
    user_id: Option<String>,
    chat_id: Option<String>,
    interrupt: Option<Arc<AtomicBool>>,
    workdir: Option<String>,
    system_prompt: Option<String>,
    streaming: Option<bool>,
    streaming_callback: Option<Arc<dyn Fn(String) + Send + Sync>>,
    knowledge_searcher: Option<Arc<dyn hakimi_common::KnowledgeSearcher>>,
    skill_store: Option<hakimi_skills::SkillStore>,
}

impl AIAgentBuilder {
    /// Create a new builder with all fields unset.
    pub fn new() -> Self {
        Self {
            model: None,
            max_iterations: None,
            transport: None,
            tool_registry: None,
            context_engine: None,
            session_id: None,
            platform: None,
            user_id: None,
            chat_id: None,
            interrupt: None,
            workdir: None,
            system_prompt: None,
            streaming: None,
            streaming_callback: None,
            knowledge_searcher: None,
            skill_store: None,
        }
    }

    /// Set the model identifier (e.g. `"gpt-4o"`, `"claude-sonnet-4-20250514"`).
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the maximum number of tool-calling iterations per conversation.
    pub fn max_iterations(mut self, max: usize) -> Self {
        self.max_iterations = Some(max);
        self
    }

    /// Set the provider transport for LLM communication.
    pub fn transport(mut self, transport: Arc<dyn ProviderTransport>) -> Self {
        self.transport = Some(transport);
        self
    }

    /// Set the tool registry for tool dispatch.
    pub fn tool_registry(mut self, registry: ToolRegistry) -> Self {
        self.tool_registry = Some(registry);
        self
    }

    /// Set the context engine for token tracking and compression.
    pub fn context_engine(mut self, engine: Arc<RwLock<dyn ContextEngine>>) -> Self {
        self.context_engine = Some(engine);
        self
    }

    /// Set the session ID. Defaults to a random UUID v4.
    pub fn session_id(mut self, id: impl Into<String>) -> Self {
        self.session_id = Some(id.into());
        self
    }

    /// Set the platform name (e.g. `"telegram"`, `"discord"`).
    pub fn platform(mut self, platform: impl Into<String>) -> Self {
        self.platform = Some(platform.into());
        self
    }

    /// Set the user ID.
    pub fn user_id(mut self, id: impl Into<String>) -> Self {
        self.user_id = Some(id.into());
        self
    }

    /// Set the chat/channel ID.
    pub fn chat_id(mut self, id: impl Into<String>) -> Self {
        self.chat_id = Some(id.into());
        self
    }

    /// Set an interrupt flag. When set to `true`, the agent loop will stop.
    pub fn interrupt(mut self, interrupt: Arc<AtomicBool>) -> Self {
        self.interrupt = Some(interrupt);
        self
    }

    /// Set the working directory for tool execution.
    pub fn workdir(mut self, dir: impl Into<String>) -> Self {
        self.workdir = Some(dir.into());
        self
    }

    /// Set a custom system prompt.
    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = Some(prompt.into());
        self
    }

    /// Enable or disable streaming mode.
    ///
    /// When enabled, `run_conversation` will use the streaming transport
    /// and print content deltas to stdout in real-time.
    pub fn streaming(mut self, enable: bool) -> Self {
        self.streaming = Some(enable);
        self
    }

    /// Set a callback function to receive streaming text tokens.
    pub fn streaming_callback<F>(mut self, callback: F) -> Self
    where
        F: Fn(String) + Send + Sync + 'static,
    {
        self.streaming_callback = Some(Arc::new(callback));
        self
    }

    pub fn skill_store(mut self, store: hakimi_skills::SkillStore) -> Self {
        self.skill_store = Some(store);
        self
    }

    /// Set the knowledge searcher for the agent.
    pub fn knowledge_searcher(
        mut self,
        searcher: Arc<dyn hakimi_common::KnowledgeSearcher>,
    ) -> Self {
        self.knowledge_searcher = Some(searcher);
        self
    }

    /// Build the [`AIAgent`].
    ///
    /// # Errors
    /// Returns [`HakimiError::Config`] if required fields are missing.
    pub fn build(self) -> Result<AIAgent> {
        let model = self
            .model
            .ok_or_else(|| HakimiError::Config("model is required".into()))?;
        let transport = self
            .transport
            .ok_or_else(|| HakimiError::Config("transport is required".into()))?;
        let context_engine = self
            .context_engine
            .ok_or_else(|| HakimiError::Config("context_engine is required".into()))?;

        let session_id = self
            .session_id
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let max_iterations = self.max_iterations.unwrap_or(90);
        let tool_registry = self.tool_registry.unwrap_or_default();
        let interrupt = self
            .interrupt
            .unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let workdir = self.workdir.unwrap_or_else(|| ".".to_string());

        info!(
            session_id = %session_id,
            model = %model,
            max_iterations = max_iterations,
            "AIAgent created"
        );

        Ok(AIAgent {
            model,
            max_iterations,
            transport,
            tool_registry,
            context_engine,
            session_id,
            platform: self.platform,
            user_id: self.user_id,
            chat_id: self.chat_id,
            messages: Vec::new(),
            interrupt,
            workdir,
            system_prompt: self.system_prompt,
            streaming: self.streaming.unwrap_or(false),
            streaming_callback: self.streaming_callback,
            knowledge_searcher: self.knowledge_searcher,
            skill_store: Some(
                self.skill_store
                    .unwrap_or_else(hakimi_skills::SkillStore::empty),
            ),
        })
    }
}

impl Default for AIAgentBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl AIAgent {
    /// Create a new builder for constructing an [`AIAgent`].
    pub fn builder() -> AIAgentBuilder {
        AIAgentBuilder::new()
    }

    /// Simple interface: send a message and get a text response.
    ///
    /// This is a convenience wrapper around [`run_conversation`](Self::run_conversation)
    /// that returns only the final text.
    pub async fn chat(&mut self, message: &str) -> Result<String> {
        let result = self.run_conversation(message).await?;
        Ok(result.final_response)
    }

    /// Alias for [`chat`](Self::chat) — kept for API compatibility.
    pub async fn query(&mut self, user_message: &str) -> Result<String> {
        self.chat(user_message).await
    }

    /// Run a full conversation turn: send a user message and iterate with tools
    /// until the model produces a text response or the budget is exhausted.
    ///
    /// Returns a [`ConversationResult`] containing the final response, all
    /// messages, accumulated usage, and the number of API calls made.
    pub async fn run_conversation(&mut self, user_message: &str) -> Result<ConversationResult> {
        // Apply skill prompt additions.
        let skill_prompt = if let Some(store) = &self.skill_store {
            store.get_system_prompt_additions(user_message)
        } else {
            String::new()
        };
        if !skill_prompt.is_empty() {
            let base = self
                .system_prompt
                .clone()
                .unwrap_or_else(|| crate::DEFAULT_SYSTEM_PROMPT.to_string());
            self.set_system_prompt(format!("{base}\n\n{skill_prompt}"));
        }

        // Append the user message to conversation history.
        self.messages.push(Message::user(user_message));

        // Run the core agent loop (streaming or non-streaming).
        let result = if self.streaming {
            loop_impl::run_loop_streaming(self).await?
        } else {
            loop_impl::run_loop(self).await?
        };

        Ok(result)
    }

    /// Convenience method: enable streaming and run a conversation.
    ///
    /// This is equivalent to calling `.streaming(true)` on the builder and
    /// then calling `run_conversation`. Returns only the final text.
    pub async fn chat_streaming(&mut self, message: &str) -> Result<String> {
        self.streaming = true;
        let result = self.run_conversation(message).await?;
        Ok(result.final_response)
    }

    /// Dynamically set the streaming callback for this agent instance.
    pub fn set_streaming_callback(&mut self, callback: Option<Arc<dyn Fn(String) + Send + Sync>>) {
        self.streaming_callback = callback;
    }

    /// Build a [`ToolContext`] from the agent's current state.
    ///
    /// Includes a [`CoreDelegateExecutor`] so that the `delegate_task` tool
    /// can spawn child agents that share this agent's transport and resources.
    pub(crate) fn build_tool_context(&self) -> ToolContext {
        let delegate_executor: Option<Arc<dyn hakimi_common::DelegateExecutor>> =
            Some(Arc::new(crate::CoreDelegateExecutor::new(
                self.transport.clone(),
                self.context_engine.clone(),
                self.model.clone(),
                self.tool_registry.clone(),
                self.workdir.clone(),
            )));

        ToolContext {
            session_id: self.session_id.clone(),
            user_id: self.user_id.clone(),
            task_id: None,
            workdir: self.workdir.clone(),
            model: Some(self.model.clone()),
            delegate_executor,
            knowledge_searcher: self.knowledge_searcher.clone(),
        }
    }

    /// Check whether the interrupt flag has been set.
    pub(crate) fn check_interrupt(&self) -> bool {
        self.interrupt.load(Ordering::Relaxed)
    }

    /// Append a message to the conversation history.
    pub fn add_message(&mut self, message: Message) {
        self.messages.push(message);
    }

    /// Get the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Get the model identifier.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the platform name, if set.
    pub fn platform(&self) -> Option<&str> {
        self.platform.as_deref()
    }

    /// Get the chat/channel ID, if set.
    pub fn chat_id(&self) -> Option<&str> {
        self.chat_id.as_deref()
    }

    /// Get the conversation message history.
    pub fn messages(&self) -> &[Message] {
        &self.messages
    }

    /// Get a reference to the tool registry.
    pub fn tool_registry(&self) -> &ToolRegistry {
        &self.tool_registry
    }

    /// Clear the conversation message history.
    pub fn clear_messages(&mut self) {
        self.messages.clear();
    }

    /// Set or replace the system prompt.
    pub fn set_system_prompt(&mut self, prompt: impl Into<String>) {
        self.system_prompt = Some(prompt.into());
    }

    /// Change the model identifier at runtime.
    pub fn set_model(&mut self, model: impl Into<String>) {
        self.model = model.into();
    }

    /// Set the interrupt flag to stop the agent loop.
    pub fn interrupt(&self) {
        self.interrupt.store(true, Ordering::Relaxed);
    }

    /// Clear the interrupt flag.
    pub fn clear_interrupt(&self) {
        self.interrupt.store(false, Ordering::Relaxed);
    }

    /// Get a reference to the skill store.
    pub fn skill_store(&self) -> Option<&hakimi_skills::SkillStore> {
        self.skill_store.as_ref()
    }
}
