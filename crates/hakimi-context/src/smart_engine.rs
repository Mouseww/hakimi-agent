use async_trait::async_trait;
use hakimi_common::{Message, MessageRole, Result, Usage};
use std::sync::Mutex;
use tracing::{debug, info};

use crate::engine::{CompressionStats, ContextEngine};

/// Statistics returned after a compression pass.
#[derive(Debug, Clone)]
pub struct CompressionSummary {
    /// Number of messages before compression.
    pub original_message_count: usize,
    /// Number of messages after compression.
    pub compressed_message_count: usize,
    /// Estimated tokens saved by this compression pass.
    pub tokens_saved: usize,
    /// Ratio of compressed to original message count (lower = more aggressive).
    pub compression_ratio: f64,
    /// Which tier was applied.
    pub tier_applied: CompressionTier,
}

/// Which compression tier was applied.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompressionTier {
    /// No compression was needed.
    None,
    /// Tier 1: Dropped old tool results.
    DropToolResults,
    /// Tier 2: Summarized old conversation turns.
    SummarizeOldTurns,
    /// Tier 3: Sliding window — keep system prompt + last N messages.
    SlidingWindow,
}

/// A smart context engine with three tiers of compression:
///
/// 1. **Tier 1 — Drop old tool results**: Remove content from tool-result
///    messages in the oldest turns while keeping the assistant tool_calls.
/// 2. **Tier 2 — Summarize old turns**: Replace multiple old conversation
///    turns with a single summary message.
/// 3. **Tier 3 — Sliding window**: Keep the system prompt plus the last N
///    messages, drop everything else.
///
/// Compression is triggered when estimated tokens exceed 70% of the
/// configured context length.
pub struct SmartContextEngine {
    /// Maximum context length in tokens.
    context_length: usize,
    /// Estimated tokens currently in use.
    estimated_tokens: usize,
    /// Optional model name for future summarization calls.
    _compression_model: Option<String>,
    /// Cumulative compression statistics (protected by Mutex for interior mutability
    /// since `compress` takes `&self`).
    stats: Mutex<CompressionStats>,
}

impl SmartContextEngine {
    /// Create a new smart context engine.
    ///
    /// - `context_length`: maximum context window in tokens.
    /// - `compression_model`: optional model name to use for summarization
    ///   (reserved for future use; currently summaries are generated locally).
    pub fn new(context_length: usize, compression_model: Option<String>) -> Self {
        Self {
            context_length,
            estimated_tokens: 0,
            _compression_model: compression_model,
            stats: Mutex::new(CompressionStats {
                compression_count: 0,
                total_tokens_saved: 0,
            }),
        }
    }

    /// Rough token estimate: 1 token ≈ 4 characters.
    fn estimate_tokens(text: &str) -> usize {
        (text.len() + 3) / 4
    }

    /// Estimate the total tokens across all messages.
    fn estimate_total_tokens(messages: &[Message]) -> usize {
        messages
            .iter()
            .map(|m| {
                let content_tokens = m
                    .content
                    .as_deref()
                    .map(Self::estimate_tokens)
                    .unwrap_or(0);
                let reasoning_tokens = m
                    .reasoning
                    .as_deref()
                    .map(Self::estimate_tokens)
                    .unwrap_or(0)
                    + m.reasoning_content
                        .as_deref()
                        .map(Self::estimate_tokens)
                        .unwrap_or(0);
                let tool_tokens = m
                    .tool_calls
                    .as_ref()
                    .map(|tc| {
                        // Rough estimate for tool call JSON
                        let json = serde_json::to_string(tc).unwrap_or_default();
                        Self::estimate_tokens(&json)
                    })
                    .unwrap_or(0);
                content_tokens + reasoning_tokens + tool_tokens + 4 // per-message overhead
            })
            .sum()
    }

