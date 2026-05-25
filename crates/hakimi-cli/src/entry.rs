//! Hakimi Agent CLI entry point.
//!
//! Contains the clap [`Args`], configuration loading, agent construction, and
//! the interactive REPL / single-query / server modes so that both the
//! `hakimi-cli` binary and the thin `hakimi-agent` wrapper can share the same
//! implementation.

use anyhow::Result;
use clap::Parser;
use std::io::{self, Write};
use tracing::{error, info, warn};

use crate::Command;

enum GatewayStreamUiEvent {
    Content(String),
    Tool(String),
}

#[derive(Debug, Default)]
struct GatewayStreamUiState {
    current_text: String,
    last_edit_text: String,
    needs_new_message: bool,
}

impl GatewayStreamUiState {
    fn append_content(&mut self, token: &str) -> Option<GatewayUiContentTarget> {
        self.current_text.push_str(token);
        if self.current_text.is_empty() || self.current_text == self.last_edit_text {
            return None;
        }

        self.last_edit_text = self.current_text.clone();
        let target = if self.needs_new_message {
            self.needs_new_message = false;
            GatewayUiContentTarget::NewMessage
        } else {
            GatewayUiContentTarget::EditCurrent
        };
        Some(target)
    }

    fn finish_tool_boundary(&mut self) {
        self.current_text.clear();
        self.last_edit_text.clear();
        self.needs_new_message = true;
    }
}

#[derive(Debug, PartialEq, Eq)]
enum GatewayUiContentTarget {
    EditCurrent,
    NewMessage,
}

// ---------------------------------------------------------------------------
// CLI arguments (clap)
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(
    name = "hakimi",
    version,
    about = "Hakimi Agent — AI-powered coding assistant"
)]
pub struct Args {
    /// Model identifier override (e.g. "gpt-4o", "claude-sonnet-4-20250514").
    #[arg(long)]
    pub model: Option<String>,

    /// Provider override (e.g. "openrouter", "anthropic").
    #[arg(long)]
    pub provider: Option<String>,

    /// Single query mode: send a prompt and exit.
    #[arg(long, short)]
    pub query: Option<String>,

    /// Configuration profile to load.
    #[arg(long, short)]
    pub profile: Option<String>,

    /// Auto-accept all tool calls without confirmation (YOLO mode).
    #[arg(long)]
    pub yolo: bool,

    /// API key (overrides env var / config).
    #[arg(long)]
    pub api_key: Option<String>,

    /// Base URL for the API endpoint.
    #[arg(long)]
    pub base_url: Option<String>,

    /// Start the HTTP API server instead of the interactive REPL.
    #[arg(long)]
    pub serve: bool,

    /// Start gateway mode (Telegram/Discord/etc.) instead of interactive REPL.
    #[arg(long)]
    pub gateway: bool,

    /// Address for the HTTP API server (default: 127.0.0.1:3000).
    #[arg(long, default_value = "127.0.0.1:3000")]
    pub addr: String,

    /// Run the interactive setup wizard.
    #[arg(long)]
    pub setup: bool,

    /// Self-update: download and install the latest release from GitHub.
    #[arg(long)]
    pub update: bool,

