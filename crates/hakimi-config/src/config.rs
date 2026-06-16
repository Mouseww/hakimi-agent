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
    /// Source identity used by credential-pool persistence/sync semantics.
    pub source: Option<String>,
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

    /// Explicit context window override in tokens. Zero means auto-resolve.
    #[serde(default)]
    pub context_length: usize,

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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WebuiConfig {
    #[serde(default)]
    pub password: String,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            default: String::new(),
            context_length: 0,
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

    /// Save completed/failed conversations as Hermes-compatible JSONL trajectories.
    #[serde(default)]
    pub save_trajectories: bool,

    /// Directory for trajectory_samples.jsonl and failed_trajectories.jsonl.
    #[serde(default)]
    pub trajectory_dir: String,
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
            save_trajectories: false,
            trajectory_dir: String::new(),
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
    hakimi_common::DEFAULT_FALLBACK_CONTEXT_LENGTH
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

    /// Static message locale. Environment variables can override it at runtime.
    #[serde(default = "default_language")]
    pub language: String,

    /// UI skin name.
    #[serde(default = "default_skin")]
    pub skin: String,
}

fn default_language() -> String {
    "en".to_string()
}

fn default_skin() -> String {
    "default".to_string()
}

impl Default for DisplayConfig {
    fn default() -> Self {
        Self {
            compact: false,
            streaming: true,
            language: default_language(),
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

/// Contextual one-time onboarding hint state.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct OnboardingConfig {
    /// Stable hint flags already shown to the user.
    #[serde(default)]
    pub seen: HashMap<String, bool>,
}

impl OnboardingConfig {
    pub fn is_seen(&self, flag: &str) -> bool {
        self.seen.get(flag).copied().unwrap_or(false)
    }

    pub fn mark_seen(&mut self, flag: impl Into<String>) {
        self.seen.insert(flag.into(), true);
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

    /// TUI push-to-talk diagnostic record key (Hermes default: Ctrl+B).
    #[serde(default = "default_voice_record_key")]
    pub record_key: String,

    /// RMS level below which input is treated as silence.
    #[serde(default = "default_voice_silence_threshold")]
    pub silence_threshold: u32,

    /// Seconds of silence before recording stops.
    #[serde(default = "default_voice_silence_duration_seconds")]
    pub silence_duration_seconds: f32,

    /// Whether start/stop voice cues are enabled.
    #[serde(default = "default_voice_beep_enabled")]
    pub beep_enabled: bool,
}

fn default_voice_provider() -> String {
    "openai".to_string()
}

fn default_voice_record_key() -> String {
    "ctrl+b".to_string()
}

fn default_voice_silence_threshold() -> u32 {
    200
}

fn default_voice_silence_duration_seconds() -> f32 {
    3.0
}

fn default_voice_beep_enabled() -> bool {
    true
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
            record_key: default_voice_record_key(),
            silence_threshold: default_voice_silence_threshold(),
            silence_duration_seconds: default_voice_silence_duration_seconds(),
            beep_enabled: default_voice_beep_enabled(),
        }
    }
}

/// Tool behavior configuration.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolsConfig {
    /// Progressive disclosure for MCP/plugin tools.
    #[serde(default)]
    pub tool_search: hakimi_common::ToolSearchConfig,

    /// Framework-level tool result truncation.
    #[serde(default)]
    pub output: hakimi_common::ToolOutputConfig,
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

    /// One-time contextual onboarding hints.
    #[serde(default)]
    pub onboarding: OnboardingConfig,

    /// Embedding configuration.
    #[serde(default)]
    pub embedding: EmbeddingConfig,

    /// Voice / TTS configuration.
    #[serde(default)]
    pub voice: VoiceConfig,

    /// Tool behavior configuration.
    #[serde(default)]
    pub tools: ToolsConfig,

    /// WebUI configuration.
    #[serde(default)]
    pub webui: WebuiConfig,

    /// Named roles — each can bind to its own bot(s).
    #[serde(default)]
    pub roles: HashMap<String, RoleConfig>,
}

/// Configuration for all gateway platforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewaysConfig {
    /// Allow all inbound gateway users regardless of allowlists.
    #[serde(default)]
    pub allow_all: bool,
    /// Global inbound gateway allowlist. Entries may be user IDs, chat IDs,
    /// or qualified as `platform:id` / `platform:bot_id:id`.
    #[serde(default)]
    pub allowed_users: Vec<String>,
    /// Drop outbound messages that are only silence narration, such as
    /// `*(silent)*`, `.`, or `no reply`, before they reach chat adapters.
    #[serde(default = "default_gateway_filter_silence_narration")]
    pub filter_silence_narration: bool,
    /// Streaming delivery behavior for gateway chat platforms.
    #[serde(default)]
    pub streaming: GatewayStreamingConfig,
    #[serde(default)]
    pub telegram: TelegramGatewayConfig,
    #[serde(default)]
    pub clawbot: ClawBotGatewayConfig,
    #[serde(default)]
    pub weixin: WeixinGatewayConfig,
    #[serde(default)]
    pub bluebubbles: BlueBubblesGatewayConfig,
    #[serde(default)]
    pub qqbot: QQBotGatewayConfig,
    #[serde(default)]
    pub slack: SlackGatewayConfig,
    #[serde(default)]
    pub discord: DiscordGatewayConfig,
    #[serde(default)]
    pub mattermost: MattermostGatewayConfig,
    #[serde(default)]
    pub webhook: WebhookGatewayConfig,
    #[serde(default)]
    pub msgraph_webhook: MSGraphWebhookGatewayConfig,
    #[serde(default)]
    pub signal: SignalGatewayConfig,
    #[serde(default)]
    pub sms: SmsGatewayConfig,
    #[serde(default)]
    pub email: EmailGatewayConfig,
    #[serde(default)]
    pub whatsapp: WhatsAppGatewayConfig,
    #[serde(default)]
    pub homeassistant: HomeAssistantGatewayConfig,
    #[serde(default)]
    pub matrix: MatrixGatewayConfig,
    #[serde(default)]
    pub dingtalk: DingTalkGatewayConfig,
    #[serde(default)]
    pub wecom: WeComGatewayConfig,
    #[serde(default)]
    pub feishu: FeishuGatewayConfig,
}

