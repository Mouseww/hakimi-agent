//! Dispatch learner — learns from history and user feedback to optimize dispatch decisions.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::path::PathBuf;

use crate::model_dispatch::{DispatchRecord, DispatchStats, UserFeedback};

/// Learning engine for intelligent dispatch optimization.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DispatchLearner {
    /// Historical dispatch records (ring buffer).
    #[serde(default)]
    history: VecDeque<DispatchRecord>,

    /// Maximum history size (default: 1000).
    #[serde(default = "default_max_history")]
    max_history: usize,

    /// Path to persist history (optional).
    #[serde(skip)]
    persist_path: Option<PathBuf>,
}

fn default_max_history() -> usize {
    1000
}

impl Default for DispatchLearner {
    fn default() -> Self {
        Self::new()
    }
}

impl DispatchLearner {
    /// Create a new learner.
    pub fn new() -> Self {
        Self {
            history: VecDeque::new(),
            max_history: default_max_history(),
            persist_path: None,
        }
    }

    /// Create a learner with persistence to a file.
    pub fn with_persistence(path: PathBuf) -> Result<Self> {
        let mut learner = Self::new();
        learner.persist_path = Some(path.clone());

        // Try to load existing history
        if path.exists()
            && let Ok(data) = std::fs::read_to_string(&path)
            && let Ok(loaded) = serde_json::from_str::<Self>(&data)
        {
            learner.history = loaded.history;
            learner.max_history = loaded.max_history;
        }

        Ok(learner)
    }

    /// Record a new dispatch result.
    pub fn record(&mut self, record: DispatchRecord) {
        self.history.push_back(record);

        // Evict old records if over limit
        while self.history.len() > self.max_history {
            self.history.pop_front();
        }

        // Auto-persist if path is set
        if self.persist_path.is_some() {
            let _ = self.save();
        }
    }

    /// Apply user feedback to the most recent dispatch.
    pub fn apply_feedback(&mut self, feedback: UserFeedback) -> bool {
        if let Some(record) = self.history.back_mut() {
            record.apply_feedback(feedback);

            // Persist change
            if self.persist_path.is_some() {
                let _ = self.save();
            }

            eprintln!("{}", feedback.message());
            true
        } else {
            eprintln!("⚠️  没有可反馈的调度记录");
            false
        }
    }

    /// Apply user feedback to a specific dispatch by ID.
    pub fn apply_feedback_by_id(&mut self, dispatch_id: &str, feedback: UserFeedback) -> bool {
        for record in self.history.iter_mut().rev() {
            if record.id == dispatch_id {
                record.apply_feedback(feedback);

                // Persist change
                if self.persist_path.is_some() {
                    let _ = self.save();
                }

                tracing::info!(dispatch_id = %dispatch_id, feedback = ?feedback, "applied user feedback");
                return true;
            }
        }

        tracing::warn!(dispatch_id = %dispatch_id, "dispatch record not found for feedback");
        false
    }

    /// Get dispatch statistics.
    pub fn get_stats(&self) -> DispatchStats {
        DispatchStats::from_history(&self.history.iter().cloned().collect::<Vec<_>>())
    }

    /// Get recent history (last N records).
    pub fn recent_history(&self, count: usize) -> Vec<&DispatchRecord> {
        self.history.iter().rev().take(count).collect()
    }

    /// Find similar past tasks based on input keywords.
    pub fn find_similar_tasks(&self, input: &str, limit: usize) -> Vec<&DispatchRecord> {
        let input_lower = input.to_lowercase();
        let keywords: Vec<&str> = input_lower.split_whitespace().collect();

        let mut scored: Vec<(&DispatchRecord, usize)> = self
            .history
            .iter()
            .map(|record| {
                let record_lower = record.input.to_lowercase();
                let matches = keywords
                    .iter()
                    .filter(|kw| record_lower.contains(*kw))
                    .count();
                (record, matches)
            })
            .filter(|(_, score)| *score > 0)
            .collect();

        // Sort by relevance (descending)
        scored.sort_by(|a, b| b.1.cmp(&a.1));

        scored.into_iter().take(limit).map(|(r, _)| r).collect()
    }
    /// Analyze tier usage trends.
    pub fn analyze_trends(&self) -> TrendAnalysis {
        let stats = self.get_stats();

        // Get recent records as owned Vec
        let recent_records: Vec<DispatchRecord> =
            self.history.iter().rev().take(100).cloned().collect();

        let recent_stats = DispatchStats::from_history(&recent_records);

        TrendAnalysis {
            overall_accuracy: stats.accuracy,
            recent_accuracy: recent_stats.accuracy,
            light_usage: stats.light_count as f32 / stats.total_dispatches.max(1) as f32,
            primary_usage: stats.primary_count as f32 / stats.total_dispatches.max(1) as f32,
            reasoning_usage: stats.reasoning_count as f32 / stats.total_dispatches.max(1) as f32,
            upgrade_rate: stats.total_upgrades as f32 / stats.total_dispatches.max(1) as f32,
        }
    }

