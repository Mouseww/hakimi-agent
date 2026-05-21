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
