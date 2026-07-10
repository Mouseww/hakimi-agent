//! Retry strategies for cron job execution.

use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Retry strategy for failed cron jobs.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RetryStrategy {
    /// Fixed interval between retries.
    FixedInterval { interval_secs: u64 },

    /// Exponential backoff with configurable multiplier and cap.
    ExponentialBackoff {
        initial_interval_secs: u64,
        max_interval_secs: u64,
        multiplier: f64,
    },

    /// Custom sequence of intervals.
    CustomIntervals { intervals_secs: Vec<u64> },

    /// No retry - fail immediately.
    NoRetry,
}

impl RetryStrategy {
    /// Calculate the delay before the next retry attempt.
    ///
    /// # Arguments
    /// * `attempt` - Zero-based attempt number (0 = first retry).
    ///
    /// # Returns
    /// `Some(duration)` if retry should happen, `None` if no more retries.
    pub fn next_retry_delay(&self, attempt: usize) -> Option<Duration> {
        match self {
            RetryStrategy::FixedInterval { interval_secs } => {
                Some(Duration::from_secs(*interval_secs))
            }

            RetryStrategy::ExponentialBackoff {
                initial_interval_secs,
                max_interval_secs,
                multiplier,
            } => {
                let delay = (*initial_interval_secs as f64) * multiplier.powi(attempt as i32);
                let delay = delay.min(*max_interval_secs as f64) as u64;
                Some(Duration::from_secs(delay))
            }

            RetryStrategy::CustomIntervals { intervals_secs } => intervals_secs
                .get(attempt)
                .copied()
                .map(Duration::from_secs),

            RetryStrategy::NoRetry => None,
        }
    }
}

impl Default for RetryStrategy {
    fn default() -> Self {
        RetryStrategy::ExponentialBackoff {
            initial_interval_secs: 60,
            max_interval_secs: 3600,
            multiplier: 2.0,
        }
    }
}

/// Configuration for retry behavior.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RetryConfig {
    /// The retry strategy to use.
    pub strategy: RetryStrategy,

    /// Maximum number of attempts (including the initial attempt).
    /// For example, max_attempts=3 means 1 initial + 2 retries.
    pub max_attempts: usize,

    /// Only retry on errors matching these patterns (case-insensitive).
    /// Empty list means retry on all errors.
    #[serde(default)]
    pub retry_on_errors: Vec<String>,
}

impl RetryConfig {
    /// Create a new retry config with sensible defaults.
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if an error should trigger a retry.
    pub fn should_retry_error(&self, error: &str) -> bool {
        if self.retry_on_errors.is_empty() {
            return true; // Retry all errors
        }

        let error_lower = error.to_lowercase();
        self.retry_on_errors
            .iter()
            .any(|pattern| error_lower.contains(&pattern.to_lowercase()))
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            strategy: RetryStrategy::default(),
            max_attempts: 3,
            retry_on_errors: vec![
                "NetworkError".to_string(),
                "TimeoutError".to_string(),
                "ConnectionError".to_string(),
                "TemporaryFailure".to_string(),
            ],
        }
    }
}

/// Status of a single retry attempt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AttemptStatus {
    /// Attempt is currently running.
    Running,
    /// Attempt succeeded.
    Success,
    /// Attempt failed.
    Failed,
    /// Attempt was cancelled.
    Cancelled,
}

/// Record of a single execution attempt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunAttempt {
    /// Attempt number (1-based).
    pub attempt_number: usize,

    /// When the attempt started (Unix timestamp).
    pub started_at: i64,

    /// When the attempt completed (Unix timestamp), if finished.
    pub completed_at: Option<i64>,

    /// Status of this attempt.
    pub status: AttemptStatus,

    /// Error message if failed.
    pub error: Option<String>,

    /// Duration in milliseconds, if completed.
    pub duration_ms: Option<u64>,
}

impl RunAttempt {
    /// Create a new attempt record.
    pub fn new(attempt_number: usize) -> Self {
        Self {
            attempt_number,
            started_at: chrono::Utc::now().timestamp(),
            completed_at: None,
            status: AttemptStatus::Running,
            error: None,
            duration_ms: None,
        }
    }

    /// Mark the attempt as completed with the given status.
    pub fn complete(&mut self, status: AttemptStatus, error: Option<String>) {
        let now = chrono::Utc::now().timestamp();
        self.completed_at = Some(now);
        self.status = status;
        self.error = error;
        self.duration_ms = Some(((now - self.started_at) * 1000) as u64);
    }
}

/// Overall status of a job run across all attempts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum RunStatus {
    /// Job is currently running (may be on a retry).
    Running,
    /// Job succeeded on one of the attempts.
    Success,
    /// Job failed after exhausting all retry attempts.
    FailedAfterRetries,
    /// Job was cancelled before completion.
    Cancelled,
}

/// Complete record of a job run with all attempts.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CronJobRun {
    /// Unique run identifier.
    pub id: String,

    /// ID of the job that was executed.
    pub job_id: String,

    /// When the run started (Unix timestamp).
    pub started_at: i64,

    /// When the run completed (Unix timestamp), if finished.
    pub completed_at: Option<i64>,

    /// Overall status of the run.
    pub status: RunStatus,

    /// All execution attempts.
    pub attempts: Vec<RunAttempt>,

    /// Final error message if failed.
    pub error: Option<String>,

    /// Total duration in milliseconds, if completed.
    pub duration_ms: Option<u64>,
}

