//! Tool call guardrails for loop detection and idempotency tracking.
//!
//! Detects infinite tool-calling loops, tracks repeated identical calls,
//! and provides halt/warning decisions to protect against runaway agents.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use tracing::{debug, warn};

const IDEMPOTENT_TOOL_NAMES: &[&str] = &[
    "read_file",
    "search_files",
    "web_search",
    "web_extract",
    "session_search",
    "browser_snapshot",
    "browser_console",
    "browser_get_images",
    "knowledge_get_context",
];

const MUTATING_TOOL_NAMES: &[&str] = &[
    "terminal",
    "process",
    "code_exec",
    "write_file",
    "patch",
    "todo",
    "memory",
    "skill_manage",
    "browser_click",
    "browser_type",
    "browser_press",
    "browser_scroll",
    "browser_navigate",
    "send_message",
    "cronjob",
    "delegate_task",
];

const NO_PROGRESS_WARN_AFTER: usize = 2;
const NO_PROGRESS_BLOCK_AFTER: usize = 5;

/// Decision returned by the guardrails system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardrailDecision {
    /// Allow the tool call — no issues detected.
    Allow,
    /// Warn about a potential loop but allow the call.
    Warn(String),
    /// Inject a synthetic error result to break the loop.
    SyntheticResult(String),
    /// Halt the agent loop entirely.
    Halt(String),
}

/// Observation of a single tool call within a turn.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallObservation {
    /// Tool name.
    pub tool_name: String,
    /// Serialized arguments.
    pub arguments: String,
    /// Hash of the tool result (to detect stalled output).
    pub result_hash: Option<String>,
    /// Turn number when this call was made.
    pub turn: usize,
}

/// Tracks per-turn tool call patterns and detects loops.
pub struct ToolGuardrails {
    /// Observations for the current turn.
    observations: Vec<ToolCallObservation>,
    /// Current turn number.
    current_turn: usize,
    /// Maximum identical calls before halting.
    max_identical_calls: usize,
    /// Maximum total tool calls per turn before warning.
    max_calls_per_turn: usize,
    /// Maximum total tool calls per turn before halting.
    hard_limit_per_turn: usize,
    /// History of tool call counts per turn (for pattern detection).
    turn_history: Vec<usize>,
}

impl ToolGuardrails {
    /// Create a new guardrails instance with default limits.
    pub fn new() -> Self {
        Self {
            observations: Vec::new(),
            current_turn: 0,
            max_identical_calls: 5,
            max_calls_per_turn: 30,
            hard_limit_per_turn: 50,
            turn_history: Vec::new(),
        }
    }

    /// Create a guardrails instance with custom limits.
    pub fn with_limits(
        max_identical_calls: usize,
        max_calls_per_turn: usize,
        hard_limit_per_turn: usize,
    ) -> Self {
        Self {
            max_identical_calls,
            max_calls_per_turn,
            hard_limit_per_turn,
            ..Self::new()
        }
    }

    /// Start a new turn — resets per-turn tracking.
    pub fn begin_turn(&mut self) {
        if !self.observations.is_empty() {
            self.turn_history.push(self.observations.len());
        }
        self.observations.clear();
        self.current_turn += 1;
        debug!(turn = self.current_turn, "Guardrails: new turn started");
    }

    /// Record a tool call and return a guardrail decision.
    pub fn record_call(&mut self, tool_name: &str, arguments: &str) -> GuardrailDecision {
        let canonical_arguments = canonical_json(arguments);
        let observation = ToolCallObservation {
            tool_name: tool_name.to_string(),
            arguments: canonical_arguments.clone(),
            result_hash: None,
            turn: self.current_turn,
        };
        self.observations.push(observation);

        // Check for identical calls
        let identical_count = self.count_identical(tool_name, &canonical_arguments);
        if identical_count >= self.max_identical_calls {
            let msg = format!(
                "[Guardrail] Tool '{}' has been called with the same arguments {} times this turn. \
                 Please try a different approach to avoid an infinite loop.",
                tool_name, identical_count
            );
            warn!("{}", msg);
            return GuardrailDecision::SyntheticResult(msg);
        }

        // Check turn limits
        let total_calls = self.observations.len();
        if total_calls > self.hard_limit_per_turn {
            let msg = format!(
                "HALT: Hard limit of {} tool calls per turn reached.",
                self.hard_limit_per_turn
            );
            warn!("{}", msg);
            return GuardrailDecision::Halt(msg);
        }

        if total_calls > self.max_calls_per_turn {
            let msg = format!(
                "Warning: Soft limit of {} tool calls per turn exceeded ({} calls so far). \
                 Consider if the task can be simplified or if there's a loop.",
                self.max_calls_per_turn, total_calls
            );
            warn!("{}", msg);
            return GuardrailDecision::Warn(msg);
        }

        GuardrailDecision::Allow
    }

