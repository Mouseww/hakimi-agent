//! Model dispatcher — resolves which model tier to use for a given task.

use anyhow::Result;
use hakimi_common::Message;
use hakimi_config::{AutoDispatchConfig, ModelTiers, TierConfig};

use crate::complexity_analyzer::ComplexityAnalyzer;
use crate::model_dispatch::{ModelTier, TaskComplexity};

/// Resolves model selection based on task complexity.
#[derive(Clone)]
pub struct ModelDispatcher {
    tiers: ModelTiers,
    config: AutoDispatchConfig,
    analyzer: ComplexityAnalyzer,
    depth: usize,
}

impl ModelDispatcher {
    /// Create a new dispatcher from config.
    pub fn new(tiers: ModelTiers, config: AutoDispatchConfig, depth: usize) -> Self {
        let analyzer = ComplexityAnalyzer::new(&config, &tiers);
        Self {
            tiers,
            config,
            analyzer,
            depth,
        }
    }

    /// Analyze task and select appropriate model tier.
    pub fn select_model(&self, message: &str, history: &[Message]) -> (TierConfig, TaskComplexity) {
        let complexity = self.analyzer.analyze(message, history, self.depth);

        let tier_config = match complexity.recommended_tier {
            ModelTier::Light => self
                .tiers
                .light
                .as_ref()
                .unwrap_or(&self.tiers.primary)
                .clone(),
            ModelTier::Primary => self.tiers.primary.clone(),
            ModelTier::Reasoning => self
                .tiers
                .reasoning
                .as_ref()
                .unwrap_or(&self.tiers.primary)
                .clone(),
        };

        (tier_config, complexity)
    }

    /// Check if two-stage execution is needed.
    pub fn should_use_two_stage(&self, complexity: &TaskComplexity) -> bool {
        self.config.two_stage.enabled
            && complexity.recommended_tier == ModelTier::Reasoning
            && self.tiers.reasoning.is_some()
    }

    /// Get reasoning tier config for two-stage execution.
    pub fn reasoning_tier(&self) -> Option<&TierConfig> {
        self.tiers.reasoning.as_ref()
    }

    /// Get primary tier config for two-stage execution.
    pub fn primary_tier(&self) -> &TierConfig {
        &self.tiers.primary
    }

    /// Get primary tier config (same as primary_tier, for consistency).
    pub fn primary_tier_config(&self) -> &TierConfig {
        &self.tiers.primary
    }

    /// Check if dispatch decision should be shown to user.
    pub fn should_show_decision(&self) -> bool {
        self.config.show_dispatch_decision
    }

    /// Format dispatch decision for user display.
    pub fn format_decision(&self, complexity: &TaskComplexity, tier_config: &TierConfig) -> String {
        let tier_emoji = complexity.recommended_tier.emoji();
        let tier_name = complexity.recommended_tier.display_name();
        format!(
            "{} **模型调度决策: {}**\n\n{}\n\n📦 选用模型: `{}/{}`",
            tier_emoji, tier_name, complexity.reasoning, tier_config.provider, tier_config.model
        )
    }
}

/// Helper to build dispatcher from HakimiConfig model section.
pub fn build_dispatcher_from_config(
    model_config: &hakimi_config::ModelConfig,
    depth: usize,
) -> Result<Option<ModelDispatcher>> {
    // Dispatch disabled or no tiers configured
    if !model_config.auto_dispatch.enabled {
        return Ok(None);
    }

    let Some(ref tiers) = model_config.tiers else {
        // No tiers configured, fall back to single-model mode
        return Ok(None);
    };

    Ok(Some(ModelDispatcher::new(
        tiers.clone(),
        model_config.auto_dispatch.clone(),
        depth,
    )))
}
