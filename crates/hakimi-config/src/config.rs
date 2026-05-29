use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Per-credential configuration entry (used in config files).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialConfig {
    /// Optional identifier; auto-generated if omitted.
    pub id: Option<String>,
    /// The API key.
    pub api_key: String,
    /// Provider-specific base URL override.
    pub base_url: Option<String>,
    /// Organization ID.
    pub org_id: Option<String>,
    /// Selection priority (higher = preferred).
    pub priority: Option<i32>,
    /// Max concurrent requests for this credential.
    pub max_concurrent: Option<usize>,
}

/// Configuration for a credential pool (one per provider).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CredentialPoolConfig {
    /// Rotation strategy name: "round_robin", "fill_first", "random", "least_used".
    pub strategy: Option<String>,
    /// Credentials in this pool.
    pub credentials: Vec<CredentialConfig>,
}

/// Model configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Default model identifier (e.g. "gpt-4o", "claude-sonnet-4-20250514").
    #[serde(default)]
    pub default: String,

    /// Provider name (e.g. "openrouter", "anthropic", "auto").
    #[serde(default = "default_provider")]
    pub provider: String,

    /// Base URL for the API endpoint.
    #[serde(default)]
    pub base_url: String,

    /// API mode override: "chat_completions", "responses", "anthropic_messages"
    /// Empty string = auto-detect from provider name
    #[serde(default)]
    pub api_mode: String,

    /// API key for this provider.
    #[serde(default)]
    pub api_key: String,
}

fn default_provider() -> String {
    "auto".to_string()
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: String::new(),
            provider: "auto".to_string(),
            base_url: String::new(),
            api_mode: String::new(),
            api_key: String::new(),
        }
    }
}

/// Terminal / sandbox configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TerminalConfig {
    /// Backend type: "local", "docker", "ssh", "modal", "daytona", "singularity".
    #[serde(default = "default_env_type")]
    pub env_type: String,

    /// Working directory for terminal operations.
    #[serde(default = "default_cwd")]
    pub cwd: String,

    /// Command execution timeout in seconds.
    #[serde(default = "default_terminal_timeout")]
    pub timeout: u64,

    /// Docker image to use (when env_type is "docker").
    #[serde(default)]
    pub docker_image: String,

    /// Environment variables to forward to Docker.
    #[serde(default)]
    pub docker_forward_env: Vec<String>,

    /// Docker volume mounts (host:container).
    #[serde(default)]
    pub docker_volumes: Vec<String>,
}

fn default_env_type() -> String {
    "local".to_string()
}

fn default_cwd() -> String {
    ".".to_string()
}

fn default_terminal_timeout() -> u64 {
    60
}

impl Default for TerminalConfig {
    fn default() -> Self {
        Self {
            env_type: default_env_type(),
            cwd: default_cwd(),
            timeout: default_terminal_timeout(),
            docker_image: String::new(),
            docker_forward_env: Vec::new(),
            docker_volumes: Vec::new(),
        }
    }
}

/// Agent configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    /// Maximum tool-calling iterations per conversation.
    #[serde(default = "default_max_turns")]
    pub max_turns: usize,

    /// Enable verbose logging.
    #[serde(default)]
    pub verbose: bool,

    /// Custom system prompt override.
    #[serde(default)]
    pub system_prompt: String,

    /// Reasoning effort level.
    #[serde(default)]
    pub reasoning_effort: String,

    /// Service tier for the API.
    #[serde(default)]
    pub service_tier: String,

    /// Disabled toolset names.
    #[serde(default)]
    pub disabled_toolsets: Vec<String>,

    /// Path to the skills directory.
    #[serde(default)]
    pub skills_path: String,
}

fn default_max_turns() -> usize {
    90
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            max_turns: default_max_turns(),
            verbose: false,
            system_prompt: String::new(),
            reasoning_effort: String::new(),
            service_tier: String::new(),
            disabled_toolsets: Vec::new(),
            skills_path: String::new(),
        }
    }
}

