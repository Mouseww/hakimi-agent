//! Hakimi Agent CLI entry point.
//!
//! Contains the clap [`Args`], configuration loading, agent construction, and
//! the interactive REPL / single-query / server modes so that both the
//! `hakimi-cli` binary and the thin `hakimi-agent` wrapper can share the same
//! implementation.

use std::io::{self, Write};

use anyhow::Result;
use clap::Parser;
use tokio::sync::RwLock;
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::Command;

// ---------------------------------------------------------------------------
// CLI arguments (clap)
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "hakimi", about = "Hakimi Agent — AI-powered coding assistant")]
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
}

// ---------------------------------------------------------------------------
// Banner
// ---------------------------------------------------------------------------

fn print_banner() {
    println!(r#"  _  _               _ _             _         "#);
    println!(r#" | || |__ _ __ _ __ (_) |_ _  _ __ _| |___ _ _ "#);
    println!(r#" | __ / _` / _| '  \| |  _| || / _` | / -_) '_|"#);
    println!(r#" |_||_\__,_\__|_|_|_|_|\__|\_,_\__,_|_\___|_|  "#);
    println!();
    println!("  Hakimi Agent v{}", env!("CARGO_PKG_VERSION"));
    println!("  Type /help for commands, /quit to exit.");
    println!();
}

// ---------------------------------------------------------------------------
// Help text
// ---------------------------------------------------------------------------

fn print_help() {
    println!("Commands:");
    println!("  /help            Show this help message");
    println!("  /quit, /exit     Exit the REPL");
    println!("  /clear           Clear the terminal screen");
    println!("  /model [name]    Switch or display the current model");
    println!("  /config [key]    Show or edit configuration");
    println!("  /resume [id]     Resume a previous session");
    println!("  /tools [name]    List or describe available tools");
    println!("  /skills [name]   List or describe available skills");
    println!("  /status          Show current session status");
    println!("  /usage           Show token usage statistics");
    println!("  /plugins [cmd]   Manage plugins, MCP servers, and templates");
    println!();
    println!("Any other input is sent as a message to the agent.");
}

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
    if let Some(p) = args_provider {
        if !p.is_empty() && p != "auto" {
            return p.to_string();
        }
    }
    // 2. Environment variable
    if let Ok(val) = std::env::var("HAKIMI_PROVIDER") {
        if !val.is_empty() && val != "auto" {
            return val;
        }
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
    if let Some(key) = args_key {
        if !key.is_empty() {
            return key.to_string();
        }
    }
    // 2. Environment variables
    for var in &[
        "HAKIMI_API_KEY",
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
    ] {
        if let Ok(val) = std::env::var(var) {
            if !val.is_empty() {
                info!(env_var = var, "using API key from environment");
                return val;
            }
        }
    }
    // 3. Config file model.api_key
    if !config.model.api_key.is_empty() {
        return config.model.api_key.clone();
    }
    // 4. Config file delegation api_key (as fallback)
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
    if let Some(url) = args_url {
        if !url.is_empty() {
            return url.to_string();
        }
    }
    // 2. Environment variable
    if let Ok(val) = std::env::var("HAKIMI_BASE_URL") {
        if !val.is_empty() {
            return val;
        }
    }
    // 3. Config
    if !config.model.base_url.is_empty() {
        return config.model.base_url.clone();
    }
    // 4. Default — OpenRouter is a reasonable default
    "https://openrouter.ai/api".to_string()
}

// ---------------------------------------------------------------------------
// Resolve model
// ---------------------------------------------------------------------------

fn resolve_model(args_model: Option<&str>, config: &hakimi_config::HakimiConfig) -> String {
    // 1. CLI argument
    if let Some(m) = args_model {
        if !m.is_empty() {
            return m.to_string();
        }
    }
    // 2. Environment variable
    if let Ok(val) = std::env::var("HAKIMI_MODEL") {
        if !val.is_empty() {
            return val;
        }
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
            unsafe { std::env::set_var(key, val); }
        }

        // Build args as &str slices
        let args: Vec<&str> = server_config.args.iter().map(|s| s.as_str()).collect();

        let mut client = match hakimi_mcp::McpClient::connect_stdio(&server_config.command, &args).await {
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
            "No API key found. Set one of:\n\
             • --api-key flag\n\
             • HAKIMI_API_KEY / OPENAI_API_KEY / OPENROUTER_API_KEY env var\n\
             • ~/.hakimi/config.yaml delegation.api_key"
        );
    }

    // Resolve effective provider (from args > config > model prefix > env).
    let effective_provider = resolve_provider(args.provider.as_deref(), config, &model);

    // Create transport — auto-detect Anthropic vs OpenAI-compatible.
    let client = reqwest::Client::new();
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
                info!(base_url = %base_url, "using OpenAI Chat Completions transport");
                std::sync::Arc::new(hakimi_transports::ChatCompletionsTransport::new(
                    base_url.clone(),
                    api_key.clone(),
                    client,
                ))
            }
        }
    };

    // Create context engine — choose smart (3-tier) or simple (truncation).
    let context_length = config.compression.context_length;
    let context_engine: std::sync::Arc<tokio::sync::RwLock<dyn hakimi_context::ContextEngine>> =
        if config.compression.engine == "simple" {
            info!(context_length, "using SimpleContextEngine (truncation)");
            std::sync::Arc::new(RwLock::new(
                hakimi_context::SimpleContextEngine::new(context_length),
            ))
        } else {
            let compression_model = Some(model.clone());
            info!(context_length, engine = "smart", "using SmartContextEngine (3-tier)");
            std::sync::Arc::new(RwLock::new(
                hakimi_context::SmartContextEngine::new(context_length, compression_model),
            ))
        };
    // Create tool registry and register ALL built-in tools.
    let tool_registry = hakimi_tools::ToolRegistry::new();
    let builtin_tools: Vec<std::sync::Arc<dyn hakimi_tools::Tool>> = vec![
        std::sync::Arc::new(hakimi_tools::ReadFileTool),
        std::sync::Arc::new(hakimi_tools::WriteFileTool),
        std::sync::Arc::new(hakimi_tools::TerminalTool),
        std::sync::Arc::new(hakimi_tools::SearchFilesTool),
        std::sync::Arc::new(hakimi_tools::PatchTool),
        std::sync::Arc::new(hakimi_tools::WebSearchTool),
        std::sync::Arc::new(hakimi_tools::WebExtractTool),
        std::sync::Arc::new(hakimi_tools::MemoryTool::new()),
        std::sync::Arc::new(hakimi_tools::TodoTool),
        std::sync::Arc::new(hakimi_tools::ProcessTool),
        std::sync::Arc::new(hakimi_tools::ImageDescribeTool),
        std::sync::Arc::new(hakimi_tools::CodeExecTool),
        std::sync::Arc::new(hakimi_tools::DelegateTaskTool),
        std::sync::Arc::new(hakimi_tools::SessionSearchTool),
        std::sync::Arc::new(hakimi_tools::SendMessageTool),
        std::sync::Arc::new(hakimi_tools::SkillManageTool),
    ];
    for tool in &builtin_tools {
        tool_registry.register(tool.clone()).await;
    }
    info!(count = builtin_tools.len(), "registered built-in tools");

    // Connect to configured MCP servers and register their tools.
    let mcp_tool_count = if !config.mcp_servers.is_empty() {
        let count = register_mcp_tools(&config.mcp_servers, &tool_registry).await;
        info!(count, server_count = config.mcp_servers.len(), "registered MCP tools");
        count
    } else {
        0
    };

    // Discover and register user plugins from ~/.hakimi/plugins/.
    // PluginManager handles manifest.yaml-based command plugins.
    let plugin_manager = hakimi_tools::PluginManager::default_location();
    let plugins = plugin_manager.discover().await;
    let manifest_plugin_count = plugins.len();
    for plugin in plugins {
        tool_registry.register(std::sync::Arc::new(plugin)).await;
    }

    // PluginLoader handles HTTP tool plugins from .yaml/.yml/.json configs
    // in ~/.hakimi/plugins/.
    let mut plugin_loader = hakimi_plugin::PluginLoader::new();
    if let Err(e) = plugin_loader.load_all() {
        warn!(error = %e, "failed to load some HTTP plugins");
    }
    let http_plugin_tools = plugin_loader.all_tools();
    let http_plugin_count = http_plugin_tools.len();
    for tool in http_plugin_tools {
        tool_registry.register(tool).await;
    }

    let plugin_count = manifest_plugin_count + http_plugin_count;

    // Print tool summary.
    println!(
        "  Loaded {} built-in tools, {} MCP tools, {} plugin tools",
        builtin_tools.len(),
        mcp_tool_count,
        plugin_count,
    );

    // Build the agent
    let mut builder = hakimi_core::AIAgent::builder()
        .model(&model)
        .transport(transport)
        .context_engine(context_engine)
        .tool_registry(tool_registry)
        .max_iterations(config.agent.max_turns)
        .workdir(&config.terminal.cwd)
        .streaming(config.display.streaming);

    if let Some(ref provider) = args.provider {
        builder = builder.provider(provider);
    }

    // Set system prompt
    if !config.agent.system_prompt.is_empty() {
        builder = builder.system_prompt(&config.agent.system_prompt);
    }

    let agent = builder.build()?;
    info!(model = %model, "agent built successfully");
    Ok(agent)
}