    /// Record the result of a tool call to detect stalled output loops.
    pub fn record_result(&mut self, tool_name: &str, result: &str) -> GuardrailDecision {
        if !is_idempotent_tool(tool_name) {
            return GuardrailDecision::Allow;
        }

        let hash = hash_text(&canonical_json(result));

        // Update the most recent observation with the result hash
        if let Some(obs) = self
            .observations
            .iter_mut()
            .rev()
            .find(|o| o.tool_name == tool_name && o.result_hash.is_none())
        {
            obs.result_hash = Some(hash.clone());
        }

        // Check if we've seen this exact output multiple times recently
        // with the same tool, even if arguments were slightly different.
        let stall_count = self
            .observations
            .iter()
            .rev()
            .take(5)
            .filter(|o| o.tool_name == tool_name && o.result_hash.as_ref() == Some(&hash))
            .count();

        if stall_count >= NO_PROGRESS_BLOCK_AFTER {
            let msg = format!(
                "STALL DETECTED: Tool '{}' has returned the same output {} times. \
                 The agent is likely stuck in an error loop or making no progress.",
                tool_name, stall_count
            );
            warn!("{}", msg);
            return GuardrailDecision::SyntheticResult(
                "[Guardrail] This tool is repeatedly returning the same output. \
                 Please stop trying this specific command or approach, as it is not producing new information.".to_string()
            );
        }

        if stall_count >= NO_PROGRESS_WARN_AFTER {
            let msg = format!(
                "[Guardrail] Tool '{}' returned the same result {} times. \
                 Use the result already provided or change the query instead of repeating it unchanged.",
                tool_name, stall_count
            );
            warn!("{}", msg);
            return GuardrailDecision::Warn(msg);
        }

        GuardrailDecision::Allow
    }

    /// Check if recent turns show a pattern of excessive tool calls.
    pub fn detect_turn_loop_pattern(&self) -> GuardrailDecision {
        if self.turn_history.len() < 3 {
            return GuardrailDecision::Allow;
        }

        // Check if the last 3 turns all had high tool call counts.
        let recent: Vec<usize> = self.turn_history.iter().rev().take(3).copied().collect();
        if recent.iter().all(|&count| count >= self.max_calls_per_turn) {
            let msg = format!(
                "HALT: Last {} turns all had {}+ tool calls, indicating a persistent loop pattern.",
                recent.len(),
                self.max_calls_per_turn
            );
            warn!("{}", msg);
            return GuardrailDecision::Halt(msg);
        }

        GuardrailDecision::Allow
    }

    /// Get the number of observations in the current turn.
    pub fn current_turn_calls(&self) -> usize {
        self.observations.len()
    }

    /// Get the total number of turns tracked.
    pub fn total_turns(&self) -> usize {
        self.current_turn
    }

    /// Count how many times the exact same tool+arguments combo was called this turn.
    fn count_identical(&self, tool_name: &str, arguments: &str) -> usize {
        self.observations
            .iter()
            .filter(|o| o.tool_name == tool_name && o.arguments == arguments)
            .count()
    }
}

fn canonical_json(value: &str) -> String {
    serde_json::from_str::<Value>(value)
        .ok()
        .and_then(|parsed| serde_json::to_string(&parsed).ok())
        .unwrap_or_else(|| value.to_string())
}

fn hash_text(value: &str) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    format!("{:x}", hasher.finish())
}

fn is_idempotent_tool(tool_name: &str) -> bool {
    if MUTATING_TOOL_NAMES.contains(&tool_name) {
        return false;
    }

    IDEMPOTENT_TOOL_NAMES.contains(&tool_name)
        || tool_name.starts_with("mcp_filesystem_read")
        || tool_name.starts_with("mcp_filesystem_list")
        || tool_name.starts_with("mcp_filesystem_get")
        || tool_name.starts_with("mcp_filesystem_search")
}

