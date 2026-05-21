use serde::{Deserialize, Serialize};

/// Token usage statistics for an API call.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Usage {
    /// Number of tokens in the prompt.
    #[serde(default)]
    pub prompt_tokens: u32,

    /// Number of tokens in the completion.
    #[serde(default)]
    pub completion_tokens: u32,

    /// Total tokens used (prompt + completion).
    #[serde(default)]
    pub total_tokens: u32,

    /// Number of prompt tokens that were served from cache.
    #[serde(default)]
    pub cached_tokens: u32,

    /// Number of tokens used for reasoning/thinking.
    #[serde(default)]
    pub reasoning_tokens: u32,
}

impl Usage {
    /// Create a zeroed-out usage.
    pub fn zero() -> Self {
        Self::default()
    }

    /// Accumulate another usage into this one.
    pub fn accumulate(&mut self, other: &Usage) {
        self.prompt_tokens += other.prompt_tokens;
        self.completion_tokens += other.completion_tokens;
        self.total_tokens += other.total_tokens;
        self.cached_tokens += other.cached_tokens;
        self.reasoning_tokens += other.reasoning_tokens;
    }
}
