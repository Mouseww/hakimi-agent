//! Hakimi Agent CLI — interactive REPL and single-query mode.

use std::io::{self, Write};
use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tokio::sync::RwLock;
use tracing::{error, info, warn};

// Re-export the Command enum so downstream crates can use it.
use hakimi_cli::Command;

// ---------------------------------------------------------------------------
// CLI arguments (clap)
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "hakimi", about = "Hakimi Agent — AI-powered coding assistant")]
struct Args {
    /// Model identifier override (e.g. "gpt-4o", "claude-sonnet-4-20250514").
    #[arg(long)]
    model: Option<String>,

    /// Provider override (e.g. "openrouter", "anthropic").
    #[arg(long)]
    provider: Option<String>,

    /// Single query mode: send a prompt and exit.
    #[arg(long, short)]
    query: Option<String>,

    /// Configuration profile to load.
    #[arg(long, short)]
    profile: Option<String>,

    /// Auto-accept all tool calls without confirmation (YOLO mode).
    #[arg(long)]
    yolo: bool,

    /// API key (overrides env var / config).
    #[arg(long)]
    api_key: Option<String>,

    /// Base URL for the API endpoint.
    #[arg(long)]
    base_url: Option<String>,
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

compression:
  enabled: true
  threshold: 0.50
  target_ratio: 0.20
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
    // 3. Config file delegation api_key (as fallback)
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

        // Build args as &str slices
        let args: Vec<&str> = server_config.args.iter().map(|s| s.as_str()).collect();

        let mut client = match hakimi_mcp::McpClient::connect_stdio(&server_config.command, &args).await {
            Ok(c) => c,
            Err(e) => {
                warn!(server = %name, error = %e, "failed to spawn MCP server");
                continue;
            }
        };

        // Set any environment variables before initializing.
        for (key, val) in &server_config.env {
            // SAFETY: We're setting env vars during single-threaded startup,
            // before any concurrent reads begin.
            unsafe { std::env::set_var(key, val); }
        }

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

    // Create transport
    let client = reqwest::Client::new();
    let transport = Arc::new(hakimi_transports::ChatCompletionsTransport::new(
        base_url.clone(),
        api_key.clone(),
        client,
    ));

    // Create context engine
    let context_length = 128_000;
    let context_engine = Arc::new(RwLock::new(
        hakimi_context::SimpleContextEngine::new(context_length),
    ));

    // Create tool registry and register ALL built-in tools.
    let tool_registry = hakimi_tools::ToolRegistry::new();
    let builtin_tools: Vec<Arc<dyn hakimi_tools::Tool>> = vec![
        Arc::new(hakimi_tools::ReadFileTool),
        Arc::new(hakimi_tools::WriteFileTool),
        Arc::new(hakimi_tools::TerminalTool),
        Arc::new(hakimi_tools::SearchFilesTool),
        Arc::new(hakimi_tools::PatchTool),
        Arc::new(hakimi_tools::WebSearchTool),
        Arc::new(hakimi_tools::MemoryTool),
        Arc::new(hakimi_tools::TodoTool),
        Arc::new(hakimi_tools::ProcessTool),
        Arc::new(hakimi_tools::ImageDescribeTool),
        Arc::new(hakimi_tools::CodeExecTool),
        Arc::new(hakimi_tools::DelegateTaskTool),
        Arc::new(hakimi_tools::SessionSearchTool),
        Arc::new(hakimi_tools::SendMessageTool),
        Arc::new(hakimi_tools::SkillManageTool),
    ];
    for tool in &builtin_tools {
        tool_registry.register(tool.clone()).await;
    }
    info!(count = builtin_tools.len(), "registered built-in tools");

    // Connect to configured MCP servers and register their tools.
    if !config.mcp_servers.is_empty() {
        let mcp_count = register_mcp_tools(&config.mcp_servers, &tool_registry).await;
        info!(count = mcp_count, server_count = config.mcp_servers.len(), "registered MCP tools");
    }

    // Discover and register user plugins from ~/.hakimi/plugins/.
    let plugin_manager = hakimi_tools::PluginManager::default_location();
    let plugins = plugin_manager.discover().await;
    for plugin in plugins {
        tool_registry.register(Arc::new(plugin)).await;
    }

    // Build the agent
    let mut builder = hakimi_core::AIAgent::builder()
        .model(&model)
        .transport(transport)
        .context_engine(context_engine)
        .tool_registry(tool_registry)
        .max_iterations(config.agent.max_turns)
        .workdir(&config.terminal.cwd)
        .streaming(true);

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
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
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

    // Build the agent
    let mut agent = match build_agent(&args, &config).await {
        Ok(agent) => agent,
        Err(e) => {
            error!("Failed to build agent: {e}");
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    print_banner();

    // Single-query mode.
    if let Some(query) = &args.query {
        info!("single-query mode: {}", query);
        println!("You: {query}");
        println!();
        match agent.chat(query).await {
            Ok(response) => println!("{response}"),
            Err(e) => {
                eprintln!("Error: {e}");
                std::process::exit(1);
            }
        }
        return Ok(());
    }

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
                Some(m) => println!("Switching model to: {m} (not yet wired)"),
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
                println!("Skills: {:?} (not yet wired)", name);
            }
            Some(Command::Status) => {
                println!("Session: {}", agent.session_id());
                println!("Model:  {}", agent.model());
                println!("Messages: {}", agent.messages().len());
            }
            Some(Command::Usage) => {
                println!("Usage: (tracked per conversation turn)");
            }
            None => {
                // Regular chat message — forward to agent runtime.
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
            }
        }
    }

    Ok(())
}