impl Default for ToolGuardrails {
    fn default() -> Self {
        Self::new()
    }
}

/// Idempotency tracker for detecting duplicate tool calls across turns.
pub struct IdempotencyTracker {
    /// Map from (tool_name, args_hash) to call count.
    calls: HashMap<(String, String), usize>,
}

impl IdempotencyTracker {
    /// Create a new tracker.
    pub fn new() -> Self {
        Self {
            calls: HashMap::new(),
        }
    }

    /// Record a call and return how many times this exact call has been made.
    pub fn record(&mut self, tool_name: &str, arguments: &str) -> usize {
        let key = (tool_name.to_string(), arguments.to_string());
        let count = self.calls.entry(key).or_insert(0);
        *count += 1;
        *count
    }

    /// Check if a call has been made before without recording it.
    pub fn has_seen(&self, tool_name: &str, arguments: &str) -> bool {
        let key = (tool_name.to_string(), arguments.to_string());
        self.calls.contains_key(&key)
    }

    /// Get the count for a specific call.
    pub fn count(&self, tool_name: &str, arguments: &str) -> usize {
        let key = (tool_name.to_string(), arguments.to_string());
        self.calls.get(&key).copied().unwrap_or(0)
    }

    /// Reset all tracking.
    pub fn clear(&mut self) {
        self.calls.clear();
    }

    /// Get the total number of unique calls tracked.
    pub fn unique_calls(&self) -> usize {
        self.calls.len()
    }
}

impl Default for IdempotencyTracker {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── ToolGuardrails ─────────────────────────────────────────────────

