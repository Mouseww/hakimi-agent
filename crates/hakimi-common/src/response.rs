use serde::{Deserialize, Serialize};

use crate::tool::ToolCall;
use crate::usage::Usage;

/// Reason the model stopped generating.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FinishReason {
    /// The model naturally stopped.
    Stop,
    /// The model invoked one or more tools.
    ToolCalls,
    /// The response hit the max token limit.
    Length,
    /// Content was filtered by the provider.
    ContentFilter,
    /// An error occurred during generation.
    Error,
}

/// A normalized response from any LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedResponse {
    /// Text content of the response, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,

    /// Tool calls requested by the model, if any.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool_calls: Option<Vec<ToolCall>>,

    /// Why the model stopped generating.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub finish_reason: Option<FinishReason>,

    /// Token usage for this response.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub usage: Option<Usage>,

    /// Chain-of-thought reasoning text, if provided by the model.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<String>,
}

impl NormalizedResponse {
    /// Returns `true` if this response contains tool calls.
    pub fn has_tool_calls(&self) -> bool {
        self.tool_calls
            .as_ref()
            .map_or(false, |calls| !calls.is_empty())
    }

    /// Returns the text content or an empty string.
    pub fn content_or_empty(&self) -> &str {
        self.content.as_deref().unwrap_or("")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_has_tool_calls_with_calls() {
        let resp = NormalizedResponse {
            content: Some("text".into()),
            tool_calls: Some(vec![ToolCall {
                id: "call_1".into(),
                name: "search".into(),
                arguments: "{}".into(),
                index: Some(0),
            }]),
            finish_reason: Some(FinishReason::ToolCalls),
            usage: None,
            reasoning: None,
        };
        assert!(resp.has_tool_calls());
    }

    #[test]
    fn test_has_tool_calls_empty_vec() {
        let resp = NormalizedResponse {
            content: None,
            tool_calls: Some(vec![]),
            finish_reason: None,
            usage: None,
            reasoning: None,
        };
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn test_has_tool_calls_none() {
        let resp = NormalizedResponse {
            content: None,
            tool_calls: None,
            finish_reason: None,
            usage: None,
            reasoning: None,
        };
        assert!(!resp.has_tool_calls());
    }

    #[test]
    fn test_content_or_empty_some() {
        let resp = NormalizedResponse {
            content: Some("hello world".into()),
            tool_calls: None,
            finish_reason: None,
            usage: None,
            reasoning: None,
        };
        assert_eq!(resp.content_or_empty(), "hello world");
    }

    #[test]
    fn test_content_or_empty_none() {
        let resp = NormalizedResponse {
            content: None,
            tool_calls: None,
            finish_reason: None,
            usage: None,
            reasoning: None,
        };
        assert_eq!(resp.content_or_empty(), "");
    }

    #[test]
    fn test_finish_reason_equality() {
        assert_eq!(FinishReason::Stop, FinishReason::Stop);
        assert_eq!(FinishReason::ToolCalls, FinishReason::ToolCalls);
        assert_eq!(FinishReason::Length, FinishReason::Length);
        assert_ne!(FinishReason::Stop, FinishReason::Length);
        assert_ne!(FinishReason::ToolCalls, FinishReason::Error);
        assert_ne!(FinishReason::ContentFilter, FinishReason::Error);
    }

    #[test]
    fn test_normalized_response_serialization() {
        let resp = NormalizedResponse {
            content: Some("hi".into()),
            tool_calls: None,
            finish_reason: Some(FinishReason::Stop),
            usage: Some(Usage::default()),
            reasoning: Some("thinking".into()),
        };
        let json = serde_json::to_string(&resp).expect("serialize");
        let deserialized: NormalizedResponse = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.content, Some("hi".into()));
        assert!(deserialized.tool_calls.is_none());
        assert_eq!(deserialized.finish_reason, Some(FinishReason::Stop));
        assert!(deserialized.usage.is_some());
        assert_eq!(deserialized.reasoning, Some("thinking".into()));
    }
}
