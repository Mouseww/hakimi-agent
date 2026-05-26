//! Hakimi Agent CLI entry point.
//!
//! Contains the clap [`Args`], configuration loading, agent construction, and
//! the interactive REPL / single-query / server modes so that both the
//! `hakimi-cli` binary and the thin `hakimi-agent` wrapper can share the same
//! implementation.

use anyhow::Result;
use clap::{Parser, ValueEnum};
use std::io::{self, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, info, warn};

use crate::Command;

#[derive(Clone, Default)]
struct GatewayTaskControl {
    id: uuid::Uuid,
    token: CancellationToken,
}

impl GatewayTaskControl {
    fn cancel(&self) {
        self.token.cancel();
    }
}

fn gateway_task_key(platform: &str, bot_id: &str, chat_id: &str) -> String {
    format!("{platform}:{bot_id}:{chat_id}")
}

async fn send_gateway_text(
    gateway: &hakimi_gateway::Gateway,
    platform: &str,
    bot_id: &str,
    chat_id: &str,
    text: impl Into<String>,
) {
    let msg = hakimi_gateway::GatewayMessage {
        platform: platform.to_string(),
        bot_id: bot_id.to_string(),
        chat_id: chat_id.to_string(),
        user_id: String::new(),
        text: text.into(),
        media: None,
    };
    let _ = gateway.route_message(&msg).await;
}

enum GatewayStreamUiEvent {
    Content(String),
    Tool(String),
    Delegate(DelegateProgressEvent),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DelegateProgressEvent {
    task_id: String,
    title: String,
    line: String,
    timestamp: String,
}

impl DelegateProgressEvent {
    fn parse(raw: &str) -> Option<Self> {
        let mut parts = raw.splitn(4, '|');
        let task_id = parts.next()?.trim();
        let title = parts.next()?.trim();
        let line = parts.next()?.trim();
        let timestamp = parts.next()?.trim();
        if task_id.is_empty() || title.is_empty() || line.is_empty() || timestamp.is_empty() {
            return None;
        }
        Some(Self {
            task_id: task_id.to_string(),
            title: title.to_string(),
            line: line.to_string(),
            timestamp: timestamp.to_string(),
        })
    }
}

#[derive(Debug, Clone, Default)]
struct DelegateProgressBubble {
    title: String,
    lines: Vec<(String, String)>,
    message_id: Option<i64>,
}

impl DelegateProgressBubble {
    fn push(&mut self, event: DelegateProgressEvent) {
        self.title = event.title;
        if self
            .lines
            .last()
            .map(|(line, _)| line.as_str() == event.line.as_str())
            .unwrap_or(false)
        {
            if let Some((_, ts)) = self.lines.last_mut() {
                *ts = event.timestamp;
            }
            return;
        }
        self.lines.push((event.line, event.timestamp));
        const MAX_PROGRESS_LINES: usize = 14;
        if self.lines.len() > MAX_PROGRESS_LINES {
            let overflow = self.lines.len() - MAX_PROGRESS_LINES;
            self.lines.drain(0..overflow);
        }
    }

    fn render(&self) -> String {
        let mut out = format!("**{}**\n```text\n", self.title);
        for (line, timestamp) in &self.lines {
            out.push_str(line);
            out.push_str("  ");
            out.push_str(timestamp);
            out.push('\n');
        }
        out.push_str("```");
        out
    }
}

#[derive(Debug, Default)]
struct GatewayChatTurnTracker {
    active_turns: usize,
    seen_concurrent_input: bool,
}

impl GatewayChatTurnTracker {
    fn start_turn(&mut self) -> bool {
        let already_busy = self.active_turns > 0;
        if already_busy {
            self.seen_concurrent_input = true;
        }
        self.active_turns += 1;
        already_busy
    }

    fn finish_turn(&mut self) {
        self.active_turns = self.active_turns.saturating_sub(1);
    }

