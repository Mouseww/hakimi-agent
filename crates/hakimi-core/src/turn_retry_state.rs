/// Per-turn recovery bookkeeping for one-shot retry guards and restart signals.
///
/// Loop mechanics such as retry counters and max-attempt limits intentionally
/// stay in the caller. This state only owns recovery flags that should have a
/// single named home instead of being scattered through the conversation loop.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct TurnRetryState {
    pub codex_auth_retry_attempted: bool,
    pub anthropic_auth_retry_attempted: bool,
    pub nous_auth_retry_attempted: bool,
    pub nous_paid_entitlement_refresh_attempted: bool,
    pub copilot_auth_retry_attempted: bool,
    pub thinking_sig_retry_attempted: bool,
    pub invalid_encrypted_content_retry_attempted: bool,
    pub image_shrink_retry_attempted: bool,
    pub multimodal_tool_content_retry_attempted: bool,
    pub oauth_1m_beta_retry_attempted: bool,
    pub llama_cpp_grammar_retry_attempted: bool,
    pub primary_recovery_attempted: bool,
    pub has_retried_429: bool,
    pub restart_with_compressed_messages: bool,
    pub restart_with_length_continuation: bool,
    output_token_budget_adjustments: u32,
}

impl TurnRetryState {
    pub const HERMES_FIELD_NAMES: [&'static str; 15] = [
        "codex_auth_retry_attempted",
        "anthropic_auth_retry_attempted",
        "nous_auth_retry_attempted",
        "nous_paid_entitlement_refresh_attempted",
        "copilot_auth_retry_attempted",
        "thinking_sig_retry_attempted",
        "invalid_encrypted_content_retry_attempted",
        "image_shrink_retry_attempted",
        "multimodal_tool_content_retry_attempted",
        "oauth_1m_beta_retry_attempted",
        "llama_cpp_grammar_retry_attempted",
        "primary_recovery_attempted",
        "has_retried_429",
        "restart_with_compressed_messages",
        "restart_with_length_continuation",
    ];

    pub fn iter(&self) -> impl Iterator<Item = (&'static str, bool)> + '_ {
        Self::HERMES_FIELD_NAMES
            .iter()
            .copied()
            .map(|name| (name, self.value(name).unwrap_or(false)))
    }

    pub fn value(&self, name: &str) -> Option<bool> {
        match name {
            "codex_auth_retry_attempted" => Some(self.codex_auth_retry_attempted),
            "anthropic_auth_retry_attempted" => Some(self.anthropic_auth_retry_attempted),
            "nous_auth_retry_attempted" => Some(self.nous_auth_retry_attempted),
            "nous_paid_entitlement_refresh_attempted" => {
                Some(self.nous_paid_entitlement_refresh_attempted)
            }
            "copilot_auth_retry_attempted" => Some(self.copilot_auth_retry_attempted),
            "thinking_sig_retry_attempted" => Some(self.thinking_sig_retry_attempted),
            "invalid_encrypted_content_retry_attempted" => {
                Some(self.invalid_encrypted_content_retry_attempted)
            }
            "image_shrink_retry_attempted" => Some(self.image_shrink_retry_attempted),
            "multimodal_tool_content_retry_attempted" => {
                Some(self.multimodal_tool_content_retry_attempted)
            }
            "oauth_1m_beta_retry_attempted" => Some(self.oauth_1m_beta_retry_attempted),
            "llama_cpp_grammar_retry_attempted" => Some(self.llama_cpp_grammar_retry_attempted),
            "primary_recovery_attempted" => Some(self.primary_recovery_attempted),
            "has_retried_429" => Some(self.has_retried_429),
            "restart_with_compressed_messages" => Some(self.restart_with_compressed_messages),
            "restart_with_length_continuation" => Some(self.restart_with_length_continuation),
            _ => None,
        }
    }

    pub fn mark_restart_with_compressed_messages(&mut self) -> bool {
        mark_once(&mut self.restart_with_compressed_messages)
    }

    pub fn mark_restart_with_length_continuation(&mut self) -> bool {
        mark_once(&mut self.restart_with_length_continuation)
    }

    pub fn clear_restart_with_length_continuation(&mut self) {
        self.restart_with_length_continuation = false;
    }

    pub fn record_output_token_budget_adjustment(&mut self, max_adjustments: u32) -> bool {
        if self.output_token_budget_adjustments >= max_adjustments {
            return false;
        }
        self.output_token_budget_adjustments += 1;
        true
    }

    pub fn output_token_budget_adjustments(&self) -> u32 {
        self.output_token_budget_adjustments
    }
}

fn mark_once(flag: &mut bool) -> bool {
    if *flag {
        false
    } else {
        *flag = true;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::TurnRetryState;

    #[test]
    fn all_hermes_guards_default_false() {
        let state = TurnRetryState::default();

        for (name, value) in state.iter() {
            assert!(!value, "{name} should default to false");
        }
    }

    #[test]
    fn field_set_matches_hermes_contract() {
        assert_eq!(
            TurnRetryState::HERMES_FIELD_NAMES,
            [
                "codex_auth_retry_attempted",
                "anthropic_auth_retry_attempted",
                "nous_auth_retry_attempted",
                "nous_paid_entitlement_refresh_attempted",
                "copilot_auth_retry_attempted",
                "thinking_sig_retry_attempted",
                "invalid_encrypted_content_retry_attempted",
                "image_shrink_retry_attempted",
                "multimodal_tool_content_retry_attempted",
                "oauth_1m_beta_retry_attempted",
                "llama_cpp_grammar_retry_attempted",
                "primary_recovery_attempted",
                "has_retried_429",
                "restart_with_compressed_messages",
                "restart_with_length_continuation",
            ]
        );
    }

    #[test]
    fn restart_signals_are_one_shot_until_cleared() {
        let mut state = TurnRetryState::default();

        assert!(state.mark_restart_with_compressed_messages());
        assert!(!state.mark_restart_with_compressed_messages());

        assert!(state.mark_restart_with_length_continuation());
        assert!(!state.mark_restart_with_length_continuation());
        state.clear_restart_with_length_continuation();
        assert!(state.mark_restart_with_length_continuation());
    }

    #[test]
    fn output_token_budget_adjustments_are_bounded() {
        let mut state = TurnRetryState::default();

        assert!(state.record_output_token_budget_adjustment(2));
        assert!(state.record_output_token_budget_adjustment(2));
        assert!(!state.record_output_token_budget_adjustment(2));
        assert_eq!(state.output_token_budget_adjustments(), 2);
    }
}
