//! Model dispatch — intelligent model tier selection based on task complexity.
//!
//! Provides automatic routing between light/primary/reasoning models based on
//! analyzed task characteristics. Supports recursive dispatch for delegated
//! sub-agents and team consultations.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// Re-export config types from hakimi-config
pub use hakimi_config::{
    AutoDispatchConfig, DispatchThresholds, ModelTiers, TierConfig, TwoStageConfig,
};

/// Model tier enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
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

    pub fn emoji(&self) -> &'static str {
        match self {
            Self::Light => "💡",
            Self::Primary => "🚀",
            Self::Reasoning => "🧠",
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

// ═══════════════════════════════════════════════════════════════════════════
// Learning & Feedback Types
// ═══════════════════════════════════════════════════════════════════════════

/// Historical dispatch record for learning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchRecord {
    /// Timestamp of this dispatch.
    pub timestamp: DateTime<Utc>,

    /// User input message that triggered dispatch.
    pub input: String,

    /// Initially predicted tier by classifier.
    pub predicted_tier: ModelTier,

    /// Actually used tier (after upgrades/downgrades).
    pub actual_tier: ModelTier,

    /// Task complexity analysis.
    pub complexity_score: u8,

    /// Whether the task succeeded without errors.
    pub success: bool,

    /// Task duration in milliseconds.
    pub duration_ms: u64,

    /// User feedback (if provided).
    pub user_feedback: Option<UserFeedback>,

    /// Number of automatic upgrades during execution.
    pub upgrade_count: u8,

    /// Agent nesting depth (0 = main, 1 = child, ...).
    pub depth: usize,
}

impl DispatchRecord {
    /// Create a new dispatch record.
    pub fn new(
        input: String,
        predicted_tier: ModelTier,
        actual_tier: ModelTier,
        complexity_score: u8,
        depth: usize,
    ) -> Self {
        Self {
            timestamp: Utc::now(),
            input,
            predicted_tier,
            actual_tier,
            complexity_score,
            success: false,
            duration_ms: 0,
            user_feedback: None,
            upgrade_count: 0,
            depth,
        }
    }

    /// Mark task as successful with duration.
    pub fn mark_success(&mut self, duration_ms: u64) {
        self.success = true;
        self.duration_ms = duration_ms;
    }

    /// Mark task as failed.
    pub fn mark_failed(&mut self, duration_ms: u64) {
        self.success = false;
        self.duration_ms = duration_ms;
    }

    /// Record an automatic upgrade.
    pub fn record_upgrade(&mut self, to_tier: ModelTier) {
        self.upgrade_count += 1;
        self.actual_tier = to_tier;
    }

    /// Apply user feedback.
    pub fn apply_feedback(&mut self, feedback: UserFeedback) {
        self.user_feedback = Some(feedback);
    }
}

/// User feedback on dispatch decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UserFeedback {
    /// User indicated task was too simple for chosen tier (/lighter).
    TooHeavy,
    /// User indicated task was too complex for chosen tier (/stronger).
    TooLight,
    /// User confirmed tier choice was appropriate (/justright).
    JustRight,
}

impl UserFeedback {
    /// Parse feedback from slash command.
    pub fn from_command(cmd: &str) -> Option<Self> {
        match cmd.to_lowercase().as_str() {
            "/lighter" | "/轻量" => Some(Self::TooHeavy),
            "/stronger" | "/增强" => Some(Self::TooLight),
            "/justright" | "/刚好" => Some(Self::JustRight),
            _ => None,
        }
    }

    /// Parse feedback from inline button callback data.
    /// Used for Telegram inline buttons, Discord components, etc.
    pub fn from_callback(data: &str) -> Option<Self> {
        match data {
            "dispatch_lighter" | "👎" => Some(Self::TooHeavy),
            "dispatch_stronger" | "💪" => Some(Self::TooLight),
            "dispatch_justright" | "👍" => Some(Self::JustRight),
            _ => None,
        }
    }

