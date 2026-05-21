//! Interactive first-run setup wizard for Hakimi Agent.
//!
//! Walks the user through LLM provider configuration, agent settings,
//! platform adapters, and MCP server setup. Saves to ~/.hakimi/config.yaml.

use anyhow::Result;
use dialoguer::{Confirm, Input, MultiSelect, Password, Select};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Configuration collected by the setup wizard.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupConfig {
    pub provider: String,
    pub api_key: String,
    pub base_url: String,
    pub model: String,
    pub max_turns: usize,
    pub max_retries: usize,
    pub streaming: bool,
    pub yolo: bool,
    pub platforms: Vec<PlatformConfig>,
    pub mcp_servers: Vec<String>,
}

impl Default for SetupConfig {
    fn default() -> Self {
        Self {
            provider: "openrouter".to_string(),
            api_key: String::new(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "anthropic/claude-sonnet-4-20250514".to_string(),
            max_turns: 90,
            max_retries: 3,
            streaming: true,
            yolo: false,
            platforms: Vec::new(),
            mcp_servers: Vec::new(),
        }
    }
}

/// Configuration for a platform adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlatformConfig {
    pub platform: String,
    pub token: String,
    pub channel_id: Option<String>,
}

// ---------------------------------------------------------------------------
// Provider definitions
// ---------------------------------------------------------------------------

struct ProviderDef {
    name: &'static str,
    label: &'static str,
    default_base_url: &'static str,
}

const PROVIDERS: &[ProviderDef] = &[
    ProviderDef {
        name: "openrouter",
        label: "OpenRouter (recommended — multi-provider gateway)",
        default_base_url: "https://openrouter.ai/api/v1",
    },
    ProviderDef {
        name: "openai",
        label: "OpenAI",
        default_base_url: "https://api.openai.com/v1",
    },
    ProviderDef {
        name: "anthropic",
        label: "Anthropic",
        default_base_url: "https://api.anthropic.com",
    },
    ProviderDef {
        name: "google",
        label: "Google (Gemini)",
        default_base_url: "https://generativelanguage.googleapis.com/v1beta",
    },
    ProviderDef {
        name: "custom",
        label: "Custom endpoint",
        default_base_url: "",
    },
];

/// Curated model list (shown after provider selection).
const MODELS: &[&str] = &[
    "anthropic/claude-sonnet-4-20250514",
    "anthropic/claude-opus-4-20250514",
    "openai/gpt-4.1",
    "openai/gpt-4.1-mini",
    "google/gemini-2.5-pro",
    "deepseek/deepseek-chat",
];

// ---------------------------------------------------------------------------
// MCP server definitions
// ---------------------------------------------------------------------------

struct McpServerDef {
    name: &'static str,
    label: &'static str,
    description: &'static str,
    env_vars: &'static str,
}

const MCP_SERVERS: &[McpServerDef] = &[
    McpServerDef {
        name: "filesystem",
        label: "filesystem — local file access",
        description: "Read, write, and manage files on the local filesystem.",
        env_vars: "No env vars required.",
    },
    McpServerDef {
        name: "github",
        label: "github — GitHub API access",
        description: "Create issues, PRs, manage repos via the GitHub API.",
        env_vars: "Requires: GITHUB_TOKEN",
    },
    McpServerDef {
        name: "brave-search",
        label: "brave-search — web search",
        description: "Search the web using the Brave Search API.",
        env_vars: "Requires: BRAVE_API_KEY",
    },
    McpServerDef {
        name: "postgres",
        label: "postgres — database access",
        description: "Query and manage PostgreSQL databases.",
        env_vars: "Requires: DATABASE_URL",
    },
];

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

