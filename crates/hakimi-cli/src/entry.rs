use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use clap::Parser;

use hakimi_common::Result;
use hakimi_core::Command;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    /// Query the agent with a single message.
    #[arg(short, long)]
    pub query: Option<String>,

    /// Model to use for the agent.
    #[arg(short, long)]
    pub model: Option<String>,

    /// Base URL for the model API.
    #[arg(long)]
    pub base_url: Option<String>,

    /// API key for the model API.
    #[arg(long)]
    pub api_key: Option<String>,

    /// Start the agent in server mode.
    #[arg(long)]
    pub serve: bool,

    /// Address to bind the server to (default: 127.0.0.1:8080).
    #[arg(long, default_value = "127.0.0.1:8080")]
    pub addr: String,

    /// Start the agent in gateway mode.
    #[arg(long)]
    pub gateway: bool,

    /// Setup wizard to configure the agent.
    #[arg(long)]
    pub setup: bool,

    /// Check for and install updates.
    #[arg(long)]
    pub update: bool,

    /// Show version information.
    #[arg(short, long)]
    pub version: bool,
}

fn resolve_model(cli_model: Option<&str>, config: &hakimi_config::HakimiConfig) -> String {
    cli_model.map(|m| m.to_string()).unwrap_or_else(|| config.model.model.clone())
}

fn resolve_base_url(cli_url: Option<&str>, config: &hakimi_config::HakimiConfig) -> String {
    cli_url.map(|u| u.to_string()).unwrap_or_else(|| config.model.base_url.clone())
}

fn resolve_api_key(cli_key: Option<&str>, config: &hakimi_config::HakimiConfig) -> String {
    cli_key.map(|k| k.to_string()).unwrap_or_else(|| config.model.api_key.clone())
}

async fn register_mcp_tools(
    servers: &std::collections::HashMap<String, hakimi_config::McpServerConfig>,
    registry: &hakimi_tools::ToolRegistry,
) -> Result<()> {
    for (name, config) in servers {
        info!(name = %name, "registering MCP server tools");
        // Simplified MCP registration logic
    }
    Ok(())
}

async fn build_agent(
    args: &Args,
    config: &hakimi_config::HakimiConfig,
) -> Result<hakimi_core::AIAgent> {
    let model = resolve_model(args.model.as_deref(), config);
    let base_url = resolve_base_url(args.base_url.as_deref(), config);
    let api_key = resolve_api_key(args.api_key.as_deref(), config);

    if api_key.is_empty() {
        return Err(anyhow::anyhow!("API key is not set. Please set it in config.yaml or via --api-key."));
    }

    let transport: Box<dyn hakimi_transports::Transport> = match config.model.provider.as_str() {
        "anthropic" => Box::new(hakimi_transports::anthropic::AnthropicTransport::new(api_key, model.clone())),
        _ => Box::new(hakimi_transports::openai::OpenAiTransport::new(base_url, api_key, model.clone())),
    };

    let tool_registry = hakimi_tools::ToolRegistry::new();
    // Register built-in tools.
    tool_registry.register(Arc::new(hakimi_tools::TerminalTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::ReadFileTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::WriteFileTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::PatchTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::SearchFilesTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::TodoTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::ProcessTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::CodeExecTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::SessionSearchTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::WebSearchTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::SendMessageTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::ClarifyTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::SkillManageTool)).await;
    tool_registry.register(Arc::new(hakimi_tools::DelegateTaskTool)).await;

    register_mcp_tools(&config.mcp_servers, &tool_registry).await?;

    let skill_store = if !config.agent.skills_path.is_empty() {
        let skills_path = std::path::PathBuf::from(&config.agent.skills_path);
        hakimi_skills::SkillStore::load(&skills_path).unwrap_or_else(|e| {
            warn!(error = %e, path = %skills_path.display(), "failed to load skills, using empty store");
            hakimi_skills::SkillStore::empty()
        })
    } else {
        hakimi_skills::SkillStore::empty()
    };

    let mut agent = hakimi_core::AIAgent::new(transport, tool_registry, skill_store);
    agent.set_model(&model);
    
    if !config.agent.system_prompt.is_empty() {
        agent.set_system_prompt(config.agent.system_prompt.clone());
    }

    Ok(agent)
}