    /// Get user-friendly message for this feedback.
    pub fn message(&self) -> &'static str {
        match self {
            Self::TooHeavy => "✅ 已记录：下次类似任务将尝试更轻量的模型",
            Self::TooLight => "✅ 已记录：下次类似任务将使用更强的模型",
            Self::JustRight => "✅ 已记录：保持当前调度策略",
        }
    }

    /// Get emoji representation for this feedback type.
    pub fn emoji(&self) -> &'static str {
        match self {
            Self::TooHeavy => "👎",
            Self::TooLight => "💪",
            Self::JustRight => "👍",
        }
    }

    /// Get button text for UI rendering.
    pub fn button_text(&self) -> &'static str {
        match self {
            Self::TooHeavy => "太重了",
            Self::TooLight => "太弱了",
            Self::JustRight => "刚刚好",
        }
    }

    /// Get callback data for inline buttons (Telegram/Discord).
    pub fn callback_data(&self) -> &'static str {
        match self {
            Self::TooHeavy => "dispatch_lighter",
            Self::TooLight => "dispatch_stronger",
            Self::JustRight => "dispatch_justright",
        }
    }

    /// Generate Telegram inline keyboard markup JSON.
    /// Returns a serializable structure for teloxide InlineKeyboardMarkup.
    pub fn telegram_inline_buttons() -> String {
        r#"{"inline_keyboard":[[
            {"text":"👍 刚刚好","callback_data":"dispatch_justright"},
            {"text":"👎 太重了","callback_data":"dispatch_lighter"},
            {"text":"💪 太弱了","callback_data":"dispatch_stronger"}
        ]]}"#
        .to_string()
    }

    /// Generate Discord message components JSON.
    pub fn discord_action_row() -> String {
        r#"{"type":1,"components":[
            {"type":2,"style":3,"label":"👍 刚刚好","custom_id":"dispatch_justright"},
            {"type":2,"style":4,"label":"👎 太重了","custom_id":"dispatch_lighter"},
            {"type":2,"style":1,"label":"💪 太弱了","custom_id":"dispatch_stronger"}
        ]}"#
        .to_string()
    }
}

/// Dispatch statistics for reporting.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DispatchStats {
    /// Total number of dispatches.
    pub total_dispatches: usize,

    /// Correct predictions (predicted == actual && success).
    pub correct_predictions: usize,

    /// Prediction accuracy (0.0 - 1.0).
    pub accuracy: f32,

    /// Count by tier.
    pub light_count: usize,
    pub primary_count: usize,
    pub reasoning_count: usize,

    /// Automatic upgrades count.
    pub total_upgrades: usize,

    /// Average task duration by tier (ms).
    pub avg_duration_light: u64,
    pub avg_duration_primary: u64,
    pub avg_duration_reasoning: u64,
}

impl DispatchStats {
    /// Calculate statistics from history.
    pub fn from_history(history: &[DispatchRecord]) -> Self {
        let mut stats = Self::default();
        stats.total_dispatches = history.len();

        if history.is_empty() {
            return stats;
        }

        let mut light_durations = Vec::new();
        let mut primary_durations = Vec::new();
        let mut reasoning_durations = Vec::new();

        for record in history {
            // Count by actual tier
            match record.actual_tier {
                ModelTier::Light => {
                    stats.light_count += 1;
                    light_durations.push(record.duration_ms);
                }
                ModelTier::Primary => {
                    stats.primary_count += 1;
                    primary_durations.push(record.duration_ms);
                }
                ModelTier::Reasoning => {
                    stats.reasoning_count += 1;
                    reasoning_durations.push(record.duration_ms);
                }
            }

            // Count correct predictions
            if record.predicted_tier == record.actual_tier && record.success {
                stats.correct_predictions += 1;
            }

            // Count upgrades
            stats.total_upgrades += record.upgrade_count as usize;
        }

        // Calculate accuracy
        stats.accuracy = stats.correct_predictions as f32 / stats.total_dispatches as f32;

        // Calculate average durations
        if !light_durations.is_empty() {
            stats.avg_duration_light = light_durations.iter().sum::<u64>() / light_durations.len() as u64;
        }
        if !primary_durations.is_empty() {
            stats.avg_duration_primary = primary_durations.iter().sum::<u64>() / primary_durations.len() as u64;
        }
        if !reasoning_durations.is_empty() {
            stats.avg_duration_reasoning =
                reasoning_durations.iter().sum::<u64>() / reasoning_durations.len() as u64;
        }

        stats
    }

    /// Format as human-readable report.
    pub fn format_report(&self) -> String {
        format!(
            "📊 **调度统计报告**\n\n\
             总调度次数: {}\n\
             准确率: {:.1}%\n\n\
             **使用分布**\n\
             💡 轻量模型: {} ({:.1}%)\n\
             🚀 主力模型: {} ({:.1}%)\n\
             🧠 高级思考: {} ({:.1}%)\n\n\
             自动升级次数: {}\n\n\
             **平均耗时**\n\
             💡 轻量: {}ms\n\
             🚀 主力: {}ms\n\
             🧠 思考: {}ms",
            self.total_dispatches,
            self.accuracy * 100.0,
            self.light_count,
            self.light_count as f32 / self.total_dispatches as f32 * 100.0,
            self.primary_count,
            self.primary_count as f32 / self.total_dispatches as f32 * 100.0,
            self.reasoning_count,
            self.reasoning_count as f32 / self.total_dispatches as f32 * 100.0,
            self.total_upgrades,
            self.avg_duration_light,
            self.avg_duration_primary,
            self.avg_duration_reasoning,
        )
    }
}