fn default_gateway_filter_silence_narration() -> bool {
    true
}

impl Default for GatewaysConfig {
    fn default() -> Self {
        Self {
            allow_all: false,
            allowed_users: Vec::new(),
            filter_silence_narration: default_gateway_filter_silence_narration(),
            streaming: GatewayStreamingConfig::default(),
            telegram: TelegramGatewayConfig::default(),
            clawbot: ClawBotGatewayConfig::default(),
            weixin: WeixinGatewayConfig::default(),
            bluebubbles: BlueBubblesGatewayConfig::default(),
            qqbot: QQBotGatewayConfig::default(),
            slack: SlackGatewayConfig::default(),
            discord: DiscordGatewayConfig::default(),
            mattermost: MattermostGatewayConfig::default(),
            webhook: WebhookGatewayConfig::default(),
            msgraph_webhook: MSGraphWebhookGatewayConfig::default(),
            signal: SignalGatewayConfig::default(),
            sms: SmsGatewayConfig::default(),
            email: EmailGatewayConfig::default(),
            whatsapp: WhatsAppGatewayConfig::default(),
            homeassistant: HomeAssistantGatewayConfig::default(),
            matrix: MatrixGatewayConfig::default(),
            dingtalk: DingTalkGatewayConfig::default(),
            wecom: WeComGatewayConfig::default(),
            feishu: FeishuGatewayConfig::default(),
        }
    }
}

/// QQBot gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QQBotGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_qqbot_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub client_secret: String,
    #[serde(default)]
    pub home_channel: String,
    #[serde(default = "default_qqbot_chat_type")]
    pub default_chat_type: String,
    #[serde(default = "default_true")]
    pub markdown_support: bool,
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub token_url: String,
}

fn default_qqbot_bot_id() -> String {
    "qqbot".to_string()
}

fn default_qqbot_chat_type() -> String {
    "c2c".to_string()
}

impl Default for QQBotGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_qqbot_bot_id(),
            app_id: String::new(),
            client_secret: String::new(),
            home_channel: String::new(),
            default_chat_type: default_qqbot_chat_type(),
            markdown_support: true,
            base_url: String::new(),
            token_url: String::new(),
        }
    }
}

/// BlueBubbles / iMessage gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlueBubblesGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_bluebubbles_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub home_channel: String,
    #[serde(default)]
    pub allow_new_chat: bool,
}

fn default_bluebubbles_bot_id() -> String {
    "bluebubbles".to_string()
}

impl Default for BlueBubblesGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_bluebubbles_bot_id(),
            server_url: String::new(),
            password: String::new(),
            home_channel: String::new(),
            allow_new_chat: false,
        }
    }
}

/// Slack gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SlackGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_slack_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub channel_id: String,
    #[serde(default)]
    pub base_url: String,
}

fn default_slack_bot_id() -> String {
    "slack".to_string()
}

impl Default for SlackGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_slack_bot_id(),
            token: String::new(),
            channel_id: String::new(),
            base_url: String::new(),
        }
    }
}

/// Discord gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiscordGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_discord_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub channel_id: String,
    #[serde(default)]
    pub base_url: String,
}

fn default_discord_bot_id() -> String {
    "discord".to_string()
}

impl Default for DiscordGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_discord_bot_id(),
            token: String::new(),
            channel_id: String::new(),
            base_url: String::new(),
        }
    }
}

/// Mattermost gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MattermostGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_mattermost_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub server_url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default)]
    pub channel_id: String,
    #[serde(default)]
    pub base_url: String,
}

fn default_mattermost_bot_id() -> String {
    "mattermost".to_string()
}

impl Default for MattermostGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_mattermost_bot_id(),
            server_url: String::new(),
            token: String::new(),
            channel_id: String::new(),
            base_url: String::new(),
        }
    }
}

/// Generic webhook gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_webhook_bot_id")]
    pub bot_id: String,
    #[serde(default = "default_webhook_port")]
    pub port: u16,
    #[serde(default = "default_webhook_path")]
    pub path: String,
    #[serde(default)]
    pub secret: String,
}

fn default_webhook_bot_id() -> String {
    "webhook".to_string()
}

fn default_webhook_port() -> u16 {
    8080
}

fn default_webhook_path() -> String {
    "/webhook".to_string()
}

impl Default for WebhookGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_webhook_bot_id(),
            port: default_webhook_port(),
            path: default_webhook_path(),
            secret: String::new(),
        }
    }
}

/// Microsoft Graph webhook gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MSGraphWebhookGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_msgraph_webhook_bot_id")]
    pub bot_id: String,
    #[serde(default = "default_msgraph_webhook_host")]
    pub host: String,
    #[serde(default = "default_msgraph_webhook_port")]
    pub port: u16,
    #[serde(default = "default_msgraph_webhook_path")]
    pub webhook_path: String,
    #[serde(default = "default_msgraph_webhook_health_path")]
    pub health_path: String,
    #[serde(default)]
    pub client_state: String,
    #[serde(default)]
    pub accepted_resources: Vec<String>,
    #[serde(default)]
    pub allowed_source_cidrs: Vec<String>,
    #[serde(default = "default_msgraph_webhook_max_seen_receipts")]
    pub max_seen_receipts: usize,
    #[serde(default)]
    pub prompt: String,
}

fn default_msgraph_webhook_bot_id() -> String {
    "msgraph_webhook".to_string()
}

fn default_msgraph_webhook_host() -> String {
    "0.0.0.0".to_string()
}