fn start_server(agent: hakimi_core::AIAgent, addr: &str) -> Result<()> {
    let addr: SocketAddr = addr.parse()?;
    info!(addr = %addr, "starting server mode");
    let rt = tokio::runtime::Runtime::new()?;
    rt.block_on(async {
        hakimi_server::Server::new(agent).serve(addr).await
    })?;
    Ok(())
}

async fn start_gateway(
    agent: hakimi_core::AIAgent,
    skill_store: hakimi_skills::SkillStore,
    config: hakimi_config::HakimiConfig,
) -> Result<()> {
    info!("starting gateway mode");
    let mut gateway = hakimi_gateway::Gateway::new();

    if !config.gateways.telegram.bot_token.is_empty() {
        let telegram = hakimi_gateway::TelegramAdapter::new(config.gateways.telegram.bot_token.clone());
        gateway.add_adapter(Box::new(telegram));
        info!("telegram gateway registered");
    }

    let agent_clone = Arc::new(Mutex::new(agent));
    let histories_clone = Arc::new(Mutex::new(std::collections::HashMap::<String, Vec<hakimi_common::Message>>::new()));
    let skill_store_ref = Arc::new(skill_store);

    gateway.connect_all().await?;
    let mut receivers = gateway.take_all_receivers();
    let (_, _, mut messages) = receivers.pop().ok_or_else(|| anyhow::anyhow!("no platform adapter receivers available"))?;

    info!("gateway listening for messages");

    while let Some(msg) = messages.recv().await {
        let chat_id = msg.chat_id.clone();
        let bot_id = msg.bot_id.clone();
        let platform = msg.platform.clone();
        let text = msg.text.clone();
        let config = config.clone();
        let agent_clone = agent_clone.clone();
        let gateway_clone = gateway.clone();
        let skill_store_ref = skill_store_ref.clone();
        let histories_clone = histories_clone.clone();

        tokio::spawn(async move {
            let typing_handle = {
                let g = gateway_clone.clone();
                let bid = bot_id.clone();
                let cid = chat_id.clone();
                tokio::spawn(async move {
                    loop {
                        let _ = g.send_chat_action(&bid, &cid, "typing").await;
                        tokio::time::sleep(std::time::Duration::from_secs(5)).await;
                    }
                })
            };

            let response = if text.starts_with('/') {
                match Command::parse(&text) {
                    Some(Command::Help) => "🤖 **Hakimi Agent Commands**\n\n/help, /clear, /model, /tools, /skills, /status".to_string(),
                    Some(Command::Clear) => {
                        histories_clone.lock().await.remove(&chat_id);
                        agent_clone.lock().await.clear_messages();
                        "🧹 Cleared history.".to_string()
                    }
                    _ => "⚠️ Command not fully implemented in gateway mode.".to_string(),
                }
            } else {
                let mut a = agent_clone.lock().await;
                let chat_msgs = histories_clone.lock().await.get(&chat_id).cloned().unwrap_or_default();
                a.clear_messages();
                for m in chat_msgs { a.add_message(m); }
                
                match a.query(&text).await {
                    Ok(resp) => {
                        let updated = a.messages().to_vec();
                        histories_clone.lock().await.insert(chat_id.clone(), updated);
                        resp
                    }
                    Err(e) => format!("❌ Error: {e}"),
                }
            };

            typing_handle.abort();
            let reply = hakimi_gateway::GatewayMessage {
                platform, bot_id, chat_id, user_id: String::new(), text: response, media: None,
            };
            let _ = gateway_clone.route_message(&reply).await;
        });
    }
    Ok(())
}

async fn self_update() -> Result<()> {
    info!("checking for updates...");
    // Update logic placeholder
    Ok(())
}

pub fn load_config() -> hakimi_config::HakimiConfig {
    hakimi_config::HakimiConfig::load().unwrap_or_else(|_| hakimi_config::HakimiConfig::default())
}

pub async fn run() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt::init();
    
    if args.update { return self_update().await; }
    if args.version { println!("hakimi v{}", env!("CARGO_PKG_VERSION")); return Ok(()); }

    let config = load_config();
    if args.setup { println!("Setup not implemented."); return Ok(()); }

    let agent = build_agent(&args, &config).await?;

    if args.serve { return start_server(agent, &args.addr); }
    if args.gateway {
        let skill_store = agent.skill_store().clone();
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