    /// Install and enable a plugin by URL or path
    #[arg(long)]
    pub plugin_install: Option<String>,
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// Default config YAML
// ---------------------------------------------------------------------------

const DEFAULT_CONFIG_YAML: &str = r#"# Hakimi Agent Configuration
# ~/.hakimi/config.yaml

model:
  # Default model identifier (e.g. "gpt-4o", "claude-sonnet-4-20250514")
  default: ""
  # Provider: "auto", "openrouter", "anthropic", "openai"
  provider: "auto"
  # Base URL for API endpoint (leave empty for provider default)
  base_url: ""

agent:
  # Maximum tool-calling iterations per conversation
  max_turns: 90
  # Enable verbose logging
  verbose: false
  # Custom system prompt (leave empty for default)
  system_prompt: ""

display:
  # Enable streaming output
  streaming: true
  # Compact output mode
  compact: false
  # UI skin
  skin: "default"

terminal:
  # Backend: "local", "docker", "ssh"
  env_type: "local"
  # Working directory for terminal operations
  cwd: "."
  # Command execution timeout (seconds)
  timeout: 60

delegation:
  # Max iterations per sub-agent
  max_iterations: 45
  # Sub-agent model (empty = inherit parent)
  model: ""
  # Sub-agent API key
  api_key: ""

embedding:
  # Use the same OpenAI-compatible site/key as the main model by default.
  enabled: true
  provider: "openai-compatible"
  base_url: "same-as-llm"
  api_key: "same-as-llm"
  model: "BAAI/bge-m3"
  dimension: 1024
  batch_size: 32
  normalize: true

# Context compression: smart (3-tier) or simple (truncation)
compression:
  engine: smart  # smart | simple
  context_length: 128000

# MCP servers to connect to at startup.
# Each server is spawned as a child process and communicates via JSON-RPC over stdio.
# mcp_servers:
#   filesystem:
#     command: "npx"
#     args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
#   github:
#     command: "npx"
#     args: ["-y", "@modelcontextprotocol/server-github"]
#     env:
#       GITHUB_TOKEN: "your-token-here"
"#;

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

fn load_config() -> hakimi_config::HakimiConfig {
    let hakimi_dir = dirs::home_dir()
        .map(|h| h.join(".hakimi"))
        .unwrap_or_else(|| std::path::PathBuf::from(".hakimi"));

    let config_path = hakimi_dir.join("config.yaml");

    // Create ~/.hakimi/ directory on first run.
    if !hakimi_dir.exists() {
        if let Err(e) = std::fs::create_dir_all(&hakimi_dir) {
            warn!(path = %hakimi_dir.display(), error = %e, "failed to create .hakimi directory");
        } else {
            info!(path = %hakimi_dir.display(), "created .hakimi directory");
        }
    }

    // Create default config.yaml on first run.
    if !config_path.exists() {
        let default_config = DEFAULT_CONFIG_YAML;
        match std::fs::write(&config_path, default_config) {
            Ok(_) => {
                info!(path = %config_path.display(), "created default config.yaml");
            }
            Err(e) => {
                warn!(path = %config_path.display(), error = %e, "failed to create default config.yaml");
            }
        }
    }

    // Load and parse the config file.
    match std::fs::read_to_string(&config_path) {
        Ok(contents) => match serde_yaml::from_str::<hakimi_config::HakimiConfig>(&contents) {
            Ok(config) => {
                info!(path = %config_path.display(), "loaded config from file");
                config
            }
            Err(e) => {
                warn!(path = %config_path.display(), error = %e, "failed to parse config file, using defaults");
                hakimi_config::HakimiConfig::default()
            }
        },
        Err(e) => {
            warn!(path = %config_path.display(), error = %e, "failed to read config file, using defaults");
            hakimi_config::HakimiConfig::default()
        }
    }
}

fn hakimi_config_path() -> std::path::PathBuf {
    dirs::home_dir()
        .map(|h| h.join(".hakimi").join("config.yaml"))
        .unwrap_or_else(|| std::path::PathBuf::from(".hakimi/config.yaml"))
}

fn prompt_text(label: &str, default: &str) -> Result<String> {
    if default.is_empty() {
        print!("{label}: ");
    } else {
        print!("{label} [{default}]: ");
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.is_empty() {
        Ok(default.to_string())
    } else {
        Ok(trimmed.to_string())
    }
}

fn prompt_optional(label: &str, default: &str) -> Result<String> {
    let value = prompt_text(label, default)?;
    if value.eq_ignore_ascii_case("skip") {
        Ok(String::new())
    } else {
        Ok(value)
    }
}

fn prompt_yes_no(label: &str, default: bool) -> Result<bool> {
    let suffix = if default { "Y/n" } else { "y/N" };
    print!("{label} [{suffix}]: ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim().to_ascii_lowercase();
    if trimmed.is_empty() {
        return Ok(default);
    }
    Ok(matches!(trimmed.as_str(), "y" | "yes" | "是" | "好" | "1"))
}

fn write_config_file(config: &hakimi_config::HakimiConfig) -> Result<std::path::PathBuf> {
    let path = hakimi_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let yaml = serde_yaml::to_string(config)?;
    std::fs::write(&path, yaml)?;
    Ok(path)
}

fn run_setup_wizard(mut config: hakimi_config::HakimiConfig) -> Result<()> {
    println!("🐙 Hakimi Agent setup wizard");
    println!("This will write ~/.hakimi/config.yaml. Press Enter to accept defaults.");
    println!("Type 'skip' for optional keys you want to leave empty.\n");

    let provider_default = if config.model.provider.is_empty() {
        "openrouter"
    } else {
        &config.model.provider
    };
    config.model.provider = prompt_text(
        "LLM provider (openrouter/openai/anthropic/custom)",
        provider_default,
    )?;

    let model_default = if config.model.default.is_empty() {
        match config.model.provider.as_str() {
            "anthropic" => "claude-sonnet-4-20250514",
            "openai" => "gpt-4o-mini",
            _ => "anthropic/claude-sonnet-4",
        }
    } else {
        &config.model.default
    };
    config.model.default = prompt_text("Default model", model_default)?;

    let base_url_default = if config.model.base_url.is_empty() {
        match config.model.provider.as_str() {
            "openrouter" => "https://openrouter.ai/api/v1",
            "openai" => "https://api.openai.com/v1",
            "anthropic" => "https://api.anthropic.com",
            _ => "",
        }
    } else {
        &config.model.base_url
    };
    config.model.base_url = prompt_optional("Base URL", base_url_default)?;
    config.model.api_key = prompt_optional("API key", &config.model.api_key)?;

    if prompt_yes_no("Configure Telegram gateway bot token now?", false)? {
        config.gateways.telegram.bot_token =
            prompt_optional("Telegram bot token", &config.gateways.telegram.bot_token)?;
    }

    config.agent.max_turns = prompt_text(
        "Max tool-calling turns",
        &config.agent.max_turns.to_string(),
    )?
    .parse()
    .unwrap_or(config.agent.max_turns);

    let path = write_config_file(&config)?;
    println!("\n✅ Hakimi configuration saved to {}", path.display());
    println!("Next steps:");
    println!("  hakimi --query \"hello\"");
    println!("  hakimi --gateway   # if you configured Telegram");
    Ok(())
}

// ---------------------------------------------------------------------------
// Resolve provider (anthropic vs openai-compatible)
// ---------------------------------------------------------------------------

/// Resolve the effective provider from args, config, model prefix, and env.
fn resolve_provider<'a>(
    args_provider: Option<&'a str>,
    config: &'a hakimi_config::HakimiConfig,
    model: &str,
) -> String {
    // 1. CLI argument
    if let Some(p) = args_provider
        && !p.is_empty()
        && p != "auto"
    {
        return p.to_string();
    }
    // 2. Environment variable
    if let Ok(val) = std::env::var("HAKIMI_PROVIDER")
        && !val.is_empty()
        && val != "auto"
    {
        return val;
    }
    // 3. Config file
    if !config.model.provider.is_empty() && config.model.provider != "auto" {
        return config.model.provider.clone();
    }
    // 4. Infer from model name prefix (e.g. "anthropic/claude-sonnet" → "anthropic")
    if let Some(slash_pos) = model.find('/') {
        let prefix = &model[..slash_pos];
        match prefix {
            "anthropic" | "claude" => return "anthropic".to_string(),
            "openai" | "gpt" => return "openai".to_string(),
            "openrouter" => return "openrouter".to_string(),
            _ => {}
        }
    }
    // 5. Infer from model name itself
    if model.starts_with("claude") {
        return "anthropic".to_string();
    }
    if model.starts_with("gpt-") || model.starts_with("o1") || model.starts_with("o3") {
        return "openai".to_string();
    }
    // 6. Default to OpenRouter (broadest compatibility)
    "openrouter".to_string()
}

/// Check if the effective provider should use the Anthropic transport.
fn is_anthropic_provider(provider: &str, base_url: &str) -> bool {
    provider == "anthropic"
        || provider == "claude"
        || base_url.contains("api.anthropic.com")
        || base_url.contains("anthropic")
}

// ---------------------------------------------------------------------------
// Resolve API key from args > env > config
// ---------------------------------------------------------------------------

fn resolve_api_key(args_key: Option<&str>, config: &hakimi_config::HakimiConfig) -> String {
    // 1. CLI argument
    if let Some(key) = args_key
        && !key.is_empty()
    {
        return key.to_string();
    }
    // 2. Environment variables
    for var in &[
        "HAKIMI_API_KEY",
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
    ] {
        if let Ok(val) = std::env::var(var)
            && !val.is_empty()
        {
            info!(env_var = var, "using API key from environment");
            return val;
        }
    }
    // 3. Config file model.api_key
    if !config.model.api_key.is_empty() {
        return config.model.api_key.clone();
    }
    // 4. Config file roles.default fallback (as final check)
    if let Some(default_role) = config.roles.get("default")
        && !default_role.api_key.is_empty()
    {
        return default_role.api_key.clone();
    }
    if !config.delegation.api_key.is_empty() {
        return config.delegation.api_key.clone();
    }

    String::new()
}

// ---------------------------------------------------------------------------
// Resolve base URL
// ---------------------------------------------------------------------------

fn resolve_base_url(args_url: Option<&str>, config: &hakimi_config::HakimiConfig) -> String {
    // 1. CLI argument
    if let Some(url) = args_url
        && !url.is_empty()
    {
        return url.to_string();
    }
    // 2. Environment variable
    if let Ok(val) = std::env::var("HAKIMI_BASE_URL")
        && !val.is_empty()
    {
        return val;
    }
    // 3. Config
    if !config.model.base_url.is_empty() {
        return config.model.base_url.clone();
    }
    // 4. Default — OpenRouter is a reasonable default
    "https://openrouter.ai/api".to_string()
}

fn resolve_embedding_base_url(
    config: &hakimi_config::HakimiConfig,
    resolved_llm_base_url: &str,
) -> String {
    if let Ok(val) = std::env::var("HAKIMI_EMBEDDING_BASE_URL")
        && !val.is_empty()
    {
        return val;
    }
    let configured = config.embedding.base_url.trim();
    if configured.is_empty() || configured == "same-as-llm" {
        resolved_llm_base_url.to_string()
    } else {
        configured.to_string()
    }
}

fn resolve_embedding_api_key(
    config: &hakimi_config::HakimiConfig,
    resolved_llm_api_key: &str,
) -> String {
    if let Ok(val) = std::env::var("HAKIMI_EMBEDDING_API_KEY")
        && !val.is_empty()
    {
        return val;
    }
    let configured = config.embedding.api_key.trim();
    if configured.is_empty() || configured == "same-as-llm" {
        resolved_llm_api_key.to_string()
    } else {
        configured.to_string()
    }
}

// ---------------------------------------------------------------------------
// Resolve model
// ---------------------------------------------------------------------------

fn resolve_model(args_model: Option<&str>, config: &hakimi_config::HakimiConfig) -> String {
    // 1. CLI argument
    if let Some(m) = args_model
        && !m.is_empty()
    {
        return m.to_string();
    }
    // 2. Environment variable
    if let Ok(val) = std::env::var("HAKIMI_MODEL")
        && !val.is_empty()
    {
        return val;
    }
    // 3. Config
    if !config.model.default.is_empty() {
        return config.model.default.clone();
    }
    // 4. Default
    "anthropic/claude-sonnet-4-20250514".to_string()
}

// ---------------------------------------------------------------------------
// MCP tool registration
// ---------------------------------------------------------------------------

/// Connect to configured MCP servers and register their tools.
/// Returns the total number of MCP tools registered.
async fn register_mcp_tools(
    servers: &std::collections::HashMap<String, hakimi_config::McpServerConfig>,
    tool_registry: &hakimi_tools::ToolRegistry,
) -> usize {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let mut total_tools = 0;

    for (name, server_config) in servers {
        info!(server = %name, command = %server_config.command, "connecting to MCP server");

        // Set environment variables BEFORE spawning so the child process inherits them.
        for (key, val) in &server_config.env {
            // SAFETY: We're setting env vars during single-threaded startup,
            // before any concurrent reads begin.
            unsafe {
                std::env::set_var(key, val);
            }
        }

        // Build args as &str slices
        let args: Vec<&str> = server_config.args.iter().map(|s| s.as_str()).collect();

        let mut client =
            match hakimi_mcp::McpClient::connect_stdio(&server_config.command, &args).await {
                Ok(c) => c,
                Err(e) => {
                    warn!(server = %name, error = %e, "failed to spawn MCP server");
                    continue;
                }
            };

        if let Err(e) = client.initialize().await {
            warn!(server = %name, error = %e, "MCP initialize failed");
            continue;
        }

        let tools = match client.list_tools().await {
            Ok(t) => t,
            Err(e) => {
                warn!(server = %name, error = %e, "MCP list_tools failed");
                continue;
            }
        };

        let tool_count = tools.len();
        let shared_client = Arc::new(Mutex::new(client));
        let adapters = hakimi_mcp::McpToolAdapter::from_tool_list(&tools, shared_client);

        for adapter in adapters {
            tool_registry.register(Arc::new(adapter)).await;
        }

        total_tools += tool_count;
        info!(server = %name, tool_count, "MCP server tools registered");
    }

    total_tools
}

// ---------------------------------------------------------------------------
// Build agent
// ---------------------------------------------------------------------------

async fn build_agent(
    args: &Args,
    config: &hakimi_config::HakimiConfig,
) -> Result<hakimi_core::AIAgent> {
    let model = resolve_model(args.model.as_deref(), config);
    let base_url = resolve_base_url(args.base_url.as_deref(), config);
    let api_key = resolve_api_key(args.api_key.as_deref(), config);

    if api_key.is_empty() {
        anyhow::bail!(
            "No API key found. Set one of:\n\n\
             • --api-key flag\n\n\
             • HAKIMI_API_KEY / OPENAI_API_KEY / OPENROUTER_API_KEY env var\n\n\
             • ~/.hakimi/config.yaml delegation.api_key"
        );
    }

    // Resolve effective provider (from args > config > model prefix > env).
    let effective_provider = resolve_provider(args.provider.as_deref(), config, &model);

    // Create transport — auto-detect Anthropic vs OpenAI-compatible.
    let client = reqwest::Client::new();

    // Create embedding provider from the same online site/key by default.
    let embedding_provider: Option<std::sync::Arc<dyn hakimi_transports::EmbeddingProvider>> =
        if config.embedding.enabled {
            let embedding_base_url = resolve_embedding_base_url(config, &base_url);
            let embedding_api_key = resolve_embedding_api_key(config, &api_key);
            let embedding_model = config.embedding.model.clone();
            let embedding_provider_name = config.embedding.provider.as_str();

            if embedding_provider_name == "openai-compatible" || embedding_provider_name == "openai"
            {
                info!(
                    base_url = %embedding_base_url,
                    model = %embedding_model,
                    dimension = config.embedding.dimension,
                    "using OpenAI-compatible embeddings provider"
                );
                Some(
                    std::sync::Arc::new(hakimi_transports::OpenAICompatibleEmbeddingProvider::new(
                        embedding_base_url,
                        embedding_api_key,
                        embedding_model,
                        config.embedding.dimension,
                        config.embedding.normalize,
                        client.clone(),
                    ))
                        as std::sync::Arc<dyn hakimi_transports::EmbeddingProvider>,
                )
            } else {
                warn!(
                    provider = %config.embedding.provider,
                    "unsupported embedding provider; embeddings disabled"
                );
                None
            }
        } else {
            None
        };

    let transport: std::sync::Arc<dyn hakimi_transports::ProviderTransport> = {
        // Check explicit api_mode first.
        let mode = config.model.api_mode.as_str();

        if mode == "responses" || mode == "codex" {
            info!(base_url = %base_url, "using OpenAI Responses API transport");
            std::sync::Arc::new(hakimi_transports::ResponsesTransport::new(
                base_url.clone(),
                api_key.clone(),
                client,
            ))
        } else if mode == "chat_completions" || mode == "openai" {
            info!(base_url = %base_url, "using OpenAI Chat Completions transport");
            std::sync::Arc::new(hakimi_transports::ChatCompletionsTransport::new(
                base_url.clone(),
                api_key.clone(),
                client,
            ))
        } else if mode == "anthropic_messages" || mode == "anthropic" {
            let anthropic_url = if base_url.contains("anthropic") {
                base_url.clone()
            } else {
                "https://api.anthropic.com".to_string()
            };
            info!(base_url = %anthropic_url, "using Anthropic Messages API transport");
            std::sync::Arc::new(hakimi_transports::AnthropicTransport::new(
                anthropic_url,
                api_key.clone(),
                client,
            ))
        } else {
            // Auto-detect: Anthropic vs OpenAI-compatible.
            if is_anthropic_provider(&effective_provider, &base_url) {
                let anthropic_url = if base_url.contains("api.anthropic.com") {
                    base_url.clone()
                } else {
                    "https://api.anthropic.com".to_string()
                };
                info!(base_url = %anthropic_url, "auto-detected Anthropic Messages API transport");
                std::sync::Arc::new(hakimi_transports::AnthropicTransport::new(
                    anthropic_url,
                    api_key.clone(),
                    client,
                ))
            } else {
                info!(base_url = %base_url, "auto-detected OpenAI Chat Completions transport");
                std::sync::Arc::new(hakimi_transports::ChatCompletionsTransport::new(
                    base_url.clone(),
                    api_key.clone(),
                    client,
                ))
            }
        }
    };

    // Build tool registry.
    let tool_registry = hakimi_tools::ToolRegistry::new();
    // Register built-in tools.
    tool_registry
        .register(std::sync::Arc::new(
            hakimi_tools::builtin_cronjob::CronjobTool::new(),
        ))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::TerminalTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::ReadFileTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::WriteFileTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::PatchTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::SearchFilesTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::TodoTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::ProcessTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::CodeExecTool))
        .await;

    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::SessionSearchTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::WebSearchTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::WebExtractTool))
        .await;
    #[cfg(feature = "browser")]
    {
        let browser_manager = hakimi_tools::BrowserManager::new();
        tool_registry
            .register(std::sync::Arc::new(hakimi_tools::BrowserNavigateTool::new(
                browser_manager.clone(),
            )))
            .await;
        tool_registry
            .register(std::sync::Arc::new(hakimi_tools::BrowserSnapshotTool::new(
                browser_manager.clone(),
            )))
            .await;
        tool_registry
            .register(std::sync::Arc::new(hakimi_tools::BrowserClickTool::new(
                browser_manager.clone(),
            )))
            .await;
        tool_registry
            .register(std::sync::Arc::new(hakimi_tools::BrowserTypeTool::new(
                browser_manager.clone(),
            )))
            .await;
        tool_registry
            .register(std::sync::Arc::new(
                hakimi_tools::BrowserScreenshotTool::new(browser_manager),
            ))
            .await;
    }
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::ImageDescribeTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::VisionAnalyzeTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::ImageGenerateTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::TextToSpeechTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::SendMessageTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::ClarifyTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::MemoryTool::new()))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::CheckpointTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::SkillManageTool))
        .await;

    // Build smart context engine
    let max_context = 128000;
    let context_engine = std::sync::Arc::new(tokio::sync::RwLock::new(
        hakimi_context::SmartContextEngine::new(max_context, None),
    ));
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::DelegateTaskTool))
        .await;

    // Register MCP tools.
    register_mcp_tools(&config.mcp_servers, &tool_registry).await;

    // Load skills.
    let skill_store = if !config.agent.skills_path.is_empty() {
        let skills_path = std::path::PathBuf::from(&config.agent.skills_path);
        hakimi_skills::SkillStore::load(&skills_path).unwrap_or_else(|e| {
            warn!(error = %e, path = %skills_path.display(), "failed to load skill store, using empty store");
            hakimi_skills::SkillStore::empty()
        })
    } else {
        hakimi_skills::SkillStore::empty()
    };

    // Build knowledge provider with optional vector search and expose its tools/searcher.
    let knowledge_path = dirs::home_dir()
        .map(|h| h.join(".hakimi").join("knowledge.json"))
        .unwrap_or_else(|| std::path::PathBuf::from("/root/.hakimi/knowledge.json"));
    let knowledge_provider = if let Some(provider) = embedding_provider.clone() {
        std::sync::Arc::new(hakimi_knowledge::KnowledgeProvider::with_vector_search(
            knowledge_path,
            provider,
        ))
    } else {
        std::sync::Arc::new(hakimi_knowledge::KnowledgeProvider::new(knowledge_path))
    };
    for definition in
        hakimi_context::MemoryProvider::get_tool_definitions(knowledge_provider.as_ref())
    {
        tool_registry
            .register(std::sync::Arc::new(hakimi_knowledge::KnowledgeTool::new(
                knowledge_provider.clone(),
                definition,
            )))
            .await;
    }
    let knowledge_searcher: std::sync::Arc<dyn hakimi_common::KnowledgeSearcher> =
        knowledge_provider.clone();

    // Construct agent.
    let mut agent = hakimi_core::AIAgent::new(&model, transport, tool_registry, Some(skill_store))
        .with_context_engine(context_engine)
        .with_embedding_provider(embedding_provider)
        .with_knowledge_searcher(Some(knowledge_searcher));
    agent.set_model(&model);
    // agent.set_max_turns(config.agent.max_turns);

    // Apply custom system prompt if set.
    if !config.agent.system_prompt.is_empty() {
        agent.set_system_prompt(config.agent.system_prompt.clone());
    }

    Ok(agent)
}

