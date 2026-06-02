use chrono::{DateTime, Utc};
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
}
