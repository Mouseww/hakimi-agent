use serde::{Deserialize, Serialize};

use crate::Usage;

const ONE_MILLION: f64 = 1_000_000.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostStatus {
    Estimated,
    Included,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostSource {
    OfficialDocsSnapshot,
    SubscriptionIncluded,
    None,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostEstimate {
    pub amount_usd: Option<f64>,
    pub status: CostStatus,
    pub source: CostSource,
    pub label: String,
    pub pricing_version: Option<String>,
    #[serde(default)]
    pub notes: Vec<String>,
}

impl CostEstimate {
    pub fn unknown() -> Self {
        Self {
            amount_usd: None,
            status: CostStatus::Unknown,
            source: CostSource::None,
            label: "n/a".to_string(),
            pricing_version: None,
            notes: Vec::new(),
        }
    }

    fn included() -> Self {
        Self {
            amount_usd: Some(0.0),
            status: CostStatus::Included,
            source: CostSource::SubscriptionIncluded,
            label: "included".to_string(),
            pricing_version: Some("included-route".to_string()),
            notes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct PricingEntry {
    provider: &'static str,
    model: &'static str,
    input_per_million: f64,
    output_per_million: f64,
    cache_read_per_million: Option<f64>,
    cache_write_per_million: Option<f64>,
    pricing_version: &'static str,
}

const PRICING: &[PricingEntry] = &[
    PricingEntry {
        provider: "openai",
        model: "gpt-4o",
        input_per_million: 2.50,
        output_per_million: 10.00,
        cache_read_per_million: Some(1.25),
        cache_write_per_million: None,
        pricing_version: "openai-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "openai",
        model: "gpt-4o-mini",
        input_per_million: 0.15,
        output_per_million: 0.60,
        cache_read_per_million: Some(0.075),
        cache_write_per_million: None,
        pricing_version: "openai-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "openai",
        model: "gpt-4.1",
        input_per_million: 2.00,
        output_per_million: 8.00,
        cache_read_per_million: Some(0.50),
        cache_write_per_million: None,
        pricing_version: "openai-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "openai",
        model: "gpt-4.1-mini",
        input_per_million: 0.40,
        output_per_million: 1.60,
        cache_read_per_million: Some(0.10),
        cache_write_per_million: None,
        pricing_version: "openai-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "openai",
        model: "gpt-4.1-nano",
        input_per_million: 0.10,
        output_per_million: 0.40,
        cache_read_per_million: Some(0.025),
        cache_write_per_million: None,
        pricing_version: "openai-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "openai",
        model: "o3",
        input_per_million: 10.00,
        output_per_million: 40.00,
        cache_read_per_million: Some(2.50),
        cache_write_per_million: None,
        pricing_version: "openai-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "openai",
        model: "o3-mini",
        input_per_million: 1.10,
        output_per_million: 4.40,
        cache_read_per_million: Some(0.55),
        cache_write_per_million: None,
        pricing_version: "openai-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-opus-4-7",
        input_per_million: 5.00,
        output_per_million: 25.00,
        cache_read_per_million: Some(0.50),
        cache_write_per_million: Some(6.25),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-opus-4-7-20250507",
        input_per_million: 5.00,
        output_per_million: 25.00,
        cache_read_per_million: Some(0.50),
        cache_write_per_million: Some(6.25),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-opus-4-6",
        input_per_million: 5.00,
        output_per_million: 25.00,
        cache_read_per_million: Some(0.50),
        cache_write_per_million: Some(6.25),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-sonnet-4-6",
        input_per_million: 3.00,
        output_per_million: 15.00,
        cache_read_per_million: Some(0.30),
        cache_write_per_million: Some(3.75),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-opus-4-5",
        input_per_million: 5.00,
        output_per_million: 25.00,
        cache_read_per_million: Some(0.50),
        cache_write_per_million: Some(6.25),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-sonnet-4-5",
        input_per_million: 3.00,
        output_per_million: 15.00,
        cache_read_per_million: Some(0.30),
        cache_write_per_million: Some(3.75),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-haiku-4-5",
        input_per_million: 1.00,
        output_per_million: 5.00,
        cache_read_per_million: Some(0.10),
        cache_write_per_million: Some(1.25),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-opus-4-20250514",
        input_per_million: 15.00,
        output_per_million: 75.00,
        cache_read_per_million: Some(1.50),
        cache_write_per_million: Some(18.75),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-sonnet-4-20250514",
        input_per_million: 3.00,
        output_per_million: 15.00,
        cache_read_per_million: Some(0.30),
        cache_write_per_million: Some(3.75),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-5-sonnet-20241022",
        input_per_million: 3.00,
        output_per_million: 15.00,
        cache_read_per_million: Some(0.30),
        cache_write_per_million: Some(3.75),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-5-haiku-20241022",
        input_per_million: 0.80,
        output_per_million: 4.00,
        cache_read_per_million: Some(0.08),
        cache_write_per_million: Some(1.00),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-opus-20240229",
        input_per_million: 15.00,
        output_per_million: 75.00,
        cache_read_per_million: Some(1.50),
        cache_write_per_million: Some(18.75),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "anthropic",
        model: "claude-3-haiku-20240307",
        input_per_million: 0.25,
        output_per_million: 1.25,
        cache_read_per_million: Some(0.03),
        cache_write_per_million: Some(0.30),
        pricing_version: "anthropic-pricing-2026-05",
    },
    PricingEntry {
        provider: "google",
        model: "gemini-2.5-pro",
        input_per_million: 1.25,
        output_per_million: 10.00,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "google-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "google",
        model: "gemini-2.5-flash",
        input_per_million: 0.15,
        output_per_million: 0.60,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "google-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "google",
        model: "gemini-2.0-flash",
        input_per_million: 0.10,
        output_per_million: 0.40,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "google-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "deepseek",
        model: "deepseek-chat",
        input_per_million: 0.14,
        output_per_million: 0.28,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "deepseek-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "deepseek",
        model: "deepseek-reasoner",
        input_per_million: 0.55,
        output_per_million: 2.19,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "deepseek-pricing-2026-03-16",
    },
    PricingEntry {
        provider: "deepseek",
        model: "deepseek-v4-pro",
        input_per_million: 1.74,
        output_per_million: 3.48,
        cache_read_per_million: Some(0.0145),
        cache_write_per_million: None,
        pricing_version: "deepseek-pricing-2026-05-12",
    },
    PricingEntry {
        provider: "minimax",
        model: "minimax-m2.7",
        input_per_million: 0.30,
        output_per_million: 1.20,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "minimax-pricing-2026-04",
    },
    PricingEntry {
        provider: "minimax-cn",
        model: "minimax-m2.7",
        input_per_million: 0.30,
        output_per_million: 1.20,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "minimax-pricing-2026-04",
    },
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct BillingRoute {
    provider: String,
    model: String,
    subscription_included: bool,
}

pub fn estimate_usage_cost(model: &str, provider: &str, usage: &Usage) -> CostEstimate {
    let route = resolve_billing_route(model, provider);
    if route.subscription_included {
        return CostEstimate::included();
    }

    let Some(entry) = lookup_pricing(&route) else {
        return CostEstimate::unknown();
    };

    let cached_read = usage.cached_tokens;
    let cache_write = if route.provider == "anthropic" {
        usage.reasoning_tokens
    } else {
        0
    };
    let prompt_tokens = if route.provider == "anthropic" {
        usage.prompt_tokens
    } else {
        usage.prompt_tokens.saturating_sub(cached_read)
    };

    if cached_read > 0 && entry.cache_read_per_million.is_none() {
        return unknown_with_note("cache-read pricing unavailable for this provider/model route");
    }
    if cache_write > 0 && entry.cache_write_per_million.is_none() {
        return unknown_with_note("cache-write pricing unavailable for this provider/model route");
    }

    let mut amount = prompt_tokens as f64 * entry.input_per_million / ONE_MILLION;
    amount += usage.completion_tokens as f64 * entry.output_per_million / ONE_MILLION;
    amount += cached_read as f64 * entry.cache_read_per_million.unwrap_or(0.0) / ONE_MILLION;
    amount += cache_write as f64 * entry.cache_write_per_million.unwrap_or(0.0) / ONE_MILLION;

    let mut notes = Vec::new();
    if route.provider == "anthropic" && cache_write > 0 {
        notes.push("Anthropic cache-write tokens are carried in reasoning_tokens.".to_string());
    }

    CostEstimate {
        amount_usd: Some(amount),
        status: CostStatus::Estimated,
        source: CostSource::OfficialDocsSnapshot,
        label: format!("~{}", format_usd(amount)),
        pricing_version: Some(entry.pricing_version.to_string()),
        notes,
    }
}

fn unknown_with_note(note: &str) -> CostEstimate {
    let mut estimate = CostEstimate::unknown();
    estimate.notes.push(note.to_string());
    estimate
}

fn lookup_pricing(route: &BillingRoute) -> Option<PricingEntry> {
    PRICING
        .iter()
        .copied()
        .find(|entry| entry.provider == route.provider && entry.model == route.model)
}

fn resolve_billing_route(model: &str, provider: &str) -> BillingRoute {
    let mut provider_name = normalize_provider(provider);
    let mut model_name = model.trim().to_ascii_lowercase();

    if provider_name == "openai-codex" {
        return BillingRoute {
            provider: provider_name,
            model: model_name,
            subscription_included: true,
        };
    }

    if let Some((prefix, bare_model)) = model_name.clone().split_once('/') {
        let normalized_prefix = normalize_provider(prefix);
        if is_known_provider(&normalized_prefix) {
            provider_name = normalized_prefix;
            model_name = bare_model.to_string();
        }
    }

    if provider_name == "auto" || provider_name == "openai-compatible" {
        provider_name = infer_provider_from_model(&model_name).unwrap_or(provider_name);
    }

    if provider_name == "claude" {
        provider_name = "anthropic".to_string();
    }
    if provider_name == "gemini" {
        provider_name = "google".to_string();
    }

    if provider_name == "anthropic" {
        model_name = normalize_anthropic_model(&model_name);
    }

    BillingRoute {
        provider: provider_name,
        model: model_name,
        subscription_included: false,
    }
}

fn normalize_provider(provider: &str) -> String {
    match provider.trim().to_ascii_lowercase().as_str() {
        "" => "auto".to_string(),
        "openai-responses" | "responses" => "openai".to_string(),
        other => other.to_string(),
    }
}

fn infer_provider_from_model(model: &str) -> Option<String> {
    let model = model.trim().to_ascii_lowercase();
    if model.starts_with("gpt-") || model == "o3" || model.starts_with("o3-") {
        Some("openai".to_string())
    } else if model.starts_with("claude-") {
        Some("anthropic".to_string())
    } else if model.starts_with("gemini-") {
        Some("google".to_string())
    } else if model.starts_with("deepseek-") {
        Some("deepseek".to_string())
    } else if model.starts_with("minimax-") {
        Some("minimax".to_string())
    } else {
        None
    }
}

fn is_known_provider(provider: &str) -> bool {
    matches!(
        provider,
        "anthropic" | "openai" | "google" | "gemini" | "deepseek" | "minimax" | "minimax-cn"
    )
}

fn normalize_anthropic_model(model: &str) -> String {
    let mut name = model.trim().to_ascii_lowercase();
    if let Some(stripped) = name.strip_prefix("anthropic/") {
        name = stripped.to_string();
    }
    name.replace('.', "-")
}

fn format_usd(amount: f64) -> String {
    if amount == 0.0 {
        "$0.00".to_string()
    } else if amount.abs() < 0.01 {
        format!("${amount:.6}")
    } else if amount.abs() < 1.0 {
        format!("${amount:.4}")
    } else {
        format!("${amount:.2}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn usage(prompt: u32, completion: u32, cached: u32, reasoning: u32) -> Usage {
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            cached_tokens: cached,
            reasoning_tokens: reasoning,
        }
    }

    #[test]
    fn estimates_openai_cost_and_subtracts_cached_prompt_tokens() {
        let estimate =
            estimate_usage_cost("gpt-4.1", "openai-responses", &usage(2_000, 500, 500, 0));

        assert_eq!(estimate.status, CostStatus::Estimated);
        assert_eq!(estimate.source, CostSource::OfficialDocsSnapshot);
        assert_eq!(
            estimate.pricing_version,
            Some("openai-pricing-2026-03-16".to_string())
        );
        assert_eq!(estimate.label, "~$0.007250");
        assert!((estimate.amount_usd.unwrap() - 0.00725).abs() < 1e-12);
    }

    #[test]
    fn estimates_anthropic_cache_read_and_write_cost() {
        let estimate = estimate_usage_cost(
            "claude-sonnet-4.5",
            "anthropic",
            &usage(1_000, 200, 100, 50),
        );

        assert_eq!(estimate.status, CostStatus::Estimated);
        assert_eq!(estimate.label, "~$0.006217");
        assert_eq!(
            estimate.pricing_version,
            Some("anthropic-pricing-2026-05".to_string())
        );
        assert!(estimate.notes[0].contains("cache-write"));
    }

    #[test]
    fn model_prefix_can_select_provider_when_transport_is_openai_compatible() {
        let estimate = estimate_usage_cost(
            "deepseek/deepseek-chat",
            "openai-compatible",
            &usage(1_000, 500, 0, 0),
        );

        assert_eq!(estimate.status, CostStatus::Estimated);
        assert_eq!(estimate.label, "~$0.000280");
        assert_eq!(
            estimate.pricing_version,
            Some("deepseek-pricing-2026-03-16".to_string())
        );
    }

    #[test]
    fn openai_codex_route_is_subscription_included() {
        let estimate = estimate_usage_cost(
            "codex-mini-latest",
            "openai-codex",
            &usage(10_000, 5_000, 0, 0),
        );

        assert_eq!(estimate.status, CostStatus::Included);
        assert_eq!(estimate.source, CostSource::SubscriptionIncluded);
        assert_eq!(estimate.label, "included");
        assert_eq!(estimate.amount_usd, Some(0.0));
    }

    #[test]
    fn unknown_model_returns_unknown_cost() {
        let estimate = estimate_usage_cost("local-model", "custom", &usage(1_000, 500, 0, 0));

        assert_eq!(estimate.status, CostStatus::Unknown);
        assert_eq!(estimate.label, "n/a");
        assert_eq!(estimate.amount_usd, None);
    }

    #[test]
    fn cache_tokens_without_known_cache_pricing_return_unknown() {
        let estimate = estimate_usage_cost("gemini-2.5-pro", "gemini", &usage(1_000, 500, 10, 0));

        assert_eq!(estimate.status, CostStatus::Unknown);
        assert!(estimate.notes[0].contains("cache-read pricing unavailable"));
    }
}
