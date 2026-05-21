use serde::{Deserialize, Serialize};

/// The API mode / protocol used to communicate with the LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ApiMode {
    /// OpenAI Chat Completions (`/v1/chat/completions`).
    ChatCompletions,
    /// OpenAI Responses API (`/v1/responses`), used by Codex.
    CodexResponses,
    /// Anthropic Messages API (`/v1/messages`).
    AnthropicMessages,
    /// AWS Bedrock Converse API.
    BedrockConverse,
    /// Google Gemini GenerateContent API.
    GeminiGenerateContent,
}

/// Configuration for an LLM provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    /// The API mode / protocol to use.
    pub api_mode: ApiMode,

    /// Base URL of the provider API.
    pub base_url: String,

    /// API key (may be `None` if auth is handled differently, e.g. IAM for Bedrock).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,

    /// Default model identifier for this provider.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,

    /// Optional org ID (OpenAI-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub org_id: Option<String>,

    /// AWS region (Bedrock-specific).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub region: Option<String>,
}

/// Configuration specific to a model.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelConfig {
    /// Model identifier string (e.g. `gpt-4o`, `claude-sonnet-4-20250514`).
    pub model: String,

    /// Sampling temperature.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub temperature: Option<f64>,

    /// Nucleus sampling top-p.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub top_p: Option<f64>,

    /// Maximum tokens in the completion.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,

    /// System prompt prepended to every conversation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,

    /// Whether to request reasoning/thinking tokens.
    #[serde(default)]
    pub reasoning: bool,
}
