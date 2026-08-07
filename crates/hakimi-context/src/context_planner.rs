//! Context Planner — Inspired by Hermes Agent + grok-build
//!
//! The planner sits BEFORE message sending and AFTER compression triggers.
//! It manages:
//! - Token budget allocation (system/skills/memory/tools/history)
//! - Message pruning (tool results dedup, old tool output truncation)
//! - Dynamic prompt planning (on-demand memory/skill retrieval)
//!
//! **Design Goals:**
//! 1. Avoid "死板的系统提示词全量注入" — system prompt is pruned/chunked.
//! 2. Enforce compression BEFORE build_send_messages (not after).
//! 3. Tool results are deduplicated and old ones are pruned.
//! 4. Memory is retrieved on-demand (not full dump).
//!
//! **NOT in this version:** LLM-based dynamic retrieval. That will come after
//! the subagents return with Hermes/grok-build analysis.

use hakimi_common::{Message, MessageRole};
use std::collections::HashSet;
use tracing::{debug, info, warn};

/// Token budget configuration.
#[derive(Debug, Clone)]
pub struct BudgetConfig {
    /// Total context length (tokens).
    pub total_context_length: usize,
    /// Reserve tokens for the next response (20% default).
    pub reserve_for_response: usize,
    /// Maximum system prompt tokens (15% default).
    pub max_system_prompt_tokens: usize,
    /// Maximum skill context tokens (10% default).
    pub max_skill_tokens: usize,
    /// Minimum messages to keep (always preserve last N messages).
    pub keep_recent_messages: usize,
}

impl BudgetConfig {
    pub fn from_context_length(context_length: usize) -> Self {
        Self {
            total_context_length: context_length,
            reserve_for_response: (context_length as f64 * 0.20) as usize,
            max_system_prompt_tokens: (context_length as f64 * 0.15) as usize,
            max_skill_tokens: (context_length as f64 * 0.10) as usize,
            keep_recent_messages: 20,
        }
    }
}

/// The context planner decides what goes into the next API call.
#[derive(Debug, Clone)]
pub struct ContextPlanner {
    config: BudgetConfig,
}

impl ContextPlanner {
    pub fn new(config: BudgetConfig) -> Self {
        Self { config }
    }

    /// Plan the messages to send by pruning/truncating based on token budget.
    ///
    /// This method should be called AFTER compression but BEFORE build_send_messages.
    /// It does NOT add the system prompt — that's still done by build_send_messages.
    pub fn plan_messages(&mut self, messages: &[Message]) -> Vec<Message> {
        let total = messages.len();
        if total == 0 {
            return vec![];
        }

        // Phase 1: Deduplicate tool results
        let messages = self.deduplicate_tool_results(messages);

        // Phase 2: Prune old tool results (keep last keep_recent_messages intact)
        let messages = self.prune_old_tool_results(&messages);

        // Phase 3: Estimate tokens and truncate if needed
        let estimated_tokens = Self::estimate_messages_tokens(&messages);
        let available_budget = self
            .config
            .total_context_length
            .saturating_sub(self.config.reserve_for_response)
            .saturating_sub(self.config.max_system_prompt_tokens)
            .saturating_sub(self.config.max_skill_tokens);

        if estimated_tokens > available_budget {
            debug!(
                estimated_tokens,
                available_budget, "Context exceeds budget after planning — truncating old messages"
            );
            self.truncate_to_budget(&messages, available_budget)
        } else {
            messages
        }
    }

    /// Phase 1: Deduplicate tool results by MD5 hash (Hermes pattern).
    ///
    /// Important protocol constraint: never remove a tool result message if its
    /// assistant tool_call is still present. OpenAI/Anthropic-compatible APIs
    /// require every assistant tool_call to be followed by a matching tool
    /// response. Therefore duplicates are replaced with a compact placeholder
    /// instead of being deleted.
    fn deduplicate_tool_results(&mut self, messages: &[Message]) -> Vec<Message> {
        let mut seen_hashes = HashSet::new();
        let mut result = Vec::with_capacity(messages.len());
        let mut compacted = 0usize;

        for msg in messages {
            if msg.role == MessageRole::Tool {
                let content = msg.content.as_deref().unwrap_or("");
                let tool_name = msg.name.as_deref().unwrap_or("unknown");
                let hash_input = format!("{}{}", tool_name, content);
                let hash = format!("{:x}", md5::compute(&hash_input));

                if seen_hashes.contains(&hash) {
                    debug!(
                        tool = tool_name,
                        "Compacting duplicate tool result while preserving tool_call pairing"
                    );
                    let mut compacted_msg = msg.clone();
                    compacted_msg.content = Some(format!(
                        "[Duplicate {tool_name} tool result compacted — identical output appeared earlier in this context]"
                    ));
                    result.push(compacted_msg);
                    compacted += 1;
                    continue;
                }
                seen_hashes.insert(hash);
            }
            result.push(msg.clone());
        }

        if compacted > 0 {
            info!(compacted, "Compacted duplicate tool results");
        }

        result
    }

    /// Phase 2: Prune old tool results (keep recent ones intact).
    fn prune_old_tool_results(&self, messages: &[Message]) -> Vec<Message> {
        let total = messages.len();
        let keep_recent = self.config.keep_recent_messages;

        if total <= keep_recent {
            return messages.to_vec();
        }

        let prune_boundary = total - keep_recent;
        let mut result = Vec::with_capacity(messages.len());

        for (i, msg) in messages.iter().enumerate() {
            if i < prune_boundary && msg.role == MessageRole::Tool {
                // Prune old tool result
                let content = msg.content.as_deref().unwrap_or("");
                if content.len() > 200 {
                    let mut pruned = msg.clone();
                    pruned.content = Some(format!(
                        "[Tool result truncated — {} chars removed]",
                        content.len()
                    ));
                    result.push(pruned);
                } else {
                    result.push(msg.clone());
                }
            } else {
                result.push(msg.clone());
            }
        }

        result
    }

