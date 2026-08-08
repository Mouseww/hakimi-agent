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

/// A conversation turn (user message + assistant response + tool results).
#[derive(Debug, Clone)]
struct ConversationTurn {
    start_idx: usize,
    end_idx: usize,
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

    /// Phase 3: Truncate to budget using turn-aware strategy.
    ///
    /// Strategy inspired by grok-build:
    /// 1. Separate system messages from conversation history
    /// 2. Identify complete turns (User + Assistant + Tools)
    /// 3. Keep newest complete turns that fit budget
    /// 4. Always preserve system messages
    fn truncate_to_budget(&self, messages: &[Message], budget: usize) -> Vec<Message> {
        if messages.is_empty() {
            return vec![];
        }

        let total_tokens = Self::estimate_messages_tokens(messages);
        if total_tokens <= budget {
            return messages.to_vec();
        }

        // Separate system messages from conversation history
        let (system_msgs, history): (Vec<_>, Vec<_>) = messages
            .iter()
            .cloned()
            .partition(|m| m.role == MessageRole::System);

        if history.is_empty() {
            // Only system messages: keep as many as fit budget
            let system_refs: Vec<&Message> = system_msgs.iter().collect();
            return Self::truncate_system_messages(&system_refs, budget);
        }

        // Calculate budget after reserving space for system messages
        let system_tokens: usize = system_msgs.iter().map(Self::estimate_message_tokens).sum();
        let history_budget = budget.saturating_sub(system_tokens);

        // Identify conversation turns
        let turns = Self::identify_turns(&history);

        if turns.is_empty() {
            // No valid turns found: fall back to simple truncation
            return Self::simple_truncate(messages, budget);
        }

        // Select turns from newest to oldest that fit budget
        let selected_indices = Self::select_turns_for_budget(&history, &turns, history_budget);

        // Reconstruct message list: system + selected history
        let mut result: Vec<Message> = system_msgs;

        for &idx in &selected_indices {
            result.push(history[idx].clone());
        }

        // Insert truncation notice if we dropped messages
        if result.len() < messages.len() {
            let dropped_count = messages.len() - result.len();
            warn!(
                dropped = dropped_count,
                kept = result.len(),
                "Dropped {} message(s) to fit budget",
                dropped_count
            );

            // Insert summary after system messages
            let system_count = result
                .iter()
                .filter(|m| m.role == MessageRole::System)
                .count();
            let summary = Message::system(format!(
                "[Context truncated: {} earlier message(s) dropped to fit token budget]",
                dropped_count
            ));
            result.insert(system_count, summary);
        }

        result
    }

    /// Identify conversation turns in history.
    ///
    /// A turn starts with User, followed by Assistant(s), followed by Tool result(s),
    /// and may end with additional Assistant message(s).
    fn identify_turns(history: &[Message]) -> Vec<ConversationTurn> {
        let mut turns = Vec::new();
        let mut i = 0;

        while i < history.len() {
            if history[i].role == MessageRole::User {
                let turn_start = i;
                i += 1;

                // Scan for Assistant messages
                while i < history.len() && history[i].role == MessageRole::Assistant {
                    i += 1;
                }

                // Scan for Tool results
                while i < history.len() && history[i].role == MessageRole::Tool {
                    i += 1;
                }

                // Scan for additional Assistant messages after tools (final responses)
                while i < history.len() && history[i].role == MessageRole::Assistant {
                    i += 1;
                }

                let turn_end = i.saturating_sub(1);
                turns.push(ConversationTurn {
                    start_idx: turn_start,
                    end_idx: turn_end,
                });
            } else {
                // Skip orphan messages outside of turns
                i += 1;
            }
        }

        turns
    }

    /// Select message indices for turns that fit within budget (newest first).
    fn select_turns_for_budget(
        history: &[Message],
        turns: &[ConversationTurn],
        budget: usize,
    ) -> Vec<usize> {
        let mut selected = Vec::new();
        let mut used_tokens = 0;

        // Iterate turns from newest to oldest
        for turn in turns.iter().rev() {
            let turn_tokens: usize = history[turn.start_idx..=turn.end_idx]
                .iter()
                .map(Self::estimate_message_tokens)
                .sum();

            if used_tokens + turn_tokens <= budget {
                // Whole turn fits
                for idx in turn.start_idx..=turn.end_idx {
                    selected.push(idx);
                }
                used_tokens += turn_tokens;
            } else {
                // Turn doesn't fit, stop here
                break;
            }
        }

        // Sort selected indices to maintain original message order
        selected.sort_unstable();
        selected
    }

