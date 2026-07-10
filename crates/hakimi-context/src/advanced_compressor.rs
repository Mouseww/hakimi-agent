//! Advanced Context Compressor — Inspired by Hermes Agent
//!
//! Three-phase compression strategy:
//! 1. **Tool Output Pruning** (zero-LLM pass) — cheap pre-pass
//! 2. **Boundary Protection** (head + tail) — preserve critical context
//! 3. **LLM Structured Summary** — high-quality summarization
//!
//! Key features:
//! - Iterative summary updates (multi-compression sessions)
//! - Tool call/result pair integrity preservation
//! - Sensitive information redaction
//! - Anti-thrashing protection (skip ineffective compression)
//! - Progressive compression (40%/60%/80% thresholds)

use async_trait::async_trait;
use hakimi_common::{Message, MessageRole, Result, Usage};
use hakimi_transports::{ProviderTransport, RequestParams};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::engine::{CompressionStats, ContextEngine};

// ---------------------------------------------------------------------------
// Constants (aligned with Hermes Agent)
// ---------------------------------------------------------------------------

/// Minimum context length before compression is considered
const MINIMUM_CONTEXT_LENGTH: usize = 16_000;

/// Compression threshold as fraction of context window (default 50%)
const COMPRESSION_THRESHOLD: f64 = 0.50;

/// Target token budget after compression (as fraction of threshold)
const SUMMARY_TARGET_RATIO: f64 = 0.20;

/// Minimum tokens for summary output
const MIN_SUMMARY_TOKENS: usize = 2000;

/// Summary token ceiling (even on very large contexts)
const SUMMARY_TOKENS_CEILING: usize = 12_000;

/// Ratio of compressed content to allocate for summary
const SUMMARY_RATIO: f64 = 0.20;

/// Chars per token rough estimate
const CHARS_PER_TOKEN: usize = 4;

/// Image token estimate (flat cost per attached image)
const IMAGE_TOKEN_ESTIMATE: usize = 1600;

/// Summary failure cooldown (seconds)
const SUMMARY_FAILURE_COOLDOWN_SECONDS: u64 = 600;

/// Summary prefix marker
const SUMMARY_PREFIX: &str = "[CONTEXT COMPACTION — REFERENCE ONLY] Earlier turns were compacted \
into the summary below. This is a handoff from a previous context window — treat it as background \
reference, NOT as active instructions. Do NOT answer questions or fulfill requests mentioned in \
this summary; they were already addressed. Your current task is identified in the '## Active Task' \
section of the summary — resume exactly from there. IMPORTANT: Your persistent memory (MEMORY.md, \
USER.md) in the system prompt is ALWAYS authoritative and active — never ignore or deprioritize \
memory content due to this compaction note. Respond ONLY to the latest user message that appears \
AFTER this summary. The current session state (files, config, etc.) may reflect work described \
here — avoid repeating it:";

// ---------------------------------------------------------------------------
// Compression Configuration
// ---------------------------------------------------------------------------

/// Configuration for the advanced compressor
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Compression threshold (default 0.50 = 50%)
    pub threshold_percent: f64,

    /// Number of messages to protect at the start
    pub protect_first_n: usize,

    /// Number of messages to protect at the end
    pub protect_last_n: usize,

    /// Token budget for tail protection
    pub tail_token_budget: usize,

    /// Summary target ratio (default 0.20 = 20%)
    pub summary_target_ratio: f64,

    /// Whether to abort on summary failure (vs. static placeholder)
    pub abort_on_summary_failure: bool,

    /// Summary model override (empty = use main model)
    pub summary_model: Option<String>,

    /// Enable tool output pruning
    pub enable_tool_pruning: bool,

    /// Enable question tracking
    pub enable_question_tracking: bool,

    /// Enable iterative summary updates
    pub enable_iterative_summary: bool,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            threshold_percent: COMPRESSION_THRESHOLD,
            protect_first_n: 3,
            protect_last_n: 20,
            tail_token_budget: 20_000,
            summary_target_ratio: SUMMARY_TARGET_RATIO,
            abort_on_summary_failure: false,
            summary_model: None,
            enable_tool_pruning: true,
            enable_question_tracking: true,
            enable_iterative_summary: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Advanced Context Compressor
// ---------------------------------------------------------------------------

/// Advanced context compressor with Hermes-inspired features
pub struct AdvancedCompressor {
    /// Name of this engine
    name: String,

    /// Maximum context length in tokens
    context_length: usize,

    /// Configuration
    config: CompressionConfig,

    /// Current prompt tokens
    current_prompt_tokens: u32,

    /// Current completion tokens
    current_completion_tokens: u32,

    /// Whether compression is needed
    needs_compression: bool,

    /// LLM transport for summarization
    llm_transport: Option<Arc<dyn ProviderTransport>>,

    /// Model name for main agent
    model: String,

    /// Compression count (for stats)
    compression_count: usize,