impl CronJobRun {
    /// Create a new run record.
    pub fn new(job_id: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            job_id: job_id.into(),
            started_at: chrono::Utc::now().timestamp(),
            completed_at: None,
            status: RunStatus::Running,
            attempts: Vec::new(),
            error: None,
            duration_ms: None,
        }
    }

    /// Mark the run as completed with the given status.
    pub fn complete(&mut self, status: RunStatus, error: Option<String>) {
        let now = chrono::Utc::now().timestamp();
        self.completed_at = Some(now);
        self.status = status;
        self.error = error;
        self.duration_ms = Some(((now - self.started_at) * 1000) as u64);
    }

    /// Get the last attempt, if any.
    pub fn last_attempt(&self) -> Option<&RunAttempt> {
        self.attempts.last()
    }

    /// Get a mutable reference to the last attempt, if any.
    pub fn last_attempt_mut(&mut self) -> Option<&mut RunAttempt> {
        self.attempts.last_mut()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fixed_interval_strategy() {
        let strategy = RetryStrategy::FixedInterval { interval_secs: 30 };

        assert_eq!(strategy.next_retry_delay(0), Some(Duration::from_secs(30)));
        assert_eq!(strategy.next_retry_delay(1), Some(Duration::from_secs(30)));
        assert_eq!(strategy.next_retry_delay(5), Some(Duration::from_secs(30)));
    }

    #[test]
    fn test_exponential_backoff_strategy() {
        let strategy = RetryStrategy::ExponentialBackoff {
            initial_interval_secs: 10,
            max_interval_secs: 100,
            multiplier: 2.0,
        };

        assert_eq!(strategy.next_retry_delay(0), Some(Duration::from_secs(10)));
        assert_eq!(strategy.next_retry_delay(1), Some(Duration::from_secs(20)));
        assert_eq!(strategy.next_retry_delay(2), Some(Duration::from_secs(40)));
        assert_eq!(strategy.next_retry_delay(3), Some(Duration::from_secs(80)));
        // Capped at max
        assert_eq!(strategy.next_retry_delay(4), Some(Duration::from_secs(100)));
        assert_eq!(strategy.next_retry_delay(5), Some(Duration::from_secs(100)));
    }

    #[test]
    fn test_custom_intervals_strategy() {
        let strategy = RetryStrategy::CustomIntervals {
            intervals_secs: vec![10, 30, 60],
        };

        assert_eq!(strategy.next_retry_delay(0), Some(Duration::from_secs(10)));
        assert_eq!(strategy.next_retry_delay(1), Some(Duration::from_secs(30)));
        assert_eq!(strategy.next_retry_delay(2), Some(Duration::from_secs(60)));
        assert_eq!(strategy.next_retry_delay(3), None); // No more intervals
    }

    #[test]
    fn test_no_retry_strategy() {
        let strategy = RetryStrategy::NoRetry;

        assert_eq!(strategy.next_retry_delay(0), None);
        assert_eq!(strategy.next_retry_delay(1), None);
    }

    #[test]
    fn test_retry_config_should_retry_error() {
        let config = RetryConfig {
            strategy: RetryStrategy::default(),
            max_attempts: 3,
            retry_on_errors: vec!["NetworkError".to_string(), "Timeout".to_string()],
        };

        assert!(config.should_retry_error("NetworkError: connection failed"));
        assert!(config.should_retry_error("Operation timeout"));
        assert!(!config.should_retry_error("InvalidInput: bad parameter"));
    }

    #[test]
    fn test_retry_config_empty_patterns_retries_all() {
        let config = RetryConfig {
            strategy: RetryStrategy::default(),
            max_attempts: 3,
            retry_on_errors: vec![],
        };

        assert!(config.should_retry_error("any error message"));
    }

    #[test]
    fn test_run_attempt_lifecycle() {
        let mut attempt = RunAttempt::new(1);

        assert_eq!(attempt.attempt_number, 1);
        assert_eq!(attempt.status, AttemptStatus::Running);
        assert!(attempt.completed_at.is_none());

        attempt.complete(AttemptStatus::Success, None);

        assert_eq!(attempt.status, AttemptStatus::Success);
        assert!(attempt.completed_at.is_some());
        assert!(attempt.duration_ms.is_some());
    }

    #[test]
    fn test_cron_job_run_lifecycle() {
        let mut run = CronJobRun::new("job-123");

        assert_eq!(run.job_id, "job-123");
        assert_eq!(run.status, RunStatus::Running);
        assert!(run.attempts.is_empty());

        let attempt = RunAttempt::new(1);
        run.attempts.push(attempt);

        assert_eq!(run.last_attempt().unwrap().attempt_number, 1);

        run.complete(RunStatus::Success, None);

        assert_eq!(run.status, RunStatus::Success);
        assert!(run.completed_at.is_some());
        assert!(run.duration_ms.is_some());
    }
}