fn default_msgraph_webhook_port() -> u16 {
    8646
}

fn default_msgraph_webhook_path() -> String {
    "/msgraph/webhook".to_string()
}

fn default_msgraph_webhook_health_path() -> String {
    "/health".to_string()
}

fn default_msgraph_webhook_max_seen_receipts() -> usize {
    5_000
}

impl Default for MSGraphWebhookGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_msgraph_webhook_bot_id(),
            host: default_msgraph_webhook_host(),
            port: default_msgraph_webhook_port(),
            webhook_path: default_msgraph_webhook_path(),
            health_path: default_msgraph_webhook_health_path(),
            client_state: String::new(),
            accepted_resources: Vec::new(),
            allowed_source_cidrs: Vec::new(),
            max_seen_receipts: default_msgraph_webhook_max_seen_receipts(),
            prompt: String::new(),
        }
    }
}

/// Signal gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignalGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_signal_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub phone_number: String,
    #[serde(default = "default_signal_cli_path")]
    pub signal_cli_path: String,
}

fn default_signal_bot_id() -> String {
    "signal".to_string()
}

fn default_signal_cli_path() -> String {
    "http://127.0.0.1:8080".to_string()
}

impl Default for SignalGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_signal_bot_id(),
            phone_number: String::new(),
            signal_cli_path: default_signal_cli_path(),
        }
    }
}

/// SMS / Twilio gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SmsGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_sms_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub account_sid: String,
    #[serde(default)]
    pub auth_token: String,
    #[serde(default)]
    pub from_number: String,
    #[serde(default)]
    pub home_channel: String,
    #[serde(default)]
    pub base_url: String,
}

fn default_sms_bot_id() -> String {
    "sms".to_string()
}

impl Default for SmsGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_sms_bot_id(),
            account_sid: String::new(),
            auth_token: String::new(),
            from_number: String::new(),
            home_channel: String::new(),
            base_url: String::new(),
        }
    }
}

/// Email / SMTP gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmailGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_email_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub smtp_host: String,
    #[serde(default = "default_email_smtp_port")]
    pub smtp_port: u16,
    #[serde(default)]
    pub address: String,
    #[serde(default)]
    pub password: String,
    #[serde(default)]
    pub username: String,
    #[serde(default)]
    pub home_channel: String,
    #[serde(default = "default_email_subject")]
    pub subject: String,
}

fn default_email_bot_id() -> String {
    "email".to_string()
}

fn default_email_smtp_port() -> u16 {
    587
}

fn default_email_subject() -> String {
    "Hakimi Agent".to_string()
}

impl Default for EmailGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_email_bot_id(),
            smtp_host: String::new(),
            smtp_port: default_email_smtp_port(),
            address: String::new(),
            password: String::new(),
            username: String::new(),
            home_channel: String::new(),
            subject: default_email_subject(),
        }
    }
}

/// WhatsApp Business Cloud API gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WhatsAppGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_whatsapp_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub phone_number_id: String,
    #[serde(default)]
    pub home_channel: String,
    #[serde(default = "default_whatsapp_api_version")]
    pub api_version: String,
    #[serde(default)]
    pub base_url: String,
}

fn default_whatsapp_bot_id() -> String {
    "whatsapp".to_string()
}

fn default_whatsapp_api_version() -> String {
    "v20.0".to_string()
}

impl Default for WhatsAppGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_whatsapp_bot_id(),
            access_token: String::new(),
            phone_number_id: String::new(),
            home_channel: String::new(),
            api_version: default_whatsapp_api_version(),
            base_url: String::new(),
        }
    }
}

/// Home Assistant gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HomeAssistantGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_homeassistant_bot_id")]
    pub bot_id: String,
    #[serde(default = "default_homeassistant_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default = "default_homeassistant_default_title")]
    pub default_title: String,
}

fn default_homeassistant_bot_id() -> String {
    "homeassistant".to_string()
}

fn default_homeassistant_base_url() -> String {
    "http://homeassistant.local:8123".to_string()
}

fn default_homeassistant_default_title() -> String {
    "Hakimi".to_string()
}

impl Default for HomeAssistantGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_homeassistant_bot_id(),
            base_url: default_homeassistant_base_url(),
            token: String::new(),
            default_title: default_homeassistant_default_title(),
        }
    }
}

/// Matrix gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MatrixGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_matrix_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub homeserver_url: String,
    #[serde(default)]
    pub access_token: String,
    #[serde(default)]
    pub room_id: String,
}

fn default_matrix_bot_id() -> String {
    "matrix".to_string()
}

impl Default for MatrixGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_matrix_bot_id(),
            homeserver_url: String::new(),
            access_token: String::new(),
            room_id: String::new(),
        }
    }
}

/// DingTalk gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DingTalkGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_dingtalk_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub webhook_url: String,
    #[serde(default)]
    pub secret: String,
}

fn default_dingtalk_bot_id() -> String {
    "dingtalk".to_string()
}

impl Default for DingTalkGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_dingtalk_bot_id(),
            webhook_url: String::new(),
            secret: String::new(),
        }
    }
}

/// WeCom gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeComGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_wecom_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub corp_id: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub secret: String,
}

fn default_wecom_bot_id() -> String {
    "wecom".to_string()
}

impl Default for WeComGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_wecom_bot_id(),
            corp_id: String::new(),
            agent_id: String::new(),
            secret: String::new(),
        }
    }
}

/// Feishu / Lark gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeishuGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_feishu_bot_id")]
    pub bot_id: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub app_secret: String,
    #[serde(default)]
    pub default_chat_id: String,
    #[serde(default = "default_feishu_receive_id_type")]
    pub receive_id_type: String,
    #[serde(default = "default_feishu_domain")]
    pub domain: String,
    #[serde(default)]
    pub base_url: String,
}