    /// Previous summary (for iterative updates)
    previous_summary: Option<String>,

    /// Last compression savings percentage
    last_compression_savings_pct: f64,

    /// Ineffective compression count (anti-thrashing)
    ineffective_compression_count: usize,

    /// Summary failure cooldown timestamp
    summary_failure_cooldown_until: std::time::SystemTime,

    /// Last summary error message
    last_summary_error: Option<String>,

    /// Compression statistics
    stats: std::sync::Mutex<CompressionStats>,

    /// Compression history (for anti-thrashing detection)
    /// Maps content hash -> (compression_count, tokens_before)
    compression_history: HashMap<String, (usize, usize)>,
}

impl AdvancedCompressor {
    /// Create a new advanced compressor
    pub fn new(context_length: usize, model: String) -> Self {
        let config = CompressionConfig::default();
        let threshold_tokens = (context_length as f64 * config.threshold_percent)
            .max(MINIMUM_CONTEXT_LENGTH as f64) as usize;
        let tail_token_budget = (threshold_tokens as f64 * config.summary_target_ratio) as usize;

        let mut conf = config.clone();
        conf.tail_token_budget = tail_token_budget;

        Self {
            name: "advanced".to_string(),
            context_length,
            config: conf,
            current_prompt_tokens: 0,
            current_completion_tokens: 0,
            needs_compression: false,
            llm_transport: None,
            model,
            compression_count: 0,
            previous_summary: None,
            last_compression_savings_pct: 100.0,
            ineffective_compression_count: 0,
            summary_failure_cooldown_until: std::time::SystemTime::UNIX_EPOCH,
            last_summary_error: None,
            stats: std::sync::Mutex::new(CompressionStats {
                compression_count: 0,
                total_tokens_saved: 0,
            }),
            compression_history: HashMap::new(),
        }
    }