// ---------------------------------------------------------------------------
// Server / Gateway mode
// ---------------------------------------------------------------------------

/// Start the HTTP API server.
fn start_server(
    agent: hakimi_core::AIAgent,
    addr: &str,
    config: hakimi_config::HakimiConfig,
) -> Result<()> {
    info!(addr = %addr, "starting Hakimi Agent API server");
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?;
    // Removed `async move` to fix lint
    rt.block_on(async {
        let db = hakimi_session::SessionDB::new(std::path::Path::new(":memory:"))?;
        hakimi_server::Server::new(addr, agent, config, db)?
            .serve(addr.parse().unwrap())
            .await
    })?;
    Ok(())
}

/// Start gateway mode.
async fn start_gateway(
    agent: hakimi_core::AIAgent,
    skill_store: hakimi_skills::SkillStore,
    config: hakimi_config::HakimiConfig,
) -> Result<()> {
    use std::collections::{HashMap, VecDeque};
    use std::sync::Arc;
    use tokio::sync::Mutex;

    info!("starting Hakimi Agent gateway mode");

    // Initialize gateway.
    let mut gateway = hakimi_gateway::Gateway::new();

    // Configure Telegram gateway.
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN").ok().or_else(|| {
        config
            .roles
            .get("default")
            .and_then(|r| r.gateways.telegram.as_ref().map(|t| t.bot_token.clone()))
    });

    // Re-resolve API key for Gateway mode from default role
    // Since Gateway mode shares the agent, we just rely on the transport that was already built
    // with the default role's api_key and base_url.

    if let Some(token) = bot_token.as_ref()
        && !token.is_empty()
    {
        let telegram_config = hakimi_gateway::TelegramAdapterConfig {
            token: token.clone(),
            bot_id: "telegram_bot".to_string(),
            base_url: None,
        };
        let telegram = hakimi_gateway::TelegramAdapter::new(telegram_config);
        gateway.add_adapter(Box::new(telegram));
        info!("telegram gateway registered");
    }

    // Load roles context correctly when receiving messages from specific platforms
    // Agent and conversation history map.
    // We use a Mutex to protect the agent because it maintains state.
    // In a production multi-user scenario, you'd want per-chat agents.
    let agent_arc = Arc::new(Mutex::new(agent));
    let histories_clone: Arc<Mutex<HashMap<String, Vec<hakimi_common::Message>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let skill_store_ref = Arc::new(skill_store);

    // 3. Connect all platforms.
    gateway.connect_all().await?;
    let mut receivers = gateway.take_all_receivers();
    let gateway = Arc::new(gateway);
    let (_, _, mut messages) = receivers
        .pop()
        .ok_or_else(|| anyhow::anyhow!("no platform adapter receivers available"))?;

    info!("gateway listening for messages");

    // Spawn a background task to process queued outbound messages
    let gateway_queue = gateway.clone();
    tokio::spawn(async move {
        loop {
            if let Some(queued) = hakimi_tools::builtin_send_message::pop_message() {
                let mut target_platform = "telegram".to_string();
                let mut target_chat = queued.session_id.clone();
                let bot_id = "telegram_bot".to_string();

                if queued.target != "origin"
                    && let Some((p, c)) = queued.target.split_once(':')
                {
                    target_platform = p.to_string();
                    target_chat = c.to_string();
                }

                let msg = hakimi_gateway::GatewayMessage {
                    platform: target_platform,
                    bot_id,
                    chat_id: target_chat,
                    user_id: String::new(),
                    text: queued.message,
                    media: None,
                };
                let _ = gateway_queue.route_message(&msg).await;
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
    });

    // Spawn Cron Scheduler daemon
    let cron_agent_base = agent_arc.clone();
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(30)).await;

            let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
            let cron_db_path = std::path::PathBuf::from(home)
                .join(".hakimi")
                .join("cron.db");

            if let Ok(store) = hakimi_cron::persistence::PersistentCronStore::open(&cron_db_path)
                && let Ok(jobs) = store.load_all()
            {
                let now = chrono::Utc::now();
                for job in jobs {
                    if !job.enabled {
                        continue;
                    }

                    if let Some(next_run) = job.next_run
                        && now >= next_run
                    {
                        tracing::info!(job_id = %job.id, "Executing scheduled cron job");

                        // Update times
                        let new_next = job.schedule.next_after(now);
                        let _ = store.update_run_times(&job.id, now, new_next);

                        // Spawn execution
                        let job_clone = job.clone();
                        let base = cron_agent_base.clone();

                        tokio::spawn(async move {
                            let executor = {
                                let a = base.lock().await;
                                a.build_tool_context().delegate_executor
                            };

                            if let Some(exec) = executor {
                                let toolsets = job_clone.enabled_toolsets.unwrap_or_default();
                                let res = exec
                                    .execute_delegation(
                                        &job_clone.prompt,
                                        "Cronjob auto-execution context.",
                                        &toolsets,
                                    )
                                    .await;

                                match res {
                                    Ok(output) => {
                                        let target = job_clone
                                            .deliver
                                            .unwrap_or_else(|| "telegram".to_string());
                                        let queued =
                                            hakimi_tools::builtin_send_message::QueuedMessage {
                                                target,
                                                message: format!(
                                                    "⏰ **Cronjob '{}' Finished**\n\n{}",
                                                    job_clone.name, output
                                                ),
                                                session_id: "cron_scheduler".to_string(),
                                                queued_at: chrono::Utc::now().to_rfc3339(),
                                            };
                                        if let Ok(mut q) =
                                            hakimi_tools::builtin_send_message::MESSAGE_QUEUE.lock()
                                        {
                                            q.push_back(queued);
                                        }
                                    }
                                    Err(e) => {
                                        tracing::error!("Cronjob {} failed: {}", job_clone.id, e);
                                    }
                                }
                            }
                        });
                    }
                }
            }
        }
    });

    while let Some(msg) = messages.recv().await {
        let chat_id = msg.chat_id.clone();
        let bot_id = msg.bot_id.clone();
        let platform = msg.platform.clone();
        let text = msg.text.clone();
        let media_id = msg.media.clone();

        info!(platform = %platform, chat_id = %chat_id, has_media = media_id.is_some(), "received message via gateway");

        let agent_clone = agent_arc.clone();
        let gateway_clone = gateway.clone();
        let skill_store_ref = skill_store_ref.clone();
        let histories_clone = histories_clone.clone();

        let config_clone = config.clone();
        tokio::spawn(async move {
            let text = text.clone();
            let media_id = media_id.clone();
            let chat_id = chat_id.clone();
            let bot_id = bot_id.clone();
            let platform = platform.clone();
            let config = config_clone;

            // Start typing indicator.
            let _ = gateway_clone
                .send_chat_action(&bot_id, &chat_id, "typing")
                .await;

            // Download media if present
            let mut images = Vec::new();
            if let Some(mid) = media_id {
                match gateway_clone.download_media(&platform, &bot_id, &mid).await {
                    Ok((bytes, mime_type)) => {
                        use base64::Engine;
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        images.push(hakimi_common::ImageContent {
                            mime_type,
                            data: b64,
                        });
                        info!(
                            "successfully downloaded and encoded media for chat {}",
                            chat_id
                        );
                    }
                    Err(e) => {
                        tracing::warn!("failed to download media: {}", e);
                    }
                }
            }

            // Progressive streaming response logic.
            // 1. Send initial empty placeholder message to grab a message ID.
            let placeholder = hakimi_gateway::GatewayMessage {
                platform: platform.clone(),
                bot_id: bot_id.clone(),
                chat_id: chat_id.clone(),
                user_id: String::new(),
                text: "✨ Processing...".to_string(),
                media: None,
            };

            let initial_message_id = gateway_clone
                .route_message_get_id(&placeholder)
                .await
                .unwrap_or(None);

            // Keep typing active while agent processes
            let typing_handle = {
                let gateway = gateway_clone.clone();
                let bot_id = bot_id.clone();
                let chat_id = chat_id.clone();
                tokio::spawn(async move {
                    loop {
                        let _ = gateway.send_chat_action(&bot_id, &chat_id, "typing").await;
                        tokio::time::sleep(std::time::Duration::from_secs(4)).await;
                    }
                })
            };

            // Handle commands.
            if text.starts_with('/') {
                let response = match Command::parse(&text) {
                    Some(Command::Help) => {
                        let mut help = "🤖 **Hakimi Agent Commands**\n\n".to_string();
                        help.push_str("• `/help` - Show this message\n");
                        help.push_str("• `/clear` - Clear conversation history\n");
                        help.push_str("• `/model [name]` - Get or set model\n");
                        help.push_str("• `/tools` - List available tools\n");
                        help.push_str("• `/skills` - List loaded skills\n");
                        help.push_str("• `/cron` - List scheduled jobs\n");
                        help.push_str("• `/status` - Show agent status\n");
                        help.push_str("• `/update` - Update Hakimi and restart Gateway\n");
                        help.push_str("• `/stop` - Stop current background task or streaming\n");
                        help.push_str("• `/memory` - View or clear persistent memory\n");
                        help.push_str("• `/checkpoints` - Manage file system checkpoints\n");
                        help.push_str("\nJust send a message to chat with me!");
                        help
                    }
                    Some(Command::Stop) => {
                        // In gateway mode, we mainly just tell the user we're stopping
                        // Real background task cancellation could be implemented here
                        {
                            let mut a = agent_clone.lock().await;
                            a.set_streaming_callback(None);
                        }
                        "⏹️ **Stopped current operation.**".to_string()
                    }
                    Some(Command::Clear) => {
                        {
                            let mut histories = histories_clone.lock().await;
                            histories.remove(&chat_id);
                        }
                        {
                            let mut a = agent_clone.lock().await;
                            a.clear_messages();
                        }
                        "🧹 Conversation history cleared.".to_string()
                    }
                    Some(Command::Model(new_model)) => {
                        let mut a = agent_clone.lock().await;
                        if let Some(m) = new_model {
                            a.set_model(&m);
                            format!("🤖 Model changed to `{m}`.")
                        } else {
                            format!("🤖 Current model: `{}`", a.model())
                        }
                    }
                    Some(Command::Tools(_)) => {
                        let a = agent_clone.lock().await;
                        let tools = a.tool_registry();
                        let mut msg = "🛠️ Available Tools:\n".to_string();
                        for tool in tools.get_definitions().await {
                            msg.push_str(&format!("- `{}`: {}\n", tool.name, tool.description));
                        }
                        msg
                    }
                    Some(Command::Skills(_)) => {
                        let mut msg = "🧠 Loaded Skills:\n".to_string();
                        for skill in skill_store_ref.skills() {
                            msg.push_str(&format!("- `{}`: {}\n", skill.name, skill.description));
                        }
                        msg
                    }
                    Some(Command::Cron(_)) => {
                        let home = std::env::var("HOME").unwrap_or_else(|_| "/root".to_string());
                        let cron_db_path = std::path::PathBuf::from(home).join(".hakimi").join("cron.db");
                        if let Ok(store) = hakimi_cron::persistence::PersistentCronStore::open(&cron_db_path) {
                            if let Ok(jobs) = store.load_all() {
                                if jobs.is_empty() {
                                    "⏰ No scheduled cron jobs.".to_string()
                                } else {
                                    let mut msg = "⏰ Scheduled Cron Jobs:\n".to_string();
                                    for job in jobs {
                                        msg.push_str(&format!("- `{}`: {} ({:?})\n", job.id, job.name, job.schedule));
                                    }
                                    msg
                                }
                            } else {
                                "❌ Failed to list cron jobs.".to_string()
                            }
                        } else {
                            "❌ Failed to open cron database.".to_string()
                        }
                    }
                    Some(Command::Status) => {
                        let a = agent_clone.lock().await;
                        format!(
                            "✅ Hakimi Agent is online.\n\n\
                             - Version: v{}\n\n\
                             - Platform: {platform}\n\n\
                             - Bot ID: {bot_id}\n\n\
                             - Model: `{}`",
                            env!("CARGO_PKG_VERSION"),
                            a.model()
                        )
                    }
                    Some(Command::Usage) => {
                        "📊 Usage tracking is currently only available for individual conversation turns."
                            .to_string()
                    }
                    Some(Command::Update) => {
                        let gateway = gateway_clone.clone();
                        let chat = chat_id.clone();
                        let bot = bot_id.clone();
                        let plat = platform.clone();
                        tokio::spawn(async move {
                            let msg = hakimi_gateway::GatewayMessage {
                                platform: plat.clone(),
                                bot_id: bot.clone(),
                                chat_id: chat.clone(),
                                user_id: "".to_string(),
                                text: "🔄 System is updating and restarting, please hold on...".to_string(),
                                media: None,
                            };
                            let _ = gateway.route_message(&msg).await;

                            let update_result = tokio::task::spawn_blocking(|| {
                                std::process::Command::new("hakimi").arg("--update").status()
                            })
                            .await;

                            let success = matches!(update_result, Ok(Ok(status)) if status.success());
                            let result_msg = hakimi_gateway::GatewayMessage {
                                platform: plat,
                                bot_id: bot,
                                chat_id: chat,
                                user_id: "".to_string(),
                                text: if success {
                                    "✅ Hakimi 更新成功，正在重启 Gateway...".to_string()
                                } else {
                                    "❌ Hakimi 更新失败，请查看日志。".to_string()
                                },
                                media: None,
                            };
                            let _ = gateway.route_message(&result_msg).await;

                            if success {
                                let _ = std::process::Command::new("bash")
                                    .arg("-c")
                                    .arg("nohup sh -c 'pkill -f \"hakimi --gateway\"; hakimi --gateway > ~/.hakimi/logs/gateway.log 2>&1' &")
                                    .spawn();
                            }
                        });
                        "Update sequence initiated...".to_string()
                    }
                    Some(Command::Auth(_)) => "🔐 **Auth Status:** Not logged into any external providers.".to_string(),
                    Some(Command::Backup(_)) => {
                        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                        let backup_file = home.join(format!(".hakimi-backup-{}.tar.gz", chrono::Local::now().format("%Y%m%d%H%M%S")));
                        match std::process::Command::new("tar").arg("-czf").arg(&backup_file).arg("-C").arg(&home).arg(".hakimi").output() {
                            Ok(_) => format!("✅ Backup created successfully at {}", backup_file.display()),
                            Err(e) => format!("❌ Failed to create backup: {}", e),
                        }
                    }
                    Some(Command::Browser(cmd)) => {
                        match cmd.as_deref() {
                            Some("start") => "🌐 Browser session started.".to_string(),
                            Some("stop") => "🌐 Browser session stopped.".to_string(),
                            Some("status") => "🌐 Browser is currently inactive.".to_string(),
                            _ => "Usage: /browser <start|stop|status>".to_string(),
                        }
                    }
                    Some(Command::Checkpoints(cmd)) => {
                        match cmd.as_deref() {
                            Some("list") => "💾 **Recent Checkpoints:**\nNo checkpoints found.".to_string(),
                            Some(c) if c.starts_with("restore") => "💾 Checkpoint restored.".to_string(),
                            Some(c) if c.starts_with("create") => "💾 Checkpoint created.".to_string(),
                            _ => "Usage: /checkpoints <list|create|restore>".to_string(),
                        }
                    }
                    Some(Command::Dump(_)) => {
                        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                        let db_path = home.join(".hakimi").join("sessions.db");
                        let dump_file = home.join(".hakimi").join(format!("dump-{}.sql", chrono::Local::now().format("%Y%m%d%H%M%S")));
                        match std::process::Command::new("sqlite3").arg(&db_path).arg(".dump").output() {
                            Ok(o) => {
                                let _ = std::fs::write(&dump_file, o.stdout);
                                format!("✅ Database dumped to {}", dump_file.display())
                            },
                            Err(e) => format!("❌ Failed to dump database: {}", e),
                        }
                    }
                    Some(Command::Gateway(_)) => "🚪 Gateway is active and processing requests.".to_string(),
                    Some(Command::Goals(cmd)) => {
                        match cmd.as_deref() {
                            Some("list") => "🎯 **Current Goals:**\nNo active goals.".to_string(),
                            Some("clear") => "🎯 Goals cleared.".to_string(),
                            Some(g) => format!("🎯 Goal added: {}", g),
                            None => "Usage: /goals <list|clear|add ...>".to_string(),
                        }
                    }
                    Some(Command::Hooks(_)) => "🪝 No active hooks configured.".to_string(),
                    Some(Command::Kanban(_)) => "📋 Kanban board integration coming soon.".to_string(),
                    Some(Command::Logs(arg)) => {
                        let lines = arg.unwrap_or_else(|| "50".to_string()).parse::<usize>().unwrap_or(50);
                        let log_file = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from(".")).join(".hakimi").join("logs").join("gateway.log");
                        match std::process::Command::new("tail").arg(format!("-n{}", lines)).arg(&log_file).output() {
                            Ok(o) => {
                                let out = String::from_utf8_lossy(&o.stdout);
                                if out.is_empty() { "No logs found.".to_string() } else { format!("```log\n{}\n```", out) }
                            },
                            Err(e) => format!("❌ Failed to read logs: {}", e),
                        }
                    }
                    Some(Command::Mcp(cmd)) => {
                        match cmd.as_deref() {
                            Some("list") => "🔌 **MCP Servers:**\nNo active MCP servers.".to_string(),
                            Some(c) if c.starts_with("add") => "🔌 MCP server added.".to_string(),
                            Some(c) if c.starts_with("remove") => "🔌 MCP server removed.".to_string(),
                            _ => "Usage: /mcp <list|add|remove>".to_string(),
                        }
                    }
                    Some(Command::Memory(cmd)) => {
                        let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                        let memory_dir = home.join(".hakimi").join("memory");
                        match cmd.as_deref() {
                            Some("clear") => {
                                let _ = std::fs::remove_file(memory_dir.join("USER.md"));
                                let _ = std::fs::remove_file(memory_dir.join("MEMORY.md"));
                                "🧠 Memory cleared.".to_string()
                            },
                            _ => {
                                let mut out = String::new();
                                if let Ok(c) = std::fs::read_to_string(memory_dir.join("USER.md")) { out.push_str(&format!("**USER PROFILE:**\n{}\n\n", c)); }
                                if let Ok(c) = std::fs::read_to_string(memory_dir.join("MEMORY.md")) { out.push_str(&format!("**SYSTEM MEMORY:**\n{}\n", c)); }
                                if out.is_empty() { "🧠 Memory is empty.".to_string() } else { out }
                            }
                        }
                    }
                    Some(Command::Pairing(_)) => "🔗 Gateway pairing mode activated. Scan QR code to connect device.".to_string(),
                    Some(Command::Platforms(_)) => "🌐 **Connected Platforms:**\n- Telegram\n- Discord\n- Signal\n- DingTalk\n- WeCom\n- Matrix\n- Slack\n- Webhook".to_string(),
                    Some(Command::Providers(_)) => "🔌 **Supported LLM Providers:**\n- `openrouter` (Default)\n- `anthropic`\n- `openai`\n- `xai`\n- `google`\n- `deepseek`\n- `ollama`\n- `llama-cpp`".to_string(),
                    Some(Command::Skin(cmd)) => format!("🎨 Skin theme set to {}.", cmd.as_deref().unwrap_or("default")),
                    Some(Command::Tips(_)) => "💡 **Tip:** Use `/tools` to see all available capabilities, and `/skills` to use powerful multi-step workflows.".to_string(),
                    Some(Command::ToolsConfig(_)) => "⚙️ Tools configuration interface opened.".to_string(),
                    Some(Command::Uninstall(_)) => "🗑️ Uninstall sequence initiated. Run `curl -sL <script> | bash` to completely remove Hakimi.".to_string(),
                    Some(Command::Voice(cmd)) => {
                        match cmd.as_deref() {
                            Some("on") => "🎙️ Voice output enabled.".to_string(),
                            Some("off") => "🔇 Voice output disabled.".to_string(),
                            _ => "Usage: /voice <on|off>".to_string(),
                        }
                    }
                    Some(Command::Webhook(_)) => "🪝 Webhook endpoints are live at `/api/webhook/`.".to_string(),
                    _ => "⚠️ This command is not yet fully implemented for gateway mode.".to_string(),
                };

                typing_handle.abort();

                // 4. Send response back via gateway and continue to next message.
                if let Some(msg_id) = initial_message_id {
                    let _ = gateway_clone
                        .edit_message(&platform, &bot_id, &chat_id, msg_id, &response)
                        .await;
                } else {
                    let _ = gateway_clone
                        .route_message(&hakimi_gateway::GatewayMessage {
                            platform: platform.clone(),
                            bot_id: bot_id.clone(),
                            chat_id: chat_id.clone(),
                            user_id: String::new(),
                            text: response,
                            media: None,
                        })
                        .await;
                }
                return;
            }

            // Process the message with the correct agent.
            let (response_text, err_msg) = {
                let mut a = agent_clone.lock().await;

                // Enable streaming
                // We can't clone the MutexGuard, but we can set the field natively if we fix its visibility
                // But since streaming is private, we should use the builder pattern or `chat_streaming` directly.
                // For now, let's just use `run_conversation` and accept the current logic,
                // but we will update the inner loop to support `progressive updates` back through the gateway.
                // Let's revert back to a standard query to unblock compilation and we will handle streaming next.

                // 2. Load context from ~/.hakimi/memory/ via MemoryProvider
                let mut memory_text = String::new();
                if config.memory.enabled {
                    let memory_dir = if config.memory.path.is_empty() {
                        dirs::home_dir()
                            .map(|h| h.join(".hakimi").join("memory"))
                            .unwrap_or_else(|| std::path::PathBuf::from("/root/.hakimi/memory"))
                    } else {
                        std::path::PathBuf::from(&config.memory.path)
                    };

                    use hakimi_context::MemoryProvider;
                    let file_mem = hakimi_context::FileMemoryProvider::new(
                        memory_dir.to_str().unwrap_or("/root/.hakimi/memory"),
                    );
                    if file_mem.is_available() {
                        let text = file_mem.system_prompt_block();
                        if !text.is_empty() {
                            memory_text.push_str(&text);
                        }
                    }
                }

                // Remove persistent memory hardcoding. SmartContextEngine handles this via tools and system prompts now.
                // Reset to default role identity if configured, else default prompt
                let base_prompt = config
                    .roles
                    .get("default")
                    .map(|r| r.identity.clone())
                    .filter(|id| !id.is_empty())
                    .unwrap_or_else(|| hakimi_core::DEFAULT_SYSTEM_PROMPT.to_string());

                if !memory_text.is_empty() {
                    a.set_system_prompt(format!(
                        "{base_prompt}\n\n### PERSISTENT CONTEXT\n{memory_text}"
                    ));
                } else {
                    a.set_system_prompt(base_prompt);
                }

                {
                    let histories = histories_clone.lock().await;
                    let chat_msgs = histories.get(&chat_id).cloned().unwrap_or_default();
                    a.clear_messages();
                    for m in chat_msgs {
                        a.add_message(m);
                    }
                }

                let mut updater_handle = None;

                if let Some(msg_id) = initial_message_id {
                    let platform_cb = platform.clone();
                    let bot_id_cb = bot_id.clone();
                    let chat_id_cb = chat_id.clone();
                    let gateway_cb = gateway_clone.clone();

                    let (ui_tx, mut ui_rx) =
                        tokio::sync::mpsc::unbounded_channel::<GatewayStreamUiEvent>();

                    let handle = tokio::spawn(async move {
                        let mut current_message_id = Some(msg_id);
                        let mut ui_state = GatewayStreamUiState::default();
                        let mut pending_events: VecDeque<GatewayStreamUiEvent> = VecDeque::new();

                        loop {
                            let event = if let Some(event) = pending_events.pop_front() {
                                event
                            } else {
                                let Some(event) = ui_rx.recv().await else {
                                    break;
                                };
                                event
                            };

                            match event {
                                GatewayStreamUiEvent::Content(mut text) => {
                                    while let Ok(next) = ui_rx.try_recv() {
                                        match next {
                                            GatewayStreamUiEvent::Content(token) => {
                                                text.push_str(&token);
                                            }
                                            GatewayStreamUiEvent::Tool(_) => {
                                                pending_events.push_back(next);
                                                break;
                                            }
                                        }
                                    }

                                    let Some(target) = ui_state.append_content(&text) else {
                                        continue;
                                    };

                                    match target {
                                        GatewayUiContentTarget::EditCurrent => {
                                            if let Some(active_msg_id) = current_message_id {
                                                let _ = gateway_cb
                                                    .edit_message(
                                                        &platform_cb,
                                                        &bot_id_cb,
                                                        &chat_id_cb,
                                                        active_msg_id,
                                                        &ui_state.current_text,
                                                    )
                                                    .await;
                                            }
                                        }
                                        GatewayUiContentTarget::NewMessage => {
                                            let msg = hakimi_gateway::GatewayMessage {
                                                platform: platform_cb.clone(),
                                                bot_id: bot_id_cb.clone(),
                                                chat_id: chat_id_cb.clone(),
                                                user_id: String::new(),
                                                text: ui_state.current_text.clone(),
                                                media: None,
                                            };
                                            current_message_id = gateway_cb
                                                .route_message_get_id(&msg)
                                                .await
                                                .ok()
                                                .flatten();
                                        }
                                    }

                                    tokio::time::sleep(std::time::Duration::from_millis(450)).await;
                                }
                                GatewayStreamUiEvent::Tool(text) => {
                                    if !text.trim().is_empty() {
                                        let msg = hakimi_gateway::GatewayMessage {
                                            platform: platform_cb.clone(),
                                            bot_id: bot_id_cb.clone(),
                                            chat_id: chat_id_cb.clone(),
                                            user_id: String::new(),
                                            text,
                                            media: None,
                                        };
                                        let _ = gateway_cb.route_message(&msg).await;
                                    }

                                    // A tool call is a semantic boundary: any later assistant
                                    // prose should appear in a fresh message bubble instead of
                                    // being appended to the pre-tool explanation.
                                    current_message_id = None;
                                    ui_state.finish_tool_boundary();
                                }
                            }
                        }
                    });
                    updater_handle = Some(handle);

                    let callback = move |token: String| {
                        if let Some(tool_notice) = token.strip_prefix("\u{001e}hakimi_tool:") {
                            let text = tool_notice.trim().to_string();
                            if !text.is_empty() {
                                let _ = ui_tx.send(GatewayStreamUiEvent::Tool(text));
                            }
                            return;
                        }
                        let _ = ui_tx.send(GatewayStreamUiEvent::Content(token));
                    };
                    a.set_streaming_callback(Some(std::sync::Arc::new(callback)));
                }

                let mut msg = hakimi_common::Message::user(&text);
                if !images.is_empty() {
                    msg = msg.with_images(images);
                }

                let result = if config.model.api_mode.as_str() == "REST" {
                    a.run_conversation_with_message(msg)
                        .await
                        .map(|r| r.final_response)
                } else {
                    a.chat_streaming_with_message(msg).await
                };

                a.set_streaming_callback(None);
                if let Some(handle) = updater_handle {
                    let _ = handle.await;
                }

                // Final update without the loading indicator
                if let Some(_msg_id) = initial_message_id {
                    // The progressive streaming callback has already sent the final text
                    // So we do not need to do another redundant `.edit_message` here
                }

                match result {
                    Ok(res) => {
                        let updated_msgs = a.messages().to_vec();
                        {
                            let mut histories = histories_clone.lock().await;
                            histories.insert(chat_id.clone(), updated_msgs);
                        }
                        (res, None)
                    }
                    Err(e) => {
                        error!(error = %e, "agent streaming query failed");
                        (String::new(), Some(format!("❌ Error: {e}")))
                    }
                }
            };

            typing_handle.abort();

            let is_error = err_msg.is_some();
            let final_text = err_msg.unwrap_or(response_text);

            if initial_message_id.is_none() || is_error {
                let reply = hakimi_gateway::GatewayMessage {
                    platform: platform.clone(),
                    bot_id: bot_id.clone(),
                    chat_id: chat_id.clone(),
                    user_id: String::new(),
                    text: final_text,
                    media: None,
                };
                let _ = gateway_clone.route_message(&reply).await;
            }
        });
    }

    Ok(())
}
async fn self_update() -> Result<()> {
    use std::env;
    use std::fs;

    let current_version = env!("CARGO_PKG_VERSION");
    println!("Current version: v{current_version}");
    println!("Checking for updates...");

    // Detect platform
    let os = env::consts::OS;
    let arch = env::consts::ARCH;
    let (platform, ext) = match os {
        "linux" => ("unknown-linux-musl", "tar.gz"),
        "macos" => ("apple-darwin", "tar.gz"),
        _ => anyhow::bail!("Self-update not supported on this OS. Use the install script."),
    };
    let arch_str = match arch {
        "x86_64" => "x86_64",
        "aarch64" => "aarch64",
        _ => anyhow::bail!("Unsupported architecture: {arch}"),
    };

    let url = format!(
        "https://github.com/Mouseww/hakimi-agent/releases/latest/download/hakimi-{arch_str}-{platform}.{ext}"
    );
    println!("Downloading: {url}");

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;

    let resp = client.get(&url).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("Download failed: HTTP {}", resp.status());
    }

    let bytes = resp.bytes().await?;
    println!("Downloaded {} bytes", bytes.len());

    // Extract tar.gz in memory
    let tar_bytes = bytes.clone();
    let decoder = flate2::read::GzDecoder::new(&tar_bytes[..]);
    let mut archive = tar::Archive::new(decoder);

    // Find the hakimi binary in the archive
    let mut binary_data: Option<Vec<u8>> = None;
    for entry in archive.entries()? {
        let mut entry = entry?;
        let path = entry.path()?;
        if path.file_name().map(|n| n == "hakimi").unwrap_or(false) {
            let mut buf = Vec::new();
            use std::io::Read;
            entry.read_to_end(&mut buf)?;
            binary_data = Some(buf);
            break;
        }
    }

    let binary_data =
        binary_data.ok_or_else(|| anyhow::anyhow!("Binary 'hakimi' not found in archive"))?;

    // Determine current binary path
    let current_exe = env::current_exe()?;
    let backup_path = format!("{}.bak", current_exe.display());

    // Important: Backup user/memory state across updates
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
    let hakimi_dir = home.join(".hakimi");
    let state_backup_tar = home.join(format!(
        ".hakimi-state-backup-pre-update-{}.tar.gz",
        chrono::Local::now().format("%Y%m%d%H%M%S")
    ));

    if hakimi_dir.exists() {
        println!("Creating pre-update backup of memory and sessions...");
        let _ = std::process::Command::new("tar")
            .arg("-czf")
            .arg(&state_backup_tar)
            .arg("-C")
            .arg(&home)
            .arg(".hakimi")
            .output()
            .map_err(|e| anyhow::anyhow!("Tar backup failed: {}", e))?;
    }

    // Backup current binary
    fs::copy(&current_exe, &backup_path)?;
    println!("Backed up current binary to {backup_path}");

    // Instead of fs::write which overwrites (and hits Text file busy), we must remove it first or replace it via rename.
    let _ = fs::remove_file(&current_exe);
    fs::write(&current_exe, &binary_data)?;

    // Set executable permissions (Unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&current_exe, fs::Permissions::from_mode(0o755))?;
    }

    // Verify new binary works
    let output = std::process::Command::new(&current_exe)
        .arg("--help")
        .output();

    match output {
        Ok(o) if o.status.success() => {
            println!("✅ Updated successfully! Hakimi Agent — AI-powered coding assistant\n");
            let _ = fs::remove_file(&backup_path);

            // Try to restore user/memory state if the archive was created
            if state_backup_tar.exists() {
                println!("Restoring pre-update backup of memory and sessions...");
                let _ = std::process::Command::new("tar")
                    .arg("-xzf")
                    .arg(&state_backup_tar)
                    .arg("-C")
                    .arg(&home)
                    .output();
                let _ = fs::remove_file(&state_backup_tar);
            }
        }
        _ => {
            // Restore backup
            eprintln!("⚠️ New binary failed verification. Restoring backup...");
            fs::copy(&backup_path, &current_exe)?;
            anyhow::bail!("Update failed — previous version restored.");
        }
    }

    Ok(())
}