fn default_feishu_bot_id() -> String {
    "feishu".to_string()
}

fn default_feishu_receive_id_type() -> String {
    "chat_id".to_string()
}

fn default_feishu_domain() -> String {
    "feishu".to_string()
}

impl Default for FeishuGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_feishu_bot_id(),
            app_id: String::new(),
            app_secret: String::new(),
            default_chat_id: String::new(),
            receive_id_type: default_feishu_receive_id_type(),
            domain: default_feishu_domain(),
            base_url: String::new(),
        }
    }
}

/// Runtime streaming behavior for gateway chat platforms.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayStreamingConfig {
    /// Streaming preview transport. `edit` preserves the legacy progressive
    /// message-edit path; `auto` prefers native drafts when an adapter supports
    /// them; `draft` requests drafts and falls back to edit when unsupported.
    #[serde(default = "default_gateway_streaming_transport")]
    pub transport: GatewayStreamingTransport,
    /// Minimum interval between progressive gateway message edits.
    #[serde(default = "default_gateway_streaming_edit_interval_ms")]
    pub edit_interval_ms: u64,
    /// Maximum adaptive interval after repeated flood-control edit failures.
    #[serde(default = "default_gateway_streaming_edit_backoff_max_ms")]
    pub edit_backoff_max_ms: u64,
    /// Consecutive flood-control edit failures before previews are disabled
    /// for the current streamed response.
    #[serde(default = "default_gateway_streaming_max_flood_strikes")]
    pub max_flood_strikes: u32,
    /// Flush a progressive edit once this many new visible characters are buffered.
    /// `0` disables the character threshold and relies on the edit interval.
    #[serde(default = "default_gateway_streaming_buffer_threshold_chars")]
    pub buffer_threshold_chars: usize,
    /// Send a fresh final message after a preview has been visible for this
    /// many seconds. `0` disables the fresh-final path.
    #[serde(default = "default_gateway_fresh_final_after_seconds")]
    pub fresh_final_after_seconds: u64,
    /// Per-platform overrides for gateway streaming previews.
    #[serde(default)]
    pub platforms: HashMap<String, GatewayStreamingPlatformConfig>,
}

/// Per-platform gateway streaming preview policy.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GatewayStreamingPlatformConfig {
    /// Enable progressive token previews for this platform. `None` inherits the
    /// global gateway streaming behavior.
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Platform-specific streaming preview transport.
    #[serde(default)]
    pub transport: Option<GatewayStreamingTransport>,
    /// Platform-specific edit cadence override in milliseconds.
    #[serde(default)]
    pub edit_interval_ms: Option<u64>,
    /// Platform-specific adaptive edit backoff ceiling in milliseconds.
    #[serde(default)]
    pub edit_backoff_max_ms: Option<u64>,
    /// Platform-specific consecutive flood-control failure threshold.
    #[serde(default)]
    pub max_flood_strikes: Option<u32>,
    /// Platform-specific visible-character flush threshold.
    #[serde(default)]
    pub buffer_threshold_chars: Option<usize>,
    /// Platform-specific fresh-final threshold in seconds.
    #[serde(default)]
    pub fresh_final_after_seconds: Option<u64>,
}

/// Gateway streaming preview transport selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GatewayStreamingTransport {
    /// Use native draft previews when available, otherwise use message edits.
    Auto,
    /// Request native draft previews and fall back to edits when unsupported.
    Draft,
    /// Use the legacy send+edit preview path.
    Edit,
    /// Disable content previews while still delivering final responses.
    Off,
}

fn default_gateway_streaming_transport() -> GatewayStreamingTransport {
    GatewayStreamingTransport::Edit
}

fn default_gateway_streaming_edit_interval_ms() -> u64 {
    800
}

fn default_gateway_streaming_edit_backoff_max_ms() -> u64 {
    10_000
}

fn default_gateway_streaming_max_flood_strikes() -> u32 {
    3
}

fn default_gateway_streaming_buffer_threshold_chars() -> usize {
    24
}

fn default_gateway_fresh_final_after_seconds() -> u64 {
    60
}

impl Default for GatewayStreamingConfig {
    fn default() -> Self {
        Self {
            transport: default_gateway_streaming_transport(),
            edit_interval_ms: default_gateway_streaming_edit_interval_ms(),
            edit_backoff_max_ms: default_gateway_streaming_edit_backoff_max_ms(),
            max_flood_strikes: default_gateway_streaming_max_flood_strikes(),
            buffer_threshold_chars: default_gateway_streaming_buffer_threshold_chars(),
            fresh_final_after_seconds: default_gateway_fresh_final_after_seconds(),
            platforms: HashMap::new(),
        }
    }
}

/// Weixin/iLink gateway configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeixinGatewayConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_weixin_bot_id")]
    pub bot_id: String,
    #[serde(default = "default_weixin_base_url")]
    pub base_url: String,
    #[serde(default)]
    pub token: String,
    #[serde(default = "default_weixin_token_store")]
    pub token_store: String,
    #[serde(default = "default_clawbot_channel_version")]
    pub channel_version: String,
    #[serde(default = "default_clawbot_app_client_version")]
    pub app_client_version: String,
    #[serde(default = "default_clawbot_poll_interval_ms")]
    pub poll_interval_ms: u64,
    #[serde(default)]
    pub home_channel: String,
    /// Optional platform that receives iLink login QR notifications.
    #[serde(default)]
    pub login_notify_platform: String,
    /// Optional bot id for login QR notifications.
    #[serde(default)]
    pub login_notify_bot_id: String,
    /// Optional chat id for login QR notifications.
    #[serde(default)]
    pub login_notify_chat_id: String,
    /// List of allowed sender IDs (empty = allow all unless a global gateway
    /// allowlist is configured).
    #[serde(default)]
    pub allowed_users: Vec<String>,
}

fn default_weixin_bot_id() -> String {
    "weixin".to_string()
}

