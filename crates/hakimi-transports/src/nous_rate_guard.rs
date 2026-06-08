use crate::rate_limit::{RateLimitBucket, RateLimitState};
use hakimi_common::effective_hakimi_home;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::debug;

const STATE_SUBDIR: &str = "rate_limits";
const STATE_FILENAME: &str = "nous.json";
const MIN_RESET_FOR_BREAKER_SECONDS: f64 = 60.0;
const GUARD_PREFIX: &str = "Nous rate limit guard active";

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct PersistedNousRateLimit {
    reset_at: f64,
    recorded_at: f64,
    reset_seconds: f64,
}

pub fn is_nous_base_url(base_url: &str) -> bool {
    let base_url = base_url.trim().to_ascii_lowercase();
    base_url.contains("inference-api.nousresearch.com")
        || base_url.contains("api.nousresearch.com")
        || base_url.contains("portal.nousresearch.com")
}

pub fn active_limit_message(base_url: &str) -> Option<String> {
    if !is_nous_base_url(base_url) {
        return None;
    }
    remaining_for_path(&state_path(), now_seconds()).map(blocked_message)
}

pub fn record_genuine_limit(
    base_url: &str,
    current: Option<&RateLimitState>,
    previous: Option<&RateLimitState>,
) -> Option<String> {
    if !is_nous_base_url(base_url) {
        return None;
    }
    let reset_seconds = genuine_reset_seconds(current, previous)?;
    let now = now_seconds();
    let path = state_path();
    if let Err(err) = write_state(&path, now, reset_seconds) {
        debug!(error = %err, path = %path.display(), "failed to write Nous rate-limit guard state");
    }
    Some(blocked_message(reset_seconds))
}

pub fn clear_success(base_url: &str) {
    if is_nous_base_url(base_url) {
        let _ = std::fs::remove_file(state_path());
    }
}

pub fn is_guard_message(message: &str) -> bool {
    message.contains(GUARD_PREFIX)
}

fn state_path() -> PathBuf {
    effective_hakimi_home()
        .join(STATE_SUBDIR)
        .join(STATE_FILENAME)
}

fn genuine_reset_seconds(
    current: Option<&RateLimitState>,
    previous: Option<&RateLimitState>,
) -> Option<f64> {
    current
        .and_then(exhausted_reset_seconds)
        .or_else(|| previous.and_then(exhausted_reset_seconds))
}

fn exhausted_reset_seconds(state: &RateLimitState) -> Option<f64> {
    [
        &state.requests_hour,
        &state.requests_minute,
        &state.tokens_hour,
        &state.tokens_minute,
    ]
    .into_iter()
    .find_map(exhausted_bucket_seconds)
}

fn exhausted_bucket_seconds(bucket: &RateLimitBucket) -> Option<f64> {
    if bucket.limit == 0 || bucket.remaining > 0 {
        return None;
    }
    let remaining = bucket.remaining_seconds_now();
    (remaining >= MIN_RESET_FOR_BREAKER_SECONDS).then_some(remaining)
}

fn remaining_for_path(path: &Path, now: f64) -> Option<f64> {
    let raw = match std::fs::read_to_string(path) {
        Ok(raw) => raw,
        Err(_) => return None,
    };
    let state: PersistedNousRateLimit = match serde_json::from_str(&raw) {
        Ok(state) => state,
        Err(_) => {
            let _ = std::fs::remove_file(path);
            return None;
        }
    };
    let remaining = state.reset_at - now;
    if remaining > 0.0 {
        Some(remaining)
    } else {
        let _ = std::fs::remove_file(path);
        None
    }
}

fn write_state(path: &Path, now: f64, reset_seconds: f64) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let state = PersistedNousRateLimit {
        reset_at: now + reset_seconds,
        recorded_at: now,
        reset_seconds,
    };
    let payload = serde_json::to_vec(&state)
        .map_err(|err| std::io::Error::other(format!("serialize Nous guard state: {err}")))?;
    let tmp_extension = format!("tmp-{}-{}", std::process::id(), now_seconds().to_bits());
    let tmp = path.with_extension(tmp_extension.as_str());
    std::fs::write(&tmp, payload)?;
    let _ = std::fs::remove_file(path);
    std::fs::rename(tmp, path)
}

