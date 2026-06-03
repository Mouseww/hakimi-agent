use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, io, path::Path};

use crate::Usage;

const ONE_MILLION: f64 = 1_000_000.0;
const PRICE_PRECISION: i32 = 12;
pub const LIVE_PRICING_CACHE_TTL_SECONDS: i64 = 60 * 60;

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
    ProviderModelsApi,
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

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LivePricingEntry {
    pub input_per_million: Option<f64>,
    pub output_per_million: Option<f64>,
    pub cache_read_per_million: Option<f64>,
    pub cache_write_per_million: Option<f64>,
    #[serde(default)]
    pub request_cost: Option<f64>,
}

pub type LivePricingCatalog = BTreeMap<String, LivePricingEntry>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LivePricingCache {
    pub provider: String,
    pub base_url: String,
    pub fetched_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub catalog: LivePricingCatalog,
}

impl LivePricingCache {
    pub fn new(
        provider: impl Into<String>,
        base_url: impl Into<String>,
        catalog: LivePricingCatalog,
        fetched_at: DateTime<Utc>,
        ttl: Duration,
    ) -> Self {
        Self {
            provider: provider.into(),
            base_url: base_url.into(),
            fetched_at,
            expires_at: fetched_at + ttl,
            catalog,
        }
    }

    pub fn is_fresh_at(&self, now: DateTime<Utc>) -> bool {
        !self.catalog.is_empty() && now < self.expires_at
    }
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
        provider: "bedrock",
        model: "anthropic.claude-opus-4-6",
        input_per_million: 15.00,
        output_per_million: 75.00,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "bedrock-pricing-2026-04",
    },
    PricingEntry {
        provider: "bedrock",
        model: "anthropic.claude-sonnet-4-6",
        input_per_million: 3.00,
        output_per_million: 15.00,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "bedrock-pricing-2026-04",
    },
    PricingEntry {
        provider: "bedrock",
        model: "anthropic.claude-sonnet-4-5",
        input_per_million: 3.00,
        output_per_million: 15.00,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "bedrock-pricing-2026-04",
    },
    PricingEntry {
        provider: "bedrock",
        model: "anthropic.claude-haiku-4-5",
        input_per_million: 0.80,
        output_per_million: 4.00,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "bedrock-pricing-2026-04",
    },
    PricingEntry {
        provider: "bedrock",
        model: "amazon.nova-pro",
        input_per_million: 0.80,
        output_per_million: 3.20,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "bedrock-pricing-2026-04",
    },
    PricingEntry {
        provider: "bedrock",
        model: "amazon.nova-lite",
        input_per_million: 0.06,
        output_per_million: 0.24,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "bedrock-pricing-2026-04",
    },
    PricingEntry {
        provider: "bedrock",
        model: "amazon.nova-micro",
        input_per_million: 0.035,
        output_per_million: 0.14,
        cache_read_per_million: None,
        cache_write_per_million: None,
        pricing_version: "bedrock-pricing-2026-04",
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

pub fn estimate_usage_cost_with_live_pricing(
    model: &str,
    provider: &str,
    usage: &Usage,
    live_pricing: &LivePricingCatalog,
) -> CostEstimate {
    estimate_usage_cost_with_live_pricing_and_requests(model, provider, usage, live_pricing, 1)
}

pub fn estimate_usage_cost_with_live_pricing_and_requests(
    model: &str,
    provider: &str,
    usage: &Usage,
    live_pricing: &LivePricingCatalog,
    request_count: usize,
) -> CostEstimate {
    let route = resolve_billing_route(model, provider);
    if route.subscription_included {
        return CostEstimate::included();
    }

    if let Some((model_id, entry)) = lookup_live_pricing(model, &route, live_pricing) {
        return estimate_from_live_pricing(&model_id, entry, usage, request_count);
    }

    estimate_usage_cost(model, provider, usage)
}

pub fn openrouter_models_pricing_from_payload(payload: &serde_json::Value) -> LivePricingCatalog {
    let mut catalog = BTreeMap::new();
    let Some(items) = payload.get("data").and_then(|value| value.as_array()) else {
        return catalog;
    };

    for item in items {
        let Some(model_id) = item
            .get("id")
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let Some(pricing) = item.get("pricing").and_then(|value| value.as_object()) else {
            continue;
        };
        let input_per_million = pricing.get("prompt").and_then(price_per_token_to_million);
        let output_per_million = pricing
            .get("completion")
            .and_then(price_per_token_to_million);
        let request_cost = pricing.get("request").and_then(price_amount);
        if input_per_million.is_none() && output_per_million.is_none() && request_cost.is_none() {
            continue;
        }
        let entry = LivePricingEntry {
            input_per_million,
            output_per_million,
            cache_read_per_million: pricing
                .get("input_cache_read")
                .and_then(price_per_token_to_million),
            cache_write_per_million: pricing
                .get("input_cache_write")
                .and_then(price_per_token_to_million),
            request_cost,
        };
        catalog.insert(normalize_live_model_id(model_id), entry);
    }

    catalog
}

pub fn save_live_pricing_cache(path: &Path, cache: &LivePricingCache) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let bytes = serde_json::to_vec_pretty(cache)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    std::fs::write(path, bytes)
}