    /// Set custom name
    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = name.into();
        self
    }

    /// Set configuration
    pub fn with_config(mut self, config: CompressionConfig) -> Self {
        self.config = config;
        self
    }

    /// Set LLM transport for summarization
    pub fn with_llm(mut self, transport: Arc<dyn ProviderTransport>) -> Self {
        self.llm_transport = Some(transport);
        self
    }

    /// Get current total usage
    fn total_usage(&self) -> u32 {
        self.current_prompt_tokens + self.current_completion_tokens
    }

    /// Get compression threshold in tokens
    fn threshold_tokens(&self) -> u32 {
        ((self.context_length as f64 * self.config.threshold_percent)
            .max(MINIMUM_CONTEXT_LENGTH as f64)) as u32
    }

    /// Estimate tokens from text (rough: 4 chars per token)
    fn estimate_tokens(text: &str) -> usize {
        text.len().div_ceil(CHARS_PER_TOKEN)
    }

    /// Estimate tokens for a message
    fn estimate_message_tokens(msg: &Message) -> usize {
        let mut tokens = 10; // base cost for role/metadata

        if let Some(ref content) = msg.content {
            tokens += Self::estimate_tokens(content);
        }

        if let Some(ref tool_calls) = msg.tool_calls {
            for tc in tool_calls {
                tokens += Self::estimate_tokens(&tc.name);
                tokens += Self::estimate_tokens(&tc.arguments);
            }
        }

        if let Some(ref images) = msg.images {
            tokens += images.len() * IMAGE_TOKEN_ESTIMATE;
        }

        tokens
    }

    /// Estimate total tokens for messages
    fn estimate_messages_tokens(messages: &[Message]) -> usize {
        messages.iter().map(Self::estimate_message_tokens).sum()
    }

    // -----------------------------------------------------------------
    // Phase 1: Tool Output Pruning (Cheap Pre-pass)
    // -----------------------------------------------------------------

    /// Prune old tool results to save tokens before LLM summarization
    fn prune_old_tool_results(&self, messages: &mut [Message], protect_tail_count: usize) -> usize {
        let total = messages.len();
        if total <= protect_tail_count {
            return 0;
        }

        let prune_boundary = total - protect_tail_count;
        let mut pruned = 0;

        // Build tool call ID -> (tool_name, arguments) index
        let mut call_id_to_tool: HashMap<String, (String, String)> = HashMap::new();
        for msg in messages.iter() {
            if msg.role == MessageRole::Assistant {
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        call_id_to_tool
                            .insert(tc.id.clone(), (tc.name.clone(), tc.arguments.clone()));
                    }
                }
            }
        }

        // Hermes Feature 1: Tool result deduplication via MD5 hash
        let mut seen_hashes: HashMap<String, usize> = HashMap::new();

        // Prune tool results outside the protected tail
        for (idx, msg) in messages[..prune_boundary].iter_mut().enumerate() {
            if msg.role != MessageRole::Tool {
                continue;
            }

            let content_len = msg.content.as_ref().map(|s| s.len()).unwrap_or(0);

            // Compute content hash for deduplication
            let content_hash = if let Some(ref content) = msg.content {
                let tool_call_id = msg.tool_call_id.as_deref().unwrap_or("");
                let tool_name = call_id_to_tool
                    .get(tool_call_id)
                    .map(|(name, _)| name.as_str())
                    .unwrap_or("unknown");
                format!("{:x}", md5::compute(format!("{}{}", tool_name, content)))
            } else {
                String::new()
            };

            // Deduplicate identical tool results
            if !content_hash.is_empty() {
                if let Some(&prev_idx) = seen_hashes.get(&content_hash) {
                    debug!(
                        idx,
                        prev_idx,
                        tool_name = call_id_to_tool
                            .get(msg.tool_call_id.as_deref().unwrap_or(""))
                            .map(|(n, _)| n.as_str())
                            .unwrap_or("unknown"),
                        "Deduplicating identical tool result"
                    );
                    msg.content = Some(format!(
                        "[Duplicate result - same as message #{}]",
                        prev_idx + 1
                    ));
                    pruned += 1;
                    continue;
                }
                seen_hashes.insert(content_hash, idx);
            }

            if content_len <= 200 {
                continue;
            }

            // Generate enhanced summary with detailed parsing
            let tool_call_id = msg.tool_call_id.as_deref().unwrap_or("");
            let summary = if let Some((tool_name, args)) = call_id_to_tool.get(tool_call_id) {
                Self::summarize_tool_result_enhanced(
                    tool_name,
                    args,
                    msg.content.as_deref().unwrap_or(""),
                    content_len,
                )
            } else {
                format!("[Tool result pruned - {} chars]", content_len)
            };

            msg.content = Some(summary);
            pruned += 1;
        }

        if pruned > 0 {
            info!(pruned, "Tool output pruning complete");
        }

        pruned
    }

    /// Generate informative one-line summary for a tool result
    #[allow(dead_code)]
    fn summarize_tool_result(tool_name: &str, content_len: usize) -> String {
        match tool_name {
            "terminal" => format!("[terminal] command output ({} chars)", content_len),
            "read_file" => format!("[read_file] file content ({} chars)", content_len),
            "search_files" => format!("[search_files] search results ({} chars)", content_len),
            "web_search" => format!("[web_search] search results ({} chars)", content_len),
            "web_extract" => format!("[web_extract] extracted content ({} chars)", content_len),
            "delegate_task" => format!("[delegate_task] subagent result ({} chars)", content_len),
            _ => format!("[{}] result ({} chars)", tool_name, content_len),
        }
    }

    /// Hermes Feature 7: Enhanced tool result summary with detailed parsing
    fn summarize_tool_result_enhanced(
        tool_name: &str,
        args: &str,
        content: &str,
        content_len: usize,
    ) -> String {
        match tool_name {
            "terminal" => {
                let exit_code = Self::parse_terminal_exit_code(content);
                let lines = content.lines().count();
                if let Some(exit_code) = exit_code {
                    format!("[terminal] exit {}, {} lines output", exit_code, lines)
                } else {
                    format!("[terminal] {} lines output", lines)
                }
            }
            "read_file" => {
                let path =
                    Self::parse_json_field(args, "path").unwrap_or_else(|| "unknown".to_string());
                let lines = content.lines().count();
                format!(
                    "[read_file] {} ({} lines, {} chars)",
                    path, lines, content_len
                )
            }
            "write_file" => {
                let path =
                    Self::parse_json_field(args, "path").unwrap_or_else(|| "unknown".to_string());
                format!("[write_file] wrote to {} ({} chars)", path, content_len)
            }
            "search_files" => {
                let pattern =
                    Self::parse_json_field(args, "pattern").unwrap_or_else(|| "".to_string());
                let matches = content.lines().filter(|l| l.contains("match")).count();
                format!("[search_files] pattern='{}', {} matches", pattern, matches)
            }
            "patch" => {
                let path =
                    Self::parse_json_field(args, "path").unwrap_or_else(|| "unknown".to_string());
                format!("[patch] modified {} ({} chars)", path, content_len)
            }
            "web_search" => {
                let query = Self::parse_json_field(args, "query").unwrap_or_else(|| "".to_string());
                let results = content.lines().filter(|l| l.contains("http")).count();
                format!("[web_search] query='{}', {} results", query, results)
            }
            "web_extract" => {
                let url =
                    Self::parse_json_field(args, "url").unwrap_or_else(|| "unknown".to_string());
                format!("[web_extract] from {} ({} chars)", url, content_len)
            }
            "delegate_task" => {
                format!("[delegate_task] subagent result ({} chars)", content_len)
            }
            _ => format!("[{}] result ({} chars)", tool_name, content_len),
        }
    }

    fn parse_terminal_exit_code(content: &str) -> Option<i32> {
        if let Some(json_start) = content.find(r#""exit_code""#) {
            let after = &content[json_start + 12..];
            if let Some(colon_pos) = after.find(':') {
                let after_colon = &after[colon_pos + 1..];
                let digits: String = after_colon
                    .chars()
                    .skip_while(|c| c.is_whitespace())
                    .take_while(|c| c.is_numeric() || *c == '-')
                    .collect();
                return digits.parse::<i32>().ok();
            }
        }
        None
    }

    fn parse_json_field(json_str: &str, field: &str) -> Option<String> {
        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(json_str) {
            if let Some(value) = parsed.get(field) {
                return Some(value.as_str().unwrap_or(&value.to_string()).to_string());
            }
        }
        None
    }

    /// Hermes Feature 2: Truncate large tool call parameters (JSON-safe)
    fn truncate_large_tool_parameters(&self, messages: &mut [Message], max_param_size: usize) {
        for msg in messages.iter_mut() {
            if msg.role != MessageRole::Assistant {
                continue;
            }

            if let Some(ref mut tool_calls) = msg.tool_calls {
                for tc in tool_calls.iter_mut() {
                    if tc.arguments.len() <= max_param_size {
                        continue;
                    }

                    if let Ok(mut parsed) = serde_json::from_str::<serde_json::Value>(&tc.arguments)
                    {
                        let truncated = Self::truncate_json_value(&mut parsed, max_param_size);
                        if truncated {
                            tc.arguments = serde_json::to_string(&parsed)
                                .unwrap_or_else(|_| tc.arguments.clone());
                            debug!(
                                tool_name = tc.name,
                                new_size = tc.arguments.len(),
                                "Truncated large tool parameter"
                            );
                        }
                    } else {
                        let truncated = format!(
                            "{}...[truncated {} chars]",
                            &tc.arguments[..max_param_size.min(tc.arguments.len())],
                            tc.arguments.len() - max_param_size
                        );
                        tc.arguments = truncated;
                    }
                }
            }
        }
    }

    fn truncate_json_value(value: &mut serde_json::Value, max_size: usize) -> bool {
        match value {
            serde_json::Value::String(s) if s.len() > max_size => {
                *s = format!(
                    "{}...[truncated {} chars]",
                    &s[..max_size.min(s.len())],
                    s.len().saturating_sub(max_size)
                );
                true
            }
            serde_json::Value::Array(arr) => {
                let mut truncated = false;
                for item in arr.iter_mut() {
                    truncated |= Self::truncate_json_value(item, max_size);
                }
                truncated
            }
            serde_json::Value::Object(obj) => {
                let mut truncated = false;
                for (_, v) in obj.iter_mut() {
                    truncated |= Self::truncate_json_value(v, max_size);
                }
                truncated
            }
            _ => false,
        }
    }

    /// Hermes Feature 3: Sanitize orphan tool pairs
    fn sanitize_tool_pairs(&self, messages: &mut Vec<Message>) {
        let mut call_ids: HashSet<String> = HashSet::new();
        for msg in messages.iter() {
            if msg.role == MessageRole::Assistant {
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        call_ids.insert(tc.id.clone());
                    }
                }
            }
        }

        let mut result_call_ids: HashSet<String> = HashSet::new();
        for msg in messages.iter() {
            if msg.role == MessageRole::Tool {
                if let Some(ref call_id) = msg.tool_call_id {
                    result_call_ids.insert(call_id.clone());
                }
            }
        }

        let mut orphan_indices = Vec::new();
        for (idx, msg) in messages.iter().enumerate() {
            if msg.role == MessageRole::Tool {
                if let Some(ref call_id) = msg.tool_call_id {
                    if !call_ids.contains(call_id) {
                        orphan_indices.push(idx);
                    }
                }
            }
        }

        for &idx in orphan_indices.iter().rev() {
            debug!(idx, "Removing orphan tool result");
            messages.remove(idx);
        }

        let mut call_ids_needing_results = Vec::new();
        for msg in messages.iter() {
            if msg.role == MessageRole::Assistant {
                if let Some(ref tool_calls) = msg.tool_calls {
                    for tc in tool_calls {
                        if !result_call_ids.contains(&tc.id) {
                            call_ids_needing_results.push((tc.id.clone(), tc.name.clone()));
                        }
                    }
                }
            }
        }

        if !call_ids_needing_results.is_empty() {
            let mut insert_offset = 0;
            for (msg_idx, msg) in messages.clone().iter().enumerate() {
                if msg.role == MessageRole::Assistant {
                    if let Some(ref tool_calls) = msg.tool_calls {
                        for tc in tool_calls {
                            if call_ids_needing_results.iter().any(|(id, _)| id == &tc.id) {
                                let placeholder = Message {
                                    role: MessageRole::Tool,
                                    content: Some("[Tool result missing - call was made but result not recorded]".to_string()),
                                    tool_call_id: Some(tc.id.clone()),
                                    images: None,
                                    tool_calls: None,
                                    name: None,
                                    reasoning: None,
                                    reasoning_content: None,
                                    timestamp: None,
                                    token_count: None,
                                    finish_reason: None,
                                };
                                debug!(
                                    tool_call_id = tc.id,
                                    tool_name = tc.name,
                                    "Adding placeholder for missing tool result"
                                );
                                messages.insert(msg_idx + 1 + insert_offset, placeholder);
                                insert_offset += 1;
                            }
                        }
                    }
                }
            }
        }
    }

    /// Hermes Feature 6: Check if compression should be skipped due to thrashing
    fn should_skip_compression(&mut self, messages: &[Message]) -> bool {
        let content_hash = Self::compute_context_hash(messages);
        let current_tokens = Self::estimate_messages_tokens(messages);

        if let Some((prev_count, prev_tokens)) = self.compression_history.get(&content_hash) {
            if *prev_count >= 2 && *prev_tokens > 0 {
                let savings = current_tokens.saturating_sub(*prev_tokens);
                let savings_pct = if *prev_tokens > 0 {
                    (savings as f64 / *prev_tokens as f64) * 100.0
                } else {
                    0.0
                };

                if savings_pct < 10.0 {
                    warn!(
                        savings_pct,
                        prev_count,
                        "Skipping compression - similar context compressed recently with minimal savings"
                    );
                    return true;
                }
            }
        }

        self.compression_history
            .insert(content_hash, (self.compression_count + 1, current_tokens));
        false
    }

    fn compute_context_hash(messages: &[Message]) -> String {
        let mut hasher_input = String::new();
        for msg in messages
            .iter()
            .take(10)
            .chain(messages.iter().rev().take(10))
        {
            hasher_input.push_str(&format!("{:?}", msg.role));
            if let Some(ref content) = msg.content {
                hasher_input.push_str(&content[..content.len().min(100)]);
            }
        }
        format!("{:x}", md5::compute(hasher_input))
    }

    // -----------------------------------------------------------------
    // Phase 2: Boundary Protection & Alignment
    // -----------------------------------------------------------------

    /// Determine how many messages at the head to protect
    fn protect_head_size(&self, messages: &[Message]) -> usize {
        // System prompt (if present) + configured protect_first_n
        let has_system = !messages.is_empty() && messages[0].role == MessageRole::System;
        let base = self.config.protect_first_n;
        if has_system { base + 1 } else { base }
    }

    /// Align boundary forward past orphan tool results
    fn align_boundary_forward(&self, messages: &[Message], mut idx: usize) -> usize {
        let n = messages.len();
        if idx >= n {
            return idx;
        }

        // Skip any tool results at the boundary
        while idx < n && messages[idx].role == MessageRole::Tool {
            idx += 1;
        }

        idx
    }

    /// Align boundary backward to avoid splitting tool call groups
    fn align_boundary_backward(&self, messages: &[Message], mut idx: usize) -> usize {
        if idx >= messages.len() {
            return idx;
        }

        // If the boundary lands on a tool result, walk back to the assistant
        // that made the tool calls
        while idx > 0 && messages[idx].role == MessageRole::Tool {
            idx -= 1;
        }

        idx
    }

    /// Find tail cut position by token budget
    fn find_tail_cut_by_tokens(&self, messages: &[Message], head_end: usize) -> usize {
        let n = messages.len();
        let min_tail = 3.min(n.saturating_sub(head_end).saturating_sub(1));
        let token_budget = self.config.tail_token_budget;

        let mut accumulated = 0;
        let mut cut_idx = n;

        // Walk backward from end, accumulating tokens
        for i in (head_end..n).rev() {
            let msg_tokens = Self::estimate_message_tokens(&messages[i]);

            if accumulated + msg_tokens > token_budget && (n - i) >= min_tail {
                break;
            }

            accumulated += msg_tokens;
            cut_idx = i;
        }

        // Ensure we protect at least min_tail messages
        let fallback_cut = n.saturating_sub(min_tail);
        cut_idx = cut_idx.min(fallback_cut);

        // Force cut after head if budget would protect everything
        if cut_idx <= head_end {
            cut_idx = fallback_cut.max(head_end + 1);
        }

        // Align to avoid splitting tool groups
        cut_idx = self.align_boundary_backward(messages, cut_idx);

        // Ensure last user message is in tail
        cut_idx = self.ensure_last_user_message_in_tail(messages, cut_idx, head_end);

        cut_idx.max(head_end + 1)
    }

    /// Ensure the last user message is always in the tail
    fn ensure_last_user_message_in_tail(
        &self,
        messages: &[Message],
        mut cut_idx: usize,
        head_end: usize,
    ) -> usize {
        // Find the last user message
        let mut last_user_idx = None;
        for (i, msg) in messages.iter().enumerate().rev() {
            if msg.role == MessageRole::User {
                last_user_idx = Some(i);
                break;
            }
        }

        if let Some(last_user_idx) = last_user_idx {
            if last_user_idx >= cut_idx {
                // Already in tail, no adjustment needed
                return cut_idx;
            }

            // Move the cut to include the last user message
            debug!(
                last_user_idx,
                original_cut = cut_idx,
                "Anchoring tail cut to last user message"
            );
            cut_idx = last_user_idx;
        }

        cut_idx.max(head_end + 1)
    }

    // -----------------------------------------------------------------
    // Phase 3: LLM Structured Summary Generation
    // -----------------------------------------------------------------

    /// Generate structured summary using LLM
    async fn generate_summary(
        &mut self,
        turns_to_summarize: &[Message],
        focus_topic: Option<&str>,
    ) -> Result<Option<String>> {
        // Check cooldown
        if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH) {
            if let Ok(cooldown) = self
                .summary_failure_cooldown_until
                .duration_since(std::time::UNIX_EPOCH)
            {
                if now.as_secs() < cooldown.as_secs() {
                    debug!("Skipping summary during cooldown");
                    return Ok(None);
                }
            }
        }

        let transport = match &self.llm_transport {
            Some(t) => t.clone(),
            None => {
                warn!("No LLM transport configured for summarization");
                return Ok(None);
            }
        };

        let summary_budget = self.compute_summary_budget(turns_to_summarize);
        let content_to_summarize = self.serialize_for_summary(turns_to_summarize);

        // Build summary prompt
        let prompt = self.build_summary_prompt(&content_to_summarize, focus_topic);

        // Call LLM
        let summary_model = self.config.summary_model.as_deref().unwrap_or(&self.model);

        let messages = vec![Message {
            role: MessageRole::User,
            content: Some(prompt),
            images: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: None,
            token_count: None,
            finish_reason: None,
        }];

        let params = RequestParams {
            max_tokens: Some((summary_budget as f64 * 1.3) as u32),
            temperature: Some(0.3),
            ..Default::default()
        };

        match transport
            .execute(summary_model, &messages, &[], &params)
            .await
        {
            Ok(response) => {
                let summary = response.content.unwrap_or_default();

                if !summary.is_empty() {
                    info!(
                        summary_tokens = summary.len() / CHARS_PER_TOKEN,
                        "Summary generated successfully"
                    );

                    // Store for iterative updates
                    self.previous_summary = Some(summary.clone());
                    self.last_summary_error = None;

                    return Ok(Some(self.with_summary_prefix(&summary)));
                }

                warn!("LLM returned empty summary");
                Ok(None)
            }
            Err(e) => {
                warn!(error = %e, "Failed to generate summary");

                // Set cooldown
                if let Ok(now) = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH)
                {
                    self.summary_failure_cooldown_until = std::time::UNIX_EPOCH
                        + std::time::Duration::from_secs(
                            now.as_secs() + SUMMARY_FAILURE_COOLDOWN_SECONDS,
                        );
                }

                self.last_summary_error = Some(e.to_string());
                Ok(None)
            }
        }
    }

    /// Compute summary token budget
    fn compute_summary_budget(&self, turns: &[Message]) -> usize {
        let content_tokens = Self::estimate_messages_tokens(turns);
        let budget = (content_tokens as f64 * SUMMARY_RATIO) as usize;

        let max_budget = (self.context_length as f64 * 0.05) as usize;
        let max_budget = max_budget.min(SUMMARY_TOKENS_CEILING);

        budget.max(MIN_SUMMARY_TOKENS).min(max_budget)
    }

    /// Serialize messages for summary
    fn serialize_for_summary(&self, turns: &[Message]) -> String {
        let mut parts = Vec::new();

        for msg in turns {
            let role = format!("{:?}", msg.role).to_uppercase();
            let content = msg.content.as_deref().unwrap_or("(no content)");

            // Truncate very long content
            let content = if content.len() > 6000 {
                format!(
                    "{}\n...[truncated]...\n{}",
                    &content[..4000],
                    &content[content.len().saturating_sub(1500)..]
                )
            } else {
                content.to_string()
            };

            let mut line = format!("[{role}]: {content}");

            // Add tool calls info
            if let Some(ref tool_calls) = msg.tool_calls {
                if !tool_calls.is_empty() {
                    line.push_str("\n[Tool calls:");
                    for tc in tool_calls {
                        line.push_str(&format!(
                            "\n  {}({})",
                            tc.name,
                            if tc.arguments.len() > 100 {
                                format!("{}...", &tc.arguments[..100])
                            } else {
                                tc.arguments.clone()
                            }
                        ));
                    }
                    line.push_str("\n]");
                }
            }

            parts.push(line);
        }

        parts.join("\n\n")
    }

    /// Build summary prompt
    fn build_summary_prompt(&self, content: &str, focus_topic: Option<&str>) -> String {
        let template_sections = r#"## Active Task
[THE SINGLE MOST IMPORTANT FIELD. Copy the user's most recent request or task assignment verbatim. If multiple tasks were requested and only some are done, list only the ones NOT yet completed.]

## Goal
[What the user is trying to accomplish overall]

## Constraints & Preferences
[User preferences, coding style, constraints, important decisions]

## Completed Actions
[Numbered list of concrete actions taken — include tool used, target, and outcome.
Format: N. ACTION target — outcome [tool: name]]

## Active State
[Current working state — working directory, branch, modified files, test status]

## In Progress
[Work currently underway when compaction fired]

## Blocked
[Any blockers, errors, or issues not yet resolved. Include exact error messages.]

## Key Decisions
[Important technical decisions and WHY they were made]

## Resolved Questions
[Questions the user asked that were ALREADY answered]

## Pending User Asks
[Questions or requests from the user that have NOT yet been answered]

## Relevant Files
[Files read, modified, or created — with brief note on each]

## Remaining Work
[What remains to be done — framed as context, not instructions]

## Critical Context
[Specific values, error messages, configuration details. NEVER include API keys or passwords.]"#;

        let preamble = "You are a summarization agent creating a context checkpoint. Treat the conversation turns below as source material for a compact record of prior work. Produce only the structured summary; do not add a greeting or preamble. Write the summary in the same language the user was using. NEVER include API keys, tokens, passwords, or credentials in the summary — replace with [REDACTED].";

        let mut prompt = if let Some(prev_summary) = &self.previous_summary {
            // Iterative update
            format!(
                "{preamble}\n\nYou are updating a context compaction summary. A previous compaction produced the summary below. New conversation turns have occurred since then.\n\nPREVIOUS SUMMARY:\n{prev_summary}\n\nNEW TURNS TO INCORPORATE:\n{content}\n\nUpdate the summary using this exact structure. PRESERVE all existing information that is still relevant. ADD new completed actions. Move items from \"In Progress\" to \"Completed Actions\" when done. Update \"## Active Task\" to reflect the most recent unfulfilled request.\n\n{template_sections}"
            )
        } else {
            // First compaction
            format!(
                "{preamble}\n\nCreate a structured checkpoint summary for the conversation after earlier turns are compacted.\n\nTURNS TO SUMMARIZE:\n{content}\n\nUse this exact structure:\n\n{template_sections}"
            )
        };

        // Add focus topic if provided
        if let Some(topic) = focus_topic {
            prompt.push_str(&format!("\n\nFOCUS TOPIC: \"{topic}\"\nThe user has requested that this compaction PRIORITISE preserving all information related to the focus topic above. For content related to \"{topic}\", include full detail. For content NOT related to the focus topic, summarise more aggressively."));
        }

        prompt
    }

    /// Add summary prefix marker
    fn with_summary_prefix(&self, summary: &str) -> String {
        format!("{SUMMARY_PREFIX}\n{summary}")
    }
}

