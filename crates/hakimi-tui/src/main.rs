//! Hakimi TUI — main entry point.
//!
//! Initializes the terminal, creates the agent, and runs the event loop.

use std::io;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use tokio::sync::{RwLock, mpsc};
use tracing::{error, info, warn};

use hakimi_tui::{AgentCommand, AgentEvent, app::App, ui};

// ---------------------------------------------------------------------------
// Default config YAML (mirrors hakimi-cli)
// ---------------------------------------------------------------------------

const DEFAULT_CONFIG_YAML: &str = r#"# Hakimi Agent Configuration
# ~/.hakimi/config.yaml

model:
  default: ""
  provider: "auto"
  base_url: ""

agent:
  max_turns: 90
  verbose: false
  system_prompt: ""

display:
  streaming: true
  compact: false
  skin: "default"

terminal:
  env_type: "local"
  cwd: "."
  timeout: 60

delegation:
  max_iterations: 45
  model: ""
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

    if !hakimi_dir.exists()
        && let Err(e) = std::fs::create_dir_all(&hakimi_dir)
    {
        warn!(path = %hakimi_dir.display(), error = %e, "failed to create .hakimi directory");
    }

    if !config_path.exists() {
        let _ = std::fs::write(&config_path, DEFAULT_CONFIG_YAML);
    }

    match std::fs::read_to_string(&config_path) {
        Ok(contents) => match serde_yaml::from_str::<hakimi_config::HakimiConfig>(&contents) {
            Ok(config) => config,
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
// Resolve configuration
// ---------------------------------------------------------------------------

fn resolve_api_key(config: &hakimi_config::HakimiConfig) -> String {
    for var in &[
        "HAKIMI_API_KEY",
        "OPENAI_API_KEY",
        "OPENROUTER_API_KEY",
        "ANTHROPIC_API_KEY",
    ] {
        if let Ok(val) = std::env::var(var)
            && !val.is_empty()
        {
            return val;
        }
    }
    if !config.delegation.api_key.is_empty() {
        return config.delegation.api_key.clone();
    }
    String::new()
}

fn resolve_base_url(config: &hakimi_config::HakimiConfig) -> String {
    if let Ok(val) = std::env::var("HAKIMI_BASE_URL")
        && !val.is_empty()
    {
        return val;
    }
    if !config.model.base_url.is_empty() {
        return config.model.base_url.clone();
    }
    "https://openrouter.ai/api".to_string()
}

fn resolve_model(config: &hakimi_config::HakimiConfig) -> String {
    if let Ok(val) = std::env::var("HAKIMI_MODEL")
        && !val.is_empty()
    {
        return val;
    }
    if !config.model.default.is_empty() {
        return config.model.default.clone();
    }
    "anthropic/claude-sonnet-4-20250514".to_string()
}

// ---------------------------------------------------------------------------
// Build agent
// ---------------------------------------------------------------------------

async fn build_agent(config: &hakimi_config::HakimiConfig) -> Result<hakimi_core::AIAgent> {
    let model = resolve_model(config);
    let base_url = resolve_base_url(config);
    let api_key = resolve_api_key(config);

    if api_key.is_empty() {
        anyhow::bail!(
            "No API key found. Set one of:\n\
             • HAKIMI_API_KEY / OPENAI_API_KEY / OPENROUTER_API_KEY env var\n\
             • ~/.hakimi/config.yaml delegation.api_key"
        );
    }

    let client = hakimi_transports::build_llm_http_client()?;
    let transport = Arc::new(hakimi_transports::ChatCompletionsTransport::new(
        base_url, api_key, client,
    ));

    let context_length = 128_000;
    let context_engine = Arc::new(RwLock::new(hakimi_context::SimpleContextEngine::new(
        context_length,
    )));

    let tool_registry = hakimi_tools::ToolRegistry::new();
    #[cfg_attr(not(feature = "browser"), allow(unused_mut))]
    let mut builtin_tools: Vec<Arc<dyn hakimi_tools::Tool>> = vec![
        Arc::new(hakimi_tools::ReadFileTool),
        Arc::new(hakimi_tools::WriteFileTool),
        Arc::new(hakimi_tools::TerminalTool),
        Arc::new(hakimi_tools::SearchFilesTool),
        Arc::new(hakimi_tools::PatchTool),
        Arc::new(hakimi_tools::WebSearchTool),
        Arc::new(hakimi_tools::HaListEntitiesTool),
        Arc::new(hakimi_tools::HaGetStateTool),
        Arc::new(hakimi_tools::HaListServicesTool),
        Arc::new(hakimi_tools::HaCallServiceTool),
        Arc::new(hakimi_tools::MemoryTool::new()),
        Arc::new(hakimi_tools::TodoTool),
        Arc::new(hakimi_tools::ProcessTool),
        Arc::new(hakimi_tools::ImageDescribeTool),
        Arc::new(hakimi_tools::VideoAnalyzeTool),
        Arc::new(hakimi_tools::CodeExecTool),
        Arc::new(hakimi_tools::DelegateTaskTool),
        Arc::new(hakimi_tools::SessionSearchTool),
        Arc::new(hakimi_tools::SendMessageTool),
        Arc::new(hakimi_tools::SkillManageTool),
        Arc::new(hakimi_tools::ImageGenerateTool),
    ];
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
        builtin_tools.push(Arc::new(hakimi_tools::BrowserScrollTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserBackTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserPressTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserGetImagesTool::new(
            browser_manager.clone(),
        )));
        builtin_tools.push(Arc::new(hakimi_tools::BrowserScreenshotTool::new(
            browser_manager,
        )));
    }
    for tool in &builtin_tools {
        tool_registry.register(tool.clone()).await;
    }

    let agent = hakimi_core::AIAgent::builder()
        .model(&model)
        .transport(transport)
        .context_engine(context_engine)
        .tool_registry(tool_registry)
        .max_iterations(config.agent.max_turns)
        .workdir(&config.terminal.cwd)
        .streaming(false)
        .build()?;

    info!(model = %model, "agent built successfully");
    Ok(agent)
}

// ---------------------------------------------------------------------------
// Agent background task
// ---------------------------------------------------------------------------

/// Run the agent in a background task, processing commands from the TUI.
async fn run_agent_task(
    mut agent: hakimi_core::AIAgent,
    mut cmd_rx: mpsc::UnboundedReceiver<AgentCommand>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) {
    while let Some(cmd) = cmd_rx.recv().await {
        match cmd {
            AgentCommand::Chat(message) => {
                event_tx.send(AgentEvent::Thinking).ok();

                match agent.run_conversation(&message).await {
                    Ok(result) => {
                        // Send tool call events for any tool messages in the result
                        for msg in &result.messages {
                            if msg.role == hakimi_common::MessageRole::Tool {
                                let name = msg.name.as_deref().unwrap_or("unknown");
                                let content = msg.content.as_deref().unwrap_or("").to_string();
                                event_tx
                                    .send(AgentEvent::ToolResult {
                                        name: name.to_string(),
                                        content,
                                        is_error: false,
                                    })
                                    .ok();
                            }
                            if let Some(ref tool_calls) = msg.tool_calls {
                                for tc in tool_calls {
                                    event_tx
                                        .send(AgentEvent::ToolCall {
                                            name: tc.name.clone(),
                                            arguments: tc.arguments.clone(),
                                        })
                                        .ok();
                                }
                            }
                        }

                        // Send the final response
                        event_tx
                            .send(AgentEvent::Response(result.final_response))
                            .ok();
                    }
                    Err(e) => {
                        error!("Agent error: {e}");
                        event_tx
                            .send(AgentEvent::Error(format!("Agent error: {e}")))
                            .ok();
                    }
                }

                event_tx.send(AgentEvent::Done).ok();
            }
            AgentCommand::Shutdown => {
                info!("Agent task shutting down");
                break;
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to a file (not stdout, since we own the terminal).
    let log_path = dirs::home_dir()
        .map(|h| h.join(".hakimi").join("tui.log"))
        .unwrap_or_else(|| std::path::PathBuf::from("hakimi-tui.log"));

    let log_file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log_path)?;

    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .with_writer(std::sync::Mutex::new(log_file))
        .init();

    info!("Hakimi TUI starting");

    // Load config and build agent.
    let config = load_config();
    let model = resolve_model(&config);

    let agent = match build_agent(&config).await {
        Ok(agent) => agent,
        Err(e) => {
            // Can't use TUI yet, print to stderr.
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
    };

    let session_id = agent.session_id().to_string();

    // Create channels for TUI ↔ agent communication.
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<AgentCommand>();
    let (event_tx, event_rx) = mpsc::unbounded_channel::<AgentEvent>();

    // Spawn the agent background task.
    let agent_handle = tokio::spawn(run_agent_task(agent, cmd_rx, event_tx));

    // Set up the terminal.
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;
    terminal.clear()?;

    // Create the app state.
    let mut app = App::new(cmd_tx, event_rx, model, session_id);

    // Event loop.
    let tick_rate = Duration::from_millis(100);
    let result = run_event_loop(&mut terminal, &mut app, tick_rate).await;

    // Cleanup terminal.
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    // Wait for agent task to finish.
    let _ = agent_handle.await;

    info!("Hakimi TUI exited");

    if let Err(e) = result {
        error!("Event loop error: {e}");
        eprintln!("Error: {e}");
    }

    Ok(())
}

/// Main event loop: poll for keyboard events and agent events, render the UI.
async fn run_event_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
    tick_rate: Duration,
) -> Result<()> {
    loop {
        // Render
        terminal.draw(|frame| ui::render(frame, app))?;

        // Poll for keyboard events (non-blocking).
        if event::poll(tick_rate)?
            && let Event::Key(key) = event::read()?
        {
            // Only process key-down events (not releases).
            if key.kind == KeyEventKind::Press {
                app.handle_key_event(key);
            }
        }

        // Check for agent events.
        app.poll_agent_events();

        // Advance spinner.
        app.tick();

        // Check if we should quit.
        if app.should_quit {
            break;
        }
    }

    Ok(())
}
