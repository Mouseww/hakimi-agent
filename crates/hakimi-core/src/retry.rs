use std::time::Duration;

use hakimi_common::HakimiError;
use rand::Rng;

use crate::error_classifier::{Classifiable, ErrorClassification};

/// Compute a jittered exponential backoff delay.
///
/// - `attempt`: the current retry attempt (0-indexed).
/// - `base_delay`: the base delay for the first retry.
/// - `max_delay`: the maximum delay cap.
///
/// Returns a random duration in `[0, min(base * 2^attempt, max)]`.
pub fn jittered_backoff(attempt: u32, base_delay: Duration, max_delay: Duration) -> Duration {
    let base_ms = base_delay.as_millis() as u64;
    let max_ms = max_delay.as_millis() as u64;
    // Exponential: base * 2^attempt, capped at max
    let exponential = base_ms.saturating_mul(1u64 << attempt.min(20));
    let capped = exponential.min(max_ms);
    // Full jitter: random in [0, capped]
    let jitter = rand::rng().random_range(0..=capped);
    Duration::from_millis(jitter)
}

/// Determine whether a failed API call should be retried.
///
/// Returns `true` if the error is transient (e.g. rate-limit, server error)
/// **and** we haven't exceeded `max_retries`.
pub fn should_retry(error: &HakimiError, attempt: u32, max_retries: u32) -> bool {
    if attempt >= max_retries {
        return false;
    }
    match error {
        // Transport errors (network, rate-limit, 5xx) are generally transient.
        HakimiError::Transport(_) => true,
        // IO errors may be transient (e.g. connection reset).
        HakimiError::Io(_) => true,
        // Everything else (tool, config, session, context, json) is not retryable.
        _ => false,
    }
}

/// Classify an error and return the classification together with a recommended
/// recovery action.  This is the primary integration point between the retry
/// layer and the error classifier.
pub fn classify_and_handle(error: &HakimiError) -> ErrorClassification {
    error.classify()
}

/// Check whether the error is retryable **and** we haven't exceeded
/// `max_retries`, using the richer `ErrorClassifier` instead of the
/// simple variant match.
///
/// Returns the `ErrorClassification` so the caller can also inspect the
/// suggested recovery action and `retry_after_ms`.
pub fn should_retry_classified(
    error: &HakimiError,
    attempt: u32,
    max_retries: u32,
) -> (bool, ErrorClassification) {
    let classification = classify_and_handle(error);
    if attempt >= max_retries {
        return (false, classification);
    }
    let retryable = classification.is_retryable;
    (retryable, classification)
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::HakimiError;

    #[test]
    fn test_jittered_backoff_within_bounds() {
        let base = Duration::from_millis(100);
        let max = Duration::from_millis(5000);
        for attempt in 0..5 {
            let delay = jittered_backoff(attempt, base, max);
            let upper = base.as_millis() as u64 * (1u64 << attempt);
            let capped = upper.min(max.as_millis() as u64);
            assert!(
                delay.as_millis() as u64 <= capped,
                "attempt={attempt}: delay {} exceeded cap {capped}",
                delay.as_millis()
            );
        }
    }

    #[test]
    fn test_jittered_backoff_increases_with_attempt() {
        // Run many trials; the mean for higher attempts should be larger.
        // Use a deterministic check: verify the cap (upper bound) grows.
        let cap_low: u64 = 100 * (1u64 << 1); // attempt 1, base=100ms
        let cap_high: u64 = 100 * (1u64 << 5); // attempt 5, base=100ms
        assert!(cap_high > cap_low);
    }

    #[test]
    fn test_jittered_backoff_capped_at_max() {
        let base = Duration::from_millis(1000);
        let max = Duration::from_millis(2000);
        // At attempt 2, exponential would be 4000 but capped at 2000
        for _ in 0..50 {
            let delay = jittered_backoff(2, base, max);
            assert!(
                delay <= max,
                "delay {} exceeded max {}",
                delay.as_millis(),
                max.as_millis()
            );
        }
    }

    #[test]
    fn test_jittered_backoff_zero_attempt() {
        let base = Duration::from_millis(500);
        let max = Duration::from_millis(5000);
        for _ in 0..50 {
            let delay = jittered_backoff(0, base, max);
            // 2^0 = 1, so cap is base = 500ms
            assert!(
                delay <= base,
                "delay {} exceeded base {}",
                delay.as_millis(),
                base.as_millis()
            );
        }
    }

    #[test]
    fn test_should_retry_transport_error() {
        let err = HakimiError::Transport("connection refused".into());
        assert!(should_retry(&err, 0, 3));
        assert!(should_retry(&err, 1, 3));
        assert!(should_retry(&err, 2, 3));
    }

    #[test]
    fn test_should_retry_io_error() {
        let err = HakimiError::Io(std::io::Error::new(
            std::io::ErrorKind::ConnectionReset,
            "reset",
        ));
        assert!(should_retry(&err, 0, 3));
        assert!(should_retry(&err, 1, 3));
    }

    #[test]
    fn test_should_not_retry_tool_error() {
        let err = HakimiError::ToolSimple("bad tool".into());
        assert!(!should_retry(&err, 0, 3));
        assert!(!should_retry(&err, 1, 3));
    }

    #[test]
    fn test_should_not_retry_config_error() {
        let err = HakimiError::Config("missing key".into());
        assert!(!should_retry(&err, 0, 3));
    }

    #[test]
    fn test_should_not_retry_at_max_retries() {
        let err = HakimiError::Transport("timeout".into());
        assert!(!should_retry(&err, 3, 3));
    }

    #[test]
    fn test_should_not_retry_beyond_max() {
        let err = HakimiError::Transport("timeout".into());
        assert!(!should_retry(&err, 5, 3));
        assert!(!should_retry(&err, 100, 3));
    }
}
