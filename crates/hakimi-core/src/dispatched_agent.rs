//! Dispatched agent — wraps AIAgent with intelligent model tier selection.

use std::ops::{Deref, DerefMut};
use std::sync::Arc;

use hakimi_common::Result;
use hakimi_config::{ModelConfig, TierConfig};
use hakimi_transports::{AnthropicTransport, ChatCompletionsTransport, ProviderTransport};
use reqwest::Client;

use crate::model_dispatch::TaskComplexity;
use crate::model_dispatcher::ModelDispatcher;
use crate::{AIAgent, ConversationResult};

/// Wrapper around AIAgent that performs model dispatch before execution.
pub struct DispatchedAgent {
    /// Base agent (will be cloned/reconfigured per dispatch decision).
    base_agent: AIAgent,
    
    /// Model dispatcher (None = single-model mode).
    dispatcher: Option<ModelDispatcher>,
    
    /// Model configuration for rebuilding agents.
    model_config: ModelConfig,
    
    /// Agent nesting depth (0 = main, 1 = child, 2 = grandchild).
    depth: usize,
}

impl DispatchedAgent {
    /// Create a dispatched agent from base agent and config.
    pub fn new(
        base_agent: AIAgent,
        model_config: ModelConfig,
        depth: usize,
    ) -> Result<Self> {
        let dispatcher = if model_config.auto_dispatch.enabled {
            if let Some(ref tiers) = model_config.tiers {
                Some(ModelDispatcher::new(
                    tiers.clone(),
                    model_config.auto_dispatch.clone(),
                    depth,
                ))
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            base_agent,
            dispatcher,
            model_config,
            depth,
        })
    }

    /// Run conversation with automatic model dispatch.
    pub async fn run_conversation(&mut self, user_message: &str) -> Result<ConversationResult> {
        // No dispatcher? Fall back to base agent
        let Some(ref dispatcher) = self.dispatcher else {
            return self.base_agent.run_conversation(user_message).await;
        };

        // Analyze complexity and select model
        let (tier_config, complexity) = dispatcher.select_model(
            user_message,
            &self.base_agent.messages,
        );

        // Show dispatch decision via streaming callback
        if dispatcher.should_show_decision() {
            if let Some(ref callback) = self.base_agent.streaming_callback {
                let decision = dispatcher.format_decision(&complexity, &tier_config);
                callback(decision);
                callback("\n\n".to_string());
            }
        }

        // Check if two-stage execution is needed
        if dispatcher.should_use_two_stage(&complexity) {
            self.run_two_stage(user_message, &complexity).await
        } else {
            self.run_single_stage(user_message, &tier_config).await
        }
    }

    /// Single-stage execution: use selected model directly.
    async fn run_single_stage(
        &mut self,
        user_message: &str,
        tier_config: &hakimi_config::TierConfig,
    ) -> Result<ConversationResult> {
        // Create a temporary agent with the selected tier's model
        let mut tier_agent = self.create_agent_for_tier(tier_config)?;
        tier_agent.run_conversation(user_message).await
    }

    /// Create a new agent configured for the specified tier.
    fn create_agent_for_tier(&self, tier_config: &TierConfig) -> Result<AIAgent> {
        // Create transport based on provider
        let transport = self.create_transport_for_tier(tier_config)?;
        
        // Clone base agent structure but replace transport and model
        let mut new_agent = AIAgent::new(
            tier_config.model.clone(),
            transport,
            self.base_agent.tool_registry().clone(),
            self.base_agent.skill_store.clone(),
        );
        
        // Preserve important state from base agent
        new_agent.messages = self.base_agent.messages.clone();
        new_agent.streaming_callback = self.base_agent.streaming_callback.clone();
        
        Ok(new_agent)
    }
    
    /// Create a transport from tier configuration with API key fallback.
    fn create_transport_for_tier(&self, tier_config: &TierConfig) -> Result<Arc<dyn ProviderTransport>> {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| hakimi_common::HakimiError::Config(
                format!("Failed to create HTTP client: {}", e)
            ))?;
        
        let base_url = if tier_config.base_url.is_empty() {
            // Use default base URLs based on provider
            match tier_config.provider.as_str() {
                "anthropic" => "https://api.anthropic.com".to_string(),
                "openai" => "https://api.openai.com".to_string(),
                _ => return Err(hakimi_common::HakimiError::Config(
                    format!("Provider '{}' requires explicit base_url", tier_config.provider)
                )),
            }
        } else {
            tier_config.base_url.clone()
        };
        
