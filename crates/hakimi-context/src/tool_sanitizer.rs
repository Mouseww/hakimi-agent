use std::collections::{HashMap, HashSet};

use hakimi_common::{Message, MessageRole};

/// Tool call pairing sanitizer.
///
/// Validates and repairs tool-call/tool-result message pairing in conversation history.
/// Implements grok-build-inspired patterns:
/// - Deduplicate repeated tool results
/// - Strip isolated tool results (no preceding assistant tool_call)
/// - Optionally synthesize placeholder tool results for dangling tool_calls
///
/// Configuration for the tool call/result pairing and validation logic.
#[derive(Default)]
pub struct ToolSanitizer {
    /// When true, synthesize placeholder results for dangling tool calls.
    pub synthesize_placeholders: bool,
    /// When true, drop unmatched tool results instead of returning an error.
    pub drop_unmatched_results: bool,
}
impl ToolSanitizer {
    /// Create a new tool sanitizer.
    pub fn new() -> Self {
        Self::default()
    }

    /// Enable or disable placeholder synthesis for dangling tool calls.
    pub fn with_placeholder_synthesis(mut self, enabled: bool) -> Self {
        self.synthesize_placeholders = enabled;
        self
    }

    /// Sanitize a conversation history, repairing tool-call/result pairing issues.
    ///
    /// Returns a sanitized copy of the conversation and a summary of actions taken.
    pub fn sanitize(&self, messages: &[Message]) -> (Vec<Message>, SanitizationReport) {
        let mut report = SanitizationReport::default();
        let mut sanitized = Vec::with_capacity(messages.len());
        let mut active_tool_calls: HashMap<String, String> = HashMap::new(); // id -> name
        let mut seen_tool_results: HashSet<String> = HashSet::new();

        for msg in messages {
            match msg.role {
                MessageRole::Assistant => {
                    // Register tool calls from this assistant message
                    if let Some(tool_calls) = &msg.tool_calls {
                        for tc in tool_calls {
                            active_tool_calls.insert(tc.id.clone(), tc.name.clone());
                        }
                    }
                    sanitized.push(msg.clone());
                }

                MessageRole::Tool => {
                    // Validate tool result pairing
                    if let Some(tool_call_id) = &msg.tool_call_id {
                        // Check for duplicate tool result first (before checking pairing)
                        if seen_tool_results.contains(tool_call_id) {
                            // Duplicate tool result: same tool_call_id already responded to
                            report.duplicate_results_deduped += 1;
                            continue; // Skip this message
                        }

                        // Check if this tool_call_id has a preceding assistant tool_call
                        if !active_tool_calls.contains_key(tool_call_id) {
                            // Isolated tool result: no matching tool_call
                            report.isolated_results_stripped += 1;
                            continue; // Skip this message
                        }

                        // Valid tool result: mark as seen and keep
                        seen_tool_results.insert(tool_call_id.clone());
                        active_tool_calls.remove(tool_call_id); // Consume the tool call
                        sanitized.push(msg.clone());
                    } else {
                        // Tool message without tool_call_id: malformed, strip it
                        report.malformed_tool_messages_stripped += 1;
                    }
                }

                _ => {
                    // System, User: pass through unchanged
                    sanitized.push(msg.clone());
                }
            }
        }

        // Handle dangling tool calls (assistant requested tools but no result)
        if !active_tool_calls.is_empty() {
            report.dangling_tool_calls = active_tool_calls.len();

            if self.synthesize_placeholders {
                // Synthesize placeholder tool results for dangling tool calls
                for (tool_call_id, tool_name) in active_tool_calls {
                    let placeholder = Message::tool_result(
                        tool_call_id.clone(),
                        tool_name.clone(),
                        "[Tool result missing — placeholder synthesized by sanitizer]",
                    );
                    sanitized.push(placeholder);
                    report.placeholders_synthesized += 1;
                }
            }
        }

        (sanitized, report)
    }