pub fn load_fresh_live_pricing_cache(
    path: &Path,
    now: DateTime<Utc>,
) -> io::Result<Option<LivePricingCache>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err),
    };
    let cache: LivePricingCache = serde_json::from_slice(&bytes)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(cache.is_fresh_at(now).then_some(cache))
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

fn lookup_live_pricing<'a>(
    model: &str,
    route: &BillingRoute,
    live_pricing: &'a LivePricingCatalog,
) -> Option<(String, &'a LivePricingEntry)> {
    let raw_model = normalize_live_model_id(model);
    if let Some(entry) = live_pricing.get(&raw_model) {
        return Some((raw_model, entry));
    }

    let route_model = normalize_live_model_id(&route.model);
    if let Some(entry) = live_pricing.get(&route_model) {
        return Some((route_model, entry));
    }

    let qualified = normalize_live_model_id(&format!("{}/{}", route.provider, route.model));
    live_pricing.get(&qualified).map(|entry| (qualified, entry))
}

fn estimate_from_live_pricing(
    model_id: &str,
    entry: &LivePricingEntry,
    usage: &Usage,
    request_count: usize,
) -> CostEstimate {
    let cached_read = usage.cached_tokens;
    if cached_read > 0 && entry.cache_read_per_million.is_none() {
        return unknown_with_note("cache-read pricing unavailable from provider models API");
    }

    let prompt_tokens = usage.prompt_tokens.saturating_sub(cached_read);
    if prompt_tokens > 0 && entry.input_per_million.is_none() {
        return unknown_with_note("input pricing unavailable from provider models API");
    }
    if usage.completion_tokens > 0 && entry.output_per_million.is_none() {
        return unknown_with_note("output pricing unavailable from provider models API");
    }

    let mut amount = 0.0;
    amount += prompt_tokens as f64 * entry.input_per_million.unwrap_or(0.0) / ONE_MILLION;
    amount +=
        usage.completion_tokens as f64 * entry.output_per_million.unwrap_or(0.0) / ONE_MILLION;
    amount += cached_read as f64 * entry.cache_read_per_million.unwrap_or(0.0) / ONE_MILLION;
    if let Some(request_cost) = entry.request_cost {
        amount += request_count as f64 * request_cost;
    }
    amount = round_decimal(amount, PRICE_PRECISION);

    let mut notes = vec![format!("Live pricing matched `{model_id}`.")];
    if entry.request_cost.is_some() {
        notes.push(format!(
            "Provider models API request pricing applied for {request_count} API call(s)."
        ));
    }

    CostEstimate {
        amount_usd: Some(amount),
        status: CostStatus::Estimated,
        source: CostSource::ProviderModelsApi,
        label: format!("~{}", format_live_usd(amount)),
        pricing_version: Some("provider-models-api".to_string()),
        notes,
    }
}

fn normalize_live_model_id(model: &str) -> String {
    model.trim().to_ascii_lowercase()
}

fn price_per_token_to_million(value: &serde_json::Value) -> Option<f64> {
    let per_token = match value {
        serde_json::Value::Number(number) => number.as_f64()?,
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    per_token
        .is_finite()
        .then_some(round_decimal(per_token * ONE_MILLION, PRICE_PRECISION))
}

fn price_amount(value: &serde_json::Value) -> Option<f64> {
    let amount = match value {
        serde_json::Value::Number(number) => number.as_f64()?,
        serde_json::Value::String(text) => text.trim().parse::<f64>().ok()?,
        _ => return None,
    };
    amount
        .is_finite()
        .then_some(round_decimal(amount, PRICE_PRECISION))
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
    } else if provider_name == "bedrock" {
        model_name = normalize_bedrock_model(&model_name);
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
    } else if model.starts_with("anthropic.claude-") || model.starts_with("amazon.nova-") {
        Some("bedrock".to_string())
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
        "anthropic"
            | "openai"
            | "google"
            | "gemini"
            | "bedrock"
            | "deepseek"
            | "minimax"
            | "minimax-cn"
    )
}