    #[test]
    fn test_guardrails_allow_normal_calls() {
        let mut g = ToolGuardrails::new();
        g.begin_turn();
        assert_eq!(
            g.record_call("read_file", r#"{"path":"/tmp/a"}"#),
            GuardrailDecision::Allow
        );
        assert_eq!(
            g.record_call("write_file", r#"{"path":"/tmp/b"}"#),
            GuardrailDecision::Allow
        );
    }

    #[test]
    fn test_guardrails_detect_identical_loop() {
        let mut g = ToolGuardrails::with_limits(3, 100, 200);
        g.begin_turn();
        let args = r#"{"path":"/tmp/test"}"#;
        assert_eq!(g.record_call("read_file", args), GuardrailDecision::Allow);
        assert_eq!(g.record_call("read_file", args), GuardrailDecision::Allow);
        // Third identical call should trigger synthetic result.
        match g.record_call("read_file", args) {
            GuardrailDecision::SyntheticResult(_) => {}
            other => panic!("Expected SyntheticResult, got {:?}", other),
        }
    }

    #[test]
    fn test_guardrails_warn_on_soft_limit() {
        let mut g = ToolGuardrails::with_limits(100, 5, 100);
        g.begin_turn();
        for i in 0..5 {
            assert_eq!(
                g.record_call(&format!("tool_{}", i), "{}"),
                GuardrailDecision::Allow
            );
        }
        // 6th call exceeds soft limit of 5.
        match g.record_call("tool_6", "{}") {
            GuardrailDecision::Warn(_) => {}
            other => panic!("Expected Warn, got {:?}", other),
        }
    }

    #[test]
    fn test_guardrails_halt_on_hard_limit() {
        let mut g = ToolGuardrails::with_limits(100, 100, 3);
        g.begin_turn();
        assert_eq!(g.record_call("t1", "{}"), GuardrailDecision::Allow);
        assert_eq!(g.record_call("t2", "{}"), GuardrailDecision::Allow);
        assert_eq!(g.record_call("t3", "{}"), GuardrailDecision::Allow);
        // 4th call exceeds hard limit of 3.
        match g.record_call("t4", "{}") {
            GuardrailDecision::Halt(_) => {}
            other => panic!("Expected Halt, got {:?}", other),
        }
    }

    #[test]
    fn test_guardrails_different_args_not_counted_as_identical() {
        let mut g = ToolGuardrails::with_limits(3, 100, 200);
        g.begin_turn();
        assert_eq!(
            g.record_call("read_file", r#"{"path":"/a"}"#),
            GuardrailDecision::Allow
        );
        assert_eq!(
            g.record_call("read_file", r#"{"path":"/b"}"#),
            GuardrailDecision::Allow
        );
        assert_eq!(
            g.record_call("read_file", r#"{"path":"/c"}"#),
            GuardrailDecision::Allow
        );
        // Different args means not identical.
        assert_eq!(
            g.record_call("read_file", r#"{"path":"/d"}"#),
            GuardrailDecision::Allow
        );
    }

    #[test]
    fn test_guardrails_turn_loop_pattern() {
        let mut g = ToolGuardrails::with_limits(100, 5, 100);
        // Simulate 3 turns with high call counts.
        for _ in 0..3 {
            g.begin_turn();
            for i in 0..6 {
                g.record_call(&format!("tool_{}", i), "{}");
            }
        }
        g.begin_turn(); // This pushes the last turn's count to history.
        match g.detect_turn_loop_pattern() {
            GuardrailDecision::Halt(_) => {}
            other => panic!("Expected Halt for turn loop pattern, got {:?}", other),
        }
    }

    #[test]
    fn test_guardrails_no_pattern_with_few_turns() {
        let mut g = ToolGuardrails::new();
        g.begin_turn();
        g.record_call("tool", "{}");
        assert_eq!(g.detect_turn_loop_pattern(), GuardrailDecision::Allow);
    }

    // ── IdempotencyTracker ─────────────────────────────────────────────

    #[test]
    fn test_idempotency_tracks_counts() {
        let mut tracker = IdempotencyTracker::new();
        assert_eq!(tracker.record("read_file", "{}"), 1);
        assert_eq!(tracker.record("read_file", "{}"), 2);
        assert_eq!(tracker.record("read_file", "{}"), 3);
        assert_eq!(tracker.count("read_file", "{}"), 3);
    }

    #[test]
    fn test_idempotency_different_args() {
        let mut tracker = IdempotencyTracker::new();
        tracker.record("read_file", r#"{"path":"/a"}"#);
        tracker.record("read_file", r#"{"path":"/b"}"#);
        assert_eq!(tracker.count("read_file", r#"{"path":"/a"}"#), 1);
        assert_eq!(tracker.unique_calls(), 2);
    }

    #[test]
    fn test_idempotency_has_seen() {
        let mut tracker = IdempotencyTracker::new();
        assert!(!tracker.has_seen("tool", "{}"));
        tracker.record("tool", "{}");
        assert!(tracker.has_seen("tool", "{}"));
    }

    #[test]
    fn test_idempotency_clear() {
        let mut tracker = IdempotencyTracker::new();
        tracker.record("tool", "{}");
        tracker.clear();
        assert!(!tracker.has_seen("tool", "{}"));
        assert_eq!(tracker.unique_calls(), 0);
    }

    #[test]
    fn test_guardrails_current_turn_calls() {
        let mut g = ToolGuardrails::new();
        g.begin_turn();
        assert_eq!(g.current_turn_calls(), 0);
        g.record_call("a", "{}");
        g.record_call("b", "{}");
        assert_eq!(g.current_turn_calls(), 2);
    }

    #[test]
    fn test_guardrails_canonicalizes_json_arguments() {
        let mut g = ToolGuardrails::with_limits(2, 100, 200);
        g.begin_turn();
        assert_eq!(
            g.record_call("read_file", r#"{"b":2,"a":1}"#),
            GuardrailDecision::Allow
        );
        match g.record_call("read_file", r#"{"a":1,"b":2}"#) {
            GuardrailDecision::SyntheticResult(_) => {}
            other => panic!("Expected SyntheticResult, got {:?}", other),
        }
    }

    #[test]
    fn test_guardrails_warn_on_repeated_idempotent_result() {
        let mut g = ToolGuardrails::new();
        g.begin_turn();
        g.record_call("read_file", r#"{"path":"/tmp/a"}"#);
        assert_eq!(
            g.record_result("read_file", "same output"),
            GuardrailDecision::Allow
        );
        g.record_call("read_file", r#"{"path":"/tmp/b"}"#);
        match g.record_result("read_file", "same output") {
            GuardrailDecision::Warn(_) => {}
            other => panic!("Expected Warn, got {:?}", other),
        }
    }

    #[test]
    fn test_guardrails_ignore_mutating_repeated_results() {
        let mut g = ToolGuardrails::new();
        g.begin_turn();
        for i in 0..NO_PROGRESS_BLOCK_AFTER {
            g.record_call("write_file", &format!(r#"{{"path":"/tmp/{i}"}}"#));
            assert_eq!(
                g.record_result("write_file", "ok"),
                GuardrailDecision::Allow
            );
        }
    }
}
