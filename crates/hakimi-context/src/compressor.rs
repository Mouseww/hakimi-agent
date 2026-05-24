use async_trait::async_trait;
use hakimi_common::{Message, MessageRole, Result, Usage};
use hakimi_transports::{ProviderTransport, RequestParams};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::engine::{CompressionStats, ContextEngine};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Compression triggers when usage exceeds this fraction of the context window.
const COMPRESSION_THRESHOLD: f64 = 0.75;

/// Number of messages at the start of the conversation to protect from compression.
const PROTECT_FIRST: usize = 3;

/// Number of messages at the end of the conversation to protect from compression.
const PROTECT_LAST: usize = 6;

/// Tool output content longer than this (in chars) is pruned before summarization.
const TOOL_OUTPUT_PRUNE_THRESHOLD: usize = 500;

/// Maximum character length for a single message preview in the local summary.
const PREVIEW_MAX_CHARS: usize = 200;

// ---------------------------------------------------------------------------
// Question tracking
// ---------------------------------------------------------------------------

/// A question extracted from the conversation, tracked for resolved/pending status.
#[derive(Debug, Clone)]
struct TrackedQuestion {
    text: String,
    resolved: bool,
}

// ---------------------------------------------------------------------------
// ContextCompressor — original simple compressor (unchanged API)
// ---------------------------------------------------------------------------

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
                    if c.len() > PREVIEW_MAX_CHARS {
                        format!("{}…", &c[..PREVIEW_MAX_CHARS])
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
            images: None,
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

// ---------------------------------------------------------------------------
// LlmCompressor — LLM-backed compressor with question tracking & pruning
// ---------------------------------------------------------------------------

/// A context compressor that can optionally use an auxiliary LLM for
/// higher-quality structured summarization.
///
/// When an LLM transport is provided, the compressor sends the messages to be
/// compressed to the LLM and asks it to produce a structured summary that
/// includes resolved and pending questions. When no LLM is available it falls
/// back to local truncation-based summarization (same algorithm as
/// [`ContextCompressor`]).
///
/// Additional features over the basic compressor:
/// - **Tool output pruning**: Large tool outputs are stripped before
///   summarization, reducing noise and token cost.
/// - **Resolved/Pending question tracking**: User questions are detected and
///   tracked through the conversation; the summary includes which questions
///   were answered (resolved) and which remain open (pending).
/// - **Compression stats**: Tracks cumulative compression statistics across
///   the session via [`CompressionStats`].
pub struct LlmCompressor {
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

    /// Optional LLM transport for high-quality summarization.
    llm_transport: Option<Arc<dyn ProviderTransport>>,

    /// Model name to use for LLM summarization calls.
    compression_model: String,

    /// Questions extracted from the conversation.
    questions: Vec<TrackedQuestion>,

    /// Cumulative compression statistics.
    stats: std::sync::Mutex<CompressionStats>,
}

impl LlmCompressor {
    /// Create a new LLM compressor with the given context window size.
    ///
    /// No LLM transport is configured; falls back to local summarization.
    pub fn new(context_length: usize) -> Self {
        Self {
            name: "llm-compressor".to_string(),
            context_length,
            current_prompt_tokens: 0,
            current_completion_tokens: 0,
            needs_compression: false,
            llm_transport: None,
            compression_model: String::new(),
            questions: Vec::new(),
            stats: std::sync::Mutex::new(CompressionStats {
                compression_count: 0,
                total_tokens_saved: 0,
            }),
        }
    }

    /// Create a new LLM compressor with a custom name.
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set the LLM transport and model for LLM-based summarization.
    pub fn with_llm(
        mut self,
        transport: Arc<dyn ProviderTransport>,
        model: impl Into<String>,
    ) -> Self {
        self.llm_transport = Some(transport);
        self.compression_model = model.into();
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

    /// Rough token estimate: ~4 chars per token.
    #[allow(dead_code)]
    fn estimate_tokens(text: &str) -> usize {
        text.len().div_ceil(4)
    }

    // -----------------------------------------------------------------
    // Tool output pruning
    // -----------------------------------------------------------------

    /// Prune large tool outputs in-place. Returns the number of messages pruned.
    fn prune_tool_outputs(messages: &mut [Message]) -> usize {
        let mut pruned = 0;
        for msg in messages.iter_mut() {
            if msg.role == MessageRole::Tool
                && let Some(ref content) = msg.content
                && content.len() > TOOL_OUTPUT_PRUNE_THRESHOLD
            {
                let orig_len = content.len();
                msg.content = Some(format!(
                    "[Tool output pruned — {} chars reduced to summary]",
                    orig_len
                ));
                pruned += 1;
            }
        }
        pruned
    }

    // -----------------------------------------------------------------
    // Question tracking helpers
    // -----------------------------------------------------------------

    /// Scan messages and update the tracked question list.
    fn update_questions(&mut self, messages: &[Message]) {
        // Collect user messages that look like questions.
        for msg in messages {
            if msg.role == MessageRole::User
                && let Some(ref content) = msg.content
            {
                let trimmed = content.trim();
                // Heuristic: a message is a question if it ends with '?'
                // or starts with common question words.
                let is_question = trimmed.ends_with('?')
                    || trimmed.starts_with("how ")
                    || trimmed.starts_with("what ")
                    || trimmed.starts_with("why ")
                    || trimmed.starts_with("when ")
                    || trimmed.starts_with("where ")
                    || trimmed.starts_with("who ")
                    || trimmed.starts_with("which ")
                    || trimmed.starts_with("can ")
                    || trimmed.starts_with("could ")
                    || trimmed.starts_with("should ")
                    || trimmed.starts_with("is ")
                    || trimmed.starts_with("are ");

                if is_question {
                    // Avoid duplicates by checking if we already track this question.
                    let already_tracked = self.questions.iter().any(|q| q.text == trimmed);
                    if !already_tracked {
                        self.questions.push(TrackedQuestion {
                            text: trimmed.to_string(),
                            resolved: false,
                        });
                    }
                }
            }
        }

        // Mark questions as resolved if a subsequent assistant message exists
        // after the question.
        let mut question_indices: Vec<usize> = Vec::new();
        for (i, msg) in messages.iter().enumerate() {
            if msg.role == MessageRole::User
                && let Some(ref content) = msg.content
            {
                let trimmed = content.trim();
                if self
                    .questions
                    .iter()
                    .any(|q| q.text == trimmed && !q.resolved)
                {
                    question_indices.push(i);
                }
            }
        }

        // For each question position, check if there's an assistant reply after it.
        for &qi in &question_indices {
            // Look for an assistant message after this user message.
            for msg in messages.iter().skip(qi + 1) {
                if msg.role == MessageRole::Assistant && msg.content.is_some() {
                    // Mark the question as resolved.
                    if let Some(q) = self.questions.iter_mut().find(|q| {
                        q.text == messages[qi].content.as_deref().unwrap_or("").trim()
                            && !q.resolved
                    }) {
                        q.resolved = true;
                    }
                    break;
                }
            }
        }
    }

    /// Format resolved/pending questions for the summary.
    fn format_question_status(&self) -> String {
        let resolved: Vec<&str> = self
            .questions
            .iter()
            .filter(|q| q.resolved)
            .map(|q| q.text.as_str())
            .collect();
        let pending: Vec<&str> = self
            .questions
            .iter()
            .filter(|q| !q.resolved)
            .map(|q| q.text.as_str())
            .collect();

        let mut parts = Vec::new();
        if !resolved.is_empty() {
            parts.push(format!(
                "Resolved questions:\n{}",
                resolved
                    .iter()
                    .map(|q| format!("  - {q}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if !pending.is_empty() {
            parts.push(format!(
                "Pending questions:\n{}",
                pending
                    .iter()
                    .map(|q| format!("  - {q}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
        if parts.is_empty() {
            "(no questions tracked)".to_string()
        } else {
            parts.join("\n\n")
        }
    }

    // -----------------------------------------------------------------
    // Summarization strategies
    // -----------------------------------------------------------------

    /// Build a local (non-LLM) summary of the given messages, including
    /// question tracking and tool output status.
    fn build_local_summary(messages: &[Message], question_status: &str) -> String {
        let mut summary_parts: Vec<String> = Vec::new();
        for msg in messages {
            let role = &msg.role;
            let preview = msg
                .content
                .as_deref()
                .map(|c| {
                    if c.len() > PREVIEW_MAX_CHARS {
                        format!("{}…", &c[..PREVIEW_MAX_CHARS])
                    } else {
                        c.to_string()
                    }
                })
                .unwrap_or_else(|| "(no content)".to_string());
            summary_parts.push(format!("[{role}] {preview}"));
        }

        format!(
            "<context-compression>\n\
             {count} earlier messages were compressed into this summary.\n\n\
             {body}\n\n\
             ---\n\
             Question tracking:\n\
             {question_status}\n\
             </context-compression>",
            count = summary_parts.len(),
            body = summary_parts.join("\n\n"),
        )
    }

    /// Attempt LLM-based summarization. Falls back to local summarization
    /// on any error or if no transport is configured.
    async fn summarize_messages(&self, messages: &[Message], question_status: &str) -> String {
        if let Some(ref transport) = self.llm_transport {
            // Build a summarization prompt.
            let mut conversation_text = String::new();
            for msg in messages {
                let role = &msg.role;
                let content = msg.content.as_deref().unwrap_or("(no content)");
                conversation_text.push_str(&format!("[{role}] {content}\n"));
            }

            let prompt = format!(
                "You are a context compression assistant. Summarize the following \
                 conversation concisely, preserving key facts, decisions, and action items.\n\n\
                 Question tracking:\n{question_status}\n\n\
                 Conversation to summarize:\n{conversation_text}\n\n\
                 Provide a structured summary with sections for: Key Facts, \
                 Decisions Made, Action Items, and Question Status."
            );

            let msg = Message::user(prompt);
            let params = RequestParams {
                temperature: Some(0.3),
                max_tokens: Some(1024),
                ..Default::default()
            };

            match transport
                .execute(&self.compression_model, &[msg], &[], &params)
                .await
            {
                Ok(response) => {
                    if let Some(content) = response.content {
                        info!("LLM-based compression succeeded");
                        return format!(
                            "<context-compression method=\"llm\">\n\
                             {content}\n\n\
                             ---\n\
                             Question tracking:\n\
                             {question_status}\n\
                             </context-compression>"
                        );
                    }
                    warn!("LLM returned empty content; falling back to local summary");
                }
                Err(e) => {
                    warn!(error = %e, "LLM summarization failed; falling back to local summary");
                }
            }
        }

        // Fallback: local summarization.
        Self::build_local_summary(messages, question_status)
    }
}

#[async_trait]
impl ContextEngine for LlmCompressor {
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
            "LlmCompressor: context usage updated"
        );

        if self.total_usage() > self.threshold_tokens() {
            self.needs_compression = true;
            info!(
                usage = self.total_usage(),
                threshold = self.threshold_tokens(),
                "LlmCompressor: context compression threshold exceeded"
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
                "LlmCompressor: not enough messages to compress; skipping"
            );
            return Ok(());
        }

        info!(
            message_count = total,
            protect_first = PROTECT_FIRST,
            protect_last = PROTECT_LAST,
            "LlmCompressor: compressing conversation context"
        );

        let compress_start = PROTECT_FIRST;
        let compress_end = total - PROTECT_LAST;

        // Extract the middle messages (those eligible for compression).
        let middle = &messages[compress_start..compress_end];

        // Step 1: Update question tracking from the full conversation.
        // Note: we use a local copy because `compress` takes `&self`.
        let question_status = {
            // Reconstruct question state from the messages we're about to compress.
            let mut temp_compressor = LlmCompressor::new(self.context_length);
            temp_compressor.update_questions(messages);
            temp_compressor.format_question_status()
        };

        // Step 2: Prune large tool outputs in the middle region.
        // We clone the middle messages so we can prune without mutating the
        // original until we're ready.
        let mut pruned_middle: Vec<Message> = middle.to_vec();
        let pruned_count = Self::prune_tool_outputs(&mut pruned_middle);
        if pruned_count > 0 {
            info!(
                pruned_count = pruned_count,
                "Pruned large tool outputs before summarization"
            );
        }

        // Step 3: Generate summary (LLM or local).
        let summary_text = self
            .summarize_messages(&pruned_middle, &question_status)
            .await;

        let summary_msg = Message {
            role: MessageRole::System,
            content: Some(summary_text),
            images: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: Some(chrono::Utc::now()),
            token_count: None,
            finish_reason: None,
        };

        // Calculate removed chars for stats before mutating messages.
        let removed_chars: usize = middle
            .iter()
            .filter_map(|m| m.content.as_ref().map(|c| c.len()))
            .sum();

        // Drain the middle and insert the summary.
        let removed = compress_end - compress_start;
        messages.drain(compress_start..compress_end);
        messages.insert(compress_start, summary_msg);

        // Update cumulative stats.
        if let Ok(mut stats) = self.stats.lock() {
            stats.compression_count += 1;
            let saved = removed_chars.div_ceil(4);
            stats.total_tokens_saved += saved;
        }

        info!(
            new_count = messages.len(),
            removed = removed,
            pruned_tool_outputs = pruned_count,
            "LlmCompressor: context compression complete"
        );

        Ok(())
    }

    fn on_session_start(&mut self) {
        self.current_prompt_tokens = 0;
        self.current_completion_tokens = 0;
        self.needs_compression = false;
        self.questions.clear();
        if let Ok(mut stats) = self.stats.lock() {
            stats.compression_count = 0;
            stats.total_tokens_saved = 0;
        }
        info!("LlmCompressor: session started — counters reset");
    }

    fn on_session_end(&mut self) {
        let q_count = self.questions.len();
        let resolved = self.questions.iter().filter(|q| q.resolved).count();
        info!(
            final_prompt_tokens = self.current_prompt_tokens,
            final_completion_tokens = self.current_completion_tokens,
            questions_tracked = q_count,
            questions_resolved = resolved,
            "LlmCompressor: session ended"
        );
        self.needs_compression = false;
    }

    fn context_length(&self) -> usize {
        self.context_length
    }

    fn compression_stats(&self) -> Option<CompressionStats> {
        self.stats.lock().ok().map(|s| s.clone())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ── Helper functions ────────────────────────────────────────────────

    fn make_usage(prompt: u32, completion: u32) -> Usage {
        Usage {
            prompt_tokens: prompt,
            completion_tokens: completion,
            total_tokens: prompt + completion,
            ..Usage::default()
        }
    }

    fn make_messages(n: usize) -> Vec<Message> {
        let mut msgs = Vec::with_capacity(n);
        msgs.push(Message::system("You are a helpful assistant."));
        msgs.push(Message::system("Be concise."));
        msgs.push(Message::system("Follow instructions."));
        for i in 0..n.saturating_sub(3) {
            if i % 2 == 0 {
                msgs.push(Message::user(format!("User message {i}")));
            } else {
                msgs.push(Message::assistant(format!("Assistant reply {i}")));
            }
        }
        msgs
    }

    fn make_large_tool_output_message(id: &str, size: usize) -> Message {
        Message::tool_result(id, "code_interpreter", "x".repeat(size))
    }

    // ── ContextCompressor tests ─────────────────────────────────────────

    #[test]
    fn test_context_compressor_new() {
        let c = ContextCompressor::new(8192);
        assert_eq!(c.name(), "default-compressor");
        assert_eq!(c.context_length(), 8192);
        assert!(!c.should_compress());
    }

    #[test]
    fn test_context_compressor_with_name() {
        let c = ContextCompressor::new(4096).with_name("my-compressor");
        assert_eq!(c.name(), "my-compressor");
        assert_eq!(c.context_length(), 4096);
    }

    #[test]
    fn test_threshold_computation() {
        let c = ContextCompressor::new(10000);
        // 75% of 10000 = 7500
        assert_eq!(c.threshold_tokens(), 7500);
    }

    // ── LlmCompressor: new / with_name ──────────────────────────────────

    #[test]
    fn test_llm_compressor_new() {
        let c = LlmCompressor::new(8192);
        assert_eq!(c.name(), "llm-compressor");
        assert_eq!(c.context_length(), 8192);
        assert!(!c.should_compress());
        assert!(c.llm_transport.is_none());
    }

    #[test]
    fn test_llm_compressor_with_name() {
        let c = LlmCompressor::new(4096).with_name("custom-llm");
        assert_eq!(c.name(), "custom-llm");
    }

    // ── LlmCompressor: threshold computation ────────────────────────────

    #[test]
    fn test_llm_compressor_threshold() {
        let c = LlmCompressor::new(10000);
        assert_eq!(c.threshold_tokens(), 7500);

        let c2 = LlmCompressor::new(4000);
        assert_eq!(c2.threshold_tokens(), 3000);
    }

    // ── LlmCompressor: update_from_response ─────────────────────────────

    #[test]
    fn test_llm_compressor_update_below_threshold() {
        let mut c = LlmCompressor::new(10000);
        let usage = make_usage(3000, 2000);
        c.update_from_response(&usage);
        assert!(!c.should_compress());
        assert_eq!(c.current_prompt_tokens, 3000);
        assert_eq!(c.current_completion_tokens, 2000);
    }

    #[test]
    fn test_llm_compressor_update_above_threshold() {
        let mut c = LlmCompressor::new(10000);
        let usage = make_usage(5000, 3000);
        c.update_from_response(&usage);
        // 5000 + 3000 = 8000 > 7500
        assert!(c.should_compress());
    }

    // ── LlmCompressor: should_compress ──────────────────────────────────

    #[test]
    fn test_llm_compressor_should_compress_default_false() {
        let c = LlmCompressor::new(10000);
        assert!(!c.should_compress());
    }

    // ── LlmCompressor: compress with small messages (no-op) ─────────────

    #[tokio::test]
    async fn test_llm_compress_small_messages_noop() {
        let c = LlmCompressor::new(10000);
        // Create fewer messages than PROTECT_FIRST + PROTECT_LAST + 1 = 10
        let mut messages = make_messages(5);
        let original_len = messages.len();
        c.compress(&mut messages).await.unwrap();
        // Should be a no-op
        assert_eq!(messages.len(), original_len);
    }

    // ── LlmCompressor: compress with enough messages ────────────────────

    #[tokio::test]
    async fn test_llm_compress_with_enough_messages() {
        let c = LlmCompressor::new(10000);
        // Need more than PROTECT_FIRST + PROTECT_LAST + 1 = 10
        let mut messages = make_messages(20);
        let original_len = messages.len();
        c.compress(&mut messages).await.unwrap();

        // Should have compressed: 20 messages -> 1 summary + 3 protected first
        // + 6 protected last = 10
        assert_eq!(messages.len(), PROTECT_FIRST + 1 + PROTECT_LAST);
        assert!(messages.len() < original_len);

        // The message at PROTECT_FIRST should be the summary.
        let summary = &messages[PROTECT_FIRST];
        assert_eq!(summary.role, MessageRole::System);
        assert!(
            summary
                .content
                .as_ref()
                .unwrap()
                .contains("context-compression")
        );
        assert!(
            summary
                .content
                .as_ref()
                .unwrap()
                .contains("Question tracking")
        );
    }

    // ── LlmCompressor: tool output pruning ──────────────────────────────

    #[test]
    fn test_tool_output_pruning() {
        let mut messages = vec![
            Message::user("Run some code"),
            make_large_tool_output_message("tc1", 1000),
            Message::user("Another request"),
            make_large_tool_output_message("tc2", 100),
            Message::user("Small output"),
            Message::tool_result("tc3", "grep", "short"),
        ];

        let pruned = LlmCompressor::prune_tool_outputs(&mut messages);
        // Only tc1 (1000 chars) and tc2 (100 chars) are tool messages;
        // tc1 > 500, tc2 < 500, tc3 < 500. So only tc1 should be pruned.
        assert_eq!(pruned, 1);
        assert!(messages[1].content.as_ref().unwrap().contains("pruned"));
        assert!(!messages[3].content.as_ref().unwrap().contains("pruned"));
    }

    #[test]
    fn test_tool_output_pruning_multiple_large() {
        let mut messages = vec![
            make_large_tool_output_message("a", 600),
            make_large_tool_output_message("b", 2000),
            make_large_tool_output_message("c", 499),
        ];

        let pruned = LlmCompressor::prune_tool_outputs(&mut messages);
        assert_eq!(pruned, 2); // a and b are pruned, c is not
    }

    #[test]
    fn test_tool_output_pruning_no_tool_messages() {
        let mut messages = vec![Message::user("Hello"), Message::assistant("Hi there")];
        let pruned = LlmCompressor::prune_tool_outputs(&mut messages);
        assert_eq!(pruned, 0);
    }

    // ── LlmCompressor: session lifecycle ────────────────────────────────

    #[test]
    fn test_llm_session_lifecycle() {
        let mut c = LlmCompressor::new(10000);

        // Simulate usage
        let usage = make_usage(5000, 3000);
        c.update_from_response(&usage);
        assert!(c.should_compress());

        // Session start resets everything
        c.on_session_start();
        assert!(!c.should_compress());
        assert_eq!(c.current_prompt_tokens, 0);
        assert_eq!(c.current_completion_tokens, 0);

        // After session start, should_compress is false again
        assert!(!c.should_compress());

        // End session
        c.on_session_end();
        assert!(!c.should_compress());
    }

    // ── LlmCompressor: context_length ───────────────────────────────────

    #[test]
    fn test_llm_context_length_various() {
        assert_eq!(LlmCompressor::new(2048).context_length(), 2048);
        assert_eq!(LlmCompressor::new(4096).context_length(), 4096);
        assert_eq!(LlmCompressor::new(128000).context_length(), 128000);
    }

    // ── LlmCompressor: question tracking ────────────────────────────────

    #[test]
    fn test_question_tracking_detects_questions() {
        let mut c = LlmCompressor::new(10000);
        let messages = vec![
            Message::user("What is Rust?"),
            Message::assistant("Rust is a systems programming language."),
            Message::user("How do I install it?"),
            Message::assistant("Use rustup."),
            Message::user("Tell me about the borrow checker"), // not a question
        ];

        c.update_questions(&messages);
        // "What is Rust?" and "How do I install it?" should be tracked.
        assert_eq!(c.questions.len(), 2);
        // Both should be resolved (assistant replied after each).
        assert!(c.questions[0].resolved);
        assert!(c.questions[1].resolved);
    }

    #[test]
    fn test_question_tracking_pending() {
        let mut c = LlmCompressor::new(10000);
        let messages = vec![
            Message::user("What is Rust?"),
            Message::assistant("Rust is a systems programming language."),
            Message::user("Can you explain lifetimes?"),
            // No assistant reply yet for this question
        ];

        c.update_questions(&messages);
        assert_eq!(c.questions.len(), 2);
        assert!(c.questions[0].resolved);
        assert!(!c.questions[1].resolved);
    }

    #[test]
    fn test_question_tracking_no_duplicates() {
        let mut c = LlmCompressor::new(10000);
        let messages = vec![
            Message::user("What is Rust?"),
            Message::assistant("It's a language."),
            Message::user("What is Rust?"),
            Message::assistant("I already told you."),
        ];

        c.update_questions(&messages);
        assert_eq!(c.questions.len(), 1);
    }

    // ── LlmCompressor: compression stats ────────────────────────────────

    #[test]
    fn test_compression_stats_initial() {
        let c = LlmCompressor::new(10000);
        let stats = c.compression_stats().unwrap();
        assert_eq!(stats.compression_count, 0);
        assert_eq!(stats.total_tokens_saved, 0);
    }

    // ── LlmCompressor: edge case — zero context length ──────────────────

    #[test]
    fn test_llm_compressor_zero_context_length() {
        let c = LlmCompressor::new(0);
        assert_eq!(c.context_length(), 0);
        assert_eq!(c.threshold_tokens(), 0);
        // Any usage should trigger compression.
        let mut c2 = LlmCompressor::new(0);
        c2.update_from_response(&make_usage(1, 0));
        assert!(c2.should_compress());
    }

    // ── LlmCompressor: full compress with pruning + question tracking ───

    #[tokio::test]
    async fn test_llm_compress_full_pipeline() {
        let c = LlmCompressor::new(10000);
        let mut messages = vec![
            // System messages (protected: indices 0-2)
            Message::system("You are a coding assistant."),
            Message::system("Be helpful."),
            Message::system("Use examples."),
            // Middle messages (compressible: indices 3..13)
            Message::user("What is a vector in Rust?"),
            Message::assistant("A vector is a growable array."),
            Message::user("How do I create one?"),
            Message::assistant("Use Vec::new() or vec![] macro."),
            Message::user("Run this code"),
            make_large_tool_output_message("tc1", 2000),
            Message::user("What about HashMap?"),
            Message::assistant("HashMap is a key-value store."),
            Message::user("Show me an example"),
            make_large_tool_output_message("tc2", 800),
            // Tail messages (protected: last 6)
            Message::assistant("Here is an example: ..."),
            Message::user("Thanks!"),
            Message::assistant("You're welcome!"),
            Message::user("One more thing?"),
            Message::assistant("Sure, what is it?"),
            Message::user("Never mind."),
        ];

        let original_len = messages.len();
        assert_eq!(original_len, 19);

        c.compress(&mut messages).await.unwrap();

        // After compression: 3 (protected first) + 1 (summary) + 6 (protected last) = 10
        assert_eq!(messages.len(), 10);

        // The summary message should contain context-compression and question tracking.
        let summary = &messages[3];
        assert_eq!(summary.role, MessageRole::System);
        let content = summary.content.as_ref().unwrap();
        assert!(content.contains("context-compression"));
        assert!(content.contains("Question tracking"));
        // Should have tracked questions about vectors, creating vectors, HashMap
        assert!(content.contains("vector") || content.contains("HashMap"));
    }

    // ── Estimate tokens helper ──────────────────────────────────────────

    #[test]
    fn test_estimate_tokens() {
        assert_eq!(LlmCompressor::estimate_tokens(""), 0);
        assert_eq!(LlmCompressor::estimate_tokens("abcd"), 1);
        assert_eq!(LlmCompressor::estimate_tokens("abcde"), 2); // 5 chars -> ceil(5/4) = 2
    }
}
