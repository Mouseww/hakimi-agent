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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_usage_default_is_zero() {
        let usage = Usage::default();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
        assert_eq!(usage.cached_tokens, 0);
        assert_eq!(usage.reasoning_tokens, 0);
    }

    #[test]
    fn test_usage_zero_method() {
        let usage = Usage::zero();
        assert_eq!(usage.prompt_tokens, 0);
        assert_eq!(usage.completion_tokens, 0);
        assert_eq!(usage.total_tokens, 0);
        assert_eq!(usage.cached_tokens, 0);
        assert_eq!(usage.reasoning_tokens, 0);
    }

    #[test]
    fn test_usage_accumulate_basic() {
        let mut a = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            cached_tokens: 5,
            reasoning_tokens: 3,
        };
        let b = Usage {
            prompt_tokens: 100,
            completion_tokens: 200,
            total_tokens: 300,
            cached_tokens: 50,
            reasoning_tokens: 15,
        };
        a.accumulate(&b);
        assert_eq!(a.prompt_tokens, 110);
        assert_eq!(a.completion_tokens, 220);
        assert_eq!(a.total_tokens, 330);
        assert_eq!(a.cached_tokens, 55);
        assert_eq!(a.reasoning_tokens, 18);
    }

    #[test]
    fn test_usage_accumulate_multiple() {
        let mut acc = Usage::zero();
        let chunk = Usage {
            prompt_tokens: 10,
            completion_tokens: 20,
            total_tokens: 30,
            cached_tokens: 5,
            reasoning_tokens: 2,
        };
        acc.accumulate(&chunk);
        acc.accumulate(&chunk);
        acc.accumulate(&chunk);
        assert_eq!(acc.prompt_tokens, 30);
        assert_eq!(acc.completion_tokens, 60);
        assert_eq!(acc.total_tokens, 90);
        assert_eq!(acc.cached_tokens, 15);
        assert_eq!(acc.reasoning_tokens, 6);
    }

    #[test]
    fn test_usage_serialization_roundtrip() {
        let usage = Usage {
            prompt_tokens: 42,
            completion_tokens: 84,
            total_tokens: 126,
            cached_tokens: 7,
            reasoning_tokens: 10,
        };
        let json = serde_json::to_string(&usage).expect("serialize");
        let deserialized: Usage = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(deserialized.prompt_tokens, usage.prompt_tokens);
        assert_eq!(deserialized.completion_tokens, usage.completion_tokens);
        assert_eq!(deserialized.total_tokens, usage.total_tokens);
        assert_eq!(deserialized.cached_tokens, usage.cached_tokens);
        assert_eq!(deserialized.reasoning_tokens, usage.reasoning_tokens);
    }
}
