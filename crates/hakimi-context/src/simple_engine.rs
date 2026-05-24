use async_trait::async_trait;
use hakimi_common::{Message, MessageRole, Result, Usage};
use tracing::debug;

use crate::engine::ContextEngine;

/// A simple context engine that tracks token count via a rough character-based
/// estimate and compresses by truncating old messages.
pub struct SimpleContextEngine {
    /// Maximum context length in tokens.
    context_length: usize,
    /// Estimated tokens used so far.
    estimated_tokens: usize,
}

impl SimpleContextEngine {
    /// Create a new simple context engine with the given context length (in tokens).
    pub fn new(context_length: usize) -> Self {
        Self {
            context_length,
            estimated_tokens: 0,
        }
    }

    /// Rough estimate: 1 token ≈ 4 characters (conservative for English text).
    fn estimate_tokens(text: &str) -> usize {
        text.len().div_ceil(4)
    }
}

#[async_trait]
impl ContextEngine for SimpleContextEngine {
    fn name(&self) -> &str {
        "simple"
    }

    fn update_from_response(&mut self, usage: &Usage) {
        self.estimated_tokens = usage.total_tokens as usize;
    }

    fn should_compress(&self) -> bool {
        let threshold = (self.context_length as f64 * 0.80) as usize;
        self.estimated_tokens > threshold
    }

    async fn compress(&self, messages: &mut Vec<Message>) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }

        let target_tokens = (self.context_length as f64 * 0.50) as usize;
        let mut used_tokens: usize = 0;

        // Keep messages from the end, accumulating token estimates, until we
        // would exceed the target.  We always keep at least the last message.
        let mut keep_from = messages.len();
        for (i, msg) in messages.iter().enumerate().rev() {
            let msg_tokens = msg
                .content
                .as_deref()
                .map(Self::estimate_tokens)
                .unwrap_or(0);
            if used_tokens + msg_tokens > target_tokens && i < messages.len() - 1 {
                break;
            }
            used_tokens += msg_tokens;
            keep_from = i;
        }

        if keep_from > 0 {
            debug!(
                dropped = keep_from,
                remaining = messages.len() - keep_from,
                "compressing context by dropping old messages"
            );
            // Replace a system-like summary message at the front, then keep tail.
            let dropped_summary = format!(
                "[Context compressed: {} earlier messages were dropped to stay within token limits.]",
                keep_from
            );
            let summary_msg = Message {
                role: MessageRole::System,
                content: Some(dropped_summary),
                images: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning: None,
                reasoning_content: None,
                timestamp: None,
                token_count: None,
                finish_reason: None,
            };
            let tail: Vec<Message> = messages[keep_from..].to_vec();
            messages.clear();
            messages.push(summary_msg);
            messages.extend(tail);
        }

        Ok(())
    }

    fn on_session_start(&mut self) {
        self.estimated_tokens = 0;
    }

    fn on_session_end(&mut self) {
        // No-op for this simple implementation.
    }

    fn context_length(&self) -> usize {
        self.context_length
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_compress_below_threshold() {
        let mut engine = SimpleContextEngine::new(1000);
        engine.estimated_tokens = 700; // 70% — below 80%
        assert!(!engine.should_compress());
    }

    #[test]
    fn test_should_compress_above_threshold() {
        let mut engine = SimpleContextEngine::new(1000);
        engine.estimated_tokens = 850; // 85% — above 80%
        assert!(engine.should_compress());
    }

    #[tokio::test]
    async fn test_compress_drops_old_messages() {
        let engine = SimpleContextEngine::new(100);
        // Each message ~50 chars = ~13 tokens. 10 messages ≈ 130 tokens.
        // Target = 50 tokens → should compress.
        let mut messages: Vec<Message> = (0..10)
            .map(|i| {
                Message::user(format!(
                    "This is a test message number {i} with some padding text."
                ))
            })
            .collect();
        engine.compress(&mut messages).await.unwrap();
        // Should keep only some tail messages and a compression summary
        assert!(messages.len() < 11);
        assert!(messages.len() >= 2);
        // First message should be the compression summary
        assert!(
            messages[0]
                .content
                .as_ref()
                .unwrap()
                .contains("Context compressed")
        );
    }
}