// ---------------------------------------------------------------------------
// Plugins command handler
// ---------------------------------------------------------------------------

fn handle_plugins_command(arg: Option<&str>) {
    match arg {
        Some("list") | None => {
            // Installed plugins
            let plugins_dir = dirs::home_dir()
                .map(|h| h.join(".hakimi").join("plugins"))
                .unwrap_or_else(|| std::path::PathBuf::from(".hakimi/plugins"));

            println!("━━━ Installed Plugins ━━━");
            if plugins_dir.exists() {
                let mut found = false;
                if let Ok(entries) = std::fs::read_dir(&plugins_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(ext) = path.extension() {
                            if ext == "yaml" || ext == "yml" || ext == "json" {
                                println!("  • {}", path.file_name().unwrap().to_string_lossy());
                                found = true;
                            }
                        }
                    }
                }
                if !found {
                    println!("  (none)");
                }
            } else {
                println!("  (no plugins directory at {})", plugins_dir.display());
            }

            // Available templates
            println!();
            println!("━━━ Available Templates ━━━");
            let templates_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .parent()
                .and_then(|p| p.parent())
                .map(|p| p.join("templates"))
                .unwrap_or_else(|| std::path::PathBuf::from("templates"));
            if templates_dir.exists() {
                if let Ok(entries) = std::fs::read_dir(&templates_dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if let Some(ext) = path.extension() {
                            if ext == "yaml" || ext == "yml" {
                                println!("  • {}", path.file_name().unwrap().to_string_lossy());
                            }
                        }
                    }
                }
            } else {
                println!("  (templates directory not found)");
            }

            // MCP catalog
            println!();
            println!("━━━ Available MCP Servers ━━━");
            let catalog = hakimi_mcp::catalog::default_catalog();
            for entry in &catalog {
                let star = if entry.popular { " ★" } else { "" };
                println!("  • {} [{}] — {}{}", entry.name, entry.category, entry.description, star);
            }
            println!();
            println!("Use /plugins enable <name> to enable an MCP server.");
            println!("Use /plugins init <template> to copy a template to ~/.hakimi/plugins/.");
        }
        Some(rest) => {
            let (subcmd, name) = match rest.split_once(char::is_whitespace) {
                Some((c, n)) => (c, Some(n.trim())),
                None => (rest, None),
            };
            match subcmd {
                "catalog" => {
                    match name {
                        Some(query) if query.starts_with("search ") => {
                            let q = query.strip_prefix("search ").unwrap().trim();
                            let results = hakimi_mcp::catalog::search(q);
                            println!("Search results for \"{q}\":");
                            for entry in &results {
                                println!("  • {} — {}", entry.name, entry.description);
                            }
                            if results.is_empty() {
                                println!("  (no results)");
                            }
                        }
                        Some(query) if query.starts_with("category ") => {
                            let cat = query.strip_prefix("category ").unwrap().trim();
                            let entries = hakimi_mcp::catalog::by_category(cat);
                            println!("MCP servers in category \"{cat}\":");
                            for entry in &entries {
                                println!("  • {} — {}", entry.name, entry.description);
                            }
                            if entries.is_empty() {
                                println!("  (none)");
                            }
                        }
                        _ => {
                            let cats = hakimi_mcp::catalog::categories();
                            println!("Available MCP server categories: {}", cats.join(", "));
                            println!();
                            let catalog = hakimi_mcp::catalog::default_catalog();
                            for entry in &catalog {
                                let star = if entry.popular { " ★" } else { "" };
                                println!("  • {} [{}] — {}{}", entry.name, entry.category, entry.description, star);
                            }
                        }
                    }
                }
                "enable" => {
                    match name {
                        Some(server_name) => {
                            match hakimi_mcp::catalog::get(server_name) {
                                Some(entry) => {
                                    println!("To enable {}, add this to ~/.hakimi/config.yaml:", entry.name);
                                    println!();
                                    let yaml = hakimi_mcp::catalog::to_config_yaml(&[entry]);
                                    println!("{}", yaml);
                                }
                                None => {
                                    println!("Unknown MCP server: {server_name}");
                                    println!("Use /plugins catalog to see available servers.");
                                }
                            }
                        }
                        None => println!("Usage: /plugins enable <server-name>"),
                    }
                }
                "init" => {
                    match name {
                        Some(template_name) => {
                            let templates_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                                .parent()
                                .and_then(|p| p.parent())
                                .map(|p| p.join("templates"))
                                .unwrap_or_else(|| std::path::PathBuf::from("templates"));
                            let template_path = templates_dir.join(template_name);
                            let template_path = if template_path.exists() {
                                template_path
                            } else {
                                templates_dir.join(format!("{template_name}.yaml"))
                            };

                            if !template_path.exists() {
                                println!("Template not found: {template_name}");
                                return;
                            }

                            let plugins_dir = dirs::home_dir()
                                .map(|h| h.join(".hakimi").join("plugins"))
                                .unwrap_or_else(|| std::path::PathBuf::from(".hakimi/plugins"));
                            if let Err(e) = std::fs::create_dir_all(&plugins_dir) {
                                println!("Failed to create plugins dir: {e}");
                                return;
                            }

                            let dest = plugins_dir.join(template_path.file_name().unwrap());
                            match std::fs::copy(&template_path, &dest) {
                                Ok(_) => println!("Copied {} to {}", template_path.file_name().unwrap().to_string_lossy(), dest.display()),
                                Err(e) => println!("Failed to copy: {e}"),
                            }
                        }
                        None => println!("Usage: /plugins init <template-name>"),
                    }
                }
                _ => {
                    println!("Unknown plugins subcommand: {subcmd}");
                    println!("Available: list, catalog, enable, init");
                }
            }
        }
    }
}