    /// Phase 3: Truncate to budget (keep last N messages).
    fn truncate_to_budget(&self, messages: &[Message], budget: usize) -> Vec<Message> {
        let mut used_tokens = 0;
        let mut keep_from = messages.len();

        // Walk backwards from the end
        for (i, msg) in messages.iter().enumerate().rev() {
            let msg_tokens = Self::estimate_message_tokens(msg);
            if used_tokens + msg_tokens > budget && i < messages.len() - 1 {
                break;
            }
            used_tokens += msg_tokens;
            keep_from = i;
        }

        if keep_from > 0 {
            warn!(
                dropped = keep_from,
                remaining = messages.len() - keep_from,
                "Truncated messages to fit budget"
            );
            // Insert a summary message at the front
            let summary = Message::system(format!(
                "[Context truncated: {} earlier messages dropped to fit token budget]",
                keep_from
            ));
            let tail: Vec<Message> = messages[keep_from..].to_vec();
            let mut result = vec![summary];
            result.extend(tail);
            result
        } else {
            messages.to_vec()
        }
    }

    /// Estimate tokens for a message (rough: 1 token ≈ 4 chars).
    fn estimate_message_tokens(msg: &Message) -> usize {
        let mut tokens = 10; // base cost for role/metadata

        if let Some(ref content) = msg.content {
            tokens += content.len().div_ceil(4);
        }

        if let Some(ref tool_calls) = msg.tool_calls {
            for tc in tool_calls {
                tokens += tc.name.len().div_ceil(4);
                tokens += tc.arguments.len().div_ceil(4);
            }
        }

        if let Some(ref images) = msg.images {
            tokens += images.len() * 1600; // flat image cost
        }

        tokens
    }

    /// Estimate total tokens for messages.
    fn estimate_messages_tokens(messages: &[Message]) -> usize {
        messages.iter().map(Self::estimate_message_tokens).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolCall;

    #[test]
    fn test_deduplicate_tool_results() {
        let mut planner = ContextPlanner::new(BudgetConfig::from_context_length(8000));

        let messages = vec![
            Message::user("Read the file"),
            Message::assistant("Reading...").with_tool_calls(vec![ToolCall {
                id: "call_1".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path":"test.txt"}"#.to_string(),
                index: Some(0),
            }]),
            Message {
                role: MessageRole::Tool,
                content: Some("File content here".to_string()),
                tool_call_id: Some("call_1".to_string()),
                name: Some("read_file".to_string()),
                images: None,
                tool_calls: None,
                reasoning: None,
                reasoning_content: None,
                timestamp: None,
                token_count: None,
                finish_reason: None,
            },
            Message::assistant("Reading again...").with_tool_calls(vec![ToolCall {
                id: "call_2".to_string(),
                name: "read_file".to_string(),
                arguments: r#"{"path":"test.txt"}"#.to_string(),
                index: Some(1),
            }]),
            Message {
                role: MessageRole::Tool,
                content: Some("File content here".to_string()), // Duplicate
                tool_call_id: Some("call_2".to_string()),
                name: Some("read_file".to_string()),
                images: None,
                tool_calls: None,
                reasoning: None,
                reasoning_content: None,
                timestamp: None,
                token_count: None,
                finish_reason: None,
            },
        ];

        let planned = planner.plan_messages(&messages);
        // Duplicate tool result must be compacted in-place, not removed: provider
        // protocols require assistant tool_calls to have matching tool results.
        assert_eq!(planned.len(), 5, "Duplicate tool result should be kept");
        let duplicate_result = planned
            .iter()
            .find(|m| m.tool_call_id.as_deref() == Some("call_2"))
            .expect("call_2 tool result should be preserved");
        assert!(
            duplicate_result
                .content
                .as_deref()
                .unwrap_or_default()
                .contains("Duplicate read_file tool result compacted"),
            "duplicate tool result content should be compacted"
        );
    }

    #[test]
    fn test_prune_old_tool_results() {
        let mut planner = ContextPlanner::new(BudgetConfig {
            total_context_length: 8000,
            reserve_for_response: 1600,
            max_system_prompt_tokens: 1200,
            max_skill_tokens: 800,
            keep_recent_messages: 3,
        });

        let long_content = "x".repeat(500);
        let messages = vec![
            Message::user("First"),
            Message {
                role: MessageRole::Tool,
                content: Some(long_content.clone()),
                tool_call_id: Some("call_old".to_string()),
                name: Some("tool".to_string()),
                images: None,
                tool_calls: None,
                reasoning: None,
                reasoning_content: None,
                timestamp: None,
                token_count: None,
                finish_reason: None,
            },
            Message::user("Second"),
            Message::user("Third"),
            Message::user("Fourth"),
        ];

        let planned = planner.plan_messages(&messages);
        // First tool result should be pruned, last 3 messages kept intact
        assert_eq!(planned.len(), 5);
        assert!(
            planned[1].content.as_ref().unwrap().contains("truncated"),
            "Old tool result should be truncated"
        );
        assert_eq!(planned[2].content, Some("Second".to_string()));
    }

    #[test]
    fn test_token_estimation() {
        let msg = Message::user("Hello world"); // ~3 tokens + 10 base = 13
        assert!(ContextPlanner::estimate_message_tokens(&msg) < 20);

        let long_msg = Message::user("x".repeat(1000)); // ~250 tokens + 10 base = 260
        assert!(ContextPlanner::estimate_message_tokens(&long_msg) > 250);
    }
}
