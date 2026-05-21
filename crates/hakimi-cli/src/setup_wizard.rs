//! Interactive first-run setup wizard.
//!
//! Guides users through model selection, API key configuration,
//! and platform setup on first launch.

use tracing::info;

/// Configuration collected by the setup wizard.
#[derive(Debug, Clone)]
pub struct WizardConfig {
    /// Selected model (e.g. "claude-sonnet-4-20250514").
    pub model: String,
    /// Selected provider (e.g. "anthropic", "openai").
    pub provider: String,
    /// API key (masked in logs).
    pub api_key: String,
    /// Base URL (if custom).
    pub base_url: Option<String>,
    /// Enable streaming.
    pub streaming: bool,
}

impl Default for WizardConfig {
    fn default() -> Self {
        Self {
            model: String::new(),
            provider: "anthropic".to_string(),
            api_key: String::new(),
            base_url: None,
            streaming: true,
        }
    }
}

/// Provider option for the wizard.
#[derive(Debug, Clone)]
pub struct ProviderOption {
    pub name: &'static str,
    pub description: &'static str,
    pub default_model: &'static str,
    pub base_url: &'static str,
}

/// Available providers for the wizard.
pub const PROVIDERS: &[ProviderOption] = &[
    ProviderOption {
        name: "anthropic",
        description: "Anthropic (Claude models)",
        default_model: "claude-sonnet-4-20250514",
        base_url: "https://api.anthropic.com",
    },
    ProviderOption {
        name: "openai",
        description: "OpenAI (GPT models)",
        default_model: "gpt-4o",
        base_url: "https://api.openai.com/v1",
    },
    ProviderOption {
        name: "openrouter",
        description: "OpenRouter (multi-provider)",
        default_model: "anthropic/claude-sonnet-4-20250514",
        base_url: "https://openrouter.ai/api/v1",
    },
    ProviderOption {
        name: "custom",
        description: "Custom endpoint",
        default_model: "",
        base_url: "",
    },
];

/// Run the setup wizard interactively.
///
/// Returns the collected configuration. In non-interactive mode,
/// returns defaults.
pub fn run_wizard(interactive: bool) -> anyhow::Result<WizardConfig> {
    if !interactive {
        info!("Non-interactive mode, using defaults");
        return Ok(WizardConfig::default());
    }

    // In a real implementation, this would:
    // 1. Display provider options
    // 2. Prompt for selection
    // 3. Prompt for API key
    // 4. Confirm configuration
    //
    // For now, return defaults.
    info!("Setup wizard would run interactively");
    Ok(WizardConfig::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wizard_config_default() {
        let config = WizardConfig::default();
        assert_eq!(config.provider, "anthropic");
        assert!(config.streaming);
        assert!(config.api_key.is_empty());
    }

    #[test]
    fn test_providers_not_empty() {
        assert!(!PROVIDERS.is_empty());
        assert!(PROVIDERS.iter().any(|p| p.name == "anthropic"));
        assert!(PROVIDERS.iter().any(|p| p.name == "openai"));
    }

    #[test]
    fn test_run_wizard_non_interactive() {
        let config = run_wizard(false).unwrap();
        assert_eq!(config.provider, "anthropic");
    }

    #[test]
    fn test_provider_option_fields() {
        let anthropic = &PROVIDERS[0];
        assert_eq!(anthropic.name, "anthropic");
        assert!(!anthropic.default_model.is_empty());
        assert!(!anthropic.base_url.is_empty());
    }
}
