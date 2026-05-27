use reqwest::header::HeaderMap;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// One provider rate-limit window, such as requests per minute.
#[derive(Debug, Clone)]
pub struct RateLimitBucket {
    pub limit: u64,
    pub remaining: u64,
    pub reset_seconds: f64,
    captured_at: Instant,
}

impl RateLimitBucket {
    fn new(limit: u64, remaining: u64, reset_seconds: f64, captured_at: Instant) -> Self {
        Self {
            limit,
            remaining,
            reset_seconds,
            captured_at,
        }
    }

    pub fn used(&self) -> u64 {
        self.limit.saturating_sub(self.remaining)
    }

    pub fn usage_percent(&self) -> f64 {
        if self.limit == 0 {
            0.0
        } else {
            (self.used() as f64 / self.limit as f64) * 100.0
        }
    }

    pub fn remaining_seconds_now(&self) -> f64 {
        (self.reset_seconds - self.captured_at.elapsed().as_secs_f64()).max(0.0)
    }
}

/// Parsed provider rate-limit state from `x-ratelimit-*` response headers.
#[derive(Debug, Clone)]
pub struct RateLimitState {
    pub requests_minute: RateLimitBucket,
    pub requests_hour: RateLimitBucket,
    pub tokens_minute: RateLimitBucket,
    pub tokens_hour: RateLimitBucket,
    pub provider: String,
    captured_at: Instant,
}

impl RateLimitState {
    pub fn age_seconds(&self) -> f64 {
        self.captured_at.elapsed().as_secs_f64()
    }

    pub fn format_display(&self) -> String {
        let freshness = if self.age_seconds() < 5.0 {
            "just now".to_string()
        } else {
            format!("{} ago", fmt_seconds(self.age_seconds()))
        };
        let provider = if self.provider.trim().is_empty() {
            "Provider".to_string()
        } else {
            self.provider.clone()
        };

        let mut lines = vec![
            format!("{provider} rate limits (captured {freshness}):"),
            String::new(),
            bucket_line("Requests/min", &self.requests_minute),
            bucket_line("Requests/hr", &self.requests_hour),
            String::new(),
            bucket_line("Tokens/min", &self.tokens_minute),
            bucket_line("Tokens/hr", &self.tokens_hour),
        ];

        let warnings = [
            ("requests/min", &self.requests_minute),
            ("requests/hr", &self.requests_hour),
            ("tokens/min", &self.tokens_minute),
            ("tokens/hr", &self.tokens_hour),
        ]
        .into_iter()
        .filter_map(|(label, bucket)| {
            if bucket.limit > 0 && bucket.usage_percent() >= 80.0 {
                Some(format!(
                    "WARN: {label} at {:.0}% - resets in {}",
                    bucket.usage_percent(),
                    fmt_seconds(bucket.remaining_seconds_now())
                ))
            } else {
                None
            }
        })
        .collect::<Vec<_>>();

        if !warnings.is_empty() {
            lines.push(String::new());
            lines.extend(warnings);
        }

        lines.join("\n")
    }

    pub fn format_compact(&self) -> String {
        let mut parts = Vec::new();
        if self.requests_minute.limit > 0 {
            parts.push(format!(
                "RPM: {}/{}",
                fmt_count(self.requests_minute.remaining),
                fmt_count(self.requests_minute.limit)
            ));
        }
        if self.requests_hour.limit > 0 {
            parts.push(format!(
                "RPH: {}/{} (resets {})",
                fmt_count(self.requests_hour.remaining),
                fmt_count(self.requests_hour.limit),
                fmt_seconds(self.requests_hour.remaining_seconds_now())
            ));
        }
        if self.tokens_minute.limit > 0 {
            parts.push(format!(
                "TPM: {}/{}",
                fmt_count(self.tokens_minute.remaining),
                fmt_count(self.tokens_minute.limit)
            ));
        }
        if self.tokens_hour.limit > 0 {
            parts.push(format!(
                "TPH: {}/{} (resets {})",
                fmt_count(self.tokens_hour.remaining),
                fmt_count(self.tokens_hour.limit),
                fmt_seconds(self.tokens_hour.remaining_seconds_now())
            ));
        }

        if parts.is_empty() {
            "No rate limit data.".to_string()
        } else {
            parts.join(" | ")
        }
    }
}

/// Thread-safe holder for the most recent provider rate-limit state.
#[derive(Debug, Clone, Default)]
pub struct RateLimitTracker {
    state: Arc<RwLock<Option<RateLimitState>>>,
}