/// Context compression configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionConfig {
    /// Whether auto-compression is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Context usage threshold to trigger compression (0.0-1.0).
    #[serde(default = "default_compression_threshold")]
    pub threshold: f64,

    /// Target compression ratio.
    #[serde(default = "default_target_ratio")]
    pub target_ratio: f64,

    /// Compression engine type: "smart" (3-tier), "simple" (truncation), or "llm".
    #[serde(default = "default_compression_engine")]
    pub engine: String,

    /// Optional model for LLM-based compression. Empty means use the active model.
    #[serde(default)]
    pub model: String,

    /// Maximum context length in tokens.
    #[serde(default = "default_context_length")]
    pub context_length: usize,
}

fn default_true() -> bool {
    true
}

fn default_compression_threshold() -> f64 {
    0.50
}

fn default_target_ratio() -> f64 {
    0.20
}

fn default_compression_engine() -> String {
    "smart".to_string()
}

fn default_context_length() -> usize {
    128_000
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold: 0.50,
            target_ratio: 0.20,
            engine: default_compression_engine(),
            model: String::new(),
            context_length: default_context_length(),
        }
    }
}

/// Display configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisplayConfig {
    /// Compact output mode.
    #[serde(default)]
    pub compact: bool,

    /// Enable streaming output.
    #[serde(default = "default_true")]
    pub streaming: bool,

    /// UI skin name.
    #[serde(default = "default_skin")]
    pub skin: String,
}

fn default_skin() -> String {
    "default".to_string()
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            compact: false,
            streaming: true,
            skin: "default".to_string(),
        }
    }
}

/// Delegation (sub-agent) configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DelegationConfig {
    /// Max iterations per child agent.
    #[serde(default = "default_max_iterations")]
    pub max_iterations: usize,

    /// Sub-agent model override (empty = inherit parent).
    #[serde(default)]
    pub model: String,

    /// Sub-agent provider override (empty = inherit parent).
    #[serde(default)]
    pub provider: String,

    /// Direct endpoint URL for sub-agents.
    #[serde(default)]
    pub base_url: String,

    /// API key for delegation base_url.
    #[serde(default)]
    pub api_key: String,
}

fn default_max_iterations() -> usize {
    45
}

impl Default for DelegationConfig {
    fn default() -> Self {
        Self {
            max_iterations: 45,
            model: String::new(),
            provider: String::new(),
            base_url: String::new(),
            api_key: String::new(),
        }
    }
}

/// MCP server configuration for a single server.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Command to spawn the MCP server (e.g. "npx", "uvx").
    pub command: String,

    /// Arguments to pass to the command.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables for the server process.
    #[serde(default)]
    pub env: HashMap<String, String>,
}

/// Memory configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// Whether memory loading into system prompt is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Custom memory directory path (default: ~/.hakimi/memory/).
    #[serde(default)]
    pub path: String,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: String::new(),
        }
    }
}

/// Embedding configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbeddingConfig {
    /// Whether embedding support is enabled.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Provider type. Currently `openai-compatible` is supported.
    #[serde(default = "default_embedding_provider")]
    pub provider: String,

    /// Embedding API base URL. Empty or `same-as-llm` means inherit model.base_url.
    #[serde(default)]
    pub base_url: String,

    /// Embedding API key. Empty or `same-as-llm` means inherit resolved model API key.
    #[serde(default)]
    pub api_key: String,

    /// Online embedding model identifier.
    #[serde(default = "default_embedding_model")]
    pub model: String,

    /// Dense embedding dimension. `BAAI/bge-m3` is normally 1024.
    #[serde(default = "default_embedding_dimension")]
    pub dimension: usize,

    /// Batch size for future indexing/search operations.
    #[serde(default = "default_embedding_batch_size")]
    pub batch_size: usize,

    /// L2-normalize vectors after receiving them.
    #[serde(default = "default_true")]
    pub normalize: bool,
}

fn default_embedding_provider() -> String {
    "openai-compatible".to_string()
}

fn default_embedding_model() -> String {
    "BAAI/bge-m3".to_string()
}