fn print_banner() {
    println!();
    println!(r"  ╔═══════════════════════════════════════════════════════╗");
    println!(r"  ║                                                       ║");
    println!(r"  ║    _  _               _ _             _               ║");
    println!(r"  ║   | || |__ _ __ _ __ (_) |_ _  _ __ _| |___ _ _      ║");
    println!(r"  ║   | __ / _` / _| '  \| |  _| || / _` | / -_) '_|    ║");
    println!(r"  ║   |_||_\__,_\__|_|_|_|_|\__|\_,_\__,_|_\___|_|       ║");
    println!(r"  ║                                                       ║");
    println!(r"  ║           Interactive Setup Wizard                    ║");
    println!(r"  ║                                                       ║");
    println!(r"  ╚═══════════════════════════════════════════════════════╝");
    println!();
    println!("  Welcome to Hakimi Agent! This wizard will guide you through");
    println!("  the initial configuration. Press Ctrl+C at any time to exit.");
    println!();
}

// ---------------------------------------------------------------------------
// Wizard steps
// ---------------------------------------------------------------------------

fn step_llm_provider() -> Result<(String, String, String)> {
    println!("━━━ Step 1/5: LLM Provider ━━━");
    println!();

    // Provider selection
    let provider_labels: Vec<&str> = PROVIDERS.iter().map(|p| p.label).collect();
    let provider_idx = Select::new()
        .with_prompt("Select your LLM provider")
        .items(&provider_labels)
        .default(0)
        .interact()?;

    let selected = &PROVIDERS[provider_idx];
    let provider = selected.name.to_string();

    // API key (masked)
    println!();
    let api_key: String = Password::new()
        .with_prompt("Enter your API key")
        .with_confirmation("Confirm API key", "Keys don't match")
        .interact()?;

    // Base URL
    let base_url = if provider == "custom" {
        println!();
        Input::<String>::new()
            .with_prompt("Enter base URL for the custom endpoint")
            .interact_text()?
    } else if provider == "openrouter" {
        println!();
        let default_url = selected.default_base_url;
        let use_default = Confirm::new()
            .with_prompt(format!("Use default base URL ({})?", default_url))
            .default(true)
            .interact()?;
        if use_default {
            default_url.to_string()
        } else {
            Input::<String>::new()
                .with_prompt("Enter custom base URL")
                .default(default_url.to_string())
                .interact_text()?
        }
    } else {
        selected.default_base_url.to_string()
    };

    Ok((provider, api_key, base_url))
}

fn step_select_model() -> Result<String> {
    println!();
    println!("━━━ Step 2/5: Default Model ━━━");
    println!();

    let mut model_items: Vec<String> = MODELS.iter().map(|s| s.to_string()).collect();
    model_items.push("Custom (enter model name)".to_string());

    let model_idx = Select::new()
        .with_prompt("Select default model")
        .items(&model_items)
        .default(0)
        .interact()?;

    let model = if model_idx == model_items.len() - 1 {
        // Custom model
        println!();
        Input::<String>::new()
            .with_prompt("Enter model name (e.g. \"my-org/my-model\")")
            .interact_text()?
    } else {
        MODELS[model_idx].to_string()
    };

    Ok(model)
}

fn step_agent_settings() -> Result<(usize, usize, bool, bool)> {
    println!();
    println!("━━━ Step 3/5: Agent Settings ━━━");
    println!();

    let max_turns: usize = Input::new()
        .with_prompt("Max turns per conversation")
        .default(90)
        .interact_text()?;

    let max_retries: usize = Input::new()
        .with_prompt("Max retries on error")
        .default(3)
        .interact_text()?;

    let streaming = Confirm::new()
        .with_prompt("Enable streaming mode?")
        .default(true)
        .interact()?;

    println!();
    println!("  ⚠  YOLO mode auto-approves ALL tool calls without confirmation.");
    println!("     Only enable this if you trust the agent in your environment.");
    let yolo = Confirm::new()
        .with_prompt("Enable YOLO mode?")
        .default(false)
        .interact()?;

    Ok((max_turns, max_retries, streaming, yolo))
}