fn default_weixin_base_url() -> String {
    "https://ilinkai.weixin.qq.com".to_string()
}

fn default_weixin_token_store() -> String {
    "~/.hakimi/weixin".to_string()
}

impl Default for WeixinGatewayConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bot_id: default_weixin_bot_id(),
            base_url: default_weixin_base_url(),
            token: String::new(),
            token_store: default_weixin_token_store(),
            channel_version: default_clawbot_channel_version(),
            app_client_version: default_clawbot_app_client_version(),
            poll_interval_ms: default_clawbot_poll_interval_ms(),
            home_channel: String::new(),
            login_notify_platform: String::new(),
            login_notify_bot_id: String::new(),
            login_notify_chat_id: String::new(),
            allowed_users: Vec::new(),
        }
    }
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
    /// List of allowed sender IDs (empty = allow all unless a global gateway
    /// allowlist is configured).
    #[serde(default)]
    pub allowed_users: Vec<String>,
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
            allowed_users: Vec::new(),
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
    #[serde(default)]
    pub allowed_users: Vec<String>,
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
        assert_eq!(config.model.context_length, 0);
        assert_eq!(config.agent.max_turns, 90);
        assert!(!config.agent.save_trajectories);
        assert_eq!(config.agent.trajectory_dir, "");
        assert_eq!(config.terminal.env_type, "local");
        assert_eq!(config.terminal.cwd, ".");
        assert!(config.compression.enabled);
        assert_eq!(config.compression.threshold, 0.50);
        assert_eq!(config.compression.engine, "smart");
        assert_eq!(config.compression.model, "");
        assert_eq!(
            config.compression.context_length,
            hakimi_common::DEFAULT_FALLBACK_CONTEXT_LENGTH
        );
        assert!(config.display.streaming);
        assert_eq!(config.display.language, "en");
        assert_eq!(config.display.skin, "default");
        assert_eq!(config.delegation.max_iterations, 45);
        assert!(config.mcp_servers.is_empty());
        assert!(config.credential_pools.is_empty());
        assert!(config.onboarding.seen.is_empty());
        assert!(config.embedding.enabled);
        assert_eq!(config.embedding.provider, "openai-compatible");
        assert_eq!(config.embedding.model, "BAAI/bge-m3");
        assert_eq!(config.embedding.dimension, 1024);
        assert_eq!(config.gateways.streaming.edit_interval_ms, 800);
        assert_eq!(config.gateways.streaming.buffer_threshold_chars, 24);
        assert_eq!(config.gateways.streaming.fresh_final_after_seconds, 60);
        assert!(config.gateways.filter_silence_narration);
        assert!(!config.gateways.clawbot.enabled);
        assert_eq!(config.gateways.clawbot.bot_id, "clawbot");
        assert!(!config.gateways.weixin.enabled);
        assert_eq!(config.gateways.weixin.bot_id, "weixin");
        assert_eq!(
            config.gateways.weixin.base_url,
            "https://ilinkai.weixin.qq.com"
        );
        assert_eq!(config.gateways.weixin.token_store, "~/.hakimi/weixin");
        assert!(!config.gateways.bluebubbles.enabled);
        assert_eq!(config.gateways.bluebubbles.bot_id, "bluebubbles");
        assert!(!config.gateways.bluebubbles.allow_new_chat);
        assert_eq!(config.voice.record_key, "ctrl+b");
        assert_eq!(config.voice.silence_threshold, 200);
        assert_eq!(config.voice.silence_duration_seconds, 3.0);
        assert!(config.voice.beep_enabled);
        assert!(!config.gateways.slack.enabled);
        assert_eq!(config.gateways.slack.bot_id, "slack");
        assert!(!config.gateways.discord.enabled);
        assert_eq!(config.gateways.discord.bot_id, "discord");
        assert!(!config.gateways.webhook.enabled);
        assert_eq!(config.gateways.webhook.path, "/webhook");
        assert!(!config.gateways.msgraph_webhook.enabled);
        assert_eq!(config.gateways.msgraph_webhook.bot_id, "msgraph_webhook");
        assert_eq!(config.gateways.msgraph_webhook.host, "0.0.0.0");
        assert_eq!(config.gateways.msgraph_webhook.port, 8646);
        assert_eq!(
            config.gateways.msgraph_webhook.webhook_path,
            "/msgraph/webhook"
        );
        assert!(!config.gateways.signal.enabled);
        assert_eq!(
            config.gateways.signal.signal_cli_path,
            "http://127.0.0.1:8080"
        );
        assert!(!config.gateways.sms.enabled);
        assert_eq!(config.gateways.sms.bot_id, "sms");
        assert!(!config.gateways.whatsapp.enabled);
        assert_eq!(config.gateways.whatsapp.bot_id, "whatsapp");
        assert_eq!(config.gateways.whatsapp.api_version, "v20.0");
        assert!(!config.gateways.homeassistant.enabled);
        assert_eq!(config.gateways.homeassistant.bot_id, "homeassistant");
        assert_eq!(
            config.gateways.homeassistant.base_url,
            "http://homeassistant.local:8123"
        );
        assert_eq!(config.gateways.homeassistant.default_title, "Hakimi");
        assert!(!config.gateways.matrix.enabled);
        assert!(!config.gateways.dingtalk.enabled);
        assert!(!config.gateways.wecom.enabled);
        assert!(!config.gateways.feishu.enabled);
        assert_eq!(config.gateways.feishu.domain, "feishu");
        assert_eq!(config.gateways.feishu.receive_id_type, "chat_id");
        assert_eq!(
            config.tools.output.max_bytes,
            hakimi_common::DEFAULT_TOOL_OUTPUT_MAX_BYTES
        );
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
  context_length: 400000
  provider: "openai"

