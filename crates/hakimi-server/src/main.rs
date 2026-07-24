//! Standalone HTTP API server binary for the Hakimi Agent.
//!
//! Usage: hakimi-server [--addr 127.0.0.1:3005]

use std::sync::Arc;

use anyhow::Result;
use clap::Parser;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "hakimi-server", about = "Hakimi Agent HTTP API server")]
struct Args {
    /// Address to bind the HTTP server to.
    #[arg(long, default_value = "127.0.0.1:3005")]
    addr: String,

    /// Model identifier override.
    #[arg(long)]
    model: Option<String>,

    /// Provider override.
    #[arg(long)]
    provider: Option<String>,

    /// API key (overrides env var / config).
    #[arg(long)]
    api_key: Option<String>,

    /// Base URL for the API endpoint.
    #[arg(long)]
    base_url: Option<String>,
}

// ---------------------------------------------------------------------------
// Config loading (mirrors hakimi-cli logic)
// ---------------------------------------------------------------------------

fn load_config() -> hakimi_config::HakimiConfig {
    let hakimi_dir = dirs::home_dir()
        .map(|h| h.join(".hakimi"))
        .unwrap_or_else(|| std::path::PathBuf::from(".hakimi"));

    let config_path = hakimi_dir.join("config.yaml");

    if !hakimi_dir.exists()
        && let Err(e) = std::fs::create_dir_all(&hakimi_dir)
    {
        warn!(path = %hakimi_dir.display(), error = %e, "failed to create .hakimi directory");
    }

    match std::fs::read_to_string(&config_path) {
        Ok(contents) => match serde_yaml::from_str::<hakimi_config::HakimiConfig>(&contents) {
            Ok(config) => {
                info!(path = %config_path.display(), "loaded config from file");
                config
            }
            Err(e) => {
                warn!(error = %e, "failed to parse config, using defaults");
                hakimi_config::HakimiConfig::default()
            }
        },
        Err(e) => {
            warn!(error = %e, "failed to read config, using defaults");
            hakimi_config::HakimiConfig::default()
        }
    }
}

// ---------------------------------------------------------------------------
// Agent builder (simplified — mirrors hakimi-cli)
// ---------------------------------------------------------------------------

fn trajectory_config_from_config(
    config: &hakimi_config::HakimiConfig,
) -> Option<hakimi_core::TrajectoryConfig> {
    if !config.agent.save_trajectories {
        return None;
    }

    let dir = if config.agent.trajectory_dir.trim().is_empty() {
        dirs::home_dir()
            .map(|home| home.join(".hakimi").join("trajectories"))
            .unwrap_or_else(|| std::path::PathBuf::from(".hakimi/trajectories"))
    } else {
        std::path::PathBuf::from(config.agent.trajectory_dir.trim())
    };

    Some(hakimi_core::TrajectoryConfig::new(dir))
}

fn is_bedrock_transport(api_mode: &str, provider: &str) -> bool {
    let mode = api_mode.trim().to_ascii_lowercase();
    let provider = provider.trim().to_ascii_lowercase();
    matches!(
        mode.as_str(),
        "bedrock" | "bedrock_converse" | "aws_bedrock"
    ) || matches!(
        provider.as_str(),
        "bedrock" | "aws" | "aws_bedrock" | "amazon-bedrock"
    )
}

fn resolve_bedrock_region() -> String {
    std::env::var("AWS_REGION")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| {
            std::env::var("AWS_DEFAULT_REGION")
                .ok()
                .filter(|value| !value.trim().is_empty())
        })
        .unwrap_or_else(|| "us-east-1".to_string())
}

fn resolve_optional_base_url(args: &Args, config: &hakimi_config::HakimiConfig) -> Option<String> {
    if let Some(url) = args.base_url.as_deref()
        && !url.trim().is_empty()
    {
        return Some(url.trim().to_string());
    }
    if let Ok(val) = std::env::var("HAKIMI_BASE_URL")
        && !val.trim().is_empty()
    {
        return Some(val.trim().to_string());
    }
    if !config.model.base_url.trim().is_empty() {
        return Some(config.model.base_url.trim().to_string());
    }
    None
}