fn blocked_message(remaining_seconds: f64) -> String {
    format!(
        "{GUARD_PREFIX}; resets in {}",
        format_remaining(remaining_seconds)
    )
}

fn format_remaining(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    if seconds < 60 {
        format!("{seconds}s")
    } else if seconds < 3600 {
        let minutes = seconds / 60;
        let rem = seconds % 60;
        if rem == 0 {
            format!("{minutes}m")
        } else {
            format!("{minutes}m {rem}s")
        }
    } else {
        let hours = seconds / 3600;
        let minutes = (seconds % 3600) / 60;
        if minutes == 0 {
            format!("{hours}h")
        } else {
            format!("{hours}h {minutes}m")
        }
    }
}

fn now_seconds() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs_f64())
        .unwrap_or(0.0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse_rate_limit_headers;
    use std::fs;

    #[test]
    fn detects_nous_urls_only() {
        assert!(is_nous_base_url(
            "https://inference-api.nousresearch.com/v1"
        ));
        assert!(is_nous_base_url("https://portal.nousresearch.com"));
        assert!(!is_nous_base_url("https://openrouter.ai/api/v1"));
    }

    #[test]
    fn genuine_limit_requires_exhausted_bucket_with_long_reset() {
        let state = parse_rate_limit_headers(
            [
                ("x-ratelimit-limit-requests-1h", "100"),
                ("x-ratelimit-remaining-requests-1h", "0"),
                ("x-ratelimit-reset-requests-1h", "1h"),
            ],
            "nous",
        )
        .expect("headers");

        let reset = genuine_reset_seconds(Some(&state), None).expect("genuine limit");

        assert!(reset > 3500.0);
    }

    #[test]
    fn short_reset_does_not_trip_breaker() {
        let state = parse_rate_limit_headers(
            [
                ("x-ratelimit-limit-requests", "10"),
                ("x-ratelimit-remaining-requests", "0"),
                ("x-ratelimit-reset-requests", "5"),
            ],
            "nous",
        )
        .expect("headers");

        assert!(genuine_reset_seconds(Some(&state), None).is_none());
    }

    #[test]
    fn previous_exhausted_snapshot_can_trip_breaker() {
        let previous = parse_rate_limit_headers(
            [
                ("x-ratelimit-limit-requests-1h", "10"),
                ("x-ratelimit-remaining-requests-1h", "0"),
                ("x-ratelimit-reset-requests-1h", "2h"),
            ],
            "nous",
        )
        .expect("headers");

        assert!(genuine_reset_seconds(None, Some(&previous)).is_some());
    }

    #[test]
    fn active_state_expires_and_cleans_up() {
        let path = temp_state_path("expired");
        let parent = path.parent().expect("parent");
        fs::create_dir_all(parent).unwrap();
        fs::write(
            &path,
            serde_json::to_vec(&PersistedNousRateLimit {
                reset_at: 10.0,
                recorded_at: 1.0,
                reset_seconds: 9.0,
            })
            .unwrap(),
        )
        .unwrap();

        assert!(remaining_for_path(&path, 11.0).is_none());
        assert!(!path.exists());
    }

    #[test]
    fn writes_and_reads_active_state() {
        let path = temp_state_path("active");

        write_state(&path, 100.0, 125.0).unwrap();

        let remaining = remaining_for_path(&path, 120.0).expect("active state");
        assert_eq!(remaining, 105.0);
        assert!(blocked_message(remaining).contains("1m 45s"));
    }

    fn temp_state_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!(
                "hakimi-nous-rate-guard-{name}-{}-{nanos}",
                std::process::id()
            ))
            .join(STATE_FILENAME)
    }
}