    /// Tier 1: Drop content from old tool-result messages.
    ///
    /// Walk backwards from the end, keeping recent tool results intact.
    /// For older tool results, replace their content with a placeholder.
    fn tier1_drop_tool_results(messages: &mut Vec<Message>, keep_recent: usize) -> (usize, usize) {
        let total = messages.len();
        if total <= keep_recent {
            return (total, 0);
        }

        // The "old" region is [0, total - keep_recent)
        let old_end = total - keep_recent;
        let before_tokens = Self::estimate_total_tokens(messages);

        for msg in &mut messages[..old_end] {
            if msg.role == MessageRole::Tool && msg.content.is_some() {
                let orig_len = msg.content.as_ref().map(|c| c.len()).unwrap_or(0);
                if orig_len > 100 {
                    msg.content = Some(format!(
                        "[Tool result truncated — {} chars removed]",
                        orig_len
                    ));
                }
            }
        }

        let after_tokens = Self::estimate_total_tokens(messages);
        let tokens_saved = before_tokens.saturating_sub(after_tokens);
        (total, tokens_saved)
    }

    /// Tier 2: Summarize old conversation turns.
    ///
    /// Keep system messages and the last `keep_recent` messages.
    /// Replace everything in between with a summary message.
    fn tier2_summarize_old_turns(
        messages: &mut Vec<Message>,
        keep_recent: usize,
    ) -> (usize, usize) {
        let total = messages.len();
        if total <= keep_recent + 1 {
            return (total, 0);
        }

        let before_tokens = Self::estimate_total_tokens(messages);

        // Find the index after the last contiguous system message at the front.
        let mut system_end = 0;
        for msg in messages.iter() {
            if msg.role == MessageRole::System {
                system_end += 1;
            } else {
                break;
            }
        }

        // If there's nothing meaningful between system messages and the tail, skip.
        let tail_start = total - keep_recent;
        if system_end >= tail_start {
            return (total, 0);
        }

        // Build a summary of the messages being compressed.
        let compress_start = system_end;
        let compress_end = tail_start;
        let count = compress_end - compress_start;

        let mut summary_parts: Vec<String> = Vec::new();
        for msg in &messages[compress_start..compress_end] {
            let preview = msg
                .content
                .as_deref()
                .map(|c| {
                    if c.len() > 150 {
                        format!("{}…", &c[..150])
                    } else {
                        c.to_string()
                    }
                })
                .unwrap_or_else(|| "(no content)".to_string());
            summary_parts.push(format!("[{}] {}", msg.role, preview));
        }

        let summary_text = format!(
            "<context-compression tier=\"2\">\n\
             {} earlier conversation turns were summarized:\n\n\
             {}\n\
             </context-compression>",
            count,
            summary_parts.join("\n")
        );

        let summary_msg = Message {
            role: MessageRole::System,
            content: Some(summary_text),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: Some(chrono::Utc::now()),
            token_count: None,
            finish_reason: None,
        };

        // Drain the middle and insert summary.
        messages.drain(compress_start..compress_end);
        messages.insert(compress_start, summary_msg);

        let after_tokens = Self::estimate_total_tokens(messages);
        let tokens_saved = before_tokens.saturating_sub(after_tokens);
        (messages.len(), tokens_saved)
    }

