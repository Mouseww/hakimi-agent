use hakimi_common::{Message, Usage};

/// The result of a full conversation loop.
#[derive(Debug, Clone)]
pub struct ConversationResult {
    /// The final text response from the assistant (empty if interrupted or budget exhausted).
    pub final_response: String,
    /// All messages in the conversation, including system, user, assistant, and tool messages.
    pub messages: Vec<Message>,
    /// Accumulated token usage across all API calls in this conversation.
    pub usage: Usage,
    /// Number of API calls made during the conversation.
    pub api_call_count: usize,
}

impl Default for ConversationResult {
    fn default() -> Self {
        Self {
            final_response: String::new(),
            messages: Vec::new(),
            usage: Usage::default(),
            api_call_count: 0,
        }
    }
}
