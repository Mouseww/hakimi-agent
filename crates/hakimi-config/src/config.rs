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

    /// Compression engine type: "smart" (3-tier) or "simple" (truncation).
    #[serde(default = "default_compression_engine")]
    pub engine: String,

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

/// Top-level Hakimi configuration.
///
/// All fields have sensible defaults via `serde(default)` so partial config
/// files work seamlessly.
#[derive(Debug, Clone, Serialize, Deserialize)]
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
}

impl Default for HakimiConfig {
    fn default() -> Self {
        Self {
            model: ModelConfig::default(),
            terminal: TerminalConfig::default(),
            agent: AgentConfig::default(),
            compression: CompressionConfig::default(),
            display: DisplayConfig::default(),
            delegation: DelegationConfig::default(),
            mcp_servers: HashMap::new(),
            credential_pools: HashMap::new(),
        }
    }
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
        assert_eq!(config.compression.context_length, 128_000);
        assert!(config.display.streaming);
        assert_eq!(config.display.skin, "default");
        assert_eq!(config.delegation.max_iterations, 45);
        assert!(config.mcp_servers.is_empty());
        assert!(config.credential_pools.is_empty());
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
  engine: simple
  context_length: 64000
  enabled: false
  threshold: 0.70
  target_ratio: 0.30
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
        assert_eq!(config.compression.engine, "simple");
        assert_eq!(config.compression.context_length, 64_000);
    }
}
