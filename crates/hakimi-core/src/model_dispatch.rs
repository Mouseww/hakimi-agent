//! Model dispatch — intelligent model tier selection based on task complexity.
//!
//! Provides automatic routing between light/primary/reasoning models based on
//! analyzed task characteristics. Supports recursive dispatch for delegated
//! sub-agents and team consultations.

// Re-export config types from hakimi-config
pub use hakimi_config::{
    AutoDispatchConfig, DispatchThresholds, ModelTiers, TierConfig, TwoStageConfig,
};

/// Model tier enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelTier {
    /// Light model for simple tasks (queries, file reads).
    Light,
    /// Primary model for standard tasks.
    Primary,
    /// Reasoning model for complex planning (two-stage execution).
    Reasoning,
}

impl ModelTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Light => "light",
            Self::Primary => "primary",
            Self::Reasoning => "reasoning",
        }
    }

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::Light => "轻量模型",
            Self::Primary => "主力模型",
            Self::Reasoning => "高级思考模型 + 主力模型",
        }
    }
}

/// Complexity analysis result.
#[derive(Debug, Clone)]
pub struct TaskComplexity {
    /// Final complexity score (0-10).
    pub score: u8,

    /// Individual complexity factors with scores and weights.
    pub factors: Vec<ComplexityFactor>,

    /// Recommended model tier based on thresholds.
    pub recommended_tier: ModelTier,

    /// Human-readable reasoning explanation.
    pub reasoning: String,
}

/// Individual complexity factor contribution.
#[derive(Debug, Clone)]
pub struct ComplexityFactor {
    /// Factor name (e.g. "任务类型", "上下文需求").
    pub name: String,

    /// Factor score (0-10).
    pub score: u8,

    /// Weight in final score calculation (can be negative for penalties).
    pub weight: f32,
}

impl ComplexityFactor {
    pub fn new(name: impl Into<String>, score: u8, weight: f32) -> Self {
        Self {
            name: name.into(),
            score,
            weight,
        }
    }
}
