use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use hakimi_common::{HakimiError, Message, Result, ToolContext, ToolSearchConfig};
use hakimi_context::ContextEngine;
use hakimi_tools::ToolRegistry;
use hakimi_transports::{EmbeddingProvider, ProviderTransport};
use tokio::sync::RwLock;
use tracing::{info, warn};
use uuid::Uuid;

use crate::conversation::ConversationResult;
use crate::loop_impl;
use crate::trajectory::TrajectoryConfig;

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
    pub(crate) embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    pub(crate) skill_store: Option<hakimi_skills::SkillStore>,
    pub(crate) tts_provider: Option<String>,
    pub(crate) tts_model: Option<String>,
    pub(crate) tts_base_url: Option<String>,
    pub(crate) tts_api_key: Option<String>,
    pub(crate) tts_voice: Option<String>,
    pub(crate) tts_auto_play: bool,
    pub(crate) transcription_provider: Option<String>,
    pub(crate) transcription_model: Option<String>,
    pub(crate) transcription_base_url: Option<String>,
    pub(crate) transcription_api_key: Option<String>,
    pub(crate) tool_search_config: ToolSearchConfig,
    pub(crate) tool_search_context_length: usize,
    pub(crate) trajectory_config: Option<TrajectoryConfig>,
}

impl Clone for AIAgent {
    fn clone(&self) -> Self {
        Self {
            model: self.model.clone(),
            max_iterations: self.max_iterations,
            transport: self.transport.clone(),
            tool_registry: self.tool_registry.clone(),
            context_engine: self.context_engine.clone(),
            session_id: self.session_id.clone(),
            platform: self.platform.clone(),
            user_id: self.user_id.clone(),
            chat_id: self.chat_id.clone(),
            messages: self.messages.clone(),
            interrupt: Arc::new(AtomicBool::new(false)),
            workdir: self.workdir.clone(),
            system_prompt: self.system_prompt.clone(),
            streaming: self.streaming,
            streaming_callback: self.streaming_callback.clone(),
            knowledge_searcher: self.knowledge_searcher.clone(),
            embedding_provider: self.embedding_provider.clone(),
            skill_store: self.skill_store.clone(),
            tts_provider: self.tts_provider.clone(),
            tts_model: self.tts_model.clone(),
            tts_base_url: self.tts_base_url.clone(),
            tts_api_key: self.tts_api_key.clone(),
            tts_voice: self.tts_voice.clone(),
            tts_auto_play: self.tts_auto_play,
            transcription_provider: self.transcription_provider.clone(),
            transcription_model: self.transcription_model.clone(),
            transcription_base_url: self.transcription_base_url.clone(),
            transcription_api_key: self.transcription_api_key.clone(),
            tool_search_config: self.tool_search_config.clone(),
            tool_search_context_length: self.tool_search_context_length,
            trajectory_config: self.trajectory_config.clone(),
        }
    }
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

    /// Apply progressive tool-disclosure settings used by the tool registry.
    pub fn with_tool_search_settings(
        mut self,
        config: ToolSearchConfig,
        context_length: usize,
    ) -> Self {
        self.tool_search_config = config.normalized();
        self.tool_search_context_length = context_length;
        self
    }

    /// Enable or disable Hermes-compatible ShareGPT JSONL trajectory saving.
    pub fn with_trajectory_saving(mut self, config: Option<TrajectoryConfig>) -> Self {
        self.trajectory_config = config;
        self
    }

    /// Set or replace the skill store.
    pub fn with_skill_store(mut self, store: Option<hakimi_skills::SkillStore>) -> Self {
        self.skill_store = store;
        self
    }

    pub fn with_context_engine(mut self, engine: Arc<RwLock<dyn ContextEngine>>) -> Self {
        self.context_engine = engine;
        self
    }

    /// Set or replace the embedding provider.
    pub fn with_embedding_provider(mut self, provider: Option<Arc<dyn EmbeddingProvider>>) -> Self {
        self.embedding_provider = provider;
        self
    }

    /// Set or replace the knowledge searcher.
    pub fn with_knowledge_searcher(
        mut self,
        searcher: Option<Arc<dyn hakimi_common::KnowledgeSearcher>>,
    ) -> Self {
        self.knowledge_searcher = searcher;
        self
    }

