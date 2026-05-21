use async_trait::async_trait;
use hakimi_common::{Message, Result, Usage};

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
}
