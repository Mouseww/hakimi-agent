//! Performance metrics collection for Hakimi Agent
//!
//! This module provides structured metrics collection for observability,
//! including latency, token counts, and tool execution times.

use serde::{Deserialize, Serialize};
use std::time::{Duration, Instant};

/// Metrics for a single agent conversation turn
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ConversationMetrics {
    /// Total duration of the conversation from start to finish
    pub total_duration_ms: u64,

    /// Number of API calls made to the LLM provider
    pub api_call_count: usize,

    /// Total tokens used (prompt + completion)
    pub total_tokens: usize,

    /// Prompt tokens sent to the model
    pub prompt_tokens: usize,

    /// Completion tokens received from the model
    pub completion_tokens: usize,

    /// Number of tool calls executed
    pub tool_call_count: usize,

    /// Total time spent executing tools
    pub tool_execution_duration_ms: u64,

    /// Individual tool execution metrics
    pub tool_metrics: Vec<ToolMetric>,

    /// Whether the conversation hit the iteration limit
    pub hit_iteration_limit: bool,

    /// Whether the conversation hit the token budget limit
    pub hit_token_limit: bool,
}

/// Metrics for a single tool execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolMetric {
    /// Name of the tool that was called
    pub tool_name: String,

    /// Duration of the tool execution
    pub duration_ms: u64,

    /// Whether the tool execution succeeded
    pub success: bool,

    /// Timestamp when the tool was called (relative to conversation start)
    pub timestamp_offset_ms: u64,
}

impl ConversationMetrics {
    /// Create a new metrics instance
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a tool execution
    pub fn record_tool_execution(
        &mut self,
        tool_name: impl Into<String>,
        duration: Duration,
        success: bool,
        timestamp_offset: Duration,
    ) {
        self.tool_call_count += 1;
        self.tool_execution_duration_ms += duration.as_millis() as u64;
        self.tool_metrics.push(ToolMetric {
            tool_name: tool_name.into(),
            duration_ms: duration.as_millis() as u64,
            success,
            timestamp_offset_ms: timestamp_offset.as_millis() as u64,
        });
    }

    /// Record token usage from an API call
    pub fn record_token_usage(&mut self, prompt: usize, completion: usize) {
        self.prompt_tokens += prompt;
        self.completion_tokens += completion;
        self.total_tokens += prompt + completion;
    }

    /// Record an API call
    pub fn record_api_call(&mut self) {
        self.api_call_count += 1;
    }

    /// Finalize the metrics with the total duration
    pub fn finalize(&mut self, total_duration: Duration) {
        self.total_duration_ms = total_duration.as_millis() as u64;
    }

    /// Get average tool execution time
    pub fn avg_tool_execution_ms(&self) -> f64 {
        if self.tool_call_count == 0 {
            0.0
        } else {
            self.tool_execution_duration_ms as f64 / self.tool_call_count as f64
        }
    }

    /// Get average tokens per API call
    pub fn avg_tokens_per_call(&self) -> f64 {
        if self.api_call_count == 0 {
            0.0
        } else {
            self.total_tokens as f64 / self.api_call_count as f64
        }
    }
}

/// Timer for measuring durations
pub struct MetricsTimer {
    start: Instant,
}

impl MetricsTimer {
    /// Start a new timer
    pub fn start() -> Self {
        Self {
            start: Instant::now(),
        }
    }

    /// Get the elapsed time since the timer started
    pub fn elapsed(&self) -> Duration {
        self.start.elapsed()
    }

    /// Stop the timer and return the duration
    pub fn stop(self) -> Duration {
        self.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_conversation_metrics_new() {
        let metrics = ConversationMetrics::new();
        assert_eq!(metrics.total_duration_ms, 0);
        assert_eq!(metrics.api_call_count, 0);
        assert_eq!(metrics.tool_call_count, 0);
    }

    #[test]
    fn test_record_tool_execution() {
        let mut metrics = ConversationMetrics::new();
        metrics.record_tool_execution(
            "test_tool",
            Duration::from_millis(100),
            true,
            Duration::from_millis(50),
        );

        assert_eq!(metrics.tool_call_count, 1);
        assert_eq!(metrics.tool_execution_duration_ms, 100);
        assert_eq!(metrics.tool_metrics.len(), 1);
        assert_eq!(metrics.tool_metrics[0].tool_name, "test_tool");
        assert_eq!(metrics.tool_metrics[0].duration_ms, 100);
        assert!(metrics.tool_metrics[0].success);
    }

    #[test]
    fn test_record_token_usage() {
        let mut metrics = ConversationMetrics::new();
        metrics.record_token_usage(100, 50);

        assert_eq!(metrics.prompt_tokens, 100);
        assert_eq!(metrics.completion_tokens, 50);
        assert_eq!(metrics.total_tokens, 150);
    }

    #[test]
    fn test_avg_tool_execution_ms() {
        let mut metrics = ConversationMetrics::new();
        metrics.record_tool_execution(
            "tool1",
            Duration::from_millis(100),
            true,
            Duration::from_millis(0),
        );
        metrics.record_tool_execution(
            "tool2",
            Duration::from_millis(200),
            true,
            Duration::from_millis(100),
        );

        assert_eq!(metrics.avg_tool_execution_ms(), 150.0);
    }

    #[test]
    fn test_metrics_timer() {
        let timer = MetricsTimer::start();
        std::thread::sleep(Duration::from_millis(10));
        let duration = timer.stop();
        assert!(duration.as_millis() >= 10);
    }
}