fn default_embedding_dimension() -> usize {
    1024
}

fn default_embedding_batch_size() -> usize {
    32
}

impl Default for EmbeddingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            provider: default_embedding_provider(),
            base_url: String::new(),
            api_key: String::new(),
            model: default_embedding_model(),
            dimension: default_embedding_dimension(),
            batch_size: default_embedding_batch_size(),
            normalize: true,
        }
    }
}

/// Voice / TTS configuration section.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VoiceConfig {
    /// Default TTS provider (e.g. "openai", "elevenlabs").
    #[serde(default = "default_voice_provider")]
    pub provider: String,

    /// Default voice model (e.g. "tts-1").
    #[serde(default)]
    pub model: String,

    /// Voice ID or name (e.g. "alloy", "onyx").
    #[serde(default)]
    pub voice: String,

    /// Default transcription model (e.g. "whisper-1").
    #[serde(default)]
    pub transcription_model: String,

    /// Base URL for the TTS API.
    #[serde(default)]
    pub base_url: String,

    /// API key for TTS.
    #[serde(default)]
    pub api_key: String,

    /// Whether to auto-play generated audio.
    #[serde(default)]
    pub auto_play: bool,
}

fn default_voice_provider() -> String {
    "openai".to_string()
}

impl Default for VoiceConfig {
    fn default() -> Self {
        Self {
            provider: default_voice_provider(),
            model: String::new(),
            voice: String::new(),
            transcription_model: String::new(),
            base_url: String::new(),
            api_key: String::new(),
            auto_play: false,
        }
    }
}

/// Tool behavior configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    /// Progressive disclosure for MCP/plugin tools.
    #[serde(default)]
    pub tool_search: hakimi_common::ToolSearchConfig,
}

/// Top-level Hakimi configuration.
///
/// All fields have sensible defaults via `serde(default)` so partial config
/// files work seamlessly.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct HakimiConfig {
    #[serde(default)]
    pub model: ModelConfig,

    #[serde(default)]
    pub terminal: TerminalConfig,

    #[serde(default)]
    pub agent: AgentConfig,

    #[serde(default)]
    pub compression: CompressionConfig,

    #[serde(default)]
    pub display: DisplayConfig,

    #[serde(default)]
    pub delegation: DelegationConfig,

    /// Named MCP servers to connect to at startup.
    /// Key is the server name, value is the server config.
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,

    /// Credential pools keyed by provider name.
    #[serde(default)]
    pub credential_pools: HashMap<String, CredentialPoolConfig>,

    /// Gateway platform configurations.
    #[serde(default)]
    pub gateways: GatewaysConfig,

    /// Memory configuration.
    #[serde(default)]
    pub memory: MemoryConfig,

    /// Embedding configuration.
    #[serde(default)]
    pub embedding: EmbeddingConfig,

    /// Voice / TTS configuration.
    #[serde(default)]
    pub voice: VoiceConfig,

    /// Tool behavior configuration.
    #[serde(default)]
    pub tools: ToolsConfig,

    /// Named roles — each can bind to its own bot(s).
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
}

/// Configuration for all gateway platforms.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewaysConfig {
    #[serde(default)]
    pub telegram: TelegramGatewayConfig,
    #[serde(default)]
    pub clawbot: ClawBotGatewayConfig,
}

/// WeChat ClawBot bridge gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClawBotGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_clawbot_mode")]
    pub mode: String,
    #[serde(default = "default_clawbot_bot_id")]
    pub bot_id: String,
    #[serde(default = "default_clawbot_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default = "default_clawbot_poll_path")]
    pub poll_path: String,
    #[serde(default = "default_clawbot_send_path")]
    pub send_path: String,
    #[serde(default = "default_clawbot_edit_path")]
    pub edit_path: String,
    #[serde(default = "default_clawbot_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_clawbot_poll_limit")]
    pub poll_limit: usize,
    #[serde(default = "default_clawbot_token_store")]
    pub token_store: String,
    #[serde(default = "default_clawbot_channel_version")]
    pub channel_version: String,
    #[serde(default = "default_clawbot_app_client_version")]
    pub app_client_version: String,
    /// Optional platform that receives iLink login QR notifications.
    #[serde(default)]
    pub login_notify_platform: String,
    /// Optional bot id for login QR notifications.
    #[serde(default)]
    pub login_notify_bot_id: String,
    /// Optional chat id for login QR notifications.
    #[serde(default)]
    pub login_notify_chat_id: String,
}

