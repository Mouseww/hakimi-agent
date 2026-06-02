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
use serde_json::{Value as JsonValue, json};
use tokio::sync::{RwLock, mpsc};
use tracing::{error, info, warn};

use hakimi_tui::{AgentCommand, AgentEvent, app::App, ui};

fn bind_runtime_home_env(runtime_home: &hakimi_common::RuntimeHome) {
    // SAFETY: The TUI binds its runtime home during single-threaded startup,
    // before the agent task and tools begin reading environment-backed paths.
    unsafe {
        std::env::set_var("HAKIMI_HOME", runtime_home.home().as_os_str());
    }
}

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
  save_trajectories: false
  # Empty means ~/.hakimi/trajectories.
  trajectory_dir: ""
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

voice:
  provider: "openai"
  model: ""
  voice: ""
  transcription_model: ""
  base_url: ""
  api_key: ""
  auto_play: false
  record_key: "ctrl+b"
  silence_threshold: 200
  silence_duration_seconds: 3.0
  beep_enabled: true
"#;

// ---------------------------------------------------------------------------
// Config loading
// ---------------------------------------------------------------------------

fn load_config(runtime_home: &hakimi_common::RuntimeHome) -> hakimi_config::HakimiConfig {
    let hakimi_dir = runtime_home.home();
    let config_path = runtime_home.config_path();

    if !hakimi_dir.exists()
        && let Err(e) = std::fs::create_dir_all(hakimi_dir)
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

fn trajectory_config_from_config(
    config: &hakimi_config::HakimiConfig,
    runtime_home: &hakimi_common::RuntimeHome,
) -> Option<hakimi_core::TrajectoryConfig> {
    if !config.agent.save_trajectories {
        return None;
    }

    let dir = if config.agent.trajectory_dir.trim().is_empty() {
        runtime_home.trajectories_dir()
    } else {
        std::path::PathBuf::from(config.agent.trajectory_dir.trim())
    };

    Some(hakimi_core::TrajectoryConfig::new(dir))
}

// ---------------------------------------------------------------------------
// Build agent
// ---------------------------------------------------------------------------

async fn build_agent(
    config: &hakimi_config::HakimiConfig,
    runtime_home: &hakimi_common::RuntimeHome,
) -> Result<hakimi_core::AIAgent> {
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
        Arc::new(hakimi_tools::HaListEntitiesTool),
        Arc::new(hakimi_tools::HaGetStateTool),
        Arc::new(hakimi_tools::HaListServicesTool),
        Arc::new(hakimi_tools::HaCallServiceTool),
        Arc::new(hakimi_tools::MemoryTool::new()),
        Arc::new(hakimi_tools::TodoTool),
        Arc::new(hakimi_tools::ProcessTool),
        Arc::new(hakimi_tools::ImageDescribeTool),
        Arc::new(hakimi_tools::VideoAnalyzeTool),
        Arc::new(hakimi_tools::MixtureOfAgentsTool),
        Arc::new(hakimi_tools::CodeExecTool),
        Arc::new(hakimi_tools::DelegateTaskTool),
        Arc::new(hakimi_tools::SessionSearchTool),
        Arc::new(hakimi_tools::SendMessageTool),
        Arc::new(hakimi_tools::SkillManageTool),
        Arc::new(hakimi_tools::ImageGenerateTool),
        Arc::new(hakimi_tools::TextToSpeechTool),
        Arc::new(hakimi_tools::TranscribeAudioTool),
        Arc::new(hakimi_tools::VoiceCaptureTool),
        Arc::new(hakimi_tools::ComputerUseTool),
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
        builtin_tools.push(Arc::new(hakimi_tools::BrowserConsoleTool::new(
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

    let skills_path = if config.agent.skills_path.trim().is_empty() {
        runtime_home.skills_dir()
    } else {
        std::path::PathBuf::from(config.agent.skills_path.trim())
    };
    let skill_store = if skills_path.exists() {
        hakimi_skills::SkillStore::load(&skills_path).unwrap_or_else(|err| {
            warn!(error = %err, path = %skills_path.display(), "failed to load skill store, using empty store");
            hakimi_skills::SkillStore::empty()
        })
    } else {
        hakimi_skills::SkillStore::empty()
    };

    let knowledge_provider = Arc::new(hakimi_knowledge::KnowledgeProvider::new(
        runtime_home.knowledge_path(),
    ));
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

    let agent = hakimi_core::AIAgent::builder()
        .model(&model)
        .transport(transport)
        .context_engine(context_engine)
        .tool_registry(tool_registry)
        .skill_store(skill_store)
        .knowledge_searcher(knowledge_searcher)
        .max_iterations(config.agent.max_turns)
        .workdir(&config.terminal.cwd)
        .streaming(false)
        .build()?
        .with_voice_settings(
            Some(config.voice.provider.clone()).filter(|s| !s.is_empty()),
            Some(config.voice.model.clone()).filter(|s| !s.is_empty()),
            Some(config.voice.base_url.clone()).filter(|s| !s.is_empty()),
            Some(config.voice.api_key.clone()).filter(|s| !s.is_empty()),
            Some(config.voice.voice.clone()).filter(|s| !s.is_empty()),
            config.voice.auto_play,
            Some(config.voice.provider.clone()).filter(|s| !s.is_empty()),
            Some(config.voice.transcription_model.clone()).filter(|s| !s.is_empty()),
            Some(config.voice.base_url.clone()).filter(|s| !s.is_empty()),
            Some(config.voice.api_key.clone()).filter(|s| !s.is_empty()),
        )
        .with_trajectory_saving(trajectory_config_from_config(config, runtime_home));

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

                run_chat_turn(&mut agent, &message, &event_tx).await;

                event_tx.send(AgentEvent::Done).ok();
            }
            AgentCommand::VoiceCapture {
                duration_seconds,
                silence_threshold,
            } => {
                event_tx.send(AgentEvent::Thinking).ok();
                let transcript = {
                    let capture = run_voice_capture_turn(
                        &mut agent,
                        duration_seconds,
                        silence_threshold,
                        &event_tx,
                    );
                    tokio::pin!(capture);
                    loop {
                        tokio::select! {
                            transcript = &mut capture => break transcript,
                            next = cmd_rx.recv() => {
                                match next {
                                    Some(AgentCommand::CancelVoiceCapture) => {
                                        event_tx.send(AgentEvent::VoiceCaptureCancelled).ok();
                                        break None;
                                    }
                                    Some(AgentCommand::Shutdown) => {
                                        event_tx.send(AgentEvent::VoiceCaptureCancelled).ok();
                                        info!("Agent task shutting down");
                                        return;
                                    }
                                    Some(other) => {
                                        warn!(
                                            command = command_kind(&other),
                                            "ignoring command while voice capture is active"
                                        );
                                    }
                                    None => break None,
                                }
                            }
                        }
                    }
                };
                if let Some(transcript) = transcript {
                    run_chat_turn(&mut agent, &transcript, &event_tx).await;
                }
                event_tx.send(AgentEvent::Done).ok();
            }
            AgentCommand::CancelVoiceCapture => {
                warn!("ignoring voice capture cancellation with no active capture");
            }
            AgentCommand::Shutdown => {
                info!("Agent task shutting down");
                break;
            }
        }
    }
}

fn command_kind(command: &AgentCommand) -> &'static str {
    match command {
        AgentCommand::Chat(_) => "chat",
        AgentCommand::VoiceCapture { .. } => "voice_capture",
        AgentCommand::CancelVoiceCapture => "cancel_voice_capture",
        AgentCommand::Shutdown => "shutdown",
    }
}

