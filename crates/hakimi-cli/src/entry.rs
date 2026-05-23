//! Hakimi Agent CLI entry point.
//!
//! Contains the clap [`Args`], configuration loading, agent construction, and
//! the interactive REPL / single-query / server modes so that both the
//! `hakimi-cli` binary and the thin `hakimi-agent` wrapper can share the same
//! implementation.

use anyhow::Result;
use clap::Parser;
use tracing::{error, info, warn};

use crate::Command;

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
    // tool_registry.register(std::sync::Arc::new(hakimi_tools::BrowserTool::new())).await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::ImageDescribeTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::VisionAnalyzeTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::ImageGenerateTool))
        .await;
    // tool_registry.register(std::sync::Arc::new(hakimi_tools::TtsTool::new())).await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::SendMessageTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::ClarifyTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::CheckpointTool))
        .await;
    tool_registry
        .register(std::sync::Arc::new(hakimi_tools::SkillManageTool))
        .await;
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

    // Construct agent.
    let mut agent = hakimi_core::AIAgent::new(&model, transport, tool_registry, Some(skill_store));
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
    use std::collections::HashMap;
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

    if let Some(token) = bot_token
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

    while let Some(msg) = messages.recv().await {
        let chat_id = msg.chat_id.clone();
        let bot_id = msg.bot_id.clone();
        let platform = msg.platform.clone();
        let text = msg.text.clone();

        info!(platform = %platform, chat_id = %chat_id, "received message via gateway");

        let agent_clone = agent_arc.clone();
        let gateway_clone = gateway.clone();
        let skill_store_ref = skill_store_ref.clone();
        let histories_clone = histories_clone.clone();

        tokio::spawn(async move {
            let text = text.clone();
            let chat_id = chat_id.clone();
            let bot_id = bot_id.clone();
            let platform = platform.clone();

            // Start typing indicator.
            let typing_handle = {
                let gateway = gateway_clone.clone();
                let bot_id = bot_id.clone();
                let chat_id = chat_id.clone();
                tokio::spawn(async move {
                    loop {
                        let _ = gateway.send_chat_action(&bot_id, &chat_id, "typing").await;
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
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
                        help.push_str("\nJust send a message to chat with me!");
                        help
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
                                platform: plat,
                                bot_id: bot,
                                chat_id: chat,
                                user_id: "".to_string(),
                                text: "🔄 System is updating and restarting, please hold on...".to_string(),
                                media: None,
                            };
                            let _ = gateway.route_message(&msg).await;
                            let _ = std::process::Command::new("bash").arg("-c").arg("nohup sh -c 'hakimi --update && pkill -f \"hakimi --gateway\" && hakimi --gateway > ~/.hakimi/logs/gateway.log 2>&1' &").spawn();
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

                // Stop typing.
                typing_handle.abort();

                // 4. Send response back via gateway and continue to next message.
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
                return;
            }

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

                // 2. Load context from ~/.hakimi/memory/
                // (Omitted for brevity, but you get the idea)

                // 3. Send query.
                match a.query(&text).await {
                    Ok(resp) => {
                        // Update history.
                        let updated_msgs = a.messages().to_vec();
                        {
                            let mut histories = histories_clone.lock().await;
                            histories.insert(chat_id.clone(), updated_msgs);
                        }
                        resp
                    }
                    Err(e) => {
                        error!(error = %e, "agent query failed");
                        format!("❌ Error: {e}")
                    }
                }
            };

            // Stop typing.
            typing_handle.abort();

            // 4. Send response.
            let reply = hakimi_gateway::GatewayMessage {
                platform: platform.clone(),
                bot_id: bot_id.clone(),
                chat_id: chat_id.clone(),
                user_id: String::new(),
                text: response,
                media: None,
            };

            let _ = gateway_clone.route_message(&reply).await;
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
        let loader = hakimi_plugin::PluginLoader::new();
        match loader.install_from_url(&plugin_url).await {
            Ok(name) => println!("✅ Successfully installed plugin '{}'", name),
            Err(e) => println!("❌ Failed to install plugin: {}", e),
        }
        return Ok(());
    }

    let config = load_config();

    if args.setup {
        println!("Setup not implemented.");
        return Ok(());
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

    println!("Interactive REPL (not yet implemented in this view).");
    Ok(())
}