fn step_platform_adapters() -> Result<Vec<PlatformConfig>> {
    println!();
    println!("━━━ Step 4/5: Platform Adapters ━━━");
    println!("  (Optional: connect Hakimi to messaging platforms)");
    println!();

    let platform_options = &[
        "Telegram — requires a bot token from @BotFather",
        "Discord — requires bot token + channel ID",
        "Slack — requires bot token + channel ID",
        "Skip — don't configure any platforms now",
    ];

    let selections = MultiSelect::new()
        .with_prompt("Select platform adapters to configure (Space to toggle, Enter to confirm)")
        .items(platform_options)
        .defaults(&[false, false, false, true])
        .interact()?;

    let mut platforms = Vec::new();

    for &idx in &selections {
        match idx {
            0 => {
                // Telegram
                println!();
                let token: String = Password::new()
                    .with_prompt("Telegram bot token")
                    .interact()?;
                platforms.push(PlatformConfig {
                    platform: "telegram".to_string(),
                    token,
                    channel_id: None,
                });
            }
            1 => {
                // Discord
                println!();
                let token: String = Password::new()
                    .with_prompt("Discord bot token")
                    .interact()?;
                let channel: String = Input::new()
                    .with_prompt("Discord channel ID")
                    .interact_text()?;
                platforms.push(PlatformConfig {
                    platform: "discord".to_string(),
                    token,
                    channel_id: Some(channel),
                });
            }
            2 => {
                // Slack
                println!();
                let token: String = Password::new().with_prompt("Slack bot token").interact()?;
                let channel: String = Input::new()
                    .with_prompt("Slack channel ID")
                    .interact_text()?;
                platforms.push(PlatformConfig {
                    platform: "slack".to_string(),
                    token,
                    channel_id: Some(channel),
                });
            }
            3 => {
                // Skip
            }
            _ => {}
        }
    }

    Ok(platforms)
}

fn step_mcp_servers() -> Result<Vec<String>> {
    println!();
    println!("━━━ Step 5/5: MCP Servers ━━━");
    println!("  (Optional: extend Hakimi with tool servers)");
    println!();

    // Show descriptions
    for server in MCP_SERVERS {
        println!("  • {}: {}", server.name, server.description);
        println!("    {}", server.env_vars);
    }
    println!();

    let server_labels: Vec<&str> = MCP_SERVERS.iter().map(|s| s.label).collect();

    let selections = MultiSelect::new()
        .with_prompt("Select MCP servers to enable (Space to toggle, Enter to confirm)")
        .items(&server_labels)
        .interact()?;

    let enabled: Vec<String> = selections
        .into_iter()
        .map(|i| MCP_SERVERS[i].name.to_string())
        .collect();

    Ok(enabled)
}

fn print_summary(config: &SetupConfig) {
    println!();
    println!("━━━ Configuration Summary ━━━");
    println!();
    println!("  Provider:       {}", config.provider);
    println!("  Base URL:       {}", config.base_url);
    println!("  Model:          {}", config.model);
    println!(
        "  API Key:        {}...{}",
        &config.api_key[..4.min(config.api_key.len())],
        if config.api_key.len() > 4 {
            &config.api_key[config.api_key.len() - 4..]
        } else {
            ""
        }
    );
    println!("  Max Turns:      {}", config.max_turns);
    println!("  Max Retries:    {}", config.max_retries);
    println!(
        "  Streaming:      {}",
        if config.streaming { "yes" } else { "no" }
    );
    println!(
        "  YOLO Mode:      {}",
        if config.yolo { "yes ⚠" } else { "no" }
    );

    if config.platforms.is_empty() {
        println!("  Platforms:      (none)");
    } else {
        for p in &config.platforms {
            println!(
                "  Platform:       {} (token: {}...)",
                p.platform,
                &p.token[..4.min(p.token.len())]
            );
        }
    }

    if config.mcp_servers.is_empty() {
        println!("  MCP Servers:    (none)");
    } else {
        println!("  MCP Servers:    {}", config.mcp_servers.join(", "));
    }
    println!();
}

// ---------------------------------------------------------------------------
// Config file generation
// ---------------------------------------------------------------------------