    /// Validate a conversation history without modifying it.
    ///
    /// Returns `Ok(())` if the history is valid, or `Err(report)` with validation issues.
    pub fn validate(&self, messages: &[Message]) -> Result<(), ValidationReport> {
        let mut report = ValidationReport::default();
        let mut active_tool_calls: HashMap<String, String> = HashMap::new();
        let mut seen_tool_results: HashSet<String> = HashSet::new();

        for (idx, msg) in messages.iter().enumerate() {
            match msg.role {
                MessageRole::Assistant => {
                    if let Some(tool_calls) = &msg.tool_calls {
                        for tc in tool_calls {
                            if active_tool_calls.contains_key(&tc.id) {
                                report.issues.push(ValidationIssue::DuplicateToolCallId {
                                    message_index: idx,
                                    tool_call_id: tc.id.clone(),
                                });
                            }
                            active_tool_calls.insert(tc.id.clone(), tc.name.clone());
                        }
                    }
                }

                MessageRole::Tool => {
                    if let Some(tool_call_id) = &msg.tool_call_id {
                        if !active_tool_calls.contains_key(tool_call_id) {
                            report.issues.push(ValidationIssue::IsolatedToolResult {
                                message_index: idx,
                                tool_call_id: tool_call_id.clone(),
                            });
                        }

                        if seen_tool_results.contains(tool_call_id) {
                            report.issues.push(ValidationIssue::DuplicateToolResult {
                                message_index: idx,
                                tool_call_id: tool_call_id.clone(),
                            });
                        }

                        seen_tool_results.insert(tool_call_id.clone());
                        active_tool_calls.remove(tool_call_id);
                    } else {
                        report
                            .issues
                            .push(ValidationIssue::MalformedToolMessage { message_index: idx });
                    }
                }

                _ => {}
            }
        }

        // Check for dangling tool calls
        for (tool_call_id, tool_name) in active_tool_calls {
            report.issues.push(ValidationIssue::DanglingToolCall {
                tool_call_id,
                tool_name,
            });
        }

        if report.issues.is_empty() {
            Ok(())
        } else {
            Err(report)
        }
    }
}

/// Summary of sanitization actions taken.
#[derive(Debug, Clone, Default)]
pub struct SanitizationReport {
    /// Number of isolated tool results stripped (no matching tool_call).
    pub isolated_results_stripped: usize,

    /// Number of duplicate tool results deduplicated.
    pub duplicate_results_deduped: usize,

    /// Number of malformed tool messages stripped (missing tool_call_id).
    pub malformed_tool_messages_stripped: usize,

    /// Number of dangling tool calls detected (assistant requested tools but no result).
    pub dangling_tool_calls: usize,

    /// Number of placeholder tool results synthesized for dangling tool calls.
    pub placeholders_synthesized: usize,
}

impl SanitizationReport {
    /// Returns `true` if any sanitization actions were taken.
    pub fn has_issues(&self) -> bool {
        self.isolated_results_stripped > 0
            || self.duplicate_results_deduped > 0
            || self.malformed_tool_messages_stripped > 0
            || self.dangling_tool_calls > 0
    }

    /// Returns a human-readable summary of sanitization actions.
    pub fn summary(&self) -> String {
        if !self.has_issues() {
            return "No tool pairing issues detected.".to_string();
        }

        let mut parts = Vec::new();
        if self.isolated_results_stripped > 0 {
            parts.push(format!(
                "{} isolated tool result(s) stripped",
                self.isolated_results_stripped
            ));
        }
        if self.duplicate_results_deduped > 0 {
            parts.push(format!(
                "{} duplicate tool result(s) deduplicated",
                self.duplicate_results_deduped
            ));
        }
        if self.malformed_tool_messages_stripped > 0 {
            parts.push(format!(
                "{} malformed tool message(s) stripped",
                self.malformed_tool_messages_stripped
            ));
        }
        if self.dangling_tool_calls > 0 {
            parts.push(format!(
                "{} dangling tool call(s) detected",
                self.dangling_tool_calls
            ));
        }
        if self.placeholders_synthesized > 0 {
            parts.push(format!(
                "{} placeholder(s) synthesized",
                self.placeholders_synthesized
            ));
        }

        parts.join(", ")
    }
}

/// Validation report for a conversation history.
#[derive(Debug, Clone, Default)]
pub struct ValidationReport {
    /// List of validation issues found.
    pub issues: Vec<ValidationIssue>,
}

impl ValidationReport {
    /// Returns a human-readable summary of validation issues.
    pub fn summary(&self) -> String {
        if self.issues.is_empty() {
            return "Conversation history is valid.".to_string();
        }

        format!("Found {} validation issue(s)", self.issues.len())
    }
}

/// A single validation issue in a conversation history.
#[derive(Debug, Clone)]
pub enum ValidationIssue {
    /// Tool result message has no matching assistant tool_call.
    IsolatedToolResult {
        message_index: usize,
        tool_call_id: String,
    },

    /// Multiple tool results for the same tool_call_id.
    DuplicateToolResult {
        message_index: usize,
        tool_call_id: String,
    },

    /// Tool message is missing a tool_call_id field.
    MalformedToolMessage { message_index: usize },

    /// Assistant requested a tool but no result was provided.
    DanglingToolCall {
        tool_call_id: String,
        tool_name: String,
    },