    /// Tier 3: Sliding window — keep system prompt + last N messages.
    fn tier3_sliding_window(messages: &mut Vec<Message>, keep_recent: usize) -> (usize, usize) {
        let total = messages.len();
        if total <= keep_recent + 1 {
            return (total, 0);
        }

        let before_tokens = Self::estimate_total_tokens(messages);

        // Preserve leading system messages.
        let mut system_end = 0;
        for msg in messages.iter() {
            if msg.role == MessageRole::System {
                system_end += 1;
            } else {
                break;
            }
        }

        let tail_start = total - keep_recent;
        if system_end >= tail_start {
            return (total, 0);
        }

        // Keep system prefix + tail, replace everything else with a notice.
        let dropped_count = tail_start - system_end;
        let notice = Message {
            role: MessageRole::System,
            content: Some(format!(
                "<context-compression tier=\"3\">\n\
                 {} messages were dropped via sliding window. \
                 Only the system prompt and the last {} messages are retained.\n\
                 </context-compression>",
                dropped_count, keep_recent
            )),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: Some(chrono::Utc::now()),
            token_count: None,
            finish_reason: None,
        };

        // Build the new message list: system prefix + notice + tail.
        let mut new_messages: Vec<Message> = Vec::with_capacity(system_end + 1 + keep_recent);
        new_messages.extend(messages[..system_end].iter().cloned());
        new_messages.push(notice);
        new_messages.extend(messages[tail_start..].iter().cloned());

        let after_tokens = Self::estimate_total_tokens(&new_messages);
        let tokens_saved = before_tokens.saturating_sub(after_tokens);

        *messages = new_messages;
        (messages.len(), tokens_saved)
    }

    /// Determine the compression tier to apply based on how far over budget we are.
    fn choose_tier(estimated_tokens: usize, context_length: usize) -> CompressionTier {
        let ratio = estimated_tokens as f64 / context_length as f64;
        if ratio <= 0.70 {
            CompressionTier::None
        } else if ratio <= 0.85 {
            CompressionTier::DropToolResults
        } else if ratio <= 0.95 {
            CompressionTier::SummarizeOldTurns
        } else {
            CompressionTier::SlidingWindow
        }
    }
}

#[async_trait]
impl ContextEngine for SmartContextEngine {
    fn name(&self) -> &str {
        "smart"
    }

    fn update_from_response(&mut self, usage: &Usage) {
        self.estimated_tokens = usage.total_tokens as usize;
        debug!(
            estimated = self.estimated_tokens,
            context_length = self.context_length,
            "SmartContextEngine: token usage updated"
        );
    }

    fn should_compress(&self) -> bool {
        let threshold = (self.context_length as f64 * 0.70) as usize;
        self.estimated_tokens > threshold
    }

    async fn compress(&self, messages: &mut Vec<Message>) -> Result<()> {
        if messages.is_empty() {
            return Ok(());
        }

        let before_total = messages.len();
        let before_tokens = Self::estimate_total_tokens(messages);

        let tier = Self::choose_tier(self.estimated_tokens, self.context_length);

        match tier {
            CompressionTier::None => {
                debug!("No compression needed");
                return Ok(());
            }
            CompressionTier::DropToolResults => {
                info!("Applying Tier 1: drop old tool results");
                // Keep the last 60% of messages with full tool results.
                let keep_recent = (messages.len() as f64 * 0.60).ceil() as usize;
                let keep_recent = keep_recent.max(4);
                Self::tier1_drop_tool_results(messages, keep_recent);
            }
            CompressionTier::SummarizeOldTurns => {
                info!("Applying Tier 2: summarize old turns");
                let keep_recent = (messages.len() as f64 * 0.40).ceil() as usize;
                let keep_recent = keep_recent.max(4);
                Self::tier2_summarize_old_turns(messages, keep_recent);
            }
            CompressionTier::SlidingWindow => {
                info!("Applying Tier 3: sliding window");
                // Keep the last 10 messages + system prompt.
                let keep_recent = 10;
                Self::tier3_sliding_window(messages, keep_recent);
            }
        }

        let after_total = messages.len();
        let after_tokens = Self::estimate_total_tokens(messages);
        let tokens_saved = before_tokens.saturating_sub(after_tokens);

        // Update cumulative stats.
        if let Ok(mut stats) = self.stats.lock() {
            stats.compression_count += 1;
            stats.total_tokens_saved += tokens_saved;
        }

        let compression_ratio = if before_total > 0 {
            after_total as f64 / before_total as f64
        } else {
            1.0
        };

        info!(
            tier = ?tier,
            before_messages = before_total,
            after_messages = after_total,
            tokens_saved = tokens_saved,
            compression_ratio = compression_ratio,
            "Context compression complete"
        );

        Ok(())
    }