agent:
  max_turns: 50
  save_trajectories: true
  trajectory_dir: "./trajectories"
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.model.default, "gpt-4o");
        assert_eq!(config.model.context_length, 400_000);
        assert_eq!(config.model.provider, "openai");
        assert_eq!(config.agent.max_turns, 50);
        assert!(config.agent.save_trajectories);
        assert_eq!(config.agent.trajectory_dir, "./trajectories");
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
    fn test_onboarding_seen_roundtrip() {
        let yaml = r#"
onboarding:
  seen:
    busy_input_prompt: true
    openclaw_residue_cleanup: false
"#;
        let mut config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.onboarding.is_seen("busy_input_prompt"));
        assert!(!config.onboarding.is_seen("openclaw_residue_cleanup"));
        assert!(!config.onboarding.is_seen("missing"));

        config.onboarding.mark_seen("openclaw_residue_cleanup");
        assert!(config.onboarding.is_seen("openclaw_residue_cleanup"));
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
    fn test_deserialize_with_weixin_gateway() {
        let yaml = r#"
gateways:
  weixin:
    enabled: true
    bot_id: "wx-main"
    base_url: "https://ilink.test"
    token: "wx-redacted"
    token_store: "~/.hakimi/weixin-test"
    channel_version: "2.2.0"
    app_client_version: "2.2.0"
    poll_interval_ms: 750
    home_channel: "wxid_home"
    allowed_users: ["wxid_abc"]
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.gateways.weixin.enabled);
        assert_eq!(config.gateways.weixin.bot_id, "wx-main");
        assert_eq!(config.gateways.weixin.base_url, "https://ilink.test");
        assert_eq!(config.gateways.weixin.token, "wx-redacted");
        assert_eq!(config.gateways.weixin.token_store, "~/.hakimi/weixin-test");
        assert_eq!(config.gateways.weixin.channel_version, "2.2.0");
        assert_eq!(config.gateways.weixin.app_client_version, "2.2.0");
        assert_eq!(config.gateways.weixin.poll_interval_ms, 750);
        assert_eq!(config.gateways.weixin.home_channel, "wxid_home");
        assert_eq!(
            config.gateways.weixin.allowed_users,
            vec!["wxid_abc".to_string()]
        );
    }

    #[test]
    fn test_deserialize_with_additional_gateway_platforms() {
        let yaml = r#"
gateways:
  slack:
    enabled: true
    bot_id: "ops-slack"
    token: "xoxb-redacted"
    channel_id: "C123"
  discord:
    enabled: true
    token: "discord-redacted"
    channel_id: "987"
  mattermost:
    enabled: true
    bot_id: "ops-mm"
    server_url: "https://mattermost.example.com"
    token: "mm-redacted"
    channel_id: "mm-channel"
  webhook:
    enabled: true
    port: 9090
    path: "/events"
    secret: "whsec-redacted"
  msgraph_webhook:
    enabled: true
    bot_id: "ops-msgraph"
    host: "127.0.0.1"
    port: 8647
    webhook_path: "/graph/notify"
    health_path: "/graph/health"
    client_state: "graph-redacted"
    accepted_resources: ["users/123/messages", "me/events/*"]
    allowed_source_cidrs: ["127.0.0.1/32"]
    max_seen_receipts: 42
    prompt: "Graph {change_type} {resource}"
  signal:
    enabled: true
    phone_number: "+15551234567"
    signal_cli_path: "http://signal-cli:8080"
  bluebubbles:
    enabled: true
    bot_id: "ops-imessage"
    server_url: "http://127.0.0.1:1234"
    password: "bb-redacted"
    home_channel: "iMessage;-;user@example.com"
    allow_new_chat: true
  qqbot:
    enabled: true
    bot_id: "ops-qq"
    app_id: "qq-app"
    client_secret: "qq-secret"
    home_channel: "group:qq-home"
    default_chat_type: "group"
    markdown_support: false
    base_url: "https://api.qq.test"
    token_url: "https://token.qq.test"
  sms:
    enabled: true
    bot_id: "ops-sms"
    account_sid: "ACredacted"
    auth_token: "twilio-redacted"
    from_number: "+15550001111"
    home_channel: "+15552223333"
    base_url: "https://api.twilio.test/2010-04-01/Accounts"
  whatsapp:
    enabled: true
    bot_id: "ops-whatsapp"
    access_token: "wa-redacted"
    phone_number_id: "1234567890"
    home_channel: "15552223333"
    api_version: "v19.0"
    base_url: "https://graph.test"
  homeassistant:
    enabled: true
    bot_id: "ops-ha"
    base_url: "http://ha.example.local:8123"
    token: "ha-redacted"
    default_title: "Hakimi Ops"
  matrix:
    enabled: true
    homeserver_url: "https://matrix.example.com"
    access_token: "syt_redacted"
    room_id: "!room:example.com"
  dingtalk:
    enabled: true
    webhook_url: "https://oapi.dingtalk.com/robot/send?access_token=redacted"
    secret: "SECredacted"
  wecom:
    enabled: true
    corp_id: "ww123"
    agent_id: "1000002"
    secret: "wecom-redacted"
  feishu:
    enabled: true
    bot_id: "ops-feishu"
    app_id: "cli_test"
    app_secret: "feishu-redacted"
    default_chat_id: "oc_chat"
    receive_id_type: "chat_id"
    domain: "lark"
    base_url: "https://open.larksuite.com"
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();

        assert!(config.gateways.slack.enabled);
        assert_eq!(config.gateways.slack.bot_id, "ops-slack");
        assert_eq!(config.gateways.slack.channel_id, "C123");
        assert!(config.gateways.discord.enabled);
        assert_eq!(config.gateways.discord.bot_id, "discord");
        assert_eq!(config.gateways.discord.channel_id, "987");
        assert!(config.gateways.mattermost.enabled);
        assert_eq!(config.gateways.mattermost.bot_id, "ops-mm");
        assert_eq!(
            config.gateways.mattermost.server_url,
            "https://mattermost.example.com"
        );
        assert_eq!(config.gateways.mattermost.channel_id, "mm-channel");
        assert!(config.gateways.webhook.enabled);
        assert_eq!(config.gateways.webhook.port, 9090);
        assert_eq!(config.gateways.webhook.path, "/events");
        assert!(config.gateways.msgraph_webhook.enabled);
        assert_eq!(config.gateways.msgraph_webhook.bot_id, "ops-msgraph");
        assert_eq!(config.gateways.msgraph_webhook.host, "127.0.0.1");
        assert_eq!(config.gateways.msgraph_webhook.port, 8647);
        assert_eq!(
            config.gateways.msgraph_webhook.webhook_path,
            "/graph/notify"
        );
        assert_eq!(
            config.gateways.msgraph_webhook.allowed_source_cidrs,
            vec!["127.0.0.1/32"]
        );
        assert_eq!(config.gateways.msgraph_webhook.max_seen_receipts, 42);
        assert_eq!(
            config.gateways.msgraph_webhook.prompt,
            "Graph {change_type} {resource}"
        );
        assert!(config.gateways.signal.enabled);
        assert_eq!(config.gateways.signal.phone_number, "+15551234567");
        assert!(config.gateways.bluebubbles.enabled);
        assert_eq!(config.gateways.bluebubbles.bot_id, "ops-imessage");
        assert_eq!(
            config.gateways.bluebubbles.server_url,
            "http://127.0.0.1:1234"
        );
        assert_eq!(
            config.gateways.bluebubbles.home_channel,
            "iMessage;-;user@example.com"
        );
        assert!(config.gateways.bluebubbles.allow_new_chat);
        assert!(config.gateways.qqbot.enabled);
        assert_eq!(config.gateways.qqbot.bot_id, "ops-qq");
        assert_eq!(config.gateways.qqbot.app_id, "qq-app");
        assert_eq!(config.gateways.qqbot.home_channel, "group:qq-home");
        assert_eq!(config.gateways.qqbot.default_chat_type, "group");
        assert!(!config.gateways.qqbot.markdown_support);
        assert_eq!(config.gateways.qqbot.base_url, "https://api.qq.test");
        assert!(config.gateways.sms.enabled);
        assert_eq!(config.gateways.sms.bot_id, "ops-sms");
        assert_eq!(config.gateways.sms.account_sid, "ACredacted");
        assert_eq!(config.gateways.sms.from_number, "+15550001111");
        assert_eq!(config.gateways.sms.home_channel, "+15552223333");
        assert_eq!(
            config.gateways.sms.base_url,
            "https://api.twilio.test/2010-04-01/Accounts"
        );
        assert!(config.gateways.whatsapp.enabled);
        assert_eq!(config.gateways.whatsapp.bot_id, "ops-whatsapp");
        assert_eq!(config.gateways.whatsapp.phone_number_id, "1234567890");
        assert_eq!(config.gateways.whatsapp.home_channel, "15552223333");
        assert_eq!(config.gateways.whatsapp.api_version, "v19.0");
        assert_eq!(config.gateways.whatsapp.base_url, "https://graph.test");
        assert!(config.gateways.homeassistant.enabled);
        assert_eq!(config.gateways.homeassistant.bot_id, "ops-ha");
        assert_eq!(
            config.gateways.homeassistant.base_url,
            "http://ha.example.local:8123"
        );
        assert_eq!(config.gateways.homeassistant.default_title, "Hakimi Ops");
        assert!(config.gateways.matrix.enabled);
        assert_eq!(config.gateways.matrix.room_id, "!room:example.com");
        assert!(config.gateways.dingtalk.enabled);
        assert_eq!(config.gateways.dingtalk.bot_id, "dingtalk");
        assert!(config.gateways.wecom.enabled);
        assert_eq!(config.gateways.wecom.agent_id, "1000002");
        assert!(config.gateways.feishu.enabled);
        assert_eq!(config.gateways.feishu.bot_id, "ops-feishu");
        assert_eq!(config.gateways.feishu.app_id, "cli_test");
        assert_eq!(config.gateways.feishu.default_chat_id, "oc_chat");
        assert_eq!(config.gateways.feishu.domain, "lark");
        assert_eq!(
            config.gateways.feishu.base_url,
            "https://open.larksuite.com"
        );
    }

    #[test]
    fn test_gateway_streaming_empty_config_uses_defaults() {
        let yaml = r#"
gateways:
  streaming: {}
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.gateways.streaming.transport,
            GatewayStreamingTransport::Edit
        );
        assert_eq!(config.gateways.streaming.edit_interval_ms, 800);
        assert_eq!(config.gateways.streaming.edit_backoff_max_ms, 10_000);
        assert_eq!(config.gateways.streaming.max_flood_strikes, 3);
        assert_eq!(config.gateways.streaming.buffer_threshold_chars, 24);
        assert_eq!(config.gateways.streaming.fresh_final_after_seconds, 60);
        assert!(config.gateways.streaming.platforms.is_empty());
    }

    #[test]
    fn test_voice_config_accepts_interactive_capture_settings() {
        let yaml = r#"
voice:
  provider: edge
  model: tts-1
  voice: en-US-AriaNeural
  transcription_model: whisper-1
  record_key: ctrl+o
  silence_threshold: 120
  silence_duration_seconds: 1.5
  beep_enabled: false
  auto_play: true
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.voice.provider, "edge");
        assert_eq!(config.voice.model, "tts-1");
        assert_eq!(config.voice.voice, "en-US-AriaNeural");
        assert_eq!(config.voice.transcription_model, "whisper-1");
        assert_eq!(config.voice.record_key, "ctrl+o");
        assert_eq!(config.voice.silence_threshold, 120);
        assert_eq!(config.voice.silence_duration_seconds, 1.5);
        assert!(!config.voice.beep_enabled);
        assert!(config.voice.auto_play);
    }

    #[test]
    fn test_gateway_streaming_can_disable_buffer_threshold() {
        let yaml = r#"
gateways:
  streaming:
    edit_interval_ms: 1500
    buffer_threshold_chars: 0
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.gateways.streaming.edit_interval_ms, 1500);
        assert_eq!(config.gateways.streaming.buffer_threshold_chars, 0);
        assert_eq!(config.gateways.streaming.fresh_final_after_seconds, 60);
    }

    #[test]
    fn test_gateway_streaming_accepts_backoff_settings() {
        let yaml = r#"
gateways:
  streaming:
    transport: auto
    edit_interval_ms: 700
    edit_backoff_max_ms: 5000
    max_flood_strikes: 2
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.gateways.streaming.transport,
            GatewayStreamingTransport::Auto
        );
        assert_eq!(config.gateways.streaming.edit_interval_ms, 700);
        assert_eq!(config.gateways.streaming.edit_backoff_max_ms, 5000);
        assert_eq!(config.gateways.streaming.max_flood_strikes, 2);
    }

    #[test]
    fn test_gateway_streaming_platform_overrides() {
        let yaml = r#"
gateways:
  streaming:
    transport: auto
    edit_interval_ms: 900
    edit_backoff_max_ms: 8000
    max_flood_strikes: 4
    buffer_threshold_chars: 24
    fresh_final_after_seconds: 60
    platforms:
      telegram:
        transport: draft
        edit_interval_ms: 1100
        edit_backoff_max_ms: 9000
        max_flood_strikes: 5
        buffer_threshold_chars: 48
      whatsapp:
        enabled: false
        transport: off
      slack:
        transport: edit
        fresh_final_after_seconds: 0
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(
            config.gateways.streaming.transport,
            GatewayStreamingTransport::Auto
        );
        let telegram = config.gateways.streaming.platforms.get("telegram").unwrap();
        assert_eq!(telegram.enabled, None);
        assert_eq!(telegram.transport, Some(GatewayStreamingTransport::Draft));
        assert_eq!(telegram.edit_interval_ms, Some(1100));
        assert_eq!(telegram.edit_backoff_max_ms, Some(9000));
        assert_eq!(telegram.max_flood_strikes, Some(5));
        assert_eq!(telegram.buffer_threshold_chars, Some(48));
        assert_eq!(telegram.fresh_final_after_seconds, None);

        let whatsapp = config.gateways.streaming.platforms.get("whatsapp").unwrap();
        assert_eq!(whatsapp.enabled, Some(false));
        assert_eq!(whatsapp.transport, Some(GatewayStreamingTransport::Off));
        assert_eq!(whatsapp.edit_interval_ms, None);

        let slack = config.gateways.streaming.platforms.get("slack").unwrap();
        assert_eq!(slack.transport, Some(GatewayStreamingTransport::Edit));
        assert_eq!(slack.fresh_final_after_seconds, Some(0));
    }

    #[test]
    fn test_gateway_silence_filter_flag_can_disable_filter() {
        let yaml = r#"
gateways:
  filter_silence_narration: false
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.gateways.filter_silence_narration);
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
  context_length: 200000
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
  language: "zh-CN"
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
  output:
    max_bytes: 123456
  tool_search:
    enabled: "on"
    threshold_pct: 15
    search_default_limit: 7
    max_search_limit: 30