/// Convert a SetupConfig into YAML config content for ~/.hakimi/config.yaml.
fn generate_config_yaml(config: &SetupConfig) -> String {
    let mut yaml = String::new();

    yaml.push_str("# Hakimi Agent Configuration\n");
    yaml.push_str("# Generated by setup wizard\n\n");

    // Model section
    yaml.push_str("model:\n");
    yaml.push_str(&format!("  default: \"{}\"\n", config.model));
    yaml.push_str(&format!("  provider: \"{}\"\n", config.provider));
    yaml.push_str(&format!("  base_url: \"{}\"\n", config.base_url));

    // Agent section
    yaml.push_str("\nagent:\n");
    yaml.push_str(&format!("  max_turns: {}\n", config.max_turns));
    yaml.push_str("  verbose: false\n");
    yaml.push_str("  system_prompt: \"\"\n");

    // Display section
    yaml.push_str("\ndisplay:\n");
    yaml.push_str(&format!("  streaming: {}\n", config.streaming));
    yaml.push_str("  compact: false\n");
    yaml.push_str("  skin: \"default\"\n");

    // Terminal section
    yaml.push_str("\nterminal:\n");
    yaml.push_str("  env_type: \"local\"\n");
    yaml.push_str("  cwd: \".\"\n");
    yaml.push_str("  timeout: 60\n");

    // Compression section
    yaml.push_str("\ncompression:\n");
    yaml.push_str("  engine: smart\n");
    yaml.push_str("  context_length: 128000\n");

    // MCP servers section
    if !config.mcp_servers.is_empty() {
        yaml.push_str("\nmcp_servers:\n");
        for server in &config.mcp_servers {
            match server.as_str() {
                "filesystem" => {
                    yaml.push_str("  filesystem:\n");
                    yaml.push_str("    command: \"npx\"\n");
                    yaml.push_str(
                        "    args: [\"-y\", \"@modelcontextprotocol/server-filesystem\", \".\"]\n",
                    );
                }
                "github" => {
                    yaml.push_str("  github:\n");
                    yaml.push_str("    command: \"npx\"\n");
                    yaml.push_str("    args: [\"-y\", \"@modelcontextprotocol/server-github\"]\n");
                    yaml.push_str("    env:\n");
                    yaml.push_str("      GITHUB_TOKEN: \"${GITHUB_TOKEN}\"\n");
                }
                "brave-search" => {
                    yaml.push_str("  brave-search:\n");
                    yaml.push_str("    command: \"npx\"\n");
                    yaml.push_str(
                        "    args: [\"-y\", \"@modelcontextprotocol/server-brave-search\"]\n",
                    );
                    yaml.push_str("    env:\n");
                    yaml.push_str("      BRAVE_API_KEY: \"${BRAVE_API_KEY}\"\n");
                }
                "postgres" => {
                    yaml.push_str("  postgres:\n");
                    yaml.push_str("    command: \"npx\"\n");
                    yaml.push_str(
                        "    args: [\"-y\", \"@modelcontextprotocol/server-postgres\"]\n",
                    );
                    yaml.push_str("    env:\n");
                    yaml.push_str("      DATABASE_URL: \"${DATABASE_URL}\"\n");
                }
                _ => {}
            }
        }
    }

    // Platform adapter hints as comments
    if !config.platforms.is_empty() {
        yaml.push_str(
            "\n# Platform adapters (configure via environment variables or dedicated config)\n",
        );
        for p in &config.platforms {
            yaml.push_str(&format!(
                "# {}: token=\"{}...\"\n",
                p.platform,
                &p.token[..4.min(p.token.len())]
            ));
        }
    }

    yaml
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Get the path to the ~/.hakimi/ directory.
pub fn hakimi_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".hakimi"))
        .unwrap_or_else(|| PathBuf::from(".hakimi"))
}