        // Get API key: tier-specific > top-level config > environment variable
        let api_key = if !tier_config.api_key.is_empty() {
            tier_config.api_key.clone()
        } else if !self.model_config.api_key.is_empty() {
            self.model_config.api_key.clone()
        } else {
            // Fallback to environment variables
            let api_key_env = format!("{}_API_KEY", tier_config.provider.to_uppercase());
            std::env::var(&api_key_env)
                .or_else(|_| std::env::var("API_KEY"))
                .unwrap_or_default()
        };
        
        if api_key.is_empty() {
            return Err(hakimi_common::HakimiError::Config(
                format!("API key not found for provider '{}'. Set tier-specific api_key in config, or {} env var.", 
                    tier_config.provider, 
                    format!("{}_API_KEY", tier_config.provider.to_uppercase()))
            ));
        }
        
        // Create transport based on provider type
        let transport: Arc<dyn ProviderTransport> = match tier_config.provider.as_str() {
            "anthropic" => Arc::new(AnthropicTransport::new(base_url, api_key, client)),
            _ => {
                // Default to OpenAI-compatible Chat Completions for all other providers
                Arc::new(ChatCompletionsTransport::new(base_url, api_key, client))
            }
        };
        
        Ok(transport)
    }

    /// Two-stage execution: reasoning planning → primary execution.
    async fn run_two_stage(
        &mut self,
        user_message: &str,
        complexity: &TaskComplexity,
    ) -> Result<ConversationResult> {
        let Some(ref dispatcher) = self.dispatcher else {
            return Err(hakimi_common::HakimiError::Config(
                "dispatcher should exist for two-stage execution".into()
            ));
        };

        // Stage 1: Reasoning agent generates plan
        let reasoning_tier = dispatcher.reasoning_tier()
            .ok_or_else(|| hakimi_common::HakimiError::Config(
                "reasoning tier should exist for two-stage execution".into()
            ))?;

        // Stream reasoning stage indicator
        if let Some(ref callback) = self.base_agent.streaming_callback {
            callback("🧠 **Stage 1: 高级思考模型规划中...**\n\n".to_string());
        }

        // Build reasoning prompt
        let reasoning_prompt = format!(
            "请为以下任务生成详细的执行计划。只需要规划步骤，不要实际执行工具调用。\\n\\n\\\
             任务: {}\\n\\n\\\
             复杂度分析: {}\\n\\n\\\
             请输出：\\n\\\
             1. 任务分解（具体的子任务列表）\\n\\\
             2. 执行步骤（每一步要做什么、使用什么工具）\\n\\\
             3. 潜在风险和注意事项\\n\\\
             4. 预期结果",
            user_message,
            complexity.reasoning
        );

        // Create reasoning agent with reasoning tier config
        let mut reasoning_agent = self.create_agent_for_tier(reasoning_tier)?;
        let reasoning_result = reasoning_agent.run_conversation(&reasoning_prompt).await?;

        // Stage 2: Primary agent executes based on plan
        let primary_tier = dispatcher.primary_tier();

        if let Some(ref callback) = self.base_agent.streaming_callback {
            callback("\\n\\n⚡ **Stage 2: 主力模型执行中...**\\n\\n".to_string());
        }

        // Build execution prompt with reasoning output
        let execution_prompt = if self.model_config.auto_dispatch.two_stage.show_reasoning_to_primary {
            format!(
                "基于以下规划，完成用户的任务。\\n\\n\\\
                 原始任务: {}\\n\\n\\\
                 执行计划:\\n{}\\n\\n\\\
                 现在请按计划执行任务，使用必要的工具调用，并给出最终结果。",
                user_message,
                reasoning_result.final_response
            )
        } else {
            user_message.to_string()
        };

        // Create primary agent with primary tier config
        let mut primary_agent = self.create_agent_for_tier(primary_tier)?;
        primary_agent.run_conversation(&execution_prompt).await
    }

    /// Get underlying base agent (for accessing messages, etc.)
    pub fn base_agent(&self) -> &AIAgent {
        &self.base_agent
    }
    
    pub fn base_agent_mut(&mut self) -> &mut AIAgent {
        &mut self.base_agent
    }
    
    /// Get the model configuration.
    pub fn model_config(&self) -> &ModelConfig {
        &self.model_config
    }
    
    /// Build a tool context with dispatched delegation support.
    /// This overrides AIAgent's default behavior to inject DispatchedDelegateExecutor.
    pub fn build_dispatched_tool_context(&self) -> hakimi_common::ToolContext {
        use std::sync::Arc;
        use hakimi_common::ToolContext;
        
        let delegate_executor: Option<Arc<dyn hakimi_common::DelegateExecutor>> =
            Some(Arc::new(crate::dispatched_delegate::DispatchedDelegateExecutor::new(
                self.base_agent.shared.transport.clone(),
                self.base_agent.context_engine.clone(),
                self.model_config.clone(),
                self.base_agent.shared.tool_registry.clone(),
                self.base_agent.workdir.clone(),
                self.base_agent.skill_store.clone(),
                self.base_agent.streaming_callback.clone(),
                self.depth,
            )));
        
        ToolContext {
            session_id: self.base_agent.session_id.clone(),
            user_id: self.base_agent.user_id.clone(),
            task_id: None,
            workdir: self.base_agent.workdir.clone(),
            model: Some(self.base_agent.model.clone()),
            delegate_executor,
            team_executor: self.base_agent.team_executor.clone(),
            knowledge_searcher: self.base_agent.shared.knowledge_searcher.clone(),
            progress_callback: self.base_agent.streaming_callback.clone(),
            tts_provider: self.base_agent.tts_provider.clone(),
            tts_model: self.base_agent.tts_model.clone(),
            tts_base_url: self.base_agent.tts_base_url.clone(),
            tts_api_key: self.base_agent.tts_api_key.clone(),
            tts_voice: self.base_agent.tts_voice.clone(),
            tts_auto_play: self.base_agent.tts_auto_play,
            transcription_provider: self.base_agent.transcription_provider.clone(),
            transcription_model: self.base_agent.transcription_model.clone(),
            transcription_base_url: self.base_agent.transcription_base_url.clone(),
            transcription_api_key: self.base_agent.transcription_api_key.clone(),
        }
    }
}