async fn run_chat_turn(
    agent: &mut hakimi_core::AIAgent,
    message: &str,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) {
    match agent.run_conversation(message).await {
        Ok(result) => {
            send_conversation_events(result, event_tx);
        }
        Err(e) => {
            error!("Agent error: {e}");
            event_tx
                .send(AgentEvent::Error(format!("Agent error: {e}")))
                .ok();
        }
    }
}

fn send_conversation_events(
    result: hakimi_core::ConversationResult,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) {
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

    event_tx
        .send(AgentEvent::Response(result.final_response))
        .ok();
}

async fn run_voice_capture_turn(
    agent: &mut hakimi_core::AIAgent,
    duration_seconds: f32,
    silence_threshold: u32,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Option<String> {
    let args = json!({
        "duration_seconds": duration_seconds,
        "silence_threshold": silence_threshold,
        "transcribe": true,
        "response_format": "text",
    });
    event_tx
        .send(AgentEvent::ToolCall {
            name: "voice_capture".to_string(),
            arguments: args.to_string(),
        })
        .ok();

    let ctx = agent.build_tool_context();
    let result = agent
        .tool_registry()
        .dispatch("voice_capture", &args, &ctx)
        .await;

    match result {
        Ok(content) => {
            event_tx
                .send(AgentEvent::ToolResult {
                    name: "voice_capture".to_string(),
                    content: content.clone(),
                    is_error: false,
                })
                .ok();
            handle_voice_capture_result(&content, event_tx)
        }
        Err(e) => {
            let error = format!("Voice capture error: {e}");
            event_tx
                .send(AgentEvent::ToolResult {
                    name: "voice_capture".to_string(),
                    content: error.clone(),
                    is_error: true,
                })
                .ok();
            event_tx.send(AgentEvent::Error(error)).ok();
            None
        }
    }
}

fn handle_voice_capture_result(
    content: &str,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Option<String> {
    let parsed: JsonValue = match serde_json::from_str(content) {
        Ok(value) => value,
        Err(_) => {
            event_tx
                .send(AgentEvent::VoiceNoSpeech {
                    reason: "Voice capture returned an unreadable response.".to_string(),
                    audio_path: None,
                })
                .ok();
            return None;
        }
    };

    let audio_path = parsed
        .get("audio_path")
        .and_then(JsonValue::as_str)
        .map(str::to_string);
    let transcript = parsed
        .get("transcript")
        .and_then(JsonValue::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);

    if let Some(transcript) = transcript {
        event_tx
            .send(AgentEvent::VoiceTranscript {
                transcript: transcript.clone(),
                audio_path,
            })
            .ok();
        return Some(transcript);
    }

    let reason = parsed
        .get("recording")
        .and_then(|recording| recording.get("rejection_reason"))
        .and_then(JsonValue::as_str)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or("No speech transcript detected.")
        .to_string();
    event_tx
        .send(AgentEvent::VoiceNoSpeech { reason, audio_path })
        .ok();
    None
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

#[tokio::main]
async fn main() -> Result<()> {
    let runtime_home = hakimi_common::RuntimeHome::resolve_default(None)?;
    bind_runtime_home_env(&runtime_home);

    // Initialize logging to a file (not stdout, since we own the terminal).
    let log_path = runtime_home.home().join("tui.log");

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
    let config = load_config(&runtime_home);
    let model = resolve_model(&config);

    let agent = match build_agent(&config, &runtime_home).await {
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
    let mut app = App::new(cmd_tx, event_rx, model, session_id)
        .with_config(&config)
        .with_voice_config(&config.voice)
        .with_session_db_path(runtime_home.sessions_db_path())
        .with_skills_dir_path(runtime_home.skills_dir())
        .with_cron_db_path(runtime_home.cron_db_path())
        .with_knowledge_home_path(runtime_home.home());

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