gateways:
  filter_silence_narration: false
  streaming:
    edit_interval_ms: 1200
    buffer_threshold_chars: 40
    fresh_final_after_seconds: 45
    platforms:
      telegram:
        edit_interval_ms: 1000
      sms:
        enabled: false
"#;
        let config: HakimiConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.model.default, "claude-sonnet-4-20250514");
        assert_eq!(config.model.context_length, 200_000);
        assert_eq!(config.model.provider, "anthropic");
        assert_eq!(config.agent.max_turns, 100);
        assert!(config.agent.verbose);
        assert_eq!(config.agent.system_prompt, "You are a helpful assistant.");
        assert_eq!(config.agent.reasoning_effort, "high");
        assert_eq!(config.agent.disabled_toolsets, vec!["code"]);
        assert_eq!(config.terminal.env_type, "docker");
        assert_eq!(config.terminal.timeout, 120);
        assert_eq!(config.terminal.docker_image, "python:3.11");
        assert_eq!(config.display.language, "zh-CN");
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
        assert_eq!(config.tools.output.max_bytes, 123_456);
        assert!(!config.gateways.filter_silence_narration);
        assert_eq!(config.gateways.streaming.edit_interval_ms, 1200);
        assert_eq!(config.gateways.streaming.edit_backoff_max_ms, 10_000);
        assert_eq!(config.gateways.streaming.max_flood_strikes, 3);
        assert_eq!(config.gateways.streaming.buffer_threshold_chars, 40);
        assert_eq!(config.gateways.streaming.fresh_final_after_seconds, 45);
        assert_eq!(
            config
                .gateways
                .streaming
                .platforms
                .get("telegram")
                .unwrap()
                .edit_interval_ms,
            Some(1000)
        );
        assert_eq!(
            config
                .gateways
                .streaming
                .platforms
                .get("sms")
                .unwrap()
                .enabled,
            Some(false)
        );
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

    #[test]
    fn test_tool_output_config_defaults_and_clamps() {
        let defaulted: HakimiConfig = serde_yaml::from_str(
            r#"
tools:
  output: {}
"#,
        )
        .unwrap();
        assert_eq!(
            defaulted.tools.output.max_bytes,
            hakimi_common::DEFAULT_TOOL_OUTPUT_MAX_BYTES
        );

        let clamped: HakimiConfig = serde_yaml::from_str(
            r#"
tools:
  output:
    max_bytes: 0
"#,
        )
        .unwrap();
        assert_eq!(clamped.tools.output.max_bytes, 1);
    }
}