async fn build_agent(
    args: &Args,
    config: &hakimi_config::HakimiConfig,
) -> Result<hakimi_core::AIAgent> {
    // Resolve model
    let model = args
        .model
        .clone()
        .or_else(|| std::env::var("HAKIMI_MODEL").ok().filter(|s| !s.is_empty()))
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if !config.model.default.is_empty() {
                config.model.default.clone()
            } else {
                "anthropic/claude-sonnet-4-20250514".to_string()
            }
        });

    // Resolve API key
    let api_key = args
        .api_key
        .clone()
        .or_else(|| {
            std::env::var("HAKIMI_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            std::env::var("OPENAI_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            std::env::var("OPENROUTER_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            std::env::var("ANTHROPIC_API_KEY")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .or_else(|| {
            let key = config.model.api_key.trim().to_string();
            (!key.is_empty()).then_some(key)
        })
        .or_else(|| {
            let key = config.delegation.api_key.trim().to_string();
            (!key.is_empty()).then_some(key)
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_default();

    let base_url = args
        .base_url
        .clone()
        .or_else(|| {
            std::env::var("HAKIMI_BASE_URL")
                .ok()
                .filter(|s| !s.is_empty())
        })
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| {
            if !config.model.base_url.is_empty() {
                config.model.base_url.clone()
            } else {
                "https://openrouter.ai/api".to_string()
            }
        });

    // Determine provider
    let provider = args
        .provider
        .clone()
        .filter(|s| !s.is_empty() && s != "auto")
        .or_else(|| {
            (!config.model.provider.is_empty() && config.model.provider != "auto")
                .then(|| config.model.provider.clone())
        })
        .unwrap_or_else(|| {
            if model.starts_with("claude") || model.contains("anthropic") {
                "anthropic".to_string()
            } else if model.starts_with("gpt-")
                || model.starts_with("o1")
                || model.starts_with("o3")
            {
                "openai".to_string()
            } else {
                "openrouter".to_string()
            }
        });
    let bedrock_mode = is_bedrock_transport(config.model.api_mode.as_str(), &provider);

    if api_key.is_empty() && !bedrock_mode {
        anyhow::bail!(
            "No API key found. Set one of:\n\
             • --api-key flag\n\
             • HAKIMI_API_KEY / OPENAI_API_KEY / OPENROUTER_API_KEY env var\n\
             • ~/.hakimi/config.yaml delegation.api_key"
        );
    }

    // Create transport
    let client = hakimi_transports::build_llm_http_client()?;
    let is_anthropic =
        provider == "anthropic" || provider == "claude" || base_url.contains("api.anthropic.com");

    let transport: Arc<dyn hakimi_transports::ProviderTransport> = if bedrock_mode {
        Arc::new(hakimi_transports::BedrockConverseTransport::from_env(
            Some(resolve_bedrock_region()),
            resolve_optional_base_url(args, config),
            client.clone(),
        )?)
    } else if is_anthropic {
        let anthropic_url = if base_url.contains("anthropic") {
            base_url.clone()
        } else {
            "https://api.anthropic.com".to_string()
        };
        Arc::new(hakimi_transports::AnthropicTransport::new(
            anthropic_url,
            api_key.clone(),
            client.clone(),
        ))
    } else {
        Arc::new(hakimi_transports::ChatCompletionsTransport::new(
            base_url.clone(),
            api_key.clone(),
            client.clone(),
        ))
    };

    // Create embedding provider only when enabled.
    let embedding_provider: Option<Arc<dyn hakimi_transports::EmbeddingProvider>> = if config
        .embedding
        .enabled
    {
        let embedding_base_url =
            if config.embedding.base_url.is_empty() || config.embedding.base_url == "same-as-llm" {
                base_url.clone()
            } else {
                config.embedding.base_url.clone()
            };
        let embedding_api_key =
            if config.embedding.api_key.is_empty() || config.embedding.api_key == "same-as-llm" {
                api_key.clone()
            } else {
                config.embedding.api_key.clone()
            };

        if embedding_api_key.is_empty() {
            warn!(provider = %config.embedding.provider, "embedding provider requires an API key; vector search disabled");
            None
        } else if config.embedding.provider == "openai-compatible"
            || config.embedding.provider == "openai"
        {
            info!(
                base_url = %embedding_base_url,
                model = %config.embedding.model,
                dimension = config.embedding.dimension,
                "using OpenAI-compatible embeddings provider"
            );
            Some(
                Arc::new(hakimi_transports::OpenAICompatibleEmbeddingProvider::new(
                    embedding_base_url,
                    embedding_api_key,
                    config.embedding.model.clone(),
                    config.embedding.dimension,
                    config.embedding.normalize,
                    client.clone(),
                )) as Arc<dyn hakimi_transports::EmbeddingProvider>,
            )
        } else {
            warn!(provider = %config.embedding.provider, "unsupported embedding provider; vector search disabled");
            None
        }
    } else {
        info!("embedding/vector search disabled by config");
        None
    };

    let compression_model = if config.compression.model.trim().is_empty() {
        model.as_str()
    } else {
        config.compression.model.as_str()
    };
    let context_engine = hakimi_context::build_context_engine(
        &config.compression.engine,
        config.compression.context_length,
        Some(compression_model),
        Some(transport.clone()),
    );

    // Tool registry with built-in tools
    let tool_registry = hakimi_tools::ToolRegistry::new();
    tool_registry
        .configure_tool_output(config.tools.output.clone())
        .await;
    #[cfg_attr(not(feature = "browser"), allow(unused_mut))]
    let mut builtin_tools: Vec<Arc<dyn hakimi_tools::Tool>> = vec![
        Arc::new(hakimi_tools::ReadFileTool),
        Arc::new(hakimi_tools::WriteFileTool),
        Arc::new(hakimi_tools::TerminalTool),
        Arc::new(hakimi_tools::SearchFilesTool),
        Arc::new(hakimi_tools::PatchTool),
        Arc::new(hakimi_tools::WebSearchTool),
        Arc::new(hakimi_tools::WebExtractTool),
        Arc::new(hakimi_tools::HaListEntitiesTool),
        Arc::new(hakimi_tools::HaGetStateTool),
        Arc::new(hakimi_tools::HaListServicesTool),
        Arc::new(hakimi_tools::HaCallServiceTool),
        Arc::new(hakimi_tools::MemoryTool::new()),
        Arc::new(hakimi_tools::TodoToolV2),
        Arc::new(hakimi_tools::ProcessTool),
        Arc::new(hakimi_tools::ImageDescribeTool),
        Arc::new(hakimi_tools::VideoAnalyzeTool),
        Arc::new(hakimi_tools::MixtureOfAgentsTool),
        Arc::new(hakimi_tools::CodeExecTool),
        Arc::new(hakimi_tools::DelegateTaskTool),
        Arc::new(hakimi_tools::TeamTool),
        Arc::new(hakimi_tools::SessionSearchTool),
        Arc::new(hakimi_tools::SendMessageTool),
        Arc::new(hakimi_tools::SkillManageTool),
        Arc::new(hakimi_tools::TextToSpeechTool),
        Arc::new(hakimi_tools::TranscribeAudioTool),
        Arc::new(hakimi_tools::VoiceCaptureTool),
        Arc::new(hakimi_tools::ComputerUseTool),
        Arc::new(hakimi_tools::ImageGenerateTool),
    ];
    builtin_tools.extend(hakimi_tools::kanban_tools());
    // Browser tools (shared browser instance)
    #[cfg(feature = "browser")]
    {
        let browser_manager = hakimi_tools::BrowserManager::new();
        builtin_tools.push(Arc::new(hakimi_tools::BrowserNavigateTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserSnapshotTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserClickTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserTypeTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserDialogTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserScreenshotTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserCdpTool::new()));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserVisionTool::new(
            browser_manager,
        )));
    }
    for tool in &builtin_tools {
        tool_registry.register(tool.clone()).await;
    }

    // Knowledge tools/searcher; vector index is attached only when embedding is enabled.
    let knowledge_path = dirs::home_dir()
        .map(|h| h.join(".hakimi").join("knowledge.json"))
        .unwrap_or_else(|| std::path::PathBuf::from(".hakimi/knowledge.json"));
    let knowledge_provider = if let Some(provider) = embedding_provider.clone() {
        Arc::new(hakimi_knowledge::KnowledgeProvider::with_vector_search(
            knowledge_path,
            provider,
        ))
    } else {
        Arc::new(hakimi_knowledge::KnowledgeProvider::new(knowledge_path))
    };
    for definition in
        hakimi_context::MemoryProvider::get_tool_definitions(knowledge_provider.as_ref())
    {
        tool_registry
            .register(Arc::new(hakimi_knowledge::KnowledgeTool::new(
                knowledge_provider.clone(),
                definition,
            )))
            .await;
    }
    let knowledge_searcher: Arc<dyn hakimi_common::KnowledgeSearcher> = knowledge_provider;

    info!(count = builtin_tools.len(), "registered built-in tools");

    // Build agent
    let mut agent = hakimi_core::AIAgent::builder()
        .model(&model)
        .fallback_models(config.model.fallback_models.clone())
        .transport(transport)
        .context_engine(context_engine)
        .tool_registry(tool_registry)
        .knowledge_searcher(knowledge_searcher)
        .max_iterations(config.agent.max_turns)
        .workdir(&config.terminal.cwd)
        .tool_search(
            config.tools.tool_search.clone(),
            config.compression.context_length,
        )
        .build()?
        .with_trajectory_saving(trajectory_config_from_config(config));
    agent = agent.with_embedding_provider(embedding_provider);

    info!(model = %model, "base agent built successfully");

    // TODO: Wrap agent with ModelDispatcher when smart dispatch is fully implemented
    let dispatched_agent = agent;

    if config.model.auto_dispatch.enabled {
        info!(
            "model dispatch enabled with tiers: light={}, reasoning={}",
            config
                .model
                .tiers
                .as_ref()
                .and_then(|t| t.light.as_ref())
                .map(|l| l.model.as_str())
                .unwrap_or("none"),
            config
                .model
                .tiers
                .as_ref()
                .and_then(|t| t.reasoning.as_ref())
                .map(|r| r.model.as_str())
                .unwrap_or("none")
        );
    }

    Ok(dispatched_agent)
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let args = Args::parse();
    info!(addr = %args.addr, "Hakimi Agent HTTP server starting");

    let config = load_config();
    let agent = build_agent(&args, &config).await?;

    // Open the session database
    let db_path = dirs::home_dir()
        .map(|h| h.join(".hakimi").join("sessions.db"))
        .unwrap_or_else(|| std::path::PathBuf::from(".hakimi/sessions.db"));
    let session_db = hakimi_session::SessionDB::new(&db_path)?;
    session_db.initialize()?;

    let server = hakimi_server::Server::new(&args.addr, agent, config, session_db)?;
    server.serve(args.addr.parse().unwrap()).await
}