impl RateLimitTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update_from_headers(
        &self,
        headers: &HeaderMap,
        provider: &str,
    ) -> Option<RateLimitState> {
        let state = parse_rate_limit_headers(
            headers.iter().filter_map(|(name, value)| {
                value.to_str().ok().map(|value| (name.as_str(), value))
            }),
            provider,
        )?;

        if let Ok(mut guard) = self.state.write() {
            *guard = Some(state.clone());
        }

        Some(state)
    }

    pub fn snapshot(&self) -> Option<RateLimitState> {
        self.state.read().ok().and_then(|guard| guard.clone())
    }
}

pub fn parse_rate_limit_headers<I, K, V>(headers: I, provider: &str) -> Option<RateLimitState>
where
    I: IntoIterator<Item = (K, V)>,
    K: AsRef<str>,
    V: AsRef<str>,
{
    let lowered = headers
        .into_iter()
        .map(|(key, value)| {
            (
                key.as_ref().trim().to_ascii_lowercase(),
                value.as_ref().trim().to_string(),
            )
        })
        .collect::<HashMap<_, _>>();

    if !lowered.keys().any(|key| key.starts_with("x-ratelimit-")) {
        return None;
    }

    let now = Instant::now();
    Some(RateLimitState {
        requests_minute: bucket(&lowered, "requests", "", now),
        requests_hour: bucket(&lowered, "requests", "-1h", now),
        tokens_minute: bucket(&lowered, "tokens", "", now),
        tokens_hour: bucket(&lowered, "tokens", "-1h", now),
        provider: provider.to_string(),
        captured_at: now,
    })
}

fn bucket(
    headers: &HashMap<String, String>,
    resource: &str,
    suffix: &str,
    now: Instant,
) -> RateLimitBucket {
    let tag = format!("{resource}{suffix}");
    RateLimitBucket::new(
        parse_u64(headers.get(&format!("x-ratelimit-limit-{tag}"))),
        parse_u64(headers.get(&format!("x-ratelimit-remaining-{tag}"))),
        parse_reset_seconds(headers.get(&format!("x-ratelimit-reset-{tag}"))),
        now,
    )
}

fn parse_u64(value: Option<&String>) -> u64 {
    value
        .and_then(|value| value.parse::<f64>().ok())
        .map(|value| value.max(0.0) as u64)
        .unwrap_or(0)
}

fn parse_reset_seconds(value: Option<&String>) -> f64 {
    value
        .and_then(|value| parse_duration_seconds(value))
        .unwrap_or(0.0)
}

fn parse_duration_seconds(value: &str) -> Option<f64> {
    let text = value.trim().to_ascii_lowercase();
    if text.is_empty() {
        return None;
    }
    if let Ok(seconds) = text.parse::<f64>() {
        return Some(seconds.max(0.0));
    }

    let mut total = 0.0;
    let mut digits = String::new();
    let mut unit = String::new();
    let mut saw_unit = false;

    for ch in text.chars().chain(std::iter::once(' ')) {
        if ch.is_ascii_digit() || ch == '.' {
            if !unit.is_empty() {
                total += duration_part(&digits, &unit)?;
                digits.clear();
                unit.clear();
                saw_unit = true;
            }
            digits.push(ch);
        } else if ch.is_ascii_alphabetic() {
            unit.push(ch);
        } else if !digits.is_empty() && !unit.is_empty() {
            total += duration_part(&digits, &unit)?;
            digits.clear();
            unit.clear();
            saw_unit = true;
        }
    }

    if saw_unit { Some(total.max(0.0)) } else { None }
}

fn duration_part(digits: &str, unit: &str) -> Option<f64> {
    let value = digits.parse::<f64>().ok()?;
    match unit {
        "s" | "sec" | "secs" | "second" | "seconds" => Some(value),
        "m" | "min" | "mins" | "minute" | "minutes" => Some(value * 60.0),
        "h" | "hr" | "hrs" | "hour" | "hours" => Some(value * 3600.0),
        _ => None,
    }
}

fn fmt_count(value: u64) -> String {
    if value >= 1_000_000 {
        format!("{:.1}M", value as f64 / 1_000_000.0)
    } else if value >= 1_000 {
        format!("{:.1}K", value as f64 / 1_000.0)
    } else {
        value.to_string()
    }
}