    fn on_session_start(&mut self) {
        self.estimated_tokens = 0;
        if let Ok(mut stats) = self.stats.lock() {
            stats.compression_count = 0;
            stats.total_tokens_saved = 0;
        }
        debug!("SmartContextEngine: session started, counters reset");
    }

    fn on_session_end(&mut self) {
        if let Ok(stats) = self.stats.lock() {
            info!(
                compression_count = stats.compression_count,
                total_tokens_saved = stats.total_tokens_saved,
                "SmartContextEngine: session ended"
            );
        }
    }

    fn context_length(&self) -> usize {
        self.context_length
    }

    fn compression_stats(&self) -> Option<CompressionStats> {
        self.stats.lock().ok().map(|s| s.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolCall;

    fn make_engine(context_length: usize) -> SmartContextEngine {
        SmartContextEngine::new(context_length, None)
    }

    fn make_assistant_with_tool_call(tool_call_id: &str) -> Message {
        Message {
            role: MessageRole::Assistant,
            content: Some("Let me check that for you.".to_string()),
            tool_calls: Some(vec![ToolCall {
                id: tool_call_id.to_string(),
                name: "search".to_string(),
                arguments: "{}".to_string(),
                index: None,
            }]),
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: None,
            token_count: None,
            finish_reason: None,
        }
    }

    fn make_tool_result(tool_call_id: &str, large: bool) -> Message {
        let content = if large {
            "x".repeat(5000)
        } else {
            "short result".to_string()
        };
        Message::tool_result(tool_call_id, "search", content)
    }

    // ── Tier 1: Drop old tool results ──────────────────────────────────

    #[test]
    fn test_tier1_drops_old_tool_results() {
        let mut messages = vec![
            Message::system("You are helpful."),
            Message::user("Search for X"),
            make_assistant_with_tool_call("tc1"),
            make_tool_result("tc1", true),
            Message::user("Now search for Y"),
            make_assistant_with_tool_call("tc2"),
            make_tool_result("tc2", true),
            Message::assistant("Here are the results."),
        ];

        // Apply tier 1, keeping only the last 4 messages with full tool results.
        let (_, tokens_saved) =
            SmartContextEngine::tier1_drop_tool_results(&mut messages, 4);

        // The first tool result (index 3) should have been truncated.
        assert!(
            messages[3].content.as_ref().unwrap().contains("truncated"),
            "Old tool result should be truncated, got: {:?}",
            messages[3].content
        );
        // The second tool result (index 6) should remain intact (within keep_recent).
        assert!(
            !messages[6].content.as_ref().unwrap().contains("truncated"),
            "Recent tool result should NOT be truncated"
        );
        assert!(tokens_saved > 0, "Should have saved tokens");
    }

    // ── Tier 2: Summarize old turns ────────────────────────────────────

    #[test]
    fn test_tier2_summarizes_old_turns() {
        let mut messages = vec![
            Message::system("You are helpful."),
            Message::user("Hello"),
            Message::assistant("Hi there!"),
            Message::user("What's 2+2?"),
            Message::assistant("4"),
            Message::user("What's 3+3?"),
            Message::assistant("6"),
            Message::user("Thanks"),
        ];

        let (_, _tokens_saved) =
            SmartContextEngine::tier2_summarize_old_turns(&mut messages, 2);

        // Should have system + summary + last 2 messages = 4
        assert_eq!(messages.len(), 4, "Expected 4 messages after tier 2, got {}", messages.len());
        // First message is the original system message.
        assert_eq!(messages[0].role, MessageRole::System);
        assert_eq!(
            messages[0].content.as_deref(),
            Some("You are helpful.")
        );
        // Second message is the compression summary.
        assert!(
            messages[1]
                .content
                .as_ref()
                .unwrap()
                .contains("context-compression"),
            "Summary message should contain context-compression tag"
        );
        assert!(
            messages[1]
                .content
                .as_ref()
                .unwrap()
                .contains("earlier conversation turns"),
            "Summary should mention summarized turns"
        );
    }

    // ── Tier 3: Sliding window ─────────────────────────────────────────

    #[test]
    fn test_tier3_sliding_window() {
        let mut messages = vec![
            Message::system("System prompt"),
            Message::user("msg 1"),
            Message::assistant("reply 1"),
            Message::user("msg 2"),
            Message::assistant("reply 2"),
            Message::user("msg 3"),
            Message::assistant("reply 3"),
            Message::user("msg 4"),
            Message::assistant("reply 4"),
            Message::user("msg 5"),
            Message::assistant("reply 5"),
            Message::user("msg 6"),
            Message::assistant("reply 6"),
        ];

        let (_, _) = SmartContextEngine::tier3_sliding_window(&mut messages, 4);

        // Should have: system prompt + notice + last 4 messages = 6
        assert_eq!(
            messages.len(),
            6,
            "Expected 6 messages after tier 3, got {}",
            messages.len()
        );
        // First is the original system prompt.
        assert_eq!(messages[0].content.as_deref(), Some("System prompt"));
        // Second is the sliding window notice.
        assert!(
            messages[1]
                .content
                .as_ref()
                .unwrap()
                .contains("sliding window"),
            "Notice should mention sliding window"
        );
        // Last 4 messages should be preserved.
        assert_eq!(messages[2].content.as_deref(), Some("msg 5"));
        assert_eq!(messages[3].content.as_deref(), Some("reply 5"));
        assert_eq!(messages[4].content.as_deref(), Some("msg 6"));
        assert_eq!(messages[5].content.as_deref(), Some("reply 6"));
    }

    // ── Compression triggers at 70% threshold ──────────────────────────

    #[test]
    fn test_should_compress_at_70_percent() {
        let mut engine = make_engine(1000);

        engine.estimated_tokens = 699;
        assert!(!engine.should_compress(), "69.9% should not trigger");

        engine.estimated_tokens = 700;
        assert!(!engine.should_compress(), "70.0% should not trigger (not > 70%)");

        engine.estimated_tokens = 701;
        assert!(engine.should_compress(), "70.1% should trigger");
    }

    #[test]
    fn test_tier_selection() {
        assert_eq!(
            SmartContextEngine::choose_tier(600, 1000),
            CompressionTier::None
        );
        assert_eq!(
            SmartContextEngine::choose_tier(700, 1000),
            CompressionTier::None
        );
        assert_eq!(
            SmartContextEngine::choose_tier(750, 1000),
            CompressionTier::DropToolResults
        );
        assert_eq!(
            SmartContextEngine::choose_tier(900, 1000),
            CompressionTier::SummarizeOldTurns
        );
        assert_eq!(
            SmartContextEngine::choose_tier(960, 1000),
            CompressionTier::SlidingWindow
        );
    }

    // ── System message is always preserved ──────────────────────────────

    #[tokio::test]
    async fn test_system_message_always_preserved() {
        let mut engine = make_engine(100);
        // Simulate high token usage so tier 3 kicks in.
        engine.estimated_tokens = 960;

        let mut messages = vec![
            Message::system("You are a helpful assistant with a very specific system prompt."),
            Message::user("Hello"),
            Message::assistant("Hi!"),
            Message::user("Tell me about Rust"),
            Message::assistant("Rust is a systems programming language."),
            Message::user("More details"),
            Message::assistant("It focuses on safety and performance."),
            Message::user("Thanks"),
            Message::assistant("You're welcome!"),
            Message::user("Bye"),
            Message::assistant("Goodbye!"),
        ];

        engine.compress(&mut messages).await.unwrap();

        // The first real system message should still be present.
        assert_eq!(
            messages[0].role,
            MessageRole::System,
            "First message should remain a system message"
        );
        assert!(
            messages[0]
                .content
                .as_ref()
                .unwrap()
                .contains("helpful assistant"),
            "Original system prompt should be preserved"
        );
    }

    // ── Integration: full compression pipeline ─────────────────────────

    #[tokio::test]
    async fn test_full_compression_pipeline() {
        let mut engine = make_engine(200);
        // Simulate very high usage.
        engine.estimated_tokens = 195;

        let mut messages: Vec<Message> = Vec::new();
        messages.push(Message::system("You are a coding assistant."));
        for i in 0..20 {
            messages.push(Message::user(format!("Question {i} about programming")));
            messages.push(Message::assistant(format!(
                "Answer to question {i}: some detailed explanation that goes on for a while."
            )));
        }

        engine.compress(&mut messages).await.unwrap();

        // Should have fewer messages than before.
        assert!(
            messages.len() < 41,
            "Should have compressed, got {} messages",
            messages.len()
        );
        // System prompt preserved.
        assert_eq!(messages[0].role, MessageRole::System);

        // Stats should be updated.
        let stats = engine.compression_stats().unwrap();
        assert_eq!(stats.compression_count, 1);
        assert!(stats.total_tokens_saved > 0);
    }

    // ── CompressionStats default ───────────────────────────────────────

    #[test]
    fn test_compression_stats_returns_some() {
        let engine = make_engine(1000);
        let stats = engine.compression_stats();
        assert!(stats.is_some(), "SmartContextEngine should return Some(stats)");
        let s = stats.unwrap();
        assert_eq!(s.compression_count, 0);
        assert_eq!(s.total_tokens_saved, 0);
    }

    // ── Token estimation ───────────────────────────────────────────────

    #[test]
    fn test_estimate_tokens() {
        // 4 chars = 1 token
        assert_eq!(SmartContextEngine::estimate_tokens("hello"), 2); // 5 chars -> ceil(5/4) = 2
        assert_eq!(SmartContextEngine::estimate_tokens("test"), 1);  // 4 chars -> 1
        assert_eq!(SmartContextEngine::estimate_tokens(""), 0);
        assert_eq!(SmartContextEngine::estimate_tokens("a"), 1);      // 1 char -> ceil(1/4) = 1
    }

    #[test]
    fn test_estimate_total_tokens() {
        let messages = vec![
            Message::system("test"),  // 4 chars content + 4 overhead = 5
            Message::user("hello"),   // 5 chars content + 4 overhead = 6
        ];
        let total = SmartContextEngine::estimate_total_tokens(&messages);
        assert!(total > 0);
    }

    // ── Edge cases ─────────────────────────────────────────────────────

    #[test]
    fn test_tier1_no_tool_results() {
        let mut messages = vec![
            Message::system("sys"),
            Message::user("hello"),
            Message::assistant("hi"),
        ];
        let (_, saved) = SmartContextEngine::tier1_drop_tool_results(&mut messages, 2);
        assert_eq!(saved, 0, "No tool results means no savings");
    }

    #[test]
    fn test_tier2_too_few_messages() {
        let mut messages = vec![
            Message::system("sys"),
            Message::user("hello"),
            Message::assistant("hi"),
        ];
        let (count, saved) = SmartContextEngine::tier2_summarize_old_turns(&mut messages, 10);
        assert_eq!(count, 3, "Should not compress if too few messages");
        assert_eq!(saved, 0);
    }

    #[test]
    fn test_tier3_too_few_messages() {
        let mut messages = vec![
            Message::system("sys"),
            Message::user("hello"),
        ];
        let (count, saved) = SmartContextEngine::tier3_sliding_window(&mut messages, 10);
        assert_eq!(count, 2, "Should not compress if too few messages");
        assert_eq!(saved, 0);
    }
}
