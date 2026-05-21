use async_trait::async_trait;
use hakimi_common::{Message, MessageRole, Result, Usage};
use tracing::{debug, info, warn};

use crate::engine::ContextEngine;

/// Compression triggers when usage exceeds this fraction of the context window.
const COMPRESSION_THRESHOLD: f64 = 0.75;

/// Number of messages at the start of the conversation to protect from compression.
const PROTECT_FIRST: usize = 3;

/// Number of messages at the end of the conversation to protect from compression.
const PROTECT_LAST: usize = 6;

/// A context engine that tracks token usage and compresses conversation history
/// when usage exceeds a configurable threshold.
pub struct ContextCompressor {
    /// Name of this engine.
    name: String,

    /// Maximum context length in tokens.
    context_length: usize,

    /// Accumulated prompt tokens from the latest response.
    current_prompt_tokens: u32,

    /// Accumulated completion tokens from the latest response.
    current_completion_tokens: u32,

    /// Whether compression was requested but not yet performed.
    needs_compression: bool,
}

impl ContextCompressor {
    /// Create a new compressor with the given context window size.
    pub fn new(context_length: usize) -> Self {
        Self {
            name: "default-compressor".to_string(),
            context_length,
            current_prompt_tokens: 0,
            current_completion_tokens: 0,
            needs_compression: false,
        }
    }

    /// Create a new compressor with a custom name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Returns the current total token usage.
    fn total_usage(&self) -> u32 {
        self.current_prompt_tokens + self.current_completion_tokens
    }

    /// Returns the compression threshold in absolute tokens.
    fn threshold_tokens(&self) -> u32 {
        (self.context_length as f64 * COMPRESSION_THRESHOLD) as u32
    }
}

#[async_trait]
impl ContextEngine for ContextCompressor {
    fn name(&self) -> &str {
        &self.name
    }

    fn update_from_response(&mut self, usage: &Usage) {
        self.current_prompt_tokens = usage.prompt_tokens;
        self.current_completion_tokens = usage.completion_tokens;

        debug!(
            prompt = self.current_prompt_tokens,
            completion = self.current_completion_tokens,
            total = self.total_usage(),
            threshold = self.threshold_tokens(),
            "Context usage updated"
        );

        if self.total_usage() > self.threshold_tokens() {
            self.needs_compression = true;
            info!(
                usage = self.total_usage(),
                threshold = self.threshold_tokens(),
                "Context compression threshold exceeded"
            );
        }
    }

    fn should_compress(&self) -> bool {
        self.needs_compression
    }

    async fn compress(&self, messages: &mut Vec<Message>) -> Result<()> {
        let total = messages.len();

        if total <= PROTECT_FIRST + PROTECT_LAST + 1 {
            warn!(
                message_count = total,
                "Not enough messages to compress; skipping"
            );
            return Ok(());
        }

        info!(
            message_count = total,
            protect_first = PROTECT_FIRST,
            protect_last = PROTECT_LAST,
            "Compressing conversation context"
        );

        // The range of messages eligible for compression (everything in the middle).
        let compress_start = PROTECT_FIRST;
        let compress_end = total - PROTECT_LAST;

        // Build a summary of the compressed messages.
        let mut summary_parts: Vec<String> = Vec::new();
        for msg in &messages[compress_start..compress_end] {
            let role = &msg.role;
            let preview = msg
                .content
                .as_deref()
                .map(|c| {
                    if c.len() > 200 {
                        format!("{}…", &c[..200])
                    } else {
                        c.to_string()
                    }
                })
                .unwrap_or_else(|| "(no content)".to_string());
            summary_parts.push(format!("[{role}] {preview}"));
        }

        let summary = format!(
            "<context-compression>\n\
             The following {} messages were compressed into this summary:\n\n\
             {}\n\
             </context-compression>",
            summary_parts.len(),
            summary_parts.join("\n\n")
        );

        // Replace the middle messages with a single summary message.
        let summary_msg = Message {
            role: MessageRole::System,
            content: Some(summary),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: Some(chrono::Utc::now()),
            token_count: None,
            finish_reason: None,
        };

        // Drain the middle and insert the summary.
        messages.drain(compress_start..compress_end);
        messages.insert(compress_start, summary_msg);

        info!(
            new_count = messages.len(),
            removed = compress_end - compress_start - 1,
            "Context compression complete"
        );

        Ok(())
    }

    fn on_session_start(&mut self) {
        self.current_prompt_tokens = 0;
        self.current_completion_tokens = 0;
        self.needs_compression = false;
        info!("Session started — context counters reset");
    }

    fn on_session_end(&mut self) {
        info!(
            final_prompt_tokens = self.current_prompt_tokens,
            final_completion_tokens = self.current_completion_tokens,
            "Session ended"
        );
        self.needs_compression = false;
    }

    fn context_length(&self) -> usize {
        self.context_length
    }
}
