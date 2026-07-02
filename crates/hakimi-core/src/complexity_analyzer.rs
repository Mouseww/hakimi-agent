//! Complexity analyzer — evaluates task difficulty and recommends model tier.

use hakimi_common::Message;

use crate::model_dispatch::{
    AutoDispatchConfig, ComplexityFactor, DispatchThresholds, ModelTier, ModelTiers, TaskComplexity,
};

/// Analyzer that evaluates task complexity and recommends model tier.
/// Analyzes task complexity to determine appropriate model tier.
#[derive(Clone)]
pub struct ComplexityAnalyzer {
    thresholds: DispatchThresholds,
    has_light: bool,
    has_reasoning: bool,
}

impl ComplexityAnalyzer {
    /// Create a new analyzer from config and available tiers.
    pub fn new(config: &AutoDispatchConfig, tiers: &ModelTiers) -> Self {
        Self {
            thresholds: config.thresholds.clone(),
            has_light: tiers.light.is_some(),
            has_reasoning: tiers.reasoning.is_some(),
        }
    }

    /// Analyze task complexity and recommend model tier.
    ///
    /// # Arguments
    /// * `message` - User message to analyze
    /// * `history` - Conversation history (affects context complexity)
    /// * `depth` - Agent nesting depth (0 = main, 1 = child, 2 = grandchild, ...)
    pub fn analyze(&self, message: &str, history: &[Message], depth: usize) -> TaskComplexity {
        let mut factors = Vec::new();

        // 1. Task type analysis (30% weight)
        let task_type_score = Self::analyze_task_type(message);
        factors.push(ComplexityFactor::new("任务类型", task_type_score, 0.30));

        // 2. Context complexity (20% weight)
        let context_score = Self::analyze_context(message, history);
        factors.push(ComplexityFactor::new("上下文需求", context_score, 0.20));

        // 3. Reasoning depth (25% weight)
        let reasoning_score = Self::analyze_reasoning_depth(message);
        factors.push(ComplexityFactor::new("推理深度", reasoning_score, 0.25));

        // 4. Tool call prediction (15% weight)
        let tool_score = Self::predict_tool_complexity(message);
        factors.push(ComplexityFactor::new("工具调用复杂度", tool_score, 0.15));

        // 5. Depth penalty (10% negative weight — child agents prefer lighter models)
        let depth_penalty = (depth as u8).min(3);
        if depth > 0 {
            factors.push(ComplexityFactor::new("嵌套深度修正", depth_penalty, -0.10));
        }

        // Weighted sum
        let weighted_score: f32 = factors.iter().map(|f| f.score as f32 * f.weight).sum();

        let score = weighted_score.clamp(0.0, 10.0).round() as u8;

        // Select tier based on thresholds
        let recommended_tier = self.select_tier(score);

        // Generate reasoning explanation
        let reasoning = Self::generate_reasoning(&factors, score, recommended_tier);

        TaskComplexity {
            score,
            factors,
            recommended_tier,
            reasoning,
        }
    }

    /// Analyze task type from message content.
    fn analyze_task_type(msg: &str) -> u8 {
        let msg_lower = msg.to_lowercase();

        // Simple query (1-2)
        if msg_lower.len() < 50
            && (msg_lower.contains("什么是")
                || msg_lower.contains("查看")
                || msg_lower.contains("列出")
                || msg_lower.contains("show")
                || msg_lower.contains("list")
                || msg_lower.contains("what is"))
        {
            return 1;
        }

        // File operations (3-5)
        if msg_lower.contains("读取")
            || msg_lower.contains("搜索")
            || msg_lower.contains("查找")
            || msg_lower.contains("read")
            || msg_lower.contains("search")
            || msg_lower.contains("find")
        {
            return 4;
        }

        // Code refactoring (6-7)
        if msg_lower.contains("重构")
            || msg_lower.contains("优化")
            || msg_lower.contains("改进")
            || msg_lower.contains("修改")
            || msg_lower.contains("refactor")
            || msg_lower.contains("optimize")
            || msg_lower.contains("improve")
        {
            return 6;
        }

        // System design / architecture (8-10)
        if msg_lower.contains("设计")
            || msg_lower.contains("架构")
            || msg_lower.contains("实现")
            || msg_lower.contains("模型调度")
            || msg_lower.contains("系统")
            || msg_lower.contains("design")
            || msg_lower.contains("architecture")
            || msg_lower.contains("implement")
            || msg_lower.contains("system")
        {
            return 9;
        }

        // Default medium complexity
        5
    }

    /// Analyze context complexity based on history length and message size.
    fn analyze_context(msg: &str, history: &[Message]) -> u8 {
        let turns = history.len();
        let msg_length = msg.len();

        match (turns, msg_length) {
            (0..=2, 0..=100) => 1, // Short conversation + short message
            (0..=2, 101..=500) => 3,
            (0..=2, _) => 5,
            (3..=10, 0..=200) => 4,
            (3..=10, _) => 6,
            (11..=20, _) => 7,
            _ => 8, // Long conversation needs stronger context understanding
        }
    }