    fn decorate_user_text(&self, text: &str, concurrent: bool) -> String {
        if concurrent {
            format!(
                "[Gateway concurrent input: the user sent this while a previous request in this chat was still running. Treat it as either supplemental context for the ongoing work or as a separate task, based on intent. Do not ignore it.]\n\n{text}"
            )
        } else {
            text.to_string()
        }
    }
}

fn should_edit_initial_gateway_message(
    initial_message_id: Option<i64>,
    is_error: bool,
    rendered_stream_content: bool,
) -> bool {
    initial_message_id.is_some() && (is_error || !rendered_stream_content)
}

fn resolve_clawbot_gateway_config(
    config: &hakimi_config::HakimiConfig,
) -> hakimi_config::ClawBotGatewayConfig {
    let mut resolved = config.gateways.clawbot.clone();
    if let Some(role_cfg) = config
        .roles
        .get("default")
        .and_then(|role| role.gateways.clawbot.clone())
    {
        if role_cfg.enabled {
            resolved.enabled = true;
        }
        if role_cfg.mode != resolved.mode {
            resolved.mode = role_cfg.mode;
        }
        if !role_cfg.bot_id.is_empty() {
            resolved.bot_id = role_cfg.bot_id;
        }
        if !role_cfg.base_url.is_empty() {
            resolved.base_url = role_cfg.base_url;
        }
        if !role_cfg.token.is_empty() {
            resolved.token = role_cfg.token;
        }
        if !role_cfg.poll_path.is_empty() {
            resolved.poll_path = role_cfg.poll_path;
        }
        if !role_cfg.send_path.is_empty() {
            resolved.send_path = role_cfg.send_path;
        }
        if !role_cfg.edit_path.is_empty() {
            resolved.edit_path = role_cfg.edit_path;
        }
        if role_cfg.poll_interval_ms > 0 {
            resolved.poll_interval_ms = role_cfg.poll_interval_ms;
        }
        if role_cfg.poll_limit > 0 {
            resolved.poll_limit = role_cfg.poll_limit;
        }
        if !role_cfg.token_store.is_empty() {
            resolved.token_store = role_cfg.token_store;
        }
        if !role_cfg.channel_version.is_empty() {
            resolved.channel_version = role_cfg.channel_version;
        }
        if !role_cfg.app_client_version.is_empty() {
            resolved.app_client_version = role_cfg.app_client_version;
        }
        if !role_cfg.login_notify_platform.is_empty() {
            resolved.login_notify_platform = role_cfg.login_notify_platform;
        }
        if !role_cfg.login_notify_bot_id.is_empty() {
            resolved.login_notify_bot_id = role_cfg.login_notify_bot_id;
        }
        if !role_cfg.login_notify_chat_id.is_empty() {
            resolved.login_notify_chat_id = role_cfg.login_notify_chat_id;
        }
    }

    if let Ok(url) = std::env::var("CLAWBOT_BASE_URL")
        && !url.trim().is_empty()
    {
        resolved.base_url = url;
        resolved.enabled = true;
    }
    if let Ok(token) = std::env::var("CLAWBOT_TOKEN")
        && !token.trim().is_empty()
    {
        resolved.token = token;
        resolved.enabled = true;
    }
    if let Ok(mode) = std::env::var("CLAWBOT_MODE")
        && !mode.trim().is_empty()
    {
        resolved.mode = mode;
        resolved.enabled = true;
    }
    resolved
}

fn parse_clawbot_mode(mode: &str) -> hakimi_gateway::ClawBotMode {
    match mode.trim().to_ascii_lowercase().as_str() {
        "ilink_native" | "ilink" | "native" => hakimi_gateway::ClawBotMode::IlinkNative,
        "weclawbot_api" | "weclawbot" => hakimi_gateway::ClawBotMode::WeClawBotApi,
        _ => hakimi_gateway::ClawBotMode::HttpBridge,
    }
}

fn merge_gateway_receivers(
    receivers: Vec<(
        String,
        String,
        tokio::sync::mpsc::UnboundedReceiver<hakimi_gateway::GatewayMessage>,
    )>,
) -> Result<tokio::sync::mpsc::UnboundedReceiver<hakimi_gateway::GatewayMessage>> {
    if receivers.is_empty() {
        anyhow::bail!("no platform adapter receivers available");
    }
    let (merged_tx, merged_rx) = tokio::sync::mpsc::unbounded_channel();
    for (platform, bot_id, mut receiver) in receivers {
        let tx = merged_tx.clone();
        tokio::spawn(async move {
            while let Some(msg) = receiver.recv().await {
                if tx.send(msg).is_err() {
                    break;
                }
            }
            tracing::info!(platform = %platform, bot_id = %bot_id, "gateway receiver stream ended");
        });
    }
    drop(merged_tx);
    Ok(merged_rx)
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum GatewayMode {
    /// Start gateway mode in the current foreground process.
    Start,
    /// Install/update the managed systemd gateway service and start it.
    Install,
    /// Restart the managed systemd gateway service, then exit.
    Restart,
    /// Show status for the managed systemd gateway service, then exit.
    Status,
}

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
    ///
    /// Optional mode: `start` (default) runs in the current process; `restart`
    /// restarts the managed systemd service and exits.
    #[arg(long, value_enum, num_args = 0..=1, default_missing_value = "start")]
    pub gateway: Option<GatewayMode>,

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

gateways:
  clawbot:
    enabled: false
    mode: "http_bridge"   # http_bridge | weclawbot_api | ilink_native
    bot_id: "clawbot"
    base_url: "http://127.0.0.1:5700"
    token: ""
    poll_path: "/messages"
    send_path: "/send_message"
    edit_path: "/edit_message"
    poll_interval_ms: 1000
    poll_limit: 50
    token_store: "~/.hakimi/clawbot"
    channel_version: "1.0.2"
    app_client_version: "2.4.3"

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

fn prompt_secret_optional(label: &str, existing: &str) -> Result<String> {
    if existing.is_empty() {
        print!("{label} (Enter = skip): ");
    } else {
        print!("{label} ([configured], Enter = keep, 'skip' = clear): ");
    }
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("skip") {
        Ok(String::new())
    } else if trimmed.is_empty() {
        Ok(existing.to_string())
    } else {
        Ok(trimmed.to_string())
    }
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

fn prompt_multi_select(label: &str, options: &[&str]) -> Result<Vec<usize>> {
    println!("{label}");
    for (idx, option) in options.iter().enumerate() {
        println!("  {}. {}", idx + 1, option);
    }
    print!("Select numbers separated by comma/space (Enter = skip): ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let mut selected = Vec::new();
    for part in input
        .split(|c: char| c == ',' || c.is_whitespace())
        .map(str::trim)
        .filter(|part| !part.is_empty())
    {
        if let Ok(number) = part.parse::<usize>()
            && (1..=options.len()).contains(&number)
        {
            let idx = number - 1;
            if !selected.contains(&idx) {
                selected.push(idx);
            }
        }
    }
    Ok(selected)
}

fn configure_telegram_gateway(config: &mut hakimi_config::HakimiConfig) -> Result<()> {
    println!("\n━━━ Telegram gateway ━━━");
    config.gateways.telegram.bot_token = prompt_secret_optional(
        "Telegram bot token (@BotFather)",
        &config.gateways.telegram.bot_token,
    )?;

    let default_allowed = config
        .gateways
        .telegram
        .allowed_users
        .iter()
        .map(i64::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let allowed = prompt_optional(
        "Allowed Telegram user IDs (comma-separated, empty = allow all)",
        &default_allowed,
    )?;
    config.gateways.telegram.allowed_users = allowed
        .split(',')
        .filter_map(|value| value.trim().parse::<i64>().ok())
        .collect();

    let role = config.roles.entry("default".to_string()).or_default();
    role.gateways.telegram = Some(hakimi_config::RoleTelegramConfig {
        bot_token: config.gateways.telegram.bot_token.clone(),
    });
    if role.allowed_users.is_empty() {
        role.allowed_users = config.gateways.telegram.allowed_users.clone();
    }
    Ok(())
}

fn configure_clawbot_gateway(config: &mut hakimi_config::HakimiConfig) -> Result<()> {
    println!("\n━━━ WeChat ClawBot gateway ━━━");
    println!(
        "Recommended mode: ilink_native. It performs QR login in the background and keeps other gateways online."
    );

    config.gateways.clawbot.enabled = true;
    config.gateways.clawbot.mode = prompt_text(
        "Mode (ilink_native/http_bridge/weclawbot_api)",
        "ilink_native",
    )?;
    config.gateways.clawbot.bot_id = prompt_text("Bot id", &config.gateways.clawbot.bot_id)?;
    config.gateways.clawbot.base_url =
        prompt_text("ClawBot/iLink base URL", &config.gateways.clawbot.base_url)?;
    config.gateways.clawbot.token = prompt_secret_optional(
        "Existing ClawBot token (skip for QR login)",
        &config.gateways.clawbot.token,
    )?;
    config.gateways.clawbot.token_store = prompt_text(
        "Token store directory",
        &config.gateways.clawbot.token_store,
    )?;

    let notify_default = if config.gateways.clawbot.login_notify_platform.is_empty() {
        "telegram"
    } else {
        &config.gateways.clawbot.login_notify_platform
    };
    config.gateways.clawbot.login_notify_platform =
        prompt_optional("QR login notify platform", notify_default)?;
    config.gateways.clawbot.login_notify_bot_id = prompt_optional(
        "QR login notify bot id (empty = default telegram_bot)",
        &config.gateways.clawbot.login_notify_bot_id,
    )?;
    config.gateways.clawbot.login_notify_chat_id = prompt_optional(
        "QR login notify chat id (optional)",
        &config.gateways.clawbot.login_notify_chat_id,
    )?;

    let role = config.roles.entry("default".to_string()).or_default();
    role.gateways.clawbot = Some(hakimi_config::RoleClawBotConfig {
        enabled: true,
        mode: config.gateways.clawbot.mode.clone(),
        bot_id: config.gateways.clawbot.bot_id.clone(),
        base_url: config.gateways.clawbot.base_url.clone(),
        token: config.gateways.clawbot.token.clone(),
        poll_path: config.gateways.clawbot.poll_path.clone(),
        send_path: config.gateways.clawbot.send_path.clone(),
        edit_path: config.gateways.clawbot.edit_path.clone(),
        poll_interval_ms: config.gateways.clawbot.poll_interval_ms,
        poll_limit: config.gateways.clawbot.poll_limit,
        token_store: config.gateways.clawbot.token_store.clone(),
        channel_version: config.gateways.clawbot.channel_version.clone(),
        app_client_version: config.gateways.clawbot.app_client_version.clone(),
        login_notify_platform: config.gateways.clawbot.login_notify_platform.clone(),
        login_notify_bot_id: config.gateways.clawbot.login_notify_bot_id.clone(),
        login_notify_chat_id: config.gateways.clawbot.login_notify_chat_id.clone(),
    });
    Ok(())
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

    let selections = prompt_multi_select(
        "\nConfigure gateway platforms (multi-select):",
        &[
            "Telegram — bot token from @BotFather",
            "WeChat ClawBot — iLink native QR login / HTTP bridge",
        ],
    )?;
    for selection in selections {
        match selection {
            0 => configure_telegram_gateway(&mut config)?,
            1 => configure_clawbot_gateway(&mut config)?,
            _ => {}
        }
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
    println!("  hakimi --gateway install   # install/start managed gateway service");
    println!("  hakimi --gateway restart   # restart without loading the agent");
    println!("  hakimi --gateway           # foreground gateway mode");
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
    let bot_token = std::env::var("TELEGRAM_BOT_TOKEN")
        .ok()
        .filter(|token| !token.trim().is_empty())
        .or_else(|| {
            config
                .roles
                .get("default")
                .and_then(|r| r.gateways.telegram.as_ref().map(|t| t.bot_token.clone()))
                .filter(|token| !token.trim().is_empty())
        })
        .or_else(|| {
            if config.gateways.telegram.bot_token.trim().is_empty() {
                None
            } else {
                Some(config.gateways.telegram.bot_token.clone())
            }
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

    let clawbot_config = resolve_clawbot_gateway_config(&config);
    if clawbot_config.enabled {
        let clawbot = hakimi_gateway::ClawBotAdapter::new(hakimi_gateway::ClawBotAdapterConfig {
            mode: parse_clawbot_mode(&clawbot_config.mode),
            bot_id: clawbot_config.bot_id,
            base_url: clawbot_config.base_url,
            token: clawbot_config.token,
            poll_path: clawbot_config.poll_path,
            send_path: clawbot_config.send_path,
            edit_path: clawbot_config.edit_path,
            poll_interval_ms: clawbot_config.poll_interval_ms,
            poll_limit: clawbot_config.poll_limit,
            token_store: clawbot_config.token_store,
            channel_version: clawbot_config.channel_version,
            app_client_version: clawbot_config.app_client_version,
            login_notify_platform: clawbot_config.login_notify_platform,
            login_notify_bot_id: clawbot_config.login_notify_bot_id,
            login_notify_chat_id: clawbot_config.login_notify_chat_id,
        });
        gateway.add_adapter(Box::new(clawbot));
        info!("clawbot gateway registered");
    }

    // Load roles context correctly when receiving messages from specific platforms
    // Agent and conversation history map.
    // We use a Mutex to protect the agent because it maintains state.
    // In a production multi-user scenario, you'd want per-chat agents.
    let agent_arc = Arc::new(Mutex::new(agent));
    let histories_clone: Arc<Mutex<HashMap<String, Vec<hakimi_common::Message>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let turn_trackers: Arc<Mutex<HashMap<String, GatewayChatTurnTracker>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let active_tasks: Arc<Mutex<HashMap<String, GatewayTaskControl>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let skill_store_ref = Arc::new(skill_store);

    // 3. Connect all platforms.
    gateway.connect_all().await?;
    let receivers = gateway.take_all_receivers();
    let gateway = Arc::new(gateway);
    let mut messages = merge_gateway_receivers(receivers)?;

    info!("gateway listening for messages");

    // Spawn a background task to process queued outbound messages
    let gateway_queue = gateway.clone();
    tokio::spawn(async move {
        loop {
            if let Some(queued) = hakimi_tools::builtin_send_message::pop_message() {
                let mut target_platform = "telegram".to_string();
                let mut target_chat = queued.session_id.clone();
                let mut bot_id = "telegram_bot".to_string();

                if queued.target != "origin"
                    && let Some((p, c)) = queued.target.split_once(':')
                {
                    target_platform = p.to_string();
                    target_chat = c.to_string();
                    if target_platform == "clawbot" {
                        bot_id = "clawbot".to_string();
                    }
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

        if platform == "__hakimi_system__" {
            let mut routed = msg.clone();
            if let Some((_, target_platform)) = text.rsplit_once("HAKIMI_ROUTE_PLATFORM=") {
                routed.platform = target_platform.trim().to_string();
                routed.text = text
                    .replace(
                        &format!("\n\nHAKIMI_ROUTE_PLATFORM={}", target_platform.trim()),
                        "",
                    )
                    .trim()
                    .to_string();
            } else {
                routed.platform = "telegram".to_string();
            }
            if let Err(err) = gateway.route_message(&routed).await {
                tracing::warn!(error = %err, "failed to route internal gateway notification");
            }
            continue;
        }

        info!(platform = %platform, chat_id = %chat_id, has_media = media_id.is_some(), "received message via gateway");

        if text.starts_with('/') {
            match Command::parse(&text) {
                Some(Command::Stop) => {
                    let key = gateway_task_key(&platform, &bot_id, &chat_id);
                    let stopped = {
                        let mut active = active_tasks.lock().await;
                        active
                            .remove(&key)
                            .map(|control| {
                                control.cancel();
                            })
                            .is_some()
                    };
                    let response = if stopped {
                        "⏹️ 已停止当前任务。"
                    } else {
                        "ℹ️ 当前没有正在运行的任务。"
                    };
                    send_gateway_text(&gateway, &platform, &bot_id, &chat_id, response).await;
                    continue;
                }
                Some(Command::Restart) => {
                    send_gateway_text(
                        &gateway,
                        &platform,
                        &bot_id,
                        &chat_id,
                        "🔄 正在重启 Hakimi Gateway...",
                    )
                    .await;
                    tokio::spawn(async move {
                        let result = tokio::task::spawn_blocking(restart_gateway_service).await;
                        if let Err(err) = result {
                            tracing::error!(error = %err, "failed to join gateway restart task");
                        }
                    });
                    continue;
                }
                _ => {}
            }
        }

        let agent_clone = agent_arc.clone();
        let gateway_clone = gateway.clone();
        let skill_store_ref = skill_store_ref.clone();
        let histories_clone = histories_clone.clone();
        let turn_trackers = turn_trackers.clone();
        let active_tasks = active_tasks.clone();

        let config_clone = config.clone();
        tokio::spawn(async move {
            let text = text.clone();
            let media_id = media_id.clone();
            let chat_id = chat_id.clone();
            let bot_id = bot_id.clone();
            let platform = platform.clone();
            let config = config_clone;
            let task_key = gateway_task_key(&platform, &bot_id, &chat_id);
            let task_id = uuid::Uuid::new_v4();
            let cancellation = CancellationToken::new();
            {
                let mut active = active_tasks.lock().await;
                if let Some(previous) = active.insert(
                    task_key.clone(),
                    GatewayTaskControl {
                        id: task_id,
                        token: cancellation.clone(),
                    },
                ) {
                    previous.cancel();
                    debug!(platform = %platform, chat_id = %chat_id, "cancelled previous active gateway task for chat");
                }
            }

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
                let cancellation = cancellation.clone();
                tokio::spawn(async move {
                    loop {
                        tokio::select! {
                            _ = cancellation.cancelled() => break,
                            _ = gateway.send_chat_action(&bot_id, &chat_id, "typing") => {}
                        }
                        tokio::select! {
                            _ = cancellation.cancelled() => break,
                            _ = tokio::time::sleep(std::time::Duration::from_secs(4)) => {}
                        }
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
                        help.push_str("• `/restart` - Restart Hakimi Gateway service\n");
                        help.push_str("• `/stop` - Stop current background task or streaming\n");
                        help.push_str("• `/memory` - View or clear persistent memory\n");
                        help.push_str("• `/checkpoints` - Manage file system checkpoints\n");
                        help.push_str("\nJust send a message to chat with me!");
                        help
                    }
                    Some(Command::Stop) => {
                        cancellation.cancel();
                        "⏹️ 已停止当前任务。".to_string()
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
                    Some(Command::Restart) => "🔄 正在重启 Hakimi Gateway...".to_string(),
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

            // Process the message with an isolated turn agent. Never hold the
            // shared gateway agent lock across the LLM/tool loop; otherwise a
            // second Telegram message waits behind the first one and appears
            // to be ignored.
            let (mut turn_agent, base_history_len, is_concurrent_turn) = {
                let mut trackers = turn_trackers.lock().await;
                let tracker = trackers.entry(chat_id.clone()).or_default();
                let concurrent = tracker.start_turn();
                drop(trackers);

                let mut a = agent_clone.lock().await.clone();

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

                let base_history_len = {
                    let histories = histories_clone.lock().await;
                    let chat_msgs = histories.get(&chat_id).cloned().unwrap_or_default();
                    let len = chat_msgs.len();
                    a.clear_messages();
                    for m in chat_msgs {
                        a.add_message(m);
                    }
                    len
                };

                (a, base_history_len, concurrent)
            };

            let (response_text, err_msg, rendered_stream_content) = {
                let mut updater_handle = None;
                let mut rendered_content = None;

                if let Some(msg_id) = initial_message_id {
                    let platform_cb = platform.clone();
                    let bot_id_cb = bot_id.clone();
                    let chat_id_cb = chat_id.clone();
                    let gateway_cb = gateway_clone.clone();
                    let rendered_content_flag = Arc::new(AtomicBool::new(false));
                    let rendered_content_for_task = rendered_content_flag.clone();
                    rendered_content = Some(rendered_content_flag.clone());

                    let (ui_tx, mut ui_rx) =
                        tokio::sync::mpsc::unbounded_channel::<GatewayStreamUiEvent>();

                    let handle = tokio::spawn(async move {
                        let mut current_message_id = Some(msg_id);
                        let mut ui_state = GatewayStreamUiState::default();
                        let mut delegate_bubbles: HashMap<String, DelegateProgressBubble> =
                            HashMap::new();
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
                                            GatewayStreamUiEvent::Tool(_)
                                            | GatewayStreamUiEvent::Delegate(_) => {
                                                pending_events.push_back(next);
                                                break;
                                            }
                                        }
                                    }

                                    let Some(target) = ui_state.append_content(&text) else {
                                        continue;
                                    };
                                    rendered_content_for_task.store(true, Ordering::Relaxed);

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
                                GatewayStreamUiEvent::Delegate(event) => {
                                    let task_id = event.task_id.clone();
                                    let bubble = delegate_bubbles.entry(task_id).or_default();
                                    bubble.push(event);
                                    let rendered = bubble.render();

                                    if let Some(progress_msg_id) = bubble.message_id {
                                        let _ = gateway_cb
                                            .edit_message(
                                                &platform_cb,
                                                &bot_id_cb,
                                                &chat_id_cb,
                                                progress_msg_id,
                                                &rendered,
                                            )
                                            .await;
                                    } else {
                                        let msg = hakimi_gateway::GatewayMessage {
                                            platform: platform_cb.clone(),
                                            bot_id: bot_id_cb.clone(),
                                            chat_id: chat_id_cb.clone(),
                                            user_id: String::new(),
                                            text: rendered,
                                            media: None,
                                        };
                                        bubble.message_id = gateway_cb
                                            .route_message_get_id(&msg)
                                            .await
                                            .ok()
                                            .flatten();
                                    }

                                    current_message_id = None;
                                    ui_state.finish_tool_boundary();
                                }
                            }
                        }
                    });
                    updater_handle = Some(handle);

                    let callback = move |token: String| {
                        if let Some(review_notice) = token.strip_prefix("\u{001e}hakimi_review:") {
                            let text = review_notice.trim().to_string();
                            if !text.is_empty() {
                                let _ = ui_tx.send(GatewayStreamUiEvent::Tool(text));
                            }
                            return;
                        }
                        if let Some(tool_notice) = token.strip_prefix("\u{001e}hakimi_tool:") {
                            let text = tool_notice.trim().to_string();
                            if !text.is_empty() {
                                let _ = ui_tx.send(GatewayStreamUiEvent::Tool(text));
                            }
                            return;
                        }
                        if let Some(delegate_notice) =
                            token.strip_prefix("\u{001e}hakimi_delegate:")
                        {
                            if let Some(event) = DelegateProgressEvent::parse(delegate_notice) {
                                let _ = ui_tx.send(GatewayStreamUiEvent::Delegate(event));
                            }
                            return;
                        }
                        let _ = ui_tx.send(GatewayStreamUiEvent::Content(token));
                    };
                    turn_agent.set_streaming_callback(Some(std::sync::Arc::new(callback)));
                }

                let user_text = {
                    let trackers = turn_trackers.lock().await;
                    trackers
                        .get(&chat_id)
                        .map(|tracker| tracker.decorate_user_text(&text, is_concurrent_turn))
                        .unwrap_or_else(|| text.clone())
                };

                let mut msg = hakimi_common::Message::user(&user_text);
                if !images.is_empty() {
                    msg = msg.with_images(images);
                }

                let result = tokio::select! {
                    _ = cancellation.cancelled() => Err(hakimi_common::HakimiError::Other("cancelled by /stop".to_string())),
                    result = async {
                        if config.model.api_mode.as_str() == "REST" {
                            turn_agent
                                .run_conversation_with_message(msg)
                                .await
                                .map(|r| r.final_response)
                        } else {
                            turn_agent.chat_streaming_with_message(msg).await
                        }
                    } => result,
                };

                turn_agent.set_streaming_callback(None);
                if let Some(handle) = updater_handle {
                    let _ = handle.await;
                }

                let stream_rendered = rendered_content
                    .as_ref()
                    .map(|flag| flag.load(Ordering::Relaxed))
                    .unwrap_or(false);

                match result {
                    Ok(res) => {
                        let updated_msgs = turn_agent.messages().to_vec();
                        let new_msgs = updated_msgs
                            .get(base_history_len..)
                            .map(|msgs| msgs.to_vec())
                            .unwrap_or_else(Vec::new);
                        {
                            let mut histories = histories_clone.lock().await;
                            let chat_history = histories.entry(chat_id.clone()).or_default();
                            chat_history.extend(new_msgs);
                        }
                        (res, None, stream_rendered)
                    }
                    Err(e) if e.to_string() == "cancelled by /stop" => {
                        debug!(platform = %platform, chat_id = %chat_id, "gateway task cancelled by /stop");
                        (
                            String::new(),
                            Some("⏹️ 已停止当前任务。".to_string()),
                            stream_rendered,
                        )
                    }
                    Err(e) => {
                        error!(error = %e, "agent streaming query failed");
                        (
                            String::new(),
                            Some(format!("❌ Error: {e}")),
                            stream_rendered,
                        )
                    }
                }
            };

            typing_handle.abort();
            cancellation.cancel();
            {
                let mut active = active_tasks.lock().await;
                if let Some(control) = active.get(&task_key)
                    && control.id == task_id
                {
                    active.remove(&task_key);
                }
            }
            {
                let mut trackers = turn_trackers.lock().await;
                if let Some(tracker) = trackers.get_mut(&chat_id) {
                    tracker.finish_turn();
                    if tracker.active_turns == 0 && !tracker.seen_concurrent_input {
                        trackers.remove(&chat_id);
                    }
                }
            }

            let is_error = err_msg.is_some();
            let final_text = err_msg.unwrap_or(response_text);

            if should_edit_initial_gateway_message(
                initial_message_id,
                is_error,
                rendered_stream_content,
            ) {
                if let Some(msg_id) = initial_message_id {
                    let _ = gateway_clone
                        .edit_message(&platform, &bot_id, &chat_id, msg_id, &final_text)
                        .await;
                }
            } else if initial_message_id.is_none() {
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
fn gateway_service_name() -> String {
    std::env::var("HAKIMI_GATEWAY_SERVICE")
        .ok()
        .filter(|name| !name.trim().is_empty())
        .unwrap_or_else(|| "hakimi".to_string())
}

fn restart_gateway_service() -> Result<()> {
    use std::process::Command as ProcessCommand;

    let service = gateway_service_name();

    let status = ProcessCommand::new("systemctl")
        .arg("restart")
        .arg(&service)
        .status()?;
    if !status.success() {
        anyhow::bail!("failed to restart gateway service `{service}` (exit status: {status})");
    }

    println!("✅ Gateway service `{service}` restarted.");
    Ok(())
}

fn install_gateway_service() -> Result<()> {
    use std::process::Command as ProcessCommand;

    let service = gateway_service_name();
    let unit_path = format!("/etc/systemd/system/{service}.service");
    let exe = std::env::current_exe()?;
    let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("/root"));
    let user = std::env::var("USER").unwrap_or_else(|_| "root".to_string());
    let unit = format!(
        "[Unit]\nDescription=Hakimi Agent Gateway\nAfter=network-online.target\nWants=network-online.target\n\n[Service]\nType=simple\nUser={user}\nWorkingDirectory={home}\nEnvironment=HOME={home}\nExecStart={exe} --gateway start\nRestart=always\nRestartSec=3\n\n[Install]\nWantedBy=multi-user.target\n",
        user = user,
        home = home.display(),
        exe = exe.display()
    );

    std::fs::write(&unit_path, unit)?;
    for args in [
        vec!["daemon-reload"],
        vec!["enable", &service],
        vec!["restart", &service],
    ] {
        let status = ProcessCommand::new("systemctl").args(args).status()?;
        if !status.success() {
            anyhow::bail!("systemctl failed while installing `{service}` (exit status: {status})");
        }
    }
    println!("✅ Gateway service `{service}` installed and started.");
    println!("   Unit: {unit_path}");
    Ok(())
}

fn gateway_service_status() -> Result<()> {
    use std::process::Command as ProcessCommand;

    let service = gateway_service_name();
    let output = ProcessCommand::new("systemctl")
        .arg("status")
        .arg(&service)
        .arg("--no-pager")
        .arg("-l")
        .output()?;
    print!("{}", String::from_utf8_lossy(&output.stdout));
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    if !output.status.success() {
        anyhow::bail!(
            "gateway service `{service}` is not active (exit status: {})",
            output.status
        );
    }
    Ok(())
}

fn resolve_hakimi_update_target(current_exe: &std::path::Path) -> std::path::PathBuf {
    if let Ok(path_env) = std::env::var("PATH") {
        for dir in std::env::split_paths(&path_env) {
            let candidate = dir.join("hakimi");
            if candidate.exists()
                && let Ok(canonical) = std::fs::canonicalize(&candidate)
                && canonical == current_exe
            {
                return candidate;
            }
        }
    }
    current_exe.to_path_buf()
}

async fn latest_release_tag(client: &reqwest::Client) -> Result<String> {
    let api = "https://api.github.com/repos/Mouseww/hakimi-agent/releases/latest";
    let value: serde_json::Value = client
        .get(api)
        .header(reqwest::header::USER_AGENT, "hakimi-self-update")
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;
    value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .map(|tag| tag.to_string())
        .ok_or_else(|| anyhow::anyhow!("GitHub latest release response missing tag_name"))
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

    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::limited(5))
        .build()?;
    let latest_tag = latest_release_tag(&client).await?;
    println!("Latest release: {latest_tag}");

    let url = format!(
        "https://github.com/Mouseww/hakimi-agent/releases/download/{latest_tag}/hakimi-{arch_str}-{platform}.{ext}"
    );
    println!("Downloading: {url}");

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

    // Determine update target. Prefer the `hakimi` found on PATH so `hakimi --update`
    // updates the command users actually run, even when current_exe resolves through a
    // symlink or a renamed wrapper binary.
    let current_exe = env::current_exe()?;
    let current_exe = fs::canonicalize(&current_exe).unwrap_or(current_exe);
    let update_target = resolve_hakimi_update_target(&current_exe);
    let backup_path = format!("{}.bak", update_target.display());
    println!("Installing to: {}", update_target.display());

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
    fs::copy(&update_target, &backup_path)?;
    println!("Backed up current binary to {backup_path}");

    let install_tmp = update_target.with_extension(format!(
        "hakimi-update-{}",
        chrono::Local::now().format("%Y%m%d%H%M%S")
    ));
    fs::write(&install_tmp, &binary_data)?;

    // Set executable permissions (Unix)
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&install_tmp, fs::Permissions::from_mode(0o755))?;
    }

    fs::rename(&install_tmp, &update_target)?;

    // Verify new binary works and reports the expected latest version.
    let output = std::process::Command::new(&update_target)
        .arg("--version")
        .output();

    match output {
        Ok(o) if o.status.success() => {
            let version_text = String::from_utf8_lossy(&o.stdout);
            if !version_text.contains(latest_tag.trim_start_matches('v')) {
                let _ = fs::copy(&backup_path, &update_target);
                anyhow::bail!(
                    "updated binary reported `{}` instead of `{latest_tag}`; previous version restored",
                    version_text.trim()
                );
            }
            println!(
                "✅ Updated successfully to {latest_tag}: {}",
                version_text.trim()
            );
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
            fs::copy(&backup_path, &update_target)?;
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

    if matches!(args.gateway, Some(GatewayMode::Install)) {
        return install_gateway_service();
    }
    if matches!(args.gateway, Some(GatewayMode::Restart)) {
        return restart_gateway_service();
    }
    if matches!(args.gateway, Some(GatewayMode::Status)) {
        return gateway_service_status();
    }

    let agent = build_agent(&args, &config).await?;

    if args.serve {
        return start_server(agent, &args.addr, config);
    }
    if args.gateway.is_some() {
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
    use super::{
        DelegateProgressBubble, DelegateProgressEvent, GatewayChatTurnTracker, GatewayMode,
        GatewayStreamUiState, GatewayUiContentTarget, resolve_clawbot_gateway_config,
        resolve_hakimi_update_target, should_edit_initial_gateway_message,
    };
    use clap::ValueEnum;

    #[test]
    fn gateway_mode_supports_install_restart_and_status() {
        assert_eq!(
            GatewayMode::from_str("start", true).unwrap(),
            GatewayMode::Start
        );
        assert_eq!(
            GatewayMode::from_str("install", true).unwrap(),
            GatewayMode::Install
        );
        assert_eq!(
            GatewayMode::from_str("restart", true).unwrap(),
            GatewayMode::Restart
        );
        assert_eq!(
            GatewayMode::from_str("status", true).unwrap(),
            GatewayMode::Status
        );
    }

    #[test]
    fn update_target_falls_back_to_current_exe_when_path_has_no_match() {
        let current = std::path::PathBuf::from("/tmp/hakimi-current-test");
        let resolved = resolve_hakimi_update_target(&current);
        assert_eq!(resolved, current);
    }

    #[test]
    fn resolves_clawbot_gateway_config_from_role_binding() {
        let yaml = r#"
roles:
  default:
    gateways:
      clawbot:
        enabled: true
        bot_id: "wechat-main"
        base_url: "http://127.0.0.1:7777"
        poll_path: "/wx/poll"
        send_path: "/wx/send"
        edit_path: "/wx/edit"
        poll_interval_ms: 250
        poll_limit: 10
"#;
        let config: hakimi_config::HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        let resolved = resolve_clawbot_gateway_config(&config);
        assert!(resolved.enabled);
        assert_eq!(resolved.bot_id, "wechat-main");
        assert_eq!(resolved.base_url, "http://127.0.0.1:7777");
        assert_eq!(resolved.poll_path, "/wx/poll");
        assert_eq!(resolved.send_path, "/wx/send");
        assert_eq!(resolved.edit_path, "/wx/edit");
        assert_eq!(resolved.poll_interval_ms, 250);
        assert_eq!(resolved.poll_limit, 10);
    }

    #[test]
    fn delegate_progress_bubble_renders_single_container_with_timestamps() {
        let event =
            DelegateProgressEvent::parse("child_1|子代理 1 · 检查代码|开始执行任务|09:01:02")
                .unwrap();
        let mut bubble = DelegateProgressBubble::default();
        bubble.push(event);
        bubble.push(
            DelegateProgressEvent::parse(
                "child_1|子代理 1 · 检查代码|⚙️ search_files (pattern: delegate)|09:01:05",
            )
            .unwrap(),
        );

        assert_eq!(bubble.title, "子代理 1 · 检查代码");
        assert_eq!(bubble.lines.len(), 2);
        assert_eq!(
            bubble.render(),
            "**子代理 1 · 检查代码**\n```text\n开始执行任务  09:01:02\n⚙️ search_files (pattern: delegate)  09:01:05\n```"
        );
    }

    #[test]
    fn delegate_progress_bubble_updates_duplicate_line_timestamp() {
        let mut bubble = DelegateProgressBubble::default();
        bubble
            .push(DelegateProgressEvent::parse("child_1|任务|等待并发执行许可|09:01:02").unwrap());
        bubble
            .push(DelegateProgressEvent::parse("child_1|任务|等待并发执行许可|09:01:03").unwrap());

        assert_eq!(bubble.lines.len(), 1);
        assert_eq!(bubble.lines[0].1, "09:01:03");
    }

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

    #[test]
    fn initial_processing_message_is_edited_when_no_stream_content_rendered() {
        assert!(should_edit_initial_gateway_message(Some(42), false, false));
        assert!(should_edit_initial_gateway_message(Some(42), true, true));
        assert!(!should_edit_initial_gateway_message(Some(42), false, true));
        assert!(!should_edit_initial_gateway_message(None, false, false));
    }

    #[test]
    fn concurrent_turn_tracker_decorates_overlapping_user_input() {
        let mut tracker = GatewayChatTurnTracker::default();

        assert!(!tracker.start_turn());
        assert!(
            !tracker
                .decorate_user_text("第一件事", false)
                .contains("Gateway concurrent input")
        );

        assert!(tracker.start_turn());
        let decorated = tracker.decorate_user_text("补充：优先改源码", true);
        assert!(decorated.contains("Gateway concurrent input"));
        assert!(decorated.contains("supplemental context"));
        assert!(decorated.ends_with("补充：优先改源码"));

        tracker.finish_turn();
        assert_eq!(tracker.active_turns, 1);
        tracker.finish_turn();
        assert_eq!(tracker.active_turns, 0);
    }
}