fn default_clawbot_mode() -> String {
    "http_bridge".to_string()
}

fn default_clawbot_bot_id() -> String {
    "clawbot".to_string()
}

fn default_clawbot_base_url() -> String {
    "http://127.0.0.1:5700".to_string()
}

fn default_clawbot_poll_path() -> String {
    "/messages".to_string()
}

fn default_clawbot_send_path() -> String {
    "/send_message".to_string()
}

fn default_clawbot_edit_path() -> String {
    "/edit_message".to_string()
}

fn default_clawbot_poll_interval_ms() -> u64 {
    1_000
}

fn default_clawbot_poll_limit() -> usize {
    50
}

fn default_clawbot_token_store() -> String {
    "~/.hakimi/clawbot".to_string()
}

fn default_clawbot_channel_version() -> String {
    "1.0.2".to_string()
}

fn default_clawbot_app_client_version() -> String {
    "2.4.3".to_string()
}

impl Default for ClawBotGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: default_clawbot_mode(),
            bot_id: default_clawbot_bot_id(),
            base_url: default_clawbot_base_url(),
            token: String::new(),
            poll_path: default_clawbot_poll_path(),
            send_path: default_clawbot_send_path(),
            edit_path: default_clawbot_edit_path(),
            poll_interval_ms: default_clawbot_poll_interval_ms(),
            poll_limit: default_clawbot_poll_limit(),
            token_store: default_clawbot_token_store(),
            channel_version: default_clawbot_channel_version(),
            app_client_version: default_clawbot_app_client_version(),
            login_notify_platform: String::new(),
            login_notify_bot_id: String::new(),
            login_notify_chat_id: String::new(),
        }
    }
}

/// Telegram-specific gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelegramGatewayConfig {
    /// Telegram Bot API token.
    #[serde(default)]
    pub bot_token: String,
    /// List of allowed user IDs (empty = allow all).
    #[serde(default)]
    pub allowed_users: Vec<i64>,
}

/// Per-role configuration — each role can bind to its own bot(s).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleConfig {
    /// Identity/system prompt for this role
    #[serde(default)]
    pub identity: String,
    /// Model override for this role (falls back to top-level model)
    #[serde(default)]
    pub model: String,
    /// API Key override for this role
    #[serde(default)]
    pub api_key: String,
    /// Base URL override for this role
    #[serde(default)]
    pub base_url: String,
    /// API mode override
    #[serde(default)]
    pub api_mode: String,
    /// Gateway bindings per platform
    #[serde(default)]
    pub gateways: RoleGatewaysConfig,
    /// Allowed Telegram user IDs
    #[serde(default)]
    pub allowed_users: Vec<i64>,
    /// Max conversation turns
    #[serde(default)]
    pub max_turns: usize,
    /// Enabled tool names (empty = all)
    #[serde(default)]
    pub tools: Vec<String>,
    /// Whether to enable streaming
    #[serde(default = "default_true")]
    pub streaming: bool,
}

/// Gateway bindings for a specific role.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleGatewaysConfig {
    #[serde(default)]
    pub telegram: Option<RoleTelegramConfig>,
    #[serde(default)]
    pub clawbot: Option<RoleClawBotConfig>,
}