fn fmt_seconds(seconds: f64) -> String {
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

fn bar(percent: f64, width: usize) -> String {
    let filled = ((percent / 100.0) * width as f64).floor() as usize;
    let filled = filled.min(width);
    format!("[{}{}]", "#".repeat(filled), "-".repeat(width - filled))
}

fn bucket_line(label: &str, bucket: &RateLimitBucket) -> String {
    if bucket.limit == 0 {
        return format!("  {label:<14} (no data)");
    }

    format!(
        "  {label:<14} {} {:>5.1}%  {}/{} used  ({} left, resets in {})",
        bar(bucket.usage_percent(), 20),
        bucket.usage_percent(),
        fmt_count(bucket.used()),
        fmt_count(bucket.limit),
        fmt_count(bucket.remaining),
        fmt_seconds(bucket.remaining_seconds_now())
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use reqwest::header::{HeaderMap, HeaderValue};

    #[test]
    fn parses_full_openai_compatible_headers() {
        let state = parse_rate_limit_headers(
            [
                ("x-ratelimit-limit-requests", "100"),
                ("x-ratelimit-remaining-requests", "80"),
                ("x-ratelimit-reset-requests", "30"),
                ("x-ratelimit-limit-requests-1h", "1000"),
                ("x-ratelimit-remaining-requests-1h", "750"),
                ("x-ratelimit-reset-requests-1h", "45m"),
                ("x-ratelimit-limit-tokens", "200000"),
                ("x-ratelimit-remaining-tokens", "150000"),
                ("x-ratelimit-reset-tokens", "1m 30s"),
                ("x-ratelimit-limit-tokens-1h", "8000000"),
                ("x-ratelimit-remaining-tokens-1h", "7600000"),
                ("x-ratelimit-reset-tokens-1h", "1h 2m"),
            ],
            "openai-compatible",
        )
        .expect("rate limit headers");

        assert_eq!(state.requests_minute.limit, 100);
        assert_eq!(state.requests_minute.remaining, 80);
        assert_eq!(state.requests_hour.reset_seconds, 2700.0);
        assert_eq!(state.tokens_minute.reset_seconds, 90.0);
        assert_eq!(state.tokens_hour.limit, 8_000_000);
    }

    #[test]
    fn header_names_are_case_insensitive() {
        let state = parse_rate_limit_headers(
            [
                ("X-RateLimit-Limit-Requests", "10"),
                ("X-RateLimit-Remaining-Requests", "3"),
                ("X-RateLimit-Reset-Requests", "5"),
            ],
            "provider",
        )
        .expect("rate limit headers");

        assert_eq!(state.requests_minute.used(), 7);
        assert!((state.requests_minute.usage_percent() - 70.0).abs() < f64::EPSILON);
    }

    #[test]
    fn missing_rate_limit_headers_return_none() {
        assert!(parse_rate_limit_headers([("content-type", "application/json")], "p").is_none());
    }

    #[test]
    fn parses_duration_reset_values() {
        assert_eq!(parse_duration_seconds("2h 3m 4s"), Some(7384.0));
        assert_eq!(parse_duration_seconds("90"), Some(90.0));
        assert_eq!(parse_duration_seconds("bad"), None);
    }

    #[test]
    fn malformed_numbers_default_to_zero() {
        let state = parse_rate_limit_headers(
            [
                ("x-ratelimit-limit-requests", "not-a-number"),
                ("x-ratelimit-remaining-requests", "-5"),
            ],
            "provider",
        )
        .expect("rate limit headers");

        assert_eq!(state.requests_minute.limit, 0);
        assert_eq!(state.requests_minute.remaining, 0);
    }

    #[test]
    fn compact_format_skips_empty_buckets() {
        let state = parse_rate_limit_headers(
            [
                ("x-ratelimit-limit-requests", "100"),
                ("x-ratelimit-remaining-requests", "80"),
            ],
            "provider",
        )
        .expect("rate limit headers");

        assert_eq!(state.format_compact(), "RPM: 80/100");
    }

    #[test]
    fn display_format_includes_hot_bucket_warning() {
        let state = parse_rate_limit_headers(
            [
                ("x-ratelimit-limit-requests", "100"),
                ("x-ratelimit-remaining-requests", "10"),
                ("x-ratelimit-reset-requests", "60"),
            ],
            "provider",
        )
        .expect("rate limit headers");

        let display = state.format_display();
        assert!(display.contains("provider rate limits"));
        assert!(display.contains("WARN: requests/min at 90%"));
    }

    #[test]
    fn tracker_stores_latest_snapshot() {
        let mut headers = HeaderMap::new();
        headers.insert("x-ratelimit-limit-requests", HeaderValue::from_static("50"));
        headers.insert(
            "x-ratelimit-remaining-requests",
            HeaderValue::from_static("49"),
        );

        let tracker = RateLimitTracker::new();
        let updated = tracker
            .update_from_headers(&headers, "openai-compatible")
            .expect("rate limit headers");
        let snapshot = tracker.snapshot().expect("snapshot");

        assert_eq!(updated.requests_minute.limit, 50);
        assert_eq!(snapshot.requests_minute.remaining, 49);
        assert_eq!(snapshot.provider, "openai-compatible");
    }
}