    /// Truncate system messages to fit budget.
    fn truncate_system_messages(system_msgs: &[&Message], budget: usize) -> Vec<Message> {
        let mut result = Vec::new();
        let mut used_tokens = 0;

        for msg in system_msgs {
            let msg_tokens = Self::estimate_message_tokens(msg);
            if used_tokens + msg_tokens > budget && !result.is_empty() {
                break;
            }
            result.push((*msg).clone());
            used_tokens += msg_tokens;
        }

        result
    }

    /// Simple truncation fallback (keep newest messages).
    fn simple_truncate(messages: &[Message], budget: usize) -> Vec<Message> {
        let mut used_tokens = 0;
        let mut keep_from = messages.len();

        for (i, msg) in messages.iter().enumerate().rev() {
            let msg_tokens = Self::estimate_message_tokens(msg);
            if used_tokens + msg_tokens > budget && i < messages.len() - 1 {
                break;
            }
            used_tokens += msg_tokens;
            keep_from = i;
        }

        if keep_from > 0 {
            let summary = Message::system(format!(
                "[Context truncated: {} earlier messages dropped]",
                keep_from
            ));
            let mut result = vec![summary];
            result.extend_from_slice(&messages[keep_from..]);
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

    #[test]
    fn test_turn_aware_budget_fitting() {
        let planner = ContextPlanner::new(BudgetConfig::from_context_length(8000));

        // Create a conversation with 3 complete turns
        let messages = vec![
            Message::system("You are a helpful assistant"),
            // Turn 1
            Message::user("Calculate 1+1"),
            Message::assistant("").with_tool_calls(vec![ToolCall {
                id: "call_1".to_string(),
                name: "calculator".to_string(),
                arguments: r#"{"expr":"1+1"}"#.to_string(),
                index: Some(0),
            }]),
            Message::tool_result("call_1".to_string(), "calculator".to_string(), "2"),
            Message::assistant("The answer is 2"),
            // Turn 2
            Message::user("Calculate 2+2"),
            Message::assistant("").with_tool_calls(vec![ToolCall {
                id: "call_2".to_string(),
                name: "calculator".to_string(),
                arguments: r#"{"expr":"2+2"}"#.to_string(),
                index: Some(0),
            }]),
            Message::tool_result("call_2".to_string(), "calculator".to_string(), "4"),
            Message::assistant("The answer is 4"),
            // Turn 3
            Message::user("Calculate 3+3"),
            Message::assistant("").with_tool_calls(vec![ToolCall {
                id: "call_3".to_string(),
                name: "calculator".to_string(),
                arguments: r#"{"expr":"3+3"}"#.to_string(),
                index: Some(0),
            }]),
            Message::tool_result("call_3".to_string(), "calculator".to_string(), "6"),
            Message::assistant("The answer is 6"),
        ];

        // Set a small budget that fits only the newest turn + system
        // Each message ~15-30 tokens, 3 turns × 5 messages ≈ 225-450 tokens
        // Set budget to ~100 tokens to force dropping old turns
        let small_budget = 100;
        let trimmed = planner.truncate_to_budget(&messages, small_budget);

        // Should keep: system + truncation notice + turn 3 (newest)
        assert!(trimmed.len() < messages.len());

        // System message should be preserved
        assert!(trimmed.iter().any(|m| {
            m.role == MessageRole::System
                && m.content
                    .as_ref()
                    .is_some_and(|c| c.contains("helpful assistant"))
        }));

        // Newest turn should be preserved
        assert!(trimmed.iter().any(|m| m.role == MessageRole::User
            && m.content.as_ref().is_some_and(|c| c.contains("3+3"))));

        // Older turns should be dropped
        assert!(!trimmed.iter().any(|m| m.role == MessageRole::User
            && m.content.as_ref().map_or(false, |c| c.contains("1+1"))));
    }

    #[test]
    fn test_identify_turns() {
        let history = vec![
            Message::user("Question 1"),
            Message::assistant("Answer 1"),
            Message::user("Question 2"),
            Message::assistant("").with_tool_calls(vec![ToolCall {
                id: "call_1".to_string(),
                name: "tool".to_string(),
                arguments: "{}".to_string(),
                index: Some(0),
            }]),
            Message::tool_result("call_1".to_string(), "tool".to_string(), "result"),
            Message::assistant("Answer 2"),
            Message::user("Question 3"),
            Message::assistant("Answer 3"),
        ];

        let turns = ContextPlanner::identify_turns(&history);

        // Should identify 3 turns
        assert_eq!(turns.len(), 3);

        // Turn 1: indices 0-1
        assert_eq!(turns[0].start_idx, 0);
        assert_eq!(turns[0].end_idx, 1);

        // Turn 2: indices 2-5 (includes tool call and result)
        assert_eq!(turns[1].start_idx, 2);
        assert_eq!(turns[1].end_idx, 5);

        // Turn 3: indices 6-7
        assert_eq!(turns[2].start_idx, 6);
        assert_eq!(turns[2].end_idx, 7);
    }
}
