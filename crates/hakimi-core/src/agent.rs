use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

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
    pub(crate) _provider: String,
    pub(crate) _base_url: String,
    pub(crate) _api_key: String,
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
}

/// Builder for constructing an [`AIAgent`].
///
/// Required fields: `model`, `transport`, `context_engine`.
/// All other fields have sensible defaults.
pub struct AIAgentBuilder {
    model: Option<String>,
    provider: Option<String>,
    base_url: Option<String>,
    api_key: Option<String>,
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
}

impl AIAgentBuilder {
    /// Create a new builder with all fields unset.
    pub fn new() -> Self {
        Self {
            model: None,
            provider: None,
            base_url: None,
            api_key: None,
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
        }
    }

    /// Set the model identifier (e.g. `"gpt-4o"`, `"claude-sonnet-4-20250514"`).
    pub fn model(mut self, model: impl Into<String>) -> Self {
        self.model = Some(model.into());
        self
    }

    /// Set the provider name (e.g. `"openai"`, `"anthropic"`).
    pub fn provider(mut self, provider: impl Into<String>) -> Self {
        self.provider = Some(provider.into());
        self
    }

    /// Set the base URL for the API endpoint.
    pub fn base_url(mut self, url: impl Into<String>) -> Self {
        self.base_url = Some(url.into());
        self
    }

    /// Set the API key.
    pub fn api_key(mut self, key: impl Into<String>) -> Self {
        self.api_key = Some(key.into());
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

        let session_id = self.session_id.unwrap_or_else(|| Uuid::new_v4().to_string());
        let provider = self
            .provider
            .unwrap_or_else(|| transport.provider_name().to_string());
        let base_url = self.base_url.unwrap_or_default();
        let api_key = self.api_key.unwrap_or_default();
        let max_iterations = self.max_iterations.unwrap_or(90);
        let tool_registry = self.tool_registry.unwrap_or_default();
        let interrupt = self.interrupt.unwrap_or_else(|| Arc::new(AtomicBool::new(false)));
        let workdir = self.workdir.unwrap_or_else(|| ".".to_string());

        info!(
            session_id = %session_id,
            model = %model,
            provider = %provider,
            max_iterations = max_iterations,
            "AIAgent created"
        );

        Ok(AIAgent {
            model,
            _provider: provider,
            _base_url: base_url,
            _api_key: api_key,
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

    /// Run a full conversation turn: send a user message and iterate with tools
    /// until the model produces a text response or the budget is exhausted.
    ///
    /// Returns a [`ConversationResult`] containing the final response, all
    /// messages, accumulated usage, and the number of API calls made.
    ///
    /// If streaming is enabled on this agent, the streaming transport will be
    /// used and content deltas will be printed to stdout in real-time.
    pub async fn run_conversation(&mut self, user_message: &str) -> Result<ConversationResult> {
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

    /// Build a [`ToolContext`] from the agent's current state.
    pub(crate) fn build_tool_context(&self) -> ToolContext {
        ToolContext {
            session_id: self.session_id.clone(),
            user_id: self.user_id.clone(),
            task_id: None,
            workdir: self.workdir.clone(),
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

    /// Set the interrupt flag to stop the agent loop.
    pub fn interrupt(&self) {
        self.interrupt.store(true, Ordering::Relaxed);
    }

    /// Clear the interrupt flag.
    pub fn clear_interrupt(&self) {
        self.interrupt.store(false, Ordering::Relaxed);
    }
}