    /// Multiple assistant messages use the same tool_call_id.
    DuplicateToolCallId {
        message_index: usize,
        tool_call_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use hakimi_common::ToolCall;

    #[test]
    fn test_valid_conversation() {
        let messages = vec![
            Message::user("Calculate 2+2"),
            Message::assistant("").with_tool_calls(vec![ToolCall {
                id: "call_1".to_string(),
                name: "calculator".to_string(),
                arguments: r#"{"expression":"2+2"}"#.to_string(),
                index: Some(0),
            }]),
            Message::tool_result("call_1".to_string(), "calculator".to_string(), "4"),
            Message::assistant("The result is 4"),
        ];

        let sanitizer = ToolSanitizer::new();
        let (sanitized, report) = sanitizer.sanitize(&messages);

        assert_eq!(sanitized.len(), 4);
        assert!(!report.has_issues());
        assert!(sanitizer.validate(&messages).is_ok());
    }

    #[test]
    fn test_isolated_tool_result() {
        let messages = vec![
            Message::user("Hello"),
            Message::tool_result(
                "call_orphan".to_string(),
                "unknown".to_string(),
                "orphan result",
            ),
            Message::assistant("Hi there"),
        ];

        let sanitizer = ToolSanitizer::new();
        let (sanitized, report) = sanitizer.sanitize(&messages);

        assert_eq!(sanitized.len(), 2); // User + Assistant, tool stripped
        assert_eq!(report.isolated_results_stripped, 1);
        assert!(report.has_issues());
    }

    #[test]
    fn test_duplicate_tool_result() {
        let messages = vec![
            Message::assistant("").with_tool_calls(vec![ToolCall {
                id: "call_1".to_string(),
                name: "tool".to_string(),
                arguments: "{}".to_string(),
                index: Some(0),
            }]),
            Message::tool_result("call_1".to_string(), "tool".to_string(), "result 1"),
            Message::tool_result("call_1".to_string(), "tool".to_string(), "result 2"),
        ];

        let sanitizer = ToolSanitizer::new();
        let (sanitized, report) = sanitizer.sanitize(&messages);

        assert_eq!(sanitized.len(), 2); // Assistant + first tool result
        assert_eq!(report.duplicate_results_deduped, 1);
    }

    #[test]
    fn test_dangling_tool_call_without_synthesis() {
        let messages = vec![
            Message::assistant("").with_tool_calls(vec![ToolCall {
                id: "call_1".to_string(),
                name: "tool".to_string(),
                arguments: "{}".to_string(),
                index: Some(0),
            }]),
            Message::user("Never mind"),
        ];

        let sanitizer = ToolSanitizer::new();
        let (sanitized, report) = sanitizer.sanitize(&messages);

        assert_eq!(sanitized.len(), 2); // No placeholder synthesized
        assert_eq!(report.dangling_tool_calls, 1);
        assert_eq!(report.placeholders_synthesized, 0);
    }

    #[test]
    fn test_dangling_tool_call_with_synthesis() {
        let messages = vec![
            Message::assistant("").with_tool_calls(vec![ToolCall {
                id: "call_1".to_string(),
                name: "tool".to_string(),
                arguments: "{}".to_string(),
                index: Some(0),
            }]),
            Message::user("Never mind"),
        ];

        let sanitizer = ToolSanitizer::new().with_placeholder_synthesis(true);
        let (sanitized, report) = sanitizer.sanitize(&messages);

        assert_eq!(sanitized.len(), 3); // Assistant + User + synthesized tool result
        assert_eq!(report.dangling_tool_calls, 1);
        assert_eq!(report.placeholders_synthesized, 1);

        // Verify the placeholder was inserted
        let last = &sanitized[2];
        assert_eq!(last.role, MessageRole::Tool);
        assert_eq!(last.tool_call_id.as_ref().unwrap(), "call_1");
        assert!(last.content.as_ref().unwrap().contains("placeholder"));
    }

    #[test]
    fn test_malformed_tool_message() {
        let mut tool_msg = Message::tool_result("call_1".to_string(), "tool".to_string(), "result");
        tool_msg.tool_call_id = None; // Malformed: missing tool_call_id

        let messages = vec![Message::user("Hello"), tool_msg, Message::assistant("Hi")];

        let sanitizer = ToolSanitizer::new();
        let (sanitized, report) = sanitizer.sanitize(&messages);

        assert_eq!(sanitized.len(), 2); // User + Assistant, malformed tool stripped
        assert_eq!(report.malformed_tool_messages_stripped, 1);
    }

    #[test]
    fn test_validation_fails_on_issues() {
        let messages = vec![
            Message::user("Hello"),
            Message::tool_result("call_orphan".to_string(), "tool".to_string(), "orphan"),
        ];

        let sanitizer = ToolSanitizer::new();
        let validation = sanitizer.validate(&messages);

        assert!(validation.is_err());
        let report = validation.unwrap_err();
        assert_eq!(report.issues.len(), 1);
        assert!(matches!(
            report.issues[0],
            ValidationIssue::IsolatedToolResult { .. }
        ));
    }
}
