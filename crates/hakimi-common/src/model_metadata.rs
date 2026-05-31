use serde::{Deserialize, Serialize};

/// Default context length when no model-specific metadata is known.
///
/// Hermes probes unknown models from this tier first, so Hakimi uses the same
/// offline fallback while leaving provider live discovery for a later slice.
pub const DEFAULT_FALLBACK_CONTEXT_LENGTH: usize = 256_000;

/// Minimum context window Hakimi expects for tool-calling agent workflows.
pub const MINIMUM_CONTEXT_LENGTH: usize = 64_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelContextLengthSource {
    ConfigOverride,
    StaticMetadata,
    Fallback,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelContextLength {
    pub context_length: usize,
    pub minimum_context_length: usize,
    pub source: ModelContextLengthSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub matched_pattern: Option<String>,
}

impl ModelContextLength {
    fn new(
        context_length: usize,
        source: ModelContextLengthSource,
        matched_pattern: Option<&str>,
    ) -> Self {
        Self {
            context_length,
            minimum_context_length: MINIMUM_CONTEXT_LENGTH,
            source,
            matched_pattern: matched_pattern.map(str::to_string),
        }
    }

    pub fn is_below_minimum(&self) -> bool {
        self.context_length < self.minimum_context_length
    }
}

#[derive(Debug, Clone, Copy)]
struct ContextLengthEntry {
    pattern: &'static str,
    length: usize,
}

const CONTEXT_LENGTHS: &[ContextLengthEntry] = &[
    ContextLengthEntry {
        pattern: "claude-opus-4-8",
        length: 1_000_000,
    },
    ContextLengthEntry {
        pattern: "claude-opus-4.8",
        length: 1_000_000,
    },
    ContextLengthEntry {
        pattern: "claude-opus-4-7",
        length: 1_000_000,
    },
    ContextLengthEntry {
        pattern: "claude-opus-4.7",
        length: 1_000_000,
    },
    ContextLengthEntry {
        pattern: "claude-opus-4-6",
        length: 1_000_000,
    },
    ContextLengthEntry {
        pattern: "claude-sonnet-4-6",
        length: 1_000_000,
    },
    ContextLengthEntry {
        pattern: "claude-opus-4.6",
        length: 1_000_000,
    },
    ContextLengthEntry {
        pattern: "claude-sonnet-4.6",
        length: 1_000_000,
    },
    ContextLengthEntry {
        pattern: "claude",
        length: 200_000,
    },
    ContextLengthEntry {
        pattern: "gpt-5.5",
        length: 1_050_000,
    },
    ContextLengthEntry {
        pattern: "gpt-5.4-nano",
        length: 400_000,
    },
    ContextLengthEntry {
        pattern: "gpt-5.4-mini",
        length: 400_000,
    },
    ContextLengthEntry {
        pattern: "gpt-5.4",
        length: 1_050_000,
    },
    ContextLengthEntry {
        pattern: "gpt-5.3-codex-spark",
        length: 128_000,
    },
    ContextLengthEntry {
        pattern: "gpt-5.1-chat",
        length: 128_000,
    },
    ContextLengthEntry {
        pattern: "gpt-5",
        length: 400_000,
    },
    ContextLengthEntry {
        pattern: "gpt-4.1",
        length: 1_047_576,
    },
    ContextLengthEntry {
        pattern: "gpt-4",
        length: 128_000,
    },
    ContextLengthEntry {
        pattern: "gemini",
        length: 1_048_576,
    },
    ContextLengthEntry {
        pattern: "gemma-4-31b",
        length: 256_000,
    },
    ContextLengthEntry {
        pattern: "gemma-4",
        length: 256_000,
    },
    ContextLengthEntry {
        pattern: "gemma4",
        length: 256_000,
    },
    ContextLengthEntry {
        pattern: "kimi",
        length: 128_000,
    },
    ContextLengthEntry {
        pattern: "qwen",
        length: 128_000,
    },
    ContextLengthEntry {
        pattern: "deepseek",
        length: 128_000,
    },
    ContextLengthEntry {
        pattern: "minimax",
        length: 1_000_000,
    },
];

const PROVIDER_PREFIXES: &[&str] = &[
    "openrouter",
    "nous",
    "openai-codex",
    "copilot",
    "copilot-acp",
    "gemini",
    "anthropic",
    "deepseek",
    "custom",
    "local",
    "google",
    "google-gemini",
    "github",
    "github-copilot",
    "github-models",
    "kimi",
    "moonshot",
    "claude",
    "ollama",
    "qwen",
    "xai",
    "x-ai",
    "x.ai",
    "grok",
    "bedrock",
];

/// Resolve the effective context length for a model.
///
/// Resolution order mirrors Hermes' safe offline subset:
/// 1. explicit config override,
/// 2. provider-prefixed model normalization + longest static metadata match,
/// 3. fallback context length.
pub fn resolve_model_context_length(
    model: &str,
    config_context_length: Option<usize>,
    fallback_context_length: usize,
) -> ModelContextLength {
    if let Some(length) = config_context_length.filter(|length| *length > 0) {
        return ModelContextLength::new(length, ModelContextLengthSource::ConfigOverride, None);
    }

    let normalized = normalize_model_id(model);
    if let Some(entry) = lookup_static_context_length(&normalized) {
        return ModelContextLength::new(
            entry.length,
            ModelContextLengthSource::StaticMetadata,
            Some(entry.pattern),
        );
    }

    ModelContextLength::new(
        fallback_context_length.max(1),
        ModelContextLengthSource::Fallback,
        None,
    )
}

pub fn normalize_model_id(model: &str) -> String {
    let trimmed = model.trim();
    let Some((prefix, suffix)) = trimmed.split_once(':') else {
        return trimmed.to_string();
    };
    if trimmed.starts_with("http") {
        return trimmed.to_string();
    }

    let prefix_lower = prefix.trim().to_ascii_lowercase();
    let suffix = suffix.trim();
    if PROVIDER_PREFIXES.contains(&prefix_lower.as_str()) && !looks_like_ollama_tag(suffix) {
        suffix.to_string()
    } else {
        trimmed.to_string()
    }
}

fn lookup_static_context_length(model: &str) -> Option<ContextLengthEntry> {
    let model_lower = model.to_ascii_lowercase();
    CONTEXT_LENGTHS
        .iter()
        .filter(|entry| model_lower.contains(entry.pattern))
        .max_by_key(|entry| entry.pattern.len())
        .copied()
}

fn looks_like_ollama_tag(suffix: &str) -> bool {
    let suffix = suffix.trim().to_ascii_lowercase();
    if suffix.is_empty() {
        return false;
    }
    suffix == "latest"
        || suffix == "stable"
        || suffix.ends_with('b') && suffix.chars().next().is_some_and(|ch| ch.is_ascii_digit())
        || suffix.starts_with('q') && suffix.chars().nth(1).is_some_and(|ch| ch.is_ascii_digit())
        || suffix.starts_with("fp")
        || suffix.starts_with("instruct")
        || suffix.starts_with("chat")
        || suffix.starts_with("coder")
        || suffix.starts_with("vision")
        || suffix.starts_with("text")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_config_context_length_wins() {
        let resolved = resolve_model_context_length("gpt-4.1", Some(32_000), 256_000);

        assert_eq!(resolved.context_length, 32_000);
        assert_eq!(resolved.source, ModelContextLengthSource::ConfigOverride);
        assert!(resolved.is_below_minimum());
    }

    #[test]
    fn longest_static_pattern_wins() {
        let resolved = resolve_model_context_length("openai:gpt-5.4-mini", None, 256_000);

        assert_eq!(resolved.context_length, 400_000);
        assert_eq!(resolved.source, ModelContextLengthSource::StaticMetadata);
        assert_eq!(resolved.matched_pattern.as_deref(), Some("gpt-5.4-mini"));
    }

    #[test]
    fn provider_prefix_is_removed_but_ollama_tags_are_preserved() {
        assert_eq!(
            normalize_model_id("openrouter:anthropic/claude-sonnet-4-6"),
            "anthropic/claude-sonnet-4-6"
        );
        assert_eq!(normalize_model_id("qwen:7b"), "qwen:7b");
        assert_eq!(normalize_model_id("deepseek:latest"), "deepseek:latest");
    }

    #[test]
    fn unknown_model_uses_fallback() {
        let resolved = resolve_model_context_length("local-special-model", None, 96_000);

        assert_eq!(resolved.context_length, 96_000);
        assert_eq!(resolved.source, ModelContextLengthSource::Fallback);
    }
}
