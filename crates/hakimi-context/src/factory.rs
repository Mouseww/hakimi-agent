use hakimi_transports::ProviderTransport;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{ContextEngine, LlmCompressor, SimpleContextEngine, SmartContextEngine};

fn normalize_engine_name(engine: &str) -> String {
    engine.trim().to_ascii_lowercase().replace('-', "_")
}

fn optional_model(model: Option<&str>) -> Option<String> {
    model
        .map(str::trim)
        .filter(|model| !model.is_empty())
        .map(str::to_string)
}

/// Build a context engine from config-facing names.
///
/// Supported engines:
/// - `simple`: truncation-only context compression.
/// - `smart`: three-tier local compression.
/// - `llm`: Hermes-style LLM-backed summarization with local fallback.
pub fn build_context_engine(
    engine: &str,
    context_length: usize,
    compression_model: Option<&str>,
    transport: Option<Arc<dyn ProviderTransport>>,
) -> Arc<RwLock<dyn ContextEngine>> {
    match normalize_engine_name(engine).as_str() {
        "simple" => Arc::new(RwLock::new(SimpleContextEngine::new(context_length))),
        "llm" | "llm_compressor" => {
            let compressor = LlmCompressor::new(context_length);
            let compressor = match (transport, optional_model(compression_model)) {
                (Some(transport), Some(model)) => compressor.with_llm(transport, model),
                _ => compressor,
            };
            Arc::new(RwLock::new(compressor))
        }
        _ => Arc::new(RwLock::new(SmartContextEngine::new(
            context_length,
            optional_model(compression_model),
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn build_context_engine_defaults_unknown_names_to_smart() {
        let engine = build_context_engine("unknown", 4096, None, None);
        let engine = engine.read().await;

        assert_eq!(engine.name(), "smart");
        assert_eq!(engine.context_length(), 4096);
    }

    #[tokio::test]
    async fn build_context_engine_selects_simple() {
        let engine = build_context_engine("simple", 8192, None, None);
        let engine = engine.read().await;

        assert_eq!(engine.name(), "simple");
        assert_eq!(engine.context_length(), 8192);
    }

    #[tokio::test]
    async fn build_context_engine_selects_llm_alias() {
        let engine = build_context_engine("llm-compressor", 128_000, Some("claude-haiku"), None);
        let engine = engine.read().await;

        assert_eq!(engine.name(), "llm-compressor");
        assert_eq!(engine.context_length(), 128_000);
    }
}
