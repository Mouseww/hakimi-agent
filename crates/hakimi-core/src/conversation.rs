use hakimi_common::{Message, Usage};

use crate::metrics::ConversationMetrics;

/// The result of a full conversation loop.
#[derive(Debug, Clone, Default)]
pub struct ConversationResult {
    /// The final text response from the assistant (empty if interrupted or budget exhausted).
    pub final_response: String,
    /// All messages in the conversation, including system, user, assistant, and tool messages.
    pub messages: Vec<Message>,
    /// Accumulated token usage across all API calls in this conversation.
    pub usage: Usage,
    /// Number of API calls made during the conversation.
    pub api_call_count: usize,
    /// Performance metrics for this conversation.
    pub metrics: ConversationMetrics,
}