/// Allow DispatchedAgent to be used like AIAgent for all non-dispatch operations.
impl Deref for DispatchedAgent {
    type Target = AIAgent;

    fn deref(&self) -> &Self::Target {
        &self.base_agent
    }
}

/// Allow mutable access to AIAgent methods.
impl DerefMut for DispatchedAgent {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base_agent
    }
}

/// Clone support for shared state scenarios (e.g., Arc<Mutex<DispatchedAgent>>).
impl Clone for DispatchedAgent {
    fn clone(&self) -> Self {
        Self {
            base_agent: self.base_agent.clone(),
            dispatcher: self.dispatcher.clone(),
            model_config: self.model_config.clone(),
            depth: self.depth,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_config::{AutoDispatchConfig, ModelTiers, TierConfig};

    fn mock_tier_config(model: &str) -> TierConfig {
        TierConfig {
            provider: "test".into(),
            model: model.into(),
            api_key: String::new(),
            base_url: String::new(),
        }
    }

    fn mock_model_config() -> ModelConfig {
        ModelConfig {
            default: "test-primary".into(),
            provider: "test".into(),
            context_length: 0,
            base_url: String::new(),
            api_mode: String::new(),
            api_key: String::new(),
            tiers: Some(ModelTiers {
                primary: mock_tier_config("test-primary"),
                light: Some(mock_tier_config("test-light")),
                reasoning: Some(mock_tier_config("test-reasoning")),
            }),
            auto_dispatch: AutoDispatchConfig::default(),
        }
    }

    #[test]
    fn test_dispatcher_creation() {
        let model_config = mock_model_config();
        
        // Auto-dispatch enabled with tiers
        let dispatcher = if model_config.auto_dispatch.enabled {
            model_config.tiers.as_ref().map(|tiers| {
                ModelDispatcher::new(
                    tiers.clone(),
                    model_config.auto_dispatch.clone(),
                    0,
                )
            })
        } else {
            None
        };

        assert!(dispatcher.is_some());
    }

    #[test]
    fn test_dispatcher_disabled() {
        let mut model_config = mock_model_config();
        model_config.auto_dispatch.enabled = false;

        let dispatcher = if model_config.auto_dispatch.enabled {
            model_config.tiers.as_ref().map(|tiers| {
                ModelDispatcher::new(
                    tiers.clone(),
                    model_config.auto_dispatch.clone(),
                    0,
                )
            })
        } else {
            None
        };

        assert!(dispatcher.is_none());
    }
}