// Public entry point
// ---------------------------------------------------------------------------

/// Main entry point for the Hakimi Agent CLI.
///
/// Parses CLI arguments, loads configuration, builds the agent, and enters
/// the appropriate mode (interactive REPL, single-query, or HTTP server).
pub async fn run() -> Result<()> {
    // Initialise logging.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    info!("Hakimi Agent starting (profile={:?})", args.profile);

    // Load configuration.
    let config = load_config();
    info!("configuration loaded (provider={})", config.model.provider);

    if args.yolo {
        info!("YOLO mode enabled — tool calls auto-accepted");
    }

    // --setup mode: run the interactive setup wizard and exit.
    if args.setup {
        match crate::setup_wizard::run_setup_wizard(false) {
            Ok(_config) => {
                return Ok(());
            }
            Err(e) => {
                eprintln!("Setup wizard error: {e}");
                std::process::exit(1);
            }
        }
    }

    // Load skills from ~/.hakimi/skills/
    let skill_store = match hakimi_skills::SkillStore::load_default() {
        Ok(store) => {
            let count = store.skills().len();
            if count > 0 {
                info!(count, "loaded skills");
            }
            store
        }
        Err(e) => {
            warn!(error = %e, "failed to load skills, continuing without them");
            hakimi_skills::SkillStore::empty()
        }
    };

    // Build the agent
    let mut agent = match build_agent(&args, &config).await {
        Ok(agent) => agent,
        Err(e) => {
            error!("Failed to build agent: {e}");
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    // --serve mode: start HTTP API server instead of REPL
    if args.serve {
        info!(addr = %args.addr, "starting HTTP API server mode");

        let db_path = dirs::home_dir()
            .map(|h| h.join(".hakimi").join("sessions.db"))
            .unwrap_or_else(|| std::path::PathBuf::from(".hakimi/sessions.db"));

        let session_db = hakimi_session::SessionDB::new(&db_path)?;
        session_db.initialize()?;

        let server = hakimi_server::Server::new(&args.addr, agent, config, session_db)?;
        server.start().await?;
        return Ok(());
    }

    // --gateway mode: start platform gateway (Telegram, Discord, etc.)
    if args.gateway {
        info!("starting gateway mode");

        // -----------------------------------------------------------------------
        // Per-role agent state.
        // Each role gets its own AIAgent, chat histories, and system prompt.
        // -----------------------------------------------------------------------
        struct AgentState {
            agent: std::sync::Arc<tokio::sync::Mutex<hakimi_core::AIAgent>>,
            chat_histories: std::sync::Arc<
                tokio::sync::Mutex<
                    std::collections::HashMap<String, Vec<hakimi_common::Message>>,
                >,
            >,
            base_system_prompt: String,
        }

        let mut agent_states: std::collections::HashMap<String, AgentState> =
            std::collections::HashMap::new();
        let mut gateway = hakimi_gateway::Gateway::new();

        // ---- Shared resources (used by all roles) ----

        let env_hints = hakimi_context::build_environment_hints(
            "gateway",
            std::env::consts::OS,
            &dirs::home_dir()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|| "/root".to_string()),
            &std::env::current_dir()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|_| ".".to_string()),
        );

        // Load memory content from ~/.hakimi/memory/
        let memory_text = if config.memory.enabled {
            let memory_dir = if config.memory.path.is_empty() {
                dirs::home_dir()
                    .map(|h| h.join(".hakimi").join("memory"))
                    .unwrap_or_else(|| std::path::PathBuf::from("/root/.hakimi/memory"))
            } else {
                std::path::PathBuf::from(&config.memory.path)
            };

            let mut memory_parts = Vec::new();

            let memory_file = memory_dir.join("memory.md");
            if memory_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&memory_file) {
                    if !content.trim().is_empty() {
                        memory_parts.push(format!("[memory]\n{content}"));
                    }
                }
            }

            let user_file = memory_dir.join("user.md");
            if user_file.exists() {
                if let Ok(content) = std::fs::read_to_string(&user_file) {
                    if !content.trim().is_empty() {
                        memory_parts.push(format!("[user profile]\n{content}"));
                    }
                }
            }

            if memory_parts.is_empty() {
                String::new()
            } else {
                memory_parts.join("\n\n")
            }
        } else {
            String::new()
        };

        // ---- Build agent states: multi-role or single-bot fallback ----

        if !config.roles.is_empty() {
            // Multi-role mode: create a separate agent + TelegramAdapter per role.
            info!(role_count = config.roles.len(), "multi-role gateway mode");

            for (role_name, role_config) in &config.roles {
                // Only process roles that have a telegram binding with a non-empty token.
                let tg_binding = match &role_config.gateways.telegram {
                    Some(tg) if !tg.bot_token.is_empty() => tg,
                    _ => {
                        warn!(
                            role = %role_name,
                            "role has no telegram binding or empty bot_token, skipping"
                        );
                        continue;
                    }
                };

                // Create a TelegramAdapter for this role.
                let adapter = hakimi_gateway::TelegramAdapter::from_token(
                    role_name,
                    &tg_binding.bot_token,
                );
                gateway.add_adapter(Box::new(adapter));

                // Build a fresh agent for this role.
                let mut role_agent = match build_agent(&args, &config).await {
                    Ok(a) => a,
                    Err(e) => {
                        error!(role = %role_name, error = %e, "failed to build agent for role");
                        std::process::exit(1);
                    }
                };

                // Override model if the role specifies one.
                if !role_config.model.is_empty() {
                    role_agent.set_model(&role_config.model);
                }

                // Build the role's system prompt from its identity (or fallback to global).
                let identity = if !role_config.identity.is_empty() {
                    role_config.identity.clone()
                } else if !config.agent.system_prompt.is_empty() {
                    config.agent.system_prompt.clone()
                } else {
                    "You are Hakimi, a helpful AI assistant.".to_string()
                };

                let base_system_prompt = hakimi_context::build_system_prompt(
                    &identity,
                    "telegram",
                    "",
                    &memory_text,
                    &env_hints,
                );

                role_agent.set_system_prompt(&base_system_prompt);

                agent_states.insert(
                    role_name.clone(),
                    AgentState {
                        agent: std::sync::Arc::new(tokio::sync::Mutex::new(role_agent)),
                        chat_histories: std::sync::Arc::new(tokio::sync::Mutex::new(
                            std::collections::HashMap::new(),
                        )),
                        base_system_prompt,
                    },
                );

                info!(role = %role_name, "role agent initialized");
            }
        } else {
            // Backward compatibility: single-bot mode using config.gateways.telegram.
            let tg_config = &config.gateways.telegram;
            if tg_config.bot_token.is_empty() {
                error!(
                    "No Telegram bot_token in config. \
                     Add gateways.telegram.bot_token to ~/.hakimi/config.yaml"
                );
                std::process::exit(1);
            }

            let adapter =
                hakimi_gateway::TelegramAdapter::from_token("default", &tg_config.bot_token);
            gateway.add_adapter(Box::new(adapter));

            let identity = if config.agent.system_prompt.is_empty() {
                "You are Hakimi, a helpful AI assistant.".to_string()
            } else {
                config.agent.system_prompt.clone()
            };

            let base_system_prompt = hakimi_context::build_system_prompt(
                &identity,
                "telegram",
                "",
                &memory_text,
                &env_hints,
            );

            // Use the pre-built agent (already constructed before gateway mode).
            agent.set_system_prompt(&base_system_prompt);

            agent_states.insert(
                "default".to_string(),
                AgentState {
                    agent: std::sync::Arc::new(tokio::sync::Mutex::new(agent)),
                    chat_histories: std::sync::Arc::new(tokio::sync::Mutex::new(
                        std::collections::HashMap::new(),
                    )),
                    base_system_prompt,
                },
            );
        }

        if agent_states.is_empty() {
            error!("No agent states created (no roles with telegram bindings and no default bot token)");
            std::process::exit(1);
        }

        // ---- Connect adapters and merge receivers ----

        gateway.connect_all().await?;

        let receivers = gateway.take_all_receivers();
        if receivers.is_empty() {
            error!("No message receivers available from any adapter");
            std::process::exit(1);
        }

        info!(
            "Gateway connected. Listening for messages from {} adapter(s) serving {} role(s)...",
            receivers.len(),
            agent_states.len()
        );

        // Merge all receivers into a single channel for one select loop.
        let (merged_tx, mut merged_rx) =
            tokio::sync::mpsc::unbounded_channel::<hakimi_gateway::GatewayMessage>();
        for (_platform, _bot_id, mut rx) in receivers {
            let tx = merged_tx.clone();
            tokio::spawn(async move {
                while let Some(msg) = rx.recv().await {
                    if tx.send(msg).is_err() {
                        break;
                    }
                }
            });
        }
        drop(merged_tx); // Close original sender so merged_rx returns None when all forwarders finish.

        // ---- Message processing loop ----

        let agent_states_ref = &agent_states;
        let skill_store_ref = &skill_store;

        loop {
            tokio::select! {
                msg = merged_rx.recv() => {
                    let msg = match msg {
                        Some(m) => m,
                        None => {
                            info!("All message channels closed, shutting down gateway");
                            break;
                        }
                    };

                    let bot_id = msg.bot_id.clone();
                    let user_text = msg.text.clone();
                    let chat_id = msg.chat_id.clone();
                    let platform = msg.platform.clone();
                    info!(
                        platform = %platform,
                        bot_id = %bot_id,
                        chat_id = %chat_id,
                        "received message"
                    );

                    // Look up the agent state for this bot_id.
                    let state = match agent_states_ref.get(&bot_id) {
                        Some(s) => s,
                        None => {
                            warn!(bot_id = %bot_id, "no agent state for bot_id, ignoring message");
                            continue;
                        }
                    };

                    let agent_clone = state.agent.clone();
                    let histories_clone = state.chat_histories.clone();
                    let base_sp = &state.base_system_prompt;

                    // Process the message with the correct agent.
                    let response = {
                        let mut a = agent_clone.lock().await;

                        // 1. Load chat-specific message history.
                        {
                            let histories = histories_clone.lock().await;
                            let chat_msgs = histories.get(&chat_id).cloned().unwrap_or_default();
                            a.clear_messages();
                            for m in chat_msgs {
                                a.add_message(m);
                            }
                        }

                        // 2. Inject matching skills into the system prompt.
                        let skill_additions =
                            skill_store_ref.get_system_prompt_additions(&user_text);
                        if !skill_additions.is_empty() {
                            let combined =
                                format!("{base_sp}\n\n## Skills\n{skill_additions}");
                            a.set_system_prompt(&combined);
                        }

                        // 3. Call agent.chat().
                        let result = match a.chat(&user_text).await {
                            Ok(resp) => resp,
                            Err(e) => {
                                error!(error = %e, "agent error");
                                format!("Error: {e}")
                            }
                        };

                        // 4. Restore base system prompt.
                        a.set_system_prompt(base_sp);

                        // 5. Save updated chat history back.
                        {
                            let mut histories = histories_clone.lock().await;
                            histories.insert(chat_id.clone(), a.messages().to_vec());
                        }

                        result
                    };

                    // Send response back via gateway (routed to the correct adapter by bot_id).
                    if let Err(e) = gateway
                        .route_message(&hakimi_gateway::GatewayMessage {
                            platform: platform.clone(),
                            bot_id: bot_id.clone(),
                            chat_id: chat_id.clone(),
                            user_id: String::new(),
                            text: response.clone(),
                            media: None,
                        })
                        .await
                    {
                        error!(error = %e, "failed to send response");
                    }
                    info!(
                        platform = %platform,
                        bot_id = %bot_id,
                        chat_id = %chat_id,
                        response_len = response.len(),
                        "sent response"
                    );
                }
                _ = tokio::signal::ctrl_c() => {
                    info!("Shutting down gateway...");
                    break;
                }
            }
        }

        gateway.disconnect_all().await?;
        return Ok(());
    }

    print_banner();

    // Save the base system prompt for skill injection.
    let base_system_prompt: Option<String> = if config.agent.system_prompt.is_empty() {
        None
    } else {
        Some(config.agent.system_prompt.clone())
    };

    // Single-query mode.
    if let Some(query) = &args.query {
        info!("single-query mode: {}", query);
        println!("You: {query}");
        println!();

        // Inject matching skills into system prompt.
        let skill_additions = skill_store.get_system_prompt_additions(query);
        if !skill_additions.is_empty() {
            let combined = match &base_system_prompt {
                Some(base) => format!("{base}\n\n## Skills\n{skill_additions}"),
                None => format!("## Skills\n{skill_additions}"),
            };
            agent.set_system_prompt(&combined);
        }

        match agent.chat(query).await {
            Ok(response) => println!("{response}"),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }

        // Restore base system prompt.
        match &base_system_prompt {
            Some(base) => agent.set_system_prompt(base),
            None => agent.set_system_prompt(""),
        }
        return Ok(());
    }

    // Initialize profile manager and cron scheduler.
    let hakimi_home = dirs::home_dir()
        .map(|h| h.join(".hakimi"))
        .unwrap_or_else(|| std::path::PathBuf::from(".hakimi"));
    let mut profile_manager = crate::profiles::ProfileManager::new(&hakimi_home);
    let cron_db_path = hakimi_home.join("cron.db");
    let mut cron_scheduler = if cron_db_path.exists() {
        match hakimi_cron::persistence::PersistentCronStore::open(&cron_db_path) {
            Ok(store) => {
                match store.load_into_scheduler() {
                    Ok(sched) => {
                        info!(jobs = sched.list().len(), "loaded persisted cron jobs");
                        sched
                    }
                    Err(e) => {
                        warn!(error = %e, "failed to load persisted cron jobs");
                        hakimi_cron::CronScheduler::new()
                    }
                }
            }
            Err(e) => {
                warn!(error = %e, "failed to open cron database");
                hakimi_cron::CronScheduler::new()
            }
        }
    } else {
        hakimi_cron::CronScheduler::new()
    };

    // Interactive REPL.
    loop {
        print!("> ");
        io::stdout().flush()?;

        let mut input = String::new();
        if io::stdin().read_line(&mut input)? == 0 {
            // EOF
            println!();
            break;
        }

        let input = input.trim();
        if input.is_empty() {
            continue;
        }

        match Command::parse(input) {
            Some(Command::Help) => print_help(),
            Some(Command::Quit) => {
                println!("Goodbye!");
                break;
            }
            Some(Command::Clear) => {
                // ANSI clear screen + move cursor home
                print!("\x1B[2J\x1B[H");
                io::stdout().flush()?;
            }
            Some(Command::Model(ref name)) => match name {
                Some(m) => {
                    agent.set_model(m);
                    println!("Model switched to: {m}");
                }
                None => println!("Current model: {}", agent.model()),
            },
            Some(Command::Config(ref key)) => match key {
                Some(k) => println!("Config key: {k} (not yet wired)"),
                None => println!("Use /config <key> to inspect a setting."),
            },
            Some(Command::Resume(ref id)) => {
                println!("Resume session: {:?} (not yet wired)", id);
            }
            Some(Command::Tools(ref _name)) => {
                println!("Registered tools:");
                for tool_name in agent.tool_registry().list().await {
                    println!("  • {tool_name}");
                }
            }
            Some(Command::Skills(ref name)) => {
                match name {
                    Some(skill_name) => {
                        // Show a specific skill's content from the store.
                        let found = skill_store.skills().iter().find(|s| s.name == *skill_name);
                        match found {
                            Some(skill) => {
                                println!("━━━ Skill: {} ━━━", skill.name);
                                if !skill.description.is_empty() {
                                    println!("Description: {}", skill.description);
                                }
                                if let Some(ref trigger) = skill.trigger {
                                    println!("Trigger: {trigger}");
                                }
                                if !skill.tags.is_empty() {
                                    println!("Tags: {}", skill.tags.join(", "));
                                }
                                println!("---");
                                println!("{}", skill.content);
                            }
                            None => {
                                println!("Skill '{skill_name}' not found.");
                            }
                        }
                    }
                    None => {
                        // List all loaded skills with metadata.
                        println!("{}", skill_store.summary());
                        println!("\nUse /skills <name> to view a skill.");
                    }
                }
            }
            Some(Command::Status) => {
                println!("Session: {}", agent.session_id());
                println!("Model:  {}", agent.model());
                println!("Messages: {}", agent.messages().len());
            }
            Some(Command::Usage) => {
                println!("Usage: (tracked per conversation turn)");
            }
            Some(Command::Profile(ref arg)) => {
                match arg.as_deref() {
                    Some("list") | None => {
                        match profile_manager.list() {
                            Ok(profiles) => {
                                if profiles.is_empty() {
                                    println!("No profiles found. Use /profile create <name> to create one.");
                                } else {
                                    println!("━━━ Profiles ━━━");
                                    let active = profile_manager.active().unwrap_or("default");
                                    for p in &profiles {
                                        let marker = if p.name == active { " (active)" } else { "" };
                                        println!("  • {}{}", p.name, marker);
                                        if let Some(ref desc) = p.description {
                                            println!("    {}", desc);
                                        }
                                    }
                                }
                            }
                            Err(e) => eprintln!("Error listing profiles: {e}"),
                        }
                    }
                    Some(rest) => {
                        let (subcmd, name) = match rest.split_once(char::is_whitespace) {
                            Some((c, n)) => (c, Some(n.trim())),
                            None => (rest, None),
                        };
                        match subcmd {
                            "create" => {
                                match name {
                                    Some(profile_name) => {
                                        match profile_manager.create(profile_name, None) {
                                            Ok(dir) => println!("Profile '{}' created at {}", profile_name, dir.display()),
                                            Err(e) => eprintln!("Error: {e}"),
                                        }
                                    }
                                    None => println!("Usage: /profile create <name>"),
                                }
                            }
                            "delete" => {
                                match name {
                                    Some(profile_name) => {
                                        match profile_manager.delete(profile_name) {
                                            Ok(()) => println!("Profile '{}' deleted.", profile_name),
                                            Err(e) => eprintln!("Error: {e}"),
                                        }
                                    }
                                    None => println!("Usage: /profile delete <name>"),
                                }
                            }
                            "use" => {
                                match name {
                                    Some(profile_name) => {
                                        match profile_manager.use_profile(profile_name) {
                                            Ok(dir) => println!("Switched to profile '{}' ({})", profile_name, dir.display()),
                                            Err(e) => eprintln!("Error: {e}"),
                                        }
                                    }
                                    None => println!("Usage: /profile use <name>"),
                                }
                            }
                            other => {
                                // Treat as a profile name to switch to.
                                match profile_manager.use_profile(other) {
                                    Ok(dir) => println!("Switched to profile '{}' ({})", other, dir.display()),
                                    Err(e) => eprintln!("Error: {e}"),
                                }
                            }
                        }
                    }
                }
            }
            Some(Command::Doctor) => {
                crate::doctor::run_and_print_diagnostics();
            }
            Some(Command::Setup) => {
                match crate::setup_wizard::run_setup_wizard(false) {
                    Ok(_config) => {}
                    Err(e) => eprintln!("Setup wizard error: {e}"),
                }
            }
            Some(Command::Cron(ref arg)) => {
                match arg.as_deref() {
                    Some("list") | None => {
                        let jobs = cron_scheduler.list();
                        if jobs.is_empty() {
                            println!("No cron jobs. Use /cron add <schedule> <prompt> to create one.");
                        } else {
                            println!("━━━ Cron Jobs ━━━");
                            for job in &jobs {
                                let status = if job.enabled { "●" } else { "○" };
                                let schedule_str = match &job.schedule {
                                    hakimi_cron::CronSchedule::IntervalMinutes(m) => format!("{m}m"),
                                    hakimi_cron::CronSchedule::IntervalHours(h) => format!("{h}h"),
                                    hakimi_cron::CronSchedule::CronExpr(e) => e.clone(),
                                };
                                let last = job.last_run
                                    .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                                    .unwrap_or_else(|| "never".to_string());
                                let next = job.next_run
                                    .map(|t| t.format("%Y-%m-%d %H:%M UTC").to_string())
                                    .unwrap_or_else(|| "—".to_string());
                                println!("  {} [{}] {} (schedule: {}, last: {}, next: {})",
                                    status, &job.id[..8], job.name, schedule_str, last, next);
                                println!("    prompt: {}", job.prompt);
                            }
                        }
                    }
                    Some(rest) => {
                        let (subcmd, payload) = match rest.split_once(char::is_whitespace) {
                            Some((c, p)) => (c, Some(p.trim())),
                            None => (rest, None),
                        };
                        match subcmd {
                            "add" => {
                                match payload {
                                    Some(p) => {
                                        // Format: <schedule> <prompt>
                                        let (schedule_str, prompt) = match p.split_once(char::is_whitespace) {
                                            Some((s, pr)) => (s.trim(), pr.trim()),
                                            None => (p, "No prompt specified"),
                                        };
                                        match hakimi_cron::parse_schedule(schedule_str) {
                                            Ok(schedule) => {
                                                let job = hakimi_cron::CronJob::new(
                                                    format!("job-{}", &Uuid::new_v4().to_string()[..8]),
                                                    schedule,
                                                    prompt,
                                                );
                                                let id = cron_scheduler.add(job.clone());
                                                // Persist if DB available.
                                                if let Ok(store) = hakimi_cron::persistence::PersistentCronStore::open(&cron_db_path) {
                                                    if let Err(e) = store.save_job(&job) {
                                                        warn!(error = %e, "failed to persist cron job");
                                                    }
                                                }
                                                println!("Cron job created: {}", &id[..8]);
                                            }
                                            Err(e) => eprintln!("Invalid schedule '{schedule_str}': {e}"),
                                        }
                                    }
                                    None => println!("Usage: /cron add <schedule> <prompt>  (e.g. /cron add 30m check status)"),
                                }
                            }
                            "remove" | "rm" => {
                                match payload {
                                    Some(id_prefix) => {
                                        // Find job by ID prefix.
                                        let jobs = cron_scheduler.list();
                                        let found = jobs.iter().find(|j| j.id.starts_with(id_prefix));
                                        match found {
                                            Some(job) => {
                                                let full_id = job.id.clone();
                                                let name = job.name.clone();
                                                cron_scheduler.remove(&full_id);
                                                // Remove from persistent store too.
                                                if let Ok(store) = hakimi_cron::persistence::PersistentCronStore::open(&cron_db_path) {
                                                    let _ = store.remove_job(&full_id);
                                                }
                                                println!("Removed cron job: {name}");
                                            }
                                            None => println!("No job found matching '{id_prefix}'"),
                                        }
                                    }
                                    None => println!("Usage: /cron remove <id-prefix>"),
                                }
                            }
                            "pause" => {
                                match payload {
                                    Some(id_prefix) => {
                                        let jobs = cron_scheduler.list();
                                        let found = jobs.iter().find(|j| j.id.starts_with(id_prefix)).map(|j| j.id.clone());
                                        match found {
                                            Some(id) => {
                                                if let Some(job) = cron_scheduler.get_mut(&id) {
                                                    job.enabled = false;
                                                    println!("Paused job: {}", job.name);
                                                    // Persist.
                                                    let job_clone = job.clone();
                                                    if let Ok(store) = hakimi_cron::persistence::PersistentCronStore::open(&cron_db_path) {
                                                        let _ = store.save_job(&job_clone);
                                                    }
                                                }
                                            }
                                            None => println!("No job found matching '{id_prefix}'"),
                                        }
                                    }
                                    None => println!("Usage: /cron pause <id-prefix>"),
                                }
                            }
                            "resume" => {
                                match payload {
                                    Some(id_prefix) => {
                                        let jobs = cron_scheduler.list();
                                        let found = jobs.iter().find(|j| j.id.starts_with(id_prefix)).map(|j| j.id.clone());
                                        match found {
                                            Some(id) => {
                                                if let Some(job) = cron_scheduler.get_mut(&id) {
                                                    job.enabled = true;
                                                    job.next_run = Some(job.schedule.next_after(chrono::Utc::now()));
                                                    println!("Resumed job: {}", job.name);
                                                    let job_clone = job.clone();
                                                    if let Ok(store) = hakimi_cron::persistence::PersistentCronStore::open(&cron_db_path) {
                                                        let _ = store.save_job(&job_clone);
                                                    }
                                                }
                                            }
                                            None => println!("No job found matching '{id_prefix}'"),
                                        }
                                    }
                                    None => println!("Usage: /cron resume <id-prefix>"),
                                }
                            }
                            "run" => {
                                match payload {
                                    Some(id_prefix) => {
                                        let jobs = cron_scheduler.list();
                                        let found = jobs.iter().find(|j| j.id.starts_with(id_prefix)).map(|j| (j.id.clone(), j.prompt.clone()));
                                        match found {
                                            Some((id, prompt)) => {
                                                println!("Running cron job manually: {}", prompt);
                                                match agent.chat(&prompt).await {
                                                    Ok(response) => {
                                                        println!();
                                                        println!("{response}");
                                                        println!();
                                                    }
                                                    Err(e) => eprintln!("Error: {e}"),
                                                }
                                                cron_scheduler.mark_executed(&id);
                                            }
                                            None => println!("No job found matching '{id_prefix}'"),
                                        }
                                    }
                                    None => println!("Usage: /cron run <id-prefix>"),
                                }
                            }
                            other => {
                                println!("Unknown cron subcommand: {other}");
                                println!("Available: list, add, remove, pause, resume, run");
                            }
                        }
                    }
                }
            }
            Some(Command::Plugins(ref arg)) => {
                handle_plugins_command(arg.as_deref());
            }
            None => {
                // Regular chat message — forward to agent runtime.
                // Inject matching skills into system prompt.
                let skill_additions = skill_store.get_system_prompt_additions(input);
                if !skill_additions.is_empty() {
                    let combined = match &base_system_prompt {
                        Some(base) => format!("{base}\n\n## Skills\n{skill_additions}"),
                        None => format!("## Skills\n{skill_additions}"),
                    };
                    agent.set_system_prompt(&combined);
                }

                match agent.chat(input).await {
                    Ok(response) => {
                        println!();
                        println!("{response}");
                        println!();
                    }
                    Err(e) => {
                        eprintln!("Error: {e}");
                        // Don't crash — continue the REPL
                    }
                }

                // Restore base system prompt after each turn.
                match &base_system_prompt {
                    Some(base) => agent.set_system_prompt(base),
                    None => agent.set_system_prompt(""),
                }
            }
        }
    }

    Ok(())
}