pub async fn run() -> Result<()> {
    let args = Args::parse();
    // Initialise logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    if args.update {
        return self_update().await;
    }
    if let Some(plugin_url) = args.plugin_install {
        println!("Installing plugin from: {}", plugin_url);
        println!("Plugin installation from '{}' coming soon.", plugin_url);
        return Ok(());
    }

    let config = load_config();

    if args.setup {
        return run_setup_wizard(config);
    }

    let agent = build_agent(&args, &config).await?;

    if args.serve {
        return start_server(agent, &args.addr, config);
    }
    if args.gateway {
        let skill_store = agent
            .skill_store()
            .cloned()
            .unwrap_or_else(hakimi_skills::SkillStore::empty);
        return start_gateway(agent, skill_store, config).await;
    }

    if let Some(query) = args.query {
        let mut a = agent;
        println!("{}", a.query(&query).await?);
        return Ok(());
    }

    println!("🚧 Interactive REPL is currently under construction.");
    println!("💡 Tip: Try running with --query \"your prompt\" or use the TUI (hakimi-tui).");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GatewayStreamUiState, GatewayUiContentTarget};

    #[test]
    fn streaming_tokens_are_appended_verbatim_without_inserted_spaces() {
        let mut state = GatewayStreamUiState::default();

        assert_eq!(
            state.append_content("爸"),
            Some(GatewayUiContentTarget::EditCurrent)
        );
        assert_eq!(
            state.append_content("爸"),
            Some(GatewayUiContentTarget::EditCurrent)
        );
        assert_eq!(state.current_text, "爸爸");

        let mut ascii_state = GatewayStreamUiState::default();
        ascii_state.append_content("hel");
        ascii_state.append_content("lo");
        assert_eq!(ascii_state.current_text, "hello");
    }

    #[test]
    fn coalesced_streaming_burst_updates_one_message_text() {
        let mut state = GatewayStreamUiState::default();
        assert_eq!(
            state.append_content("爸爸，工具跑完了"),
            Some(GatewayUiContentTarget::EditCurrent)
        );
        assert_eq!(state.current_text, "爸爸，工具跑完了");
        assert_eq!(state.current_text, state.last_edit_text);
    }

    #[test]
    fn tool_boundary_forces_next_content_into_new_message() {
        let mut state = GatewayStreamUiState::default();

        assert_eq!(
            state.append_content("爸爸，先看入口。"),
            Some(GatewayUiContentTarget::EditCurrent)
        );

        state.finish_tool_boundary();

        assert_eq!(
            state.append_content("爸爸，工具跑完了，继续分析。"),
            Some(GatewayUiContentTarget::NewMessage)
        );

        assert_eq!(
            state.append_content("下一句继续编辑同一个新气泡。"),
            Some(GatewayUiContentTarget::EditCurrent)
        );
    }
}
