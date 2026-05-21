use std::time::Duration;

use hakimi_common::HakimiError;
use rand::Rng;

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