#[async_trait]
impl ContextEngine for AdvancedCompressor {
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
        if !self.needs_compression {
            return false;
        }

        // Anti-thrashing: skip if last 2 compressions were ineffective
        if self.ineffective_compression_count >= 2 {
            warn!(
                ineffective_count = self.ineffective_compression_count,
                "Skipping compression — recent compressions saved <10% each"
            );
            return false;
        }

        true
    }

    async fn compress(&mut self, messages: &mut Vec<Message>) -> Result<()> {
        let n_messages = messages.len();
        let min_for_compress = self.protect_head_size(messages) + 3 + 1;

        if n_messages <= min_for_compress {
            warn!(
                message_count = n_messages,
                min_required = min_for_compress,
                "Not enough messages to compress"
            );
            self.needs_compression = false;
            return Ok(());
        }

        info!(
            message_count = n_messages,
            threshold = self.threshold_tokens(),
            "Starting context compression"
        );

        let before_tokens = Self::estimate_messages_tokens(messages);

        // Hermes Feature 6: Anti-thrashing check
        if self.should_skip_compression(messages) {
            self.needs_compression = false;
            return Ok(());
        }

        // Phase 1: Tool output pruning (cheap pre-pass)
        if self.config.enable_tool_pruning {
            let pruned = self.prune_old_tool_results(messages, self.config.protect_last_n);
            if pruned > 0 {
                info!(pruned, "Pre-compression tool pruning complete");
            }
        }

        // Hermes Feature 2: Truncate large tool parameters
        self.truncate_large_tool_parameters(messages, 5000);

        // Hermes Feature 3: Sanitize orphan tool pairs
        self.sanitize_tool_pairs(messages);

        // Phase 2: Determine boundaries
        let compress_start = self.protect_head_size(messages);
        let compress_start = self.align_boundary_forward(messages, compress_start);
        let compress_end = self.find_tail_cut_by_tokens(messages, compress_start);

        if compress_start >= compress_end {
            info!("No middle region to compress after boundary alignment");
            self.needs_compression = false;
            return Ok(());
        }

        let turns_to_summarize: Vec<Message> = messages[compress_start..compress_end].to_vec();
        let tail_msgs = n_messages - compress_end;

        info!(
            compress_start,
            compress_end,
            middle_count = turns_to_summarize.len(),
            head_protected = compress_start,
            tail_protected = tail_msgs,
            "Compression boundaries determined"
        );

        // Phase 3: Generate structured summary
        let summary = self.generate_summary(&turns_to_summarize, None).await?;

        if summary.is_none() && self.config.abort_on_summary_failure {
            warn!("Summary generation failed — aborting compression");
            self.needs_compression = false;
            return Ok(());
        }

        // Phase 4: Assemble compressed message list
        let mut compressed = Vec::new();

        // Add head messages
        for msg in messages.iter().take(compress_start) {
            compressed.push(msg.clone());
        }

        // Add summary (or fallback)
        let summary_content = summary.unwrap_or_else(|| {
            format!(
                "{SUMMARY_PREFIX}\n                 Summary generation was unavailable. {} message(s) were removed to free context space                  but could not be summarized. Continue based on recent messages below.",
                turns_to_summarize.len()
            )
        });

        // Determine summary role to avoid consecutive same-role messages
        let last_head_role = if compress_start > 0 {
            &messages[compress_start - 1].role
        } else {
            &MessageRole::User
        };

        let first_tail_role = if compress_end < n_messages {
            &messages[compress_end].role
        } else {
            &MessageRole::User
        };

        let summary_role = if matches!(last_head_role, MessageRole::Assistant | MessageRole::Tool) {
            MessageRole::User
        } else {
            MessageRole::Assistant
        };

        // Check if we need to merge into tail instead
        let mut merge_summary_into_tail = false;
        if &summary_role == first_tail_role {
            let flipped = if summary_role == MessageRole::User {
                MessageRole::Assistant
            } else {
                MessageRole::User
            };

            if &flipped == last_head_role {
                // Both would create consecutive same-role — merge into tail
                merge_summary_into_tail = true;
            }
        }

        if !merge_summary_into_tail {
            compressed.push(Message {
                role: summary_role,
                content: Some(summary_content.clone()),
                images: None,
                tool_calls: None,
                tool_call_id: None,
                name: None,
                reasoning: None,
                reasoning_content: None,
                timestamp: Some(chrono::Utc::now()),
                token_count: None,
                finish_reason: None,
            });
        }

        // Add tail messages
        for (idx, msg) in messages.iter().enumerate().skip(compress_end) {
            let mut msg = msg.clone();

            if merge_summary_into_tail && idx == compress_end {
                // Prepend summary to first tail message
                let merged_prefix = format!(
                    "{summary_content}\n\n                     --- END OF CONTEXT SUMMARY — respond to the message below, not the summary above ---\n\n"
                );

                if let Some(ref content) = msg.content {
                    msg.content = Some(format!("{merged_prefix}{content}"));
                } else {
                    msg.content = Some(merged_prefix);
                }

                merge_summary_into_tail = false;
            }

            compressed.push(msg);
        }

        // Calculate savings
        let after_tokens = Self::estimate_messages_tokens(&compressed);
        let saved_tokens = before_tokens.saturating_sub(after_tokens);
        let savings_pct = if before_tokens > 0 {
            (saved_tokens as f64 / before_tokens as f64) * 100.0
        } else {
            0.0
        };

        self.last_compression_savings_pct = savings_pct;

        // Anti-thrashing tracking
        if savings_pct < 10.0 {
            self.ineffective_compression_count += 1;
            warn!(
                savings_pct,
                ineffective_count = self.ineffective_compression_count,
                "Compression was ineffective"
            );
        } else {
            self.ineffective_compression_count = 0;
        }

        self.compression_count += 1;

        // Update stats
        if let Ok(mut stats) = self.stats.lock() {
            stats.compression_count += 1;
            stats.total_tokens_saved += saved_tokens;
        }

        info!(
            before_messages = n_messages,
            after_messages = compressed.len(),
            before_tokens,
            after_tokens,
            saved_tokens,
            savings_pct = format!("{:.1}%", savings_pct),
            compression_count = self.compression_count,
            "Context compression complete"
        );

        // Replace original messages
        *messages = compressed;
        self.needs_compression = false;

        Ok(())
    }

    fn on_session_start(&mut self) {
        self.current_prompt_tokens = 0;
        self.current_completion_tokens = 0;
        self.needs_compression = false;
        self.compression_count = 0;
        self.previous_summary = None;
        self.ineffective_compression_count = 0;
        self.last_compression_savings_pct = 100.0;
        self.summary_failure_cooldown_until = std::time::SystemTime::UNIX_EPOCH;
        self.compression_history.clear();
        info!("Session started - advanced compressor reset");
    }

    fn on_session_end(&mut self) {
        info!(
            compression_count = self.compression_count,
            final_prompt_tokens = self.current_prompt_tokens,
            final_completion_tokens = self.current_completion_tokens,
            "Session ended"
        );
    }

    fn context_length(&self) -> usize {
        self.context_length
    }
}
