use chrono::{DateTime, TimeZone, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountUsageWindow {
    pub label: String,
    pub used_percent: Option<f64>,
    pub reset_at: Option<DateTime<Utc>>,
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AccountUsageSnapshot {
    pub provider: String,
    pub source: String,
    pub fetched_at: DateTime<Utc>,
    pub title: String,
    pub plan: Option<String>,
    #[serde(default)]
    pub windows: Vec<AccountUsageWindow>,
    #[serde(default)]
    pub details: Vec<String>,
    pub unavailable_reason: Option<String>,
}

impl AccountUsageSnapshot {
    pub fn available(&self) -> bool {
        self.unavailable_reason.is_none() && (!self.windows.is_empty() || !self.details.is_empty())
    }
}

pub fn openrouter_account_usage_from_payloads(
    credits_payload: &Value,
    key_payload: Option<&Value>,
    fetched_at: DateTime<Utc>,
) -> AccountUsageSnapshot {
    let credits = data_object(credits_payload);
    let key_data = key_payload.map(data_object).unwrap_or(Value::Null);

    let total_credits = number_field(&credits, "total_credits").unwrap_or(0.0);
    let total_usage = number_field(&credits, "total_usage").unwrap_or(0.0);
    let mut details = vec![format!(
        "Credits balance: ${:.2}",
        (total_credits - total_usage).max(0.0)
    )];

    let mut windows = Vec::new();
    let limit = number_field(&key_data, "limit");
    let limit_remaining = number_field(&key_data, "limit_remaining");
    if let (Some(limit), Some(remaining)) = (limit, limit_remaining)
        && limit > 0.0
        && remaining >= 0.0
        && remaining <= limit
    {
        let used_percent = ((limit - remaining) / limit) * 100.0;
        let mut detail_parts = vec![format!("${remaining:.2} of ${limit:.2} remaining")];
        if let Some(limit_reset) = string_field(&key_data, "limit_reset") {
            detail_parts.push(format!("resets {limit_reset}"));
        }
        windows.push(AccountUsageWindow {
            label: "API key quota".to_string(),
            used_percent: Some(used_percent),
            reset_at: None,
            detail: Some(detail_parts.join(" - ")),
        });
    }

    if let Some(usage) = number_field(&key_data, "usage") {
        let mut usage_parts = vec![format!("API key usage: ${usage:.2} total")];
        for (field, label) in [
            ("usage_daily", "today"),
            ("usage_weekly", "this week"),
            ("usage_monthly", "this month"),
        ] {
            if let Some(value) = number_field(&key_data, field).filter(|value| *value > 0.0) {
                usage_parts.push(format!("${value:.2} {label}"));
            }
        }
        details.push(usage_parts.join(" - "));
    }

    AccountUsageSnapshot {
        provider: "openrouter".to_string(),
        source: "credits_api".to_string(),
        fetched_at,
        title: "Account limits".to_string(),
        plan: None,
        windows,
        details,
        unavailable_reason: None,
    }
}

pub fn anthropic_token_is_oauth(token: &str) -> bool {
    let token = token.trim();
    if token.is_empty() || token.starts_with("sk-ant-api") {
        return false;
    }
    token.starts_with("sk-ant-") || token.starts_with("eyJ") || token.starts_with("cc-")
}

pub fn anthropic_api_key_unavailable_snapshot(fetched_at: DateTime<Utc>) -> AccountUsageSnapshot {
    AccountUsageSnapshot {
        provider: "anthropic".to_string(),
        source: "oauth_usage_api".to_string(),
        fetched_at,
        title: "Account limits".to_string(),
        plan: None,
        windows: Vec::new(),
        details: Vec::new(),
        unavailable_reason: Some(
            "Anthropic account limits are only available for OAuth-backed Claude accounts."
                .to_string(),
        ),
    }
}

pub fn anthropic_account_usage_from_payload(
    payload: &Value,
    fetched_at: DateTime<Utc>,
) -> AccountUsageSnapshot {
    let payload = data_object(payload);
    let mut windows = Vec::new();
    for (field, label) in [
        ("five_hour", "Current session"),
        ("seven_day", "Current week"),
        ("seven_day_opus", "Opus week"),
        ("seven_day_sonnet", "Sonnet week"),
    ] {
        let Some(window) = payload.get(field).filter(|value| value.is_object()) else {
            continue;
        };
        let Some(utilization) = number_field(window, "utilization") else {
            continue;
        };
        let used_percent = if utilization <= 1.0 {
            utilization * 100.0
        } else {
            utilization
        };
        windows.push(AccountUsageWindow {
            label: label.to_string(),
            used_percent: Some(used_percent),
            reset_at: date_time_field(window, "resets_at"),
            detail: None,
        });
    }

    let mut details = Vec::new();
    if let Some(extra_usage) = payload.get("extra_usage").filter(|value| value.is_object())
        && extra_usage
            .get("is_enabled")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        let used_credits = number_field(extra_usage, "used_credits");
        let monthly_limit = number_field(extra_usage, "monthly_limit");
        if let (Some(used_credits), Some(monthly_limit)) = (used_credits, monthly_limit) {
            let currency = string_field(extra_usage, "currency").unwrap_or_else(|| "USD".into());
            details.push(format!(
                "Extra usage: {used_credits:.2} / {monthly_limit:.2} {currency}"
            ));
        }
    }

    AccountUsageSnapshot {
        provider: "anthropic".to_string(),
        source: "oauth_usage_api".to_string(),
        fetched_at,
        title: "Account limits".to_string(),
        plan: None,
        windows,
        details,
        unavailable_reason: None,
    }
}

pub fn codex_account_usage_api_url(base_url: &str) -> String {
    let mut normalized = base_url.trim().trim_end_matches('/').to_string();
    if normalized.is_empty() {
        normalized = "https://chatgpt.com/backend-api/codex".to_string();
    }
    if let Some(root) = normalized.strip_suffix("/codex") {
        normalized = root.to_string();
    }
    if normalized.contains("/backend-api") {
        format!("{normalized}/wham/usage")
    } else {
        format!("{normalized}/api/codex/usage")
    }
}

pub fn codex_account_usage_from_payload(
    payload: &Value,
    fetched_at: DateTime<Utc>,
) -> AccountUsageSnapshot {
    let payload = data_object(payload);
    let rate_limit = payload
        .get("rate_limit")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or(Value::Null);

    let mut windows = Vec::new();
    for (field, label) in [
        ("primary_window", "Session"),
        ("secondary_window", "Weekly"),
    ] {
        let Some(window) = rate_limit.get(field).filter(|value| value.is_object()) else {
            continue;
        };
        let Some(used_percent) = number_field(window, "used_percent") else {
            continue;
        };
        windows.push(AccountUsageWindow {
            label: label.to_string(),
            used_percent: Some(used_percent),
            reset_at: date_time_field(window, "reset_at"),
            detail: None,
        });
    }

    let mut details = Vec::new();
    if let Some(credits) = payload.get("credits").filter(|value| value.is_object())
        && bool_field(credits, "has_credits").unwrap_or(false)
    {
        if let Some(balance) = number_field(credits, "balance") {
            details.push(format!("Credits balance: ${balance:.2}"));
        } else if bool_field(credits, "unlimited").unwrap_or(false) {
            details.push("Credits balance: unlimited".to_string());
        }
    }

    AccountUsageSnapshot {
        provider: "openai-codex".to_string(),
        source: "usage_api".to_string(),
        fetched_at,
        title: "Account limits".to_string(),
        plan: string_field(&payload, "plan_type").map(title_case_slug),
        windows,
        details,
        unavailable_reason: None,
    }
}

pub fn render_account_usage_lines(snapshot: &AccountUsageSnapshot, markdown: bool) -> Vec<String> {
    let mut lines = Vec::new();
    let title = if markdown {
        format!("**{}**", snapshot.title)
    } else {
        snapshot.title.clone()
    };
    lines.push(title);
    if let Some(plan) = snapshot.plan.as_deref().filter(|plan| !plan.is_empty()) {
        lines.push(format!("Provider: {} ({plan})", snapshot.provider));
    } else {
        lines.push(format!("Provider: {}", snapshot.provider));
    }

    for window in &snapshot.windows {
        let mut line = match window.used_percent {
            Some(used) => {
                let used = used.clamp(0.0, 100.0).round() as u32;
                let remaining = 100u32.saturating_sub(used);
                format!("{}: {remaining}% remaining ({used}% used)", window.label)
            }
            None => format!("{}: unavailable", window.label),
        };
        if let Some(reset_at) = window.reset_at {
            line.push_str(&format!(" - resets {}", reset_at.to_rfc3339()));
        } else if let Some(detail) = window.detail.as_deref().filter(|detail| !detail.is_empty()) {
            line.push_str(&format!(" - {detail}"));
        }
        lines.push(line);
    }

    lines.extend(snapshot.details.iter().cloned());
    if let Some(reason) = snapshot
        .unavailable_reason
        .as_deref()
        .filter(|reason| !reason.is_empty())
    {
        lines.push(format!("Unavailable: {reason}"));
    }
    lines
}

fn data_object(payload: &Value) -> Value {
    payload
        .get("data")
        .filter(|value| value.is_object())
        .cloned()
        .unwrap_or_else(|| payload.clone())
}

fn number_field(payload: &Value, field: &str) -> Option<f64> {
    match payload.get(field)? {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn string_field(payload: &Value, field: &str) -> Option<String> {
    payload
        .get(field)?
        .as_str()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

fn bool_field(payload: &Value, field: &str) -> Option<bool> {
    match payload.get(field)? {
        Value::Bool(value) => Some(*value),
        Value::String(text) => match text.trim().to_ascii_lowercase().as_str() {
            "true" | "1" | "yes" => Some(true),
            "false" | "0" | "no" => Some(false),
            _ => None,
        },
        _ => None,
    }
}

fn title_case_slug(value: String) -> String {
    value
        .split(['_', '-'])
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            let Some(first) = chars.next() else {
                return String::new();
            };
            format!(
                "{}{}",
                first.to_uppercase(),
                chars.as_str().to_ascii_lowercase()
            )
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn date_time_field(payload: &Value, field: &str) -> Option<DateTime<Utc>> {
    match payload.get(field)? {
        Value::Number(number) => {
            let seconds = number.as_i64()?;
            Utc.timestamp_opt(seconds, 0).single()
        }
        Value::String(text) => {
            let text = text.trim();
            if text.is_empty() {
                return None;
            }
            let normalized = text
                .strip_suffix('Z')
                .map_or_else(|| text.to_string(), |value| format!("{value}+00:00"));
            DateTime::parse_from_rfc3339(&normalized)
                .ok()
                .map(|dt| dt.with_timezone(&Utc))
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_openrouter_credits_and_key_usage() {
        let snapshot = openrouter_account_usage_from_payloads(
            &json!({"data": {"total_credits": 25.0, "total_usage": 8.25}}),
            Some(&json!({"data": {
                "limit": 10.0,
                "limit_remaining": 6.5,
                "limit_reset": "monthly",
                "usage": 3.5,
                "usage_daily": 0.25,
                "usage_weekly": 1.25,
                "usage_monthly": 3.5
            }})),
            Utc::now(),
        );

        assert!(snapshot.available());
        assert_eq!(snapshot.provider, "openrouter");
        assert_eq!(snapshot.details[0], "Credits balance: $16.75");
        assert_eq!(snapshot.windows.len(), 1);
        assert_eq!(snapshot.windows[0].label, "API key quota");
        assert_eq!(
            snapshot.windows[0].detail.as_deref(),
            Some("$6.50 of $10.00 remaining - resets monthly")
        );
        assert_eq!(
            snapshot.details[1],
            "API key usage: $3.50 total - $0.25 today - $1.25 this week - $3.50 this month"
        );
    }

    #[test]
    fn openrouter_key_payload_is_optional() {
        let snapshot = openrouter_account_usage_from_payloads(
            &json!({"data": {"total_credits": "2.5", "total_usage": "3.0"}}),
            None,
            Utc::now(),
        );

        assert!(snapshot.available());
        assert!(snapshot.windows.is_empty());
        assert_eq!(snapshot.details, vec!["Credits balance: $0.00"]);
    }

    #[test]
    fn renders_markdown_account_usage_lines() {
        let snapshot = openrouter_account_usage_from_payloads(
            &json!({"data": {"total_credits": 10.0, "total_usage": 1.0}}),
            Some(&json!({"data": {"limit": 20.0, "limit_remaining": 15.0}})),
            Utc::now(),
        );

        let lines = render_account_usage_lines(&snapshot, true);

        assert_eq!(lines[0], "**Account limits**");
        assert_eq!(lines[1], "Provider: openrouter");
        assert!(lines[2].contains("75% remaining"));
        assert!(lines[3].contains("Credits balance: $9.00"));
    }

    #[test]
    fn detects_anthropic_oauth_token_shapes() {
        assert!(anthropic_token_is_oauth("sk-ant-oat-abc"));
        assert!(anthropic_token_is_oauth("sk-ant-managed-abc"));
        assert!(anthropic_token_is_oauth("eyJhbGciOi"));
        assert!(anthropic_token_is_oauth("cc-oauth-token"));
        assert!(!anthropic_token_is_oauth("sk-ant-api03-regular-key"));
        assert!(!anthropic_token_is_oauth("sk-or-v1-openrouter"));
        assert!(!anthropic_token_is_oauth(""));
    }

    #[test]
    fn parses_anthropic_oauth_usage_windows() {
        let snapshot = anthropic_account_usage_from_payload(
            &json!({
                "five_hour": {
                    "utilization": 0.25,
                    "resets_at": "2026-06-03T08:00:00Z"
                },
                "seven_day": {
                    "utilization": 62.5,
                    "resets_at": 1780502400
                },
                "seven_day_opus": {
                    "utilization": null
                },
                "extra_usage": {
                    "is_enabled": true,
                    "used_credits": 4.5,
                    "monthly_limit": 20,
                    "currency": "USD"
                }
            }),
            Utc::now(),
        );

        assert!(snapshot.available());
        assert_eq!(snapshot.provider, "anthropic");
        assert_eq!(snapshot.source, "oauth_usage_api");
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].label, "Current session");
        assert_eq!(snapshot.windows[0].used_percent, Some(25.0));
        assert!(snapshot.windows[0].reset_at.is_some());
        assert_eq!(snapshot.windows[1].label, "Current week");
        assert_eq!(snapshot.windows[1].used_percent, Some(62.5));
        assert_eq!(snapshot.details, vec!["Extra usage: 4.50 / 20.00 USD"]);
    }

    #[test]
    fn renders_anthropic_api_key_unavailable_snapshot() {
        let snapshot = anthropic_api_key_unavailable_snapshot(Utc::now());

        assert!(!snapshot.available());
        let lines = render_account_usage_lines(&snapshot, true);

        assert_eq!(lines[0], "**Account limits**");
        assert_eq!(lines[1], "Provider: anthropic");
        assert!(
            lines
                .iter()
                .any(|line| line.contains("OAuth-backed Claude accounts"))
        );
    }

    #[test]
    fn codex_usage_url_matches_backend_and_custom_routes() {
        assert_eq!(
            codex_account_usage_api_url(""),
            "https://chatgpt.com/backend-api/wham/usage"
        );
        assert_eq!(
            codex_account_usage_api_url("https://chatgpt.com/backend-api/codex"),
            "https://chatgpt.com/backend-api/wham/usage"
        );
        assert_eq!(
            codex_account_usage_api_url("https://codex.example.test"),
            "https://codex.example.test/api/codex/usage"
        );
    }

    #[test]
    fn parses_codex_usage_windows_and_credits() {
        let snapshot = codex_account_usage_from_payload(
            &json!({
                "plan_type": "chatgpt_plus",
                "rate_limit": {
                    "primary_window": {
                        "used_percent": 25,
                        "reset_at": "2026-06-03T08:00:00Z"
                    },
                    "secondary_window": {
                        "used_percent": "62.5",
                        "reset_at": 1780502400
                    }
                },
                "credits": {
                    "has_credits": true,
                    "balance": 12.5
                }
            }),
            Utc::now(),
        );

        assert!(snapshot.available());
        assert_eq!(snapshot.provider, "openai-codex");
        assert_eq!(snapshot.source, "usage_api");
        assert_eq!(snapshot.plan.as_deref(), Some("Chatgpt Plus"));
        assert_eq!(snapshot.windows.len(), 2);
        assert_eq!(snapshot.windows[0].label, "Session");
        assert_eq!(snapshot.windows[0].used_percent, Some(25.0));
        assert_eq!(snapshot.windows[1].label, "Weekly");
        assert_eq!(snapshot.windows[1].used_percent, Some(62.5));
        assert_eq!(snapshot.details, vec!["Credits balance: $12.50"]);
    }

    #[test]
    fn parses_codex_unlimited_credits() {
        let snapshot = codex_account_usage_from_payload(
            &json!({
                "credits": {
                    "has_credits": "true",
                    "unlimited": true
                }
            }),
            Utc::now(),
        );

        assert!(snapshot.available());
        assert_eq!(snapshot.details, vec!["Credits balance: unlimited"]);
    }
}