    /// Apply runtime voice settings used by TTS and transcription tools.
    #[allow(clippy::too_many_arguments)]
    pub fn with_voice_settings(
        mut self,
        tts_provider: Option<String>,
        tts_model: Option<String>,
        tts_base_url: Option<String>,
        tts_api_key: Option<String>,
        tts_voice: Option<String>,
        tts_auto_play: bool,
        transcription_provider: Option<String>,
        transcription_model: Option<String>,
        transcription_base_url: Option<String>,
        transcription_api_key: Option<String>,
    ) -> Self {
        self.tts_provider = tts_provider;
        self.tts_model = tts_model;
        self.tts_base_url = tts_base_url;
        self.tts_api_key = tts_api_key;
        self.tts_voice = tts_voice;
        self.tts_auto_play = tts_auto_play;
        self.transcription_provider = transcription_provider;
        self.transcription_model = transcription_model;
        self.transcription_base_url = transcription_base_url;
        self.transcription_api_key = transcription_api_key;
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
    embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
    skill_store: Option<hakimi_skills::SkillStore>,
    tts_provider: Option<String>,
    tts_model: Option<String>,
    tts_base_url: Option<String>,
    tts_api_key: Option<String>,
    tts_voice: Option<String>,
    tts_auto_play: bool,
    transcription_provider: Option<String>,
    transcription_model: Option<String>,
    transcription_base_url: Option<String>,
    transcription_api_key: Option<String>,
    tool_search_config: Option<ToolSearchConfig>,
    tool_search_context_length: Option<usize>,
    trajectory_config: Option<TrajectoryConfig>,
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
            embedding_provider: None,
            skill_store: None,
            tts_provider: None,
            tts_model: None,
            tts_base_url: None,
            tts_api_key: None,
            tts_voice: None,
            tts_auto_play: false,
            transcription_provider: None,
            transcription_model: None,
            transcription_base_url: None,
            transcription_api_key: None,
            tool_search_config: None,
            tool_search_context_length: None,
            trajectory_config: None,
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

    /// Set the embedding provider for vector search / RAG features.
    pub fn embedding_provider(mut self, provider: Arc<dyn EmbeddingProvider>) -> Self {
        self.embedding_provider = Some(provider);
        self
    }

    /// Set progressive tool-disclosure behavior.
    pub fn tool_search(mut self, config: ToolSearchConfig, context_length: usize) -> Self {
        self.tool_search_config = Some(config.normalized());
        self.tool_search_context_length = Some(context_length);
        self
    }

    /// Configure Hermes-compatible ShareGPT JSONL trajectory saving.
    pub fn trajectory_saving(mut self, config: TrajectoryConfig) -> Self {
        self.trajectory_config = Some(config);
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
            embedding_provider: self.embedding_provider,
            skill_store: Some(
                self.skill_store
                    .unwrap_or_else(hakimi_skills::SkillStore::empty),
            ),
            tts_provider: self.tts_provider,
            tts_model: self.tts_model,
            tts_base_url: self.tts_base_url,
            tts_api_key: self.tts_api_key,
            tts_voice: self.tts_voice,
            tts_auto_play: self.tts_auto_play,
            transcription_provider: self.transcription_provider,
            transcription_model: self.transcription_model,
            transcription_base_url: self.transcription_base_url,
            transcription_api_key: self.transcription_api_key,
            tool_search_config: self.tool_search_config.unwrap_or_default().normalized(),
            tool_search_context_length: self.tool_search_context_length.unwrap_or(128_000),
            trajectory_config: self.trajectory_config,
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
        // Refresh the runtime skill working set for this turn. Skills are not
        // appended permanently to `system_prompt`; `build_send_messages` will
        // render the current working set dynamically on each model call.
        if let Some(store) = &mut self.skill_store {
            store.observe(user_message);
        }

        // Append the user message to conversation history.
        self.messages.push(Message::user(user_message));

        // Run the core agent loop (streaming or non-streaming).
        let result = if self.streaming {
            loop_impl::run_loop_streaming(self).await
        } else {
            loop_impl::run_loop(self).await
        };

        match result {
            Ok(result) => {
                self.save_trajectory_if_enabled(!result.final_response.is_empty());
                Ok(result)
            }
            Err(err) => {
                self.save_trajectory_if_enabled(false);
                Err(err)
            }
        }
    }

    /// Run a conversation with a custom pre-constructed message.
    /// Use this when you need to attach images or other multimodal content.
    pub async fn run_conversation_with_message(
        &mut self,
        msg: Message,
    ) -> Result<ConversationResult> {
        if let Some(store) = &mut self.skill_store
            && let Some(content) = &msg.content
        {
            store.observe(content);
        }

        self.messages.push(msg);

        let result = if self.streaming {
            loop_impl::run_loop_streaming(self).await
        } else {
            loop_impl::run_loop(self).await
        };

        match result {
            Ok(result) => {
                self.save_trajectory_if_enabled(!result.final_response.is_empty());
                Ok(result)
            }
            Err(err) => {
                self.save_trajectory_if_enabled(false);
                Err(err)
            }
        }
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

    /// Convenience method: enable streaming and run a conversation with a custom message.
    pub async fn chat_streaming_with_message(&mut self, msg: Message) -> Result<String> {
        self.streaming = true;
        let result = self.run_conversation_with_message(msg).await?;
        Ok(result.final_response)
    }

    /// Run a streaming conversation and return the full result, including usage.
    pub async fn run_conversation_streaming_with_message(
        &mut self,
        msg: Message,
    ) -> Result<ConversationResult> {
        self.streaming = true;
        self.run_conversation_with_message(msg).await
    }

    /// Dynamically set the streaming callback for this agent instance.
    pub fn set_streaming_callback(&mut self, callback: Option<Arc<dyn Fn(String) + Send + Sync>>) {
        self.streaming_callback = callback;
    }

    /// Enable or disable streaming for subsequent conversation turns.
    pub fn set_streaming(&mut self, streaming: bool) {
        self.streaming = streaming;
    }

    fn save_trajectory_if_enabled(&self, completed: bool) {
        let Some(config) = self.trajectory_config.as_ref() else {
            return;
        };

        let system_prompt = self
            .system_prompt
            .as_deref()
            .unwrap_or(crate::DEFAULT_SYSTEM_PROMPT);
        let mut snapshot = Vec::with_capacity(self.messages.len() + 1);
        if !system_prompt.trim().is_empty() {
            snapshot.push(Message::system(system_prompt));
        }
        snapshot.extend(self.messages.iter().cloned());

        match crate::trajectory::save_trajectory(&snapshot, &self.model, completed, config) {
            Ok(path) => {
                info!(
                    completed = completed,
                    path = %path.display(),
                    "trajectory saved"
                );
            }
            Err(err) => {
                warn!(
                    completed = completed,
                    error = %err,
                    "failed to save trajectory"
                );
            }
        }
    }

    /// Convert a loaded skill slash command into the user-message payload that
    /// should be sent to the model, recording the explicit skill use if found.
    pub fn build_skill_slash_invocation_message(&mut self, input: &str) -> Option<String> {
        self.skill_store
            .as_mut()
            .and_then(|store| store.build_slash_invocation_message(input))
    }

    /// Build a [`ToolContext`] from the agent's current state.
    ///
    /// Includes a [`CoreDelegateExecutor`] so that the `delegate_task` tool
    /// can spawn child agents that share this agent's transport and resources.
    pub fn build_tool_context(&self) -> ToolContext {
        let delegate_executor: Option<Arc<dyn hakimi_common::DelegateExecutor>> =
            Some(Arc::new(crate::CoreDelegateExecutor::new(
                self.transport.clone(),
                self.context_engine.clone(),
                self.model.clone(),
                self.tool_registry.clone(),
                self.workdir.clone(),
                self.skill_store.clone(),
                self.streaming_callback.clone(),
            )));

        ToolContext {
            session_id: self.session_id.clone(),
            user_id: self.user_id.clone(),
            task_id: None,
            workdir: self.workdir.clone(),
            model: Some(self.model.clone()),
            delegate_executor,
            knowledge_searcher: self.knowledge_searcher.clone(),
            progress_callback: self.streaming_callback.clone(),
            tts_provider: self.tts_provider.clone(),
            tts_model: self.tts_model.clone(),
            tts_base_url: self.tts_base_url.clone(),
            tts_api_key: self.tts_api_key.clone(),
            tts_voice: self.tts_voice.clone(),
            tts_auto_play: self.tts_auto_play,
            transcription_provider: self.transcription_provider.clone(),
            transcription_model: self.transcription_model.clone(),
            transcription_base_url: self.transcription_base_url.clone(),
            transcription_api_key: self.transcription_api_key.clone(),
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

    pub fn set_session_id(&mut self, session_id: impl Into<String>) {
        self.session_id = session_id.into();
    }

    /// Get the model identifier.
    pub fn model(&self) -> &str {
        &self.model
    }

    /// Get the provider name for the active transport.
    pub fn provider_name(&self) -> &str {
        self.transport.provider_name()
    }

    /// Get the latest provider rate-limit snapshot, if available.
    pub fn rate_limits(&self) -> Option<hakimi_transports::RateLimitState> {
        self.transport.rate_limits()
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

    /// Clone the interrupt flag for external run controllers.
    pub fn interrupt_handle(&self) -> Arc<AtomicBool> {
        self.interrupt.clone()
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