/// Create the ~/.hakimi/ directory structure.
pub fn create_directory_structure() -> Result<()> {
    let base = hakimi_dir();
    std::fs::create_dir_all(&base)?;
    std::fs::create_dir_all(base.join("plugins"))?;
    std::fs::create_dir_all(base.join("skills"))?;
    std::fs::create_dir_all(base.join("memory"))?;
    Ok(())
}

/// Save a SetupConfig to ~/.hakimi/config.yaml.
pub fn save_config(config: &SetupConfig) -> Result<()> {
    create_directory_structure()?;
    let config_path = hakimi_dir().join("config.yaml");
    let yaml = generate_config_yaml(config);
    std::fs::write(&config_path, yaml)?;
    Ok(())
}

/// Run the interactive setup wizard.
///
/// Returns the collected configuration. If `non_interactive` is true,
/// returns defaults without prompting.
pub fn run_setup_wizard(non_interactive: bool) -> Result<SetupConfig> {
    if non_interactive {
        return Ok(SetupConfig::default());
    }

    print_banner();

    let (provider, api_key, base_url) = step_llm_provider()?;
    let model = step_select_model()?;
    let (max_turns, max_retries, streaming, yolo) = step_agent_settings()?;
    let platforms = step_platform_adapters()?;
    let mcp_servers = step_mcp_servers()?;

    let config = SetupConfig {
        provider,
        api_key,
        base_url,
        model,
        max_turns,
        max_retries,
        streaming,
        yolo,
        platforms,
        mcp_servers,
    };

    print_summary(&config);

    let save = Confirm::new()
        .with_prompt("Save this configuration?")
        .default(true)
        .interact()?;

    if save {
        save_config(&config)?;
        println!();
        println!("  ✓ Configuration saved to ~/.hakimi/config.yaml");
        println!("  ✓ Directory structure created at ~/.hakimi/");
        println!();
        println!("  You can now run `hakimi` to start, or `hakimi doctor` to verify.");
    } else {
        println!();
        println!("  Configuration not saved. You can run `hakimi setup` anytime.");
    }

    println!();
    Ok(config)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_serialization() {
        let config = SetupConfig {
            provider: "openrouter".to_string(),
            api_key: "sk-test-1234".to_string(),
            base_url: "https://openrouter.ai/api/v1".to_string(),
            model: "anthropic/claude-sonnet-4-20250514".to_string(),
            max_turns: 90,
            max_retries: 3,
            streaming: true,
            yolo: false,
            platforms: vec![],
            mcp_servers: vec!["filesystem".to_string()],
        };

        let yaml = generate_config_yaml(&config);
        assert!(yaml.contains("openrouter"));
        assert!(yaml.contains("claude-sonnet-4-20250514"));
        assert!(yaml.contains("max_turns: 90"));
        assert!(yaml.contains("streaming: true"));
        assert!(yaml.contains("filesystem"));
    }

    #[test]
    fn test_directory_structure_creation() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join(".hakimi");
        // Simulate create_directory_structure with a custom base
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(base.join("plugins")).unwrap();
        std::fs::create_dir_all(base.join("skills")).unwrap();
        std::fs::create_dir_all(base.join("memory")).unwrap();

        assert!(base.exists());
        assert!(base.join("plugins").exists());
        assert!(base.join("skills").exists());
        assert!(base.join("memory").exists());
    }

    #[test]
    fn test_doctor_config_validation() {
        // Test that a generated config YAML can be parsed by hakimi-config
        let config = SetupConfig::default();
        let yaml = generate_config_yaml(&config);
        let parsed: hakimi_config::HakimiConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed.agent.max_turns, 90);
        assert!(parsed.display.streaming);
    }

    #[test]
    fn test_provider_list_not_empty() {
        assert!(!PROVIDERS.is_empty());
        assert!(PROVIDERS.iter().any(|p| p.name == "openrouter"));
        assert!(PROVIDERS.iter().any(|p| p.name == "openai"));
        assert!(PROVIDERS.iter().any(|p| p.name == "anthropic"));
        assert!(PROVIDERS.iter().any(|p| p.name == "google"));
        assert!(PROVIDERS.iter().any(|p| p.name == "custom"));
    }

    #[test]
    fn test_default_setup_config() {
        let config = SetupConfig::default();
        assert_eq!(config.provider, "openrouter");
        assert_eq!(config.max_turns, 90);
        assert!(config.streaming);
        assert!(!config.yolo);
    }

    #[test]
    fn test_generate_config_with_platforms() {
        let config = SetupConfig {
            platforms: vec![PlatformConfig {
                platform: "telegram".to_string(),
                token: "123456:ABC-DEF".to_string(),
                channel_id: None,
            }],
            ..Default::default()
        };
        let yaml = generate_config_yaml(&config);
        assert!(yaml.contains("telegram"));
    }

    #[test]
    fn test_default_config_base_url() {
        let config = SetupConfig::default();
        assert_eq!(config.base_url, "https://openrouter.ai/api/v1");
    }

    #[test]
    fn test_default_config_model() {
        let config = SetupConfig::default();
        assert_eq!(config.model, "anthropic/claude-sonnet-4-20250514");
    }

    #[test]
    fn test_non_interactive_returns_defaults() {
        let config = run_setup_wizard(true).unwrap();
        assert_eq!(config.provider, "openrouter");
        assert_eq!(config.max_turns, 90);
        assert_eq!(config.max_retries, 3);
        assert!(config.streaming);
        assert!(!config.yolo);
        assert!(config.platforms.is_empty());
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn test_mcp_server_list_not_empty() {
        assert!(!MCP_SERVERS.is_empty());
        assert!(MCP_SERVERS.iter().any(|s| s.name == "filesystem"));
        assert!(MCP_SERVERS.iter().any(|s| s.name == "github"));
        assert!(MCP_SERVERS.iter().any(|s| s.name == "brave-search"));
        assert!(MCP_SERVERS.iter().any(|s| s.name == "postgres"));
    }

    #[test]
    fn test_model_list_not_empty() {
        assert!(!MODELS.is_empty());
        assert!(MODELS.iter().any(|m| m.contains("claude-sonnet")));
        assert!(MODELS.iter().any(|m| m.contains("gpt-4.1")));
        assert!(MODELS.iter().any(|m| m.contains("gemini")));
    }

    #[test]
    fn test_generate_config_yaml_with_mcp_servers() {
        let config = SetupConfig {
            mcp_servers: vec!["github".to_string(), "brave-search".to_string()],
            ..Default::default()
        };
        let yaml = generate_config_yaml(&config);
        assert!(yaml.contains("github"));
        assert!(yaml.contains("GITHUB_TOKEN"));
        assert!(yaml.contains("brave-search"));
        assert!(yaml.contains("BRAVE_API_KEY"));
        assert!(yaml.contains("mcp_servers:"));
    }

    #[test]
    fn test_generate_config_yaml_no_mcp_servers() {
        let config = SetupConfig::default();
        let yaml = generate_config_yaml(&config);
        assert!(!yaml.contains("mcp_servers:"));
    }

    #[test]
    fn test_generate_config_yaml_with_discord_platform() {
        let config = SetupConfig {
            platforms: vec![PlatformConfig {
                platform: "discord".to_string(),
                token: "discord-token-12345".to_string(),
                channel_id: Some("123456789".to_string()),
            }],
            ..Default::default()
        };
        let yaml = generate_config_yaml(&config);
        assert!(yaml.contains("discord"));
    }

    #[test]
    fn test_setup_config_serialization_roundtrip() {
        let config = SetupConfig {
            provider: "openai".to_string(),
            api_key: "sk-test".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4.1".to_string(),
            max_turns: 50,
            max_retries: 5,
            streaming: false,
            yolo: true,
            platforms: vec![],
            mcp_servers: vec![],
        };
        let json = serde_json::to_string(&config).unwrap();
        let deserialized: SetupConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.provider, "openai");
        assert_eq!(deserialized.max_turns, 50);
        assert!(deserialized.yolo);
        assert!(!deserialized.streaming);
    }
}