    /// Analyze reasoning depth requirements.
    fn analyze_reasoning_depth(msg: &str) -> u8 {
        let msg_lower = msg.to_lowercase();

        // Multi-step indicators
        let multi_step_keywords = [
            "首先", "然后", "接着", "最后", "步骤", "first", "then", "next", "finally", "step",
        ];
        let has_multi_step = multi_step_keywords.iter().any(|k| msg_lower.contains(k));

        // Deep reasoning indicators
        let reasoning_keywords = [
            "为什么",
            "如何实现",
            "最佳实践",
            "trade-off",
            "权衡",
            "比较",
            "分析",
            "评估",
            "why",
            "how to",
            "best practice",
            "compare",
            "analyze",
            "evaluate",
        ];
        let has_deep_reasoning = reasoning_keywords.iter().any(|k| msg_lower.contains(k));

        match (has_multi_step, has_deep_reasoning) {
            (true, true) => 9,   // Multi-step + deep reasoning
            (true, false) => 6,  // Multi-step only
            (false, true) => 7,  // Deep reasoning only
            (false, false) => 3, // Simple reasoning
        }
    }

    /// Predict tool call complexity.
    fn predict_tool_complexity(msg: &str) -> u8 {
        let msg_lower = msg.to_lowercase();

        let mut tool_count = 0;

        // Search tools
        if msg_lower.contains("搜索") || msg_lower.contains("search") {
            tool_count += 1;
        }

        // Read tools
        if msg_lower.contains("读取") || msg_lower.contains("read") || msg_lower.contains("查看")
        {
            tool_count += 1;
        }

        // Edit tools
        if msg_lower.contains("修改") || msg_lower.contains("edit") || msg_lower.contains("更改")
        {
            tool_count += 1;
        }

        // Execution tools
        if msg_lower.contains("运行")
            || msg_lower.contains("执行")
            || msg_lower.contains("run")
            || msg_lower.contains("execute")
        {
            tool_count += 1;
        }

        // Batch/loop operations (multiply tool count)
        if msg_lower.contains("所有")
            || msg_lower.contains("每个")
            || msg_lower.contains("all")
            || msg_lower.contains("each")
            || msg_lower.contains("批量")
        {
            tool_count += 3; // Batch operations need many tool calls
        }

        match tool_count {
            0..=1 => 2,
            2..=3 => 5,
            4..=5 => 7,
            _ => 9,
        }
    }

    /// Select tier based on score and available models.
    fn select_tier(&self, score: u8) -> ModelTier {
        if score <= self.thresholds.light && self.has_light {
            ModelTier::Light
        } else if score >= self.thresholds.reasoning && self.has_reasoning {
            ModelTier::Reasoning
        } else {
            ModelTier::Primary
        }
    }

    /// Generate human-readable reasoning explanation.
    fn generate_reasoning(factors: &[ComplexityFactor], score: u8, tier: ModelTier) -> String {
        let factor_details: Vec<String> = factors
            .iter()
            .filter(|f| f.score > 0)
            .map(|f| format!("{}: {}/10", f.name, f.score))
            .collect();

        format!(
            "📊 复杂度评分: {}/10\n💡 评估因素: {}\n🎯 调度决策: {}",
            score,
            factor_details.join(", "),
            tier.display_name()
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_config() -> AutoDispatchConfig {
        AutoDispatchConfig::default()
    }

    fn mock_tiers() -> ModelTiers {
        ModelTiers {
            primary: crate::model_dispatch::TierConfig {
                provider: "test".into(),
                model: "primary-model".into(),
                api_key: String::new(),
                base_url: String::new(),
            },
            light: Some(crate::model_dispatch::TierConfig {
                provider: "test".into(),
                model: "light-model".into(),
                api_key: String::new(),
                base_url: String::new(),
            }),
            reasoning: Some(crate::model_dispatch::TierConfig {
                provider: "test".into(),
                model: "reasoning-model".into(),
                api_key: String::new(),
                base_url: String::new(),
            }),
        }
    }

    #[test]
    fn test_simple_query() {
        let analyzer = ComplexityAnalyzer::new(&mock_config(), &mock_tiers());
        let result = analyzer.analyze("什么是 Rust?", &[], 0);

        assert!(result.score <= 3);
        assert_eq!(result.recommended_tier, ModelTier::Light);
    }

    #[test]
    fn test_complex_architecture() {
        let analyzer = ComplexityAnalyzer::new(&mock_config(), &mock_tiers());
        let result = analyzer.analyze("设计一个智能模型调度系统，支持三层模型和递归委派", &[], 0);

        assert!(result.score >= 8);
        assert_eq!(result.recommended_tier, ModelTier::Reasoning);
    }

    #[test]
    fn test_depth_penalty() {
        let analyzer = ComplexityAnalyzer::new(&mock_config(), &mock_tiers());

        // Same message, different depths
        let depth0 = analyzer.analyze("设计系统", &[], 0);
        let depth2 = analyzer.analyze("设计系统", &[], 2);

        // Deeper agent should have lower score
        assert!(depth2.score < depth0.score);
    }
}
