use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fmt;

use crate::tool::ToolCall;

/// Role of a message in a conversation.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MessageRole {
    System,
    User,
    Assistant,
    Tool,
}

impl fmt::Display for MessageRole {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MessageRole::System => write!(f, "system"),
            MessageRole::User => write!(f, "user"),
            MessageRole::Assistant => write!(f, "assistant"),
            MessageRole::Tool => write!(f, "tool"),
        }
    }
}

/// A single message in OpenAI-compatible format.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Role of the message sender.
    pub role: MessageRole,

    /// Text content of the message. May be `None` when tool_calls are present.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Tool calls requested by the assistant.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    /// ID of the tool call this message is a response to (for `tool` role messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,

    /// Name of the function/tool (for `tool` role messages).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,

    /// Chain-of-thought reasoning text (provider-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,

    /// Alternative reasoning content field (provider-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning_content: Option<String>,

    /// Timestamp when the message was created.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timestamp: Option<DateTime<Utc>>,

    /// Token count for this message, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_count: Option<u32>,

    /// Finish reason from the provider for this message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<String>,
}

impl Message {
    /// Create a new system message.
    pub fn system(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::System,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: Some(Utc::now()),
            token_count: None,
            finish_reason: None,
        }
    }

    /// Create a new user message.
    pub fn user(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::User,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: Some(Utc::now()),
            token_count: None,
            finish_reason: None,
        }
    }

    /// Create a new assistant message.
    pub fn assistant(content: impl Into<String>) -> Self {
        Self {
            role: MessageRole::Assistant,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: Some(Utc::now()),
            token_count: None,
            finish_reason: None,
        }
    }

    /// Create a new tool-result message.
    pub fn tool_result(
        tool_call_id: impl Into<String>,
        name: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            role: MessageRole::Tool,
            content: Some(content.into()),
            tool_calls: None,
            tool_call_id: Some(tool_call_id.into()),
            name: Some(name.into()),
            reasoning: None,
            reasoning_content: None,
            timestamp: Some(Utc::now()),
            token_count: None,
            finish_reason: None,
        }
    }
}

impl fmt::Display for Message {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let role = &self.role;
        let content_preview = self.content.as_deref().unwrap_or("(no content)");
        let truncated: String = content_preview.chars().take(120).collect();
        if content_preview.len() > 120 {
            write!(f, "[{role}] {truncated}…")
        } else {
            write!(f, "[{role}] {truncated}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Constructor tests ────────────────────────────────────────────────

    #[test]
    fn test_message_system_constructor() {
        let msg = Message::system("You are a helpful assistant.");
        assert_eq!(msg.role, MessageRole::System);
        assert_eq!(msg.content.as_deref(), Some("You are a helpful assistant."));
        assert!(
            msg.timestamp.is_some(),
            "system messages should have a timestamp"
        );
        assert!(msg.tool_calls.is_none());
        assert!(msg.tool_call_id.is_none());
        assert!(msg.name.is_none());
        assert!(msg.reasoning.is_none());
        assert!(msg.reasoning_content.is_none());
        assert!(msg.token_count.is_none());
        assert!(msg.finish_reason.is_none());
    }

    #[test]
    fn test_message_user_constructor() {
        let msg = Message::user("Hello, world!");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content.as_deref(), Some("Hello, world!"));
        assert!(msg.timestamp.is_some());
        assert!(msg.tool_calls.is_none());
        assert!(msg.tool_call_id.is_none());
        assert!(msg.name.is_none());
    }

    #[test]
    fn test_message_assistant_constructor() {
        let msg = Message::assistant("I can help with that.");
        assert_eq!(msg.role, MessageRole::Assistant);
        assert_eq!(msg.content.as_deref(), Some("I can help with that."));
        assert!(msg.timestamp.is_some());
        assert!(msg.tool_calls.is_none());
        assert!(msg.tool_call_id.is_none());
        assert!(msg.name.is_none());
    }

    #[test]
    fn test_message_tool_result_constructor() {
        let msg = Message::tool_result("call_123", "get_weather", "{\"temp\":72}");
        assert_eq!(msg.role, MessageRole::Tool);
        assert_eq!(msg.content.as_deref(), Some("{\"temp\":72}"));
        assert_eq!(msg.tool_call_id.as_deref(), Some("call_123"));
        assert_eq!(msg.name.as_deref(), Some("get_weather"));
        assert!(msg.timestamp.is_some());
        assert!(msg.tool_calls.is_none());
    }

    // ── Display tests ────────────────────────────────────────────────────

    #[test]
    fn test_message_display_short() {
        let msg = Message::user("short message");
        let display = format!("{msg}");
        assert_eq!(display, "[user] short message");
        // Should not contain the ellipsis character
        assert!(!display.contains('…'));
    }

    #[test]
    fn test_message_display_long() {
        let long_content: String = "a".repeat(200);
        let msg = Message::user(&long_content);
        let display = format!("{msg}");
        // Should be truncated with ellipsis
        assert!(display.starts_with("[user] "));
        assert!(display.ends_with('…'));
        // The visible content portion should be at most 120 chars
        let content_part = &display["[user] ".len()..display.len() - '…'.len_utf8()];
        assert_eq!(content_part.chars().count(), 120);
    }

    #[test]
    fn test_message_display_no_content() {
        let msg = Message {
            role: MessageRole::Assistant,
            content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: None,
            token_count: None,
            finish_reason: None,
        };
        let display = format!("{msg}");
        assert_eq!(display, "[assistant] (no content)");
    }

    // ── MessageRole Display tests ────────────────────────────────────────

    #[test]
    fn test_message_role_display() {
        assert_eq!(format!("{}", MessageRole::System), "system");
        assert_eq!(format!("{}", MessageRole::User), "user");
        assert_eq!(format!("{}", MessageRole::Assistant), "assistant");
        assert_eq!(format!("{}", MessageRole::Tool), "tool");
    }

    // ── Serialization roundtrip tests ────────────────────────────────────

    #[test]
    fn test_message_serialization_roundtrip() {
        let messages = vec![
            Message::system("You are helpful."),
            Message::user("Hi there"),
            Message::assistant("Hello!"),
            Message::tool_result("tc_1", "search", "results here"),
        ];

        for msg in &messages {
            let json = serde_json::to_string(msg).expect("serialization should succeed");
            let deserialized: Message =
                serde_json::from_str(&json).expect("deserialization should succeed");
            assert_eq!(deserialized.role, msg.role);
            assert_eq!(deserialized.content, msg.content);
            assert_eq!(deserialized.tool_call_id, msg.tool_call_id);
            assert_eq!(deserialized.name, msg.name);
            // Timestamps survive the roundtrip (compared to the millisecond)
            assert_eq!(deserialized.timestamp, msg.timestamp);
        }
    }

    #[test]
    fn test_message_no_content() {
        let msg = Message {
            role: MessageRole::Assistant,
            content: None,
            tool_calls: None,
            tool_call_id: None,
            name: None,
            reasoning: None,
            reasoning_content: None,
            timestamp: None,
            token_count: None,
            finish_reason: None,
        };

        let json = serde_json::to_string(&msg).expect("serialization should succeed");
        // content should be skipped (skip_serializing_if = "Option::is_none")
        assert!(
            !json.contains("\"content\""),
            "None content should not appear in JSON: {json}"
        );

        let deserialized: Message =
            serde_json::from_str(&json).expect("deserialization should succeed");
        assert_eq!(deserialized.role, MessageRole::Assistant);
        assert!(deserialized.content.is_none());
    }
}