/// ClawBot-specific config for a role gateway binding.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleClawBotConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_clawbot_mode")]
    pub mode: String,
    #[serde(default = "default_clawbot_bot_id")]
    pub bot_id: String,
    #[serde(default = "default_clawbot_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default = "default_clawbot_poll_path")]
    pub poll_path: String,
    #[serde(default = "default_clawbot_send_path")]
    pub send_path: String,
    #[serde(default = "default_clawbot_edit_path")]
    pub edit_path: String,
    #[serde(default = "default_clawbot_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default = "default_clawbot_poll_limit")]
    pub poll_limit: usize,
    #[serde(default = "default_clawbot_token_store")]
    pub token_store: String,
    #[serde(default = "default_clawbot_channel_version")]
    pub channel_version: String,
    #[serde(default = "default_clawbot_app_client_version")]
    pub app_client_version: String,
    #[serde(default)]
    pub login_notify_platform: String,
    #[serde(default)]
    pub login_notify_bot_id: String,
    #[serde(default)]
    pub login_notify_chat_id: String,
}

/// Telegram-specific config for a role gateway binding.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RoleTelegramConfig {
    #[serde(default)]
    pub bot_token: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = HakimiConfig::default();
        assert_eq!(config.model.provider, "auto");
        assert_eq!(config.agent.max_turns, 90);
        assert_eq!(config.terminal.env_type, "local");
        assert_eq!(config.terminal.cwd, ".");
        assert!(config.compression.enabled);
        assert_eq!(config.compression.threshold, 0.50);
        assert_eq!(config.compression.engine, "smart");
        assert_eq!(config.compression.model, "");
        assert_eq!(config.compression.context_length, 128_000);
        assert!(config.display.streaming);
        assert_eq!(config.display.skin, "default");
        assert_eq!(config.delegation.max_iterations, 45);
        assert!(config.mcp_servers.is_empty());
        assert!(config.credential_pools.is_empty());
        assert!(config.embedding.enabled);
        assert_eq!(config.embedding.provider, "openai-compatible");
        assert_eq!(config.embedding.model, "BAAI/bge-m3");
        assert_eq!(config.embedding.dimension, 1024);
        assert!(!config.gateways.clawbot.enabled);
        assert_eq!(config.gateways.clawbot.bot_id, "clawbot");
    }

    #[test]
    fn test_deserialize_empty_yaml() {
        let config: HakimiConfig = serde_yaml::from_str("").unwrap();
        assert_eq!(config.model.provider, "auto");
        assert_eq!(config.agent.max_turns, 90);
    }

    #[test]
    fn test_deserialize_partial_yaml() {
        let yaml = r#"
model:
  default: "gpt-4o"
  provider: "openai"

agent:
  max_turns: 50
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.model.default, "gpt-4o");
        assert_eq!(config.model.provider, "openai");
        assert_eq!(config.agent.max_turns, 50);
        // Defaults for unset fields
        assert_eq!(config.terminal.env_type, "local");
        assert_eq!(config.delegation.max_iterations, 45);
    }

    #[test]
    fn test_deserialize_with_mcp_servers() {
        let yaml = r#"
mcp_servers:
  filesystem:
    command: "npx"
    args: ["-y", "@modelcontextprotocol/server-filesystem", "/tmp"]
    env:
      NODE_ENV: "production"
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        let fs = config.mcp_servers.get("filesystem").unwrap();
        assert_eq!(fs.command, "npx");
        assert_eq!(fs.args.len(), 3);
        assert_eq!(fs.env.get("NODE_ENV").unwrap(), "production");
    }

    #[test]
    fn test_deserialize_with_clawbot_gateway() {
        let yaml = r#"
gateways:
  clawbot:
    enabled: true
    bot_id: "wechat-main"
    base_url: "http://127.0.0.1:7777"
    token: "[REDACTED]"
    poll_path: "/wx/poll"
    send_path: "/wx/send"
    edit_path: "/wx/edit"
    poll_interval_ms: 300
    poll_limit: 20
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.gateways.clawbot.enabled);
        assert_eq!(config.gateways.clawbot.bot_id, "wechat-main");
        assert_eq!(config.gateways.clawbot.base_url, "http://127.0.0.1:7777");
        assert_eq!(config.gateways.clawbot.token, "[REDACTED]");
        assert_eq!(config.gateways.clawbot.poll_path, "/wx/poll");
        assert_eq!(config.gateways.clawbot.send_path, "/wx/send");
        assert_eq!(config.gateways.clawbot.edit_path, "/wx/edit");
        assert_eq!(config.gateways.clawbot.poll_interval_ms, 300);
        assert_eq!(config.gateways.clawbot.poll_limit, 20);
    }

    #[test]
    fn test_serialize_roundtrip() {
        let config = HakimiConfig::default();
        let yaml = serde_yaml::to_string(&config).unwrap();
        let back: HakimiConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(back.model.provider, config.model.provider);
        assert_eq!(back.agent.max_turns, config.agent.max_turns);
        assert_eq!(back.terminal.cwd, config.terminal.cwd);
    }

    #[test]
    fn test_full_config_yaml() {
        let yaml = r#"
model:
  default: "claude-sonnet-4-20250514"
  provider: "anthropic"
  base_url: "https://api.anthropic.com"

agent:
  max_turns: 100
  verbose: true
  system_prompt: "You are a helpful assistant."
  reasoning_effort: "high"
  disabled_toolsets: ["code"]

terminal:
  env_type: "docker"
  cwd: "/workspace"
  timeout: 120
  docker_image: "python:3.11"

display:
  streaming: true
  compact: false
  skin: "dark"

delegation:
  max_iterations: 30
  model: "gpt-4o-mini"
  provider: "openai"

compression:
  engine: llm
  model: "claude-3-5-haiku-latest"
  context_length: 64000
  enabled: false
  threshold: 0.70
  target_ratio: 0.30

tools:
  tool_search:
    enabled: "on"
    threshold_pct: 15
    search_default_limit: 7
    max_search_limit: 30
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.model.default, "claude-sonnet-4-20250514");
        assert_eq!(config.model.provider, "anthropic");
        assert_eq!(config.agent.max_turns, 100);
        assert!(config.agent.verbose);
        assert_eq!(config.agent.system_prompt, "You are a helpful assistant.");
        assert_eq!(config.agent.reasoning_effort, "high");
        assert_eq!(config.agent.disabled_toolsets, vec!["code"]);
        assert_eq!(config.terminal.env_type, "docker");
        assert_eq!(config.terminal.timeout, 120);
        assert_eq!(config.terminal.docker_image, "python:3.11");
        assert_eq!(config.display.skin, "dark");
        assert_eq!(config.delegation.model, "gpt-4o-mini");
        assert!(!config.compression.enabled);
        assert_eq!(config.compression.engine, "llm");
        assert_eq!(config.compression.model, "claude-3-5-haiku-latest");
        assert_eq!(config.compression.context_length, 64_000);
        assert_eq!(
            config.tools.tool_search.enabled,
            hakimi_common::ToolSearchMode::On
        );
        assert_eq!(config.tools.tool_search.threshold_pct, 15.0);
        assert_eq!(config.tools.tool_search.search_default_limit, 7);
        assert_eq!(config.tools.tool_search.max_search_limit, 30);
    }

    #[test]
    fn test_tool_search_config_bool_and_clamp() {
        let disabled: HakimiConfig = serde_yaml::from_str(
            r#"
tools:
  tool_search: false
"#,
        )
        .unwrap();
        assert_eq!(
            disabled.tools.tool_search.enabled,
            hakimi_common::ToolSearchMode::Off
        );

        let clamped: HakimiConfig = serde_yaml::from_str(
            r#"
tools:
  tool_search:
    enabled: "maybe"
    threshold_pct: 150
    search_default_limit: 999
    max_search_limit: 999
"#,
        )
        .unwrap();
        assert_eq!(
            clamped.tools.tool_search.enabled,
            hakimi_common::ToolSearchMode::Auto
        );
        assert_eq!(clamped.tools.tool_search.threshold_pct, 100.0);
        assert_eq!(clamped.tools.tool_search.max_search_limit, 50);
        assert_eq!(clamped.tools.tool_search.search_default_limit, 50);
    }
}
