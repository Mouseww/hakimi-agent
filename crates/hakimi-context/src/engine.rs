use async_trait::async_trait;
use hakimi_common::{Message, Result, Usage};

/// Cumulative compression statistics for a session.
#[derive(Debug, Clone)]
pub struct CompressionStats {
    /// Total number of compression passes performed.
    pub compression_count: usize,
    /// Total estimated tokens saved across all compressions.
    pub total_tokens_saved: usize,
}

/// Trait for managing conversation context (token tracking, compression, lifecycle).
#[async_trait]
pub trait ContextEngine: Send + Sync {
    /// Human-readable name of this engine.
    fn name(&self) -> &str;

    /// Update internal usage counters after an LLM response.
    fn update_from_response(&mut self, usage: &Usage);

    /// Returns `true` when context compression should be triggered.
    fn should_compress(&self) -> bool;

    /// Compress the conversation history in-place.
    async fn compress(&self, messages: &mut Vec<Message>) -> Result<()>;

    /// Called when a new session begins.
    fn on_session_start(&mut self);

    /// Called when a session ends.
    fn on_session_end(&mut self);

    /// The maximum context length (in tokens) this engine supports.
    fn context_length(&self) -> usize;

    /// Returns cumulative compression statistics, if available.
    ///
    /// Engines that track compression stats should override this.
    /// The default implementation returns `None`.
    fn compression_stats(&self) -> Option<CompressionStats> {
        None
    }
}