fn normalize_anthropic_model(model: &str) -> String {
    let mut name = model.trim().to_ascii_lowercase();
    if let Some(stripped) = name.strip_prefix("anthropic/") {
        name = stripped.to_string();
    }
    name.replace('.', "-")
}

fn normalize_bedrock_model(model: &str) -> String {
    let mut name = model.trim().to_ascii_lowercase();
    if let Some(stripped) = name.strip_prefix("bedrock/") {
        name = stripped.to_string();
    }
    name = strip_bedrock_runtime_version(&name).to_string();
    strip_trailing_date_segment(&name).to_string()
}

fn strip_bedrock_runtime_version(model: &str) -> &str {
    let Some((base, suffix)) = model.rsplit_once("-v") else {
        return model;
    };
    let Some((version, revision)) = suffix.split_once(':') else {
        return model;
    };
    if version.chars().all(|ch| ch.is_ascii_digit())
        && revision.chars().all(|ch| ch.is_ascii_digit())
    {
        base
    } else {
        model
    }
}

fn strip_trailing_date_segment(model: &str) -> &str {
    let Some((base, date)) = model.rsplit_once('-') else {
        return model;
    };
    if date.len() == 8 && date.chars().all(|ch| ch.is_ascii_digit()) {
        base
    } else {
        model
    }
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

fn round_decimal(value: f64, decimals: i32) -> f64 {
    let factor = 10_f64.powi(decimals);
    (value * factor).round() / factor
}

fn display_decimals(amount: f64) -> usize {
    if amount.abs() < 0.01 {
        6
    } else if amount.abs() < 1.0 {
        4
    } else {
        2
    }
}

fn format_live_usd(amount: f64) -> String {
    if amount == 0.0 {
        "$0.00".to_string()
    } else {
        format_usd_decimal(amount, display_decimals(amount))
    }
}

fn format_usd_decimal(amount: f64, decimals: usize) -> String {
    let factor = 10_u64.pow(decimals as u32);
    let scaled = (amount.abs() * factor as f64 + 1e-9).round() as u64;
    let whole = scaled / factor;
    let fraction = scaled % factor;
    let sign = if amount.is_sign_negative() { "-" } else { "" };
    format!("${sign}{whole}.{fraction:0decimals$}")
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

    #[test]
    fn estimates_bedrock_anthropic_runtime_model_cost() {
        let estimate = estimate_usage_cost(
            "anthropic.claude-sonnet-4-5-20250929-v1:0",
            "bedrock",
            &usage(1_000, 200, 0, 0),
        );

        assert_eq!(estimate.status, CostStatus::Estimated);
        assert_eq!(estimate.source, CostSource::OfficialDocsSnapshot);
        assert_eq!(estimate.label, "~$0.006000");
        assert_eq!(
            estimate.pricing_version,
            Some("bedrock-pricing-2026-04".to_string())
        );
    }

    #[test]
    fn bedrock_model_prefix_can_select_amazon_nova_pricing() {
        let estimate = estimate_usage_cost(
            "bedrock/amazon.nova-lite-v1:0",
            "openai-compatible",
            &usage(2_000, 1_000, 0, 0),
        );

        assert_eq!(estimate.status, CostStatus::Estimated);
        assert_eq!(estimate.label, "~$0.000360");
        assert_eq!(
            estimate.pricing_version,
            Some("bedrock-pricing-2026-04".to_string())
        );
    }

    #[test]
    fn parses_openrouter_models_pricing_payload() {
        let catalog = openrouter_models_pricing_from_payload(&serde_json::json!({
            "data": [
                {
                    "id": "Acme/New-Model",
                    "pricing": {
                        "prompt": "0.00000015",
                        "completion": "0.00000060",
                        "input_cache_read": "0.00000002",
                        "input_cache_write": 0.00000020,
                        "request": "0.00025"
                    }
                },
                {"id": "skip/no-pricing"}
            ]
        }));

        let entry = catalog.get("acme/new-model").unwrap();
        assert_eq!(entry.input_per_million, Some(0.15));
        assert_eq!(entry.output_per_million, Some(0.60));
        assert_eq!(entry.cache_read_per_million, Some(0.02));
        assert_eq!(entry.cache_write_per_million, Some(0.20));
        assert_eq!(entry.request_cost, Some(0.00025));
        assert!(!catalog.contains_key("skip/no-pricing"));
    }

    #[test]
    fn estimates_cost_from_live_provider_models_pricing() {
        let catalog = openrouter_models_pricing_from_payload(&serde_json::json!({
            "data": [{
                "id": "acme/new-model",
                "pricing": {
                    "prompt": "0.00000015",
                    "completion": "0.00000060",
                    "input_cache_read": "0.00000002",
                    "input_cache_write": "0.00000020"
                }
            }]
        }));

        let estimate = estimate_usage_cost_with_live_pricing(
            "ACME/New-Model",
            "openrouter",
            &usage(2_000, 500, 250, 50),
            &catalog,
        );

        assert_eq!(estimate.status, CostStatus::Estimated);
        assert_eq!(estimate.source, CostSource::ProviderModelsApi);
        assert_eq!(
            estimate.pricing_version,
            Some("provider-models-api".to_string())
        );
        assert_eq!(estimate.label, "~$0.000568");
        assert!((estimate.amount_usd.unwrap() - 0.0005675).abs() < 1e-12);
    }

    #[test]
    fn live_pricing_missing_cache_rate_returns_unknown() {
        let catalog = openrouter_models_pricing_from_payload(&serde_json::json!({
            "data": [{
                "id": "acme/new-model",
                "pricing": {
                    "prompt": "0.00000015",
                    "completion": "0.00000060"
                }
            }]
        }));

        let estimate = estimate_usage_cost_with_live_pricing(
            "acme/new-model",
            "openrouter",
            &usage(2_000, 500, 250, 0),
            &catalog,
        );

        assert_eq!(estimate.status, CostStatus::Unknown);
        assert!(estimate.notes[0].contains("cache-read pricing unavailable"));
    }

    #[test]
    fn live_provider_models_pricing_adds_request_costs() {
        let catalog = openrouter_models_pricing_from_payload(&serde_json::json!({
            "data": [{
                "id": "acme/request-priced",
                "pricing": {
                    "prompt": "0.00000015",
                    "completion": "0.00000060",
                    "request": "0.002"
                }
            }]
        }));

        let estimate = estimate_usage_cost_with_live_pricing_and_requests(
            "acme/request-priced",
            "openrouter",
            &usage(2_000, 500, 0, 0),
            &catalog,
            3,
        );

        assert_eq!(estimate.status, CostStatus::Estimated);
        assert_eq!(estimate.source, CostSource::ProviderModelsApi);
        assert_eq!(estimate.label, "~$0.006600");
        assert!((estimate.amount_usd.unwrap() - 0.0066).abs() < 1e-12);
        assert!(
            estimate
                .notes
                .iter()
                .any(|note| note.contains("3 API call"))
        );
    }

    #[test]
    fn live_provider_models_pricing_accepts_request_only_entries() {
        let catalog = openrouter_models_pricing_from_payload(&serde_json::json!({
            "data": [{
                "id": "acme/request-only",
                "pricing": {
                    "request": "0.005"
                }
            }]
        }));

        let estimate = estimate_usage_cost_with_live_pricing_and_requests(
            "acme/request-only",
            "openrouter",
            &usage(0, 0, 0, 0),
            &catalog,
            2,
        );

        assert_eq!(estimate.status, CostStatus::Estimated);
        assert_eq!(estimate.label, "~$0.0100");
        assert_eq!(estimate.amount_usd, Some(0.01));
    }

    #[test]
    fn live_pricing_cache_round_trips_fresh_catalog() {
        let fetched_at = DateTime::parse_from_rfc3339("2026-06-03T01:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let path = std::env::temp_dir().join(format!(
            "hakimi-live-pricing-cache-{}.json",
            uuid::Uuid::new_v4()
        ));
        let mut catalog = LivePricingCatalog::new();
        catalog.insert(
            "openrouter/acme".to_string(),
            LivePricingEntry {
                input_per_million: Some(0.15),
                output_per_million: Some(0.60),
                cache_read_per_million: None,
                cache_write_per_million: None,
                request_cost: None,
            },
        );
        let cache = LivePricingCache::new(
            "openrouter",
            "https://openrouter.ai/api/v1",
            catalog.clone(),
            fetched_at,
            Duration::seconds(60),
        );

        save_live_pricing_cache(&path, &cache).unwrap();

        let loaded = load_fresh_live_pricing_cache(&path, fetched_at + Duration::seconds(30))
            .unwrap()
            .unwrap();
        assert_eq!(loaded.provider, "openrouter");
        assert_eq!(loaded.base_url, "https://openrouter.ai/api/v1");
        assert_eq!(loaded.catalog, catalog);
        assert!(
            load_fresh_live_pricing_cache(&path, fetched_at + Duration::seconds(61))
                .unwrap()
                .is_none()
        );
        let _ = std::fs::remove_file(path);
    }
}
