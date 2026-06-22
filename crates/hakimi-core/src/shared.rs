use std::sync::Arc;

use hakimi_tools::ToolRegistry;
use hakimi_transports::{EmbeddingProvider, ProviderTransport};

/// Resources shared across all personas (agents) in a single instance.
///
/// Constructed once and shared via [`Arc`] so that N personas can run
/// concurrently without duplicating heavy resources. Per-persona state
/// (model, system prompt, skills, context engine, messages) lives on
/// [`AIAgent`](crate::AIAgent) directly, not here.
///
/// Derives `Clone` (every field is `Arc`-backed or cheaply cloneable) so that
/// post-construction setters can mutate it in place via [`Arc::make_mut`].
#[derive(Clone)]
pub struct SharedRuntime {
    /// The LLM provider transport (connection + credential pool live inside).
    pub transport: Arc<dyn ProviderTransport>,
    /// The tool registry (its internals are already `Arc`-shared).
    pub tool_registry: ToolRegistry,
    /// Optional knowledge-graph searcher, shared across personas.
    pub knowledge_searcher: Option<Arc<dyn hakimi_common::KnowledgeSearcher>>,
    /// Optional embedding provider, shared across personas.
    pub embedding_provider: Option<Arc<dyn EmbeddingProvider>>,
}