    /// Persist history to disk (if path is set).
    pub fn save(&self) -> Result<()> {
        if let Some(ref path) = self.persist_path {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            let data = serde_json::to_string_pretty(self)?;
            std::fs::write(path, data)?;
        }
        Ok(())
    }

    /// Clear all history.
    pub fn clear(&mut self) {
        self.history.clear();
        if self.persist_path.is_some() {
            let _ = self.save();
        }
    }

    /// Get total dispatch count.
    pub fn total_dispatches(&self) -> usize {
        self.history.len()
    }

    /// Check if learner has enough data for reliable statistics.
    pub fn has_sufficient_data(&self) -> bool {
        self.history.len() >= 20
    }
}

/// Trend analysis result.
#[derive(Debug, Clone)]
pub struct TrendAnalysis {
    /// Overall accuracy across all history.
    pub overall_accuracy: f32,

    /// Recent accuracy (last 100 records).
    pub recent_accuracy: f32,

    /// Tier usage rates (0.0 - 1.0).
    pub light_usage: f32,
    pub primary_usage: f32,
    pub reasoning_usage: f32,

    /// Automatic upgrade rate.
    pub upgrade_rate: f32,
}

impl TrendAnalysis {
    /// Format as human-readable report.
    pub fn format_report(&self) -> String {
        format!(
            "📈 **调度趋势分析**\n\n\
             准确率: {:.1}% (近期: {:.1}%)\n\n\
             **模型使用率**\n\
             💡 轻量: {:.1}%\n\
             🚀 主力: {:.1}%\n\
             🧠 思考: {:.1}%\n\n\
             自动升级率: {:.1}%",
            self.overall_accuracy * 100.0,
            self.recent_accuracy * 100.0,
            self.light_usage * 100.0,
            self.primary_usage * 100.0,
            self.reasoning_usage * 100.0,
            self.upgrade_rate * 100.0,
        )
    }

    /// Suggest optimization actions.
    pub fn suggest_optimizations(&self) -> Vec<String> {
        let mut suggestions = Vec::new();

        if self.recent_accuracy < 0.7 {
            suggestions.push("⚠️  近期准确率偏低，建议调整复杂度评分阈值".to_string());
        }

        if self.upgrade_rate > 0.3 {
            suggestions.push("⚠️  自动升级频繁，初始分类可能过于保守".to_string());
        }

        if self.light_usage < 0.1 && self.upgrade_rate < 0.1 {
            suggestions.push("💡 轻量模型使用率较低，可尝试提高轻量模型阈值".to_string());
        }

        if self.reasoning_usage > 0.4 {
            suggestions.push("🧠 高级思考模型使用率较高，可能增加成本".to_string());
        }

        if suggestions.is_empty() {
            suggestions.push("✅ 调度策略运行良好，暂无优化建议".to_string());
        }

        suggestions
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ModelTier;

    #[test]
    fn test_learner_basic() {
        let mut learner = DispatchLearner::new();
        assert_eq!(learner.total_dispatches(), 0);

        let record = DispatchRecord::new(
            "测试任务".to_string(),
            ModelTier::Primary,
            ModelTier::Primary,
            5,
            0,
        );

        learner.record(record);
        assert_eq!(learner.total_dispatches(), 1);
    }

    #[test]
    fn test_history_eviction() {
        let mut learner = DispatchLearner::new();
        learner.max_history = 3;

        for i in 0..5 {
            let record = DispatchRecord::new(
                format!("Task {}", i),
                ModelTier::Primary,
                ModelTier::Primary,
                5,
                0,
            );
            learner.record(record);
        }

        assert_eq!(learner.total_dispatches(), 3);
    }

    #[test]
    fn test_find_similar() {
        let mut learner = DispatchLearner::new();

        learner.record(DispatchRecord::new(
            "实现模型调度系统".to_string(),
            ModelTier::Reasoning,
            ModelTier::Reasoning,
            9,
            0,
        ));

        learner.record(DispatchRecord::new(
            "读取文件内容".to_string(),
            ModelTier::Light,
            ModelTier::Light,
            2,
            0,
        ));

        let similar = learner.find_similar_tasks("模型调度", 5);
        assert_eq!(similar.len(), 1);
        assert_eq!(similar[0].actual_tier, ModelTier::Reasoning);
    }
}
